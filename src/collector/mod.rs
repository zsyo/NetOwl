//! 连接采集:Collector trait 是 UI 唯一数据入口(AGENTS.md 架构规范)。
//! 真实数据源见 windows_table(TCP/UDP 表快照);mock 保留供演示与测试,
//! 设置页可切换数据源,ETW 短命连接捕获后续接入。

mod icon;
mod mock;
mod query;
mod signature;
mod windows_table;

pub use icon::IconImage;
pub use mock::MockCollector;
pub use query::query_process_path;
pub use windows_table::TableCollector;

use std::sync::Arc;

use crate::model::Connection;

/// 进程映像图标状态(按映像路径查询;None 表示已提取且无图标)
pub enum IconState {
    /// 提取中(后台线程执行)
    Pending,
    /// 已完成;无图标资源/提取失败为 None
    Ready(Option<Arc<IconImage>>),
}

/// 连接采集器
pub trait Collector {
    /// 推进内部状态并返回当前连接快照
    fn snapshot(&mut self) -> Vec<Connection>;
    /// 数据源种类(导航状态与设置页展示用)
    fn kind(&self) -> CollectorKind;
    /// 查询映像路径的图标;默认(模拟数据)恒为提取中
    fn icon_image(&mut self, _path: &str) -> IconState {
        IconState::Pending
    }
}

/// 数据源种类(配置持久化为 "real" / "mock")
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectorKind {
    /// 真实采集(Windows TCP/UDP 表)
    Real,
    /// 模拟数据(演示与测试)
    Mock,
}

impl CollectorKind {
    /// 从配置值解析;空串与未知值按真实采集处理
    pub fn from_config(value: &str) -> Self {
        if value == "mock" {
            CollectorKind::Mock
        } else {
            CollectorKind::Real
        }
    }

    pub fn as_config(self) -> &'static str {
        match self {
            CollectorKind::Real => "real",
            CollectorKind::Mock => "mock",
        }
    }
}

/// 按数据源种类构造采集器实例
pub fn build(kind: CollectorKind) -> Box<dyn Collector> {
    match kind {
        CollectorKind::Real => Box::new(TableCollector::new()),
        CollectorKind::Mock => Box::new(MockCollector::new()),
    }
}
