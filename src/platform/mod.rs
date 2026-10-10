//! 平台集成:数据根路径、应用图标、托盘、单实例、开机自启动、显示器几何、
//! 关机落库与无边框窗口缩放、全局热键。
pub mod autostart;
pub mod global_hotkey;
pub mod icon;
pub mod monitor;
pub mod paths;
pub mod resize;
pub mod shutdown_hook;
pub mod single_instance;
pub mod tray;
pub mod tray_pin;
pub mod update;
