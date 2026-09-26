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
- **多语言** — 基于 fluent 的界面多语言,设置页可切换,支持外部词条文件热加载

计划中:

- 真实连接采集(ETW / TCP 表,无需驱动)
- 允许/拒绝拦截(WFP)与弹窗询问
- 规则引擎与持久化
- macOS / Linux 支持

## 构建与运行

需要 Rust stable 工具链:

```bash
cargo run --release
```

当前仅支持 Windows 10+(界面中文渲染使用系统自带的微软雅黑字体)。

## 地图数据来源

流量地图的底图由两部分矢量数据拼接绘制,均离线下载后经脚本量化/抽稀生成 `assets/mapdata.bin`,编译期内嵌,程序运行时**不调用任何地图接口**:

- **中国行政区划**(省界、港澳台、藏南、南海诸岛、南海断续国界线):[阿里云 DataV GeoAtlas](https://datav.aliyun.com/area/svgconfig/) 静态 GeoJSON(`geo.datav.aliyun.com/areas_v3/bound/100000_full.json`,免 key)。中国(含港澳台)的 Natural Earth 数据不参与,港澳台按省级行政区正常显示(台湾省、香港特别行政区、澳门特别行政区),南海断续国界线按**十段线**绘制。
- **世界国界、海洋与内陆湖**: [Natural Earth](https://www.naturalearthdata.com/)(公有领域),110m/50m 两档经共享边感知的 Douglas-Peucker 抽稀;大湖(咸海、五大湖、贝加尔等)取自 NE 湖泊层与历史层(咸海为历史整体轮廓),作为洞环绘入世界层。

分层与衔接:底图分世界层与中国层渲染——世界层先画,中国层整体覆盖其上,NE 中伸入中国境内的邻国国界与边界误划(如藏南)被中国层盖住;两国边界线再由邻国顶点按 ≤0.2 度向中国边界温和吸合,消除相邻不重合的细缝,远离边界的邻国几何保持原样。

坐标系说明:DataV 数据为 GCJ-02(与 NE 的 WGS-84 在边界处有 <0.02 度的固有偏移,不影响定位观感)。

授权提示:DataV GeoAtlas 数据官方定位于阿里云产品内使用;三方分发或商用前请自行确认其授权条款,数据文件本身不从程序外获取,也不在运行时联网。重新生成底图数据的方法见 [tools/build_mapdata.py](tools/build_mapdata.py) 头部注释。

## 技术栈

- **GUI**: egui/eframe(即时模式,纯 Rust 自绘 GPU 渲染,无 webview)
- **托盘**: tray-icon + muda
- **多语言**: fluent-bundle
- 图标等资源编译期内嵌(`include_bytes!`),发布产物为单文件,无需携带资源目录

开发规范与架构约定见 [AGENTS.md](AGENTS.md)。

## 许可

[GPL-3.0](LICENSE)
