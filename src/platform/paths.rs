//! 数据根目录:Windows 便携式布局,数据根 = exe 同级目录。
//! 启动时切换工作目录到数据根,此后 config.toml、data/ 等相对路径都落在根内。
//! 后续扩展 macOS/Linux 时按平台补充数据目录规则(见 AGENTS.md 扩展备忘)。

use std::path::PathBuf;

/// 配置文件(相对数据根)
pub const CONFIG_FILE: &str = "config.toml";
/// 数据目录(相对数据根,存放数据库等)
pub const DATA_DIR: &str = "data";
/// 日志目录(相对数据根,存放运行日志 latest.log 与轮转归档)
pub const LOGS_DIR: &str = "logs";

/// 创建数据根并切换工作目录
pub fn init_data_root() {
    let root = app_root_dir();
    if let Err(e) = std::fs::create_dir_all(&root) {
        panic!("[启动] 创建数据目录失败 {}: {e}", root.display());
    }
    if let Err(e) = std::env::set_current_dir(&root) {
        panic!("[启动] 切换工作目录失败 {}: {e}", root.display());
    }
}

/// 数据根:exe 同级目录;取不到 exe 路径时回退当前目录
fn app_root_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 资源管理器打开目录
pub fn open_in_explorer(dir: &std::path::Path) {
    if let Err(e) = std::process::Command::new("explorer").arg(dir).spawn() {
        tracing::warn!("[Paths] 打开目录失败 {}: {e}", dir.display());
    }
}

/// 资源管理器定位文件(选中该文件)
pub fn select_in_explorer(file: &std::path::Path) {
    use std::os::windows::process::CommandExt;

    // /select, 与路径间不能有空格;路径含空格须整体加引号,普通 arg
    // 会对含空格参数自动加引号导致 /select, 被拆开,须 raw_arg
    if let Err(e) = std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", file.display()))
        .spawn()
    {
        tracing::warn!("[Paths] 定位文件失败 {}: {e}", file.display());
    }
}
