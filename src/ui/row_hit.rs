//! 表格行热区表(每帧重建):自建右键浮层(ui/menu_float)的命中测试
//! 数据源。仿 RowHover 范式——App 持 Vec,UiCtx 传 &mut,各表格行
//! 渲染时按行 rect 注册;列右边界由各页列宽累加,供"复制该单元格
//! 内容"定位命中列。
//!
//! RowCtx 一律持 owned 数据:菜单打开期间表格照常刷新,行引用会失效,
//! 克隆/提取字段后热区与菜单生命期自洽。

use std::net::IpAddr;

use eframe::egui;

use crate::model::{Connection, ListenEntry, Protocol};
use crate::net::lan::DeviceRow;
use crate::storage::history_query::SummaryRow;

/// 单个行热区:行整格区域 + 列右边界 + 行上下文
#[derive(Clone)]
pub struct RowHit {
    /// 行整格区域(屏幕坐标)
    pub rect: egui::Rect,
    /// 各列右边界 x(屏幕坐标,升序;末列右界 = rect.right())
    pub col_edges: Vec<f32>,
    /// 行上下文(owned)
    pub ctx: RowCtx,
}

impl RowHit {
    /// 注册一行热区;col_widths 为从左到右各列宽(累加和 = 行宽)
    pub fn push(row_hits: &mut Vec<RowHit>, rect: egui::Rect, col_widths: &[f32], ctx: RowCtx) {
        let mut edges = Vec::with_capacity(col_widths.len());
        let mut x = rect.left();
        for (i, w) in col_widths.iter().enumerate() {
            x += w;
            // 末列右界贴行右缘,吸收 flex 列均分的累积误差
            edges.push(if i + 1 == col_widths.len() {
                rect.right()
            } else {
                x
            });
        }
        row_hits.push(RowHit {
            rect,
            col_edges: edges,
            ctx,
        });
    }

    /// 右键位置命中的列下标(0 基);不在行内返回 None
    pub fn col_at(&self, pos: egui::Pos2) -> Option<usize> {
        if !self.rect.contains(pos) {
            return None;
        }
        self.col_edges.iter().position(|&right| pos.x < right)
    }
}

/// 行上下文:六表格行的 owned 数据子集(不可 Clone 的行类型手动提取
/// 菜单动作所需字段)
#[derive(Clone)]
pub enum RowCtx {
    /// 连接页连接行
    Conn(Connection),
    /// 连接页监听视图行
    Listen(ListenEntry),
    /// 历史明细行:删除键(event_id + 首末时刻)+ 定位/复制字段
    Detail {
        event_id: u64,
        first_seen: u64,
        last_seen: u64,
        proc_path: Option<String>,
        remote: IpAddr,
        remote_port: u16,
    },
    /// 历史聚合行:组删除键(进程 + 协议 + 远端)
    Aggregate {
        process: String,
        proto: Protocol,
        remote: IpAddr,
    },
    /// 历史汇总行(整进程)
    Summary(SummaryRow),
    /// 局域网页设备行
    Lan(DeviceRow),
}

impl RowCtx {
    /// 页标识(调试日志用)
    pub fn kind(&self) -> &'static str {
        match self {
            RowCtx::Conn(_) => "conn",
            RowCtx::Listen(_) => "listen",
            RowCtx::Detail { .. } => "detail",
            RowCtx::Aggregate { .. } => "aggregate",
            RowCtx::Summary(_) => "summary",
            RowCtx::Lan(_) => "lan",
        }
    }
}
