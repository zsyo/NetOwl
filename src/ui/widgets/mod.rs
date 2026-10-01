//! 公共 UI 组件库:跨页面复用的绘制小件。
//!
//! 组件只做绘制与交互返回,状态由调用方持有(架构规范:数据与绘制分离);
//! 全部取色经 [`theme::c()`](super::theme::c),字号/间距用刻度常量。

pub mod badge;
pub mod bar_chart;
pub mod card;
pub mod header;
pub mod menu;
pub mod process;
pub mod segmented;
pub mod sparkline;
pub mod table;
pub mod toggle;
