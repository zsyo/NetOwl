//! 悬浮球主窗口侧编排:viewport 绘制调度与交互结果落地(位置记忆、
//! 唤出主窗口、开关关闭)。球体绘制与交互状态机见 crate::ui::floating_ball。

use eframe::egui;

use super::NetOwlApp;
use crate::ui::floating_ball;

impl NetOwlApp {
    /// 悬浮窗(独立 viewport;主窗口隐藏时低频帧仍维持显示与数据刷新)
    pub(super) fn show_floating_ball(&mut self, ui: &mut egui::Ui) {
        if !self.config.floating_ball.enabled {
            return;
        }
        let ball = floating_ball::show(
            ui.ctx(),
            &mut self.floating_ball,
            &self.ball_data,
            &self.i18n,
            &mut self.config.floating_ball,
        );
        if ball.pos_dirty {
            let (x, y) = self.floating_ball.pos();
            self.config.floating_ball.x = x.round() as i32;
            self.config.floating_ball.y = y.round() as i32;
            self.mark_config_dirty();
        }
        if let Some(page) = ball.show_main {
            // 与托盘"显示主窗口"同路径:恢复可见、解除最小化并落到目标页
            self.window_visible = true;
            self.page = page;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if ball.config_touched {
            self.mark_config_dirty();
        }
        if ball.close {
            // 与设置页开关同一数据源,下一帧起悬浮窗整体不再创建
            self.config.floating_ball.enabled = false;
            self.mark_config_dirty();
        }
    }
}
