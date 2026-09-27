//! 简单多边形三角化(耳剪法):底图陆地与洞环的填充用。
//!
//! 输入为量化坐标(0.001 度整数):几何判定全部走整数运算,共线/包含
//! 关系精确无误差。坐标经 f32 存储会破坏共线性,导致量化数据中大量
//! 压边点被误判为耳内阻挡,故此处不接受浮点输入。
//! 环方向任意,内部统一为逆时针;耳剪以双向链表维护存活顶点,剪除一耳
//! 只需复查其两个邻居,单环 O(n^2),对本项目最大环(约 2000 顶点)足够。
//! 数值退化导致整圈无耳时以扇形三角化兜底,保证终止。

/// 耳剪三角化:返回三角形顶点索引(指向输入序列)三元组列表。
pub fn triangulate(pts: &[(i32, i32)]) -> Vec<[u32; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }

    // 环有符号面积(2 倍);顺时针时反转遍历序,统一为逆时针
    let mut order: Vec<u32> = (0..n as u32).collect();
    if signed_area2(pts) < 0 {
        order.reverse();
    }

    let mut prev = vec![0u32; n];
    let mut next = vec![0u32; n];
    for i in 0..n {
        prev[order[i] as usize] = order[(i + n - 1) % n];
        next[order[i] as usize] = order[(i + 1) % n];
    }

    let mut alive = vec![true; n];
    let mut tris = Vec::with_capacity(n / 2);
    let mut cur = order[0];
    let mut scans_without_clip = 0u32;

    while alive.iter().filter(|a| **a).count() > 3 {
        let (a, b, c) = (prev[cur as usize], cur, next[cur as usize]);
        let cross = cross3(pts, a, b, c);
        if cross > 0 && ear_clear(pts, a, b, c, &alive) {
            tris.push([a, b, c]);
            next[a as usize] = c;
            prev[c as usize] = a;
            alive[b as usize] = false;
            scans_without_clip = 0;
            cur = a; // 从剪除处继续,剪耳通常在邻域产生新耳
        } else if cross == 0 && collinear_between(pts, a, b, c) {
            // 共线退化点位于 a-c 之间:安全剔除,不产生三角形
            next[a as usize] = c;
            prev[c as usize] = a;
            alive[b as usize] = false;
            scans_without_clip = 0;
            cur = a;
        } else {
            cur = next[cur as usize];
            scans_without_clip += 1;
            if scans_without_clip >= n as u32 {
                // 数值退化:整圈无耳,扇形兜底后立即返回(否则会反复兜底)
                #[cfg(debug_assertions)]
                eprintln!(
                    "[triangulate] fan fallback: alive={} pts={}",
                    alive.iter().filter(|a| **a).count(),
                    n
                );
                fan(&alive, &next, &mut tris);
                return tris;
            }
        }
    }

    let alive_left = alive.iter().filter(|a| **a).count();
    if alive_left == 3 {
        let mut v = cur;
        while !alive[v as usize] {
            v = (v + 1) % n as u32;
        }
        let a = v;
        let b = next[v as usize];
        let c = next[b as usize];
        tris.push([a, b, c]);
    } else {
        fan(&alive, &next, &mut tris);
    }
    tris
}

/// 兜底扇形三角化:从第一个存活顶点按链表顺序扇开(仅防死循环,允许退化)
fn fan(alive: &[bool], next: &[u32], tris: &mut Vec<[u32; 3]>) {
    let n = alive.len();
    let mut start = 0usize;
    while start < n && !alive[start] {
        start += 1;
    }
    if start >= n {
        return;
    }
    let mut chain: Vec<u32> = Vec::new();
    let mut p = start as u32;
    loop {
        chain.push(p);
        p = next[p as usize];
        if p as usize == start || chain.len() > n {
            break;
        }
    }
    if chain.len() < 3 {
        return;
    }
    let a = chain[0];
    for i in 1..chain.len() - 1 {
        tris.push([a, chain[i], chain[i + 1]]);
    }
}

/// 环有符号面积(2 倍,i64 防溢出)
fn signed_area2(pts: &[(i32, i32)]) -> i64 {
    let n = pts.len();
    let mut sum = 0i64;
    for i in 0..n {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % n];
        sum += (x0 as i64) * (y1 as i64) - (x1 as i64) * (y0 as i64);
    }
    sum
}

/// (a,b,c) 逆时针序下的叉积:正=左转(凸)
fn cross3(pts: &[(i32, i32)], a: u32, b: u32, c: u32) -> i64 {
    let (ax, ay) = pts[a as usize];
    let (bx, by) = pts[b as usize];
    let (cx, cy) = pts[c as usize];
    ((bx as i64 - ax as i64) * (cy as i64 - ay as i64))
        - ((by as i64 - ay as i64) * (cx as i64 - ax as i64))
}

/// 耳空测试:除耳自身三顶点外,无其他存活顶点落入耳三角形。
/// 采用闭三角形测试(含边界):压边的顶点必须视为阻挡,否则剪除会
/// 使多边形折叠自交(实测 Natural Earth 环上会卡死在自交残环)。
fn ear_clear(pts: &[(i32, i32)], a: u32, b: u32, c: u32, alive: &[bool]) -> bool {
    let (ax, ay) = pts[a as usize];
    let (bx, by) = pts[b as usize];
    let (cx, cy) = pts[c as usize];
    let (min_x, max_x) = (ax.min(bx).min(cx), ax.max(bx).max(cx));
    let (min_y, max_y) = (ay.min(by).min(cy), ay.max(by).max(cy));

    for (i, &(px, py)) in pts.iter().enumerate() {
        let i = i as u32;
        if i == a || i == b || i == c || !alive[i as usize] {
            continue;
        }
        if px < min_x || px > max_x || py < min_y || py > max_y {
            continue;
        }
        if point_in_tri((px, py), (ax, ay), (bx, by), (cx, cy)) {
            return false;
        }
    }
    true
}

/// 点是否在逆时针三角形内(含边界)
fn point_in_tri(p: (i32, i32), a: (i32, i32), b: (i32, i32), c: (i32, i32)) -> bool {
    let (px, py) = (p.0 as i64, p.1 as i64);
    let d1 = ((b.0 as i64 - a.0 as i64) * (py - a.1 as i64))
        - ((b.1 as i64 - a.1 as i64) * (px - a.0 as i64));
    let d2 = ((c.0 as i64 - b.0 as i64) * (py - b.1 as i64))
        - ((c.1 as i64 - b.1 as i64) * (px - b.0 as i64));
    let d3 = ((a.0 as i64 - c.0 as i64) * (py - c.1 as i64))
        - ((a.1 as i64 - c.1 as i64) * (px - c.0 as i64));
    d1 >= 0 && d2 >= 0 && d3 >= 0
}

/// 共线时 b 是否位于 a、c 之间(点积投影测试)
fn collinear_between(pts: &[(i32, i32)], a: u32, b: u32, c: u32) -> bool {
    let (ax, ay) = (pts[a as usize].0 as i64, pts[a as usize].1 as i64);
    let (bx, by) = (pts[b as usize].0 as i64, pts[b as usize].1 as i64);
    let (cx, cy) = (pts[c as usize].0 as i64, pts[c as usize].1 as i64);
    let (acx, acy) = (cx - ax, cy - ay);
    let dot = (bx - ax) * acx + (by - ay) * acy;
    dot >= 0 && dot <= acx * acx + acy * acy
}
