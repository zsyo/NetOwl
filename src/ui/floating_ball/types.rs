//! 悬浮窗类型与刻度常量:三态相位、数据快照、交互结果与交互状态,
//! 以及几何/时序刻度(窗口尺寸、延迟阈值、viewport 标识)。

use std::time::{Duration, Instant};

use eframe::egui;

use crate::platform::monitor;
use crate::storage::config::FloatingBallConfig;
use crate::ui::Page;

/// 全显控件尺寸(逻辑点):两行速率文字 + 内边距,容纳最长速率形态
/// (高度同时容纳半隐窄条的上下两组信号格与组间隔)
pub(super) const BAR_W: f32 = 88.0;
pub(super) const BAR_H: f32 = 52.0;
/// 全显态控件滑出后与屏幕边缘的间隙
pub(super) const REVEAL: f32 = 10.0;
/// 半隐时露出的窄条宽度(贴边窗口比控件多出的部分;比全显间隙
/// 更宽以保证信号格识别度)
pub(super) const STRIP_W: f32 = 14.0;
/// 球窗口尺寸:容纳全显控件 + 半隐窄条余量(半隐时控件大部分移出
/// 窗口被裁剪,只露贴边侧 STRIP_W 窄条)
pub(super) const BALL_WIN_W: f32 = BAR_W + STRIP_W;
pub(super) const BALL_WIN_H: f32 = BAR_H;
/// 浮窗尺寸(逻辑点;头部一行 + 上/下榜各 TOP_N 行 + 分隔线与按钮
/// 紧跟内容流式排布,实测满布局内容约 302,底部留少量余量)
pub(super) const HOVER_W: f32 = 264.0;
pub(super) const HOVER_H: f32 = 332.0;
/// 浮窗与球的间距
pub(super) const BUBBLE_GAP: f32 = 8.0;
/// 全显态鼠标停止移动后弹出浮窗的静默时长(移动即重置,滑过不弹)
pub(super) const EXPAND_DELAY: Duration = Duration::from_millis(500);
/// 进入全显后的判定静默窗:吸收触发全显时的反应延迟尾段与手部漂移,
/// 窗口结束后才开始二次移动采样(期间不计移动、不武装)
pub(super) const REVEAL_GRACE: Duration = Duration::from_millis(500);
/// 信号格档位阈值(字节/秒,对数分档):速率达到第 i 档点亮第 i 格
/// (靠圆心的格先亮)
pub(super) const RATE_STEPS: [u64; 4] = [1, 8 * 1024, 256 * 1024, 8 * 1024 * 1024];
/// 上传/下载榜单各取前 N
pub(super) const TOP_N: usize = 3;
/// 鼠标离开球与浮窗后半隐延迟(期间重入则跳过)
pub(super) const RETRACT_DELAY: Duration = Duration::from_millis(500);
/// 拖动判定阈值(逻辑点)
pub(super) const DRAG_THRESHOLD: f32 = 6.0;
/// 几何命令下发阈值(逻辑点;小于此差异不重发防抖动)
pub(super) const GEOM_EPSILON: f32 = 0.5;
/// 三个 viewport 的唯一 id 与窗口标题(标题仅作 FindWindow 定位锚,不显示)
pub(super) const BALL_VIEWPORT_ID: &str = "netowl-floating-ball";
pub(super) const PANEL_VIEWPORT_ID: &str = "netowl-floating-ball-panel";
pub(super) const MENU_VIEWPORT_ID: &str = "netowl-floating-ball-menu";
pub(super) const BALL_TITLE: &str = "NetOwl Ball";
pub(super) const PANEL_TITLE: &str = "NetOwl Ball Panel";
pub(super) const MENU_TITLE: &str = "NetOwl Ball Menu";

/// 贴边方向
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapEdge {
    Left,
    Right,
}

/// 悬浮球三态:贴边半隐 / 全显(hover 滑出)/ 详情浮窗
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Phase {
    Docked,
    Revealed,
    Expanded,
}

/// 单进程实时速率行(浮窗榜单;icon 为图标纹理快照,绘制不回查 App)
#[derive(Clone)]
pub struct ProcRate {
    pub name: String,
    pub icon: Option<egui::TextureHandle>,
    pub down: u64,
    pub up: u64,
}

/// 悬浮球数据快照(app 层节流聚合,绘制只读;速率均为字节/秒)
#[derive(Default)]
pub struct BallData {
    pub logo: Option<egui::TextureHandle>,
    pub default_icon: Option<egui::TextureHandle>,
    /// 总速率 (下行, 上行)
    pub rates: (u64, u64),
    /// 今日累计总量 (下行, 上行),本地时区自然日
    pub today: (u64, u64),
    pub up_top: Vec<ProcRate>,
    pub down_top: Vec<ProcRate>,
    /// 提权可用;否则 ETW 关闭,进程速率不可统计(浮窗显示提示)
    pub elevated: bool,
}

/// show 的交互结果(帧内聚合,由 app 层消费)
#[derive(Default)]
pub struct BallOutcome {
    /// 位置已变化,需回写 config
    pub pos_dirty: bool,
    /// 唤出主窗口并落到指定页(浮窗"查看详情" = 连接页;菜单"显示窗口"
    /// = 地图页、菜单"设置" = 设置页)
    pub show_main: Option<Page>,
    /// 菜单改动过配置开关,需 mark_config_dirty 持久化
    pub config_touched: bool,
    /// 菜单"关闭悬浮窗":app 层置 enabled = false
    pub close: bool,
}

/// 悬浮窗交互状态;持久化位置以 config.floating_ball 为唯一来源,
/// 变更经 BallOutcome::pos_dirty 回写。
/// 字段对悬浮窗模块族(interaction 等)开放,外部不可触达
pub struct BallState {
    /// 当前贴边方向(None = 尚未完成首次贴边)
    pub(super) edge: Option<SnapEdge>,
    /// 球窗口位置(外框左上角,逻辑点)
    pub(super) pos: (f32, f32),
    /// 当前交互阶段(贴边/全显/浮窗)
    pub(super) phase: Phase,
    /// 右键菜单是否打开
    pub(super) menu_open: bool,
    /// 进入全显的时刻(静默窗 REVEAL_GRACE 计时起点,窗口内不采样)
    pub(super) revealed_at: Option<Instant>,
    /// 全显内二次移动的最后时刻(停止 EXPAND_DELAY 后弹出浮窗;
    /// None = 未二次移动,静止维持全显不弹)
    pub(super) last_move_at: Option<Instant>,
    /// 全显后鼠标是否已静止过一帧(二次移动 = 从静止发起的移动;
    /// 进入全显的滑行尾段不算,不武装弹出)
    pub(super) stationary_seen: bool,
    /// 鼠标离开时刻(None = 指针在球或浮窗上)
    pub(super) left_at: Option<Instant>,
    /// 按下位置(None = 未按下;超阈值成拖动,原位释放为点击)
    pub(super) press_pos: Option<egui::Pos2>,
    /// 系统拖动中(StartDrag 已发出)
    pub(super) dragging: bool,
    /// DWM 去边框下次检查时刻(None = 立即;viewport 重建后随节流重设)
    pub(super) dwm_retry_at: Option<Instant>,
    /// 浮窗本次显示周期内已提升过 z 序(显示上升沿置位,隐藏复位;
    /// 未找到 HWND 视为未提升,下一帧重试)
    pub(super) panel_raised: bool,
    /// 右键菜单本次显示周期内已提升过 z 序(同上)
    pub(super) menu_raised: bool,
}

impl BallState {
    pub fn new(cfg: &FloatingBallConfig) -> Self {
        let (wa_w, wa_h) = monitor::workarea_logical();
        let (x, y) = if cfg.x != i32::MIN && cfg.y != i32::MIN {
            (cfg.x as f32, cfg.y as f32)
        } else {
            // 无记忆位置:主屏右缘垂直居中
            (wa_w - BALL_WIN_W, (wa_h - BALL_WIN_H) * 0.5)
        };
        // 贴边方向按记忆位置所在显示器判断(多屏下副屏坐标可为负)
        let (wl, _, wr, _) = monitor::workarea_of(x + BALL_WIN_W * 0.5, y);
        BallState {
            edge: Some(if x + BALL_WIN_W * 0.5 <= (wl + wr) * 0.5 {
                SnapEdge::Left
            } else {
                SnapEdge::Right
            }),
            pos: (x, y),
            // 关闭"贴边自动隐藏"时常显(初始即为全显形态)
            phase: if cfg.auto_hide_edge {
                Phase::Docked
            } else {
                Phase::Revealed
            },
            menu_open: false,
            revealed_at: None,
            last_move_at: None,
            stationary_seen: false,
            left_at: None,
            press_pos: None,
            dragging: false,
            dwm_retry_at: None,
            panel_raised: false,
            menu_raised: false,
        }
    }

    /// 当前收起条位置(逻辑点,回写 config 用)
    pub fn pos(&self) -> (f32, f32) {
        self.pos
    }
}
