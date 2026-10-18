//! Stand-ins for MinGW binutils on a Windows machine without MSVC nor MSYS2, for the
//! `x86_64-pc-windows-gnu` Rust target. One executable, dispatching on its own name; install it
//! as `bin/windres.exe`, `bin/dlltool.exe` (put `bin` first in PATH) and `lld/ld.exe` (passed to
//! gcc with `-B`). See app/README.md, "Windows sans MSVC", and app/scripts/windows-gnu-env.ps1.
//!
//! - `windres`: `tauri-build` compiles the Windows resources (icon, version, manifest) through
//!   `embed-resource`, which runs `windres --input X --output Y --output-format=coff ...`. This is
//!   translated to `zig rc` (zig >= 0.14 writes COFF objects, preprocessing with its MinGW headers).
//! - `dlltool`: rustc builds raw-dylib import libraries (crate `windows-link`) with
//!   `dlltool -d X.def -D X.dll -l X.lib -m i386:x86-64 -f --64 --no-leading-underscore
//!   --temp-prefix P`. GNU dlltool needs an assembler; `zig dlltool` (llvm-dlltool) does not.
//! - `ld`: those import libraries are MSVC-style "short" ones. GNU ld mis-orders the import
//!   address table when they name a DLL that MinGW import libraries also name (observed: the
//!   tauri build script crashed on an unpatched `GetModuleHandleA` slot), so linking goes to
//!   `rust-lld -flavor gnu`, which handles both formats.
//!
//! zig is found through `N7RT_ZIG`, otherwise at `<this exe>/../zig-0.15.2/zig.exe`; rust-lld
//! through `N7RT_RUST_LLD`, otherwise in the sysroot of the active `rustc`.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

fn zig() -> PathBuf {
    if let Some(p) = env::var_os("N7RT_ZIG") {
        return PathBuf::from(p);
    }
    let exe = env::current_exe().expect("current exe");
    exe.parent().and_then(Path::parent).expect("toolchain dir").join("zig-0.15.2").join("zig.exe")
}

fn fail(msg: &str) -> ! {
    eprintln!("zig-shims: {msg}");
    exit(2)
}

fn run(args: Vec<String>) -> ! {
    let status = Command::new(zig())
        .args(&args)
        .status()
        .unwrap_or_else(|e| fail(&format!("cannot run zig ({e}); set N7RT_ZIG")));
    exit(status.code().unwrap_or(1))
}

fn machine(target: &str) -> &'static str {
    match target {
        "pe-x86-64" | "pe-x86_64" => "x64",
        "pe-i386" => "x86",
        "pe-aarch64-little" => "arm64",
        other => fail(&format!("unsupported windres target {other}")),
    }
}

fn windres(argv: Vec<String>) -> ! {
    let mut args = argv.into_iter();
    let (mut input, mut output, mut target) = (None, None, "pe-x86-64".to_string());
    let mut rc: Vec<String> = Vec::new();
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().unwrap_or_else(|| fail(&format!("{name} needs a value")));
        match a.as_str() {
            "-i" | "--input" => input = Some(value(&a)),
            "-o" | "--output" => output = Some(value(&a)),
            "-I" | "--include-dir" => rc.extend(["/i".into(), value(&a)]),
            "-D" | "--define" => rc.extend(["/d".into(), value(&a)]),
            "-c" | "--codepage" => rc.extend(["/c".into(), value(&a)]),
            "-F" | "--target" => target = value(&a),
            "-O" => {
                if value(&a) != "coff" {
                    fail("only COFF output is supported");
                }
            }
            "--output-format=coff" | "--no-preprocess" => {}
            "-V" | "--version" => {
                println!("zig-shims windres (zig rc)");
                exit(0)
            }
            _ if a.starts_with("--input=") => input = Some(a["--input=".len()..].into()),
            _ if a.starts_with("--output=") => output = Some(a["--output=".len()..].into()),
            _ if a.starts_with("--include-dir=") => rc.extend(["/i".into(), a["--include-dir=".len()..].into()]),
            _ if a.starts_with("--target=") => target = a["--target=".len()..].into(),
            _ if a.starts_with("-I") => rc.extend(["/i".into(), a[2..].into()]),
            _ if a.starts_with("-D") => rc.extend(["/d".into(), a[2..].into()]),
            _ if a.starts_with("--output-format=") => fail(&format!("only --output-format=coff is supported ({a})")),
            _ if input.is_none() && !a.starts_with('-') => input = Some(a),
            _ if output.is_none() && !a.starts_with('-') => output = Some(a),
            _ => fail(&format!("unsupported windres option {a}")),
        }
    }
    let (Some(input), Some(output)) = (input, output) else { fail("--input and --output are required") };
    let mut z: Vec<String> = ["rc", "/:output-format", "coff", "/:target", machine(&target), "/:auto-includes", "gnu"]
        .map(String::from)
        .into();
    z.extend(rc);
    z.extend(["/fo".into(), output, "--".into(), input]);
    run(z)
}

fn dlltool(argv: Vec<String>) -> ! {
    let mut z = vec!["dlltool".to_string()];
    let mut args = argv.into_iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            // Temporary-file prefix of GNU dlltool: meaningless for llvm-dlltool.
            "--temp-prefix" => {
                args.next();
            }
            _ if a.starts_with("--temp-prefix=") => {}
            _ => z.push(a),
        }
    }
    run(z)
}

/// `rust-lld` of the active Rust toolchain (`N7RT_RUST_LLD` overrides).
fn rust_lld() -> PathBuf {
    if let Some(p) = env::var_os("N7RT_RUST_LLD") {
        return PathBuf::from(p);
    }
    let out = Command::new("rustc").args(["--print", "sysroot"]).output().unwrap_or_else(|e| fail(&format!("rustc: {e}")));
    let sysroot = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let host = Command::new("rustc").args(["-vV"]).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let host = host.lines().find_map(|l| l.strip_prefix("host: ")).unwrap_or("x86_64-pc-windows-gnu").to_string();
    PathBuf::from(sysroot).join("lib").join("rustlib").join(host).join("bin").join("rust-lld.exe")
}

/// `ld.lld` for `gcc -fuse-ld=lld`: GNU ld mis-orders the IAT when MSVC-style short import
/// libraries (llvm-dlltool, raw-dylib) and MinGW import libraries name the same DLL; lld does not.
fn ld_lld(argv: Vec<String>) -> ! {
    // gcc passes its own driver options through; lld's MinGW driver does not know them.
    let args: Vec<String> = argv.into_iter().filter(|a| !a.starts_with("-fuse-ld=") && a != "-plugin-opt=-pass-through=-lgcc").collect();
    if let Some(log) = env::var_os("N7RT_SHIM_LOG") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log) {
            let _ = writeln!(f, "ld -> rust-lld -flavor gnu ({} args)", args.len());
        }
    }
    let status = Command::new(rust_lld())
        .args(["-flavor", "gnu"])
        .args(&args)
        .status()
        .unwrap_or_else(|e| fail(&format!("cannot run rust-lld ({e}); set N7RT_RUST_LLD")));
    exit(status.code().unwrap_or(1))
}

fn main() {
    let exe = env::args().next().unwrap_or_default();
    let name = Path::new(&exe).file_name().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    let stem = name.strip_suffix(".exe").unwrap_or(&name).to_string();
    let args: Vec<String> = env::args().skip(1).collect();
    match stem.as_str() {
        s if s.ends_with("windres") => windres(args),
        s if s.ends_with("dlltool") => dlltool(args),
        "ld.lld" | "ld" => ld_lld(args),
        _ => fail("install this executable as windres.exe, dlltool.exe and lld/ld.exe"),
    }
}
