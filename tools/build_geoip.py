#!/usr/bin/env python3
# 构建 GeoIP 归属定位数据 -> assets/geoip.bin(城市级粒度)
#
# 数据源(均放 tools/cache/,不入库;缺失时脚本自行下载):
#   ip2region_v4.xdb                  - ip2region v4(IPv4 归属,Apache-2.0),region = 国家|省|市|机构|ISO
#   ip2region_v6.xdb                  - ip2region v6(IPv6 归属,Apache-2.0),region 同构五段
#   100000_full.json                  - DataV 中国省级(名称/质心/子级 adcode)
#   {adcode}_full.json                - DataV 各省 children(地级市名称/质心,34 个,缺失时下载)
#   ne_50m_admin_0_countries.geojson  - Natural Earth 国家(ISO_A2/中文名/标签点,与底图同源)
#   cities15000.zip                   - GeoNames 全球城市(公有领域,name/ascii/坐标/人口)
#
# 归属粒度:中国(含港澳台,数据源均标"中国")到地级市(直辖市/港澳为省级本身,
# 无市级信息的段回退省级);外国城市按 (ISO, 城市名) 匹配 GeoNames 命中则到城市,
# 否则回退国家级;私网/保留段无归属。显示名双语内嵌:中国城市 zh=市名,
# en=GeoNames 最近邻英文名(未命中则同中文);外国城市 zh/en 均为 GeoNames 英文名。
#
# 输出格式(NWGI v3 = v2 布局尾部追加 IPv6 两段):
#   magic "NWGI" | u16 version | u32 loc_count
#   locs[]: u8 zh_len + zh | u8 en_len + en | f32 lon | f32 lat
#   u32 range_count
#   ranges[](按 start_ip 升序):LEB128 距上区间末尾距离 | 区间长度 | zigzag(loc_idx delta)
#   u32 org_count
#   orgs[]: u8 len + utf-8(运营商/机构名,单语言直显)
#   u32 isp_range_count
#   isp_ranges[](按 start_ip 升序):LEB128 同款(delta | 长度 | zigzag(org_idx delta))
#   u32 range6_count
#   range6[](u128 版同款编码:start/length 为 u128 LEB128,loc delta 同款)
#   u32 isp_range6_count
#   isp_range6[](同 range6,值指向 orgs)
#
# ISP 与城市边界独立:org 变化打断合并,同一城市不同运营商各成区间;
# 机构字段为 "0"/空串的段(多为国外)不进 ISP 区间表,查询返回空。

import json
import struct
import sys
import urllib.request
import zipfile
from collections import Counter
from pathlib import Path

CACHE = Path(__file__).parent / "cache"
OUT = Path(__file__).parent.parent / "assets" / "geoip.bin"

MAGIC = b"NWGI"
VERSION = 3

DATAV_BASE = "https://geo.datav.aliyun.com/areas_v3/bound/{}_full.json"
GEONAMES_URL = "http://download.geonames.org/export/dump/cities15000.zip"

# 34 省级行政区英文名(规范化键 -> 英文),DataV 中文名一一对应
PROVINCE_EN = {
    "北京": "Beijing", "天津": "Tianjin", "河北": "Hebei", "山西": "Shanxi",
    "内蒙古": "Inner Mongolia", "辽宁": "Liaoning", "吉林": "Jilin",
    "黑龙江": "Heilongjiang", "上海": "Shanghai", "江苏": "Jiangsu",
    "浙江": "Zhejiang", "安徽": "Anhui", "福建": "Fujian", "江西": "Jiangxi",
    "山东": "Shandong", "河南": "Henan", "湖北": "Hubei", "湖南": "Hunan",
    "广东": "Guangdong", "广西": "Guangxi", "海南": "Hainan", "重庆": "Chongqing",
    "四川": "Sichuan", "贵州": "Guizhou", "云南": "Yunnan", "西藏": "Tibet",
    "陕西": "Shaanxi", "甘肃": "Gansu", "青海": "Qinghai", "宁夏": "Ningxia",
    "新疆": "Xinjiang", "香港": "Hong Kong", "澳门": "Macau", "台湾": "Taiwan",
}

REGION_SUFFIXES = ["特别行政区", "维吾尔自治区", "回族自治区", "壮族自治区", "自治区", "省", "市"]
# 外国城市名的行政区修饰,匹配 GeoNames 前尝试剥离
FOREIGN_CITY_TRIM = [" kommun", " shi", " county", " municipality"]


def norm_region(name):
    for suf in REGION_SUFFIXES:
        if name.endswith(suf):
            return name[: -len(suf)]
    return name


def fetch(url, dest):
    if dest.exists():
        return dest
    print(f"  下载 {url}")
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(dest.suffix + ".part")
    try:
        urllib.request.urlretrieve(url, tmp)
    except urllib.error.HTTPError as e:
        print(f"  下载失败({e.code}),跳过: {url}")
        return None
    tmp.rename(dest)
    return dest


def load_provinces():
    """DataV 34 省:规范化名 -> (adcode, zh, en, lon, lat)"""
    data = json.loads((CACHE / "100000_full.json").read_text(encoding="utf-8"))
    table = {}
    for feat in data["features"]:
        props = feat["properties"]
        name = props.get("name")
        pos = props.get("centroid") or props.get("center")
        if not name or not pos:
            continue
        key = norm_region(name)
        en = PROVINCE_EN.get(key)
        if en is None:
            raise SystemExit(f"省份缺少英文名映射: {name}")
        table[key] = (props["adcode"], name, en, float(pos[0]), float(pos[1]))
    if len(table) != len(PROVINCE_EN):
        raise SystemExit(f"省份数不符: {len(table)} != {len(PROVINCE_EN)}")
    return table


def load_cn_cities(provinces):
    """中国地级市:规范化 (省名, 市名) -> (zh, en, lon, lat)。
    直辖市/港澳的 children 为区级,不生成市级条目(城市即省级本身);
    台湾省 children 为市。英文名取 GeoNames 中国城市最近邻(距离 < 0.55 度)。"""
    gn_cn = load_geonames().get("CN", {})
    table = {}
    for prov_key, (adcode, _zh, _en, _lon, _lat) in provinces.items():
        path = fetch(DATAV_BASE.format(adcode), CACHE / f"{adcode}_full.json")
        if path is None:
            continue
        data = json.loads(path.read_text(encoding="utf-8"))
        for feat in data["features"]:
            props = feat["properties"]
            name = props.get("name")
            # 城市点位优先取政府驻地(center),几何质心在大市域会偏离市区
            pos = props.get("center") or props.get("centroid")
            if not name or not pos or props.get("level") != "city":
                continue
            lon, lat = float(pos[0]), float(pos[1])
            en = nearest_name(gn_cn, lon, lat) or name
            table[(prov_key, norm_region(name))] = (name, en, lon, lat)
    return table


def load_geonames():
    """GeoNames:(iso, {name_lower, ascii_lower}) -> (en, lat, lon, pop),同名取人口最多"""
    fetch(GEONAMES_URL, CACHE / "cities15000.zip")
    z = zipfile.ZipFile(CACHE / "cities15000.zip")
    table = {}
    with z.open("cities15000.txt") as f:
        for line in f:
            c = line.decode("utf-8").rstrip("\n").split("\t")
            if len(c) < 15 or not c[8]:
                continue
            iso = c[8]
            pop = int(c[14]) if c[14] else 0
            entry = (c[1], float(c[4]), float(c[5]), pop)
            for nm in {c[1].lower(), c[2].lower()}:
                if nm:
                    key = (iso, nm)
                    if key not in table or pop > table[key][3]:
                        table[key] = entry
    return table


def nearest_name(gn_cities, lon, lat):
    """GeoNames 城市表中找最近邻英文名(平方度距离)"""
    best, best_d = None, 0.55 * 0.55
    for en, glat, glon, _pop in gn_cities.values():
        d = (glat - lat) ** 2 + (glon - lon) ** 2
        if d < best_d:
            best, best_d = en, d
    return best


def load_countries():
    """NE 国家:ISO_A2 -> (zh, en, lon, lat)"""
    data = json.loads((CACHE / "ne_50m_admin_0_countries.geojson").read_text(encoding="utf-8"))
    table = {}
    for feat in data["features"]:
        props = feat["properties"]
        iso = props.get("ISO_A2_EH") or props.get("ISO_A2")
        if not iso or iso == "-99" or "LABEL_X" not in props:
            continue
        table[iso] = (props["NAME_ZH"], props["NAME"], float(props["LABEL_X"]), float(props["LABEL_Y"]))
    return table


def parse_xdb():
    """遍历 xdb 索引区全部记录 -> [(start_ip, end_ip, region)]"""
    data = (CACHE / "ip2region_v4.xdb").read_bytes()
    ver, _policy, _ctime, istart, iend = struct.unpack_from("<HHIII", data, 0)
    if ver != 3:
        raise SystemExit(f"不支持的 xdb 版本: {ver}")
    out = []
    p = istart
    while p + 14 <= iend:
        sip, eip, dlen, dptr = struct.unpack_from("<IIHI", data, p)
        out.append((sip, eip, data[dptr : dptr + dlen].decode("utf-8")))
        p += 14
    return out


def parse_xdb6():
    """遍历 IPv6 xdb 索引区全部记录 -> [(start_ip, end_ip, region)](u128)。
    xdb 3.0 结构(官方 maker/rust header.rs 与 searcher.rs 定案):头部
    16 字节(start_index_ptr@8 / end_index_ptr@12,半开区间)+ 512KB
    向量索引;索引项 38 字节 = sip(16) + eip(16) + dlen(u16 LE) +
    dptr(u32 LE),IP 为网络序大端字节,直接按数值比较"""
    data = (CACHE / "ip2region_v6.xdb").read_bytes()
    ver, _policy, _ctime, istart, iend = struct.unpack_from("<HHIII", data, 0)
    ipver = struct.unpack_from("<H", data, 16)[0]
    if ver != 3 or ipver != 6:
        raise SystemExit(f"不支持的 IPv6 xdb: ver={ver} ipver={ipver}")
    out = []
    p = istart
    while p + 38 <= iend:
        sip = int.from_bytes(data[p : p + 16], "big")
        eip = int.from_bytes(data[p + 16 : p + 32], "big")
        dlen, dptr = struct.unpack_from("<HI", data, p + 32)
        out.append((sip, eip, data[dptr : dptr + dlen].decode("utf-8")))
        p += 38
    return out


def foreign_city_key(iso, city, geonames):
    """外国城市匹配:原名 -> 逗号前段 -> 剥离行政区修饰"""
    n = city.strip().lower()
    candidates = [n]
    if "," in n:
        candidates.append(n.split(",")[0].strip())
    for trim in FOREIGN_CITY_TRIM:
        if candidates[-1].endswith(trim):
            candidates.append(candidates[-1][: -len(trim)].strip())
    for key in candidates:
        if key and (iso, key) in geonames:
            return (iso, key)
    return None


def write_varint(buf, value):
    while True:
        b = value & 0x7F
        value >>= 7
        if value:
            buf.append(b | 0x80)
        else:
            buf.append(b)
            break


def zigzag(value):
    return (value << 1) ^ (value >> 31)


def main():
    provinces = load_provinces()
    cn_cities = load_cn_cities(provinces)
    geonames = load_geonames()
    countries = load_countries()

    places = []          # [(zh, en, lon, lat)]
    place_index = {}     # 键 -> 位置索引(去重,v4/v6 共用)
    orgs = []            # 运营商/机构名表(去重)
    org_index = {}       # 名 -> org 索引

    def place(key, zh, en, lon, lat):
        if key not in place_index:
            place_index[key] = len(places)
            places.append((zh, en, lon, lat))
        return place_index[key]

    def cn_national():
        nat = countries.get("CN")
        if nat is None:
            raise SystemExit("NE 数据缺少中国")
        return place(("nat", "CN"), nat[0], nat[1], nat[2], nat[3])

    def classify(records):
        """region 五段 -> 位置区间与 ISP 区间;v4/v6 共用同一套
        位置表与机构表(xdb v6 的 region 与 v4 同构)"""
        ranges = []          # [(start, end, loc_idx)]
        isp_ranges = []      # [(start, end, org_idx)]
        stats = Counter()    # 归属粒度统计
        for sip, eip, region in records:
            country, province, city, org, iso = (region.split("|") + ["0"] * 5)[:5]
            if country == "中国":
                prov_key = norm_region(province) if province != "0" else None
                city_key = norm_region(city) if city != "0" else None
                if prov_key and city_key and (prov_key, city_key) in cn_cities:
                    zh, en, lon, lat = cn_cities[(prov_key, city_key)]
                    loc = place(("cn-city", prov_key, city_key), zh, en, lon, lat)
                    stats["中国-城市"] += 1
                elif prov_key and prov_key in provinces:
                    _, zh, en, lon, lat = provinces[prov_key]
                    loc = place(("cn-prov", prov_key), zh, en, lon, lat)
                    stats["中国-省级回退"] += 1
                else:
                    loc = cn_national()
                    stats["中国-国家级回退"] += 1
            elif iso != "0":
                city_hit = foreign_city_key(iso, city, geonames) if city != "0" else None
                if city_hit is not None:
                    en, lat, lon, _pop = geonames[city_hit]
                    loc = place(("intl-city",) + city_hit, en, en, lon, lat)
                    stats["外国-城市"] += 1
                elif iso in countries:
                    zh, en, lon, lat = countries[iso]
                    loc = place(("nat", iso), zh, en, lon, lat)
                    stats["外国-国家级回退"] += 1
                else:
                    stats[f"跳过|{country}|{iso}"] += 1
                    continue
            else:
                stats["跳过|保留段"] += 1
                continue
            ranges.append((sip, eip, loc))
            # 运营商/机构:占位 "0"/空串表示数据源无此信息,不进 ISP 区间表
            org = org.strip()
            if org and org != "0":
                if org not in org_index:
                    org_index[org] = len(orgs)
                    orgs.append(org[:255])
                isp_ranges.append((sip, eip, org_index[org]))
        return ranges, isp_ranges, stats

    def merge(ranges):
        """合并相邻且归属相同的区间(xdb 索引段之间不重叠)"""
        out = []
        for s, e, v in ranges:
            if out and out[-1][1] + 1 == s and out[-1][2] == v:
                out[-1] = (out[-1][0], e, v)
            else:
                out.append((s, e, v))
        return out

    records = parse_xdb()
    ranges, isp_ranges, stats = classify(records)
    records6 = parse_xdb6()
    ranges6, isp_ranges6, stats6 = classify(records6)

    merged = merge(sorted(ranges))
    isp_merged = merge(sorted(isp_ranges))
    merged6 = merge(sorted(ranges6))
    isp_merged6 = merge(sorted(isp_ranges6))

    # 序列化
    buf = bytearray()
    buf += MAGIC
    buf += struct.pack("<H", VERSION)
    buf += struct.pack("<I", len(places))
    for zh, en, lon, lat in places:
        zh_b = zh.encode("utf-8")
        en_b = en.encode("utf-8")
        buf.append(len(zh_b))
        buf += zh_b
        buf.append(len(en_b))
        buf += en_b
        buf += struct.pack("<ff", lon, lat)
    buf += struct.pack("<I", len(merged))
    prev_end = 0
    prev_idx = 0
    for s, e, loc in merged:
        write_varint(buf, s - prev_end)
        write_varint(buf, e - s + 1)
        write_varint(buf, zigzag(loc - prev_idx))
        prev_end = (e + 1) & 0xFFFFFFFF
        prev_idx = loc

    buf += struct.pack("<I", len(orgs))
    for org in orgs:
        ob = org.encode("utf-8")
        buf.append(len(ob))
        buf += ob
    buf += struct.pack("<I", len(isp_merged))
    prev_end = 0
    prev_idx = 0
    for s, e, o in isp_merged:
        write_varint(buf, s - prev_end)
        write_varint(buf, e - s + 1)
        write_varint(buf, zigzag(o - prev_idx))
        prev_end = (e + 1) & 0xFFFFFFFF
        prev_idx = o

    # IPv6 区间表(u128 编码;loc/org delta 仍是位置表索引,复用 i32 zigzag)
    buf += struct.pack("<I", len(merged6))
    prev_end = 0
    prev_idx = 0
    for s, e, loc in merged6:
        write_varint(buf, s - prev_end)
        write_varint(buf, e - s + 1)
        write_varint(buf, zigzag(loc - prev_idx))
        prev_end = e + 1
        prev_idx = loc
    buf += struct.pack("<I", len(isp_merged6))
    prev_end = 0
    prev_idx = 0
    for s, e, o in isp_merged6:
        write_varint(buf, s - prev_end)
        write_varint(buf, e - s + 1)
        write_varint(buf, zigzag(o - prev_idx))
        prev_end = e + 1
        prev_idx = o

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_bytes(buf)

    print(f"places: {len(places)}")
    print(f"v4 ranges: {len(records)} -> 合并 {len(merged)}")
    print(f"v4 isp: ranges {len(isp_ranges)} -> 合并 {len(isp_merged)}")
    print(f"v6 ranges: {len(records6)} -> 合并 {len(merged6)}")
    print(f"v6 isp: ranges {len(isp_ranges6)} -> 合并 {len(isp_merged6)}")
    print(f"output: {OUT} {len(buf)} bytes")
    print("运营商分布(v4):")
    org_counter = Counter(o for _, _, o in isp_ranges)
    for name, cnt in sorted(((orgs[i], c) for i, c in org_counter.items()), key=lambda kv: -kv[1])[:10]:
        print(f"  {name}: {cnt} 段")
    print("归属粒度 v4:")
    for key, cnt in stats.most_common(20):
        print(f"  {key}: {cnt}")
    print("归属粒度 v6:")
    for key, cnt in stats6.most_common(20):
        print(f"  {key}: {cnt}")


if __name__ == "__main__":
    sys.exit(main())
