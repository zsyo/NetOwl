# NetOwl 项目记忆文件

## 项目概述
- 项目名称: NetOwl
- 项目类型: 网络连接监控工具(Windows 平台的 Little Snitch 复刻,首期定位只读监控+流量地图)
- 开发语言: Rust
- GUI 框架: egui/eframe 0.36(glow 后端),即时模式,纯 Rust 自绘渲染,不依赖 webview
- 支持平台: 前期仅 Windows 10+;macOS/Linux 为后续扩展目标,架构需为此预留边界
- 核心功能: 流量地图(本机节点为中心,连接动画可视化)、连接列表;拦截与规则引擎为后续里程碑
- 许可: GPLv3(仓库根 LICENSE)

## 选型记录(勿轻易重议)
- egui/eframe 0.36:即时模式 painter 可直接绘制节点/连线/动画,同一库覆盖表单式 UI(连接、规则)与画布式 UI(地图);MIT/Apache 双许可
- 否决项: iced 0.14(Elm 架构清晰但 canvas 自定义绘制样板多)、slint 1.16(地图仍需嵌 canvas,社区较小)、xilem(实验性)、floem(生态小)、Tauri(webview,违背"非 web"硬性要求)、native-windows-gui(Win32 控件做不出流量地图)
- 渲染后端用 glow 而非默认 wgpu:编译更快、依赖更少,Windows 上 OpenGL 足够

## 项目结构
- src/main.rs - 应用入口(NativeOptions、窗口图标、run_native)
- src/app.rs - NetOwlApp:状态编排(托盘事件、采集 tick、页面切换、关闭到托盘、
  退出;进程图标纹理缓存按键 = 映像路径,logic 每帧对未缓存路径向采集器请求
  IconState,到位经 ColorImage::from_rgba_unmultiplied 建纹理)
- src/model.rs - Connection/Protocol/Place 等数据结构(city 为 Option<Place>:
  内网/保留段/未收录 IP 归属未知,地图不绘制,列表显示占位;UDP 表行远端
  以 *:* 展示)
- src/geoip.rs - GeoIP 归属定位(assets/geoip.bin 编译期内嵌,首用解析一次;
  IPv4 区间表 LEB128 delta 编码二分查询;城市级粒度:中国含港澳台到地级市、
  外国按城市名匹配 GeoNames,未命中回退省/国家级;place_pos/place_label 为
  地图与列表的统一归属坐标/显示名入口,显示名双语内嵌不走词条)
- src/local_ip.rs - 本机公网 IP 探测(6 个知名公共回显接口并发,手写
  HTTP/1.1 GET 不走系统代理、不引 TLS 依赖,最先返回的合法 IPv4 胜出;
  app 层每 10 分钟重探,经 geoip 得到本机地图点位,失败回退 world::LOCAL)
- src/rdns.rs - rDNS 域名解析(异步 PTR:getnameinfo NI_NAMEREQD 于独立线程
  执行,app 每帧 update 收割;并发上限 8 + 每 250ms 派发 2 个限流,成功/失败
  分别 10min/2min TTL 缓存,失效仅对仍活跃连接重查;回环/私网/保留段不查;
  lookup 供列表与地图信息卡域名优先显示,rdns::display 超长截断)
- src/traffic.rs - 总上传/下载速率(GetIfTable2 各接口 In/OutOctets 采样差值;
  排除回环/隧道;**必须排除 InterfaceAndOperStatusFlags.FilterInterface(bit1)
  接口——WFP 轻量过滤/QoS 过滤接口会镜像底层物理网卡计数,不过滤速率成倍
  虚高**;窗口可见 1s 采样、隐藏 5s,表查询失败沿用旧速率)
- src/collector/ - 连接采集(mod.rs:Collector trait 与 real/mock 工厂 +
  CollectorKind,icon_image 默认返回 Pending;mock.rs:模拟数据供演示/测试;
  query.rs:Win32 查询原语,GetExtendedTcpTable/GetExtendedUdpTable owner-PID
  快照(TCP 仅活动状态,两段式缓冲重试)与 OpenProcess+QueryFullProcessImageNameW
  全路径反查;windows_table.rs:真实采集器,1s 节流,TCP 过滤 SYN_SENT..LAST_ACK,
  连接身份四元组+PID hash 派生稳定 id,进程元数据(名/路径/签名)按 PID 缓存、
  行消失即剔除(PID 4 特判 System),图标按路径常驻缓存(连接关闭后进程再现
  即取即用);signature.rs:WinVerifyTrust Authenticode 校验(UI_NONE+REVOKE_NONE
  不弹窗不联网,须在工作线程跑),每轮限流派发(在途 4/每轮 2 个)结果回填,
  回填前 Unknown;icon.rs:SHGetFileInfoW 取 32x32 关联图标 + GetIconInfo/
  GetDIBits 转 RGBA(工作线程执行,失败缓存 None);表快照无字节语义,
  下载/上传列为 0,字节/速率待 ETW;均为只读 API,无需管理员权限)
- src/i18n/ - 多语言模块(mod.rs:locales 扫描/加载/语言列表;translate.rs:查找/插值/回退/告警)
- locales/ - fluent 词条文件(zh-cn.ftl / en.ftl;目录缺失时使用编译期内嵌兜底)
- src/theme.rs - 主题(深/浅两套 Palette 调色板 + AtomicUsize 主题索引,
  theme::c() 统一取色;Visuals 双主题定制;字体加载;设置页切换即时生效,
  持久化于 config [general] theme)
- src/icon.rs - 应用图标加载(assets 资源编译期内嵌,PNG 解码为 RGBA)
- src/tray.rs - 托盘与菜单(tray-icon + muda)
- src/ui.rs - 主窗口布局与页面(导航栏、连接列表、设置与占位页)
- src/map.rs - 流量地图画布(painter 自绘:连线动画、节点聚合、悬停信息卡、
  视图交互:滚轮锚点缩放/拖拽/双击复位,视图状态存 NetOwlApp;
  经度方向无缝循环:中心经度归一化 [-180,180),节点/连线按可见
  副本平移绘制,悬停按模周期距离,连线取最短方向走短弧)
- src/basemap.rs - 地图底图(视图/投影、经纬网格、陆地与洞环填充、
  海岸线/国界 quad 线段、主要河流折线、南海十段线、国家/海洋/省级
  名称标签;经度按 360 度周期对世界副本循环绘制;每帧视口剔除即时
  渲染,无缓存无状态;标签按缩放分级显隐,英文国家名大写逐字符
  字距,省名弱化色)
- src/triangulate.rs - 简单多边形耳剪三角化(输入量化整数坐标,精确几何判定;
  f32 坐标会破坏共线性导致耳被误判,勿改回浮点)
- src/world.rs - 城市坐标表与矢量底图解码(Natural Earth 世界 +
  DataV 中国混合,两档 LOD:110m 全局 / 50m 放大,zoom>=3 切换;
  海岸线/国界按邻国共享边分类;labels 三种 kind + 十段线段节 +
  河流折线节两档)
- tools/build_mapdata.py - 底图数据生成脚本(混合数据源 ->
    assets/mapdata.bin;原始 GeoJSON 放 tools/cache/,该目录不入库)
- tools/build_geoip.py - GeoIP 归属数据生成脚本(tools/cache/ip2region_v4.xdb
  [Apache-2.0,持续更新] + DataV 100000_full.json 与各省 {adcode}_full.json
  [市级政府驻地坐标] + NE 50m admin_0 + GeoNames cities15000[公有领域,外国
  城市坐标与英文名] -> assets/geoip.bin,NWGI v1 格式;城市级:中国含港澳台
  到地级市[台湾 DataV 无 full 数据回退省级]、外国 (ISO,城市名) 匹配 GeoNames
  同名取人口最多者,未命中回退省/国家级;55 万条 -> 合并 49.6 万条/约 2.7MB;
  重生成需将数据源文件放入 tools/cache/,缺失的会自动下载)

## 架构规范(egui 即时模式)
- 数据与绘制分离:collector 产出模型状态,ui/map 只读绘制,绘制逻辑不修改数据
- 状态集中:全部可变应用状态收敛在 NetOwlApp;绘制函数只接收不可变引用
- 系统调用隔离:平台/采集逻辑不得散落在 UI 层;Collector trait 是唯一数据入口,
  后续替换 MockCollector 为 ETW 实现时不改 UI 代码
- 按需重绘:动画存在时 request_repaint_after(约 30fps),静态页面用长间隔(约 500ms),
  禁止盲目持续 60fps 空转耗电

## 平台规范(Windows 首期)
- 字体:运行时加载 C:\Windows\Fonts\msyh.ttc(微软雅黑)注册为中文 fallback,
  Consolas 注册到 monospace;文件缺失时直接报错,不做静默降级
- 托盘:tray-icon 在 eframe 主线程创建(eframe setup 时),由 winit 消息循环代泵;
  TrayIcon 必须保活(存于 App 内),drop 即移除图标
- 托盘事件:muda/tray-icon 的 set_event_handler 把命令推入 std::sync::mpsc 通道,
  并调用 ctx.request_repaint() 唤醒隐藏状态下的主循环
- 关闭到托盘:实现 App::on_close_event,非退出态时 Visible(false) 隐藏窗口并返回 false;
  仅退出标志置位时放行关闭
- 扩展备忘(跨平台时启用,源自同类项目实战):
  - tray-icon 0.25 的 default features 在 Linux 引入 libappindicator/muda-gtk3/libxdo,
    Windows 无影响;扩展 Linux 时评估 default-features = false + 显式选择后端
    (ksni 或 libappindicator),libxdo 仅服务于未使用的菜单 accelerator
  - Linux 托盘无双击事件(appindicator 限制);Wayland 下托盘与全局坐标能力受限
  - macOS 侧 tray-icon 正迁移 objc2,扩展时核对版本
  - 届时平台专属调用必须收敛到独立 platform 模块,依赖放 [target.'cfg(...)'.dependencies],
    UI 层禁止直接引用平台 crate

## 开发规范

### 1. 代码风格
- 使用 Rust 标准代码风格 (rustfmt),4 空格缩进
- 遵循 Rust 命名约定 (snake_case 函数/变量,PascalCase 类型/trait)
- 公共 API 包含文档注释;注释说明约束与意图,不复述代码,不使用 emoji

### 2. 文件组织
- 代码文件尽量不超过 300 行;超过且逻辑易拆时拆分,承担多职责时必须拆分
- 每个文件职责单一,便于后续用 AI 工具读取和迭代

### 3. 样式常量
- 颜色统一在 theme.rs 的 Palette 调色板字段定义(深浅两套),绘制代码经
  theme::c() 取色,禁止散落硬编码色值,新增色前先查重
- 唯一强调色: 深色主题 #5C9DFF(hover/active 派生),禁止散落蓝色硬编码;
  语义色(入站/出站等)在调色板内按主题分别定义
- 圆角刻度: RADIUS_SM=4 / RADIUS_MD=8 / RADIUS_LG=12,不出现圆角魔法数字
- 可交互控件样式必须区分 hovered / active / disabled 状态

### 4. 多语言(i18n)
- 基于 fluent-bundle:全部 UI 文本经 I18n::t()/t_with_args() 获取,
  禁止在绘制代码里散落字符串字面量(品牌名等与语言无关的常量除外)
- 词条在 locales/<lang>.ftl,每个文件必须含 lang-name;新增文案必须同步补全
  zh-cn 与 en 两种语言
- FTL 变量用 {$name} 插值,代码用 t_with_args(key, &[("name", value)]),
  禁止 .replace() 手动替换;{name} 是消息引用而非变量
- 词条查找顺序:当前语言 → 默认语言(zh-cn)→ 返回键名并告警(相同键只告警一次)
- 数据类显示名(如城市)以数据键派生词条键(city-<key>);新增数据项必须补词条
- 新增语言:按现有模板翻译后放入 locales/ 目录即被自动发现(设置页进入时重扫);
  NETOWL_LANG 环境变量可覆盖初始语言

### 5. 错误处理
- 使用 Result 与有意义的错误信息;文件/系统 API 操作必须处理错误
- 不做静默 fallback:失败直接暴露,不用默认值掩盖(如字体加载失败应显式报错)

### 6. 代码复用
- 相同/相似逻辑必须提取公共函数;仅单文件使用的私有化在本文件,
  跨文件复用的上提到对应公共模块
- 功能相同仅参数不同的函数合并为参数化通用函数
- update/绘制主流程保持简洁,复杂细节委托辅助函数,提交前自查重复代码

### 7. 日志(引入日志库后生效)
- 统一前缀格式: [模块名] [标识] 消息内容,必须包含操作对象,
  禁止"请求成功"式无信息日志
- 第三方库日志收敛到 warn;应用自身档位由 RUST_LOG 控制

### 8. 依赖管理
- 新依赖一律最新稳定版;与依赖树冲突时保持生态一致版本并在 Cargo.toml 注释说明
- 每次改动 Cargo.toml 后核对 Cargo.lock 关键包版本未被降级
- 重要依赖在 Cargo.toml 注释中写明选型理由
- 最小化依赖: 标准库可覆盖的能力(std mpsc、简单 PRNG 等)不引入第三方

### 9. 图标
- 应用图标源文件位于 assets/:窗口图标 app_icon.png(256x256)、托盘图标
  tray_icon.png(64x64),经 include_bytes! 编译期内嵌,发布程序不携带 assets 目录
- app_icon.ico 保留供打包阶段编译进 exe 资源段(文件管理器/快捷方式/安装程序),
  届时以多尺寸层(16/32/48/256)替换现有单层版本
- 图标必须为 8-bit RGBA PNG,解码失败/格式不符时显式报错,不做静默降级
- UI 内禁止 emoji 与 unicode 符号充当图标;assets/icons.ttf(Bootstrap Icons)已就位,
  接入 UI 时码点必须对照字体 cmap 或官方码点表验证,
  注释写真实 glyph 名(注释即契约)
- 底图数据 assets/mapdata.bin 由 tools/build_mapdata.py 生成后入库,
  混合数据源:v5 格式(magic "NWLD" + version=5)
  - 世界国界/海洋:Natural Earth admin_0_countries + geography_marine_polys
    (公有领域);中国(CHN/HKG/MAC/TWN)NE feature 几何与标签剔除
  - 中国行政区划:阿里云 DataV GeoAtlas areas_v3/bound/100000_full.json
    (免 key 静态 GeoJSON,基于天地图,GCJ-02),含 34 省级 geometry、
    100000_JD 十段线;分层渲染(世界层在前、中国层在后),NE 中越界
    的邻国国界由中国层覆盖;邻国界顶点向中国边界温和吸合(<=0.2 度)
    消细缝,远离边界者不动
  - 湖泊:110m 档取 ne_50m_lakes 面积 >=0.2 平方度大湖;50m 档取
    ne_10m_lakes 面积 >=0.05 平方度(补入鄱阳湖、洞庭湖、纳木错、
    色林错、巢湖等)并固定容差 DP 预抽稀,不参与世界层预算二分;
    境内湖洞按 pip 判定归入中国层(否则被中国层陆地填充盖住,
    境内湖泊不可见),境外湖(贝加尔、五大湖等)留世界层;咸海一律
    取历史层整体轮廓
  - 河流:ne_10m_rivers_lake_centerlines 仅 River 类(排除 Lake
    Centerline 避免画进湖面),scalerank<=3(全局档)/<=4(精细档)
    过滤全球主要河流,固定容差 DP 预抽稀;独立折线节存储与渲染
    (世界/中国层之上),不参与环三角化与共享边分类;NE 长江口
    江阴以东整体缺段,由脚本内手工补线(YANGTZE_MOUTH_PATCH,沿
    南岸主槽衔接 NE 端点至吴淞口外入海)
  量化 0.001 度 + varint delta,几何两档;50m 档经共享边感知的
  Douglas-Peucker 抽稀(相邻国家共享段顶点序列一致且 DP 方向对称,
  边界无缝);南极洲几何与标签剔除,底图窗口纬度 [-58, 84];
  港澳台为省级行政区正常显示省名(kind=2,样式弱于国家名),台湾
  NAME_ZH 问题随 NE 中国数据剔除从根本上消除;重生成需下载
  ne_110m/50m_admin_0_countries、ne_50m_geography_marine_polys、
  ne_50m_lakes、ne_10m_lakes、ne_50m_lakes_historic、
  ne_10m_rivers_lake_centerlines 与 100000_full.json 到 tools/cache/
- 地名标签(国家 238 + 海洋 111 + 省 34,共 383 条)内嵌于 mapdata.bin
  标签段,不走 fluent 词条(数量庞大);南海十段线独立线段节;
  河流折线两档独立节;城市(含港澳台市)仍走 city-<key> 词条

### 10. 持久化
- 数据根 = exe 同级目录(Windows 便携式),启动时切换工作目录;
  config.toml 与 data/ 均为相对路径(paths.rs)
- 配置:serde TOML;变更只更新内存,由 App 层 500ms 防抖合并写盘,
  写盘走 临时文件 -> .bak 备份 -> rename;托盘"退出"时立即落盘
- 窗口几何存物理像素(i32::MIN 表示未设置居中);最小化污染坐标(Windows 移窗到
  -32000)不落盘;恢复在首帧按当前 pixels_per_point 换算下发 ViewportCommand
  (egui 坐标全为逻辑 points,物理除以当前 ppp 后精确还原)
- SQLite:data/netowl.db(rusqlite bundled 免系统依赖,WAL);结构迁移以
  schema_version 表记录版本;所有含外部输入的查询必须参数绑定(rusqlite `params!`),
  禁止字符串拼接/format 组装 SQL

## 开发流程规范

### 功能点与 git 提交
- 每完成一个单独的功能点,创建一个 git commit 提交
- commit 前置条件(两者都必须满足):
  - `cargo build` 正常编译通过
  - `cargo clippy` 无任何警告
- commit message 用中文简述功能点(首行一句话),禁止 emoji
- 提交内容仅包含该功能点相关文件;临时验证产物(截图、日志等)不入库,
  收尾删除

### TODO 管理
- 用户提出多功能开发需求时,先将功能点清单写入 TODO 文件(TODO.md),
  每项含简要说明与验收要点
- 每完成一个功能点:在 TODO 文件中勾掉对应项,并为该功能点做一次 git commit
- TODO.md 命中用户全局 gitignore,为本地工作文件,不提交入库,README 也不引用

## 验证规范
- 每次改动 cargo build 通过;界面改动 cargo run 冒烟确认无 panic
- GUI 无法自动断言的视觉项,说明验证方式并交用户确认
