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
        icon_active: crate::icon::active(SIZE as u32, crate::icon::ByteOrder::Argb),
        icon_paused: crate::icon::paused(SIZE as u32, crate::icon::ByteOrder::Argb),
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
/// Re-execs the current binary with the `view` subcommand. The child
/// inherits the parent's env, including any DISPLAY/XAUTHORITY fallbacks
/// that `main` already filled in for autostart/systemd-user setups.
fn spawn_viewer() -> Result<()> {
    let exe = std::env::current_exe().context("current_exe")?;
    std::process::Command::new(exe)
        .arg("view")
        .spawn()
        .context("spawning viewer")?;
    info!("viewer launched from tray");
    Ok(())
}

