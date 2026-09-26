<p align="center"><a href="README.md">简体中文</a> | <strong>English</strong></p>

# NetOwl

![License](https://img.shields.io/badge/license-GPL--3.0-blue) ![Platform](https://img.shields.io/badge/platform-Windows%2010%2B-0078D6) ![Rust](https://img.shields.io/badge/Rust-stable-DEA584?logo=rust)

NetOwl is a network connection monitor for Windows, inspired by Little Snitch on macOS: it shows you where every application is sending your data. The core experience is the **traffic map** — every network connection rendered in real time on a world map, centered on your machine.

> The project is in its early scaffold stage: the UI and simulated data already run, while real data collection and interception are delivered step by step following the roadmap.

![NetOwl screenshot](docs/screenshot.png)

## Features

Implemented:

- **Traffic map** — live process connections on a world map: arc links, animated inbound/outbound particles, pulsing nodes aggregated by connection count, hover for per-city details
- **Connection list** — active connections sorted by traffic: process, protocol, remote address, geolocation, upload/download volume
- **System tray** — stays in the background, closing the window minimizes to tray, left click to restore
- **Multi-language** — fluent-based UI localization with a language switcher in Settings, supporting hot-loading of external translation files

Planned:

- Real connection collection (ETW / TCP tables, no driver required)
- Allow/deny interception (WFP) with prompt dialogs
- Rule engine and persistence
- macOS / Linux support

## Build & Run

Requires a Rust stable toolchain:

```bash
cargo run --release
```

Currently Windows 10+ only (the UI renders Chinese text using the system bundled Microsoft YaHei font).

## Map Data Sources

The basemap is assembled from two offline vector datasets, quantized/simplified by a script into `assets/mapdata.bin` and embedded at compile time. The app **never calls a map API at runtime**:

- **China administrative boundaries** (province borders, Hong Kong/Macao/Taiwan, Zangnan/South Tibet, South China Sea islands, and the dotted line): [Alibaba Cloud DataV GeoAtlas](https://datav.aliyun.com/area/svgconfig/) static GeoJSON (`geo.datav.aliyun.com/areas_v3/bound/100000_full.json`, no API key required). Natural Earth's China-related features are excluded; Hong Kong, Macao and Taiwan are displayed as provincial-level divisions (Taiwan Province, Hong Kong SAR, Macao SAR), and the South China Sea boundary is drawn as the **ten-segment line**.
- **World borders, oceans and inland lakes**: [Natural Earth](https://www.naturalearthdata.com/) (Public Domain), 110m/50m tiers simplified with shared-edge-aware Douglas-Peucker; large lakes (Aral Sea, Great Lakes, Baikal, ...) come from the NE lakes and historic-lakes layers (the Aral Sea uses its historic outline) and are drawn as holes in the world layer.

Layering: the basemap renders the world layer first and covers it with the China layer, so NE borderlines and misdrawn geometry that extend into China (e.g. Zangnan) are hidden beneath it. The remaining seams at shared borders are closed by snapping neighbor vertices onto the China boundary within 0.2 degree; neighbor geometry farther away is left untouched.

Coordinate systems: DataV data is GCJ-02 (an inherent <0.02 degree offset against NE's WGS-84 at borders, below visual notice).

Licensing note: DataV GeoAtlas data is officially positioned for use within Alibaba Cloud products; confirm the terms before third-party distribution or commercial use. The data is downloaded offline during the build and never fetched at runtime. See the header comment of [tools/build_mapdata.py](tools/build_mapdata.py) for regeneration steps.

## Tech Stack

- **GUI**: egui/eframe (immediate mode, pure-Rust GPU-drawn rendering, no webview)
- **Tray**: tray-icon + muda
- **Localization**: fluent-bundle
- Icons and other resources are embedded at compile time (`include_bytes!`); the release artifact is a single file with no resource directory required

See [AGENTS.md](AGENTS.md) for development conventions and architecture notes.

## License

[GPL-3.0](LICENSE)
