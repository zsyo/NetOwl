//! 历史查询与页面状态(conn_events 只读侧;写入见 super::history)。
//! 归属地不落库,渲染时实时反查 geoip;全部查询走 rusqlite 参数绑定。
//!
//! Filter 与行/排序类型在 filter,只读查询与删除在 query,时间与
//! 文本格式化在 fmt,历史页状态在 page。

mod filter;
mod fmt;
mod page;
mod query;

/// 历史库超容提醒阈值(1 GiB)
pub const REMIND_SIZE: u64 = 1024 * 1024 * 1024;
/// 明细/聚合单次查询行数上限
pub const QUERY_LIMIT: usize = 500;

pub use filter::{
    AggregateRow, AggregateSort, DetailRow, Filter, SummaryRow, SummarySort, UsageRow,
};
pub use fmt::{
    day_start, fmt_duration, fmt_local, local_tz_offset_secs, month_start, parse_ip_prefix,
};
pub use page::{PageState, Range, Rows, ViewMode};
pub use query::{
    PendingDelete, delete_aggregate_group, delete_detail, delete_process, query_month_bytes,
    query_today_bytes,
};
