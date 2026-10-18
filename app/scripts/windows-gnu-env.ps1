# Build environment for Windows WITHOUT Visual Studio Build Tools (Rust host x86_64-pc-windows-gnu).
# Dot-source it in the PowerShell session that builds the app:   . .\scripts\windows-gnu-env.ps1
#
# Prerequisites (user-level, no admin), see app/README.md "Windows sans MSVC":
#   - rustup, toolchain stable-x86_64-pc-windows-gnu (rustup-init --default-host x86_64-pc-windows-gnu)
#   - zig 0.15.x in %LOCALAPPDATA%\n7rt-toolchain\zig-0.15.2
#   - tools/zig-shims built and copied as bin\windres.exe, bin\dlltool.exe and lld\ld.exe there
# With the standard MSVC toolchain none of this is needed.

$toolchain = Join-Path $env:LOCALAPPDATA "n7rt-toolchain"
$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
foreach ($p in @("$toolchain\bin\windres.exe", "$toolchain\bin\dlltool.exe", "$toolchain\lld\ld.exe", "$toolchain\zig-0.15.2\zig.exe", "$cargoBin\cargo.exe")) {
    if (-not (Test-Path $p)) { Write-Warning "absent : $p (voir app/README.md)" }
}
$env:PATH = "$toolchain\bin;$cargoBin;$env:PATH"
# GNU ld mis-links MSVC-style short import libraries (raw-dylib); gcc is routed to rust-lld.
$env:RUSTFLAGS = "-C link-arg=-B$($toolchain -replace '\\','/')/lld/"
Write-Host "N7RT CAN Lab : environnement Windows GNU prêt (cargo, windres/dlltool via zig, édition de liens rust-lld)."
