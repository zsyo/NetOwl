<p align="center"><strong>简体中文</strong> | <a href="README_EN.md">English</a></p>

# NetOwl

![License](https://img.shields.io/badge/license-GPL--3.0-blue) ![Platform](https://img.shields.io/badge/platform-Windows%2010%2B-0078D6) ![Rust](https://img.shields.io/badge/Rust-stable-DEA584?logo=rust)

NetOwl 是一个 Windows 平台的网络连接监控工具,灵感来自 macOS 上的 Little Snitch:让你看清每个应用正在把数据发往何处。核心体验是**流量地图**——以本机为中心,把每一条网络连接实时呈现在世界地图上。

![NetOwl 界面截图](docs/screenshot.png)

## 功能特性

### 实时监控

- **流量地图** — 世界地图上实时呈现进程连接:弧线连线、入站/出站流动粒子、按连接数聚合的脉冲节点,滚轮缩放/拖拽/双击复位,悬停查看端点明细,点击联动侧栏
- **连接列表** — 活跃连接表:进程(图标/签名状态)、协议、远端地址与 rDNS 域名、GeoIP 归属、实时速率与累计字节;表头排序、文本搜索、右键菜单(结束连接/定位程序/复制)
- **每连接字节统计** — ETW 内核网络事件采集(TCP/UDP 收发字节、短命连接捕获),无需驱动
- **悬浮球** — 贴边常显上/下行总速率(信号格分档),展开查看进程速率排行,可拖动贴边记忆位置

### 拦截与询问

- **新连接询问** — 未命中规则的公网新连接弹窗询问(允许/拒绝),生效范围可选仅本次/仅此目标/整个程序,倒计时超时默认拒绝
- **规则引擎** — 动作(允许/阻断)× 方向 × 协议 × 进程 × 远端(网段或域名)× 端口的组合匹配,优先级排序,启停/上移下移,SQLite 持久化,支持多配置档与 JSON 导入导出
- **WFP 拦截** — 启用规则翻译为 Windows Filtering Platform 过滤器真实生效(进程按映像路径精确匹配),动态会话退出自毁,不残留拦截
- **静默模式** — off(按询问开关)/ allow(静默放行)/ deny(全阻断兜底)三态,设置页与托盘双入口

### 历史与统计

- **连接历史** — 已完结连接整行落盘 SQLite(含收发字节),进程汇总视图实时并入活跃连接字节
- **四视图查询** — 明细 / 聚合(进程×协议×远端)/ 进程汇总 / 用量(按天/按小时分桶柱状图),时间范围与多条件筛选,CSV 导出(UTF-8 BOM)
- **自动清理** — 按保留期自动删除并归还磁盘空间,超容提醒

### 系统集成

- **系统托盘** — 常驻后台,关闭窗口即最小化到托盘,悬停提示跟随实时速率刷新,静默模式/询问开关菜单直控
- **局域网设备** — ARP 缓存发现同网段设备,新设备徽章、在线状态
- **日志** — 分级日志(文件 latest.log / 日志窗口实时查看),关键路径与失败原因可追溯
- **其他** — GeoIP 城市级归属(中国到地级市)、rDNS 域名反查、深/浅主题、中英双语界面、单实例唤出、关机时活跃连接兜底落库

## 构建与运行

需要 Rust stable 工具链:

```bash
cargo run --release
```

- 仅支持 Windows 10+(界面渲染使用系统自带的微软雅黑与 Consolas 字体,缺失时显式报错)
- **管理员权限可选**:普通权限下为只读监控(无每连接字节统计、拦截与"结束连接"不可用,界面有提示);以管理员运行获得完整能力。ETW 采集与 WFP 拦截均在进程内完成,无需驱动与服务
- **便携式数据**:数据根为 exe 同级目录——`config.toml`(配置)、`data/netowl.db`(历史与规则)、`logs/`(运行日志),删除目录即完全重置

### 构建安装包

[cargo-packager](https://crates.io/crates/cargo-packager) 生成 NSIS 安装程序:

```bash
cargo install cargo-packager
cargo packager --release
```

产物为 `target/release/netowl_<版本>_x64-setup.exe`:按当前用户安装(免管理员),提供开始菜单/桌面快捷方式与"开机自启动"选项(勾选后由应用自身写入注册表,可随时在设置页关闭),卸载时自动清理。安装包内嵌全部资源,`locales/` 词条目录随包分发(目录缺失时应用回退到内嵌词条)。安装器脚本基于 cargo-packager 默认模板定制(见 `packaging/nsis/installer.nsi`),升级 cargo-packager 时需与上游模板比对同步。

## 离线数据与授权

程序内嵌两类离线数据,运行时**不调用任何地图/IP 库接口**(公网 IP 探测与 rDNS 除外,均为可选功能):

- **地图底图** `assets/mapdata.bin`:中国行政区划([阿里云 DataV GeoAtlas](https://datav.aliyun.com/area/svgconfig/) 静态 GeoJSON,免 key,含省级边界、南海断续国界线按**十段线**绘制,港澳台按省级行政区显示)+ 世界国界/海洋/湖泊([Natural Earth](https://www.naturalearthdata.com/),公有领域,110m/50m 两档抽稀)。分层渲染:世界层先画、中国层覆盖其上,消除 NE 数据中伸入中国境内的边界误划。DataV 数据为 GCJ-02,与 NE 的 WGS-84 存在 <0.02 度固有偏移,不影响观感;DataV 官方定位于阿里云产品内使用,三方分发或商用前请自行确认条款。
- **GeoIP 归属库** `assets/geoip.bin`(约 2.7 MB,49.6 万条):[ip2region](https://github.com/lionsoul2014/ip2region) v4 xdb(Apache-2.0)提供 IP 段与归属地映射,中国城市坐标取自 DataV 省市数据,外国城市坐标与英文名取自 [GeoNames](https://www.geonames.org/) cities15000(公有领域)。城市级粒度:中国(含港澳台)到地级市,外国按城市匹配、未命中回退省/国家级。

重新生成两类数据的方法分别见 [tools/build_mapdata.py](tools/build_mapdata.py) 与 [tools/build_geoip.py](tools/build_geoip.py) 头部注释(数据源文件放入 `tools/cache/`,缺失项自动下载;`tools/cache/` 不入库)。

## 技术栈

- **GUI**: egui/eframe 0.36(glow 后端,即时模式纯 Rust 自绘渲染,无 webview)
- **拦截**: Windows Filtering Platform(手写 FFI,动态会话)
- **采集**: IP Helper API(TCP/UDP 表、GetIfTable2、ARP 缓存)+ ETW(Microsoft-Windows-Kernel-Network)
- **持久化**: rusqlite(SQLite bundled,WAL)
- **托盘**: tray-icon + muda
- **多语言**: fluent-bundle
- **日志**: tracing(文件 / 控制台 / 日志窗口三层输出)
- 图标等资源编译期内嵌(`include_bytes!`),发布产物为单文件,无需携带资源目录

开发规范与架构约定见 [AGENTS.md](AGENTS.md)。

## 许可

[GPL-3.0](LICENSE)
