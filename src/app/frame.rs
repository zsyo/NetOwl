//! eframe::App trait 实现(trait impl 不可拆分,logic 与 ui 必须同块):
//! logic = 每帧逻辑编排(窗口隐藏时仍被调用);ui = 每帧绘制编排
//! (标题栏、导航栏、地图面板、中央面板、重绘节奏、边缘缩放、浮层),
//! 可下沉的函数体在 layout。

use std::time::Instant;

use eframe::egui;

use super::{CONFIG_SAVE_DEBOUNCE, FRAME_STATS_INTERVAL, HITS_FLUSH_INTERVAL, NetOwlApp};
use crate::collector;
use crate::platform::{shutdown_hook, single_instance};
use crate::storage::history_query;
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
        // 离开设置页即取消快捷键捕获:录入框不在渲染,挂起的捕获会让
        // 下次进入设置页时任何按键都被当成组合键录制
        if self.hotkey_capture.active && self.page != crate::ui::Page::Settings {
            self.hotkey_capture.active = false;
        }
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

    /// 键盘快捷键:Ctrl+F 聚焦连接页搜索框——仅连接页内有效(连接与
    /// 监听两视图共用搜索词),其它页面按下不跳页:从地图/历史页按
    /// Ctrl+F 突然跳到连接页不符合"就地聚焦"的直觉。Esc 清搜索词由
    /// 搜索框自身处理(焦点在框内时),Inspector 的 Esc 清选中已在其
    /// 面板内实现;全局热键走托盘命令通道(platform::global_hotkey),
    /// 与托盘菜单同一处理路径
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let focus_search = self.page == crate::ui::Page::Connections
            && ctx.input(|i| {
                i.key_pressed(egui::Key::F) && (i.modifiers.ctrl || i.modifiers.mac_cmd)
            });
        if focus_search {
            self.focus_conn_search = true;
            ctx.request_repaint();
        }
    }

    /// 日志归档自动清理:本地日期翻转时清一次(启动必清,之后每天
    /// 0 点过后再清);保留天数 0 = 不清理。清理本身按文件修改时间
    /// 计(NTFS 同名新文件继承创建时间,创建时间不可靠)
    fn poll_log_cleanup(&mut self) {
        let today = crate::logging::local_day();
        if self.log_cleanup_day == today {
            return;
        }
        self.log_cleanup_day = today;
        let days = self.config.general.log_retain_days;
        let removed = crate::logging::cleanup_old_logs(u64::from(days));
        if removed > 0 {
            tracing::info!("[Log] 自动清理 {removed} 个超过 {days} 天的日志归档");
        }
    }
    /// 自建右键菜单浮层:开/关判定 + 绘制 + 动作入队。菜单是
    /// Foreground Area 浮层,与表格行 rect 不相交——点击不经表格
    /// hit-test(context_menu"按住跨帧重排点击错位"的根除),右键也
    /// 不经 egui(raw_input_hook 拦截,Label 选中态不受影响)
    fn show_menu_float(&mut self, ui: &mut egui::Ui) {
        // 本帧右键 press:命中行热区则开(替换旧菜单),未命中关闭。
        // 选中快照/范围随菜单定格(打开期间拖选/取消不影响已开菜单)
        for (pos, pressed) in self.right_clicks.drain(..) {
            if !pressed {
                continue;
            }
            self.menu = ui::menu_float::OpenMenu::open_at(
                &self.row_hits,
                pos,
                self.selected_text.clone(),
                self.sel_range,
            );
            if let Some(m) = &self.menu {
                tracing::debug!("[Menu] 打开 page={} 列={:?}", m.hit.ctx.kind(), m.col);
                // 冻结被右击表格的数据源:打开期间行内容/顺序/速率列
                // 静止(采集与查询在后台继续,关闭后恢复即为最新)
                self.menu_frozen = match m.hit.ctx.kind() {
                    "conn" => Some(ui::menu_float::MenuFrozen::Conns {
                        conns: self.conns.clone(),
                        rates: self.conn_rates.clone(),
                    }),
                    "listen" => Some(ui::menu_float::MenuFrozen::Listens(self.listens.clone())),
                    "lan" => Some(ui::menu_float::MenuFrozen::Lan(self.lan.devices().to_vec())),
                    "detail" | "aggregate" | "summary" => Some(ui::menu_float::MenuFrozen::History),
                    _ => None,
                };
                if self.menu_frozen.is_some() {
                    tracing::debug!("[Menu] 冻结表格数据源({})", m.hit.ctx.kind());
                }
            }
        }
        let Some(mut menu) = self.menu.take() else {
            return;
        };
        // 关闭:Esc / 左键点在菜单外。首帧 rect 未建立(ZERO),跳过
        // 外点判定;按下点即本帧右键锚点,同帧左键 press 不存在
        let (esc, outside) = ui.ctx().input(|i| {
            let esc = i.key_pressed(egui::Key::Escape);
            let outside = menu.rect.is_positive()
                && i.pointer.primary_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !menu.rect.contains(p));
            (esc, outside)
        });
        if esc || outside {
            tracing::debug!("[Menu] 关闭({})", if esc { "Esc" } else { "外点" });
            self.menu = None;
            self.menu_frozen = None;
            return;
        }
        let (actions, rect) = ui::menu_float::show(ui.ctx(), &self.i18n, &menu, self.elevated);
        menu.rect = rect;
        if actions.is_empty() {
            // 无动作:菜单保持打开(rect 已刷新,供下帧外点判定)
            self.menu = Some(menu);
        } else {
            // 有点击项:菜单关闭,冻结一并解除(动作帧末执行)
            self.menu_frozen = None;
        }
        // 有点击项:菜单关闭,动作帧末执行
        self.menu_actions.extend(actions);
    }

    /// 帧末执行菜单动作(剪贴板 + 底部气泡 / 结束连接 / 定位 / 跳转)
    fn run_menu_actions(&mut self, ctx: &egui::Context) {
        for a in self.menu_actions.drain(..) {
            tracing::debug!("[Menu] 执行动作 {}", a.kind());
            match a {
                ui::menu_float::MenuAction::CopyText(text) => {
                    ctx.copy_text(text);
                    ui::toast::push_bottom(
                        &mut self.toasts,
                        ui::toast::ToastKind::Success,
                        self.i18n.t("toast-copied"),
                    );
                }
                ui::menu_float::MenuAction::CopySelectedText(text) => {
                    ctx.copy_text(text);
                    ui::toast::push_bottom(
                        &mut self.toasts,
                        ui::toast::ToastKind::Success,
                        self.i18n.t("toast-copied"),
                    );
                    // 选中态随本次复制结束(清高亮与快照,一次性)
                    self.selected_text = None;
                    self.sel_range = None;
                    self.sel_active = false;
                    ctx.plugin_or_default::<egui::text_selection::LabelSelectionState>()
                        .lock()
                        .clear_selection();
                }
                ui::menu_float::MenuAction::KillConn(conn) => {
                    match collector::close_tcp_connection(&conn) {
                        Ok(()) => tracing::info!(
                            "[Connections] 已请求结束连接 {}:{} -> {}:{} (PID {})",
                            conn.local_addr,
                            conn.local_port,
                            conn.remote_ip,
                            conn.remote_port,
                            conn.pid
                        ),
                        Err(e) => tracing::warn!(
                            "[Connections] 结束连接失败 {}:{} -> {}:{}: {e}",
                            conn.local_addr,
                            conn.local_port,
                            conn.remote_ip,
                            conn.remote_port
                        ),
                    }
                }
                ui::menu_float::MenuAction::LocateProc(path) => {
                    crate::platform::paths::select_in_explorer(std::path::Path::new(&path));
                }
                ui::menu_float::MenuAction::NavHistory => {
                    self.nav_request = Some(ui::Page::History);
                }
                ui::menu_float::MenuAction::DeleteDetail {
                    event_id,
                    first_seen,
                    last_seen,
                } => {
                    // 明细删除直接执行(与迁移前 detail 菜单一致,无确认)
                    match history_query::delete_detail(
                        &self.history_db,
                        event_id,
                        first_seen,
                        last_seen,
                    ) {
                        Ok(n) => {
                            self.history.dirty = true;
                            tracing::info!("[History] 已删除 {n} 条明细记录");
                        }
                        Err(e) => tracing::warn!("[History] 删除明细记录失败: {e}"),
                    }
                }
                ui::menu_float::MenuAction::AskDelete(pending) => {
                    // 聚合组/整进程删除走确认弹窗链路(confirm_delete_modal)
                    self.history.pending_delete = Some(pending);
                }
            }
        }
    }
}

impl eframe::App for NetOwlApp {
    /// 右键事件拦截(egui 处理输入之前):摘掉 Secondary press/release
    /// 两事件(必须一并删——只删 press 时,若同帧 Primary 在按下,
    /// release 仍会生成 Click 让 secondary_clicked 触发)。egui 的
    /// Label 选中把任何按键 press 当"开始新选择"(text_cursor_state.rs
    /// :79 any_pressed 不分键),右键在选中文字上会破坏选中态;摘掉后
    /// Label 完全感知不到右键,选中分毫不动。事件转入 right_clicks
    /// 队列,由 ui() 末尾的自建右键菜单消费
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let mut rights = Vec::new();
        raw_input.events.retain(|e| {
            if let egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                ..
            } = e
            {
                rights.push((*pos, *pressed));
                return false;
            }
            true
        });
        self.right_clicks.extend(rights);
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let t0 = Instant::now();
        // 选中文本快照与范围(T5):egui 无公开 API 读选中内容,拖选
        // 结束(松手且仍有选中)后注入一次 Event::Copy,CopyCatcher 在
        // 帧末 output_hook 捕获 egui 写出的 CopyText 定格。不能每帧
        // 注入——egui 的 copy_text 是同帧多 Label 累加器,拖选中间态
        // (按下点塌缩/跨 Label 半选)会覆写快照,定格注入只取松手瞬间
        // 的最终选中(探针实证:单 Label 子串正确,右键拦截后 selection
        // 保留)。sel_range = 拖选指针起止边界框,菜单据此判断"是否
        // 在选中文字上右键";单击(无拖选)与选中取消即清,杜绝跨页污染
        let sel_now = ctx
            .plugin_or_default::<egui::text_selection::LabelSelectionState>()
            .lock()
            .has_selection();
        let (primary_down, press_origin, hover_pos, any_released) = ctx.input(|i| {
            (
                i.pointer.primary_down(),
                i.pointer.press_origin(),
                i.pointer.hover_pos(),
                i.pointer.any_released(),
            )
        });
        // 拖选进行中:primary down 且指针离开按下点超过阈值(区分单击)。
        // 边界框向外 expand 10px——水平拖选矩形高度为 0,用户右键
        // 落在文字行内任意位置(非精确拖选线)也应算选中区
        if let (Some(o), Some(h)) = (press_origin, hover_pos)
            && primary_down
            && o.distance(h) > 4.0
        {
            self.sel_range = Some(egui::Rect::from_two_pos(o, h).expand(10.0));
            if !self.sel_active {
                tracing::debug!("[Sel] 拖选开始 {o:?}");
            }
            self.sel_active = true;
        }
        if any_released {
            if self.sel_active && sel_now {
                // 拖选松手:注入一次 Copy 定格快照(当帧 ui 消费,
                // 下帧 logic 经 CopyCatcher 读走)
                ctx.input_mut(|i| i.events.push(egui::Event::Copy));
            } else if !self.sel_active {
                // 单击(光标移动/无拖选):快照与范围清空,菜单不再
                // 出现"复制选中文本"
                self.selected_text = None;
                self.sel_range = None;
            }
            self.sel_active = false;
        }
        if !sel_now {
            // 选中被取消(点击他处/Esc):范围与快照一并清
            self.sel_range = None;
            self.selected_text = None;
        }
        if let Some(text) = ctx
            .plugin_or_default::<ui::copy_catcher::CopyCatcher>()
            .lock()
            .take()
        {
            tracing::debug!("[Sel] 快照定格 {} 字符", text.chars().count());
            self.selected_text = Some(text);
        }
        self.handle_tray_commands(ctx);
        self.calibrate_window_visible();
        self.sync_tray_pinned();
        self.sync_autostart();
        self.poll_ball_data();
        self.update_tray_tooltip();
        self.handle_shortcuts(ctx);
        self.sync_hotkey();
        // 规则命中计数 30s 批量落盘(硬杀最多丢 30s 的增量)
        if self.hits_flush_at.elapsed() >= HITS_FLUSH_INTERVAL {
            self.hits_flush_at = Instant::now();
            if let Err(e) = self.rules.flush_hits(&self.history_db) {
                tracing::warn!("[Rules] 命中计数落盘失败: {e}");
            }
        }
        // 日志归档自动清理:启动时一次,之后每天 0 点过后(本地日期
        // 翻转)一次;保留 0 天 = 不清理
        self.poll_log_cleanup();
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
        self.poll_quality();
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
            // 选中快照与范围随切页清空(旧页 Label 已不在场,保留会
            // 让新页右键菜单误显"复制选中文本"——跨页污染根治)
            self.selected_text = None;
            self.sel_range = None;
            self.sel_active = false;
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
        // 行热区表每帧重建(show_panels 内各行渲染时注册)
        self.row_hits.clear();
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
        // 自建右键菜单浮层(开/关判定 + 绘制 + 动作入队)
        self.show_menu_float(ui);
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
        // 菜单动作先行执行:复制等反馈走底部气泡,与 toast 同帧展示
        self.run_menu_actions(ui.ctx());
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
