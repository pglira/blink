use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{NaiveDate, NaiveDateTime};
use serde::Deserialize;
use tracing::warn;

#[derive(Clone, Debug)]
pub struct Shot {
    pub time: NaiveDateTime,
    pub png: PathBuf,
    pub duration_s: u64,
}

#[derive(Clone, Debug, Default)]
pub struct DayIndex {
    pub shots: Vec<Shot>,
}

impl DayIndex {
    pub fn total_duration_s(&self) -> u64 {
        self.shots.iter().map(|s| s.duration_s).sum()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Index {
    pub days: BTreeMap<NaiveDate, DayIndex>,
}

impl Index {
    pub fn build(output_dir: &Path, fallback_interval_s: u64) -> Result<Self> {
        let mut days: BTreeMap<NaiveDate, DayIndex> = BTreeMap::new();
        let year_iter = match fs::read_dir(output_dir) {
            Ok(it) => it,
            Err(e) => {
                warn!("cannot read output dir {}: {e}", output_dir.display());
                return Ok(Index { days });
            }
        };
        for year_entry in year_iter.flatten() {
            if !year_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let Ok(month_iter) = fs::read_dir(year_entry.path()) else { continue };
            for month_entry in month_iter.flatten() {
                if !month_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let Ok(file_iter) = fs::read_dir(month_entry.path()) else { continue };
                for file_entry in file_iter.flatten() {
                    let path = file_entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("png") {
                        continue;
                    }
                    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
                    let Ok(time) = NaiveDateTime::parse_from_str(stem, "%Y_%m_%d_%H_%M_%S") else {
                        continue;
                    };
                    let duration_s = read_sidecar_interval(&path).unwrap_or(fallback_interval_s);
                    let date = time.date();
                    days.entry(date).or_default().shots.push(Shot {
                        time,
                        png: path,
                        duration_s,
                    });
                }
            }
        }
        for d in days.values_mut() {
            d.shots.sort_by_key(|s| s.time);
        }
        Ok(Index { days })
    }

    pub fn total_duration_s(&self) -> u64 {
        self.days.values().map(|d| d.total_duration_s()).sum()
    }

}

#[derive(Deserialize)]
struct Sidecar {
    interval_seconds: u64,
}

fn read_sidecar_interval(png_path: &Path) -> Option<u64> {
    let toml_path = png_path.with_extension("toml");
    let raw = fs::read_to_string(&toml_path).ok()?;
    let s: Sidecar = toml::from_str(&raw).ok()?;
    Some(s.interval_seconds)
}
