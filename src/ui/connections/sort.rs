//! 连接列表表头排序:文本键大小写不敏感,未知归属(无定位)恒排在
//! 有位置连接之后。排序键先逐行预计算(每帧一次)再排序,免每次比较
//! 重复小写化与归属查表(文本比较次数 O(n log n))。

use std::collections::HashMap;

use super::ConnSortState;
use crate::i18n::I18n;
use crate::model::Connection;
use crate::net::geoip;
use crate::ui::ConnSort;

/// 预计算后的排序键(同轮排序键类型一致)
enum SortKey {
    Text(String),
    /// 归属显示名;None = 未知归属(排序恒靠后)
    Loc(Option<String>),
    Num(u64),
}

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
    let key_of = |c: &Connection| match key {
        ConnSort::Process => SortKey::Text(c.process.to_lowercase()),
        ConnSort::Location => SortKey::Loc(c.city.map(|p| geoip::place_label(p, i18n))),
        ConnSort::RateDown => SortKey::Num(rate(c.id, false)),
        ConnSort::RateUp => SortKey::Num(rate(c.id, true)),
        ConnSort::TotalDown => SortKey::Num(c.bytes_in),
        ConnSort::TotalUp => SortKey::Num(c.bytes_out),
    };
    let cmp = |a: &SortKey, b: &SortKey| -> Ordering {
        match (a, b) {
            (SortKey::Text(x), SortKey::Text(y)) => flip(x.cmp(y)),
            (SortKey::Loc(x), SortKey::Loc(y)) => match (x, y) {
                (Some(x), Some(y)) => flip(x.cmp(y)),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            },
            (SortKey::Num(x), SortKey::Num(y)) => flip(x.cmp(y)),
            // 同轮排序键类型一致,混合不可达
            _ => Ordering::Equal,
        }
    };
    let mut keyed: Vec<(SortKey, &Connection)> = shown.iter().map(|c| (key_of(c), *c)).collect();
    keyed.sort_unstable_by(|a, b| cmp(&a.0, &b.0));
    for (dst, (_, c)) in shown.iter_mut().zip(keyed) {
        *dst = c;
    }
}
