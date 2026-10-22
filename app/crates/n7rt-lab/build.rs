//! Embeds the pinned N7RT runtime so that the application can check the board's `HELLO_ACK`
//! challenge (SHA-256(nonce ‖ runtime image)).
//!
//! Single source of truth per board family: `firmware/n7rt_student/runtime-release.json` (NUCLEO-H753ZI)
//! and `runtime-release-g0b1.json` (NUCLEO-G0B1RE): version, build id, ABI, size, SHA-256. The bytes come
//! from the student SDK object `firmware/n7rt_student/N7RT_CANLab_Student/<n7rt|n7rt_g0b1>/lib/n7rt_runtime.o`,
//! section `.n7rt_runtime`, the very image that is flashed with every student build. The build fails if
//! they disagree.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

fn project_root() -> PathBuf {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    manifest.ancestors().nth(3).expect("app/crates/n7rt-lab is three levels below the project").to_path_buf()
}

fn u16_at(b: &[u8], o: usize) -> usize {
    u16::from_le_bytes([b[o], b[o + 1]]) as usize
}

fn u32_at(b: &[u8], o: usize) -> usize {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize
}

/// Contents of one section of a little-endian ELF32 relocatable object.
fn elf32_section<'a>(elf: &'a [u8], wanted: &str, path: &Path) -> &'a [u8] {
    assert!(elf.len() > 0x34 && elf[..4] == *b"\x7fELF" && elf[4] == 1 && elf[5] == 1, "{}: not an ELF32 LE object", path.display());
    let (shoff, shentsize, shnum, shstrndx) = (u32_at(elf, 0x20), u16_at(elf, 0x2E), u16_at(elf, 0x30), u16_at(elf, 0x32));
    let header = |i: usize| &elf[shoff + i * shentsize..shoff + (i + 1) * shentsize];
    let strtab_off = u32_at(header(shstrndx), 0x10);
    for i in 0..shnum {
        let h = header(i);
        let name_off = strtab_off + u32_at(h, 0);
        let end = elf[name_off..].iter().position(|&c| c == 0).unwrap() + name_off;
        if &elf[name_off..end] == wanted.as_bytes() {
            let (off, size) = (u32_at(h, 0x10), u32_at(h, 0x14));
            return &elf[off..off + size];
        }
    }
    panic!("{}: no section {wanted}", path.display());
}

fn main() {
    println!("cargo::rustc-check-cfg=cfg(n7rt_lab_oracle_tests)");
    println!("cargo::rerun-if-env-changed=N7RT_ORACLE_DIR");
    if let Some(dir) = env::var_os("N7RT_ORACLE_DIR").filter(|d| !d.is_empty()) {
        let dir = PathBuf::from(dir);
        assert!(dir.is_absolute(), "oracle sources require an absolute path");
        let tests = dir.parent().expect("oracle parent").join("n7rt-lab/tests");
        if tests.is_dir() {
            println!("cargo::rerun-if-changed={}", tests.display());
            println!("cargo::rustc-cfg=n7rt_lab_oracle_tests");
            println!("cargo::rustc-env=N7RT_LAB_ORACLE_TESTS={}", tests.display().to_string().replace('\\', "/"));
        }
    }
    let root = project_root();
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    // One pinned runtime per board family: NUCLEO-H753ZI (module `pinned`), NUCLEO-G0B1RE (`pinned_g0b1`).
    let h753 = pinned_release(&root, "runtime-release.json", "n7rt", "n7rt_runtime.bin", &out);
    let g0b1 = pinned_release(&root, "runtime-release-g0b1.json", "n7rt_g0b1", "n7rt_runtime_g0b1.bin", &out);
    fs::write(out.join("runtime_release.rs"), h753).unwrap();
    fs::write(out.join("runtime_release_g0b1.rs"), g0b1).unwrap();
}

/// Constants of one pinned runtime, checked against the runtime object of its SDK folder.
fn pinned_release(root: &Path, release_name: &str, sdk_dir: &str, image_name: &str, out: &Path) -> String {
    let release_path = root.join("firmware/n7rt_student").join(release_name);
    let object_path = root.join("firmware/n7rt_student/N7RT_CANLab_Student").join(sdk_dir).join("lib/n7rt_runtime.o");
    println!("cargo:rerun-if-changed={}", release_path.display());
    println!("cargo:rerun-if-changed={}", object_path.display());

    let release: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&release_path).expect(release_name)).expect("valid JSON");
    let object = fs::read(&object_path).unwrap_or_else(|e| panic!("{}: {e} (run `stm32lab sdk`)", object_path.display()));
    let image = elf32_section(&object, ".n7rt_runtime", &object_path);

    let version = release["runtime_version"].as_str().unwrap();
    let build_id = release["build_id"].as_str().unwrap();
    let size = release["image_size"].as_u64().unwrap() as usize;
    let sha = release["image_sha256"].as_str().unwrap();
    let abi: Vec<u64> = release["abi"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    let actual: String = Sha256::digest(image).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(image.len(), size, "SDK runtime size differs from {release_name}");
    assert_eq!(actual, sha, "SDK runtime SHA-256 differs from {release_name}");
    assert_eq!(build_id.len(), 16, "build_id is 8 bytes");
    let mut parts = version.split('.').map(|p| p.parse::<u8>().expect("semver"));
    let (major, minor, patch) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());

    fs::write(out.join(image_name), image).unwrap();
    let build_id_bytes: Vec<String> =
        (0..8).map(|i| format!("0x{}", &build_id[2 * i..2 * i + 2])).collect();
    format!(
        "/// Pinned runtime (generated from firmware/n7rt_student/{release_name}).\n\
         pub const VERSION: (u8, u8, u8) = ({major}, {minor}, {patch});\n\
         pub const VERSION_STR: &str = \"{version}\";\n\
         pub const BUILD_ID: [u8; 8] = [{ids}];\n\
         pub const BUILD_ID_HEX: &str = \"{build_id}\";\n\
         pub const ABI: (u16, u16) = ({abi0}, {abi1});\n\
         pub const IMAGE_SHA256: &str = \"{sha}\";\n\
         pub const IMAGE: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{image_name}\"));\n",
        ids = build_id_bytes.join(", "),
        abi0 = abi[0],
        abi1 = abi[1],
    )
}
