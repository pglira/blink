use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

/// Staged JPEG filenames: `<unix_millis>.jpg`. 13-digit zero-padded so they
/// sort lexically in capture order. The contract is stable so a future
/// `blink ocr` can iterate staged frames by timestamp.
pub fn stage_jpeg(dir: &Path, bytes: &[u8]) -> Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let final_path = dir.join(format!("{:013}.jpg", ms));
    let tmp_path   = dir.join(format!("{:013}.jpg.tmp", ms));

    {
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    // Atomic rename on the same filesystem.
    fs::rename(&tmp_path, &final_path)?;
    Ok(final_path)
}

/// Every staged `*.jpg` in `dir`, sorted ascending by filename (= by time).
pub fn pending_frames(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut v: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jpg"))
        .collect();
    v.sort();
    Ok(v)
}
