//! 局域网设备发现编排:ARP 轮询、lan_devices 库合并与 LAN 页视图组装。
//!
//! 轮询节流独立于连接采集(ARP 表小,10s 足够);每轮把 ARP 条目合并进
//! 设备表(新 MAC 插入并记日志,已知项刷新 last_seen),再读全表与本轮
//! 在线集合合并为视图行。"新设备"口径 = first_seen 距今不足 24 小时,
//! 纯读时判定,无已读状态。

use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use rusqlite::params;

use crate::net::lan::{self, DeviceRow};
use crate::storage::history::unix_now;

/// ARP 轮询间隔:表项老化分钟级,10s 足够跟手
const POLL_INTERVAL: Duration = Duration::from_secs(10);
/// 新设备判定窗口(秒)
const NEW_WINDOW: u64 = 24 * 3600;

/// LAN 设备发现状态(App 持有)
pub struct LanState {
    devices: Vec<DeviceRow>,
    poll_at: Instant,
    /// 首轮基线:启动时库里已有设备(或首轮 ARP 全量入库)不视为
    /// "新接入",基线后才产生通知事件
    baseline_done: bool,
}

impl LanState {
    pub fn new() -> Self {
        LanState {
            devices: Vec::new(),
            poll_at: Instant::now() - POLL_INTERVAL,
            baseline_done: false,
        }
    }

    /// 当前设备视图(last_seen 降序)
    pub fn devices(&self) -> &[DeviceRow] {
        &self.devices
    }

    /// 轮询:查询 ARP 表 -> 合并设备表 -> 重建视图;查询失败保留旧视图。
    /// 返回本轮新接入的设备(基线轮不计),供通知层使用
    pub fn poll(&mut self, db: &rusqlite::Connection) -> Vec<(Ipv4Addr, String)> {
        if self.poll_at.elapsed() < POLL_INTERVAL {
            return Vec::new();
        }
        self.poll_at = Instant::now();
        let entries = match lan::query_arp() {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("[Lan] {e}");
                return Vec::new();
            }
        };
        let now = unix_now();
        let fresh = self.merge(db, &entries, now);
        self.baseline_done = true;
        self.devices = self.load_view(db, &entries, now);
        fresh
    }

    /// ARP 条目合并进 lan_devices:新 MAC 插入(日志留痕),已知项刷新
    /// last_seen 与最近已知 IP;返回新插入的设备(基线轮不计)
    fn merge(
        &mut self,
        db: &rusqlite::Connection,
        entries: &[lan::ArpEntry],
        now: u64,
    ) -> Vec<(Ipv4Addr, String)> {
        let mut fresh = Vec::new();
        // 现有 MAC 一次读出:免每条设备一次存在性查询,且区分空表与库错误
        let known: HashSet<String> = match (|| -> rusqlite::Result<HashSet<String>> {
            let mut stmt = db.prepare("SELECT mac FROM lan_devices")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        })() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("[Lan] 设备表读取失败,本轮合并跳过: {e}");
                return Vec::new();
            }
        };
        for (ip, mac) in entries {
            let result = if known.contains(mac) {
                db.execute(
                    "UPDATE lan_devices SET last_seen = ?1, ip = ?2 WHERE mac = ?3",
                    params![now as i64, u32::from(*ip) as i64, mac],
                )
            } else {
                tracing::info!("[Lan] 发现新设备 {mac}({ip})");
                if self.baseline_done {
                    fresh.push((*ip, mac.clone()));
                }
                db.execute(
                    "INSERT INTO lan_devices (mac, first_seen, last_seen, ip)
                     VALUES (?1, ?2, ?2, ?3)",
                    params![mac, now as i64, u32::from(*ip) as i64],
                )
            };
            if let Err(e) = result {
                tracing::warn!("[Lan] 设备 {mac} 写库失败: {e}");
            }
        }
        fresh
    }

    /// 读全表组装视图:本轮 ARP 可见 = 在线;first_seen 24h 内 = 新设备
    fn load_view(
        &self,
        db: &rusqlite::Connection,
        entries: &[lan::ArpEntry],
        now: u64,
    ) -> Vec<DeviceRow> {
        let online: HashSet<&String> = entries.iter().map(|(_, mac)| mac).collect();
        let result = (|| -> rusqlite::Result<Vec<DeviceRow>> {
            let mut stmt = db.prepare(
                "SELECT mac, ip, first_seen, last_seen FROM lan_devices
                 ORDER BY last_seen DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?;
            Ok(rows
                .filter_map(|r| r.ok())
                .map(|(mac, ip, first_seen, last_seen)| DeviceRow {
                    online: online.contains(&mac),
                    is_new: now.saturating_sub(first_seen as u64) < NEW_WINDOW,
                    mac,
                    ip: Ipv4Addr::from(ip as u32),
                    first_seen: first_seen.max(0) as u64,
                    last_seen: last_seen.max(0) as u64,
                })
                .collect())
        })();
        match result {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("[Lan] 设备视图读取失败: {e}");
                Vec::new()
            }
        }
    }
}
