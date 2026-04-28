use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use chrono::{DateTime, Local, Utc};
use image::{
    codecs::png::{CompressionType, FilterType, PngEncoder},
    ExtendedColorType, ImageEncoder, RgbImage,
};
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::state::AppState;

pub fn run(cfg: Config, state: Arc<AppState>) -> Result<()> {
    let interval = Duration::from_secs(cfg.capture.interval_seconds.max(1));
    let output_dir = cfg.output_dir();
    fs::create_dir_all(&output_dir)?;
    info!(
        interval_s = interval.as_secs(),
        output = %output_dir.display(),
        "capture thread started"
    );

    loop {
        let tick_start = Instant::now();

        if state.shutting_down.load(Ordering::SeqCst) {
            break;
        }

        if !state.paused.load(Ordering::SeqCst) {
            // Capture timestamp shared by the PNG filename and the sidecar so
            // they can never disagree.
            let captured_at = Local::now();
            match grab_composite(&cfg) {
                Ok(Some(canvas)) => match encode_png(&canvas) {
                    Ok(bytes) => match write_screenshot(&output_dir, &captured_at, &bytes) {
                        Ok(path) => {
                            debug!(path = %path.display(), bytes = bytes.len(), "screenshot saved");
                            if let Err(e) = write_sidecar(&path, &captured_at, interval.as_secs())
                            {
                                warn!("sidecar metadata write failed: {e:#}");
                            }
                        }
                        Err(e) => error!("writing screenshot failed: {e:#}"),
                    },
                    Err(e) => error!("PNG encode failed: {e:#}"),
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

    let mut canvas = RgbImage::new(total_w, max_h);
    let mut x_off: i64 = 0;
    for img in &rgbs {
        image::imageops::overlay(&mut canvas, img, x_off, 0);
        x_off += img.width() as i64;
    }
    Ok(Some(canvas))
}

fn encode_png(img: &RgbImage) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(1 << 20);
    // Best compression — screenshots are flat-colour heavy and we run at a
    // 60 s cadence, so the extra CPU per frame is well under the budget.
    let encoder =
        PngEncoder::new_with_quality(&mut buf, CompressionType::Best, FilterType::Adaptive);
    encoder.write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        ExtendedColorType::Rgb8,
    )?;
    Ok(buf)
}

/// Write `<output_dir>/YYYY/MM/YYYY_MM_DDTHHMMSSZ.png` atomically.
///
/// The directory uses the *local* year/month at capture, so the on-disk
/// layout matches the calendar day a user would associate with the shot.
/// The filename uses *UTC* in compact ISO 8601 form (with the trailing
/// `Z`), which makes it globally unique even if the capturing machine's
/// timezone changes mid-day — without that, two shots taken at the same
/// local wall-clock time but in different timezones would collide and
/// the second write would clobber the first.
fn write_screenshot(
    output_dir: &Path,
    captured_at: &DateTime<Local>,
    bytes: &[u8],
) -> Result<PathBuf> {
    let dir = output_dir
        .join(captured_at.format("%Y").to_string())
        .join(captured_at.format("%m").to_string());
    fs::create_dir_all(&dir)?;

    let stem = captured_at
        .with_timezone(&Utc)
        .format("%Y_%m_%dT%H%M%SZ")
        .to_string();
    let final_path = dir.join(format!("{stem}.png"));
    let tmp_path = dir.join(format!("{stem}.png.tmp"));

    {
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp_path, &final_path)?;
    Ok(final_path)
}

/// Write the per-screenshot metadata sidecar `<stem>.toml` next to the PNG.
/// Atomic via `.tmp` + rename, like the PNG. Captures the wall-clock time
/// (RFC 3339 with timezone offset) and the capture interval in effect, so
/// downstream tools don't have to re-parse the filename.
fn write_sidecar(
    png_path: &Path,
    captured_at: &DateTime<Local>,
    interval_seconds: u64,
) -> Result<()> {
    let toml_path = png_path.with_extension("toml");
    let tmp_path = toml_path.with_extension("toml.tmp");
    let body = format!(
        "captured_at = \"{}\"\ninterval_seconds = {interval_seconds}\n",
        captured_at.to_rfc3339()
    );
    {
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(body.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp_path, &toml_path)?;
    Ok(())
}
