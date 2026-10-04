//! 规则页导入导出:当前档持久规则与 JSON 文件互转(rfd 原生模态),
//! 结果以工具栏反馈消息呈现。

use rusqlite::Connection as Db;

use super::{Feedback, PageState};
use crate::i18n::I18n;
use crate::rules::RuleSet;

/// 导出对话框默认文件名
const EXPORT_FILE_NAME: &str = "netowl-rules.json";

/// 导出:原生保存对话框选路径,持久规则写为 JSON 文件
pub(super) fn do_export(rules: &RuleSet, state: &mut PageState, i18n: &I18n) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("JSON", &["json"])
        .set_file_name(EXPORT_FILE_NAME)
        .save_file()
    else {
        return;
    };
    let n = rules.rules.iter().filter(|r| r.id > 0).count();
    let result = std::fs::write(&path, rules.export_json()).map(|_| n);
    state.feedback = Some(match result {
        Ok(n) => {
            tracing::info!("[Rules] 已导出 {n} 条规则到 {}", path.display());
            Feedback::now(
                false,
                i18n.t_with_args("rules-export-done", &[("n", n.to_string())]),
            )
        }
        Err(e) => {
            tracing::warn!("[Rules] 规则导出失败 {}: {e}", path.display());
            Feedback::now(
                true,
                i18n.t_with_args("rules-export-failed", &[("err", e.to_string())]),
            )
        }
    });
}

/// 导入:原生打开对话框选 JSON 文件,规则追加为最低优先级
pub(super) fn do_import(db: &Db, rules: &mut RuleSet, state: &mut PageState, i18n: &I18n) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("JSON", &["json"])
        .pick_file()
    else {
        return;
    };
    let result = std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| rules.import_json(db, &text));
    state.feedback = Some(match result {
        Ok((n, skipped)) => {
            tracing::info!(
                "[Rules] 从 {} 导入 {n} 条规则(跳过 {skipped} 条)",
                path.display()
            );
            Feedback::now(
                false,
                i18n.t_with_args(
                    "rules-import-done",
                    &[("n", n.to_string()), ("skipped", skipped.to_string())],
                ),
            )
        }
        Err(e) => {
            tracing::warn!("[Rules] 从 {} 导入规则失败: {e}", path.display());
            Feedback::now(true, i18n.t_with_args("rules-import-failed", &[("err", e)]))
        }
    });
}
