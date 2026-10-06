//! 采集器后台回填:Authenticode 签名校验与进程图标的限流派发、结果
//! 收割与缓存回写(工作线程经 mpsc 回报);采集主流程在 windows_table。

use std::collections::HashSet;
use std::sync::Arc;

use super::windows_table::TableCollector;
use super::{IconState, signature};
use crate::model::Signing;

const SIG_MAX_INFLIGHT: usize = 4;
const SIG_DISPATCH_PER_POLL: usize = 2;
const ICON_DISPATCH_PER_POLL: usize = 4;
impl TableCollector {
    /// 收割已完成的签名查询结果回填缓存
    pub(super) fn collect_signatures(&mut self) {
        while let Ok((pid, signed)) = self.sig_rx.try_recv() {
            self.sig_pending.remove(&pid);
            if let Some(meta) = self.proc_metas.get_mut(&pid) {
                meta.signed = signed;
                if signed == Signing::Invalid {
                    tracing::debug!(
                        "[Collector] 签名校验未通过:{}({pid})",
                        meta.path.as_deref().unwrap_or("?")
                    );
                }
            }
        }
    }

    /// 收割已完成的图标提取结果
    pub(super) fn collect_icons(&mut self) {
        while let Ok((path, img)) = self.icon_rx.try_recv() {
            self.icon_inflight.remove(&path);
            if img.is_none() {
                // 失败也缓存(Ready(None)),此后不再重试,留痕供排查
                tracing::debug!("[Collector] 进程图标提取失败:{path}");
            }
            self.icons.insert(path, IconState::Ready(img.map(Arc::new)));
        }
        // 未派发的 Pending 条目(超出上轮预算的)按上限补齐
        let budget = ICON_DISPATCH_PER_POLL.saturating_sub(self.icon_inflight.len());
        for path in self
            .icons
            .iter()
            .filter(|(_, s)| matches!(s, IconState::Pending))
            .map(|(p, _)| p.clone())
            .take(budget)
            .collect::<Vec<_>>()
        {
            self.icon_inflight.insert(path.clone());
            let tx = self.icon_tx.clone();
            std::thread::spawn(move || {
                let img = super::icon::extract(&path);
                let _ = tx.send((path, img));
            });
        }
    }

    /// 为缓存中签名未知的存活进程派发校验(限流:在途/每轮数量双重上限)
    pub(super) fn dispatch_signature_queries(&mut self, live_pids: &HashSet<u32>) {
        let budget = SIG_MAX_INFLIGHT.saturating_sub(self.sig_pending.len());
        let candidates = self
            .proc_metas
            .iter()
            .filter(|(pid, meta)| {
                live_pids.contains(pid)
                    && meta.signed == Signing::Unknown
                    && meta.path.is_some()
                    && !self.sig_pending.contains(*pid)
            })
            .map(|(pid, meta)| (*pid, meta.path.clone().expect("path is some")))
            .take(budget.min(SIG_DISPATCH_PER_POLL))
            .collect::<Vec<_>>();
        for (pid, path) in candidates {
            if self.sig_pending.len() >= SIG_MAX_INFLIGHT {
                break;
            }
            self.sig_pending.insert(pid);
            let tx = self.sig_tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send((pid, signature::verify(&path)));
            });
        }
    }
}
