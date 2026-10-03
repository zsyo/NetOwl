//! 规则导入导出(JSON 文件):文件不含 id/priority/created_at,条目顺序
//! 即优先顺序;导入按文件顺序逐条追加为最低优先级,事务包裹全进或全不进;
//! 与现有规则或文件内前文语义重复的条目跳过。文件读写与对话框在 UI 层,
//! 本模块只处理文本与规则集之间的转换。

use std::collections::HashSet;

use rusqlite::Connection as Db;
use serde::{Deserialize, Serialize};

use super::profile::{parse_action, parse_direction, parse_proto, parse_remote_kind};
use super::{Action, Direction, RemoteKind, Rule, RuleSet};
use crate::model::Protocol;

/// 当前导出格式版本
const FORMAT_VERSION: u32 = 1;

/// 导出文件顶层结构
#[derive(Serialize, Deserialize)]
pub struct RuleFile {
    pub version: u32,
    pub rules: Vec<RuleEntry>,
}

/// 可序列化规则条目:字段值与库存储同为规范字符串(proto 空串 = 任意协议)
#[derive(Serialize, Deserialize)]
pub struct RuleEntry {
    pub name: String,
    pub enabled: bool,
    pub action: String,
    pub direction: String,
    pub proto: String,
    pub process: String,
    pub remote_kind: String,
    pub remote_value: String,
    pub port: u16,
}

impl RuleSet {
    /// 导出全部持久规则(会话临时规则不含)为 JSON 文本,
    /// 条目按当前优先级顺序(即规则页显示顺序)
    pub fn export_json(&self) -> String {
        let file = RuleFile {
            version: FORMAT_VERSION,
            rules: self
                .rules
                .iter()
                .filter(|r| r.id > 0)
                .map(|r| RuleEntry {
                    name: r.name.clone(),
                    enabled: r.enabled,
                    action: r.action.as_str().to_owned(),
                    direction: r.direction.as_str().to_owned(),
                    proto: r.proto.map(|p| p.as_str()).unwrap_or_default().to_owned(),
                    process: r.process.clone(),
                    remote_kind: r.remote_kind.as_str().to_owned(),
                    remote_value: r.remote_value.clone(),
                    port: r.port,
                })
                .collect(),
        };
        // 纯数据结构,序列化不会失败
        serde_json::to_string_pretty(&file).expect("规则导出序列化失败")
    }

    /// 从 JSON 文本导入规则:条目按文件顺序逐条追加为最低优先级;
    /// 与现有规则或文件内前文语义重复的条目跳过(判定见 dedup_key);
    /// 事务包裹,任一条落库失败则整体回滚(含内存规则集)。
    /// 返回 (导入条数, 跳过的重复条数)
    pub fn import_json(&mut self, db: &Db, text: &str) -> Result<(usize, usize), String> {
        let file: RuleFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if file.version != FORMAT_VERSION {
            return Err(format!("unsupported format version {}", file.version));
        }
        let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
        let base_len = self.rules.len();
        // 已有规则(含临时)全量入集合,边导边补实现文件内去重
        let mut seen: HashSet<DedupKey> = self.rules.iter().map(dedup_key).collect();
        let mut count = 0usize;
        let mut skipped = 0usize;
        for entry in file.rules {
            let rule = Rule {
                id: 0,
                name: entry.name,
                enabled: entry.enabled,
                priority: 0,
                action: parse_action(&entry.action),
                direction: parse_direction(&entry.direction),
                proto: parse_proto(&entry.proto),
                process: entry.process,
                remote_kind: parse_remote_kind(&entry.remote_kind),
                remote_value: entry.remote_value,
                port: entry.port,
                local_port: 0,
            };
            if !seen.insert(dedup_key(&rule)) {
                skipped += 1;
                continue;
            }
            if let Err(e) = self.insert(&tx, rule) {
                self.rules.truncate(base_len);
                return Err(format!("insert failed: {e}"));
            }
            count += 1;
        }
        match tx.commit() {
            Ok(()) => Ok((count, skipped)),
            Err(e) => {
                self.rules.truncate(base_len);
                Err(format!("commit failed: {e}"))
            }
        }
    }
}

/// 导入去重键:除名称/启用态/优先级/id 外的全部语义字段(见 dedup_key)
type DedupKey = (
    Action,
    Direction,
    Option<Protocol>,
    String,
    RemoteKind,
    String,
    u16,
    u16,
);

/// 导入去重键:除名称/启用态/优先级/id 外的全部语义字段。进程与域名
/// 大小写不敏感(与匹配语义一致)统一小写;动作不同不视为重复。
/// 网段按原文比较——同数据重导场景均为原样字符串,不做解析归一
fn dedup_key(r: &Rule) -> DedupKey {
    (
        r.action,
        r.direction,
        r.proto,
        r.process.to_lowercase(),
        r.remote_kind,
        match r.remote_kind {
            RemoteKind::Domain => r.remote_value.to_lowercase(),
            _ => r.remote_value.clone(),
        },
        r.port,
        r.local_port,
    )
}
