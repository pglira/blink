use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use crossbeam_channel::Sender;
use image::{codecs::jpeg::JpegEncoder, ExtendedColorType, ImageEncoder, RgbImage};
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::staging;
use crate::state::AppState;

pub enum Signal {
    /// A new JPEG has been written to staging. The encoder rescans the
    /// staging dir on wake-up, so the path itself is not carried here.
    FrameReady,
}

pub fn run(cfg: Config, state: Arc<AppState>, tx: Sender<Signal>) -> Result<()> {
    let interval = Duration::from_secs(cfg.capture.interval_seconds.max(1));
    let staging_dir = cfg.staging_dir();
    info!(
        interval_s = interval.as_secs(),
        staging = %staging_dir.display(),
        "capture thread started"
    );

    loop {
        let tick_start = Instant::now();

        if state.shutting_down.load(Ordering::SeqCst) {
            break;
        }

        if !state.paused.load(Ordering::SeqCst) {
            match grab_composite(&cfg) {
                Ok(Some(canvas)) => match encode_jpeg(&canvas, cfg.staging.jpeg_quality) {
                    Ok(bytes) => match staging::stage_jpeg(&staging_dir, &bytes) {
                        Ok(path) => {
                            debug!(path = %path.display(), "frame staged");
                            let _ = tx.send(Signal::FrameReady);
                        }
                        Err(e) => error!("staging frame failed: {e:#}"),
                    },
                    Err(e) => error!("JPEG encode failed: {e:#}"),
                },
                Ok(None) => warn!("no monitors available, skipping tick"),
                Err(e) => error!("screenshot failed: {e:#}"),
            }
        }

        // Sleep the remainder of the interval, but wake early on shutdown.
        let remaining = interval.saturating_sub(tick_start.elapsed());
        let mut slept = Duration::ZERO;
        while slept < remaining {
            if state.shutting_down.load(Ordering::SeqCst) {
                break;
            }
            let chunk = Duration::from_millis(200).min(remaining - slept);
            thread::sleep(chunk);
            slept += chunk;
        }
    }

    info!("capture thread exiting");
    Ok(())
}

/// Grab configured monitors and composite them horizontally into one RGB image.
/// Dimension changes between calls are what drive segment rotation downstream:
/// the encoder sees the JPEG size and rotates when it shifts.
fn grab_composite(cfg: &Config) -> Result<Option<RgbImage>> {
    let monitors = xcap::Monitor::all().map_err(|e| anyhow!("xcap::Monitor::all: {e}"))?;
    if monitors.is_empty() {
        return Ok(None);
    }

    let selected: Vec<&xcap::Monitor> = match cfg.capture.monitors.as_str() {
        "primary" => monitors
            .iter()
            .find(|m| m.is_primary())
            .into_iter()
            .collect(),
        _ => monitors.iter().collect(),
    };
    if selected.is_empty() {
        return Ok(None);
    }

    let mut rgbs: Vec<RgbImage> = Vec::with_capacity(selected.len());
    for m in &selected {
        let rgba = m
            .capture_image()
            .map_err(|e| anyhow!("capture_image: {e}"))?;
        rgbs.push(image::DynamicImage::ImageRgba8(rgba).to_rgb8());
    }

    let total_w: u32 = rgbs.iter().map(|i| i.width()).sum();
    let max_h: u32 = rgbs.iter().map(|i| i.height()).max().unwrap_or(0);
    if total_w == 0 || max_h == 0 {
        return Ok(None);
    }
    // H.264 (yuv420p) requires even dimensions.
    let canvas_w = total_w + (total_w & 1);
    let canvas_h = max_h + (max_h & 1);

    let mut canvas = RgbImage::new(canvas_w, canvas_h);
    let mut x_off: i64 = 0;
    for img in &rgbs {
        image::imageops::overlay(&mut canvas, img, x_off, 0);
        x_off += img.width() as i64;
    }
    Ok(Some(canvas))
}

fn encode_jpeg(img: &RgbImage, quality: u8) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(1 << 20);
    let encoder = JpegEncoder::new_with_quality(&mut buf, quality);
    encoder.write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        ExtendedColorType::Rgb8,
    )?;
    Ok(buf)
}
