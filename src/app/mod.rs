//! 应用编排:托盘事件、采集 tick、页面切换、配置持久化(窗口几何/语言)、
//! 关闭到托盘与退出。
//!
//! eframe 0.36 的 App trait 拆分为 logic(每帧逻辑,窗口隐藏时仍会被调用)
//! 与 ui(绘制)。结构体单点定义在本模块,方法按功能域分散在子模块的
//! impl 块:frame(eframe::App trait,logic+ui 不可再分)、etw_merge
//! (字节合并)、window(几何/可见性)、tray(托盘与系统注册表同步)、
//! poll(采集编排)、ask_flow(询问决策)、ball(悬浮球)、init(构造)。

mod ask_flow;
mod ball;
mod etw_merge;
mod frame;
mod init;
mod layout;
mod poll;
mod tray;
mod window;

pub mod ask;
pub mod lan;

use std::collections::HashMap;
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;

use self::ask::Asker;
use self::lan::LanState;
use crate::map::basemap;
use crate::model::{Connection, Place};
use crate::net::etw;
use crate::net::local_ip;
use crate::net::rdns;
use crate::net::traffic;
use crate::platform::resize::DragResize;
use crate::platform::tray::Tray;
use crate::platform::update::ReleaseInfo;
use crate::rules;
use crate::rules::wfp;
use crate::storage::config::Config;
use crate::storage::history;
use crate::storage::history_query;
use crate::ui::floating_ball;
use crate::ui::rules as ui_rules;
use crate::ui::{self, Page};

/// 重绘节奏:地图动画 30fps;连接页速率与历史页反查 500ms;其余静态页 1s;
/// 窗口隐藏(托盘)时一律 500ms(动画不可见,不再高帧率空转)
const REPAINT_ANIMATED: Duration = Duration::from_millis(33);
const REPAINT_IDLE: Duration = Duration::from_millis(500);
const REPAINT_STATIC: Duration = Duration::from_millis(1000);
/// 配置写盘防抖:合并连续变更(窗口拖动/缩放每帧都在变)
const CONFIG_SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
/// TRACE 帧耗时统计的汇总周期
const FRAME_STATS_INTERVAL: Duration = Duration::from_secs(5);
/// 窗口几何恢复完成判定:超时放弃匹配(避免命令未生效时永久跳过捕获)
const RESTORE_TIMEOUT: Duration = Duration::from_secs(2);
/// 恢复匹配容差(物理像素)
const RESTORE_TOLERANCE: i32 = 2;
/// 本机公网 IP 重探间隔(重拨/换网后点位跟随更新)
const LOCAL_IP_PROBE_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// 启动地图定位窗口:窗口内收到首个探测结果即把地图跳到本机中心最大缩放;
/// 覆盖单接口 4s×两段超时的最坏探测时长,窗口后(如断网启动后重探成功)
/// 不再自动定位,避免会话中途视图突然飞走
const MAP_LOCATE_WINDOW: Duration = Duration::from_secs(15);
/// 总速率采样间隔:可见/隐藏(托盘)均 1s——托盘悬停提示按秒跟随刷新,
/// GetIfTable2 为本地内核查询开销极小,无需为功耗放宽
const TRAFFIC_INTERVAL_ACTIVE: Duration = Duration::from_secs(1);
const TRAFFIC_INTERVAL_HIDDEN: Duration = Duration::from_secs(1);
/// 速率历史采样点数(约 1 分钟窗口,迷你走势图用)
const RATE_HIST_LEN: usize = 60;
/// 主窗口最小逻辑尺寸(= 默认窗口尺寸;main.rs 视口 min_inner_size 与
/// 无边框缩放钳制同源)
pub const MIN_WINDOW_SIZE: (f32, f32) = (1440.0, 800.0);
/// WFP 过滤器目标集合同步间隔(与采集同频:进程路径出现/消失的生效延迟上限)
const WFP_SYNC_INTERVAL: Duration = Duration::from_secs(1);
/// ETW 流量事件合并间隔(与表快照采集同频)
const ETW_POLL_INTERVAL: Duration = Duration::from_secs(1);
/// 连接快照与派生数据(rDNS/图标/历史/速率/询问)刷新间隔:与表快照采集
/// 同频;地图动画 30fps 的帧内只做绘制,O(连接数) 的逻辑不逐帧执行
const CONNS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
/// 托盘常驻写入重试间隔:托盘设置项由 Explorer 在图标注册时创建,
/// 启动数秒内可能尚不存在
const TRAY_PIN_RETRY_INTERVAL: Duration = Duration::from_secs(60);
/// 自启动 Run 键写入重试间隔(标准用户键,失败多为暂时性系统状态)
const AUTOSTART_RETRY_INTERVAL: Duration = Duration::from_secs(60);
/// 托盘悬停提示的速率刷新间隔(秒级,与速率采样同频)
const TRAY_TIP_INTERVAL: Duration = Duration::from_secs(1);
/// 当日用量(本地时区)查库间隔:浮窗"今日总量"= 落库值 + 活跃连接
/// 实时字节,秒级 SUM 全表开销不值当,分钟级即可
const TODAY_BYTES_INTERVAL: Duration = Duration::from_secs(60);

/// 待恢复的窗口几何(物理像素)
pub(super) type WindowRect = (i32, i32, i32, i32, bool);

pub struct NetOwlApp {
    page: Page,
    /// 上次所在页面:用于检测进入设置页时重扫 locales 新增语言
    last_page: Page,
    /// 流量地图视图(中心/缩放,跨帧保持)
    map_view: basemap::View,
    collector: Box<dyn crate::collector::Collector>,
    /// rDNS 解析(异步 PTR 查询,连接列表/地图信息卡域名优先显示)
    rdns: rdns::Rdns,
    /// 总上传/下载速率采样(GetIfTable2 接口字节差值)
    traffic: traffic::Sampler,
    /// 最近总速率(字节/秒):(下行, 上行),导航栏展示
    rates: (u64, u64),
    /// 总速率历史环形缓冲(时间正序,(下行, 上行));导航栏迷你走势图数据源
    rate_hist: Vec<(u64, u64)>,
    /// 应用图标纹理(导航栏品牌区);启动时从内嵌 PNG 建一次
    logo_tex: egui::TextureHandle,
    /// Windows 默认"应用程序"图标纹理(无路径/提取失败进程的兜底);
    /// None = 尚未提取成功,每轮 poll_icons 重试
    default_icon_tex: Option<egui::TextureHandle>,
    /// 进程图标纹理(键 = 映像路径);None 表示已提取且无图标
    icon_tex: HashMap<String, Option<egui::TextureHandle>>,
    /// 连接历史写线程(批量落盘 conn_events)
    writer: history::Writer,
    /// 连接快照对比器:跟踪活跃连接,消失时生成完结事件
    tracker: history::Tracker,
    /// 历史页状态与查询只读连接
    history: history_query::PageState,
    history_db: history::Db,
    /// 规则集(内存 + SQLite 同步,规则页编辑与连接页求值共用)
    rules: rules::RuleSet,
    rules_page: ui_rules::PageState,
    /// 地图页左右面板与选中状态(端点点击联动,会话态)
    map_panels: ui::map_panel::MapPanelState,
    /// WFP 拦截引擎(启用规则翻译为过滤器,管理线程持有动态会话)
    wfp: wfp::Manager,
    wfp_sync_at: Instant,
    /// ETW 流量事件采集(提权时启动:连接字节填充与短命连接收割)
    etw: Option<etw::Etw>,
    etw_poll_at: Instant,
    /// 连接快照上次刷新时刻(1s 节流,高帧率帧内跳过 O(连接数) 逻辑)
    conns_refresh_at: Instant,
    /// 自身可执行文件路径(启动时缓存一次;静默拒绝模式的自身放行规则用)
    self_path: Option<String>,
    /// 右下角 toast 通知队列(新设备接入/用量配额告警)
    toasts: Vec<crate::ui::toast::Toast>,
    /// 监听条目快照(采集同频;端口监听视图用)
    listens: Vec<crate::model::ListenEntry>,
    /// 用量配额告警检查时刻(分钟级节流)与已通知档位(80%/100%)
    quota_checked_at: Instant,
    quota_flags: (bool, bool),
    /// 连接页视图形态(会话态)
    conn_view: ui::ConnView,
    /// 连接页按进程分组(会话态)
    conn_grouped: bool,
    /// 连接页分组折叠集合(键 = 进程名,会话态)
    conn_collapsed: HashSet<String>,
    /// 跨页跳转请求(绘制侧写入,帧末应用到 page)
    nav_request: Option<ui::Page>,
    /// 曾被表快照合并覆盖的 ETW 流键:完结流命中此集合说明表快照
    /// 跟踪器已记录,不按短命连接重复落盘
    etw_seen: HashSet<etw::FlowKey>,
    /// 每连接实时速率(键 = conn.id,ETW 字节差值/秒)
    conn_rates: HashMap<u64, (u64, u64)>,
    /// UDP 行最近已知远端缓存:表快照无远端,活跃流回填仅在流活跃的轮次
    /// 生效,流空闲收割后行会回落 *:*;缓存 socket 最后通信的远端,
    /// socket 存活期间持续展示(键 = (pid, 本地端口),表行消失时清理)
    udp_last_remote: HashMap<(u32, u16), (Ipv4Addr, u16)>,
    /// 连接列表表头排序状态(会话内,不持久化)
    conn_sort: ui::ConnSortState,
    /// 连接页搜索词(进程/远端/域名包含过滤,会话内,不持久化)
    conn_search: String,
    /// 连接页协议筛选(连接/监听两视图共用,会话内,不持久化)
    conn_proto: Option<crate::model::Protocol>,
    /// 检查更新:UI 触发标志(设置页按钮写入,帧内派发后台线程)
    update_request: bool,
    /// 检查更新:在途请求通道(存在 = 检查中,禁止重复发起)
    update_rx: Option<Receiver<Result<Option<ReleaseInfo>, String>>>,
    /// 检查更新:最近一次结果(None = 尚未检查;渠道切换时清空)
    update_result: Option<Result<Option<ReleaseInfo>, String>>,
    /// 表格行悬停辅助(连接/规则页,跨帧行高供行首垫底判定)
    conn_row_hover: ui::widgets::table::RowHover,
    rules_row_hover: ui::widgets::table::RowHover,
    /// 日志浏览窗口状态(内存层日志展示,参照 wallwarp)
    log_window: ui::log_window::PageState,
    /// 上一轮 ETW 字节快照(速率差值基准,键 = conn.id)
    conn_prev_bytes: HashMap<u64, (u64, u64)>,
    /// 新连接询问(Little Snitch 式弹窗)
    asker: Asker,
    /// 本机公网 IP 探测(公共接口并发,最先成功者胜出)
    local_probe: local_ip::Probe,
    /// 本机公网 IP 的归属定位键;探测失败/未收录时为 None(地图用默认点位)
    local_place: Option<Place>,
    local_probe_at: Instant,
    /// 启动地图定位待执行标志(首个探测结果消费即清,含窗口超期情形)
    map_locate_pending: bool,
    /// 启动地图定位的截止时刻(超过则放弃本次自动定位)
    map_locate_deadline: Instant,
    i18n: crate::i18n::I18n,
    config: Config,
    conns: Vec<Connection>,
    tray_rx: Receiver<String>,
    should_exit: bool,
    /// 是否以管理员令牌运行(启动时判定;ETW/WFP 可用性与右键"结束连接")
    elevated: bool,
    /// 主窗口可见性(自行跟踪):egui 0.36 的 viewport().visible() 恒为 None
    /// 不可依赖;全部可见性变更路径(托盘命令/关闭按钮/单实例唤出)都必须同步此字段
    window_visible: bool,
    /// 主窗口 HWND(启动时按标题缓存,0 = 未找到);用于感知外部 ShowWindow
    main_hwnd: isize,
    /// 无边框窗口拖拽缩放状态(winit 对 undecorated 窗口无边缘 hit-test)
    window_resize: DragResize,
    /// 首帧窗口几何恢复目标;发送命令后转为 restore_active 等待生效
    pending_restore: Option<WindowRect>,
    /// 恢复命令已发送,几何生效前跳过捕获(防止默认位置覆盖配置)
    restore_active: bool,
    restore_started: Instant,
    config_dirty: bool,
    config_dirty_since: Instant,
    /// 帧统计窗口起点(TRACE 帧耗时统计,每 FRAME_STATS_INTERVAL 汇总一条)
    frame_stats_at: Instant,
    frame_stats_frames: u32,
    frame_stats_logic_us: u64,
    frame_stats_ui_us: u64,
    frame_stats_logic_max_us: u64,
    frame_stats_ui_max_us: u64,
    /// 上次下发的重绘间隔(TRACE 输出节奏变化用;None = 尚未输出)
    last_repaint_ms: Option<u64>,
    /// 已写入注册表的托盘常驻状态(None = 尚未成功达成目标态)
    tray_pinned_applied: Option<bool>,
    /// 托盘常驻下次重试时刻(写入失败后定时重试)
    tray_pin_retry_at: Instant,
    /// 已写入注册表的自启动状态(None = 尚未成功达成目标态)
    autostart_applied: Option<bool>,
    /// 自启动下次重试时刻(写入失败后定时重试)
    autostart_retry_at: Instant,
    /// 悬浮球交互状态(贴边/展开/拖动,位置记忆经 show 结果回写 config)
    floating_ball: floating_ball::BallState,
    /// 悬浮球数据快照(总速率 + 进程速率榜,1s 节流聚合)
    ball_data: floating_ball::BallData,
    ball_data_at: Instant,
    /// 当日(本地时区)落库收发字节 (入站, 出站) 与查询时刻:
    /// 浮窗"今日总量"= 此值 + 活跃连接实时字节(分钟级查库)
    today_db_bytes: (u64, u64),
    today_bytes_at: Instant,
    /// 托盘悬停提示上次刷新时刻(速率跟随)
    tray_tip_at: Instant,
    /// 静默模式上次同步值(config 变化或采集器重建后重同步:兜底规则
    /// 与托盘勾选态一次校准)
    silent_synced: Option<String>,
    /// 新连接询问上次同步值(config 变化后校准托盘勾选态一次)
    ask_synced: Option<bool>,
    /// 询问弹窗已应用的可见性(builder 仅在创建窗口时生效,切换靠命令)
    ask_ui_visible: bool,
    /// 局域网设备发现(ARP 轮询 + lan_devices 库合并)
    lan: LanState,
    /// 托盘句柄保活,drop 时移除托盘图标
    _tray: Tray,
}
