//! 悬浮球数据聚合:活跃连接按进程汇总实时速率,生成上传/下载双榜。

use std::collections::HashMap;

use eframe::egui;

use super::types::ProcRate;
use super::types::TOP_N;
use crate::model::Connection;
use crate::storage::config::Config;
use crate::ui::conn_visible;

/// 每进程实时速率聚合:按映像名汇总 ETW 每连接速率,生成上传/下载
/// Top-N 两榜;过滤口径与连接列表一致(hide_local/hide_lan)。
/// 未提权时 conn_rates 为空,两榜为空——由绘制层提示
pub fn collect_proc_rates(
    conns: &[Connection],
    conn_rates: &HashMap<u64, (u64, u64)>,
    config: &Config,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
) -> (Vec<ProcRate>, Vec<ProcRate>) {
    // 按映像名聚合 (down, up, 首个可用的进程路径[图标查找键])
    let mut acc: HashMap<&str, (u64, u64, Option<String>)> = HashMap::new();
    for c in conns {
        if !conn_visible(config, c) {
            continue;
        }
        let Some((down, up)) = conn_rates.get(&c.id) else {
            continue;
        };
        let e = acc.entry(c.process.as_str()).or_default();
        e.0 += down;
        e.1 += up;
        if e.2.is_none() {
            e.2 = c.proc_path.clone();
        }
    }

    let mut rows: Vec<ProcRate> = acc
        .into_iter()
        .map(|(name, (down, up, path))| ProcRate {
            name: name.to_owned(),
            icon: path
                .as_deref()
                .and_then(|p| icon_tex.get(p))
                .cloned()
                .flatten(),
            down,
            up,
        })
        .collect();
    let mut up_rows = rows.clone();
    up_rows.sort_by(|a, b| b.up.cmp(&a.up).then_with(|| a.name.cmp(&b.name)));
    up_rows.truncate(TOP_N);
    rows.sort_by(|a, b| b.down.cmp(&a.down).then_with(|| a.name.cmp(&b.name)));
    rows.truncate(TOP_N);
    (up_rows, rows)
}
