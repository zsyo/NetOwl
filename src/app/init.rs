//! NetOwlApp 构造:托盘创建、历史库/规则档装配、ETW 启动与全字段初始化。

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use eframe::egui;

use super::ask::Asker;
use super::lan::LanState;
use super::listen_watch::ListenWatch;
use super::{CONNS_REFRESH_INTERVAL, MAP_LOCATE_WINDOW, NetOwlApp, RATE_HIST_LEN};
use crate::collector::{self, CollectorKind};
use crate::i18n::I18n;
use crate::map::basemap;
use crate::net::{etw, local_ip, rdns, traffic};
use crate::platform::resize::DragResize;
use crate::platform::{single_instance, tray};
use crate::rules;
use crate::rules::wfp;
use crate::storage::config::Config;
use crate::storage::{db, history, history_query};
use crate::ui::rules as ui_rules;
use crate::ui::{Page, floating_ball, theme};

impl NetOwlApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        i18n: I18n,
        mut config: Config,
        minimized: bool,
        autostart_on: bool,
    ) -> Self {
        theme::install(&cc.egui_ctx, &config.general.theme);
        // 静默启动兜底:main.rs 已 with_visible(false),此处再补发一次隐藏,
        // 防 eframe 首帧渲染后自动显示窗口(幂等,窗口不可见时为空操作)
        if minimized {
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        // 安装器 Finish 页"开机自启"勾选交接(见 platform::autostart):置位
        // config 并标记落盘,Run 键由本实例 sync_autostart 写入,config 单源不变
        if autostart_on {
            config.general.autostart = true;
        }
        let (_tray, tray_rx) = tray::create(
            cc.egui_ctx.clone(),
            &i18n,
            &config.general.silent_mode,
            config.general.ask_connections,
        );
        let pending_restore = config.window_position();
        let pending_restore =
            pending_restore.map(|(x, y, w, h)| (x, y, w, h, config.window.maximized));
        let history_db = db::open();
        // 配置档:确保默认档存在并校验配置值有效(无效回退 1,启动语言命名)
        let profile_id = rules::RuleSet::ensure_default_profile(
            &history_db,
            config.general.profile_id,
            &i18n.t("rules-profile-default"),
        );
        config.general.profile_id = profile_id;
        let rules = rules::RuleSet::load(&history_db, profile_id);
        tracing::info!(
            "[Rules] 配置档 {} 已加载 {} 条规则",
            profile_id,
            rules.rules.iter().filter(|r| r.id > 0).count()
        );
        // ETW 流量事件仅在提权进程内可用;失败只记录,字节列退化为 0。
        // 提权状态进程生命周期内不变,顺带给右键"结束连接"等能力判定
        let elevated = wfp::is_elevated();
        tracing::info!(
            "[App] 以{}运行{}",
            if elevated {
                "管理员权限"
            } else {
                "普通权限"
            },
            if elevated {
                ":ETW 流量采集/WFP 拦截/结束连接可用"
            } else {
                ":ETW 流量采集/WFP 拦截/结束连接不可用"
            }
        );
        let etw = if elevated {
            match etw::Etw::start() {
                Ok(e) => {
                    tracing::info!("[ETW] 流量事件采集会话已启动");
                    Some(e)
                }
                Err(e) => {
                    tracing::warn!("[ETW] {e}");
                    None
                }
            }
        } else {
            None
        };
        // 导航栏品牌区图标:内嵌 PNG 一次性建纹理(256x256 RGBA)
        let icon_data = crate::platform::icon::window_icon();
        let logo_tex = cc.egui_ctx.load_texture(
            "app-logo",
            egui::ColorImage::from_rgba_unmultiplied(
                [icon_data.width as usize, icon_data.height as usize],
                &icon_data.rgba,
            ),
            egui::TextureOptions::LINEAR,
        );
        // 悬浮球状态初始化需读取 config(结构体字面量内 config 随后 move)
        let ball_state = floating_ball::BallState::new(&config.floating_ball);
        NetOwlApp {
            page: Page::Map,
            last_page: Page::Map,
            map_view: basemap::View::global(),
            collector: collector::build(CollectorKind::from_config(&config.general.collector)),
            rdns: rdns::Rdns::new(),
            traffic: traffic::Sampler::new(),
            rates: (0, 0),
            rate_hist: Vec::with_capacity(RATE_HIST_LEN + 1),
            logo_tex,
            default_icon_tex: None,
            icon_tex: HashMap::new(),
            writer: history::Writer::spawn(config.general.history_days),
            tracker: history::Tracker::new(),
            history: history_query::PageState::new(),
            history_db,
            rules,
            rules_page: ui_rules::PageState::new(),
            map_panels: crate::ui::map_panel::MapPanelState::default(),
            wfp: wfp::Manager::spawn(),
            wfp_sync_at: Instant::now(),
            etw,
            etw_poll_at: Instant::now(),
            // 首帧立即拉取快照:把起点回拨一个周期
            conns_refresh_at: Instant::now() - CONNS_REFRESH_INTERVAL,
            self_path: std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().into_owned()),
            toasts: Vec::new(),
            listens: Vec::new(),
            quota_checked_at: Instant::now(),
            quota_flags: (false, false),
            conn_view: crate::ui::ConnView::Conns,
            conn_grouped: false,
            conn_collapsed: HashSet::new(),
            nav_request: None,
            etw_seen: HashSet::new(),
            conn_rates: HashMap::new(),
            udp_last_remote: HashMap::new(),
            conn_sort: None,
            conn_search: String::new(),
            focus_conn_search: false,
            conn_proto: None,
            update_request: false,
            update_rx: None,
            update_result: None,
            conn_row_hover: Default::default(),
            rules_row_hover: Default::default(),
            log_window: crate::ui::log_window::PageState::new(),
            conn_prev_bytes: HashMap::new(),
            asker: Asker::new(),
            local_probe: local_ip::Probe::new(),
            local_place: None,
            local_probe_at: Instant::now(),
            map_locate_pending: true,
            map_locate_deadline: Instant::now() + MAP_LOCATE_WINDOW,
            i18n,
            config,
            conns: Vec::new(),
            tray_rx,
            should_exit: false,
            elevated,
            window_visible: !minimized,
            main_hwnd: single_instance::main_hwnd(crate::APP_NAME),
            window_resize: DragResize::default(),
            pending_restore,
            restore_active: false,
            restore_started: Instant::now(),
            config_dirty: autostart_on,
            config_dirty_since: Instant::now(),
            frame_stats_at: Instant::now(),
            frame_stats_frames: 0,
            frame_stats_logic_us: 0,
            frame_stats_ui_us: 0,
            frame_stats_logic_max_us: 0,
            frame_stats_ui_max_us: 0,
            last_repaint_ms: None,
            tray_pinned_applied: None,
            tray_pin_retry_at: Instant::now(),
            autostart_applied: None,
            autostart_retry_at: Instant::now(),
            floating_ball: ball_state,
            ball_data: floating_ball::BallData::default(),
            ball_data_at: Instant::now(),
            today_db_bytes: (0, 0),
            today_bytes_at: Instant::now(),
            tray_tip_at: Instant::now(),
            silent_synced: None,
            ask_synced: None,
            ask_ui_visible: false,
            lan: LanState::new(),
            listen_watch: ListenWatch::new(),
            _tray,
        }
    }
}
