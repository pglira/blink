# blink

A small Linux daemon that silently takes a screenshot every N seconds and
saves it as a compressed PNG into a `YYYY/MM/` archive. Designed to keep
running in the background and stay out of the way — visible only as a
tray icon.

## Features

- Periodic multi-monitor capture via `xcap` (X11 primary, Wayland best-effort).
  Monitors are composited side-by-side into one PNG per tick.
- Screenshots saved as PNG at maximum zlib compression with adaptive
  per-scanline filtering. Lossless and well suited to flat-colour-heavy
  desktop captures.
- Output organised as `<output_dir>/YYYY/MM/YYYY_MM_DD_HH_MM_SS.png`,
  written atomically via a temporary `.tmp` file + rename so a crash mid-write
  never leaves a half-written PNG behind.
- System tray icon with Pause / Resume / Quit. Icon shows current status
  (blue open eye = capturing, gray closed eye = paused).
- Single TOML config at `~/.config/blink/config.toml` (written on first run).

## Build

The project targets Linux (X11). Native dependencies (apt package names
for Ubuntu 24.04):

```
pkg-config
libxcb1-dev libxrandr-dev libxfixes-dev libxext-dev libxdo-dev
libssl-dev
```

Then install a stable Rust toolchain via `rustup` and:

```
cargo build --release
```

The binary is `target/release/blink`.

### Using the devcontainer

The [`.devcontainer`](.devcontainer/) here installs the toolchain:

```
Reopen in Container  →  cargo build --release
```

## Run

```
blink              # start the daemon (same as `blink run`)
blink config-path  # print path to config.toml
blink help
```

Start blink once and leave it sitting in the tray; pause/resume from there.

To launch it automatically on login, drop the bundled desktop entry into the
XDG autostart directory:

```
install -Dm644 config/blink.desktop ~/.config/autostart/blink.desktop
```

The session manager (GNOME, KDE, XFCE, …) will then start blink as part of
your graphical session — independent of any terminal — and it will reappear
in the tray after every login.

First run writes a default config to `~/.config/blink/config.toml`.
Default paths:

- Screenshots: `~/Pictures/blink/YYYY/MM/YYYY_MM_DD_HH_MM_SS.png`
- PID lock:    `~/.cache/blink/blink.pid`

The tray menu offers:

- **Pause / Resume** — toggles capture. Icon changes to reflect status.
- **Quit** — clean shutdown: stop the capture loop, remove PID file, exit.

The daemon also handles `SIGINT` and `SIGTERM` as graceful shutdown.

## Config reference

`~/.config/blink/config.toml`:

```toml
[capture]
interval_seconds = 60      # one frame per minute
monitors = "all"           # "all" | "primary"

[output]
dir = ""                   # empty → ~/Pictures/blink

[daemon]
pid_file = ""              # empty → ~/.cache/blink/blink.pid
log_file = ""              # (reserved; current build logs to stderr)
```

## Project layout

```
.
├── .devcontainer/          VS Code devcontainer
├── config/default.toml     Embedded default config, written on first run
├── config/blink.desktop    XDG autostart entry (copy to ~/.config/autostart/)
├── Cargo.toml
└── src/
    ├── main.rs             CLI, thread orchestration, signal handling
    ├── config.rs           TOML loader, XDG path resolution
    ├── state.rs            Shared AtomicBool flags + PID file guard
    ├── capture.rs          xcap screenshot loop, PNG encode + atomic write
    └── tray.rs             ksni StatusNotifierItem tray icon + menu
```

## Licence

MIT
