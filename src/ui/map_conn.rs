//! 地图面板连接明细行(左列表与右侧 Inspector 共用)。
//! 阻断 = 创建永久 Block 规则,开关式操作(迷你滑动开关,开启 = 阻断中);
//! 已被进程级规则阻断的连接开关置灰(撤销进程规则会放大放行范围,不做)。
//!
//! 行布局:缩进 + 远端(Truncate 自适应省略)+ 右侧区(协议、下载/上传
//! 分色字节[带方向箭头]、目标级阻断开关,从右往左排,字节天然右对齐)。
//! 行底画 1px 分隔线(LS 式表格行界)。Truncate 控件会请求拉满剩余宽,
//! 必须放进固定宽度容器且容器宽扣除同行其余控件,否则溢出部分经
//! resizable 面板的宽度记忆逐帧放大(见 map_inspector 模块注释)。

use eframe::egui;
use egui::Label;
use egui::RichText;

use crate::model::{Connection, fmt_bytes};
use crate::net::rdns;
use crate::rules::{RemoteKind, Rule};
use crate::ui::icons;
use crate::ui::text_width;
use crate::ui::theme;
use crate::ui::widgets;

/// 迷你阻断开关尺寸
const SWITCH_W: f32 = 26.0;
const SWITCH_H: f32 = 15.0;

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

        // 右侧固定预算:协议 + 双向字节(含方向箭头)+ 阻断开关与行内间距
        let arrow_w = text_width(ui, icons::ARROW_DOWN, theme::font::XS);
        let in_w = text_width(ui, &in_text, theme::font::XS) + arrow_w;
        let out_w = text_width(ui, &out_text, theme::font::XS) + arrow_w;
        let proto_w = text_width(ui, proto_text, theme::font::XS);
        let block_w = if can_block { SWITCH_W } else { 0.0 };
        let right_w = proto_w + in_w + out_w + block_w + 4.0 * 6.0;
        let remote_w = (ui.available_width() - right_w).max(60.0);

        // 远端:受限容器内 Truncate,拉满容器宽并绘制省略号(不溢出)
        ui.allocate_ui(egui::vec2(remote_w, 22.0), |ui| {
            ui.add(
                Label::new(
                    RichText::new(remote)
                        .size(theme::font::SM)
                        .color(theme::c().text),
                )
                .truncate(),
            );
        });
        // 右侧区从右往左排:阻断开关、上传数值+箭头、下载数值+箭头、协议
        // (先加的靠右;数值先于箭头,视觉上箭头在数值左侧)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.style_mut().spacing.item_spacing.x = 6.0;
            if can_block {
                block_action(ui, rules, db, i18n, rdns, c);
            }
            ui.label(
                RichText::new(&out_text)
                    .size(theme::font::XS)
                    .color(theme::c().outbound),
            );
            ui.label(
                RichText::new(icons::ARROW_UP)
                    .size(theme::font::XS)
                    .color(theme::c().outbound),
            );
            ui.label(
                RichText::new(&in_text)
                    .size(theme::font::XS)
                    .color(theme::c().inbound),
            );
            ui.label(
                RichText::new(icons::ARROW_DOWN)
                    .size(theme::font::XS)
                    .color(theme::c().inbound),
            );
            ui.label(theme::dim_text(proto_text, theme::font::XS));
        });
    });
    ui.add_space(theme::sp::XS);
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
    // 先求值再交互:规则借用不能跨越开关回调里的 insert/delete
    let hit = rules
        .blocking_rule(c, rdns.lookup(c.remote_ip))
        .map(|r| (r.id, r.remote_kind == RemoteKind::Ip));
    match hit {
        // 可撤销的目标级规则:开关开启(红 = 阻断中),关 = 撤销
        Some((id, true)) => {
            let mut on = true;
            let resp = widgets::toggle::block_switch(
                ui,
                &mut on,
                true,
                SWITCH_W,
                SWITCH_H,
                egui::Id::new(("conn-block-toggle", c.id)),
            );
            if resp.changed() && !on {
                let _ = rules.delete(db, id);
            }
            resp.on_hover_text(i18n.t("map-unblock-target"));
        }
        // 命中的是进程级规则:目标级开关置灰,撤销交给组头的进程开关
        Some((_, false)) => {
            let mut on = true;
            widgets::toggle::block_switch(
                ui,
                &mut on,
                false,
                SWITCH_W,
                SWITCH_H,
                egui::Id::new(("conn-block-toggle", c.id)),
            )
            .on_hover_text(i18n.t("map-blocked-by-process"));
        }
        None => {
            let mut on = false;
            let resp = widgets::toggle::block_switch(
                ui,
                &mut on,
                true,
                SWITCH_W,
                SWITCH_H,
                egui::Id::new(("conn-block-toggle", c.id)),
            );
            if resp.changed() && on {
                let _ = rules.insert(db, Rule::block(&c.process, Some(c.remote_ip)));
            }
            resp.on_hover_text(i18n.t("map-block-target"));
        }
    }
}
