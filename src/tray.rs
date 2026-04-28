use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, ToolTip, Tray, TrayService};
use tracing::info;

use crate::state::AppState;

const SIZE: i32 = 32;
const DESC_ACTIVE: &str = "Blink — capturing";
const DESC_PAUSED: &str = "Blink — paused";

struct BlinkTray {
    state: Arc<AppState>,
    icon_active: Vec<u8>,
    icon_paused: Vec<u8>,
}

impl Tray for BlinkTray {
    fn id(&self) -> String {
        "blink".into()
    }

    fn title(&self) -> String {
        "Blink".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        let data = if self.state.paused.load(Ordering::SeqCst) {
            self.icon_paused.clone()
        } else {
            self.icon_active.clone()
        };
        vec![Icon {
            width: SIZE,
            height: SIZE,
            data,
        }]
    }

    fn tool_tip(&self) -> ToolTip {
        let title = if self.state.paused.load(Ordering::SeqCst) {
            DESC_PAUSED
        } else {
            DESC_ACTIVE
        };
        ToolTip {
            title: title.into(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Pause / Resume".into(),
                activate: Box::new(|t: &mut Self| {
                    let now_paused = !t.state.paused.load(Ordering::SeqCst);
                    t.state.paused.store(now_paused, Ordering::SeqCst);
                    info!(paused = now_paused, "pause toggled");
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Open Viewer".into(),
                activate: Box::new(|_t: &mut Self| {
                    if let Err(e) = spawn_viewer() {
                        tracing::error!("could not launch viewer: {e:#}");
                    }
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|t: &mut Self| {
                    info!("quit requested from tray");
                    t.state.shutting_down.store(true, Ordering::SeqCst);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Runs the tray. ksni manages its own DBus thread and handles the
/// StatusNotifierWatcher protocol — including registering whenever the
/// watcher (re)appears on the bus, so the autostart-vs-panel race that
/// libappindicator stumbled on is handled by the library itself.
pub fn run(state: Arc<AppState>) -> Result<()> {
    let tray = BlinkTray {
        state: state.clone(),
        icon_active: render_active_icon(),
        icon_paused: render_paused_icon(),
    };
    let service = TrayService::new(tray);
    let handle = service.handle();
    service.spawn();

    // Block on shutdown. Push a refresh into the tray any time `paused`
    // changes externally (in our case, only via the menu — but cheap).
    let mut last_paused = state.paused.load(Ordering::SeqCst);
    while !state.shutting_down.load(Ordering::SeqCst) {
        let cur = state.paused.load(Ordering::SeqCst);
        if cur != last_paused {
            last_paused = cur;
            handle.update(|_| {});
        }
        std::thread::sleep(Duration::from_millis(300));
    }

    handle.shutdown();
    info!("tray loop exiting");
    Ok(())
}

/// Launch the viewer as a separate process so the daemon keeps capturing.
/// Re-execs the current binary with the `view` subcommand.
fn spawn_viewer() -> Result<()> {
    let exe = std::env::current_exe().context("current_exe")?;
    std::process::Command::new(exe)
        .arg("view")
        .spawn()
        .context("spawning viewer")?;
    info!("viewer launched from tray");
    Ok(())
}

/// Active icon: open eye — filled blue almond with a white pupil.
/// The almond is the intersection of two circles of radius R centred h pixels
/// above/below the canvas centre. With h=8, R=13 on 32×32 the almond spans
/// ~20×10 px — classic eye proportions.
fn render_active_icon() -> Vec<u8> {
    const H: f32 = 8.0;
    const R_SQ: f32 = 13.0 * 13.0;
    render_icon(|dx, dy| {
        if dx * dx + dy * dy <= 9.0 {
            return Some([255, 255, 255, 255]);
        }
        let d1 = dx * dx + (dy - H) * (dy - H);
        let d2 = dx * dx + (dy + H) * (dy + H);
        if d1 <= R_SQ && d2 <= R_SQ {
            Some([255, 60, 130, 220])
        } else {
            None
        }
    })
}

/// Paused icon: closed eye — a flat horizontal pill.
fn render_paused_icon() -> Vec<u8> {
    const HALF_W: f32 = 8.0;
    const HALF_H: f32 = 2.0;
    render_icon(|dx, dy| {
        let in_pill = if dx.abs() <= HALF_W {
            dy.abs() <= HALF_H
        } else {
            let ex = dx.abs() - HALF_W;
            ex * ex + dy * dy <= HALF_H * HALF_H
        };
        if in_pill {
            Some([255, 140, 140, 140])
        } else {
            None
        }
    })
}

/// ksni wants ARGB32 in network byte order — i.e. the byte sequence per pixel
/// is [A, R, G, B]. Returns a `SIZE * SIZE * 4` byte buffer.
fn render_icon(pixel: impl Fn(f32, f32) -> Option<[u8; 4]>) -> Vec<u8> {
    let cx = SIZE as f32 / 2.0;
    let cy = SIZE as f32 / 2.0;
    let mut data = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let argb = pixel(dx, dy).unwrap_or([0, 0, 0, 0]);
            let i = ((y * SIZE + x) * 4) as usize;
            data[i..i + 4].copy_from_slice(&argb);
        }
    }
    data
}
