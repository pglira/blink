use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context as _, Result};
use gtk::prelude::*;
use image::{ImageBuffer, Rgba, RgbaImage};
use libappindicator::{AppIndicator, AppIndicatorStatus};
use tracing::info;

use crate::state::AppState;

const ICON_REC: &str = "blink-tray";
const ICON_PAUSED: &str = "blink-tray-paused";
const DESC_REC: &str = "Blink — recording";
const DESC_PAUSED: &str = "Blink — paused";

/// Blocks on the GTK main loop until the user picks Quit, or until
/// another thread sets `state.shutting_down`.
pub fn run(state: Arc<AppState>) -> Result<()> {
    gtk::init().map_err(|e| anyhow!("gtk init failed: {e}"))?;

    // AppIndicator loads icons by name from a theme search path. Write both
    // status PNGs into a private temp dir upfront so we can swap between them
    // with a cheap set_icon_full() call when pause toggles.
    let icon_dir = write_icons()?;

    let mut indicator = AppIndicator::new("blink", "");
    indicator.set_icon_theme_path(
        icon_dir
            .to_str()
            .ok_or_else(|| anyhow!("non-utf8 icon dir"))?,
    );
    let (name, desc) = current_icon(&state);
    indicator.set_icon_full(name, desc);
    indicator.set_title("Blink");
    indicator.set_status(AppIndicatorStatus::Active);

    // Shared with the pause-toggle closure so it can swap the icon in place.
    let indicator = Rc::new(RefCell::new(indicator));

    let mut menu = gtk::Menu::new();
    let pause_item = gtk::MenuItem::with_label("Pause / Resume");
    let sep = gtk::SeparatorMenuItem::new();
    let quit_item = gtk::MenuItem::with_label("Quit");
    menu.append(&pause_item);
    menu.append(&sep);
    menu.append(&quit_item);
    menu.show_all();

    {
        let state = Arc::clone(&state);
        let indicator = Rc::clone(&indicator);
        pause_item.connect_activate(move |_| {
            let now_paused = !state.paused.load(Ordering::SeqCst);
            state.paused.store(now_paused, Ordering::SeqCst);
            info!(paused = now_paused, "pause toggled");
            let (name, desc) = icon_for(now_paused);
            indicator.borrow_mut().set_icon_full(name, desc);
        });
    }
    {
        let state = Arc::clone(&state);
        quit_item.connect_activate(move |_| {
            info!("quit requested from tray");
            state.shutting_down.store(true, Ordering::SeqCst);
            gtk::main_quit();
        });
    }

    indicator.borrow_mut().set_menu(&mut menu);

    // Poll the shutdown flag so SIGINT/SIGTERM can unwind the GTK loop too.
    {
        let state = Arc::clone(&state);
        gtk::glib::timeout_add_local(Duration::from_millis(300), move || {
            if state.shutting_down.load(Ordering::SeqCst) {
                gtk::main_quit();
                gtk::glib::ControlFlow::Break
            } else {
                gtk::glib::ControlFlow::Continue
            }
        });
    }

    gtk::main();

    let _ = fs::remove_dir_all(&icon_dir);
    info!("tray loop exiting");
    Ok(())
}

fn current_icon(state: &AppState) -> (&'static str, &'static str) {
    icon_for(state.paused.load(Ordering::SeqCst))
}

fn icon_for(paused: bool) -> (&'static str, &'static str) {
    if paused {
        (ICON_PAUSED, DESC_PAUSED)
    } else {
        (ICON_REC, DESC_REC)
    }
}

/// Render both status icons (recording and paused) into a per-PID temp dir
/// and return the dir. Icon names are the consts above; AppIndicator resolves
/// them against the theme path.
fn write_icons() -> Result<PathBuf> {
    const SIZE: u32 = 32;
    let dir = std::env::temp_dir().join(format!("blink-{}", std::process::id()));
    fs::create_dir_all(&dir).context("creating tray icon dir")?;

    // Recording: white dot inside a blue disc.
    draw_icon(&dir.join(format!("{ICON_REC}.png")), SIZE, |x, y, cx, cy| {
        let d = dist(x, y, cx, cy);
        if d <= 5.0 {
            Some(Rgba([255, 255, 255, 255]))
        } else if d <= 14.0 {
            Some(Rgba([60, 130, 220, 255]))
        } else {
            None
        }
    })?;

    // Paused: two white vertical bars inside a muted gray disc.
    draw_icon(&dir.join(format!("{ICON_PAUSED}.png")), SIZE, |x, y, cx, cy| {
        let d = dist(x, y, cx, cy);
        if d > 14.0 {
            return None;
        }
        let dx = x as f32 + 0.5 - cx;
        let dy = y as f32 + 0.5 - cy;
        // Two vertical bars, ~3px wide, centred at ±3.5px from centre, height ±6.
        let in_bar = (dx.abs() > 2.0 && dx.abs() < 5.0) && dy.abs() < 6.0;
        Some(if in_bar {
            Rgba([255, 255, 255, 255])
        } else {
            Rgba([140, 140, 140, 255])
        })
    })?;

    Ok(dir)
}

fn draw_icon(
    path: &Path,
    size: u32,
    pixel: impl Fn(u32, u32, f32, f32) -> Option<Rgba<u8>>,
) -> Result<()> {
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let mut img: RgbaImage = ImageBuffer::new(size, size);
    for (x, y, px) in img.enumerate_pixels_mut() {
        *px = pixel(x, y, cx, cy).unwrap_or(Rgba([0, 0, 0, 0]));
    }
    img.save(path).context("writing tray icon png")?;
    Ok(())
}

#[inline]
fn dist(x: u32, y: u32, cx: f32, cy: f32) -> f32 {
    let dx = x as f32 + 0.5 - cx;
    let dy = y as f32 + 0.5 - cy;
    (dx * dx + dy * dy).sqrt()
}
