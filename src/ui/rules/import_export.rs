//! 规则页导入导出:当前档持久规则与 JSON 文件互转。原生文件对话框与
//! 文件 IO 在工作线程执行——模态对话框开多久,同步调用就阻塞主循环
//! 多久(采集/速率采样/动画停摆);结果经 mpsc 回主线程收割为工具栏
//! 反馈消息,收割见 poll_file_job(每帧调用)。

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use rusqlite::Connection as Db;

use super::{Feedback, PageState};
use crate::i18n::I18n;
use crate::rules::RuleSet;

/// 导出对话框默认文件名
const EXPORT_FILE_NAME: &str = "netowl-rules.json";

/// 在途导入/导出任务(存在 = 任务在途)。用户取消对话框时工作线程不发
/// 消息直接退出,收割端按通道断开静默清理
pub(in crate::ui) enum FileJob {
    /// 导出:条数(派发时已定)+ (路径, 写文件结果)
    Export {
        n: usize,
        rx: Receiver<Option<(PathBuf, Result<(), String>)>>,
    },
    /// 导入:(路径, 文件内容;读取失败为 Err 摘要)
    Import(Receiver<Option<(PathBuf, Result<String, String>)>>),
}

/// 导出:持久规则 JSON 在主线程序列化(借用 rules),对话框与写文件
/// 在工作线程执行
pub(super) fn do_export(rules: &RuleSet, state: &mut PageState) {
    let n = rules.rules.iter().filter(|r| r.id > 0).count();
    let json = rules.export_json();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name(EXPORT_FILE_NAME)
            .save_file()
        else {
            return;
        };
        let result = std::fs::write(&path, json).map_err(|e| e.to_string());
        // 发送失败 = 任务已被替换(接收端丢弃),静默退出
        let _ = tx.send(Some((path, result)));
    });
    state.file_job = Some(FileJob::Export { n, rx });
}

/// 导入:文件选择与读取在工作线程执行,落库(rules 可变借用)回主线程
pub(super) fn do_import(state: &mut PageState) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .pick_file()
        else {
            return;
        };
        let result = std::fs::read_to_string(&path).map_err(|e| e.to_string());
        let _ = tx.send(Some((path, result)));
    });
    state.file_job = Some(FileJob::Import(rx));
}

/// 收割在途任务:导出按结果直接反馈;导入在主线程落库后反馈。
/// 在途(Empty)时放回原任务,下帧继续
pub(super) fn poll_file_job(db: &Db, rules: &mut RuleSet, state: &mut PageState, i18n: &I18n) {
    use std::sync::mpsc::TryRecvError;
    let Some(job) = state.file_job.take() else {
        return;
    };
    let mut pending = None;
    match job {
        FileJob::Export { n, rx } => match rx.try_recv() {
            Ok(Some((path, Ok(())))) => {
                tracing::info!("[Rules] 已导出 {n} 条规则到 {}", path.display());
                state.feedback = Some(Feedback::now(
                    false,
                    i18n.t_with_args("rules-export-done", &[("n", n.to_string())]),
                ));
            }
            Ok(Some((path, Err(e)))) => {
                tracing::warn!("[Rules] 规则导出失败 {}: {e}", path.display());
                state.feedback = Some(Feedback::now(
                    true,
                    i18n.t_with_args("rules-export-failed", &[("err", e)]),
                ));
            }
            Ok(None) => {}
            // 在途:放回下帧继续;断开 = 对话框取消(线程未发消息退出)
            Err(TryRecvError::Empty) => pending = Some(FileJob::Export { n, rx }),
            Err(TryRecvError::Disconnected) => {}
        },
        FileJob::Import(rx) => match rx.try_recv() {
            Ok(Some((path, Ok(text)))) => match rules.import_json(db, &text) {
                Ok((n, skipped)) => {
                    tracing::info!(
                        "[Rules] 从 {} 导入 {n} 条规则(跳过 {skipped} 条)",
                        path.display()
                    );
                    state.feedback = Some(Feedback::now(
                        false,
                        i18n.t_with_args(
                            "rules-import-done",
                            &[("n", n.to_string()), ("skipped", skipped.to_string())],
                        ),
                    ));
                }
                Err(e) => {
                    tracing::warn!("[Rules] 从 {} 导入规则失败: {e}", path.display());
                    state.feedback = Some(Feedback::now(
                        true,
                        i18n.t_with_args("rules-import-failed", &[("err", e)]),
                    ));
                }
            },
            Ok(Some((path, Err(e)))) => {
                tracing::warn!("[Rules] 读取导入文件 {} 失败: {e}", path.display());
                state.feedback = Some(Feedback::now(
                    true,
                    i18n.t_with_args("rules-import-failed", &[("err", e)]),
                ));
            }
            Ok(None) => {}
            Err(TryRecvError::Empty) => pending = Some(FileJob::Import(rx)),
            Err(TryRecvError::Disconnected) => {}
        },
    }
    state.file_job = pending;
}
