//! 日志基础设施:tracing + EnvFilter(AGENTS.md 日志规范)。
//!
//! 档位:RUST_LOG 环境变量优先(面向调试,支持按模块定向,如
//! `RUST_LOG=netowl=debug,warn`);默认应用自身 info、其余(第三方库)warn,
//! 避免依赖树噪音。输出到 stderr,release 构建(windows subsystem)无控制台,
//! 调试以 `cargo run` 终端查看。

use tracing_subscriber::{EnvFilter, fmt, fmt::format::Writer, fmt::time::FormatTime};

/// 本地时间计时器:fmt 层默认输出 UTC,与本地时区观感不符;
/// GetLocalTime 直出本地时间,免时区换算(跨平台时移入 platform 模块)
struct LocalTimer;

impl FormatTime for LocalTimer {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        write!(
            w,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
        )
    }
}

/// 初始化全局日志;必须在启动早期调用且全程仅一次
pub fn init() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("netowl=info,warn"));
    fmt()
        .with_env_filter(filter)
        .with_timer(LocalTimer)
        // 消息自带 [模块] 前缀,不重复输出 target 模块路径
        .with_target(false)
        .with_thread_ids(false)
        .init();
}
