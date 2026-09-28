//! 新连接询问弹窗:桌面右下角(任务栏上方)的置顶无标题栏小窗,
//! 参照安全软件网络授权弹窗惯例;展示进程与远端身份、倒计时,
//! 决策 = 动作(允许/拒绝)× 生效范围(仅本次/永久此目标/永久整个程序)。

use eframe::egui;
use egui::{CornerRadius, RichText, Stroke};
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, SPI_GETWORKAREA,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};

use crate::app::ask::{AskItem, Decision, Scope};
use crate::i18n::I18n;
use crate::ui::theme;

/// 弹窗尺寸(逻辑点)与右下角边距
const WIDTH: f32 = 430.0;
const HEIGHT: f32 = 266.0;
const MARGIN: f32 = 16.0;

/// 主屏工作区(物理像素,已排除任务栏)换算为逻辑点;查询失败退回
/// 整屏尺寸(GetSystemMetrics 恒成功,不丢失定位能力)
fn workarea_logical() -> (f32, f32) {
    unsafe {
        let sys_scale = GetDpiForSystem() as f32 / 96.0;
        let mut rect = RECT::default();
        let ok = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut rect as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        if ok.is_ok() && rect.right > 0 && rect.bottom > 0 {
            (
                rect.right as f32 / sys_scale,
                rect.bottom as f32 / sys_scale,
            )
        } else {
            (
                GetSystemMetrics(SM_CXSCREEN) as f32 / sys_scale,
                GetSystemMetrics(SM_CYSCREEN) as f32 / sys_scale,
            )
        }
    }
}

/// 弹出询问;返回用户决策(None = 本帧未决策)。
/// show_viewport_immediate 的闭包在本帧渲染期同步执行;
/// 范围选择写入 item.scope(随当前询问跨帧持久,避免每帧重置)
pub fn show(ctx: &egui::Context, item: &mut AskItem, i18n: &I18n) -> Option<Decision> {
    let (wa_w, wa_h) = workarea_logical();
    let mut decision: Option<Decision> = None;

    let builder = egui::ViewportBuilder::default()
        .with_decorations(false)
        .with_taskbar(false)
        .with_always_on_top()
        .with_resizable(false)
        .with_close_button(false)
        .with_active(true)
        .with_inner_size([WIDTH, HEIGHT])
        .with_position([wa_w - WIDTH - MARGIN, wa_h - HEIGHT - MARGIN]);

    ctx.show_viewport_immediate(
        egui::ViewportId(egui::Id::new("netowl-ask")),
        builder,
        |ui, _class| {
            // CentralPanel 填满整个窗口(Frame 按内容收缩会在底部露出
            // 未绘制背景);面板带 accent 细边框
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(theme::c().bg_panel)
                        .stroke(Stroke::new(1.0, theme::c().accent.gamma_multiply(0.55)))
                        .inner_margin(egui::Margin::same(16)),
                )
                .show(ui, |ui| {
                    let process = if item.process.is_empty() {
                        format!("PID {}", item.pid)
                    } else {
                        item.process.clone()
                    };
                    ui.label(
                        RichText::new(i18n.t("ask-title"))
                            .size(16.0)
                            .strong()
                            .color(theme::c().accent),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(i18n.t_with_args("ask-question", &[("process", process)]))
                            .size(14.0)
                            .color(theme::c().text),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(item.remote_display())
                            .size(15.0)
                            .strong()
                            .color(theme::c().text),
                    );
                    if item.domain.is_some() {
                        ui.label(theme::dim_text(&item.remote_ip.to_string(), 12.0));
                    }
                    ui.label(theme::dim_text(
                        &format!(
                            "{} {} · {} {}",
                            i18n.t("col-proto"),
                            item.proto.as_str(),
                            i18n.t("col-port"),
                            item.remote_port
                        ),
                        12.0,
                    ));
                    ui.add_space(8.0);

                    // 倒计时:超时自动执行默认动作(拒绝·仅本次);
                    // 剩余秒数直接显示在条内,高度需容纳文字避免垂直裁剪;
                    // 余量分级配色:>70% 正常绿,>30% 警示黄,更低红色
                    let remaining = item
                        .deadline
                        .checked_duration_since(std::time::Instant::now())
                        .unwrap_or_default();
                    let total = crate::app::ask::ASK_TIMEOUT.as_secs_f32().max(0.001);
                    let ratio = (remaining.as_secs_f32() / total).clamp(0.0, 1.0);
                    let bar_color = if ratio > 0.7 {
                        theme::c().status_ok
                    } else if ratio > 0.3 {
                        theme::c().status_warn
                    } else {
                        theme::c().danger
                    };
                    ui.add(
                        egui::ProgressBar::new(ratio)
                            .desired_height(18.0)
                            .fill(bar_color)
                            .text(i18n.t_with_args(
                                "ask-timeout-hint",
                                &[("n", remaining.as_secs().to_string())],
                            )),
                    );
                    ui.add_space(8.0);

                    // 永久选项说明:跟随进度条之后、决策控件之前
                    ui.label(theme::dim_text(&i18n.t("ask-always-hint"), 11.0));
                    ui.add_space(8.0);

                    // 生效范围:仅本次 / 永久·仅此目标 / 永久·整个程序
                    ui.horizontal(|ui| {
                        ui.label(theme::dim_text(&i18n.t("ask-scope"), 13.0));
                        let name = |s: Scope| match s {
                            Scope::Once => i18n.t("ask-scope-once"),
                            Scope::Target => i18n.t("ask-scope-target"),
                            Scope::Process => i18n.t("ask-scope-process"),
                        };
                        egui::ComboBox::from_id_salt("ask-scope")
                            .width(190.0)
                            .selected_text(RichText::new(name(item.scope)).size(13.0))
                            .show_ui(ui, |ui| {
                                for s in [Scope::Once, Scope::Target, Scope::Process] {
                                    if ui
                                        .selectable_label(
                                            item.scope == s,
                                            RichText::new(name(s)).size(13.0),
                                        )
                                        .clicked()
                                    {
                                        item.scope = s;
                                    }
                                }
                            });
                    });
                    ui.add_space(10.0);

                    // 动作按钮:允许为主按钮(accent 填充白字),拒绝 danger 描边;
                    // cross 用 Min:按钮行按内容高度排列(用 Center 会占满
                    // 剩余高度导致按钮垂直悬空在窗口中部)
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        let btn = |ui: &mut egui::Ui, key: &str, allow: bool| {
                            let mut text = RichText::new(i18n.t(key)).size(14.0).strong();
                            if allow {
                                text = text.color(egui::Color32::WHITE);
                            }
                            let mut b = egui::Button::new(text)
                                .corner_radius(CornerRadius::same(theme::RADIUS_MD))
                                .min_size(egui::vec2(120.0, 32.0));
                            if allow {
                                b = b.fill(theme::c().accent);
                            } else {
                                b = b.stroke(Stroke::new(1.0, theme::c().danger));
                            }
                            ui.add(b).clicked()
                        };
                        if btn(ui, "ask-allow", true) {
                            decision = Some(Decision {
                                allow: true,
                                scope: item.scope,
                            });
                        }
                        if btn(ui, "ask-deny", false) {
                            decision = Some(Decision {
                                allow: false,
                                scope: item.scope,
                            });
                        }
                    });
                });
            // 驱动倒计时条刷新
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        },
    );
    decision
}
