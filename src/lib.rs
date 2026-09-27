//! NetOwl crate 根:按功能域组织模块(程序入口与 Windows 子系统属性见 main.rs)。
//!
//! 各组件的 new() 需初始化通道、时间戳等运行时状态,Default 语义不成立;
//! lib 化后公开构造函数会触发 new_without_default,在此关闭。

#![allow(clippy::new_without_default)]

pub mod app;
pub mod collector;
pub mod i18n;
pub mod map;
pub mod model;
pub mod net;
pub mod platform;
pub mod rules;
pub mod storage;
pub mod ui;
