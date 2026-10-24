//! 连接页派生缓存:过滤/排序/分组的重建结果按(连接快照代次, 排序/
//! 筛选/分组设置)门控。30fps 重绘帧内 match_search 的每行多次小写化、
//! 排序键重建(含归属查表与 fluent 查询)与分组 HashMap 重建是千级
//! 连接下连接页的主要帧成本;重建只在 1s 采集轮或用户改设置时发生,
//! 交互帧(悬停/滚动/动画)直接复用缓存。

use std::collections::HashMap;

use super::ConnSortState;
use super::group;
use super::sort;
use crate::i18n::I18n;
use crate::model::Connection;
use crate::net::rdns;
use crate::storage::config::Config;
use crate::ui::conn_visible;

/// 连接页列表缓存(结构见模块注释);App 持有,每帧经 UiCtx 传入
pub struct ConnListCache {
    /// 缓存对应的连接快照代次(u64::MAX = 从未构建,首帧必重建)
    snapshot_gen: u64,
    sort: ConnSortState,
    /// 筛选状态快照(搜索词已 trim + 小写化)
    needle: String,
    proto: Option<crate::model::Protocol>,
    hide_local: bool,
    hide_lan: bool,
    grouped: bool,
    /// 重建结果:扁平行列表(分组头/连接行交错)
    flat: Vec<FlatEntry>,
}

impl ConnListCache {
    pub fn new() -> Self {
        ConnListCache {
            snapshot_gen: u64::MAX,
            sort: None,
            needle: String::new(),
            proto: None,
            hide_local: false,
            hide_lan: false,
            grouped: false,
            flat: Vec::new(),
        }
    }

    /// 筛选/排序/分组/折叠设置或快照代次变化时重建,否则复用。
    /// 折叠切换也走重建:点击一次的重建成本(过滤+排序+分组,毫秒级)
    /// 远低于维护两套行视图的复杂度
    #[allow(clippy::too_many_arguments)]
    pub fn ensure_fresh(
        &mut self,
        conns: &[Connection],
        snapshot_gen: u64,
        sort: ConnSortState,
        needle: &str,
        proto: Option<crate::model::Protocol>,
        config: &Config,
        grouped: bool,
        collapsed: &std::collections::HashSet<String>,
        rates: &HashMap<u64, (u64, u64)>,
        i18n: &I18n,
        rdns: &rdns::Rdns,
    ) {
        if self.snapshot_gen == snapshot_gen
            && self.sort == sort
            && self.needle == needle
            && self.proto == proto
            && self.hide_local == config.general.hide_local
            && self.hide_lan == config.general.hide_lan
            && self.grouped == grouped
        {
            return;
        }
        let hide_local = config.general.hide_local;
        let hide_lan = config.general.hide_lan;
        // 过滤:显示口径(本地/局域网)+ 协议 + 搜索词(进程名/映像路径/
        // 远端 IP/rDNS 域名包含,大小写不敏感),收集为快照索引
        let mut order: Vec<usize> = conns
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                conn_visible(config, c)
                    && proto.is_none_or(|p| c.proto == p)
                    && (needle.is_empty() || match_search(c, needle, rdns))
            })
            .map(|(i, _)| i)
            .collect();
        sort::sort_indices(&mut order, conns, &sort, rates, i18n);

        let mut flat: Vec<FlatEntry> = Vec::with_capacity(order.len() + 8);
        if grouped {
            // 按进程分组:按进程名合并全部连接(不能只合并连续同名行——
            // order 按用户排序键排列,默认快照序下同进程连接可能被拆断,
            // 连续合并会产生同名分组头,组头 interact 的 ID(进程名)相撞),
            // 组序保持首现顺序
            let mut index: HashMap<&str, usize> = HashMap::new();
            let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
            for &i in &order {
                let c = &conns[i];
                let idx = *index.entry(c.process.as_str()).or_insert_with(|| {
                    groups.push((c.process.clone(), Vec::new()));
                    groups.len() - 1
                });
                groups[idx].1.push(i);
            }
            for (name, members) in groups {
                let (bytes_out, bytes_in) = members.iter().fold((0u64, 0u64), |a, &i| {
                    (a.0 + conns[i].bytes_out, a.1 + conns[i].bytes_in)
                });
                flat.push(FlatEntry::Header(group::HeaderData {
                    name: name.clone(),
                    proc_path: members.iter().find_map(|&i| conns[i].proc_path.clone()),
                    count: members.len(),
                    bytes_in,
                    bytes_out,
                }));
                // 组头行点击折叠/展开(状态键 = 进程名,会话态):折叠组
                // 跳过成员行
                if !collapsed.contains(&name) {
                    flat.extend(members.into_iter().map(FlatEntry::Conn));
                }
            }
        } else {
            flat.extend(order.into_iter().map(FlatEntry::Conn));
        }
        *self = ConnListCache {
            snapshot_gen,
            sort,
            needle: needle.to_owned(),
            proto,
            hide_local,
            hide_lan,
            grouped,
            flat,
        };
    }

    /// 重建结果行数(空表判定与虚拟化总行数)
    pub(super) fn len(&self) -> usize {
        self.flat.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.flat.is_empty()
    }

    /// 行内容(虚拟化按可见区间取行)
    pub(super) fn row(&self, idx: usize) -> &FlatEntry {
        &self.flat[idx]
    }
}

/// 扁平行:连接行(快照索引)或分组头(聚合数据)
pub(super) enum FlatEntry {
    Conn(usize),
    Header(group::HeaderData),
}

/// 搜索词匹配(与缓存重建同口径;进程名/映像路径/远端 IP/端口/协议/
/// rDNS 域名包含,大小写不敏感)
fn match_search(c: &Connection, needle: &str, rdns: &rdns::Rdns) -> bool {
    c.process.to_lowercase().contains(needle)
        || c.proc_path
            .as_deref()
            .is_some_and(|p| p.to_lowercase().contains(needle))
        || c.remote_ip.to_string().contains(needle)
        || c.remote_port.to_string().contains(needle)
        || c.proto.as_str().to_lowercase().contains(needle)
        || rdns
            .lookup(c.remote_ip)
            .is_some_and(|d| d.to_lowercase().contains(needle))
}
