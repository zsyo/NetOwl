<p align="center"><strong>简体中文</strong> | <a href="README_EN.md">English</a></p>

# NetOwl

![License](https://img.shields.io/badge/license-GPL--3.0-blue) ![Platform](https://img.shields.io/badge/platform-Windows%2010%2B-0078D6) ![Rust](https://img.shields.io/badge/Rust-stable-DEA584?logo=rust)

NetOwl 是一个 Windows 平台的网络连接监控工具,灵感来自 macOS 上的 Little Snitch:让你看清每个应用正在把数据发往何处。核心体验是**流量地图**——以本机为中心,把每一条网络连接实时呈现在世界地图上。

> 项目目前处于早期骨架阶段,界面与模拟数据已可运行,真实数据采集与拦截能力按路线图逐步交付。

![NetOwl 界面截图](docs/screenshot.png)

## 功能特性

已实现:

- **流量地图** — 世界地图上实时呈现进程连接:弧线连接、入站/出站流动粒子、按连接数聚合的脉冲节点,悬停查看城市明细
- **连接列表** — 按流量排序的活跃连接表:进程、协议、远端地址、地理位置、上传/下载量
- **系统托盘** — 常驻后台,关闭窗口即最小化到托盘,左键快速唤出

计划中:

- 真实连接采集(ETW / TCP 表,无需驱动)
- 多语言界面(基于 fluent)
- 允许/拒绝拦截(WFP)与弹窗询问
- 规则引擎与持久化
- macOS / Linux 支持

## 构建与运行

需要 Rust stable 工具链:

```bash
cargo run --release
```

当前仅支持 Windows 10+(界面中文渲染使用系统自带的微软雅黑字体)。

## 技术栈

- **GUI**: egui/eframe(即时模式,纯 Rust 自绘 GPU 渲染,无 webview)
- **托盘**: tray-icon + muda
- **多语言**: fluent-bundle
- 图标等资源编译期内嵌(`include_bytes!`),发布产物为单文件,无需携带资源目录

开发规范与架构约定见 [AGENTS.md](AGENTS.md)。

## 许可

[GPL-3.0](LICENSE)
