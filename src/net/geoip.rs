//! GeoIP 归属定位:构建期生成的 assets/geoip.bin(IPv4/IPv6 区间 ->
//! 位置表,另有区间 -> 运营商表),中国(含港澳台)到省级行政区,其余
//! 按 ISO 码到国家;私网/保留段无归属。数据源与生成见
//! tools/build_geoip.py;编译期内嵌,首次使用时解析一次。

use std::net::IpAddr;
use std::sync::OnceLock;

use crate::i18n::I18n;
use crate::map::world;
use crate::model::Place;

/// 位置条目:双语显示名 + 经纬度(显示名数量大,内嵌不走 fluent 词条)
struct PlaceEntry {
    name_zh: String,
    name_en: String,
    lon: f32,
    lat: f32,
}

struct GeoIp {
    places: Vec<PlaceEntry>,
    starts: Vec<u32>,
    ends: Vec<u32>,
    locs: Vec<u32>,
    /// 运营商/机构名表与 ISP 区间(布局同归属区间表,值指向 orgs);
    /// 数据源无机构信息的段不在表内
    orgs: Vec<String>,
    isp_starts: Vec<u32>,
    isp_ends: Vec<u32>,
    isp_orgs: Vec<u32>,
    /// IPv6 区间表(布局同 v4,IP 为 u128 数值;places/orgs 共用)
    v6_starts: Vec<u128>,
    v6_ends: Vec<u128>,
    v6_locs: Vec<u32>,
    isp6_starts: Vec<u128>,
    isp6_ends: Vec<u128>,
    isp6_orgs: Vec<u32>,
}

static GEOIP: OnceLock<GeoIp> = OnceLock::new();

fn table() -> &'static GeoIp {
    GEOIP.get_or_init(|| GeoIp::parse(include_bytes!("../../assets/geoip.bin")))
}

impl GeoIp {
    /// 解析;数据由构建脚本生成,格式不符直接报错(不做静默降级)
    fn parse(data: &[u8]) -> GeoIp {
        let mut r = Reader { data, pos: 0 };
        if r.bytes(4) != b"NWGI" {
            panic!("[GeoIp] geoip.bin 格式错误: magic 不符");
        }
        let version = r.u16();
        if version != 3 {
            panic!("[GeoIp] geoip.bin 版本不支持: {version}(需以 tools/build_geoip.py 重新生成)");
        }
        let n = r.u32() as usize;
        let mut places = Vec::with_capacity(n);
        for _ in 0..n {
            let name_zh = r.str();
            let name_en = r.str();
            let (lon, lat) = r.f32x2();
            places.push(PlaceEntry {
                name_zh,
                name_en,
                lon,
                lat,
            });
        }
        let n_ranges = r.u32() as usize;
        let (starts, ends, locs) = Self::parse_ranges(&mut r, n_ranges);
        // loc 索引直接用于 places[idx]:数据被截断/损坏时越界 panic 会
        // 落在地图渲染帧里且无法定位到数据问题,解析期校验带上下文报错
        if let Some(&bad) = locs.iter().find(|&&i| i as usize >= places.len()) {
            panic!(
                "[GeoIp] geoip.bin 数据损坏: loc 索引 {bad} 超出位置表 {} 条",
                places.len()
            );
        }
        let n_orgs = r.u32() as usize;
        let mut orgs = Vec::with_capacity(n_orgs);
        for _ in 0..n_orgs {
            orgs.push(r.str());
        }
        let n_isp = r.u32() as usize;
        let (isp_starts, isp_ends, isp_orgs) = Self::parse_ranges(&mut r, n_isp);
        if let Some(&bad) = isp_orgs.iter().find(|&&i| i as usize >= orgs.len()) {
            panic!(
                "[GeoIp] geoip.bin 数据损坏: org 索引 {bad} 超出机构表 {} 条",
                orgs.len()
            );
        }
        let n6 = r.u32() as usize;
        let (v6_starts, v6_ends, v6_locs) = Self::parse_ranges128(&mut r, n6);
        if let Some(&bad) = v6_locs.iter().find(|&&i| i as usize >= places.len()) {
            panic!(
                "[GeoIp] geoip.bin 数据损坏: v6 loc 索引 {bad} 超出位置表 {} 条",
                places.len()
            );
        }
        let n_isp6 = r.u32() as usize;
        let (isp6_starts, isp6_ends, isp6_orgs) = Self::parse_ranges128(&mut r, n_isp6);
        if let Some(&bad) = isp6_orgs.iter().find(|&&i| i as usize >= orgs.len()) {
            panic!(
                "[GeoIp] geoip.bin 数据损坏: v6 org 索引 {bad} 超出机构表 {} 条",
                orgs.len()
            );
        }
        GeoIp {
            places,
            starts,
            ends,
            locs,
            orgs,
            isp_starts,
            isp_ends,
            isp_orgs,
            v6_starts,
            v6_ends,
            v6_locs,
            isp6_starts,
            isp6_ends,
            isp6_orgs,
        }
    }

    /// 区间表解析:LEB128 delta(起点距上区间末尾、区间长度、值索引 zigzag)
    fn parse_ranges(r: &mut Reader, n: usize) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
        let (mut starts, mut ends, mut vals) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );
        let mut prev_end = 0u32;
        let mut prev_idx = 0i64;
        for _ in 0..n {
            let start = prev_end.wrapping_add(r.varint() as u32);
            let end = start.wrapping_add(r.varint().wrapping_sub(1) as u32);
            let idx = (prev_idx + r.zigzag()) as u32;
            starts.push(start);
            ends.push(end);
            vals.push(idx);
            prev_end = end.wrapping_add(1);
            prev_idx = idx as i64;
        }
        (starts, ends, vals)
    }

    /// IPv6 区间表解析(u128 起止,编码同 v4;start/length 的 delta 可能
    /// 超出 u64——v6 地址空间内相邻表段可隔着巨大的未分配区)
    fn parse_ranges128(r: &mut Reader, n: usize) -> (Vec<u128>, Vec<u128>, Vec<u32>) {
        let (mut starts, mut ends, mut vals) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );
        let mut prev_end = 0u128;
        let mut prev_idx = 0i64;
        for _ in 0..n {
            let start = prev_end.wrapping_add(r.varint128());
            let end = start.wrapping_add(r.varint128().wrapping_sub(1));
            let idx = (prev_idx + r.zigzag()) as u32;
            starts.push(start);
            ends.push(end);
            vals.push(idx);
            prev_end = end.wrapping_add(1);
            prev_idx = idx as i64;
        }
        (starts, ends, vals)
    }

    fn locate(&self, ip: u32) -> Option<u32> {
        bisect(&self.starts, &self.ends, &self.locs, ip)
    }

    fn locate6(&self, ip: u128) -> Option<u32> {
        bisect(&self.v6_starts, &self.v6_ends, &self.v6_locs, ip)
    }
}

/// 区间表二分(starts 升序且互不重叠);命中返回值索引。
/// v4 表为 u32、v6 表为 u128,同形比较逻辑泛型复用
fn bisect<T: PartialOrd>(starts: &[T], ends: &[T], vals: &[u32], ip: T) -> Option<u32> {
    let (mut lo, mut hi) = (0i64, starts.len() as i64 - 1);
    while lo <= hi {
        let mid = (lo + hi) / 2;
        if ip < starts[mid as usize] {
            hi = mid - 1;
        } else if ip > ends[mid as usize] {
            lo = mid + 1;
        } else {
            return Some(vals[mid as usize]);
        }
    }
    None
}

/// 远端 IP 的归属位置索引;私网/保留段/未收录返回 None
pub fn locate(ip: IpAddr) -> Option<u32> {
    let t = table();
    match ip {
        IpAddr::V4(v4) => t.locate(v4.to_bits()),
        IpAddr::V6(v6) => t.locate6(u128::from(v6)),
    }
}

/// 远端 IP 的运营商/机构名(数据源机构字段原文直显,国内多为"电信/
/// 联通/移动"等,国外为机构注册名);该段无机构信息时返回 None
pub fn locate_isp(ip: IpAddr) -> Option<&'static str> {
    let t = table();
    let idx = match ip {
        IpAddr::V4(v4) => bisect(&t.isp_starts, &t.isp_ends, &t.isp_orgs, v4.to_bits())?,
        IpAddr::V6(v6) => bisect(&t.isp6_starts, &t.isp6_ends, &t.isp6_orgs, u128::from(v6))?,
    };
    t.orgs.get(idx as usize).map(String::as_str)
}

/// 归属地的地图坐标(经度,纬度)
pub fn place_pos(place: Place) -> (f32, f32) {
    match place {
        Place::City(key) => {
            let c = world::city(key);
            (c.lon, c.lat)
        }
        Place::Geo(idx) => {
            let p = &table().places[idx as usize];
            (p.lon, p.lat)
        }
    }
}

/// 归属地显示名(按界面语言)
pub fn place_label(place: Place, i18n: &I18n) -> String {
    match place {
        Place::City(key) => i18n.t(&format!("city-{key}")),
        Place::Geo(idx) => {
            let p = &table().places[idx as usize];
            if i18n.current_lang.starts_with("zh") {
                p.name_zh.clone()
            } else {
                p.name_en.clone()
            }
        }
    }
}

/// geoip.bin 字节读取
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> &[u8] {
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        s
    }

    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.bytes(2).try_into().expect("u16"))
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.bytes(4).try_into().expect("u32"))
    }

    fn f32x2(&mut self) -> (f32, f32) {
        let a = f32::from_le_bytes(self.bytes(4).try_into().expect("f32"));
        let b = f32::from_le_bytes(self.bytes(4).try_into().expect("f32"));
        (a, b)
    }

    fn str(&mut self) -> String {
        let len = self.data[self.pos] as usize;
        self.pos += 1;
        String::from_utf8_lossy(self.bytes(len)).into_owned()
    }

    /// LEB128 varint(u64 最多 10 字节)
    fn varint(&mut self) -> u64 {
        let mut result = 0u64;
        let mut shift = 0u32;
        loop {
            let b = self.data[self.pos];
            self.pos += 1;
            // 数据编译期内嵌,超长 varint 只可能来自损坏的 geoip.bin:
            // 显式报错,不让 shift 在 debug 下溢出 panic、release 下回绕
            assert!(
                shift < 64,
                "geoip.bin varint 超长(数据损坏),pos {}",
                self.pos
            );
            result |= ((b & 0x7F) as u64) << shift;
            if b & 0x80 == 0 {
                return result;
            }
            shift += 7;
        }
    }

    /// u128 版 varint(v6 区间 delta 可超 u64,最多 19 字节)
    fn varint128(&mut self) -> u128 {
        let mut result = 0u128;
        let mut shift = 0u32;
        loop {
            let b = self.data[self.pos];
            self.pos += 1;
            assert!(
                shift < 128,
                "geoip.bin varint128 超长(数据损坏),pos {}",
                self.pos
            );
            result |= ((b & 0x7F) as u128) << shift;
            if b & 0x80 == 0 {
                return result;
            }
            shift += 7;
        }
    }

    fn zigzag(&mut self) -> i64 {
        let v = self.varint();
        ((v >> 1) as i64) ^ -((v & 1) as i64)
    }
}
