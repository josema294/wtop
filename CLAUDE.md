# CLAUDE.md - wtop

## What is wtop

wtop is a real-time, web-based system monitor built in Rust. It streams live system metrics (CPU, memory, GPU, network, disk I/O, processes) to a browser UI via Server-Sent Events (SSE). Designed to be lightweight enough to run alongside heavy workloads (ML training, gaming) and accessible from any device on the local network (phone, tablet).

## Architecture

- **Single binary**: Backend (Rust/Axum/Tokio) + frontend (static HTML/JS/Tailwind) compiled together via `include_bytes!`
- **Backend** (`src/main.rs`): Collects metrics every 1.5s using `sysinfo` crate + direct Linux sysfs/procfs reads. Broadcasts via `tokio::broadcast` channel to SSE subscribers.
- **Frontend** (`static/index.html`, `static/locales.json`): Single-page app using vanilla JS + Tailwind CSS CDN. Tokyo Night theme. Features: collapsible/draggable panels, process pinning/sorting, i18n, light/dark themes, settings persistence via localStorage.
- **GPU support**: NVIDIA via `nvml-wrapper`, AMD/Intel via sysfs fallback.
- **No database, no build step for frontend** - just `cargo build`.

## Build & Run

```bash
cargo build              # debug build
cargo build --release    # release build
cargo run                # runs on http://0.0.0.0:3000
```

## Project structure

```
src/main.rs          # Entire backend: metrics collection, SSE, static file serving
static/index.html    # Full frontend SPA (HTML + CSS + JS in one file)
static/locales.json  # i18n translations
aur/                 # Arch Linux AUR package files (PKGBUILD)
assets/              # Screenshots/demo images for README
```

## Key conventions

- Edition 2024 Rust
- License: AGPL-3.0-or-later
- Static assets are embedded at compile time (`include_bytes!`), not served from disk
- Linux-only system metrics (reads `/proc/cpuinfo`, `/proc/diskstats`, `/sys/class/hwmon`, `/sys/class/drm`)
- Frontend uses Tailwind via CDN, no npm/node toolchain needed
- Commit messages follow conventional commits style (feat:, fix:, chore:, docs:, build:)

## Crate dependencies

- `axum` - async web framework
- `tokio` + `tokio-stream` - async runtime
- `sysinfo` - cross-platform system info
- `nvml-wrapper` - NVIDIA GPU monitoring
- `serde` + `serde_json` - serialization
- `futures` - stream utilities

## When editing

- Changing metrics structs in `main.rs` requires updating both the Rust serialization and the JS consumer in `index.html`
- The frontend is a single large HTML file with inline JS and CSS - search by function name or HTML id
- GPU detection has two code paths: NVML (NVIDIA proprietary) and sysfs (AMD/Intel/nouveau)
- CPU power is estimated, not measured directly (see `power_w` calculation)
