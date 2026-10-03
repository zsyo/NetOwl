//! 历史页用量视图:按天/按小时分桶的收发字节柱状图(bar_chart 双序列),
//! 未提权全零时提示不渲染空图。

use eframe::egui;

use super::cells;
use crate::i18n::I18n;
use crate::storage::history_query::{self, Rows};
use crate::ui::{icons, theme, widgets};

pub(super) fn table(
    ui: &mut egui::Ui,
    state: &mut history_query::PageState,
    i18n: &I18n,
    elevated: bool,
) {
    let Rows::Usage(rows) = &state.rows else {
        return;
    };
    // 分桶粒度切换:按天/按小时(写回 usage_bucket,切换即重查)
    ui.horizontal(|ui| {
        let items = [
            (&*i18n.t("history-usage-daily"), icons::CALENDAR_WEEK),
            (&*i18n.t("history-usage-hourly"), icons::CLOCK_HISTORY),
        ];
        let current = usize::from(state.usage_bucket != 86400);
        if let Some(i) = widgets::segmented::segmented(ui, &items, current) {
            state.usage_bucket = if i == 0 { 86400 } else { 3600 };
            state.dirty = true;
        }
    });
    ui.add_space(theme::sp::SM);
    if rows.is_empty() {
        cells::empty_hint(ui, i18n);
        return;
    }
    // 未提权无 ETW 字节,柱图全零:提示而非渲染空图
    if !elevated && rows.iter().all(|r| r.bytes_in == 0 && r.bytes_out == 0) {
        ui.label(theme::dim_text(
            &i18n.t("history-usage-no-etw"),
            theme::font::BODY,
        ));
        return;
    }
    let bars: Vec<widgets::bar_chart::UsageBar> = rows
        .iter()
        .map(|r| widgets::bar_chart::UsageBar {
            label: usage_label(r.bucket_start, state.usage_bucket),
            down: r.bytes_in,
            up: r.bytes_out,
        })
        .collect();
    // 图表占满面板剩余高度(数据可视化面积随窗口伸缩,不留下方空白)
    let height = ui.available_height().max(200.0);
    widgets::bar_chart::bar_chart(ui, &bars, egui::vec2(ui.available_width(), height));
}

/// 用量桶标签:按天取 "MM-DD",按小时取 "MM-DD HH"(fmt_local 输出
/// "MM-DD HH:MM:SS",ASCII 字段按字节切片安全)
pub(super) fn usage_label(bucket_start: u64, bucket_secs: u64) -> String {
    let text = history_query::fmt_local(bucket_start);
    let len = if bucket_secs == 86400 { 5 } else { 11 };
    text.chars().take(len).collect()
}
