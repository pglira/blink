use anyhow::{Context as _, Result};
use tracing::info;

use crate::config::Config;

pub mod app;
pub mod index;
pub mod widgets;

pub fn run(cfg: Config) -> Result<()> {
    let output_dir = cfg.output_dir();
    let fallback = cfg.capture.interval_seconds;
    info!(output = %output_dir.display(), "viewer indexing screenshots");
    let index = index::Index::build(&output_dir, fallback)
        .context("indexing screenshots")?;
    info!(
        days = index.days.len(),
        total_s = index.total_duration_s(),
        "viewer index built"
    );

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Blink — viewer")
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Blink — viewer",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(app::ViewerApp::new(index)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))?;
    Ok(())
}

/// Render a duration in seconds as `H h MM m` (or `M m` when under one hour).
pub fn fmt_duration(s: u64) -> String {
    let h = s / 3600;
    let m = (s % 3600) / 60;
    if h > 0 {
        format!("{h} h {m:02} m")
    } else {
        format!("{m} m")
    }
}
