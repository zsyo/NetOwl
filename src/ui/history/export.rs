//! 历史页 CSV 导出:按当前视图与筛选导出全部结果(UTF-8 BOM,Excel
//! 直开中文不乱码),列结构与页面一致,位置列实时反查。

use super::usage;
use crate::i18n::I18n;
use crate::model::Place;
use crate::net::geoip;
use crate::storage::history_query::{self, Rows};

/// CSV 单元格转义:含逗号/引号/换行的字段加引号包裹,内部引号翻倍
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

/// 位置列文本(与页面 location_cell 同口径):GeoIP 反查,未知占位
fn location_text(i18n: &I18n, ip: std::net::Ipv4Addr) -> String {
    match geoip::locate(ip).map(Place::Geo) {
        Some(place) => geoip::place_label(place, i18n),
        None => i18n.t("conn-loc-unknown"),
    }
}

/// 导出当前视图的查询结果为 CSV(UTF-8 BOM,Excel 直开中文不乱码);
/// 列结构与页面一致(位置列实时反查)。汇总视图带活跃连接实时合并
/// (与页面同口径,缓存过期时含近期数据)。
/// 保存对话框与规则页导入导出同为 rfd 模态,取消即不做任何写入
pub(super) fn export_csv(state: &history_query::PageState, i18n: &I18n) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("CSV", &["csv"])
        .set_file_name("netowl-history.csv")
        .save_file()
    else {
        return;
    };
    let rows: Vec<Vec<String>> = match &state.rows {
        Rows::Detail(rows) => {
            let mut out = vec![
                [
                    "history-col-process",
                    "col-proto",
                    "col-remote",
                    "col-location",
                    "history-col-first",
                    "history-col-duration",
                    "col-down-total",
                    "col-up-total",
                ]
                .iter()
                .map(|k| i18n.t(k))
                .collect(),
            ];
            for r in rows {
                out.push(vec![
                    r.process.clone(),
                    r.proto.as_str().to_owned(),
                    format!("{}:{}", r.remote_ip, r.remote_port),
                    location_text(i18n, r.remote_ip),
                    history_query::fmt_local(r.first_seen),
                    history_query::fmt_duration(r.last_seen.saturating_sub(r.first_seen)),
                    r.bytes_in.to_string(),
                    r.bytes_out.to_string(),
                ]);
            }
            out
        }
        Rows::Aggregate(rows) => {
            let mut out = vec![
                [
                    "history-col-process",
                    "col-proto",
                    "col-remote",
                    "col-location",
                    "history-col-count",
                    "history-col-total",
                    "history-col-last",
                    "col-down-total",
                    "col-up-total",
                ]
                .iter()
                .map(|k| i18n.t(k))
                .collect(),
            ];
            for r in rows {
                out.push(vec![
                    r.process.clone(),
                    r.proto.as_str().to_owned(),
                    r.remote_ip.to_string(),
                    location_text(i18n, r.remote_ip),
                    r.count.to_string(),
                    history_query::fmt_duration(r.total_secs),
                    history_query::fmt_local(r.last_active),
                    r.bytes_in.to_string(),
                    r.bytes_out.to_string(),
                ]);
            }
            out
        }
        Rows::Summary(_) => {
            let merged = state
                .summary_merged
                .as_ref()
                .map(|(_, m)| m.clone())
                .unwrap_or_default();
            let mut out = vec![
                [
                    "history-col-process",
                    "col-up-total",
                    "col-down-total",
                    "history-col-count",
                    "history-col-total",
                ]
                .iter()
                .map(|k| i18n.t(k))
                .collect(),
            ];
            for r in merged {
                out.push(vec![
                    r.process,
                    r.bytes_out.to_string(),
                    r.bytes_in.to_string(),
                    r.count.to_string(),
                    history_query::fmt_duration(r.total_secs),
                ]);
            }
            out
        }
        Rows::Usage(rows) => {
            let mut out = vec![
                ["history-col-bucket", "col-down-total", "col-up-total"]
                    .iter()
                    .map(|k| i18n.t(k))
                    .collect(),
            ];
            for r in rows {
                out.push(vec![
                    usage::usage_label(r.bucket_start, state.usage_bucket),
                    r.bytes_in.to_string(),
                    r.bytes_out.to_string(),
                ]);
            }
            out
        }
    };
    let mut text = String::from("\u{feff}");
    for row in rows {
        text.push_str(
            &row.iter()
                .map(|f| csv_field(f))
                .collect::<Vec<_>>()
                .join(","),
        );
        text.push('\n');
    }
    match std::fs::write(&path, text) {
        Ok(()) => tracing::info!("[History] 已导出 CSV 到 {}", path.display()),
        Err(e) => tracing::warn!("[History] 导出失败 {}: {e}", path.display()),
    }
}
