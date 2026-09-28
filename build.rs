//! Windows 目标的 exe 资源编译:图标、版本信息与应用 manifest
//! (UAC 提权级别、DPI 感知、系统兼容性)。其余目标为空操作。

/// 应用 manifest:highestAvailable = 管理员组用户启动时提权(ETW 流量事件
/// 与 WFP 拦截需管理员权限),标准用户以普通权限运行并走应用内降级只读;
/// PerMonitorV2 在进程启动前生效,先于 winit 的运行时设置消除首批 DPI 抖动。
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="1.0.0.0" processorArchitecture="*" name="NetOwl" type="win32"/>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="highestAvailable" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 10 / Windows 11 / Windows Server 2022 -->
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
      <supportedOS Id="{a4f06c33-274c-4d2c-9b1e-1c9e4b0a4f2a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/app_icon.ico");
    // 版本号按 16 位一段映射(major/minor/patch,第四段补 0);
    // tag 预发布段无法进入 16 位整数,字符串版本恒取 Cargo.toml 干净版本
    let version = env!("CARGO_PKG_VERSION");
    let mut parts = version.split('.').map(|s| s.parse::<u64>().unwrap_or(0));
    let (major, minor, patch) = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    let numeric = (major << 48) | (minor << 32) | (patch << 16);
    res.set_version_info(winresource::VersionInfo::PRODUCTVERSION, numeric);
    res.set_version_info(winresource::VersionInfo::FILEVERSION, numeric);
    res.set("ProductName", "NetOwl");
    res.set("ProductVersion", version);
    res.set("FileVersion", version);
    res.set("FileDescription", "NetOwl 网络连接监控");
    res.set("OriginalFilename", "netowl.exe");
    res.set_manifest(MANIFEST);
    res.compile().expect("编译 Windows exe 资源(图标/manifest)失败");
}
