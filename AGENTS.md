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
- src/app.rs - NetOwlApp:状态编排(托盘事件、采集 tick、页面切换、关闭到托盘、退出)
- src/model.rs - Connection/Protocol 等数据结构
- src/collector.rs - Collector trait + MockCollector(模拟数据;真实采集后续以 ETW/TCP 表实现替换)
- src/i18n/ - 多语言模块(mod.rs:locales 扫描/加载/语言列表;translate.rs:查找/插值/回退/告警)
- locales/ - fluent 词条文件(zh-cn.ftl / en.ftl;目录缺失时使用编译期内嵌兜底)
- src/theme.rs - 主题(Visuals 定制、字体加载、颜色与圆角常量)
- src/icon.rs - 应用图标加载(assets 资源编译期内嵌,PNG 解码为 RGBA)
- src/tray.rs - 托盘与菜单(tray-icon + muda)
- src/ui.rs - 主窗口布局与页面(导航栏、连接列表、设置与占位页)
- src/map.rs - 流量地图画布(painter 自绘:投影、大陆背景、连线动画、节点聚合)
- src/world.rs - 简化世界轮廓多边形与城市坐标数据

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
- 颜色/尺寸/圆角统一在 theme.rs 定义,UPPER_SNAKE_CASE 命名,新增前先查重
- 唯一强调色: 深色主题 #5C9DFF(hover/active 派生),禁止散落蓝色硬编码;
  随主题变化的色与固定语义色(入站/出站等)在 theme.rs 内分区定义
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
