//! 模拟采集器:维持 8~28 条活跃连接,每秒累加流量,小概率关闭/新建。
//! 供演示与测试(设置页可切换数据源)。

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::collector::{Collector, CollectorKind};
use crate::model::{Connection, Protocol};
use crate::world::CITIES;

/// xorshift64* 伪随机数:骨架期避免引入 rand 依赖(AGENTS.md 规范 8)
struct Rng(u64);

impl Rng {
    fn from_time() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15);
        Rng(nanos.max(1))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn range(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.range(100) < percent
    }
}

/// 模拟进程池:常见联网进程
const PROCESSES: &[&str] = &[
    "chrome.exe", "msedge.exe", "Code.exe", "steam.exe", "spotify.exe",
    "discord.exe", "svchost.exe", "outlook.exe", "explorer.exe", "NetOwl.exe",
];

/// 每城市绑定的假公网网段首字节(仅用于演示观感,非真实归属)
const CITY_PREFIX: &[(&str, u8)] = &[
    ("shanghai", 101), ("tokyo", 126), ("seoul", 175), ("hongkong", 27),
    ("macau", 182), ("taipei", 111), ("singapore", 103),
    ("mumbai", 49), ("moscow", 95), ("frankfurt", 92), ("amsterdam", 145),
    ("london", 51), ("newyork", 74), ("sanjose", 104), ("sydney", 1),
    ("saopaulo", 177),
];

const MAX_CONNS: usize = 28;

/// 模拟采集器:维持 8~28 条活跃连接,每秒累加流量,小概率关闭/新建
pub struct MockCollector {
    rng: Rng,
    conns: HashMap<u64, Connection>,
    next_id: u64,
    last_tick: Instant,
    next_spawn: Instant,
}

impl MockCollector {
    pub fn new() -> Self {
        let now = Instant::now();
        let mut c = MockCollector {
            rng: Rng::from_time(),
            conns: HashMap::new(),
            next_id: 1,
            last_tick: now,
            next_spawn: now,
        };
        // 初始即有 8~12 条连接,首屏不为空
        for _ in 0..(8 + c.rng.range(5)) {
            c.spawn();
        }
        c
    }

    fn spawn(&mut self) {
        let idx = self.rng.range(CITIES.len() as u64) as usize;
        let city = &CITIES[idx];
        let prefix = CITY_PREFIX
            .iter()
            .find(|(key, _)| *key == city.key)
            .map(|(_, p)| *p)
            .expect("CITY_PREFIX 覆盖了全部城市");
        let ip = Ipv4Addr::new(
            prefix,
            (self.rng.range(256)) as u8,
            (self.rng.range(256)) as u8,
            (1 + self.rng.range(254)) as u8,
        );
        let port = match self.rng.range(10) {
            0..=5 => 443,
            6 => 80,
            7 => 53,
            8 => 8080,
            _ => 1000 + self.rng.range(64000) as u16,
        };
        let process = PROCESSES[self.rng.range(PROCESSES.len() as u64) as usize];
        let id = self.next_id;
        self.next_id += 1;
        self.conns.insert(id, Connection {
            id,
            pid: (1000 + self.rng.range(90000)) as u32,
            process: process.to_owned(),
            proto: if self.rng.chance(85) { Protocol::Tcp } else { Protocol::Udp },
            remote_ip: ip,
            remote_port: port,
            city: Some(city.key),
            bytes_in: self.rng.range(2 << 20),
            bytes_out: self.rng.range(2 << 18),
            first_seen: Instant::now(),
        });
    }

    /// 推进模拟:流量累加、随机关闭、到点新建
    fn advance(&mut self, now: Instant) {
        let dt = now.duration_since(self.last_tick).as_secs_f32().clamp(0.0, 5.0);
        self.last_tick = now;
        let mut expired = Vec::new();
        for conn in self.conns.values_mut() {
            conn.bytes_in += (self.rng.range(180 << 10) as f32 * dt) as u64;
            conn.bytes_out += (self.rng.range(48 << 10) as f32 * dt) as u64;
            // 生命周期 30s~3min 的模拟衰减,保持地图动态
            if now.duration_since(conn.first_seen) > Duration::from_secs(30 + self.rng.range(150)) {
                expired.push(conn.id);
            }
        }
        for id in expired {
            self.conns.remove(&id);
        }
        if now >= self.next_spawn && self.conns.len() < MAX_CONNS {
            self.spawn();
            self.next_spawn = now + Duration::from_millis(400 + self.rng.range(2200));
        }
    }
}

impl Collector for MockCollector {
    fn snapshot(&mut self) -> Vec<Connection> {
        let now = Instant::now();
        self.advance(now);
        let mut conns: Vec<Connection> = self.conns.values().cloned().collect();
        conns.sort_by_key(|c| std::cmp::Reverse(c.total_bytes()));
        conns
    }

    fn kind(&self) -> CollectorKind {
        CollectorKind::Mock
    }
}
