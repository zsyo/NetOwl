//! 世界地理数据:城市坐标与 Natural Earth 矢量底图(等距圆柱投影)。
//!
//! 底图数据由 tools/build_mapdata.py 从 Natural Earth admin_0_countries
//! (公有领域)生成,编译期内嵌于 assets/mapdata.bin;含两档精度:
//! levels[0] = 110m(全局视图),levels[1] = 50m 抽稀(放大视图)。
//! 每档预处理出:多边形环(含包围盒)、三角形(陆地/洞)与
//! 海岸线/国界线段(按邻国共享边分类),全部在首次访问时构建一次。
//! 城市坐标表在 cities,二进制解码与几何预处理在 decode。

mod cities;
mod decode;

use std::sync::OnceLock;

pub use cities::{CITIES, City, LOCAL, city};

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
    DATA.get_or_init(|| decode::build(include_bytes!("../../../assets/mapdata.bin")))
}
