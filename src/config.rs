use std::fs;
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

/// Embedded at compile time so a first run can materialise a config.
pub const DEFAULT_CONFIG_TOML: &str = include_str!("../config/default.toml");

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub capture: CaptureConfig,
    pub output: OutputConfig,
    pub daemon: DaemonConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CaptureConfig {
    pub interval_seconds: u64,
    /// "all" or "primary".
    pub monitors: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OutputConfig {
    #[serde(default)]
    pub dir: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DaemonConfig {
    #[serde(default)]
    pub pid_file: String,
    #[serde(default)]
    pub log_file: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let cfg_path = config_path()?;
        if !cfg_path.exists() {
            if let Some(parent) = cfg_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&cfg_path, DEFAULT_CONFIG_TOML)?;
            tracing::info!(path = %cfg_path.display(), "wrote default config");
        }
        let raw = fs::read_to_string(&cfg_path)
            .with_context(|| format!("reading {}", cfg_path.display()))?;
        let mut cfg: Config = toml::from_str(&raw)
            .with_context(|| format!("parsing {}", cfg_path.display()))?;
        cfg.finalize()?;
        Ok(cfg)
    }

    fn finalize(&mut self) -> Result<()> {
        if self.output.dir.is_empty() {
            self.output.dir = default_output_dir()?.to_string_lossy().into_owned();
        }
        if self.daemon.pid_file.is_empty() {
            self.daemon.pid_file = default_cache_dir()?
                .join("blink.pid").to_string_lossy().into_owned();
        }
        if self.daemon.log_file.is_empty() {
            self.daemon.log_file = default_cache_dir()?
                .join("blink.log").to_string_lossy().into_owned();
        }
        Ok(())
    }

    pub fn output_dir(&self) -> PathBuf { expand(&self.output.dir) }
    pub fn pid_file(&self)   -> PathBuf { expand(&self.daemon.pid_file) }
}

fn expand(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(base) = directories::BaseDirs::new() {
            return base.home_dir().join(rest);
        }
    }
    PathBuf::from(s)
}

pub fn config_path() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "blink")
        .context("cannot resolve XDG project dirs")?;
    Ok(dirs.config_dir().join("config.toml"))
}

fn default_cache_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "blink")
        .context("cannot resolve XDG project dirs")?;
    Ok(dirs.cache_dir().to_path_buf())
}

fn default_output_dir() -> Result<PathBuf> {
    if let Some(ud) = directories::UserDirs::new() {
        if let Some(pictures) = ud.picture_dir() {
            return Ok(pictures.join("blink"));
        }
    }
    Ok(default_cache_dir()?.join("output"))
}
