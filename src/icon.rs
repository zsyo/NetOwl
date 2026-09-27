//! 应用图标:编译期内嵌 assets 资源(`include_bytes!`),运行时解码为 RGBA,
//! 发布产物无需携带 assets 目录(AGENTS.md 规范 9)。
//!
//! 图标源文件要求 8-bit RGBA PNG,非 RGBA 时显式报错,不做静默转换。

use eframe::egui;

/// 窗口/任务栏图标(256x256,高分辨率供任务栏与 Alt-Tab 缩放)
pub fn window_icon() -> egui::IconData {
    let (rgba, width, height) = decode_png(include_bytes!("../assets/app_icon.png"));
    egui::IconData {
        rgba,
        width,
        height,
    }
}

/// 托盘图标 RGBA(64x64,由系统缩放到 DPI 对应的小图标尺寸)
pub fn tray_icon_rgba() -> (Vec<u8>, u32, u32) {
    decode_png(include_bytes!("../assets/tray_icon.png"))
}

/// 解码内嵌 PNG 为非预乘 RGBA 像素
fn decode_png(bytes: &[u8]) -> (Vec<u8>, u32, u32) {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().expect("内嵌应用图标 PNG 解析失败");
    // None 仅在像素尺寸计算溢出时出现,视为数据异常
    let mut buf = vec![
        0u8;
        reader
            .output_buffer_size()
            .expect("内嵌图标输出缓冲区尺寸非法")
    ];
    let info = reader.next_frame(&mut buf).expect("内嵌应用图标解码失败");
    assert!(
        info.color_type == png::ColorType::Rgba,
        "应用图标必须是 8-bit RGBA PNG,当前为 {:?}",
        info.color_type
    );
    buf.truncate(info.buffer_size());
    (buf, info.width, info.height)
}
