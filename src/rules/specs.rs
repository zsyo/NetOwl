//! 启用规则 -> WFP 过滤器目标集合翻译:weight 布局、进程规则按映像名
//! 展开为命中过的完整路径集合(粘滞缓存),方向 Any 拆 Out/In 两层。
//! 拦截执行引擎见 super::wfp。

use super::{Action, Direction, RemoteKind, Rule, RuleSet};
use crate::model::{Connection, Protocol};

/// WFP 的 TCP/UDP 协议号(IPPROTO)
const PROTO_TCP: u8 = 6;
const PROTO_UDP: u8 = 17;
/// 过滤器 weight 上限(FWP_UINT8 有效范围 0..=15)
const MAX_WEIGHT: usize = 15;

/// 预留最高 weight:询问 pending 阻断与静默自身放行(二者互斥,静默拒绝
/// 模式下询问关闭,pending 不存在)
pub const WEIGHT_RESERVED_HIGH: u8 = MAX_WEIGHT as u8;
/// 静默兜底阻断 weight(低于全部用户规则,与子层基线同级)
pub const WEIGHT_FALLBACK: u8 = 0;

impl RuleSet {
    /// 把启用规则翻译为 WFP 过滤器目标集合(语义见 wfp::Spec;域名规则
    /// 不参与翻译)。与求值引擎一致:优先级高者 weight 大。进程规则按
    /// 映像名展开为命中过的完整路径集合(粘滞缓存,见字段注释)
    pub fn wfp_specs(&mut self, conns: &[Connection]) -> Vec<super::wfp::Spec> {
        let mut applicable: Vec<&Rule> = self
            .rules
            .iter()
            .filter(|r| r.enabled && r.remote_kind != RemoteKind::Domain)
            .collect();
        applicable.sort_by_key(|r| (r.priority, r.id));
        // WFP 对同 weight 的冲突过滤器选择未定义:用户规则 weight 布局
        // 1..=14(0 = 静默兜底,15 = 询问 pending/自身放行),启用规则超过
        // 上限时截断——求值标注不受影响,仅内核执行止于前 14 条
        if applicable.len() > MAX_WEIGHT - 1 {
            tracing::warn!(
                "[Rules] 启用规则 {} 条超过 WFP 执行上限 {},超出部分仅求值标注、不参与内核拦截",
                applicable.len(),
                MAX_WEIGHT - 1
            );
            applicable.truncate(MAX_WEIGHT - 1);
        }

        // 本轮连接完整路径的小写化预计算(O(conns) 一次),供全部进程规则复用
        let conn_paths: Vec<(&str, String)> = conns
            .iter()
            .filter_map(|c| c.proc_path.as_ref().map(|p| (p.as_str(), p.to_lowercase())))
            .collect();

        let mut specs = Vec::new();
        for (rank, r) in applicable.iter().enumerate() {
            // weight 布局:15 = 询问 pending / 静默自身放行(WEIGHT_RESERVED_HIGH),
            // 1..=14 = 用户规则,0 = 静默兜底阻断(WEIGHT_FALLBACK,与子层基线
            // 同级);用户规则超过 14 条后钳制到 1,恒高于兜底,避免同 weight
            // 时 WFP 动作未定义
            let weight = (MAX_WEIGHT - 1 - rank.min(MAX_WEIGHT - 2)) as u8;
            let remote = match r.remote_kind {
                RemoteKind::Any => None,
                // 网段区间取预计算缓存;None = 非法值,与求值一致跳过
                RemoteKind::Ip => match self.eval_cache.get(&r.id).and_then(|m| m.remote_range) {
                    Some(v) => Some(v),
                    None => continue,
                },
                RemoteKind::Domain => unreachable!("filtered above"),
            };
            let paths: Vec<Option<String>> = if r.process.is_empty() {
                self.sticky_paths.remove(&r.id);
                vec![None]
            } else {
                if let Some(m) = self.eval_cache.get(&r.id)
                    && !m.process_lower.is_empty()
                {
                    let known = self.sticky_paths.entry(r.id).or_default();
                    for (p, lp) in &conn_paths {
                        if lp == &m.process_lower || super::ends_with_path(lp, &m.process_lower) {
                            known.insert((*p).to_owned());
                        }
                    }
                }
                self.sticky_paths
                    .get(&r.id)
                    .map(|s| s.iter().cloned().map(Some).collect())
                    .unwrap_or_default()
            };
            let proto = r.proto.map(|p| match p {
                Protocol::Tcp => PROTO_TCP,
                Protocol::Udp => PROTO_UDP,
            });
            let port = (r.port != 0).then_some(r.port);
            let layers: &[super::wfp::Layer] = match r.direction {
                Direction::Any => &[super::wfp::Layer::Out, super::wfp::Layer::In],
                Direction::Out => &[super::wfp::Layer::Out],
                Direction::In => &[super::wfp::Layer::In],
            };
            for path in &paths {
                for &layer in layers {
                    specs.push(super::wfp::Spec {
                        layer,
                        weight,
                        block: r.action == Action::Block,
                        app_path: path.clone(),
                        remote,
                        proto,
                        port,
                    });
                }
            }
        }
        specs
    }
}
