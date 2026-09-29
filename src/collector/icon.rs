//! 进程图标提取:SHGetFileInfoW 取 exe 关联图标(32x32),GetIconInfo +
//! GetDIBits 转为 RGBA 像素供 egui 纹理使用。
//!
//! 读取文件与 GDI 调用可能阻塞数十毫秒,调用方须在工作线程执行;
//! 提取链:SHGetFileInfoW → ExtractIconExW(资源型 exe 的兜底)→
//! 仍失败返回 None,由调用方缓存避免重复尝试。
//! [`default_app_icon`] 提供 Windows 默认"应用程序"图标兜底
//! (SHGFI_USEFILEATTRIBUTES 按扩展名查询,不触碰真实文件)。

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HBITMAP, HDC,
};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::UI::Shell::{
    ExtractIconExW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES,
    SHGetFileInfoW,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};
use windows::core::PCWSTR;

/// 提取出的图标位图(非预乘 alpha)
pub struct IconImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 提取映像文件的关联图标;SHGetFileInfoW 失败再试 ExtractIconExW
/// (个别 exe 的关联图标查询失败但资源可直读),仍失败返回 None
pub fn extract(path: &str) -> Option<IconImage> {
    via_shgetfileinfo(path).or_else(|| via_extract_icon(path))
}

/// Windows 默认"应用程序"图标(通用 exe 样式):按扩展名属性提取,
/// 不触碰真实文件;用于无路径(服务进程反查受限)或提取失败的进程兜底。
/// 纯注册表查询,无文件 IO,同步调用可接受
pub fn default_app_icon() -> Option<IconImage> {
    shgetfileinfo_hicon(
        "placeholder.exe",
        SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES,
    )
    .and_then(|hicon| {
        let img = unsafe { hicon_to_rgba(hicon) };
        unsafe {
            let _ = DestroyIcon(hicon);
        }
        img
    })
}

/// SHGetFileInfoW 路径查询取关联图标
fn via_shgetfileinfo(path: &str) -> Option<IconImage> {
    shgetfileinfo_hicon(path, SHGFI_ICON | SHGFI_LARGEICON).and_then(|hicon| {
        let img = unsafe { hicon_to_rgba(hicon) };
        unsafe {
            let _ = DestroyIcon(hicon);
        }
        img
    })
}

/// SHGetFileInfoW 取 HICON(通用入口;flags 决定是否走文件属性模式)
fn shgetfileinfo_hicon(path: &str, flags: windows::Win32::UI::Shell::SHGFI_FLAGS) -> Option<HICON> {
    unsafe {
        let mut path_w: Vec<u16> = path.encode_utf16().collect();
        path_w.push(0);
        let mut sfi = SHFILEINFOW::default();
        let ok = SHGetFileInfoW(
            PCWSTR(path_w.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut sfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        );
        (ok != 0 && !sfi.hIcon.is_invalid()).then_some(sfi.hIcon)
    }
}

/// ExtractIconExW 直读 exe 图标资源(第一枚大图标)
fn via_extract_icon(path: &str) -> Option<IconImage> {
    unsafe {
        let mut path_w: Vec<u16> = path.encode_utf16().collect();
        path_w.push(0);
        let mut hicon = HICON::default();
        let n = ExtractIconExW(PCWSTR(path_w.as_ptr()), 0, Some(&mut hicon), None, 1);
        if n == 0 || hicon.is_invalid() {
            return None;
        }
        let img = hicon_to_rgba(hicon);
        let _ = DestroyIcon(hicon);
        img
    }
}

/// HICON -> 32bpp RGBA(非预乘);GetIconInfo 成功后位图句柄由调用方释放
unsafe fn hicon_to_rgba(
    hicon: windows::Win32::UI::WindowsAndMessaging::HICON,
) -> Option<IconImage> {
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
        Some(IconImage {
            width: w as u32,
            height: h as u32,
            rgba: buf,
        })
    }
}
