use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{bail, Context as _, Result};

#[derive(Debug, Default)]
pub struct AppState {
    pub paused: AtomicBool,
    pub shutting_down: AtomicBool,
}

impl AppState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

/// Best-effort PID file. Writes our PID on acquire, removes on drop.
/// Refuses to start if a stored PID is still alive in /proc.
pub struct PidFile {
    path: PathBuf,
}

impl PidFile {
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        if path.exists() {
            if let Ok(contents) = fs::read_to_string(path) {
                if let Ok(pid) = contents.trim().parse::<i32>() {
                    if Path::new(&format!("/proc/{pid}")).exists() {
                        bail!(
                            "another blink daemon appears to be running (pid {pid}); \
                             delete {} if stale",
                            path.display()
                        );
                    }
                }
            }
        }
        let mut f = fs::File::create(path)
            .with_context(|| format!("writing {}", path.display()))?;
        writeln!(f, "{}", std::process::id())?;
        f.sync_all()?;
        Ok(Self { path: path.to_path_buf() })
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
