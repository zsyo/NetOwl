//! mapdata.bin 二进制解码:varint/zigzag 游标、分层/十段线/河流/标签
//! 读取与几何预处理(共享边分类、耳剪三角化、包围盒)。

use std::collections::HashMap;

use super::{LabelKind, MapData, MapLabel, MapLevel, Ring, RingKind};
use crate::map::triangulate::triangulate;

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

pub(super) fn build(bin: &[u8]) -> MapData {
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
