//! 选中文本快照捕手:egui 不公开 Label 的选中内容(text_to_copy 为
//! 私有),拖选期间由 App 层每帧注入 Event::Copy,egui 在帧末经
//! PlatformOutput.commands 输出 CopyText——本插件在 output_hook
//! (output 交 backend 之前)捕获该文本存底,供“复制选中文本”菜单项
//! 读取。选中态被清除(点击他处/菜单)后快照仍在,复制不依赖 selection
//! 存活。

use std::sync::Mutex;

use eframe::egui;

/// 最近一次捕获的待复制文本
#[derive(Default)]
pub struct CopyCatcher {
    last: Mutex<Option<String>>,
}

impl CopyCatcher {
    /// 取走缓存的文本(无则 None)
    pub fn take(&self) -> Option<String> {
        self.last.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

impl egui::Plugin for CopyCatcher {
    fn debug_name(&self) -> &'static str {
        "copy_catcher"
    }

    fn output_hook(&mut self, _ctx: &egui::Context, output: &mut egui::FullOutput) {
        for cmd in &output.platform_output.commands {
            if let egui::OutputCommand::CopyText(t) = cmd
                && !t.is_empty()
            {
                *self.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(t.clone());
            }
        }
    }
}
