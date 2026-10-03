//! Inspector 进程详情视图:图标与名称、流量卡、路径、签名状态、
//! 进程级阻断开关与连接明细(范围跟随当前端点选中)。

use std::collections::HashMap;

use eframe::egui;
use egui::{Label, RichText};

use super::title_row;
use crate::i18n::I18n;
use crate::model::{Connection, Signing};
use crate::net::rdns;
use crate::storage::config::Config;
use crate::storage::history;
use crate::ui::conn_visible;
use crate::ui::map_conn::conn_row;
use crate::ui::map_panel::MapPanelState;
use crate::ui::map_widgets::{section_title, traffic_cards};
use crate::ui::theme;
use crate::ui::widgets;

/// 进程详情:图标与名称、双向流量、路径、签名、阻断开关与连接明细。
/// 连接范围跟随当前端点选中(从端点进入时仅显示该端点下的连接)
#[allow(clippy::too_many_arguments)]
pub(super) fn view(
    ui: &mut egui::Ui,
    panels: &mut MapPanelState,
    rules: &mut crate::rules::RuleSet,
    db: &history::Db,
    i18n: &I18n,
    rdns: &rdns::Rdns,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    config: &Config,
    conns: &[Connection],
    name: &str,
) {
    let rows: Vec<&Connection> = conns
        .iter()
        .filter(|c| c.process == name)
        .filter(|c| conn_visible(config, c))
        .filter(|c| panels.place.is_none_or(|p| c.city == Some(p)))
        .collect();
    let path = rows.iter().find_map(|c| c.proc_path.clone());
    let display = if name.is_empty() {
        i18n.t("conn-proc-unknown")
    } else {
        name.to_owned()
    };
    let tex = path
        .as_deref()
        .and_then(|p| icon_tex.get(p))
        .and_then(|t| t.as_ref())
        .or(default_icon_tex);
    if title_row(ui, &display, tex, i18n.t("map-inspector-clear")) {
        panels.process = None;
        return;
    }
    ui.label(theme::dim_text(
        &i18n.t_with_args("status-conn-count", &[("count", rows.len().to_string())]),
        12.0,
    ));
    traffic_cards(
        ui,
        rows.iter().map(|c| c.bytes_in).sum(),
        rows.iter().map(|c| c.bytes_out).sum(),
        i18n,
    );

    // 进程信息:完整路径可换行;签名取已知状态中优先的一条(语义色圆点)
    if let Some(p) = &path {
        ui.add(
            Label::new(
                RichText::new(format!("{}: {}", i18n.t("map-inspector-path"), p))
                    .size(theme::font::XS)
                    .color(theme::c().text_dim),
            )
            .wrap(),
        );
    }
    let signed = rows
        .iter()
        .map(|c| c.signed)
        .find(|s| *s != Signing::Unknown)
        .unwrap_or(Signing::Unknown);
    let (dot, key) = match signed {
        Signing::Signed => (theme::c().status_ok, "proc-signed"),
        Signing::Unsigned => (theme::c().status_warn, "proc-unsigned"),
        Signing::Invalid => (theme::c().danger, "proc-sign-invalid"),
        Signing::Unknown => (theme::c().text_dim, "proc-sign-unknown"),
    };
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 5.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 3.0, dot);
        ui.label(theme::dim_text(&i18n.t(key), theme::font::XS));
    });

    // 进程级阻断开关(off 绿 = 放行 / on 红 = 阻断,未知进程不可阻断,
    // 避免空进程条件生成全局规则)
    if !name.is_empty() {
        ui.add_space(2.0);
        let existing = rules
            .process_block_rule(name, path.as_deref())
            .map(|r| r.id);
        let mut on = existing.is_some();
        let resp = widgets::toggle::block_switch(
            ui,
            &mut on,
            true,
            36.0,
            20.0,
            egui::Id::new(("inspector-block-toggle", name.to_owned())),
        );
        if resp.changed() {
            match (on, existing) {
                (true, None) => {
                    if let Err(e) = rules.insert(db, crate::rules::Rule::block(name, None)) {
                        tracing::warn!("[Map] 进程 {name} 阻断规则写入失败: {e}");
                    }
                }
                (false, Some(id)) => {
                    if let Err(e) = rules.delete(db, id) {
                        tracing::warn!("[Map] 进程 {name} 阻断规则(id {id})删除失败: {e}");
                    }
                }
                _ => {}
            }
        }
        ui.label(theme::dim_text(
            &i18n.t(if on {
                "map-unblock-process"
            } else {
                "map-block-process"
            }),
            theme::font::SM,
        ));
    }

    section_title(ui, &i18n.t("map-inspector-conns"));
    for c in &rows {
        conn_row(ui, rules, db, i18n, rdns, c, None);
    }
}
