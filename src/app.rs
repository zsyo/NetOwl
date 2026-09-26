//! 应用编排:托盘事件、采集 tick、页面切换、配置持久化(窗口几何/语言)、
//! 关闭到托盘与退出。
//!
//! eframe 0.36 的 App trait 拆分为 logic(每帧逻辑,窗口隐藏时仍会被调用)
//! 与 ui(绘制)。托盘命令、几何捕获与配置写盘节流放在 logic。

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::collector::{Collector, MockCollector};
use crate::config::Config;
use crate::i18n::I18n;
use crate::model::Connection;
use crate::theme;
use crate::tray::{self, Tray};
use crate::ui::{self, Page};

/// 重绘节奏:地图页动画 30fps,静态页面低频
const REPAINT_ANIMATED: Duration = Duration::from_millis(33);
const REPAINT_IDLE: Duration = Duration::from_millis(500);
/// 配置写盘防抖:合并连续变更(窗口拖动/缩放每帧都在变)
const CONFIG_SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
/// 窗口几何恢复完成判定:超时放弃匹配(避免命令未生效时永久跳过捕获)
const RESTORE_TIMEOUT: Duration = Duration::from_secs(2);
/// 恢复匹配容差(物理像素)
const RESTORE_TOLERANCE: i32 = 2;

/// 待恢复的窗口几何(物理像素)
type WindowRect = (i32, i32, i32, i32, bool);

pub struct NetOwlApp {
    page: Page,
    /// 上次所在页面:用于检测进入设置页时重扫 locales 新增语言
    last_page: Page,
    collector: Box<dyn Collector>,
    i18n: I18n,
    config: Config,
    conns: Vec<Connection>,
    tray_rx: Receiver<String>,
    should_exit: bool,
    /// 首帧窗口几何恢复目标;发送命令后转为 restore_active 等待生效
    pending_restore: Option<WindowRect>,
    /// 恢复命令已发送,几何生效前跳过捕获(防止默认位置覆盖配置)
    restore_active: bool,
    restore_started: Instant,
    config_dirty: bool,
    config_dirty_since: Instant,
    /// 托盘句柄保活,drop 时移除托盘图标
    _tray: Tray,
}

impl NetOwlApp {
    pub fn new(cc: &eframe::CreationContext<'_>, i18n: I18n, config: Config) -> Self {
        theme::install(&cc.egui_ctx);
        let (_tray, tray_rx) = tray::create(cc.egui_ctx.clone());
        let pending_restore = config.window_position();
        let pending_restore =
            pending_restore.map(|(x, y, w, h)| (x, y, w, h, config.window.maximized));
        NetOwlApp {
            page: Page::Map,
            last_page: Page::Map,
            collector: Box::new(MockCollector::new()),
            i18n,
            config,
            conns: Vec::new(),
            tray_rx,
            should_exit: false,
            pending_restore,
            restore_active: false,
            restore_started: Instant::now(),
            config_dirty: false,
            config_dirty_since: Instant::now(),
            _tray,
        }
    }

    fn handle_tray_commands(&mut self, ctx: &egui::Context) {
        for cmd in tray::drain(&self.tray_rx) {
            match cmd.as_str() {
                tray::CMD_SHOW => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                tray::CMD_HIDE => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
                tray::CMD_QUIT => {
                    // 退出前把待写配置立即落盘
                    self.config.save_to_file();
                    self.config_dirty = false;
                    self.should_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                _ => {}
            }
        }
    }

    /// 首帧按当前 DPI 把配置中的物理几何换算为逻辑值下发给窗口。
    /// egui 命令路径内部会乘回同一 pixels_per_point,因此物理位置精确还原,
    /// 多屏不同 DPI 下也不会漂移。
    fn restore_window_geometry(&mut self, ctx: &egui::Context) {
        let Some((x, y, w, h, maximized)) = self.pending_restore.take() else {
            return;
        };
        let ppp = ctx.pixels_per_point();
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
            x as f32 / ppp,
            y as f32 / ppp,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            w as f32 / ppp,
            h as f32 / ppp,
        )));
        if maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        self.restore_active = true;
        self.restore_started = Instant::now();
    }

    /// 捕获窗口几何(物理像素)写入配置;恢复生效期间跳过,防默认位置覆盖。
    fn capture_window_geometry(&mut self, ctx: &egui::Context) {
        if self.restore_active {
            let target = self
                .config
                .window_position()
                .map(|(x, y, w, h)| (x, y, w, h, self.config.window.maximized));
            let ppp = ctx.pixels_per_point();
            let (outer, inner, maximized) = ctx.input(|i| {
                let v = i.viewport();
                (v.outer_rect, v.inner_rect, v.maximized)
            });
            let settled = match (target, outer, inner) {
                (Some((tx, ty, tw, th, tmax)), Some(outer), Some(inner)) => {
                    if tmax {
                        maximized == Some(true)
                    } else {
                        (outer.min.x * ppp).round() as i32 - tx <= RESTORE_TOLERANCE
                            && (outer.min.y * ppp).round() as i32 - ty <= RESTORE_TOLERANCE
                            && (inner.width() * ppp).round() as i32 - tw <= RESTORE_TOLERANCE
                            && (inner.height() * ppp).round() as i32 - th <= RESTORE_TOLERANCE
                    }
                }
                _ => false,
            };
            if settled || self.restore_started.elapsed() > RESTORE_TIMEOUT {
                self.restore_active = false;
            } else {
                return;
            }
        }

        let minimized = ctx.input(|i| i.viewport().minimized);
        if minimized == Some(true) {
            return;
        }
        let ppp = ctx.pixels_per_point();
        let (outer, inner, maximized) = ctx.input(|i| {
            let v = i.viewport();
            (v.outer_rect, v.inner_rect, v.maximized)
        });
        if let (Some(outer), Some(inner)) = (outer, inner) {
            let changed = self.config.set_window(
                (outer.min.x * ppp).round() as i32,
                (outer.min.y * ppp).round() as i32,
                (inner.width() * ppp).round() as i32,
                (inner.height() * ppp).round() as i32,
                maximized == Some(true),
            );
            if changed {
                self.mark_config_dirty();
            }
        }
    }

    /// 界面语言与配置不一致(设置页切换)时同步进配置
    fn sync_language_to_config(&mut self) {
        if self.i18n.current_lang != self.config.general.language {
            self.config.set_language(self.i18n.current_lang.clone());
            self.mark_config_dirty();
        }
    }

    fn mark_config_dirty(&mut self) {
        self.config_dirty = true;
        self.config_dirty_since = Instant::now();
    }
}

impl eframe::App for NetOwlApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_tray_commands(ctx);
        self.conns = self.collector.snapshot();

        // 进入设置页时重扫 locales,加载运行期间新增的词条文件
        if self.page == Page::Settings && self.last_page != Page::Settings {
            self.i18n.refresh_languages();
        }
        self.last_page = self.page;

        self.sync_language_to_config();
        self.restore_window_geometry(ctx);
        self.capture_window_geometry(ctx);

        // 点关闭按钮 = 隐藏到托盘;仅托盘"退出"命令置位后才放行
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.should_exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // 配置写盘节流:防抖窗口内合并连续变更,只落盘最后一次
        if self.config_dirty && self.config_dirty_since.elapsed() >= CONFIG_SAVE_DEBOUNCE {
            self.config.save_to_file();
            self.config_dirty = false;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let page = &mut self.page;
        let conns = &self.conns;
        let i18n = &mut self.i18n;

        egui::Panel::left("nav")
            .exact_size(210.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_PANEL)
                    .inner_margin(egui::Margin { left: 14, right: 14, top: 18, bottom: 14 }),
            )
            .show(ui, |ui| ui::nav_ui(ui, page, conns, i18n));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_BASE)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ui, |ui| ui::central_ui(ui, page, conns, i18n));

        let repaint = match page {
            Page::Map => REPAINT_ANIMATED,
            _ => REPAINT_IDLE,
        };
        ui.ctx().request_repaint_after(repaint);
    }
}
