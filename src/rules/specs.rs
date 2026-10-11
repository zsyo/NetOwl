//! 启用规则 -> WFP 过滤器目标集合翻译:weight 布局、进程规则按映像名
//! 展开为命中过的完整路径集合(粘滞缓存)、域名规则按解析 IP 集展开
//! (一 IP 一过滤器),方向 Any 拆 Out/In 两层。拦截执行引擎见 super::wfp。

use super::wfp::RemoteNet;
use super::{Action, Direction, NetRange, RemoteKind, Rule, RuleSet};
use crate::model::{Connection, Protocol};

/// WFP 的 TCP/UDP 协议号(IPPROTO)
const PROTO_TCP: u8 = 6;
const PROTO_UDP: u8 = 17;
/// 过滤器 weight 上限(FWP_UINT8 有效范围 0..=15)
const MAX_WEIGHT: usize = 15;
/// 单条域名规则展开的 IP 数上限:CDN 域名可解析到数十个地址,超限时
/// 取数值最小的若干个(BTreeSet 序),防过滤器数量与 reconcile 开销失控
const DOMAIN_IP_LIMIT: usize = 64;

/// 预留最高 weight:询问 pending 阻断与静默自身放行(二者互斥,静默拒绝
/// 模式下询问关闭,pending 不存在)
pub const WEIGHT_RESERVED_HIGH: u8 = MAX_WEIGHT as u8;
/// 静默兜底阻断 weight(低于全部用户规则,与子层基线同级)
pub const WEIGHT_FALLBACK: u8 = 0;

impl RuleSet {
    /// 把启用规则翻译为 WFP 过滤器目标集合(语义见 wfp::Spec)。与求值
    /// 引擎一致:优先级高者 weight 大。进程规则按映像名展开为命中过的
    /// 完整路径集合(粘滞缓存,见字段注释);域名规则按解析 IP 集展开
    /// 为单 IP 过滤器(尚未解析/无 A 记录时本轮不产 spec,解析完成后
    /// 自动纳入)
    pub fn wfp_specs(&mut self, conns: &[Connection]) -> Vec<super::wfp::Spec> {
        let mut applicable: Vec<&Rule> = self.rules.iter().filter(|r| r.enabled).collect();
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
            // 远端条件展开为区间列表:网段 -> 单条件,域名 -> 解析 IP 集
            //(一 IP 一条件),None = 不限远端;列表为空 = 无可用条件,跳过
            let remotes: Vec<Option<RemoteNet>> = match r.remote_kind {
                RemoteKind::Any => vec![None],
                // 网段条件取预计算缓存(v4 区间 / v6 前缀);None = 非法值,
                // 与求值一致跳过
                RemoteKind::Ip => match self.eval_cache.get(&r.id).and_then(|m| m.remote_range) {
                    Some(NetRange::V4(lo, hi)) => vec![Some(RemoteNet::V4(lo, hi))],
                    Some(NetRange::V6 { addr, prefix }) => {
                        vec![Some(RemoteNet::V6 { addr, prefix })]
                    }
                    None => continue,
                },
                // 域名解析首期仅 A 记录(v4),AAAA 随 v6 解析扩展
                RemoteKind::Domain => match self.domain_ips.get(&r.id) {
                    Some(ips) => ips
                        .iter()
                        .take(DOMAIN_IP_LIMIT)
                        .map(|ip| Some(RemoteNet::V4(*ip, *ip)))
                        .collect(),
                    None => continue,
                },
            };
            if remotes.is_empty() {
                continue;
            }
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
            // 层按远端条件的地址族分派:网段/单 IP 条件落到对应族;
            // 不限远端(进程/端口/协议规则)在 v4/v6 四层同时生效,
            // 否则另一族流量不受该规则约束
            let layers: &[super::wfp::Layer] =
                if remotes.iter().any(|r| r.is_some_and(|n| n.is_v6())) {
                    match r.direction {
                        Direction::Any => &[super::wfp::Layer::Out6, super::wfp::Layer::In6],
                        Direction::Out => &[super::wfp::Layer::Out6],
                        Direction::In => &[super::wfp::Layer::In6],
                    }
                } else if remotes.iter().all(|r| r.is_none()) {
                    match r.direction {
                        Direction::Any => &[
                            super::wfp::Layer::Out,
                            super::wfp::Layer::In,
                            super::wfp::Layer::Out6,
                            super::wfp::Layer::In6,
                        ],
                        Direction::Out => &[super::wfp::Layer::Out, super::wfp::Layer::Out6],
                        Direction::In => &[super::wfp::Layer::In, super::wfp::Layer::In6],
                    }
                } else {
                    match r.direction {
                        Direction::Any => &[super::wfp::Layer::Out, super::wfp::Layer::In],
                        Direction::Out => &[super::wfp::Layer::Out],
                        Direction::In => &[super::wfp::Layer::In],
                    }
                };
            for remote in &remotes {
                for path in &paths {
                    for &layer in layers {
                        specs.push(super::wfp::Spec {
                            layer,
                            weight,
                            block: r.action == Action::Block,
                            app_path: path.clone(),
                            remote: *remote,
                            proto,
                            port,
                        });
                    }
                }
            }
        }
        specs
    }

    /// 域名解析 IP 回填(采集频率调用):对启用中的域名规则,把 Resolver
    /// 已解析的 A 记录写入 domain_ips,下一轮 wfp_specs 自然纳入展开。
    /// 键小写化,与 Resolver 缓存键一致;规则停用期间旧值保留(重新启用
    /// 后立即生效,重解析完成后自动刷新)
    pub fn sync_domain_ips(&mut self, resolve: &crate::net::resolve::Resolver) {
        for r in &self.rules {
            if !r.enabled || r.remote_kind != RemoteKind::Domain {
                continue;
            }
            let Some(ips) = resolve.lookup(r.remote_value.trim()) else {
                continue;
            };
            let set: std::collections::BTreeSet<u32> =
                ips.iter().map(|ip| u32::from(*ip)).collect();
            self.domain_ips.insert(r.id, set);
        }
    }
}
