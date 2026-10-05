//! 历史页汇总视图:按进程聚合的 5 列表(表头可排序,数字列右对齐),
//! 实时叠加活跃连接字节(按秒缓存合并结果),整行点击下钻明细。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eframe::egui;

use super::cells;
use crate::i18n::I18n;
use crate::model::Connection;
use crate::storage::config::Config;
use crate::storage::history_query::{self, Rows, SummaryRow, SummarySort};
use crate::ui::{conn_visible, theme, widgets};

/// 汇总视图活跃合并缓存的生存期:略短于静态页 1s 重绘间隔,
/// 保证每次常规重绘都会拿到新鲜活跃字节,交互帧复用不重算
const SUMMARY_MERGE_TTL: Duration = Duration::from_millis(900);

pub(super) fn table(
    ui: &mut egui::Ui,
    state: &mut history_query::PageState,
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    conns: &[Connection],
    config: &Config,
) {
    // 实时叠加活跃连接(口径注记见下):克隆+聚合+重排开销大,
    // 缓存于 summary_merged 按秒失效,交互帧直接复用不重算;
    // 不写回 state.rows(活跃字节逐轮增长,固化进查询缓存会
    // 污染 dirty 门控的下次重查)
    let expired = state
        .summary_merged
        .as_ref()
        .is_none_or(|(at, _)| at.elapsed() >= SUMMARY_MERGE_TTL);
    if expired {
        let mut merged = match &state.rows {
            Rows::Summary(rows) => rows.clone(),
            _ => Vec::new(),
        };
        for (process, live) in live_proc_sums(conns, config) {
            match merged.iter_mut().find(|r| r.process == process) {
                Some(r) => {
                    r.bytes_out += live.bytes_out;
                    r.bytes_in += live.bytes_in;
                    r.count += live.count;
                    r.total_secs += live.total_secs;
                }
                None => merged.push(SummaryRow {
                    process,
                    proc_path: live.proc_path,
                    bytes_out: live.bytes_out,
                    bytes_in: live.bytes_in,
                    count: live.count,
                    total_secs: live.total_secs,
                }),
            }
        }
        sort_summary(&mut merged, state.summary_sort);
        state.summary_merged = Some((Instant::now(), merged));
    }
    let Some((_, merged)) = state.summary_merged.as_ref() else {
        return;
    };
    if merged.is_empty() {
        cells::empty_hint(ui, i18n);
        return;
    }
    // 口径注记:数字含当前活跃连接的实时字节
    ui.label(theme::dim_text(
        &i18n.t("history-summary-note"),
        theme::font::SM,
    ));
    ui.add_space(theme::sp::XS);
    const SUM_UP_W: f32 = 85.0;
    const SUM_DOWN_W: f32 = 85.0;
    const SUM_CNT_W: f32 = 65.0;
    const SUM_DUR_W: f32 = 85.0;
    let table_w = ui.available_width();
    let flex_w = (table_w - (SUM_UP_W + SUM_DOWN_W + SUM_CNT_W + SUM_DUR_W)).max(240.0);
    let summary_sort = &mut state.summary_sort;
    // 表头固定在滚动区外(虚拟化行定位不含表头);全部数字列数据右对齐,
    // 表头贴右
    let mut sort_clicked = false;
    widgets::table::sort_header_grid(
        ui,
        "history_summary_header",
        &[
            widgets::table::SortCol::new("history-col-process", None::<SummarySort>, flex_w, false),
            widgets::table::SortCol::new(
                "col-up-total",
                Some(SummarySort::BytesOut),
                SUM_UP_W,
                true,
            ),
            widgets::table::SortCol::new(
                "col-down-total",
                Some(SummarySort::BytesIn),
                SUM_DOWN_W,
                true,
            ),
            widgets::table::SortCol::new(
                "history-col-count",
                Some(SummarySort::Count),
                SUM_CNT_W,
                true,
            ),
            widgets::table::SortCol::new(
                "history-col-total",
                Some(SummarySort::TotalSecs),
                SUM_DUR_W,
                true,
            ),
        ],
        Some(*summary_sort),
        |s| {
            let (cur, asc) = *summary_sort;
            *summary_sort = if cur == s { (s, !asc) } else { (s, false) };
            sort_clicked = true;
        },
        &|k| i18n.t(k),
    );
    if sort_clicked {
        state.dirty = true;
    }
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        22.0,
        merged.len(),
        |ui, row_range| {
            let table_left = ui.max_rect().left();
            let table_right = ui.max_rect().right();
            egui::Grid::new("history_summary_rows")
                .num_columns(5)
                .striped(false)
                .spacing([0.0, widgets::table::ROW_SPACING_Y])
                .show(ui, |ui| {
                    for r_idx in row_range {
                        let r = &merged[r_idx];
                        let row_top = ui.cursor().top();
                        let row_rect =
                            cells::row_background(ui, table_left, table_right, row_top, r_idx);
                        // 整行点击下钻:切明细视图并按该进程名过滤
                        // (空进程名行无法在明细中精确过滤,禁用交互)
                        if !r.process.is_empty() {
                            let mut resp = ui.interact(
                                row_rect,
                                egui::Id::new(("history_summary_row", r_idx)),
                                egui::Sense::click(),
                            );
                            if resp.clicked() {
                                state.view = history_query::ViewMode::Detail;
                                state.process = r.process.clone();
                                state.dirty = true;
                            }
                            resp = resp.on_hover_text(i18n.t("history-summary-drill"));
                            resp.context_menu(|ui| {
                                if super::menu::summary_menu(ui, r, i18n) {
                                    state.pending_delete = Some(history_query::PendingDelete {
                                        process: r.process.clone(),
                                        proto: None,
                                        remote_ip: None,
                                    });
                                }
                            });
                        }
                        widgets::table::fixed_cell(ui, flex_w, 22.0, |ui| {
                            cells::proc_cell(
                                ui,
                                &r.process,
                                None,
                                r.proc_path.as_deref(),
                                icon_tex,
                                default_icon_tex,
                                i18n,
                            );
                        });
                        widgets::table::fixed_num_cell(ui, SUM_UP_W, |ui| {
                            cells::bytes_cell(ui, r.bytes_out, true);
                        });
                        widgets::table::fixed_num_cell(ui, SUM_DOWN_W, |ui| {
                            cells::bytes_cell(ui, r.bytes_in, false);
                        });
                        widgets::table::fixed_num_cell(ui, SUM_CNT_W, |ui| {
                            widgets::table::num_cell(ui, r.count.to_string(), theme::c().text);
                        });
                        widgets::table::fixed_num_cell(ui, SUM_DUR_W, |ui| {
                            widgets::table::num_cell(
                                ui,
                                history_query::fmt_duration(r.total_secs),
                                theme::c().text,
                            );
                        });
                        ui.end_row();
                    }
                });
        },
    );
    // 行数按合并后的结果计:活跃连接并入的新进程不在 SQL 行数里
    cells::truncated_hint(ui, merged.len(), i18n);
}

/// 活跃连接按进程聚合的实时增量(叠加进汇总视图);过滤口径与连接页
/// 共用 conn_visible(本地/局域网远端隐藏与历史 SQL 侧一致)
struct LiveSum {
    bytes_out: u64,
    bytes_in: u64,
    count: u64,
    total_secs: u64,
    proc_path: Option<String>,
}

fn live_proc_sums(conns: &[Connection], config: &Config) -> HashMap<String, LiveSum> {
    let mut map: HashMap<String, LiveSum> = HashMap::new();
    for c in conns.iter().filter(|c| conn_visible(config, c)) {
        let e = map.entry(c.process.clone()).or_insert_with(|| LiveSum {
            bytes_out: 0,
            bytes_in: 0,
            count: 0,
            total_secs: 0,
            proc_path: c.proc_path.clone(),
        });
        e.bytes_out += c.bytes_out;
        e.bytes_in += c.bytes_in;
        e.count += 1;
        e.total_secs += c.first_seen.elapsed().as_secs();
    }
    map
}

/// 汇总行按当前排序键内存重排(活跃合并后次序需重算,与 SQL ORDER BY
/// 同键同方向);次级键恒为进程名,防 HashMap 迭代序不定致同值行每帧跳动
fn sort_summary(rows: &mut [SummaryRow], (sort, asc): (SummarySort, bool)) {
    let key = |r: &SummaryRow| match sort {
        SummarySort::BytesOut => r.bytes_out,
        SummarySort::BytesIn => r.bytes_in,
        SummarySort::Count => r.count,
        SummarySort::TotalSecs => r.total_secs,
    };
    rows.sort_by(|a, b| {
        let ord = key(a).cmp(&key(b));
        let ord = if asc { ord } else { ord.reverse() };
        ord.then_with(|| a.process.cmp(&b.process))
    });
}
