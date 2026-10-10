<p align="center"><a href="README.md">简体中文</a> | <strong>English</strong></p>

# NetOwl

![License](https://img.shields.io/badge/license-GPL--3.0-blue) ![Platform](https://img.shields.io/badge/platform-Windows%2010%2B-0078D6) ![Rust](https://img.shields.io/badge/Rust-stable-DEA584?logo=rust)

NetOwl is a network connection monitor for Windows, inspired by Little Snitch on macOS: it shows you where every application is sending your data. The core experience is the **traffic map** — every network connection rendered in real time on a world map, centered on your machine.

![NetOwl screenshot](docs/screenshot.png)

## Download

Grab a build from [GitHub Releases](https://github.com/zsyo/NetOwl/releases) (x64 and arm64):

- **Installer** `netowl_<version>_windows_x64-setup.exe`: per-user install (no admin required) with start-menu/desktop shortcuts and a "launch at startup" option; the uninstaller can optionally wipe settings and user data
- **Portable** `netowl_<version>_windows_x64-portable.zip`: unzip and run, data lives next to the executable

The app ships with built-in update checks (Settings → About), on both stable and preview channels.

## Features

### Real-time monitoring

- **Traffic map** — live process connections on a world map: arc links, animated inbound/outbound particles, pulsing nodes aggregated by connection count, auto-centered on your machine at startup, wheel zoom / drag / double-click reset, hover for endpoint details, click to sync with the side panel
- **Connection list** — active connections: process (icon / signature state), protocol, remote address with rDNS hostname, GeoIP location, live rates and total bytes; sortable columns, text search (Ctrl+F to focus), protocol filter, collapsible grouping by process, context menu (close connection / locate binary / copy / view history)
- **Port listening** — TCP LISTEN and UDP bound endpoints in one view, switchable on the same page as the connection list, searchable by process/path; a toast fires when a new listening port appears
- **Per-connection byte accounting** — ETW kernel network events (TCP/UDP send/receive bytes, short-lived connection capture), no driver required
- **Floating ball** — an edge-docked always-on indicator of total up/down rates (bar-level tiers), expandable into a per-process rate ranking with today's totals, draggable with position memory

### Blocking & prompting

- **New-connection prompt** — public connections that match no rule raise a prompt (allow/deny), scoped to this connection only / this target / the whole program; timeout defaults to deny
- **Rule engine** — matching on action (allow/block) × direction × protocol × process × remote (CIDR or domain) × port, priority-ordered, enable toggle and reordering, SQLite persistence, multiple profiles, JSON import/export; hit counters persist across restarts so dead rules stand out
- **WFP blocking** — enabled rules are translated into Windows Filtering Platform filters that take real effect (processes matched by image path); a dynamic session self-destructs on exit, leaving no residual blocking
- **Silent mode** — three states: off (follow the prompt switch) / allow (silent permit) / deny (block-everything fallback), reachable from both Settings and the tray

### History & statistics

- **Connection history** — each closed connection is persisted to SQLite as a whole row (including transfer bytes); the process summary view merges live connections in real time
- **Four query views** — detail / aggregate (process × protocol × remote) / process summary / usage (daily or hourly bucketed bar chart), time range and multi-condition filters, CSV export (UTF-8 BOM)
- **Usage quota** — monthly traffic quota (accumulated on the local-timezone month boundary) with toasts at 80%/100%
- **Auto cleanup** — retention-based deletion that reclaims disk space, plus an over-quota reminder

### System integration

- **System tray** — stays in the background, closing the window minimizes to tray, hover tooltip follows live rates, silent mode / prompt switch controlled straight from the menu
- **Global hotkey** — click the field and press any combo (Win/Ctrl/Alt/Shift freely combined) to summon the main window back to the traffic map from anywhere
- **LAN devices** — same-subnet devices discovered from the ARP cache, new-device badge, online state, toast on new-device join
- **Update check** — About section in Settings, stable and preview channels, one-click jump to the release page
- **Logging** — leveled logs (latest.log file / live log window) with key paths and failure reasons traceable
- **Also** — city-level GeoIP (down to prefecture-level cities in China), rDNS hostname lookup, English & Simplified Chinese UI, cold-HUD dark/light themes, number formatting (Chinese 万/亿 or international K/M/B, following the UI language or set manually), single-instance activation, best-effort flush of live connections at system shutdown

## Build & Run

Requires a Rust stable toolchain:

```bash
cargo run --release
```

- Windows 10+ only (the UI renders with the system bundled Microsoft YaHei and Consolas fonts, and fails loudly when missing)
- **Admin rights are optional**: unelevated runs are read-only monitoring (no per-connection bytes, no blocking, no "close connection"; the UI says so); elevated runs get the full capability set. ETW collection and WFP blocking run in-process — no driver, no service
- **Portable data**: the data root sits next to the exe — `config.toml` (settings), `data/netowl.db` (history and rules), `logs/` (runtime logs); deleting the directory resets everything

### Building the Installer

[cargo-packager](https://crates.io/crates/cargo-packager) produces the NSIS installer:

```bash
cargo install cargo-packager
cargo packager --release
```

The artifact lands at `target/release/netowl_<version>_x64-setup.exe`: per-user install (no admin required), start-menu/desktop shortcuts and a "launch at startup" option (the app writes the registry itself, and the option can be toggled any time in Settings; uninstalled cleanly). The package embeds all resources; the `locales/` catalog ships with it (the app falls back to embedded strings when the directory is missing). The installer script is customized from the cargo-packager default template (see `packaging/nsis/installer.nsi`); re-diff against upstream when bumping cargo-packager.

## Offline Data & Licensing

Two offline datasets are embedded; the app **never calls a map or IP-database API at runtime** (public-IP probing and rDNS aside, both optional):

- **Map basemap** `assets/mapdata.bin`: China administrative divisions ([Alibaba Cloud DataV GeoAtlas](https://datav.aliyun.com/area/svgconfig/) static GeoJSON, no API key; province borders, the South China Sea boundary drawn as the **ten-segment line**, Hong Kong/Macao/Taiwan shown as provincial-level divisions) + world borders/oceans/lakes ([Natural Earth](https://www.naturalearthdata.com/), Public Domain, 110m/50m simplified tiers). Layered rendering: the world layer draws first and the China layer covers it, hiding NE border geometry that mis-extends into China. DataV data is GCJ-02, with an inherent <0.02 degree offset against NE's WGS-84 — below visual notice. DataV is officially positioned for use within Alibaba Cloud products; confirm the terms before third-party distribution or commercial use.
- **GeoIP database** `assets/geoip.bin` (~2.7 MB, 496k entries): [ip2region](https://github.com/lionsoul2014/ip2region) v4 xdb (Apache-2.0) provides the IP-range to location mapping, Chinese city coordinates come from DataV province/city data, and foreign city coordinates plus English names come from [GeoNames](https://www.geonames.org/) cities15000 (Public Domain). City-level granularity: China (incl. HK/Macao/Taiwan) down to prefecture-level cities; foreign matches by city with fallback to province/country.

Regeneration steps for both datasets are in the header comments of [tools/build_mapdata.py](tools/build_mapdata.py) and [tools/build_geoip.py](tools/build_geoip.py) (source files go into `tools/cache/`, missing items are downloaded automatically; `tools/cache/` is not committed).

## Tech Stack

- **GUI**: egui/eframe 0.36 (glow backend, immediate-mode pure-Rust rendering, no webview)
- **Blocking**: Windows Filtering Platform (hand-written FFI, dynamic session)
- **Collection**: IP Helper API (TCP/UDP tables, GetIfTable2, ARP cache) + ETW (Microsoft-Windows-Kernel-Network)
- **Persistence**: rusqlite (bundled SQLite, WAL)
- **Tray**: tray-icon + muda
- **Localization**: fluent-bundle
- **Logging**: tracing (file / console / log window, three sinks)
- Icons and other resources are embedded at compile time (`include_bytes!`); the release artifact is a single file with no resource directory required

See [AGENTS.md](AGENTS.md) for development conventions and architecture notes.

## License

[GPL-3.0](LICENSE)
