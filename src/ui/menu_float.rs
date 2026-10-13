//! 自建右键菜单浮层:替代 egui context_menu。旧方案两大致命伤在浮层
//! 下从根上不存在——菜单是 Foreground Area 浮层,与表格行 rect 不相交,
//! 点击不经表格 hit-test(消除"按住跨帧菜单重排、点击错位误触删除");
//! 右键事件走 App 的 right_clicks 队列(egui 感知不到右键,Label 选中态
//! 分毫不动)。
//!
//! 打开 = ui 末尾消费 right_clicks 命中 row_hits;关闭 = 点菜单项 /
//! 左键外点 / Esc;动作入 MenuAction 队列帧末统一执行(可访问 db/
//! collector/platform,不受 Ui 借用约束)。T3 只接连接页,其余五页 T4 铺开。

use eframe::egui;
use egui::{CornerRadius, Frame, Margin, RichText, Stroke};

use super::row_hit::{RowCtx, RowHit};
use crate::i18n::I18n;
use crate::model::{Connection, Protocol};
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
    /// 上帧菜单 rect(左键外点关闭判定;首帧 ZERO 跳过判定)
    pub rect: egui::Rect,
}

impl OpenMenu {
    /// 右键位置命中行热区则开菜单(未命中返回 None,由调用方关闭旧菜单)
    pub fn open_at(row_hits: &[RowHit], pos: egui::Pos2) -> Option<Self> {
        // 后注册行居上(与 egui 平手取最后注册者同语义)
        row_hits
            .iter()
            .rev()
            .find(|h| h.rect.contains(pos))
            .map(|hit| OpenMenu {
                pos,
                col: hit.col_at(pos),
                hit: hit.clone(),
                rect: egui::Rect::ZERO,
            })
    }
}

/// 菜单动作(帧末由 App 统一执行)
pub enum MenuAction {
    /// 写剪贴板 + 底部气泡
    CopyText(String),
    /// 结束连接(SetTcpEntry;构造时已校验 TCP/v4/提权)
    KillConn(Connection),
    /// 资源管理器定位映像
    LocateProc(String),
    /// 跳转历史页
    NavHistory,
}

impl MenuAction {
    /// 动作标识(调试日志用)
    pub fn kind(&self) -> &'static str {
        match self {
            MenuAction::CopyText(_) => "copy",
            MenuAction::KillConn(_) => "kill",
            MenuAction::LocateProc(_) => "locate",
            MenuAction::NavHistory => "nav-history",
        }
    }
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
    let resp = egui::Area::new(egui::Id::new("menu_float"))
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

/// 菜单项列表(按命中行类型;T3 仅连接页,其余五页 T4 铺开)
fn items(
    ui: &mut egui::Ui,
    i18n: &I18n,
    menu: &OpenMenu,
    elevated: bool,
    actions: &mut Vec<MenuAction>,
) {
    let RowCtx::Conn(conn) = &menu.hit.ctx else {
        return;
    };
    if conn.proto == Protocol::Tcp {
        // 强制关闭走 SetTcpEntry(MIB_TCPROW),内核无 IPv6 等价入口,
        // v6 行禁用并提示(与迁移前 conn_menu 同口径)
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
    if widgets::menu::menu_item(ui, i18n.t("menu-locate"), conn.proc_path.is_some()).clicked()
        && let Some(path) = &conn.proc_path
    {
        actions.push(MenuAction::LocateProc(path.clone()));
    }
    if widgets::menu::menu_item(ui, i18n.t("menu-copy-remote"), true).clicked() {
        actions.push(MenuAction::CopyText(format!(
            "{}:{}",
            conn.remote_ip, conn.remote_port
        )));
    }
    if widgets::menu::menu_item(ui, i18n.t("menu-copy-path"), conn.proc_path.is_some()).clicked()
        && let Some(path) = &conn.proc_path
    {
        actions.push(MenuAction::CopyText(path.clone()));
    }
    if widgets::menu::menu_item(ui, i18n.t("menu-view-history"), true).clicked() {
        actions.push(MenuAction::NavHistory);
    }
}
