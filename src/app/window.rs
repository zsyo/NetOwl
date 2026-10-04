//! 窗口几何与可见性:首帧 DPI 修正恢复、几何捕获写配置、可见性跟踪与
//! 单实例唤出校准。可见性字段为自行跟踪(egui 0.36 viewport().visible()
//! 恒为 None),全部变更路径必须同步。

use std::time::Instant;

use eframe::egui;

use super::{NetOwlApp, RESTORE_TIMEOUT, RESTORE_TOLERANCE};
use crate::platform::single_instance;

impl NetOwlApp {
    /// 首帧修正窗口几何:创建时 with_position 用物理坐标当逻辑值,主屏 DPI 为 100%
    /// 时已精确;其他 DPI 下 winit 会按主屏 scale 放大产生偏差,这里按当前
    /// pixels_per_point 反推逻辑值重新下发,egui 命令路径乘回同一 ppp 后物理精确。
    /// 若创建位置已正确,这些命令为无操作。
    pub(super) fn restore_window_geometry(&mut self, ctx: &egui::Context) {
        let Some((x, y, w, h, maximized)) = self.pending_restore.take() else {
            return;
        };
        let ppp = ctx.pixels_per_point();
        tracing::debug!("[Window] 恢复窗口几何 {x},{y} {w}x{h}(最大化 {maximized},ppp {ppp:.2})");
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
            x as f32 / ppp,
            y as f32 / ppp,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            w as f32 / ppp,
            h as f32 / ppp,
        )));
        if maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        self.restore_active = true;
        self.restore_started = Instant::now();
    }

    /// 捕获窗口几何(物理像素)写入配置;恢复生效期间跳过,防默认位置覆盖。
    pub(super) fn capture_window_geometry(&mut self, ctx: &egui::Context) {
        if self.restore_active {
            let target = self
                .config
                .window_position()
                .map(|(x, y, w, h)| (x, y, w, h, self.config.window.maximized));
            let ppp = ctx.pixels_per_point();
            let (outer, inner, maximized) = ctx.input(|i| {
                let v = i.viewport();
                (v.outer_rect, v.inner_rect, v.maximized)
            });
            let settled = match (target, outer, inner) {
                (Some((tx, ty, tw, th, tmax)), Some(outer), Some(inner)) => {
                    if tmax {
                        maximized == Some(true)
                    } else {
                        (outer.min.x * ppp).round() as i32 - tx <= RESTORE_TOLERANCE
                            && (outer.min.y * ppp).round() as i32 - ty <= RESTORE_TOLERANCE
                            && (inner.width() * ppp).round() as i32 - tw <= RESTORE_TOLERANCE
                            && (inner.height() * ppp).round() as i32 - th <= RESTORE_TOLERANCE
                    }
                }
                _ => false,
            };
            if settled || self.restore_started.elapsed() > RESTORE_TIMEOUT {
                self.restore_active = false;
            } else {
                return;
            }
        }

        let minimized = ctx.input(|i| i.viewport().minimized);
        if minimized == Some(true) {
            return;
        }
        let ppp = ctx.pixels_per_point();
        let (outer, inner, maximized) = ctx.input(|i| {
            let v = i.viewport();
            (v.outer_rect, v.inner_rect, v.maximized)
        });
        if let (Some(outer), Some(inner)) = (outer, inner) {
            let changed = self.config.set_window(
                (outer.min.x * ppp).round() as i32,
                (outer.min.y * ppp).round() as i32,
                (inner.width() * ppp).round() as i32,
                (inner.height() * ppp).round() as i32,
                maximized == Some(true),
            );
            if changed {
                self.mark_config_dirty();
            }
        }
    }

    /// 窗口是否对用户可见:自行跟踪的可见性 && 未最小化
    /// (最小化状态由 egui 填充,可信)
    pub(super) fn is_shown(&self, ctx: &egui::Context) -> bool {
        let minimized = ctx.input(|i| i.viewport().minimized) == Some(true);
        self.window_visible && !minimized
    }

    /// 校准可见性跟踪:单实例二次启动经系统 API 直接 ShowWindow 唤出主窗口,
    /// 不经过 app 命令路径,以 IsWindowVisible 为准纠正(隐藏/显示命令自身的
    /// 发送路径窗口状态一致,校准为无操作)
    pub(super) fn calibrate_window_visible(&mut self) {
        let visible = single_instance::is_window_visible(self.main_hwnd);
        if visible != self.window_visible {
            tracing::debug!("[Window] 可见性外部变更校准 -> {visible}(单实例唤出或系统操作)");
            self.window_visible = visible;
        }
    }
}
