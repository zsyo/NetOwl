//! 采集器后台回填:Authenticode 签名校验与进程图标的限流派发、结果
//! 收割与缓存回写(工作线程经 mpsc 回报);采集主流程在 windows_table。

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::windows_table::TableCollector;
use super::{IconState, signature};
use crate::model::Signing;

const SIG_MAX_INFLIGHT: usize = 4;
/// 签名校验在途超时:WinVerifyTrust 在目标文件被占用/证书存储异常时可能
/// 长时间不返回,无兜底会把这 4 个槽位永久占满(与图标侧同口径)
const SIG_INFLIGHT_TIMEOUT: Duration = Duration::from_secs(30);
const SIG_DISPATCH_PER_POLL: usize = 2;
const ICON_DISPATCH_PER_POLL: usize = 4;
/// 图标提取在途超时:提取线程卡住(UNC 路径/巨大文件)时 inflight 恒满、
/// budget 恒 0,之后新进程的图标永不派发;超时未回的路径移除并允许重试
const ICON_INFLIGHT_TIMEOUT: Duration = Duration::from_secs(30);
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
        // 在途超时清理:工作线程 panic 时 tx.send 永不发生,该 PID 会
        // 一直留在在途集合里把 4 个槽位占满;超时移除后下一轮可重派
        let now = Instant::now();
        self.sig_pending.retain(|pid, at| {
            if now.duration_since(*at) >= SIG_INFLIGHT_TIMEOUT {
                tracing::debug!("[Collector] 签名校验超时未返回,允许重试:pid {pid}");
                false
            } else {
                true
            }
        });
    }

    /// 收割已完成的图标提取结果
    pub(super) fn collect_icons(&mut self) {
        while let Ok((path, img)) = self.icon_rx.try_recv() {
            self.icon_inflight.remove(&path);
            match img {
                Some(img) => {
                    self.icons
                        .insert(path, IconState::Ready(Some(Arc::new(img))));
                }
                None => {
                    // 无色位图老式图标与提取失败同样返回 None:前者是"该
                    // 进程确实无彩色图标"的既成事实,缓存占位避免每轮重派;
                    // 但提取线程崩溃/句柄异常等瞬时失败不缓存,下轮重试
                    tracing::debug!("[Collector] 进程图标提取失败:{path}");
                    self.icons.insert(path, IconState::Ready(None));
                }
            }
        }
        // 在途超时清理:提取线程无超时机制,卡住时该路径永不复位,会把
        // 派发预算永久占满。超时路径在 icons 中仍是 Pending,下轮可重派
        let now = Instant::now();
        let mut timed_out = Vec::new();
        self.icon_inflight.retain(|path, at| {
            if now.duration_since(*at) >= ICON_INFLIGHT_TIMEOUT {
                timed_out.push(path.clone());
                false
            } else {
                true
            }
        });
        for path in timed_out {
            tracing::debug!("[Collector] 图标提取超时未返回,允许重试:{path}");
        }
        // 未派发的 Pending 条目(超出上轮预算的)按上限补齐。在途路径必须
        // 排除:卡住的路径恒为 Pending,若重复派发会把超时计时不断清零,
        // 30s 兜底永不触发,每轮新生一个线程直到句柄耗尽
        let budget = ICON_DISPATCH_PER_POLL.saturating_sub(self.icon_inflight.len());
        for path in self
            .icons
            .iter()
            .filter(|(p, s)| {
                matches!(s, IconState::Pending) && !self.icon_inflight.contains_key(p.as_str())
            })
            .map(|(p, _)| p.clone())
            .take(budget)
            .collect::<Vec<_>>()
        {
            self.icon_inflight.insert(path.clone(), Instant::now());
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
                    && !self.sig_pending.contains_key(pid)
            })
            .map(|(pid, meta)| (*pid, meta.path.clone().expect("path is some")))
            .take(budget.min(SIG_DISPATCH_PER_POLL))
            .collect::<Vec<_>>();
        for (pid, path) in candidates {
            if self.sig_pending.len() >= SIG_MAX_INFLIGHT {
                break;
            }
            self.sig_pending.insert(pid, Instant::now());
            let tx = self.sig_tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send((pid, signature::verify(&path)));
            });
        }
    }
}
