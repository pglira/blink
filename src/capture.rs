use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash as _, Hasher as _};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
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

    // In-memory state for "skip when unchanged": when the canvas pixel hash
    // matches the previous tick's, we don't write a new image — we extend
    // the previous shot's `interval_seconds` instead, so the timeline still
    // accounts for the elapsed time. State is per-process; on restart we
    // simply save the first tick fresh.
    let mut prev_hash: Option<u64> = None;
    let mut prev_sidecar: Option<PathBuf> = None;

    loop {
        let tick_start = Instant::now();

        if state.shutting_down.load(Ordering::SeqCst) {
            break;
        }

        if !state.paused.load(Ordering::SeqCst) {
            let captured_at = Local::now();
            match grab_composite(&cfg) {
                Ok(Some(canvas)) => {
                    let h = hash_canvas(&canvas);
                    if Some(h) == prev_hash {
                        if let Some(sidecar) = prev_sidecar.as_deref() {
                            match bump_sidecar_interval(sidecar, interval.as_secs()) {
                                Ok(new_total) => debug!(
                                    sidecar = %sidecar.display(),
                                    new_total_s = new_total,
                                    "screen unchanged, extended previous sidecar"
                                ),
                                Err(e) => warn!("extending sidecar failed: {e:#}"),
                            }
                        }
                    } else {
                        match encode_png(&canvas) {
                            Ok(bytes) => match write_screenshot(&output_dir, &captured_at, &bytes) {
                                Ok(path) => {
                                    debug!(path = %path.display(), bytes = bytes.len(), "screenshot saved");
                                    let sidecar = path.with_extension("toml");
                                    if let Err(e) =
                                        write_sidecar(&sidecar, &captured_at, interval.as_secs())
                                    {
                                        warn!("sidecar metadata write failed: {e:#}");
                                    }
                                    prev_hash = Some(h);
                                    prev_sidecar = Some(sidecar);
                                }
                                Err(e) => error!("writing screenshot failed: {e:#}"),
                            },
                            Err(e) => error!("PNG encode failed: {e:#}"),
                        }
                    }
                }
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
    // (Empirically beats WebP-lossless by ~10% on flat desktop content.)
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

/// SipHash of the raw RGB bytes. Cheap (~30 ms for a 14 MB canvas) and good
/// enough to drive the "skip when unchanged" guard — collisions would only
/// cost us a missed save, not corruption.
fn hash_canvas(img: &RgbImage) -> u64 {
    let mut h = DefaultHasher::new();
    img.as_raw().hash(&mut h);
    h.finish()
}

/// Update the previous shot's sidecar to extend its `interval_seconds`
/// by `add_seconds`. Used when the screen hasn't changed, so the unchanged
/// span is accounted for in the day's total without writing a duplicate image.
/// Returns the new total.
fn bump_sidecar_interval(toml_path: &Path, add_seconds: u64) -> Result<u64> {
    let raw = fs::read_to_string(toml_path)?;
    let mut new_lines: Vec<String> = Vec::with_capacity(raw.lines().count());
    let mut new_total: Option<u64> = None;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("interval_seconds") {
            let cur: u64 = rest
                .trim_start()
                .strip_prefix('=')
                .ok_or_else(|| anyhow!("malformed interval_seconds line: {line:?}"))?
                .trim()
                .parse()?;
            let total = cur + add_seconds;
            new_total = Some(total);
            new_lines.push(format!("interval_seconds = {total}"));
        } else {
            new_lines.push(line.to_string());
        }
    }
    let Some(total) = new_total else {
        bail!("interval_seconds line not found in {}", toml_path.display());
    };
    let body = format!("{}\n", new_lines.join("\n"));
    let tmp = toml_path.with_extension("toml.tmp");
    fs::write(&tmp, body)?;
    fs::rename(&tmp, toml_path)?;
    Ok(total)
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

/// Write the per-screenshot metadata sidecar `<stem>.toml`. Atomic via
/// `.tmp` + rename. Captures the wall-clock time (RFC 3339 with timezone
/// offset) and the capture interval in effect.
fn write_sidecar(
    toml_path: &Path,
    captured_at: &DateTime<Local>,
    interval_seconds: u64,
) -> Result<()> {
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
