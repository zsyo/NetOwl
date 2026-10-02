//! 平台集成:数据根路径、应用图标、托盘、单实例、开机自启动、关机落库与
//! 无边框窗口缩放。

pub mod autostart;
pub mod icon;
pub mod paths;
pub mod resize;
pub mod shutdown_hook;
pub mod single_instance;
pub mod tray;
