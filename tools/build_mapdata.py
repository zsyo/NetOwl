# -*- coding: utf-8 -*-
"""构建 assets/mapdata.bin:NetOwl 流量地图底图的紧凑矢量数据。

数据源:
  世界国界/海洋/湖泊:Natural Earth admin_0_countries / geography_marine_polys
  / lakes / lakes_historic(公有领域,Public Domain)
    https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/
    注:NE admin_0 50m 档不含咸海(GeoJSON 4.x 之后主体数据稳定);
    大湖由湖泊层补,咸海用历史层整体轮廓(残存南北水域太细)。
  中国行政区划(省界、港澳台、藏南、南海诸岛、十段线):
    阿里云 DataV GeoAtlas areas_v3 静态 GeoJSON(无 key,离线下载;
    数据基于天地图,GCJ-02 坐标系,与 NE 邻国顶点有 <0.02 度的正常偏移)。
    中国(含港澳台)的 NE feature(CHN/HKG/MAC/TWN)完全剔除,
    改由 DataV 提供,以修正 NE 源数据中的政治表述(台湾 NAME_ZH
    为"中华民国")与边界偏差问题。

  DataV URLs:
    https://geo.datav.aliyun.com/areas_v3/bound/100000_full.json
    返回国家级+ 34 省级 feature(properties: adcode/name/level/parent),
    末尾 100000_JD 为南海断续国界线(十段线,10 段梭形闭合环)。

使用前把下载的 GeoJSON 放到 tools/cache/(该目录不入库,构建后可删除):
  ne_110m_admin_0_countries.geojson
  ne_50m_admin_0_countries.geojson
  ne_50m_geography_marine_polys.geojson
  ne_50m_lakes.geojson
  ne_50m_lakes_historic.geojson
  100000_full.json
在仓库根目录运行(python 3.8+):
  python tools/build_mapdata.py
结果写入 assets/mapdata.bin,该文件入库。

流程:
  1. 环顶点量化到 0.001 度,去除相邻重复点与首尾闭合点;
  2. 共享边检测:量化后顶点全局编号,边按 (min,max) 键计数,
     两环共用的边为共享边,独占边为海岸线;
  3. 每个环按"共享段/独占段"切分后逐段 Douglas-Peucker 抽稀。
     相邻国家共享段的顶点序列相同(方向相反),而 DP 对序列反转对称,
     故两环抽稀结果无缝衔接,不会在边界处产生缝隙;
  4. 世界层(NE)50m 档对容差二分,顶点数不超过预算;110m 档原始顶点;
     中国层(DataV)同样两档分别二分抽稀,独立预算;
  5. 分层渲染:世界层先画(陆地/洞环/海岸/国界),中国层整体覆盖其上
     (陆地填充 + 国界/省界)。NE 里伸入中国境内的邻国国界线与误划
     几何(如 NE 把藏南划入印度)被中国层陆地填充盖住,邻国整体几何
     保持 NE 原样,从根上消除双线与扭曲;
  6. 邻国界顶点温和吸合:NE 邻国与中国边界相邻处(<=0.2 度)的小细缝
     对齐到中国边界顶点,消除两层边界"相邻不重合"(尼泊尔/中亚段);
     超出半径一律不动,远离边界的邻国顶点不受影响;
  7. 湖泊层:面积 >= 0.2 平方度的大湖(咸海、五大湖、贝加尔等)作为
     洞环并入世界层;咸海取历史层整体轮廓(南北残存水域过细);
  8. 标签段:国家(NE,中国除外)+ 手动补"中华人民共和国";省级 34 个
     (kind=2, 中文名取 DataV name,英文名查表;rank=2 高倍缩放显示,
     与国家 rank=0 形成层次);海洋(NE)。港澳台省名正常显示,
     其名称来自 DataV(台湾省/香港特别行政区/澳门特别行政区),
     不存在 NE 中的"中华民国"表述;
  9. 整体位于南纬 58 度以下的几何不参与(与底图一致不显示南极)。

二进制格式(小端):
  magic "NWLD" u8 version=4 u8 level_count=2
  每档 LOD 依次为:世界层 level、中国层 level;各 level:
    varint vert_count
    verts: zigzag varint (lon*1000, lat*1000) 增量序列(首点相对 0,0)
    varint ring_count
    每 ring: u8 kind(0=陆地外环,1=海洋洞环) varint n
             zigzag varint 顶点索引增量序列(首索引为绝对值)
  十段线段:
    varint seg_count
    每段: zigzag varint lon0*1000, lat0*1000, lon1*1000, lat1*1000
  标签段:
    varint label_count
    每 label: varint zh_len + utf8 bytes, varint en_len + utf8 bytes,
              zigzag varint lon*1000, zigzag varint lat*1000,
              u8 kind(0=国家,1=海洋,2=省), u8 rank(0/1/2)
"""

import json
import math
import pathlib
import sys

# 输入均为仓库内固定相对路径,不含任何动态成分
SRC_110M = "tools/cache/ne_110m_admin_0_countries.geojson"
SRC_50M = "tools/cache/ne_50m_admin_0_countries.geojson"
SRC_MARINE = "tools/cache/ne_50m_geography_marine_polys.geojson"
SRC_LAKES = "tools/cache/ne_50m_lakes.geojson"
SRC_LAKES_HISTORIC = "tools/cache/ne_50m_lakes_historic.geojson"
SRC_CN = "tools/cache/100000_full.json"
OUT_PATH = "assets/mapdata.bin"

# 50m 档顶点预算:控制运行时三角化(ear clipping O(n^2))的启动耗时。
# 湖泊层与世界层中国剥离后按实际用量上调(见 main 输出)
BUDGET_50M = 46000
QUANT = 1000  # 经纬度量化精度 0.001 度
# 南纬 58 度以下不参与(底图窗口 [-58, 84] 不显示南极)
SOUTH_CUTOFF = -58 * QUANT
# 湖泊层面积下限(平方度):咸海南北半场、五大湖、贝加尔等大湖入层。
# NE admin_0 的 50m 档不含咸海洞,地理要素(内陆湖)由本层补齐
LAKE_MIN_AREA = 0.2
# NE 邻国界顶点向中国边界顶点吸合的半径(量化单位,0.2 度):
# 分层渲染藏南已由中国层覆盖,这里只消"两国边界相邻不重合"的细缝
NEAR_SNAP_TOL = 200
NEAR_SNAP_CELL = 100

# 国家标签分级:面积(平方度)+ NE LABELRANK(越小越重要)
COUNTRY_RANK0_AREA = 150.0
COUNTRY_RANK1_AREA = 30.0
# 海洋标签分级:NE scalerank(越小越是大洋)+ 面积
SEA_RANK0_AREA = 800.0
SEA_RANK1_AREA = 60.0

# 中国及港澳台:NE geometry 与标签均剔除,改由 DataV 提供
EXCLUDED_A3 = {"CHN", "HKG", "MAC", "TWN"}
# 与中国陆地接壤的 NE 国家:仅其边界顶点参与温和吸合(<=0.2 度)
CN_NEIGHBORS = {"IND", "PAK", "AFG", "TJK", "KGZ", "KAZ",
                "MNG", "RUS", "PRK", "VNM", "LAO", "MMR", "NPL", "BTN"}
# 中国国家标签:NE 中国被剔除后手动补(国土几何质心附近)
CN_COUNTRY_LABEL = ("中华人民共和国", "China", 104.1, 37.6, 0, 0)
# DataV 中国层顶点预算(分层渲染,与世界层分开控制 ear clipping 启动耗时)
CN_BUDGET_110M = 12000
CN_BUDGET_50M = 22000

# 省级英文名(DataV name 仅中文,英文侧按 adcode 查表)
PROVINCE_EN = {
    110000: "Beijing", 120000: "Tianjin", 130000: "Hebei", 140000: "Shanxi",
    150000: "Inner Mongolia", 210000: "Liaoning", 220000: "Jilin",
    230000: "Heilongjiang", 310000: "Shanghai", 320000: "Jiangsu",
    330000: "Zhejiang", 340000: "Anhui", 350000: "Fujian", 360000: "Jiangxi",
    370000: "Shandong", 410000: "Henan", 420000: "Hubei", 430000: "Hunan",
    440000: "Guangdong", 450000: "Guangxi", 460000: "Hainan",
    500000: "Chongqing", 510000: "Sichuan", 520000: "Guizhou",
    530000: "Yunnan", 540000: "Tibet", 610000: "Shaanxi", 620000: "Gansu",
    630000: "Qinghai", 640000: "Ningxia", 650000: "Xinjiang",
    710000: "Taiwan", 810000: "Hong Kong SAR", 820000: "Macao SAR",
}

# 省名标签手工微调(度),对几何质心落在不宜展示位置的省做偏移:
#   河北:京津被掏空后质心仍压在北京附近,向左下移到保定—冀中一带;
#   内蒙古:东西大长条形,质心偏西,向右下移到域中部偏东。
PROVINCE_LABEL_NUDGE = {
    130000: (-1.0, -1.4),
    150000: (2.0, -1.2),
}


def zigzag(value):
    return (value << 1) ^ (value >> 63) if value < 0 else value << 1


def encode_varint(out, value):
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return


def douglas_peucker(points, eps):
    """迭代式 DP(避免深递归),返回保留的下标集合。"""
    n = len(points)
    keep = [False] * n
    keep[0] = keep[n - 1] = True
    stack = [(0, n - 1)]
    while stack:
        lo, hi = stack.pop()
        if hi - lo < 2:
            continue
        (x0, y0), (x1, y1) = points[lo], points[hi]
        dx, dy = x1 - x0, y1 - y0
        norm = math.hypot(dx, dy)
        best, best_i = -1.0, -1
        for i in range(lo + 1, hi):
            px, py = points[i]
            if norm > 0.0:
                dist = abs(dy * (px - x0) - dx * (py - y0)) / norm
            else:
                dist = math.hypot(px - x0, py - y0)
            if dist > best:
                best, best_i = dist, i
        if best > eps:
            keep[best_i] = True
            stack.append((lo, best_i))
            stack.append((best_i, hi))
    return keep


def simplify_ring(points, eps):
    """DP 抽稀,返回保留点列表;首尾视为锚点必然保留。

    points 为量化坐标(0.001 度整数),eps 单位为度,统一换算到量化尺度。
    """
    if len(points) <= 4:
        return points
    keep = douglas_peucker(points, eps * QUANT)
    return [p for p, k in zip(points, keep) if k]


def ne_a3(props):
    """NE feature 的行政区三字码(ISO_A3 缺失的历史版本回退 ADM0_A3)。"""
    a3 = (props.get("ISO_A3") or "").strip().upper()
    if a3 and a3 != "-99":
        return a3
    return (props.get("ADM0_A3") or "").strip().upper()


def load_rings(path, quant):
    """读取 NE GeoJSON,返回 (a3, kind, [(qlon, qlat)]) 列表(已量化、去重、去闭合点)。

    整体位于南纬 58 度以下的环(南极洲)直接丢弃:底图视图窗口为
    纬度 [-58, 84],与同类监控工具一致不显示南极,且南极收口边在
    等距圆柱投影下会呈现为刺眼的直线。
    CHN/HKG/MAC/TWN 剔除:中国(含港澳台)几何改由 DataV 提供。
    """
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    rings = []
    for feature in data["features"]:
        props = feature["properties"]
        a3 = ne_a3(props)
        if a3 in EXCLUDED_A3:
            continue
        geom = feature["geometry"]
        polys = geom["coordinates"] if geom["type"] == "MultiPolygon" else [geom["coordinates"]]
        for poly in polys:
            for idx, raw in enumerate(poly):
                pts = []
                for lon, lat in raw:
                    q = (int(round(lon * quant)), int(round(lat * quant)))
                    if not pts or q != pts[-1]:
                        pts.append(q)
                # 去除首尾闭合重复点
                if len(pts) > 1 and pts[0] == pts[-1]:
                    pts.pop()
                if len(pts) >= 3 and max(la for _, la in pts) >= SOUTH_CUTOFF:
                    rings.append((a3, 1 if idx > 0 else 0, pts))
    return rings


def load_cn_rings(path, quant):
    """读取 DataV 100000_full.json 的省级 polygons,量化去重。

    返回 (kind, [(qlon, qlat)]) 列表。省界相邻顶点天然重合,运行时
    共享边机制自动归为省界/国界线宽。邻国侧不越界的顶点不动,
    仅由 snap_near_neighbors 对小细缝做温和吸合。
    """
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    rings = []
    for feature in data["features"]:
        if feature["properties"].get("level") != "province":
            continue
        geom = feature["geometry"]
        polys = geom["coordinates"] if geom["type"] == "MultiPolygon" else [geom["coordinates"]]
        for poly in polys:
            for idx, raw in enumerate(poly):
                pts = []
                for lon, lat in raw:
                    q = (int(round(lon * quant)), int(round(lat * quant)))
                    if not pts or q != pts[-1]:
                        pts.append(q)
                if len(pts) > 1 and pts[0] == pts[-1]:
                    pts.pop()
                if len(pts) >= 3 and max(la for _, la in pts) >= SOUTH_CUTOFF:
                    rings.append((1 if idx > 0 else 0, pts))
    return rings


def load_lake_rings(path, quant, drop_names=()):
    """NE 湖泊层:大湖(咸海、五大湖、贝加尔等)作为洞环并入世界层。
    NE admin_0 50m 档不含咸海,地理要素由本层补齐。

    与国家数据语义相反:湖的每个 polygon 外环(idx=0)是水面,须按
    Hole 渲染为海色;poly 内的环(idx>0)是湖中岛,按陆地渲染。

    drop_names: 剔除指定湖名(咸海残存水域由历史层整体轮廓替代)。
    """
    rings = []
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    for feature in data["features"]:
        if (feature["properties"].get("name") or "") in drop_names:
            continue
        geom = feature["geometry"]
        polys = geom["coordinates"] if geom["type"] == "MultiPolygon" else [geom["coordinates"]]
        for poly in polys:
            for idx, raw in enumerate(poly):
                pts = []
                for lon, lat in raw:
                    q = (int(round(lon * quant)), int(round(lat * quant)))
                    if not pts or q != pts[-1]:
                        pts.append(q)
                if len(pts) > 1 and pts[0] == pts[-1]:
                    pts.pop()
                if len(pts) < 3 or max(la for _, la in pts) < SOUTH_CUTOFF:
                    continue
                area, _, _ = polygon_area_centroid(pts)
                if area >= LAKE_MIN_AREA:
                    rings.append((0 if idx > 0 else 1, pts))
    return rings


def snap_near_neighbors(ne_rings, cn_rings):
    """邻国界顶点温和吸合:距中国边界顶点 <= 0.2 度时对齐到该顶点。

    分层渲染已由世界层解决藏南等国界冲突,这里只消除 NE 与 DataV
    在两国边界处"相邻不重合"的细缝(如尼泊尔/不丹/中亚段)。
    超出半径的顶点一律不动,邻国整体几何保持 NE 原样。
    """
    grid = {}
    for _, pts in cn_rings:
        for p in pts:
            grid.setdefault((p[0] // NEAR_SNAP_CELL, p[1] // NEAR_SNAP_CELL), []).append(p)
    max_r = NEAR_SNAP_TOL // NEAR_SNAP_CELL + 1
    moved = 0

    def nearest(pt):
        nonlocal moved
        cx, cy = pt[0] // NEAR_SNAP_CELL, pt[1] // NEAR_SNAP_CELL
        for r in range(0, max_r + 1):
            best, best_d = None, NEAR_SNAP_TOL * NEAR_SNAP_TOL
            for gx in range(cx - r, cx + r + 1):
                for gy in range(cy - r, cy + r + 1):
                    for p in grid.get((gx, gy), ()):
                        d = (p[0] - pt[0]) ** 2 + (p[1] - pt[1]) ** 2
                        if d <= best_d:
                            best, best_d = p, d
            if best is not None:
                moved += 1
                return best
        return pt

    out = []
    for a3, kind, pts in ne_rings:
        if a3 not in CN_NEIGHBORS:
            out.append((a3, kind, pts))
            continue
        new_pts = [nearest(p) for p in pts]
        dedup = []
        for p in new_pts:
            if not dedup or p != dedup[-1]:
                dedup.append(p)
        if len(dedup) > 1 and dedup[0] == dedup[-1]:
            dedup.pop()
        out.append((a3, kind, dedup if len(dedup) >= 3 else pts))
    print(f"near-snap: {moved} neighbor verts aligned", file=sys.stderr)
    return out


def dedupe_vertices(rings):
    """量化顶点全局编号,返回 (verts, 编号后的环)。"""
    table = {}
    verts = []
    numbered = []
    for kind, pts in rings:
        ids = []
        for q in pts:
            vid = table.get(q)
            if vid is None:
                vid = len(verts)
                table[q] = vid
                verts.append(q)
            ids.append(vid)
        numbered.append((kind, ids))
    return verts, numbered


def segments_of(ids, shared_keys):
    """把环的边序列切成连续同质段(共享/独占),返回 [(is_shared, [vid]) 列表。

    段以顶点序列表示(环遍历顺序);相邻段共用衔接顶点,由重组逻辑去重。
    """
    n = len(ids)
    edges = []
    for i in range(n):
        a, b = ids[i], ids[(i + 1) % n]
        edges.append((min(a, b), max(a, b)) in shared_keys)
    # 找一个段边界作为起点:任一"共享状态变化"处
    start = 0
    for i in range(n):
        if edges[i] != edges[i - 1]:
            start = (i + 1) % n
            break
    segs = []
    cur = [ids[start]]
    for k in range(n):
        i = (start + k) % n
        nxt = ids[(i + 1) % n]
        cur.append(nxt)
        if edges[i] != edges[(i + 1) % n] or k + 1 == n:
            segs.append((edges[i], cur))
            cur = [nxt]
    # 最后一段的收尾顶点即起点,去掉重复(至少保留首尾两点)
    if len(segs[-1][1]) > 2:
        segs[-1] = (segs[-1][0], segs[-1][1][:-1])
    return segs


def simplify_level(rings, eps):
    """对一层环数据按段抽稀并重组,返回 (verts, [(kind, 量化点列表)])。"""
    verts, numbered = dedupe_vertices(rings)
    counts = {}
    for _, ids in numbered:
        n = len(ids)
        for i in range(n):
            key = (min(ids[i], ids[(i + 1) % n]), max(ids[i], ids[(i + 1) % n]))
            counts[key] = counts.get(key, 0) + 1
    shared_keys = {k for k, v in counts.items() if v > 1}
    out = []
    for kind, ids in numbered:
        if len(ids) <= 8:
            pts = [verts[i] for i in ids]
        else:
            pts = []
            for _, seg in segments_of(ids, shared_keys):
                sub = simplify_ring([verts[i] for i in seg], eps)
                # 段衔接去重:新段首点应等于上段尾点
                pts.extend(sub if not pts else sub[1:])
            if pts[0] == pts[-1] and len(pts) > 3:
                pts.pop()
        if len(pts) < 3:
            # 大容差 DP 会把微小环(不足 1 像素的小岛)退化成 2 点,直接丢弃
            continue
        out.append((kind, pts))
    return verts, out


def encode_level(out, rings_out):
    """只编码被环引用的顶点(抽稀后顶点表含大量未引用点,先压缩再编号)。"""
    used = []
    seen = set()
    for _, pts in rings_out:
        for p in pts:
            if p not in seen:
                seen.add(p)
                used.append(p)
    index_of = {p: i for i, p in enumerate(used)}
    encode_varint(out, len(used))
    px = py = 0
    for x, y in used:
        encode_varint(out, zigzag(x - px))
        encode_varint(out, zigzag(y - py))
        px, py = x, y
    encode_varint(out, len(rings_out))
    for kind, pts in rings_out:
        out.append(kind)
        encode_varint(out, len(pts))
        prev = 0
        for j, p in enumerate(pts):
            idx = index_of[p]
            encode_varint(out, zigzag(idx - prev if j else idx))
            prev = idx


def build_level(path, quant, budget=None, cn_rings=None, lake_rings=None):
    """世界层:NE GeoJSON(中国及港澳台已剔除)。budget=None 为原始顶点。

    cn_rings 提供中国层边界顶点时,邻国界顶点做温和吸合;
    lake_rings 为已加载的大湖洞环,直接并入世界层。
    """
    ne_rings = load_rings(path, quant)
    if cn_rings is not None:
        ne_rings = snap_near_neighbors(ne_rings, cn_rings)
    rings = [(kind, pts) for _, kind, pts in ne_rings]
    if lake_rings:
        rings = rings + lake_rings
    if budget is None:
        out = rings
    else:
        out = bisect_simplify(rings, budget)
    total = sum(len(pts) for _, pts in out)
    print(f"world level: verts={total} rings={len(out)}", file=sys.stderr)
    return out


def build_cn_level(rings, budget=None):
    """中国层:DataV 省级几何(已加载的 rings),独立抽稀,渲染时覆盖世界层。"""
    if budget is not None:
        rings = bisect_simplify(rings, budget)
    total = sum(len(pts) for _, pts in rings)
    print(f"china level: verts={total} rings={len(rings)}", file=sys.stderr)
    return rings


def bisect_simplify(rings, budget):
    """二分容差:使抽稀后总顶点数 <= budget 的最小容差。"""
    lo, hi = 0.0002, 0.2
    for _ in range(14):
        mid = math.sqrt(lo * hi)
        _, out = simplify_level(rings, mid)
        if sum(len(pts) for _, pts in out) > budget:
            lo = mid
        else:
            hi = mid
    _, out = simplify_level(rings, hi)
    return out


def polygon_area_centroid(pts):
    """环的 shoelace 面积(平方度)与加权质心(量化坐标)。

    质心按标准公式计算;退化环(面积为 0)回退为顶点平均。
    """
    n = len(pts)
    a2 = cx = cy = 0.0
    for i in range(n):
        x0, y0 = pts[i]
        x1, y1 = pts[(i + 1) % n]
        cross = x0 * y1 - x1 * y0
        a2 += cross
        cx += (x0 + x1) * cross
        cy += (y0 + y1) * cross
    if abs(a2) < 1e-9:
        return 0.0, sum(x for x, _ in pts) / n, sum(y for y, _ in pts) / n
    return abs(a2) * 0.5 / (QUANT * QUANT), cx / (3.0 * a2), cy / (3.0 * a2)


def largest_ring_centroid(geom, quant):
    """取 geometry 中面积最大的外环,返回 (centroid_lon, centroid_lat, area_deg2)。"""
    polys = geom["coordinates"] if geom["type"] == "MultiPolygon" else [geom["coordinates"]]
    best = None
    for poly in polys:
        pts = []
        for lon, lat in poly[0]:
            q = (int(round(lon * quant)), int(round(lat * quant)))
            if not pts or q != pts[-1]:
                pts.append(q)
        if len(pts) > 1 and pts[0] == pts[-1]:
            pts.pop()
        area, cx, cy = polygon_area_centroid(pts)
        if best is None or area > best[2]:
            xs = [p[0] for p in pts]
            ys = [p[1] for p in pts]
            # 质心钳制到环包围盒内,避免狭长国家(智利/挪威)标签漂出版图
            tx = min(max(cx, min(xs)), max(xs))
            ty = min(max(cy, min(ys)), max(ys))
            best = (tx / quant, ty / quant, area)
    return best


def country_rank(area_deg2, labelrank):
    if area_deg2 >= COUNTRY_RANK0_AREA and labelrank is not None and labelrank <= 4:
        return 0
    if area_deg2 >= COUNTRY_RANK1_AREA:
        return 1
    return 2


def sea_rank(area_deg2, scalerank):
    if scalerank is not None and scalerank <= 1 and area_deg2 >= SEA_RANK0_AREA:
        return 0
    if area_deg2 >= SEA_RANK1_AREA:
        return 1
    return 2


def build_labels(countries_path, marine_path, quant, cn_path):
    """国家(含中国手动补充)、省(kind=2)与海洋标签。"""
    labels = []
    with open(countries_path, encoding="utf-8") as f:
        data = json.load(f)
    for feature in data["features"]:
        props = feature["properties"]
        if ne_a3(props) in EXCLUDED_A3:
            continue
        zh = (props.get("NAME_ZH") or "").strip() or (props.get("ADMIN") or "").strip()
        en = (props.get("NAME") or "").strip()
        if not zh or not en:
            continue
        cent = largest_ring_centroid(feature["geometry"], quant)
        if cent is None:
            continue
        lon, lat, area = cent
        # 与底图几何一致的窗口过滤:质心落在南极区域(或窗口外)的标签不显示
        if not (-58.0 <= lat <= 84.0):
            continue
        rank = country_rank(area, props.get("LABELRANK"))
        labels.append((zh, en, lon, lat, 0, rank))
    with open(marine_path, encoding="utf-8") as f:
        mdata = json.load(f)
    for feature in mdata["features"]:
        props = feature["properties"]
        en = (props.get("name") or "").strip()
        zh = (props.get("name_zh") or "").strip() or en
        if not en:
            continue
        cent = largest_ring_centroid(feature["geometry"], quant)
        if cent is None:
            continue
        lon, lat, area = cent
        if not (-58.0 <= lat <= 84.0):
            continue
        rank = sea_rank(area, props.get("scalerank"))
        labels.append((zh, en, lon, lat, 1, rank))
    labels.append(CN_COUNTRY_LABEL)
    labels += build_province_labels(cn_path, quant)
    return labels


def pip(pt, ring):
    """射线法判断点是否在环内(ring 为量化顶点闭合序列,不含重复首尾点)。"""
    inside = False
    n = len(ring)
    for i in range(n):
        (x0, y0), (x1, y1) = ring[i], ring[(i + 1) % n]
        if (y0 > pt[1]) != (y1 > pt[1]):
            cross = (x1 - x0) * (pt[1] - y0) / (y1 - y0) + x0
            if cross > pt[0]:
                inside = not inside
    return inside


def province_label_pos(feature, quant):
    """省名标签位置:面积质心优先,回退最大环质心/中位数/政区中心。

    DataV 的 center 是政区中心(约等于政府驻地),天然贴边(台北之于台湾、
    海口之于海南、西宁之于青海)。标签要落在省的几何内部且视觉居中,
    按以下顺序挑选,并一律用 PIP 校验(必须落在该省某个外环内):
      1. properties.centroid(面积加权质心,视觉最居中);
      2. 最大外环的质心(centroid 缺失的省,如河北);
      3. 最大环顶点中位数(质心落在海上的沿海省/岛省,如台湾、海南);
      4. properties.center(政区中心)。
    """
    props = feature["properties"]
    geom = feature["geometry"]
    polys = geom["coordinates"] if geom["type"] == "MultiPolygon" else [geom["coordinates"]]
    outers = []
    for poly in polys:
        pts = []
        for lon, lat in poly[0]:
            q = (int(round(lon * quant)), int(round(lat * quant)))
            if not pts or q != pts[-1]:
                pts.append(q)
        if len(pts) > 1 and pts[0] == pts[-1]:
            pts.pop()
        if len(pts) >= 3:
            outers.append(pts)
    if not outers:
        return None
    best = max(outers, key=lambda p: abs(signed_area2(p)))

    def inside(p):
        return any(pip(p, r) for r in outers)

    def q(lon, lat):
        return (int(round(lon * quant)), int(round(lat * quant)))

    # 1. 官方面积质心
    c = props.get("centroid")
    if c:
        p = q(c[0], c[1])
        if inside(p):
            return c[0], c[1]
    # 2. 最大环质心(we shoelace 面积加权)
    ring_pts = best
    a2 = cx = cy = 0.0
    n = len(ring_pts)
    for i in range(n):
        (x0, y0), (x1, y1) = ring_pts[i], ring_pts[(i + 1) % n]
        cross = x0 * y1 - x1 * y0
        a2 += cross
        cx += (x0 + x1) * cross
        cy += (y0 + y1) * cross
    if abs(a2) > 1e-9:
        p = (int(round(cx / (3.0 * a2))), int(round(cy / (3.0 * a2))))
        if inside(p):
            return p[0] / quant, p[1] / quant
    # 3. 最大环顶点中位数
    med = (sorted(p[0] for p in best)[len(best) // 2],
           sorted(p[1] for p in best)[len(best) // 2])
    if inside(med):
        return med[0] / quant, med[1] / quant
    # 4. 政区中心
    s = props.get("center")
    if s:
        return s[0], s[1]
    return med[0] / quant, med[1] / quant


def signed_area2(ring):
    a = 0
    n = len(ring)
    for i in range(n):
        (x0, y0), (x1, y1) = ring[i], ring[(i + 1) % n]
        a += x0 * y1 - x1 * y0
    return a


def build_province_labels(cn_path, quant):
    """DataV 省级标签(kind=2, rank=2:高倍缩放才显示)。

    位置取几何居中点(最大环顶点中位数,PIP 校验),回退质心/政区中心,
    避免标签贴边(见 province_label_pos)。
    """
    labels = []
    with open(cn_path, encoding="utf-8") as f:
        data = json.load(f)
    for feature in data["features"]:
        props = feature["properties"]
        if props.get("level") != "province":
            continue
        zh = (props.get("name") or "").strip()
        if not zh:
            continue
        pos = province_label_pos(feature, quant)
        if pos is None:
            continue
        lon, lat = pos
        dx, dy = PROVINCE_LABEL_NUDGE.get(int(props["adcode"]), (0.0, 0.0))
        lon += dx
        lat += dy
        adcode = int(props["adcode"])
        en = PROVINCE_EN.get(adcode) or zh
        labels.append((zh, en, lon, lat, 2, 2))
    return labels


def build_south_sea_line(cn_path, quant):
    """南海断续国界(十段线):DataV 100000_JD 的 10 段梭形闭合环,取连续边。

    JD 数据即十段线(含台湾东北一段),环数与顶点由数据决定,不写死段数。
    """
    with open(cn_path, encoding="utf-8") as f:
        data = json.load(f)
    segs = []
    for feature in data["features"]:
        if feature["properties"].get("adchar") != "JD":
            continue
        geom = feature["geometry"]
        polys = geom["coordinates"] if geom["type"] == "MultiPolygon" else [geom["coordinates"]]
        for poly in polys:
            for ring in poly:
                pts = []
                for lon, lat in ring:
                    q = (int(round(lon * quant)), int(round(lat * quant)))
                    if not pts or q != pts[-1]:
                        pts.append(q)
                if len(pts) > 1 and pts[0] == pts[-1]:
                    pts.pop()
                for i in range(len(pts)):
                    a = pts[i]
                    b = pts[(i + 1) % len(pts)]
                    if a != b:
                        segs.append((a[0], a[1], b[0], b[1]))
    return segs


def encode_south_sea_line(out, segs):
    encode_varint(out, len(segs))
    for x0, y0, x1, y1 in segs:
        encode_varint(out, zigzag(x0))
        encode_varint(out, zigzag(y0))
        encode_varint(out, zigzag(x1))
        encode_varint(out, zigzag(y1))


def encode_labels(out, labels):
    encode_varint(out, len(labels))
    for zh, en, lon, lat, kind, rank in labels:
        zb = zh.encode("utf-8")
        eb = en.encode("utf-8")
        encode_varint(out, len(zb))
        out.extend(zb)
        encode_varint(out, len(eb))
        out.extend(eb)
        encode_varint(out, zigzag(int(round(lon * QUANT))))
        encode_varint(out, zigzag(int(round(lat * QUANT))))
        out.append(kind)
        out.append(rank)


def main():
    print("building world 110m level ...", file=sys.stderr)
    cn_rings = load_cn_rings(SRC_CN, QUANT)
    # 大湖洞环:当前湖泊层 + 历史层(咸海用历史整体轮廓,剔除南北残存水域)
    lake_rings = (load_lake_rings(SRC_LAKES, QUANT, drop_names=("North Aral Sea", "South Aral Sea"))
                  + load_lake_rings(SRC_LAKES_HISTORIC, QUANT))
    print(f"lakes: {len(lake_rings)} rings", file=sys.stderr)
    world0 = build_level(SRC_110M, QUANT, cn_rings=cn_rings, lake_rings=lake_rings)
    print("building world 50m level (DP binary search) ...", file=sys.stderr)
    world1 = build_level(SRC_50M, QUANT, budget=BUDGET_50M, cn_rings=cn_rings, lake_rings=lake_rings)
    print("building china 110m level ...", file=sys.stderr)
    china0 = build_cn_level(cn_rings, budget=CN_BUDGET_110M)
    print("building china 50m level (DP binary search) ...", file=sys.stderr)
    china1 = build_cn_level(cn_rings, budget=CN_BUDGET_50M)

    out = bytearray()
    out.extend(b"NWLD")
    # version 4:分层渲染,每档 LOD 世界层在前、中国层在后(中国层覆盖)
    out.extend(bytes([4, 2]))
    encode_level(out, world0)
    encode_level(out, china0)
    encode_level(out, world1)
    encode_level(out, china1)

    jds = build_south_sea_line(SRC_CN, QUANT)
    print(f"south sea line: {len(jds)} segs", file=sys.stderr)
    encode_south_sea_line(out, jds)

    labels = build_labels(SRC_50M, SRC_MARINE, QUANT, SRC_CN)
    from collections import Counter
    dist = Counter(rank for *_, rank in labels)
    kinds = Counter(kind for *_, kind, rank in labels)
    print(f"labels: {len(labels)} ranks={dict(sorted(dist.items()))} kinds={dict(sorted(kinds.items()))}", file=sys.stderr)
    encode_labels(out, labels)
    pathlib.Path(OUT_PATH).write_bytes(bytes(out))
    print(f"written {OUT_PATH}: {len(out)} bytes", file=sys.stderr)


if __name__ == "__main__":
    main()
