//! Oracle switch (docs/distribution.md §1). When the environment variable N7RT_ORACLE_DIR names the private oracle
//! sources of this crate (instructor repository: `oracle/n7rt-vehicle`), they are compiled in: cfg `n7rt_oracle`, and
//! N7RT_ORACLE_SRC tells `include!` where they are. Otherwise the public stand-ins of `src/public/` are compiled.
//! Opt-in only: nothing is looked up on disk, so a build of the student repository never picks up an instructor
//! checkout lying next to it.

use std::path::PathBuf;

const FILES: [&str; 7] = ["consumer.rs", "supervision.rs", "e2e_check.rs", "judge.rs",
    "consumer_wsf.rs", "consumer_sas.rs", "consumer_bps.rs"];

fn main() {
    println!("cargo::rustc-check-cfg=cfg(n7rt_oracle)");
    println!("cargo::rerun-if-env-changed=N7RT_ORACLE_DIR");
    let Some(dir) = std::env::var_os("N7RT_ORACLE_DIR").filter(|d| !d.is_empty()) else {
        return;
    };
    // A relative path would be resolved from this crate's folder (cargo runs build scripts there), not from the
    // caller's: refuse it. Cargo's [env] with `relative = true` and the instructor scripts give absolute paths.
    let dir = PathBuf::from(dir);
    if !dir.is_absolute() {
        panic!("N7RT_ORACLE_DIR doit être un chemin absolu (reçu : {})", dir.display());
    }
    for f in FILES {
        let p = dir.join(f);
        if !p.is_file() {
            panic!("N7RT_ORACLE_DIR = {} : {f} absent (sources de l'oracle incomplètes)", dir.display());
        }
        println!("cargo::rerun-if-changed={}", p.display());
    }
    let tests = dir.join("tests");
    if tests.is_dir() {
        println!("cargo::rerun-if-changed={}", tests.display());
    }
    println!("cargo::rustc-cfg=n7rt_oracle");
    println!("cargo::rustc-env=N7RT_ORACLE_SRC={}", dir.display().to_string().replace('\\', "/"));
}
