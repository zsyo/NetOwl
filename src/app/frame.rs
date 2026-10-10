//! eframe::App trait 实现(trait impl 不可拆分,logic 与 ui 必须同块):
//! logic = 每帧逻辑编排(窗口隐藏时仍被调用);ui = 每帧绘制编排
//! (标题栏、导航栏、地图面板、中央面板、重绘节奏、边缘缩放、浮层),
//! 可下沉的函数体在 layout。

use std::time::Instant;

use eframe::egui;

use super::{CONFIG_SAVE_DEBOUNCE, FRAME_STATS_INTERVAL, NetOwlApp};
use crate::platform::{shutdown_hook, single_instance};
use crate::ui::ask as ui_ask;
use crate::ui::{self, Page, theme};

impl NetOwlApp {
    /// 界面语言与配置不一致(设置页切换)时同步进配置
    pub(super) fn sync_language_to_config(&mut self) {
        if self.i18n.current_lang != self.config.general.language {
            tracing::info!("[App] 界面语言 -> {}", self.i18n.current_lang);
            self.config.set_language(self.i18n.current_lang.clone());
            self.mark_config_dirty();
        }
    }

    /// 界面主题与配置不一致(设置页切换)时同步进配置
    pub(super) fn sync_theme_to_config(&mut self) {
        let current = theme::theme_str();
        if current != self.config.general.theme {
            tracing::info!("[App] 主题 -> {current}");
            self.config.set_theme(current.to_owned());
            self.mark_config_dirty();
        }
    }

    pub(super) fn mark_config_dirty(&mut self) {
        self.config_dirty = true;
        self.config_dirty_since = Instant::now();
    }

    /// 键盘快捷键:Ctrl+F 聚焦连接页搜索(非连接页先切页);Esc 清
    /// 搜索词由搜索框自身处理(焦点在框内时),Inspector 的 Esc 清
    /// 选中已在其面板内实现;全局热键 Ctrl+Alt+N 走托盘命令通道
    /// (platform::global_hotkey),与托盘菜单同一处理路径
    /// 全局热键同步:config 组合变化即热切换(线程内注销重注册,不必
    /// 重启);注册失败(组合被其它程序占用)弹 toast——冲突从静默死亡
    /// 变为可见,用户可改选其它预设组合
    fn sync_hotkey(&mut self) {
        if self.config.general.hotkey != self.hotkey_synced {
            self.hotkey_synced = self.config.general.hotkey.clone();
            tracing::debug!("[Hotkey] 应用组合切换 -> {:?}", self.config.general.hotkey);
            self.hotkey.set(&self.config.general.hotkey);
        }
        while let Ok(report) = self.hotkey_report_rx.try_recv() {
            let crate::platform::global_hotkey::HotkeyReport::Failed { combo, code } = report
            else {
                continue;
            };
            let reason = match code {
                crate::platform::global_hotkey::ERROR_HOTKEY_TAKEN => {
                    self.i18n.t("hotkey-reason-taken")
                }
                // 0 = 组合串无法解析(config 被手工改坏)
                0 => self.i18n.t("hotkey-reason-invalid"),
                _ => self
                    .i18n
                    .t_with_args("hotkey-reason-other", &[("code", code.to_string())]),
            };
            let text = self.i18n.t_with_args(
                "hotkey-fail-toast",
                &[
                    ("combo", crate::platform::global_hotkey::display(&combo)),
                    ("reason", reason),
                ],
            );
            crate::ui::toast::push(&mut self.toasts, crate::ui::toast::ToastKind::Warn, text);
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let focus_search =
            ctx.input(|i| i.key_pressed(egui::Key::F) && (i.modifiers.ctrl || i.modifiers.mac_cmd));
        if focus_search {
            if self.page != crate::ui::Page::Connections {
                self.page = crate::ui::Page::Connections;
            }
            self.focus_conn_search = true;
            ctx.request_repaint();
        }
    }
}

impl eframe::App for NetOwlApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let t0 = Instant::now();
        self.handle_tray_commands(ctx);
        self.calibrate_window_visible();
        self.sync_tray_pinned();
        self.sync_autostart();
        self.poll_ball_data();
        self.update_tray_tooltip();
        self.handle_shortcuts(ctx);
        self.sync_hotkey();
        // 关机/注销落库钩子:首帧安装一次,内部防重复(winit 不处理
        // ENDSESSION,关机时唯一能把活跃连接落库的路径)
        shutdown_hook::install(
            if self.main_hwnd != 0 {
                self.main_hwnd
            } else {
                single_instance::main_hwnd(crate::APP_NAME)
            },
            &mut self.tracker,
            &mut self.writer,
        );
        self.ensure_collector();
        self.poll_local_ip(ctx);
        // 新设备接入通知(设置页可关):基线轮之后的插入事件入 toast 队列
        for (ip, mac) in self.lan.poll(&mut self.history_db) {
            if self.config.general.lan_notify {
                let text = self
                    .i18n
                    .t_with_args("lan-notify-toast", &[("ip", ip.to_string()), ("mac", mac)]);
                crate::ui::toast::push(&mut self.toasts, crate::ui::toast::ToastKind::Info, text);
            }
        }
        self.poll_traffic(ctx);
        self.poll_conns(ctx);
        self.writer.set_retention(self.config.general.history_days);
        self.poll_quota();
        self.poll_update_check(ctx);
        self.sync_silent_mode();
        self.sync_ask_toggle();
        self.poll_wfp();

        // 进入设置页时重扫 locales,加载运行期间新增的词条文件;
        // 进入历史页时标记重新加载(结果与库大小)
        if self.page != self.last_page {
            tracing::trace!("[Frame] 页面 {:?} -> {:?}", self.last_page, self.page);
            if self.page == Page::Settings {
                self.i18n.refresh_languages();
            }
            if self.page == Page::History {
                self.history.dirty = true;
            }
        }
        self.last_page = self.page;

        self.sync_language_to_config();
        self.sync_theme_to_config();
        self.restore_window_geometry(ctx);
        self.capture_window_geometry(ctx);

        // 点关闭按钮 = 隐藏到托盘;仅托盘"退出"命令置位后才放行
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.should_exit {
            tracing::info!("[Window] 关闭到托盘,主窗口隐藏");
            self.window_visible = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // 配置写盘节流:防抖窗口内合并连续变更,只落盘最后一次
        if self.config_dirty && self.config_dirty_since.elapsed() >= CONFIG_SAVE_DEBOUNCE {
            self.config.save_to_file();
            self.config_dirty = false;
        }

        // TRACE 帧统计:累计本帧 logic 耗时,按周期汇总输出(全隐身帧
        // 只有 logic,ui 耗时按实际发生帧均摊)
        let logic_us = t0.elapsed().as_micros() as u64;
        self.frame_stats_frames += 1;
        self.frame_stats_logic_us += logic_us;
        self.frame_stats_logic_max_us = self.frame_stats_logic_max_us.max(logic_us);
        if self.frame_stats_at.elapsed() >= FRAME_STATS_INTERVAL {
            let frames = self.frame_stats_frames;
            if frames > 0 {
                let n = u64::from(frames);
                tracing::trace!(
                    "[Frame] 近 {:?}: {frames} 帧, logic 平均 {:.2}/峰值 {:.2} ms, ui 平均 {:.2}/峰值 {:.2} ms",
                    FRAME_STATS_INTERVAL,
                    self.frame_stats_logic_us as f64 / n as f64 / 1000.0,
                    self.frame_stats_logic_max_us as f64 / 1000.0,
                    self.frame_stats_ui_us as f64 / n as f64 / 1000.0,
                    self.frame_stats_ui_max_us as f64 / 1000.0,
                );
            }
            self.frame_stats_at = Instant::now();
            self.frame_stats_frames = 0;
            self.frame_stats_logic_us = 0;
            self.frame_stats_ui_us = 0;
            self.frame_stats_logic_max_us = 0;
            self.frame_stats_ui_max_us = 0;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ui_t0 = Instant::now();
        // 页面与重绘判定先行(page 借用持续到帧末,后续不能碰 self)
        let shown = self.is_shown(ui.ctx());
        let log_open = self.log_window.open;
        let collector_kind = self.collector.kind();

        // 自绘标题栏(顶栏最先声明,位于导航栏之上);窗口动作落地
        let title_action = egui::Panel::top("titlebar")
            .exact_size(ui::titlebar::HEIGHT)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::c().bg_panel))
            .show(ui, |ui| ui::titlebar::show(ui, crate::APP_NAME))
            .inner;
        self.handle_title_action(title_action, ui.ctx());

        let config_changed = self.show_panels(ui, collector_kind);
        // 跨页跳转请求(Inspector 跳连接/历史、右键菜单查看历史)帧末应用
        if let Some(p) = self.nav_request.take() {
            self.page = p;
        }
        let repaint = super::layout::repaint_interval(shown, &self.page, log_open);
        // 节奏变化(页面切换/显示隐藏)输出 TRACE;稳定节奏不重复输出
        let repaint_ms = u64::try_from(repaint.as_millis()).unwrap_or(u64::MAX);
        if self.last_repaint_ms != Some(repaint_ms) {
            tracing::trace!(
                "[Frame] 重绘间隔 -> {repaint_ms} ms(主窗口{}可见)",
                if shown { "" } else { "不" }
            );
            self.last_repaint_ms = Some(repaint_ms);
        }
        ui.ctx().request_repaint_after(repaint);

        self.resize_hotspots(ui);

        // 新连接询问弹窗(独立 viewport,预建常驻);决策即时生效
        let decision = ui_ask::show(
            ui.ctx(),
            &mut self.asker,
            &mut self.ask_ui_visible,
            &self.i18n,
        );
        if let Some(d) = decision {
            self.apply_decision(d);
        }
        // 日志浏览窗口(独立 viewport;内存层增量拉取,关闭时停止收集)
        ui::log_window::show(ui.ctx(), &mut self.log_window, &self.i18n);

        // 悬浮窗(独立 viewport;主窗口隐藏时低频帧仍维持显示与数据刷新)
        self.show_floating_ball(ui);

        // 右下角 toast 通知(新设备接入/用量配额告警)
        crate::ui::toast::show(ui.ctx(), &mut self.toasts);

        if config_changed {
            self.mark_config_dirty();
        }

        // ui 耗时累计进帧统计(logic 侧汇总输出)
        let ui_us = ui_t0.elapsed().as_micros() as u64;
        self.frame_stats_ui_us += ui_us;
        self.frame_stats_ui_max_us = self.frame_stats_ui_max_us.max(ui_us);
    }
}
