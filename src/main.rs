use std::sync::atomic::Ordering;

use anyhow::{bail, Context as _, Result};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

mod capture;
mod config;
mod state;
mod tray;

fn main() -> Result<()> {
    // Writes to a broken peer (e.g. the tray DBus socket going away) must
    // surface as EPIPE, not a process-killing signal. Default daemon hygiene.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN); }
    // Closing the launching terminal sends SIGHUP to its children; a daemon
    // should keep running across that.
    unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN); }

    init_tracing();

    let sub = std::env::args().nth(1).unwrap_or_else(|| "run".into());
    match sub.as_str() {
        "run" => run(),
        "config-path" => {
            println!("{}", config::config_path()?.display());
            Ok(())
        }
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        other => bail!("unknown subcommand: {other}. Try `blink help`."),
    }
}

fn print_help() {
    println!("blink — background screenshot daemon");
    println!();
    println!("USAGE:");
    println!("  blink [run]       Start the daemon (default)");
    println!("  blink config-path Print the path of config.toml");
    println!("  blink help        Show this help");
    println!();
    println!("On first start a default config is written to ~/.config/blink/config.toml.");
}

fn run() -> Result<()> {
    let cfg = config::Config::load().context("loading config")?;
    info!(
        output = %cfg.output_dir().display(),
        interval_s = cfg.capture.interval_seconds,
        "starting blink"
    );

    let _pid_guard = state::PidFile::acquire(&cfg.pid_file())
        .context("acquiring PID file")?;

    let state = state::AppState::new();

    // SIGINT / SIGTERM → flip the shutdown flag. Threads drain and exit.
    {
        let s = state.clone();
        ctrlc::set_handler(move || {
            info!("signal received, shutting down");
            s.shutting_down.store(true, Ordering::SeqCst);
        })
        .context("installing signal handler")?;
    }

    let cap_handle = {
        let cfg = cfg.clone();
        let state = state.clone();
        std::thread::Builder::new()
            .name("blink-capture".into())
            .spawn(move || {
                if let Err(e) = capture::run(cfg, state) {
                    error!("capture thread: {e:#}");
                }
            })?
    };

    // Tray runs on the main thread; blocks until Quit. If the tray can't
    // initialise (e.g. no DBus session bus), fall back to waiting on shutdown
    // so the daemon still works headlessly.
    if let Err(e) = tray::run(state.clone()) {
        error!("tray unavailable ({e:#}); running headless — send SIGINT to stop");
        while !state.shutting_down.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    // Tell the worker thread to wrap up, then join.
    state.shutting_down.store(true, Ordering::SeqCst);
    let _ = cap_handle.join();

    info!("blink stopped cleanly");
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
