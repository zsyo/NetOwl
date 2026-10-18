//! 系统已安装字体枚举:读 Windows 字体注册表键,产出 (显示名, 文件路径) 列表,
//! 供设置页"界面字体"下拉选择。只读枚举,免管理员权限。

use std::collections::HashSet;

use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, RegCloseKey, RegEnumValueW,
    RegOpenKeyExW, RegQueryInfoKeyW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

/// 字体注册表键(HKLM 与 HKCU 同路径,后者对每用户安装的字体生效)
const FONTS_KEY: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts";
/// 系统字体目录:值数据只给文件名时拼接(带全路径的值原样使用)
const FONTS_DIR: &str = "C:\\Windows\\Fonts";

/// 一条可选字体:显示名(下拉展示兼配置持久化值)与字体文件路径
#[derive(Debug, Clone)]
pub struct FontEntry {
    pub name: String,
    pub path: String,
}

/// 枚举 HKLM 与 HKCU 两处字体键,按显示名去重、按名称排序。
/// 单个键打不开(不存在/无权限)只损失该来源,不影响另一处与整体结果
pub fn installed_fonts() -> Vec<FontEntry> {
    let mut out: Vec<FontEntry> = Vec::new();
    unsafe {
        collect_key(HKEY_LOCAL_MACHINE, &mut out);
        collect_key(HKEY_CURRENT_USER, &mut out);
    }
    // 同一路径可能被多个值名指向(如 "X & X UI"),按路径去重保留先出现者
    // (HKLM 优先);再按显示名排序,便于下拉浏览
    let mut seen: HashSet<String> = HashSet::with_capacity(out.len());
    out.retain(|e| seen.insert(e.path.to_lowercase()));
    out.sort_by_cached_key(|a| a.name.to_lowercase());
    out
}

/// 枚举单个根的字体键,把 (显示名, 路径) 追加到 out
unsafe fn collect_key(root: HKEY, out: &mut Vec<FontEntry>) {
    unsafe {
        let mut key = HKEY::default();
        let sub = HSTRING::from(FONTS_KEY);
        if RegOpenKeyExW(root, PCWSTR(sub.as_ptr()), None, KEY_READ, &mut key).is_err() {
            return;
        }
        let mut count = 0u32;
        let mut max_name_len = 0u32;
        let mut max_data_len = 0u32;
        // 取不到容量信息视为无值可枚举
        if RegQueryInfoKeyW(
            key,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&mut count),
            Some(&mut max_name_len),
            Some(&mut max_data_len),
            None,
            None,
        )
        .is_err()
        {
            let _ = RegCloseKey(key);
            return;
        }
        // 名称长度单位是字符(不含结尾 null),数据长度单位是字节
        let mut name_buf = vec![0u16; max_name_len as usize + 1];
        // 数据是 UTF-16 字符串,按 u16 单元申请(字节数 / 2,向上取整防奇数),
        // 多留一个单元容纳结尾 null
        let mut data_buf = vec![0u16; (max_data_len as usize).div_ceil(2) + 1];
        for i in 0..count {
            let mut name_len = name_buf.len() as u32;
            // lpcbData 入参单位是字节:按缓冲区字节数传。错按 u16 单元数传会把
            // 可用长度砍半,长路径(每用户安装字体)全被 ERROR_MORE_DATA 静默丢弃
            let mut data_len = data_buf.len() as u32 * 2;
            let mut value_type = 0u32;
            let err = RegEnumValueW(
                key,
                i,
                Some(PWSTR(name_buf.as_mut_ptr())),
                &raw mut name_len,
                None,
                Some(&mut value_type),
                Some(data_buf.as_mut_ptr().cast()),
                Some(&raw mut data_len),
            );
            if err == ERROR_NO_MORE_ITEMS {
                break;
            }
            if err != ERROR_SUCCESS {
                continue;
            }
            if value_type != REG_SZ.0 {
                continue;
            }
            let name = match utf16_slice(&name_buf[..name_len as usize]) {
                Some(n) => clean_name(&n),
                None => continue,
            };
            // 回写的数据长度单位是字节,换回 u16 单元数
            let file = match utf16_slice(&data_buf[..data_len as usize / 2]) {
                Some(f) => f,
                None => continue,
            };
            if let Some(entry) = FontEntry::new(&name, &file) {
                out.push(entry);
            }
        }
        let _ = RegCloseKey(key);
    }
}

/// UTF-16 缓冲区切片(去掉结尾 null)转 String;非法序列返回 None(跳过该条)
fn utf16_slice(units: &[u16]) -> Option<String> {
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16(&units[..end]).ok()
}

/// 显示名清洗:去 " (TrueType)"/" (OpenType)" 后缀,取 " & " 前第一段
/// ("Microsoft YaHei & Microsoft YaHei UI" → "Microsoft YaHei",与系统
/// 字体文件夹里显示的主名一致)
fn clean_name(raw: &str) -> String {
    let base = raw
        .trim()
        .trim_end_matches(" (TrueType)")
        .trim_end_matches(" (OpenType)")
        .trim();
    base.split(" & ").next().unwrap_or(base).trim().to_owned()
}

impl FontEntry {
    /// 由注册表显示名与文件名构建;位图字体(.fon/.fnt,egui 无法解析)跳过
    fn new(name: &str, file: &str) -> Option<Self> {
        let lower = file.to_lowercase();
        if lower.ends_with(".fon") || lower.ends_with(".fnt") {
            return None;
        }
        let path = if file.contains('\\') || file.contains('/') {
            file.to_owned()
        } else {
            format!("{FONTS_DIR}\\{file}")
        };
        Some(Self {
            name: name.to_owned(),
            path,
        })
    }
}
