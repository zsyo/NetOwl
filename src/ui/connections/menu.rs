//! 连接列表行右键菜单:结束连接(仅 TCP,需提权)/定位程序/复制远端
//! 与路径。菜单项用默认 Button 样式,禁用态带原因悬停提示。

use eframe::egui;
use egui::RichText;

use crate::collector;
use crate::i18n::I18n;
use crate::model::{Connection, Protocol};
use crate::ui::{theme, widgets};

/// 行右键菜单;返回"查看历史记录"是否被点击(跳转由调用方执行)
pub(super) fn conn_menu(ui: &mut egui::Ui, conn: &Connection, elevated: bool, i18n: &I18n) -> bool {
    if conn.proto == Protocol::Tcp {
        let kill = ui.add_enabled(
            elevated,
            egui::Button::new(RichText::new(i18n.t("menu-kill")).size(theme::font::BODY)),
        );
        let kill = if elevated {
            kill
        } else {
            kill.on_disabled_hover_text(i18n.t("menu-kill-need-admin"))
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
