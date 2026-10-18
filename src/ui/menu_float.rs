//! 自建右键菜单浮层:替代 egui context_menu。旧方案两大致命伤在浮层
//! 下从根上不存在——菜单是 Foreground Area 浮层,与表格行 rect 不相交,
//! 点击不经表格 hit-test(消除"按住跨帧菜单重排、点击错位误触删除");
//! 右键事件走 App 的 right_clicks 队列(egui 感知不到右键,Label 选中态
//! 分毫不动)。
//!
//! 打开 = ui 末尾消费 right_clicks 命中 row_hits;关闭 = 点菜单项 /
//! 左键外点 / Esc;动作入 MenuAction 队列帧末统一执行(可访问 db/
//! collector/platform,不受 Ui 借用约束)。

use std::collections::HashMap;
use std::net::IpAddr;

use eframe::egui;
use egui::{CornerRadius, Frame, Margin, RichText, Stroke};

use super::row_hit::{RowCtx, RowHit};
use crate::i18n::I18n;
use crate::model::{Connection, ListenEntry, Place, Protocol, fmt_bytes};
use crate::net::geoip;
use crate::net::lan::DeviceRow;
use crate::storage::history_query::{self, PendingDelete};
use crate::ui::theme;
use crate::ui::widgets;

/// 打开中的右键菜单
pub struct OpenMenu {
    /// 右键按下点(屏幕坐标;菜单锚点)
    pub pos: egui::Pos2,
    /// 命中行(owned)
    pub hit: RowHit,
    /// 命中列下标("复制该单元格内容"用;None = 不在任何列内)
    pub col: Option<usize>,
    /// 打开时的选中文本快照(None = 无选中)
    pub selected: Option<String>,
    /// 打开时的选中范围(屏幕矩形;None = 无选中)
    pub sel_range: Option<egui::Rect>,
    /// 上帧菜单 rect(左键外点关闭判定;首帧 ZERO 跳过判定)
    pub rect: egui::Rect,
}

impl OpenMenu {
    /// 右键位置命中行热区则开菜单(未命中返回 None,由调用方关闭旧菜单)
    pub fn open_at(
        row_hits: &[RowHit],
        pos: egui::Pos2,
        selected: Option<String>,
        sel_range: Option<egui::Rect>,
    ) -> Option<Self> {
        // 后注册行居上(与 egui 平手取最后注册者同语义)
        row_hits
            .iter()
            .rev()
            .find(|h| h.rect.contains(pos))
            .map(|hit| OpenMenu {
                pos,
                col: hit.col_at(pos),
                hit: hit.clone(),
                selected,
                sel_range,
                rect: egui::Rect::ZERO,
            })
    }
}

/// 菜单动作(帧末由 App 统一执行)
pub enum MenuAction {
    /// 写剪贴板 + 底部气泡
    CopyText(String),
    /// 复制选中文本(执行时清选中态与快照,一次性)
    CopySelectedText(String),
    /// 结束连接(SetTcpEntry;构造时已校验 TCP/v4/提权)
    KillConn(Connection),
    /// 资源管理器定位映像
    LocateProc(String),
    /// 跳转历史页
    NavHistory,
    /// 删除历史明细行(直接删,与迁移前行为一致无确认)
    DeleteDetail {
        event_id: u64,
        first_seen: u64,
        last_seen: u64,
    },
    /// 删除确认(聚合组/整进程):转 history 页确认弹窗链路
    AskDelete(PendingDelete),
}

impl MenuAction {
    /// 动作标识(调试日志用)
    pub fn kind(&self) -> &'static str {
        match self {
            MenuAction::CopyText(_) => "copy",
            MenuAction::CopySelectedText(_) => "copy-selected",
            MenuAction::KillConn(_) => "kill",
            MenuAction::LocateProc(_) => "locate",
            MenuAction::NavHistory => "nav-history",
            MenuAction::DeleteDetail { .. } => "delete-detail",
            MenuAction::AskDelete(_) => "ask-delete",
        }
    }
}

/// 菜单打开期间被右击表格的冻结数据源:打开瞬间定格,菜单关闭即弃。
/// 采集/速率/查询在后台继续跑(数据不丢),关闭后 UI 恢复即为最新
pub enum MenuFrozen {
    /// 连接页:连接表 + 每连接速率表克隆(行内容/顺序/速率列静止)
    Conns {
        conns: Vec<Connection>,
        rates: HashMap<u64, (u64, u64)>,
    },
    /// 监听视图:监听表克隆
    Listens(Vec<ListenEntry>),
    /// 局域网页:设备表克隆
    Lan(Vec<DeviceRow>),
    /// 历史页:查询结果不克隆,冻结 = 暂停刷新(rows/汇总合并定格)
    History,
}

/// 绘制菜单浮层;返回本帧动作与刷新后的 rect。开/关判定在调用方
/// (frame.rs),绘制只负责内容与命中
pub fn show(
    ctx: &egui::Context,
    i18n: &I18n,
    menu: &OpenMenu,
    elevated: bool,
) -> (Vec<MenuAction>, egui::Rect) {
    let mut actions = Vec::new();
    let resp = egui::Area::new(egui::Id::unique("menu_float"))
        .order(egui::Order::Foreground)
        .fixed_pos(menu.pos)
        .constrain(true)
        .show(ctx, |ui| {
            Frame::new()
                .fill(theme::c().bg_elevated)
                .stroke(Stroke::new(1.0, theme::c().stroke))
                .corner_radius(CornerRadius::same(theme::RADIUS_MD))
                .inner_margin(Margin::same(theme::sp::SM as i8))
                .show(ui, |ui| items(ui, i18n, menu, elevated, &mut actions))
                .response
        });
    (actions, resp.response.rect)
}

/// 菜单项列表:选中复制项置顶(有选中且右键在选中范围内才出现),
/// 其后按命中行类型(顺序与迁移前 context_menu 一致),尾部统一
/// "复制该单元格内容"
fn items(
    ui: &mut egui::Ui,
    i18n: &I18n,
    menu: &OpenMenu,
    elevated: bool,
    actions: &mut Vec<MenuAction>,
) {
    // 复制选中文本:确有选中快照,且右键位置落在选中范围内
    // (拖选起止边界框)才出现——平时右键/点别处不显示
    if let Some(text) = &menu.selected
        && menu.sel_range.is_some_and(|r| r.contains(menu.pos))
        && widgets::menu::menu_item(ui, i18n.t("menu-copy-selected"), true).clicked()
    {
        actions.push(MenuAction::CopySelectedText(text.clone()));
    }
    match &menu.hit.ctx {
        RowCtx::Conn(conn) => {
            if conn.proto == Protocol::Tcp {
                // 强制关闭走 SetTcpEntry(MIB_TCPROW),内核无 IPv6 等价入口,
                // v6 行禁用并提示
                let is_v4 = conn.local_addr.is_ipv4() && conn.remote_ip.is_ipv4();
                let btn = ui.add_enabled(
                    elevated && is_v4,
                    egui::Button::new(RichText::new(i18n.t("menu-kill")).size(theme::font::BODY)),
                );
                let btn = if !elevated {
                    btn.on_disabled_hover_text(i18n.t("menu-kill-need-admin"))
                } else if !is_v4 {
                    btn.on_disabled_hover_text(i18n.t("menu-kill-v6"))
                } else {
                    btn
                };
                if btn.clicked() {
                    actions.push(MenuAction::KillConn(conn.clone()));
                }
            }
            if widgets::menu::menu_item(ui, i18n.t("menu-locate"), conn.proc_path.is_some())
                .clicked()
                && let Some(path) = &conn.proc_path
            {
                actions.push(MenuAction::LocateProc(path.clone()));
            }
            if widgets::menu::menu_item(ui, i18n.t("menu-copy-remote"), true).clicked() {
                actions.push(MenuAction::CopyText(conn.remote_display()));
            }
            if widgets::menu::menu_item(ui, i18n.t("menu-copy-path"), conn.proc_path.is_some())
                .clicked()
                && let Some(path) = &conn.proc_path
            {
                actions.push(MenuAction::CopyText(path.clone()));
            }
            if widgets::menu::menu_item(ui, i18n.t("menu-view-history"), true).clicked() {
                actions.push(MenuAction::NavHistory);
            }
        }
        RowCtx::Listen(l) => {
            if widgets::menu::menu_item(ui, i18n.t("menu-copy-remote"), true).clicked() {
                actions.push(MenuAction::CopyText(format!(
                    "{}:{}",
                    l.local_addr, l.local_port
                )));
            }
        }
        RowCtx::Detail {
            event_id,
            first_seen,
            last_seen,
            proc_path,
            remote,
            remote_port,
            ..
        } => {
            if widgets::menu::menu_item(ui, i18n.t("history-menu-delete-row"), true).clicked() {
                actions.push(MenuAction::DeleteDetail {
                    event_id: *event_id,
                    first_seen: *first_seen,
                    last_seen: *last_seen,
                });
            }
            if widgets::menu::menu_item(ui, i18n.t("menu-locate"), proc_path.is_some()).clicked()
                && let Some(path) = proc_path
            {
                actions.push(MenuAction::LocateProc(path.clone()));
            }
            if widgets::menu::menu_item(ui, i18n.t("menu-copy-remote"), true).clicked() {
                actions.push(MenuAction::CopyText(format!("{remote}:{remote_port}")));
            }
            if widgets::menu::menu_item(ui, i18n.t("menu-copy-path"), proc_path.is_some()).clicked()
                && let Some(path) = proc_path
            {
                actions.push(MenuAction::CopyText(path.clone()));
            }
        }
        RowCtx::Aggregate {
            process,
            proto,
            remote,
            ..
        } => {
            if widgets::menu::menu_item(ui, i18n.t("history-menu-delete-group"), true).clicked() {
                actions.push(MenuAction::AskDelete(PendingDelete {
                    process: process.clone(),
                    proto: Some(*proto),
                    remote_ip: Some(*remote),
                }));
            }
            if widgets::menu::menu_item(ui, i18n.t("menu-copy-remote"), true).clicked() {
                actions.push(MenuAction::CopyText(remote.to_string()));
            }
        }
        RowCtx::Summary(r) => {
            if widgets::menu::menu_item(ui, i18n.t("history-menu-delete-process"), true).clicked() {
                actions.push(MenuAction::AskDelete(PendingDelete {
                    process: r.process.clone(),
                    proto: None,
                    remote_ip: None,
                }));
            }
            if widgets::menu::menu_item(ui, i18n.t("menu-locate"), r.proc_path.is_some()).clicked()
                && let Some(path) = &r.proc_path
            {
                actions.push(MenuAction::LocateProc(path.clone()));
            }
            if widgets::menu::menu_item(ui, i18n.t("menu-copy-path"), r.proc_path.is_some())
                .clicked()
                && let Some(path) = &r.proc_path
            {
                actions.push(MenuAction::CopyText(path.clone()));
            }
        }
        RowCtx::Lan(d) => {
            if widgets::menu::menu_item(ui, i18n.t("lan-copy-ip"), true).clicked() {
                actions.push(MenuAction::CopyText(d.ip.to_string()));
            }
            if widgets::menu::menu_item(ui, i18n.t("lan-copy-mac"), true).clicked() {
                actions.push(MenuAction::CopyText(d.mac.clone()));
            }
        }
    }
    // 复制该单元格内容:命中列有可复制文本才出现(徽章列/无文本列
    // 不出现)。速率列等 App 层数据不出现在 RowCtx,一并不出现
    if let Some(col) = menu.col
        && let Some(text) = cell_text(&menu.hit, col, i18n)
        && widgets::menu::menu_item(ui, i18n.t("menu-copy-cell"), true).clicked()
    {
        actions.push(MenuAction::CopyText(text));
    }
}

/// 命中列的单元格文本(与各行渲染显示一致;徽章列/速率列/状态列
/// 返回 None,菜单不出现"复制该单元格内容"项)
pub fn cell_text(hit: &RowHit, col: usize, i18n: &I18n) -> Option<String> {
    match &hit.ctx {
        RowCtx::Conn(conn) => match col {
            // 进程列:未知进程占位与渲染一致
            0 => Some(if conn.process.is_empty() {
                format!(
                    "{} {}",
                    i18n.t("conn-proc-unknown"),
                    i18n.t_with_args("pid-suffix", &[("pid", conn.pid.to_string())])
                )
            } else {
                conn.process.clone()
            }),
            2 => Some(conn.remote_display()),
            3 => Some(match conn.city {
                Some(place) => geoip::place_label(place, i18n),
                None => i18n.t("conn-loc-unknown"),
            }),
            6 => Some(fmt_bytes(conn.bytes_in)),
            7 => Some(fmt_bytes(conn.bytes_out)),
            _ => None,
        },
        RowCtx::Listen(l) => match col {
            0 => Some(l.process.clone()),
            2 => Some(format!("{}:{}", l.local_addr, l.local_port)),
            3 => Some(l.pid.to_string()),
            _ => None,
        },
        RowCtx::Detail {
            first_seen,
            last_seen,
            process,
            pid,
            remote,
            remote_port,
            bytes_in,
            bytes_out,
            ..
        } => match col {
            0 => Some(if process.is_empty() {
                format!(
                    "{} {}",
                    i18n.t("conn-proc-unknown"),
                    i18n.t_with_args("pid-suffix", &[("pid", pid.to_string())])
                )
            } else {
                format!(
                    "{process} {}",
                    i18n.t_with_args("pid-suffix", &[("pid", pid.to_string())])
                )
            }),
            2 => Some(format!("{remote}:{remote_port}")),
            3 => Some(location_label(*remote, i18n)),
            4 => Some(history_query::fmt_local(*first_seen)),
            5 => Some(history_query::fmt_duration(
                last_seen.saturating_sub(*first_seen),
            )),
            6 => Some(fmt_bytes(*bytes_in)),
            7 => Some(fmt_bytes(*bytes_out)),
            _ => None,
        },
        RowCtx::Aggregate {
            process,
            remote,
            count,
            total_secs,
            last_active,
            bytes_in,
            bytes_out,
            ..
        } => match col {
            0 => Some(process.clone()),
            2 => Some(remote.to_string()),
            3 => Some(location_label(*remote, i18n)),
            4 => Some(count.to_string()),
            5 => Some(history_query::fmt_duration(*total_secs)),
            6 => Some(history_query::fmt_local(*last_active)),
            7 => Some(fmt_bytes(*bytes_in)),
            8 => Some(fmt_bytes(*bytes_out)),
            _ => None,
        },
        RowCtx::Summary(r) => match col {
            0 => Some(if r.process.is_empty() {
                i18n.t("conn-proc-unknown")
            } else {
                r.process.clone()
            }),
            1 => Some(fmt_bytes(r.bytes_out)),
            2 => Some(fmt_bytes(r.bytes_in)),
            3 => Some(r.count.to_string()),
            4 => Some(history_query::fmt_duration(r.total_secs)),
            _ => None,
        },
        RowCtx::Lan(d) => match col {
            0 => Some(d.ip.to_string()),
            1 => Some(d.mac.clone()),
            2 => Some(history_query::fmt_local(d.last_seen)),
            3 => Some(history_query::fmt_local(d.first_seen)),
            _ => None,
        },
    }
}

/// 归属列文本:实时反查 + 运营商后缀(与 cells::location_cell 一致)
fn location_label(ip: IpAddr, i18n: &I18n) -> String {
    let mut text = match geoip::locate(ip).map(Place::Geo) {
        Some(p) => geoip::place_label(p, i18n),
        None => i18n.t("conn-loc-unknown"),
    };
    if let Some(isp) = geoip::locate_isp(ip) {
        text.push_str(" · ");
        text.push_str(isp);
    }
    text
}
