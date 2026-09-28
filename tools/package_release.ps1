# Release 便携包打包:构建 release 并产出 target/dist 下的便携 zip。
# 全部资源(底图/GeoIP/图标字体/词条)编译期内嵌,发布产物仅 exe,
# 用户数据(config.toml / data/)在首次运行时于 exe 同级生成。

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)

cargo build --release

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "netowl_${version}_windows_x64-portable"
$stage = "target/dist/$name"

New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item target/release/netowl.exe $stage/
Compress-Archive -Path "$stage/*" -DestinationPath "target/dist/$name.zip" -Force
Remove-Item -Recurse -Force $stage

Write-Host "已打包 target/dist/$name.zip"
