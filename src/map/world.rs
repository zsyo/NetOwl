//! 世界地理数据:城市坐标与 Natural Earth 矢量底图(等距圆柱投影)。
//!
//! 底图数据由 tools/build_mapdata.py 从 Natural Earth admin_0_countries
//! (公有领域)生成,编译期内嵌于 assets/mapdata.bin;含两档精度:
//! levels[0] = 110m(全局视图),levels[1] = 50m 抽稀(放大视图)。
//! 每档预处理出:多边形环(含包围盒)、三角形(陆地/洞)与
//! 海岸线/国界线段(按邻国共享边分类),全部在首次访问时构建一次。

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::map::triangulate::triangulate;

/// 城市/节点:显示名经 i18n 词条(`city-<key>`)获取,这里只保留标识与坐标
pub struct City {
    /// 稳定标识(如 "tokyo"),供 [`crate::model::Connection::city`] 与词条键引用
    pub key: &'static str,
    pub lon: f32,
    pub lat: f32,
}

/// 本机节点
pub const LOCAL: City = City {
    key: "local",
    lon: 116.4,
    lat: 39.9,
};

/// 远端城市表(MockCollector 按此生成模拟连接的地理归属)
pub const CITIES: &[City] = &[
    City {
        key: "shanghai",
        lon: 121.47,
        lat: 31.23,
    },
    City {
        key: "tokyo",
        lon: 139.69,
        lat: 35.69,
    },
    City {
        key: "seoul",
        lon: 126.98,
        lat: 37.57,
    },
    City {
        key: "hongkong",
        lon: 114.17,
        lat: 22.32,
    },
    City {
        key: "macau",
        lon: 113.55,
        lat: 22.20,
    },
    City {
        key: "taipei",
        lon: 121.56,
        lat: 25.03,
    },
    City {
        key: "singapore",
        lon: 103.82,
        lat: 1.35,
    },
    City {
        key: "mumbai",
        lon: 72.88,
        lat: 19.08,
    },
    City {
        key: "moscow",
        lon: 37.62,
        lat: 55.75,
    },
    City {
        key: "frankfurt",
        lon: 8.68,
        lat: 50.11,
    },
    City {
        key: "amsterdam",
        lon: 4.90,
        lat: 52.37,
    },
    City {
        key: "london",
        lon: -0.13,
        lat: 51.51,
    },
    City {
        key: "newyork",
        lon: -74.01,
        lat: 40.71,
    },
    City {
        key: "sanjose",
        lon: -121.89,
        lat: 37.34,
    },
    City {
        key: "sydney",
        lon: 151.21,
        lat: -33.87,
    },
    City {
        key: "saopaulo",
        lon: -46.63,
        lat: -23.55,
    },
];

/// 按键查城市;`key` 必须来自 [`CITIES`](crate::map::world::CITIES)
pub fn city(key: &str) -> &City {
    CITIES
        .iter()
        .find(|c| c.key == key)
        .expect("unknown city key")
}

/// 环类型:陆地外环 / 海洋洞环(里海、莱索托等,渲染时以海洋色盖回)
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RingKind {
    Land,
    Hole,
}

/// 标签类别:国家 / 海洋 / 中国省级
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LabelKind {
    Country,
    Sea,
    Province,
}

/// 地图名称标签(国家/海洋/省),随缩放按 rank 分级显隐
pub struct MapLabel {
    pub name_zh: String,
    pub name_en: String,
    pub lon: f32,
    pub lat: f32,
    pub kind: LabelKind,
    /// 0 = 全局即显示,2 = 高倍缩放才显示
    pub rank: u8,
}

/// 多边形环:顶点索引区间
pub struct Ring {
    pub kind: RingKind,
    /// 在 [`MapLevel::ring_indices`] 中的区间 [start, start + len)
    pub start: u32,
    pub len: u32,
    /// 三角形索引区间;三角形顶点为环内局部索引(0..len)
    pub tri_start: u32,
    pub tri_len: u32,
    /// 顶点经纬度包围盒(视口剔除用)
    pub min_lon: f32,
    pub min_lat: f32,
    pub max_lon: f32,
    pub max_lat: f32,
}

/// 单档底图数据
pub struct MapLevel {
    /// 量化坐标(0.001 度)还原后的经纬度
    pub verts: Vec<(f32, f32)>,
    /// 环顶点序列:按环顺序展开的全局顶点索引
    pub ring_indices: Vec<u32>,
    pub rings: Vec<Ring>,
    /// 陆地三角形(环内局部索引三元组)
    pub tris: Vec<[u32; 3]>,
    /// 洞环三角形(海洋色填充)
    pub hole_tris: Vec<[u32; 3]>,
    /// 海岸线段(全局顶点索引对)
    pub coast: Vec<[u32; 2]>,
    /// 国界线段(全局顶点索引对)
    pub border: Vec<[u32; 2]>,
}

/// 全部底图数据:世界层(NE)与中国层(DataV)分层,中国层渲染时覆盖其上
pub struct MapData {
    /// 世界层两档 LOD(NE,中国及港澳台已剔除)
    pub world: [MapLevel; 2],
    /// 中国层两档 LOD(DataV 省级几何),覆盖在世界层之上
    pub china: [MapLevel; 2],
    /// 国家/海洋/省级名称标签
    pub labels: Vec<MapLabel>,
    /// 南海断续国界十段线(经纬度线段,世界数据独立一节)
    pub south_sea_line: Vec<[f32; 4]>,
    /// 主要河流折线两档(全局档/精细档);独立于世界/中国层,
    /// 渲染在其上(中国层陆地填充会覆盖层内线条)
    pub rivers: [Vec<Vec<(f32, f32)>>; 2],
}

/// 底图数据(内嵌二进制,首次访问时解码并做几何预处理)
pub fn map_data() -> &'static MapData {
    static DATA: OnceLock<MapData> = OnceLock::new();
    DATA.get_or_init(|| build(include_bytes!("../../assets/mapdata.bin")))
}

/// 二进制游标:varint / zigzag 解码
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    fn byte(&mut self) -> u8 {
        let b = *self.data.get(self.pos).expect("mapdata.bin truncated");
        self.pos += 1;
        b
    }

    fn varint(&mut self) -> u64 {
        let mut result = 0u64;
        let mut shift = 0;
        loop {
            let b = self.byte();
            result |= ((b & 0x7F) as u64) << shift;
            if b & 0x80 == 0 {
                return result;
            }
            shift += 7;
        }
    }

    fn zigzag(&mut self) -> i32 {
        let v = self.varint();
        ((v >> 1) as i32) ^ -((v & 1) as i32)
    }

    fn take(&mut self, n: usize) -> &'a [u8] {
        let s = self
            .data
            .get(self.pos..self.pos + n)
            .expect("mapdata.bin truncated");
        self.pos += n;
        s
    }
}

fn build(bin: &[u8]) -> MapData {
    let mut r = Reader::new(bin);
    assert!(
        r.byte() == b'N' && r.byte() == b'W' && r.byte() == b'L' && r.byte() == b'D',
        "mapdata.bin: bad magic"
    );
    // version 5:分层渲染 + 河流折线;每档 LOD 依次为世界层、中国层
    assert!(r.byte() == 5, "mapdata.bin: unsupported version");
    assert!(r.byte() == 2, "mapdata.bin: expected 2 levels");
    // 编码顺序:world0, china0, world1, china1
    let w0 = decode_level(&mut r);
    let c0 = decode_level(&mut r);
    let w1 = decode_level(&mut r);
    let c1 = decode_level(&mut r);
    let world = [w0, w1];
    let china = [c0, c1];
    let south_sea_line = decode_south_sea_line(&mut r);
    let rivers = [decode_rivers(&mut r), decode_rivers(&mut r)];
    let labels = decode_labels(&mut r);
    MapData {
        world,
        china,
        labels,
        south_sea_line,
        rivers,
    }
}

/// 南海断续国界十段线:每段 (lon0, lat0, lon1, lat1),量化 0.001 度还原
fn decode_south_sea_line(r: &mut Reader) -> Vec<[f32; 4]> {
    let n = r.varint() as usize;
    let mut segs = Vec::with_capacity(n);
    for _ in 0..n {
        let seg = [
            r.zigzag() as f32 / 1000.0,
            r.zigzag() as f32 / 1000.0,
            r.zigzag() as f32 / 1000.0,
            r.zigzag() as f32 / 1000.0,
        ];
        segs.push(seg);
    }
    segs
}

/// 河流折线:量化坐标增量解码,渲染时展开为相邻点对线段
fn decode_rivers(r: &mut Reader) -> Vec<Vec<(f32, f32)>> {
    let n = r.varint() as usize;
    let mut lines = Vec::with_capacity(n);
    for _ in 0..n {
        let len = r.varint() as usize;
        let mut pts = Vec::with_capacity(len);
        let (mut x, mut y) = (0i32, 0i32);
        for _ in 0..len {
            x += r.zigzag();
            y += r.zigzag();
            pts.push((x as f32 / 1000.0, y as f32 / 1000.0));
        }
        lines.push(pts);
    }
    lines
}

fn decode_string(r: &mut Reader) -> String {
    let len = r.varint() as usize;
    String::from_utf8(r.take(len).to_vec()).expect("mapdata.bin: label not utf-8")
}

fn decode_labels(r: &mut Reader) -> Vec<MapLabel> {
    let n = r.varint() as usize;
    let mut labels = Vec::with_capacity(n);
    for _ in 0..n {
        let name_zh = decode_string(r);
        let name_en = decode_string(r);
        let lon = r.zigzag() as f32 / 1000.0;
        let lat = r.zigzag() as f32 / 1000.0;
        let kind = match r.byte() {
            1 => LabelKind::Sea,
            2 => LabelKind::Province,
            _ => LabelKind::Country,
        };
        let rank = r.byte();
        labels.push(MapLabel {
            name_zh,
            name_en,
            lon,
            lat,
            kind,
            rank,
        });
    }
    labels
}

fn decode_level(r: &mut Reader) -> MapLevel {
    // 顶点表:量化坐标增量解码;整数坐标另存一份供三角化精确判定
    let nv = r.varint() as usize;
    let mut quants = Vec::with_capacity(nv);
    let mut verts = Vec::with_capacity(nv);
    let (mut x, mut y) = (0i32, 0i32);
    for _ in 0..nv {
        x += r.zigzag();
        y += r.zigzag();
        quants.push((x, y));
        verts.push((x as f32 / 1000.0, y as f32 / 1000.0));
    }

    // 环:索引增量解码,扁平存放
    let nr = r.varint() as usize;
    let mut flat: Vec<u32> = Vec::new();
    let mut raw: Vec<(RingKind, u32, u32)> = Vec::with_capacity(nr);
    for _ in 0..nr {
        let kind = if r.byte() == 1 {
            RingKind::Hole
        } else {
            RingKind::Land
        };
        let len = r.varint() as u32;
        let start = flat.len() as u32;
        let mut idx = 0u32;
        for j in 0..len {
            idx = if j == 0 {
                r.zigzag() as u32
            } else {
                (idx as i64 + r.zigzag() as i64) as u32
            };
            flat.push(idx);
        }
        raw.push((kind, start, len));
    }

    // 海岸线/国界:按无向边出现次数分类(1 次 = 海岸,>=2 次 = 国界)
    let mut counts: HashMap<u64, u32> = HashMap::with_capacity(flat.len() / 2);
    for &(_, start, len) in &raw {
        for i in 0..len {
            let a = flat[(start + i) as usize] as u64;
            let b = flat[(start + (i + 1) % len) as usize] as u64;
            let key = a.min(b) << 32 | a.max(b);
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    let mut coast = Vec::new();
    let mut border = Vec::new();
    for &(_, start, len) in &raw {
        for i in 0..len {
            let a = flat[(start + i) as usize] as u64;
            let b = flat[(start + (i + 1) % len) as usize] as u64;
            let key = a.min(b) << 32 | a.max(b);
            if counts[&key] == 1 {
                coast.push([
                    flat[(start + i) as usize],
                    flat[(start + (i + 1) % len) as usize],
                ]);
            } else {
                border.push([
                    flat[(start + i) as usize],
                    flat[(start + (i + 1) % len) as usize],
                ]);
            }
        }
    }

    // 三角化与包围盒
    let mut rings = Vec::with_capacity(raw.len());
    let mut tris = Vec::new();
    let mut hole_tris = Vec::new();
    for &(kind, start, len) in &raw {
        let idxs = &flat[start as usize..(start + len) as usize];
        let (mut min_lon, mut min_lat) = (f32::MAX, f32::MAX);
        let (mut max_lon, mut max_lat) = (f32::MIN, f32::MIN);
        for &i in idxs {
            let (lon, lat) = verts[i as usize];
            min_lon = min_lon.min(lon);
            max_lon = max_lon.max(lon);
            min_lat = min_lat.min(lat);
            max_lat = max_lat.max(lat);
        }
        let ring_pts: Vec<(i32, i32)> = idxs.iter().map(|&i| quants[i as usize]).collect();
        let local = triangulate(&ring_pts);
        let tri_start = if kind == RingKind::Land {
            tris.len()
        } else {
            hole_tris.len()
        } as u32;
        for t in local {
            if kind == RingKind::Land {
                tris.push(t);
            } else {
                hole_tris.push(t);
            }
        }
        let tri_len = if kind == RingKind::Land {
            tris.len() as u32 - tri_start
        } else {
            hole_tris.len() as u32 - tri_start
        };
        rings.push(Ring {
            kind,
            start,
            len,
            tri_start,
            tri_len,
            min_lon,
            min_lat,
            max_lon,
            max_lat,
        });
    }

    MapLevel {
        verts,
        ring_indices: flat,
        rings,
        tris,
        hole_tris,
        coast,
        border,
    }
}
