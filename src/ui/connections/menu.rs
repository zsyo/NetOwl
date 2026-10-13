//! 连接列表行右键菜单:结束连接(仅 TCP,需提权)/定位程序/复制远端
//! 与路径。菜单项用默认 Button 样式,禁用态带原因悬停提示。

use eframe::egui;
use egui::RichText;

use crate::collector;
use crate::i18n::I18n;
use crate::model::{Connection, Protocol};
use crate::ui::{theme, widgets};

/// 行右键菜单;返回"查看历史记录"是否被点击(跳转由调用方执行)
// 旧 context_menu 挂载点已移除(改走 ui/menu_float 自建浮层),
// 本函数保留至 T4 菜单项迁移后删除
#[allow(dead_code)]
pub(super) fn conn_menu(ui: &mut egui::Ui, conn: &Connection, elevated: bool, i18n: &I18n) -> bool {
    if conn.proto == Protocol::Tcp {
        // 强制关闭走 SetTcpEntry(MIB_TCPROW),内核无 IPv6 等价入口,
        // v6 行禁用并提示
        let is_v4 = conn.local_addr.is_ipv4() && conn.remote_ip.is_ipv4();
        let kill = ui.add_enabled(
            elevated && is_v4,
            egui::Button::new(RichText::new(i18n.t("menu-kill")).size(theme::font::BODY)),
        );
        let kill = if !elevated {
            kill.on_disabled_hover_text(i18n.t("menu-kill-need-admin"))
        } else if !is_v4 {
            kill.on_disabled_hover_text(i18n.t("menu-kill-v6"))
        } else {
            kill
        };
        if kill.clicked() {
            match collector::close_tcp_connection(conn) {
                Ok(()) => tracing::info!(
                    "[Connections] 已请求结束连接 {}:{} -> {}:{} (PID {})",
                    conn.local_addr,
                    conn.local_port,
                    conn.remote_ip,
                    conn.remote_port,
                    conn.pid
                ),
                Err(e) => tracing::warn!(
                    "[Connections] 结束连接失败 {}:{} -> {}:{}: {e}",
                    conn.local_addr,
                    conn.local_port,
                    conn.remote_ip,
                    conn.remote_port
                ),
            }
        }
    }
    widgets::menu::locate_copy_items(
        ui,
        i18n,
        conn.proc_path.as_deref(),
        Some((conn.remote_ip.to_string().as_str(), conn.remote_port)),
    );
    widgets::menu::menu_item(ui, i18n.t("menu-view-history"), true).clicked()
}
