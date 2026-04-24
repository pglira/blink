# blink

A small Linux daemon that silently takes a screenshot every N seconds and
appends it to a compressed H.264/Matroska video. Designed to keep running in
the background, survive crashes without corrupting the archive, and stay out
of the way — visible only as a tray icon.

## Features

- Periodic multi-monitor capture via `xcap` (X11 primary, Wayland best-effort).
  Monitors are composited side-by-side into one canvas per frame.
- Continuous H.264 encoding into MKV segments via linked `libav` (no
  `ffmpeg` subprocess). MKV is chosen because truncated files remain playable.
- Automatic segment rotation when the monitor layout changes (resolution or
  connect/disconnect of a display) or when `segment_minutes` elapses.
- Crash resilience: staged JPEGs are only deleted once the corresponding
  frame is flushed and fsynced; MKV muxer runs with `flush_packets=1`.
  Any orphan JPEGs from a prior run are drained into `recovery_*.mkv` at
  startup.
- System tray icon with Pause / Resume / Quit. Icon shows current status
  (blue disc with dot = recording, gray disc with pause bars = paused).
- Single TOML config at `~/.config/blink/config.toml` (written on first run).

## Build

The project targets Linux (X11 + GTK). It currently assumes the versions of
libav that ship with Ubuntu 24.04 (`libav*.so.60`).

### Using the devcontainer

The [`.devcontainer`](.devcontainer/) here installs the full toolchain:

```
Reopen in Container  →  cargo build --release
```

### Building manually

Install native dependencies (apt package names for Ubuntu 24.04):

```
pkg-config
libavformat-dev libavcodec-dev libavutil-dev libswscale-dev
libavfilter-dev libavdevice-dev
libgtk-3-dev libayatana-appindicator3-dev
libxcb1-dev libxrandr-dev libxfixes-dev libxext-dev libxdo-dev
libssl-dev
```

Then install a stable Rust toolchain via `rustup` and:

```
cargo build --release
```

The binary is `target/release/blink` (~4 MB stripped).

## Run

```
blink                    # start the daemon (same as `blink run`)
blink config-path        # print path to config.toml
blink install-service    # install & start a systemd user service
blink uninstall-service  # stop & remove the systemd user service
blink help
```

### Running under systemd (recommended)

`blink &` from a terminal emulator dies when the terminal window closes,
because modern terminals (VTE-based ones like gnome-terminal and
xfce4-terminal, plus Konsole) register each shell in a transient systemd
user scope and tear it down — SIGTERM'ing everything in the cgroup — when
the window goes away. Ignoring SIGHUP does not help.

The fix is to run blink as a systemd **user** service, which lives in its
own unit and is independent of any terminal:

```
blink install-service
```

That writes `~/.config/systemd/user/blink.service` pointing at the current
binary, then runs `systemctl --user daemon-reload` and
`systemctl --user enable --now blink.service`. The daemon will start
automatically on login from then on. `blink uninstall-service` reverses it.

First run writes a default config to `~/.config/blink/config.toml`.
Default paths:

- Frames staging: `~/.cache/blink/staging/` (each frame as a timestamped JPEG
  until it has been successfully encoded; normally empty)
- Video output:   `~/Videos/blink/` (`blink_YYYYMMDD-HHMMSS_WxH.mkv`)
- PID lock:       `~/.cache/blink/blink.pid`

The tray menu offers:

- **Pause / Resume** — toggles capture. Icon changes to reflect status.
- **Quit** — clean shutdown: flush encoder, write MKV trailer, remove PID
  file, exit.

The daemon also handles `SIGINT` and `SIGTERM` as graceful shutdown, so
system shutdown or a `kill <pid>` from a terminal leaves properly finalized
files.

## Config reference

`~/.config/blink/config.toml`:

```toml
[capture]
interval_seconds = 60      # one frame per minute
monitors = "all"           # "all" | "primary"

[video]
codec = "h264"             # h264 | h265 | av1
crf = 28                   # lower = higher quality, larger files
segment_minutes = 60       # roll over to a new MKV every hour

[staging]
jpeg_quality = 85
keep_after_encode = false  # keep the JPEGs on disk after encoding
                           # (enable to feed a future OCR pass)

[output]
dir = ""                   # empty → ~/Videos/blink

[daemon]
pid_file = ""              # empty → ~/.cache/blink/blink.pid
log_file = ""              # (reserved; current build logs to stderr)
```

## What the daemon does on shutdown

| Scenario                                      | Resulting file                          | Frame loss |
|-----------------------------------------------|-----------------------------------------|------------|
| `Quit` from tray, `SIGINT`, `SIGTERM`, logout | Properly finalized MKV with trailer     | None       |
| `SIGKILL`, kernel panic, power cut            | Valid MKV without trailer (plays fine up to last flushed cluster) | ≤ 1 in-flight frame |
| Daemon exits then restarts with JPEGs left in staging | Drained into `recovery_*.mkv` on next startup | None       |

## Project layout

```
.
├── .devcontainer/          VS Code devcontainer (Rust + libav + GTK + X11)
├── config/default.toml     Embedded default config, written on first run
├── config/blink.service    Embedded systemd user unit template
├── Cargo.toml
└── src/
    ├── main.rs             CLI, thread orchestration, signal handling
    ├── config.rs           TOML loader, XDG path resolution
    ├── service.rs          systemd --user install/uninstall
    ├── state.rs            Shared AtomicBool flags + PID file guard
    ├── staging.rs          Atomic JPEG writes + pending_frames scan
    ├── capture.rs          xcap screenshot loop, monitor composite
    ├── encoder.rs          libav MKV muxer, segment rotation, recovery
    └── tray.rs             AppIndicator tray icon + GTK menu
```

## Planned / hook points

- Text journal: OCR each frame and write timestamped text entries. The code
  already names staged JPEGs `<unix_millis>.jpg` and retains them when
  `keep_after_encode = true`, so a future `blink ocr` subcommand can iterate
  them in timestamp order without touching the capture/encode pipeline.

## Licence

MIT
