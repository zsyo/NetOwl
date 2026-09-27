//! GeoIP 归属定位:构建期生成的 assets/geoip.bin(IPv4 区间 -> 位置表),
//! 中国(含港澳台)到省级行政区,其余按 ISO 码到国家;私网/保留段无归属。
//! 数据源与生成见 tools/build_geoip.py;编译期内嵌,首次使用时解析一次。

use std::net::Ipv4Addr;
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
        if version != 1 {
            panic!("[GeoIp] geoip.bin 版本不支持: {version}");
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
        let n = r.u32() as usize;
        let mut geo = GeoIp {
            places,
            starts: Vec::with_capacity(n),
            ends: Vec::with_capacity(n),
            locs: Vec::with_capacity(n),
        };
        // 区间按 LEB128 delta 编码(起点距上区间末尾,区间长度,loc 索引 zigzag)
        let mut prev_end = 0u32;
        let mut prev_idx = 0i64;
        for _ in 0..n {
            let start = prev_end.wrapping_add(r.varint() as u32);
            let end = start.wrapping_add(r.varint().wrapping_sub(1) as u32);
            let idx = (prev_idx + r.zigzag()) as u32;
            geo.starts.push(start);
            geo.ends.push(end);
            geo.locs.push(idx);
            prev_end = end.wrapping_add(1);
            prev_idx = idx as i64;
        }
        geo
    }

    fn locate(&self, ip: u32) -> Option<u32> {
        let (mut lo, mut hi) = (0i64, self.starts.len() as i64 - 1);
        while lo <= hi {
            let mid = (lo + hi) / 2;
            if ip < self.starts[mid as usize] {
                hi = mid - 1;
            } else if ip > self.ends[mid as usize] {
                lo = mid + 1;
            } else {
                return Some(self.locs[mid as usize]);
            }
        }
        None
    }
}

/// 远端 IP 的归属位置索引;私网/保留段/未收录返回 None
pub fn locate(ip: Ipv4Addr) -> Option<u32> {
    table().locate(ip.to_bits())
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

    fn varint(&mut self) -> u64 {
        let mut result = 0u64;
        let mut shift = 0;
        loop {
            let b = self.data[self.pos];
            self.pos += 1;
            result |= ((b & 0x7F) as u64) << shift;
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
