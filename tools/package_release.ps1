# Release 便携包打包:构建 release 并产出 target/dist 下的便携 zip。
# 底图/GeoIP/图标字体编译期内嵌,词条以 locales 目录外置随包分发,
# 用户数据(config.toml / data/)在首次运行时于 exe 同级生成。

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)

cargo build --release

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "netowl_${version}_windows_x64-portable"
$stage = "target/dist/$name"

New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item target/release/netowl.exe $stage/

# 词条外置:locales 整个目录进包,用户增删语言无需重新构建;
# 缺失或为空直接失败,不静默产出一个没有词条的便携包
$ftl = Get-ChildItem -Path "locales" -Filter "*.ftl" -File
if (-not $ftl) {
    throw "No locales/*.ftl found!"
}
Copy-Item -Recurse -Path "locales" -Destination "$stage/"

Compress-Archive -Path "$stage/*" -DestinationPath "target/dist/$name.zip" -Force
Remove-Item -Recurse -Force $stage

Write-Host "已打包 target/dist/$name.zip"
