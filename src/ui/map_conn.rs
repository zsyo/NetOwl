//! 地图面板连接明细行(左列表与右侧 Inspector 共用)。
//! 阻断 = 创建永久 Block 规则(求值未命中默认放行),再次点击删除撤销;
//! 已被进程级规则阻断的连接按钮置灰(撤销进程规则会放大放行范围,不做)。
//!
//! 行布局:缩进 + 远端(Truncate 自适应省略)+ 右侧区(协议、下载/上传
//! 分色字节、目标级阻断开关,从右往左排,字节天然右对齐)。Truncate
//! 控件会请求拉满剩余宽,必须放进固定宽度容器且容器宽扣除同行其余
//! 控件,否则溢出部分经 resizable 面板的宽度记忆逐帧放大(见
//! map_inspector 模块注释)。

use eframe::egui;
use egui::{Button, Label, RichText, Vec2};

use crate::model::{Connection, fmt_bytes};
use crate::net::rdns;
use crate::rules::{RemoteKind, Rule};
use crate::ui::icons;
use crate::ui::text_width;
use crate::ui::theme;

/// 连接明细行:远端(域名/地址)、协议、累计字节与目标级阻断开关
pub(crate) fn conn_row(
    ui: &mut egui::Ui,
    rules: &mut crate::rules::RuleSet,
    db: &crate::storage::history::Db,
    i18n: &crate::i18n::I18n,
    rdns: &rdns::Rdns,
    c: &Connection,
) {
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 4.0;
        ui.add_space(16.0);
        let remote = match rdns.lookup(c.remote_ip) {
            Some(host) => format!("{host}:{}", c.remote_port),
            None => c.remote_display(),
        };
        let in_text = fmt_bytes(c.bytes_in);
        let out_text = fmt_bytes(c.bytes_out);
        let proto_text = c.proto.as_str();
        let can_block = !c.process.is_empty();

        // 右侧固定预算:协议 + 双向字节 + 阻断按钮与行内间距
        let in_w = text_width(ui, &in_text, 11.0);
        let out_w = text_width(ui, &out_text, 11.0);
        let proto_w = text_width(ui, proto_text, 11.0);
        let block_w = if can_block { 20.0 } else { 0.0 };
        let right_w = proto_w + in_w + out_w + block_w + 4.0 * 5.0;
        let remote_w = (ui.available_width() - right_w).max(60.0);

        // 远端:受限容器内 Truncate,拉满容器宽并绘制省略号(不溢出)
        ui.allocate_ui(egui::vec2(remote_w, 16.0), |ui| {
            ui.add(Label::new(RichText::new(remote).size(12.0).color(theme::c().text)).truncate());
        });
        // 右侧区从右往左排:阻断按钮、上传、下载、协议
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.style_mut().spacing.item_spacing.x = 6.0;
            if can_block {
                block_action(ui, rules, db, i18n, rdns, c);
            }
            ui.label(
                RichText::new(&out_text)
                    .size(11.0)
                    .color(theme::c().outbound),
            );
            ui.label(RichText::new(&in_text).size(11.0).color(theme::c().inbound));
            ui.label(theme::dim_text(proto_text, 11.0));
        });
    });
}

/// 目标级阻断开关:按命中规则分流(可撤销的 Ip 规则 / 进程级置灰 /
/// 未阻断可新建);未知进程不渲染(空进程条件会生成全局 IP 规则)
#[allow(clippy::too_many_arguments)]
fn block_action(
    ui: &mut egui::Ui,
    rules: &mut crate::rules::RuleSet,
    db: &crate::storage::history::Db,
    i18n: &crate::i18n::I18n,
    rdns: &rdns::Rdns,
    c: &Connection,
) {
    // 先求值再交互:规则借用不能跨越按钮回调里的 insert/delete
    let hit = rules
        .blocking_rule(c, rdns.lookup(c.remote_ip))
        .map(|r| (r.id, r.remote_kind == RemoteKind::Ip));
    let delete_id = match hit {
        Some((id, true)) => {
            if block_button(ui, icons::BAN, &i18n.t("map-unblock-target"), true, true).clicked() {
                Some(id)
            } else {
                None
            }
        }
        // 命中的是进程级规则:目标级按钮置灰,撤销交给组头的进程开关
        Some((_, false)) => {
            block_button(
                ui,
                icons::BAN,
                &i18n.t("map-blocked-by-process"),
                true,
                false,
            )
            .on_disabled_hover_text(i18n.t("map-blocked-by-process"));
            None
        }
        None => {
            if block_button(ui, icons::X_LG, &i18n.t("map-block-target"), false, true).clicked() {
                let _ = rules.insert(db, Rule::block(&c.process, Some(c.remote_ip)));
            }
            None
        }
    };
    if let Some(id) = delete_id {
        let _ = rules.delete(db, id);
    }
}

/// 小型图标操作按钮(阻断/撤销;danger = 已阻断语义色)
fn block_button(
    ui: &mut egui::Ui,
    glyph: &str,
    tip: &str,
    danger: bool,
    enabled: bool,
) -> egui::Response {
    let color = if danger && enabled {
        theme::c().danger
    } else {
        theme::c().text_dim
    };
    // 紧凑 padding(style.button_padding 默认 10x5 太宽)
    ui.style_mut().spacing.button_padding = egui::vec2(4.0, 2.0);
    let btn =
        Button::new(RichText::new(glyph).size(12.0).color(color)).min_size(Vec2::new(20.0, 18.0));
    ui.add_enabled(enabled, btn).on_hover_text(tip)
}
