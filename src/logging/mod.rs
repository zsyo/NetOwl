//! 日志基础设施:tracing + EnvFilter(AGENTS.md 日志规范)。
//!
//! 三条输出通道互不影响:控制台(stderr)、文件(logs/latest.log,设置页
//! 可开关)、内存层(日志浏览窗口数据源,见 window_layer)。档位经 reload
//! 句柄运行时调整(设置页);RUST_LOG 环境变量在启动时优先于配置档位。
//! 默认应用自身 info、其余(第三方库)warn,避免依赖树噪音。
//!
//! 级别语义(全项目统一,排查时按 debug → trace 逐级展开):
//! - ERROR:最终失败,功能不可用且无恢复
//! - WARN:可恢复失败的事实一句话(细节不进 WARN,避免双记)
//! - INFO:状态迁移与用户可见动作的结果(默认档,保持少量)
//! - DEBUG:操作流转细节与失败细节——机制选择、外部命令行与退出码、
//!   注册表操作结果、分支决策、配置写盘、设置项变更
//! - TRACE:高频细节与帧事件——页面切换、重绘节奏切换、帧耗时统计

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use tracing_subscriber::{
    EnvFilter, Layer, Registry, fmt, fmt::format::Writer, fmt::time::FormatTime,
    fmt::writer::MakeWriter, layer::Layered, layer::SubscriberExt, reload, util::SubscriberInitExt,
};
use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::platform::paths;

pub mod window_layer;

pub use window_layer::LogLine;

/// 文件输出层的类型擦除别名(层本体固定挂载,开关经写入器动态切换)
type FileLayer = Box<dyn Layer<Registry> + Send + Sync>;
/// 文件层挂载后的组合 subscriber 类型(控制台层的 inner subscriber)
type FileSubscriber = Layered<FileLayer, Registry>;

/// 控制台层日志档位句柄,设置页调整档位时 reload
static CONSOLE_FILTER: OnceLock<reload::Handle<EnvFilter, FileSubscriber>> = OnceLock::new();
/// 文件层日志档位句柄,设置页调整档位时 reload
static FILE_FILTER: OnceLock<reload::Handle<EnvFilter, Registry>> = OnceLock::new();
/// 文件异步写入器:Some 表示运行日志开启,None 表示关闭(写入被跳过)
static FILE_WRITER: OnceLock<Mutex<Option<NonBlocking>>> = OnceLock::new();
/// 文件写线程凭证;关闭文件日志时 drop 以落盘缓冲
static FILE_GUARD: OnceLock<Mutex<Option<WorkerGuard>>> = OnceLock::new();
/// 文件日志当前是否处于开启状态
static FILE_ENABLED: AtomicBool = AtomicBool::new(false);

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

/// 日志级别档位(设置页下拉;off 关闭全部输出)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub const ALL: [LogLevel; 6] = [
        LogLevel::Off,
        LogLevel::Error,
        LogLevel::Warn,
        LogLevel::Info,
        LogLevel::Debug,
        LogLevel::Trace,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Off => "off",
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }

    /// 配置文件字符串解析;未知值回退 info
    pub fn parse(s: &str) -> LogLevel {
        LogLevel::ALL
            .into_iter()
            .find(|l| l.as_str() == s)
            .unwrap_or(LogLevel::Info)
    }
}

/// 初始化全局日志;必须在启动早期调用且全程仅一次。
/// 控制台档位:RUST_LOG 环境变量优先(面向开发者调试),否则配置档位;
/// 文件层恒挂载,写入行为由开关决定
pub fn init(level: LogLevel, file_enabled: bool) {
    let (file_reload, file_handle) = reload::Layer::new(level_filter(level));
    let file_layer: FileLayer = Box::new(
        fmt::layer()
            .with_target(false)
            .with_file(true)
            .with_line_number(true)
            .with_thread_ids(false)
            .with_ansi(false)
            .with_timer(LocalTimer)
            .with_writer(ToggleableWriter)
            .with_filter(file_reload),
    );

    let (console_reload, console_handle) = reload::Layer::new(startup_filter(level));
    let console_layer = fmt::layer()
        .with_target(false)
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(false)
        .with_timer(LocalTimer)
        .with_filter(console_reload);

    // 层的先后顺序不影响过滤行为(每层独立过滤);文件层先挂使其
    // reload 句柄类型保持简单(见 FileSubscriber 别名)
    tracing_subscriber::registry()
        .with(file_layer)
        .with(console_layer)
        .with(window_layer::MemoryLayer)
        .init();

    if CONSOLE_FILTER.set(console_handle).is_err() || FILE_FILTER.set(file_handle).is_err() {
        panic!("logger 只能初始化一次");
    }

    if file_enabled {
        let (writer, guard) = build_file_writer();
        *file_writer_cell().lock().unwrap_or_else(|e| e.into_inner()) = Some(writer);
        *file_guard_cell().lock().unwrap_or_else(|e| e.into_inner()) = Some(guard);
    }
    FILE_ENABLED.store(file_enabled, Ordering::Release);
}

/// 运行时调整日志档位(设置页),控制台与文件层同时生效
pub fn set_level(level: LogLevel) {
    if let Some(handle) = CONSOLE_FILTER.get() {
        handle
            .reload(level_filter(level))
            .expect("console 日志过滤器 reload 失败");
    }
    if let Some(handle) = FILE_FILTER.get() {
        handle
            .reload(level_filter(level))
            .expect("file 日志过滤器 reload 失败");
    }
}

/// 开关文件日志;由关闭转为开启时自动轮转旧日志文件
pub fn set_file_enabled(enable: bool) {
    if FILE_ENABLED.swap(enable, Ordering::Release) == enable {
        return;
    }
    if enable {
        // 先停掉旧写线程关闭文件句柄,否则 Windows 下轮转重命名会因文件占用失败
        *file_guard_cell().lock().unwrap_or_else(|e| e.into_inner()) = None;
        let (writer, guard) = build_file_writer();
        *file_writer_cell().lock().unwrap_or_else(|e| e.into_inner()) = Some(writer);
        *file_guard_cell().lock().unwrap_or_else(|e| e.into_inner()) = Some(guard);
    } else {
        // 先卸下写入器停止写入,再 drop 写线程凭证落盘剩余缓冲
        *file_writer_cell().lock().unwrap_or_else(|e| e.into_inner()) = None;
        *file_guard_cell().lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    tracing::info!("[Log] 文件日志已{}", if enable { "开启" } else { "关闭" });
}

/// 进程退出前调用:落盘文件日志缓冲并停止写线程
pub fn flush() {
    if let Some(cell) = FILE_GUARD.get() {
        *cell.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

fn file_writer_cell() -> &'static Mutex<Option<NonBlocking>> {
    FILE_WRITER.get_or_init(|| Mutex::new(None))
}

fn file_guard_cell() -> &'static Mutex<Option<WorkerGuard>> {
    FILE_GUARD.get_or_init(|| Mutex::new(None))
}

/// 按开关状态分发写入的 MakeWriter:文件日志关闭时丢弃写入内容
struct ToggleableWriter;

impl<'a> MakeWriter<'a> for ToggleableWriter {
    type Writer = FileWriter;

    fn make_writer(&'a self) -> Self::Writer {
        let inner = file_writer_cell()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|writer| writer.make_writer());
        FileWriter(inner)
    }
}

/// 封装文件写入器;开关关闭时为 None,写入被跳过
struct FileWriter(Option<NonBlocking>);

impl Write for FileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &mut self.0 {
            Some(writer) => writer.write(buf),
            None => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match &mut self.0 {
            Some(writer) => writer.flush(),
            None => Ok(()),
        }
    }
}

/// 构建文件写入器:轮转旧日志文件、创建异步写入器与写线程凭证
fn build_file_writer() -> (NonBlocking, WorkerGuard) {
    let logs_dir = paths::LOGS_DIR;
    if let Err(e) = std::fs::create_dir_all(logs_dir) {
        tracing::warn!("[Log] 创建日志目录 {logs_dir} 失败: {e}");
    }
    let latest = format!("{logs_dir}/latest.log");
    if std::path::Path::new(&latest).exists() {
        let archived = format!(
            "{logs_dir}/{}.log",
            local_timestamp().replace([':', ' '], "-")
        );
        if let Err(e) = std::fs::rename(&latest, &archived) {
            tracing::warn!("[Log] 轮转旧日志文件失败: {e}");
        }
    }
    let appender = tracing_appender::rolling::never(logs_dir, "latest.log");
    tracing_appender::non_blocking(appender)
}

/// 启动期过滤器:RUST_LOG 环境变量优先,否则使用配置档位
fn startup_filter(level: LogLevel) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| level_filter(level))
}

/// 运行期过滤器:应用自身按档位,其余(第三方库)恒收敛 warn;
/// 需要第三方细节时用 RUST_LOG 定向(如 RUST_LOG=netowl=debug,wgpu_core=trace)
fn level_filter(level: LogLevel) -> EnvFilter {
    match level {
        LogLevel::Off => EnvFilter::new("off"),
        l => EnvFilter::new(format!("netowl={},warn", l.as_str())),
    }
}

/// 本地时间计时器(fmt 层默认输出 UTC,与本地时区观感不符);
/// GetLocalTime 直出本地时间,免时区换算(跨平台时移入 platform 模块)
struct LocalTimer;

impl FormatTime for LocalTimer {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", local_timestamp())
    }
}

/// 本地时间戳 "YYYY-MM-DD HH:MM:SS.mmm"(文件层与内存层共用)
pub(crate) fn local_timestamp() -> String {
    let st = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
    )
}
