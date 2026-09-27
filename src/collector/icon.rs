//! 进程图标提取:SHGetFileInfoW 取 exe 关联图标(32x32),GetIconInfo +
//! GetDIBits 转为 RGBA 像素供 egui 纹理使用。
//!
//! 读取文件与 GDI 调用可能阻塞数十毫秒,调用方须在工作线程执行;
//! 提取失败(无图标资源/文件已消失)返回 None,由调用方缓存避免重复尝试。

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::UI::Shell::{
    SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

/// 提取出的图标位图(非预乘 alpha)
pub struct IconImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 提取映像文件的关联图标;失败返回 None
pub fn extract(path: &str) -> Option<IconImage> {
    unsafe {
        let mut path_w: Vec<u16> = path.encode_utf16().collect();
        path_w.push(0);
        let mut sfi = SHFILEINFOW::default();
        let ok = SHGetFileInfoW(
            PCWSTR(path_w.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut sfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if ok == 0 || sfi.hIcon.is_invalid() {
            return None;
        }
        let img = hicon_to_rgba(sfi.hIcon);
        let _ = DestroyIcon(sfi.hIcon);
        img
    }
}

/// HICON -> 32bpp RGBA(非预乘);GetIconInfo 成功后位图句柄由调用方释放
unsafe fn hicon_to_rgba(hicon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<IconImage> {
    unsafe {
        let mut info = ICONINFO::default();
        GetIconInfo(hicon, &mut info).ok()?;
        let bitmap = hbitmap_size(info.hbmColor);
        let img = bitmap.and_then(|(w, h)| read_rgba(info.hbmColor, w, h));
        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(info.hbmColor.into());
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(info.hbmMask.into());
        }
        img
    }
}

/// 位图尺寸(GetObjectW 取 BITMAP 头)
unsafe fn hbitmap_size(hbm: HBITMAP) -> Option<(i32, i32)> {
    unsafe {
        let mut bmp = BITMAP::default();
        let n = GetObjectW(
            hbm.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bmp as *mut BITMAP as *mut core::ffi::c_void),
        );
        (n != 0 && bmp.bmWidth > 0 && bmp.bmHeight > 0).then_some((bmp.bmWidth, bmp.bmHeight))
    }
}

/// 读取位图像素为 top-down RGBA;负高度指定 top-down,避免手工翻转
unsafe fn read_rgba(hbm: HBITMAP, w: i32, h: i32) -> Option<IconImage> {
    unsafe {
        let hdc: HDC = CreateCompatibleDC(None);
        if hdc.is_invalid() {
            return None;
        }
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(
            hdc,
            hbm,
            0,
            h as u32,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            &mut bi,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(hdc);
        if lines != h {
            return None;
        }
        // GetDIBits 输出 BGRA;翻转通道为 RGBA(数据非预乘,由 egui 建纹理时处理)
        for px in buf.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        // 个别老图标 alpha 位无效(全 0):视为不透明,避免整图消失
        if buf.iter().skip(3).step_by(4).all(|&a| a == 0) {
            for a in buf.iter_mut().skip(3).step_by(4) {
                *a = 255;
            }
        }
        Some(IconImage { width: w as u32, height: h as u32, rgba: buf })
    }
}
