use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context as _, Result};

const SERVICE_TEMPLATE: &str = include_str!("../config/blink.service");
const SERVICE_NAME: &str = "blink.service";

fn unit_path() -> Result<PathBuf> {
    let base = directories::BaseDirs::new()
        .context("cannot resolve XDG base dirs")?;
    Ok(base.config_dir().join("systemd/user").join(SERVICE_NAME))
}

pub fn install() -> Result<()> {
    let exe = std::env::current_exe()
        .context("cannot locate current executable")?
        .canonicalize()
        .context("cannot canonicalize current executable path")?;

    let rendered = SERVICE_TEMPLATE.replace("__EXEC_START__", &exe.display().to_string());

    let path = unit_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(&path, rendered)
        .with_context(|| format!("writing {}", path.display()))?;
    println!("wrote {}", path.display());

    run_systemctl(&["daemon-reload"])?;
    run_systemctl(&["enable", "--now", SERVICE_NAME])?;

    println!();
    println!("blink is now running as a user service and will start on login.");
    println!("  status:    systemctl --user status blink");
    println!("  logs:      journalctl --user -u blink -f");
    println!("  stop:      systemctl --user stop blink");
    println!("  uninstall: blink uninstall-service");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let path = unit_path()?;
    // Best-effort: stop & disable even if never started or already gone.
    let _ = run_systemctl(&["disable", "--now", SERVICE_NAME]);
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("removing {}", path.display()))?;
        println!("removed {}", path.display());
    } else {
        println!("{} did not exist", path.display());
    }
    let _ = run_systemctl(&["daemon-reload"]);
    Ok(())
}

fn run_systemctl(args: &[&str]) -> Result<()> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .context("invoking systemctl (is systemd installed?)")?;
    if !status.success() {
        bail!("`systemctl --user {}` failed", args.join(" "));
    }
    Ok(())
}
