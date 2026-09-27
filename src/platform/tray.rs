//! 托盘与菜单:tray-icon + muda(经 tray_icon::menu re-export)。
//!
//! 托盘在 eframe 主线程创建(见 AGENTS.md 平台规范),事件经
//! set_event_handler 转发到 mpsc 通道,并 request_repaint 唤醒隐藏状态下的主循环。

use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// 托盘命令:显示主窗口
pub const CMD_SHOW: &str = "show";
/// 托盘命令:隐藏到托盘
pub const CMD_HIDE: &str = "hide";
/// 托盘命令:退出应用
pub const CMD_QUIT: &str = "quit";

/// 托盘句柄;保活即图标在位,drop 时自动移除
pub struct Tray {
    _inner: TrayIcon,
}

/// 在当前线程创建托盘与菜单,返回(句柄, 命令接收端)
pub fn create(ctx: egui::Context) -> (Tray, Receiver<String>) {
    let menu = Menu::new();
    let show = MenuItem::with_id(CMD_SHOW, "显示主窗口", true, None);
    let hide = MenuItem::with_id(CMD_HIDE, "隐藏到托盘", true, None);
    let quit = MenuItem::with_id(CMD_QUIT, "退出", true, None);
    menu.append(&show).expect("追加托盘菜单项失败");
    menu.append(&hide).expect("追加托盘菜单项失败");
    menu.append(&PredefinedMenuItem::separator())
        .expect("追加托盘分隔符失败");
    menu.append(&quit).expect("追加托盘菜单项失败");

    let (rgba, width, height) = crate::platform::icon::tray_icon_rgba();
    let icon = tray_icon::Icon::from_rgba(rgba, width, height).expect("托盘图标数据非法");
    let tray = TrayIconBuilder::new()
        .with_id("netowl-tray")
        .with_tooltip("NetOwl 网络监控")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        // 左键不弹菜单,而是直接显示主窗口(TrayIconEvent 处理)
        .with_menu_on_left_click(false)
        .build()
        .expect("创建系统托盘失败");

    let (tx, rx) = channel::<String>();
    MenuEvent::set_event_handler(Some(forward(tx.clone(), ctx.clone())));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            send(&tx, CMD_SHOW);
            ctx.request_repaint();
        }
    }));

    (Tray { _inner: tray }, rx)
}

/// 消费托盘命令,返回是否还有未处理事件由调用方决定;这里一次抽干
pub fn drain(receiver: &Receiver<String>) -> Vec<String> {
    let mut cmds = Vec::new();
    while let Ok(cmd) = receiver.try_recv() {
        cmds.push(cmd);
    }
    cmds
}

fn forward(tx: Sender<String>, ctx: egui::Context) -> impl Fn(MenuEvent) + Send + Sync {
    move |event: MenuEvent| {
        send(&tx, &event.id.0);
        ctx.request_repaint();
    }
}

fn send(tx: &Sender<String>, cmd: &str) {
    let _ = tx.send(cmd.to_owned());
}
