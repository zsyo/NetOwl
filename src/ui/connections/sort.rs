//! 连接列表表头排序:文本键大小写不敏感,未知归属(无定位)恒排在
//! 有位置连接之后。

use std::collections::HashMap;

use super::ConnSortState;
use crate::i18n::I18n;
use crate::model::Connection;
use crate::net::geoip;
use crate::ui::ConnSort;

/// 按表头排序状态排列连接(None = 表快照原序)
pub(super) fn sort_conns(
    shown: &mut [&Connection],
    sort: &ConnSortState,
    conn_rates: &HashMap<u64, (u64, u64)>,
    i18n: &I18n,
) {
    use std::cmp::Ordering;
    let Some((key, asc)) = *sort else {
        return;
    };
    let flip = |c: Ordering| if asc { c } else { c.reverse() };
    let rate = |id: u64, up: bool| match conn_rates.get(&id) {
        Some(r) => {
            if up {
                r.1
            } else {
                r.0
            }
        }
        None => 0,
    };
    shown.sort_unstable_by(|a, b| match key {
        ConnSort::Process => flip(a.process.to_lowercase().cmp(&b.process.to_lowercase())),
        ConnSort::Location => match (a.city, b.city) {
            (Some(x), Some(y)) => {
                flip(geoip::place_label(x, i18n).cmp(&geoip::place_label(y, i18n)))
            }
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        },
        ConnSort::RateDown => flip(rate(a.id, false).cmp(&rate(b.id, false))),
        ConnSort::RateUp => flip(rate(a.id, true).cmp(&rate(b.id, true))),
        ConnSort::TotalDown => flip(a.bytes_in.cmp(&b.bytes_in)),
        ConnSort::TotalUp => flip(a.bytes_out.cmp(&b.bytes_out)),
    });
}
