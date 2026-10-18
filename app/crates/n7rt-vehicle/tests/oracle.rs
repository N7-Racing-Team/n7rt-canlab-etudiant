//! Tests of the private oracle (docs/distribution.md §1). Their sources live with the oracle, in the instructor
//! repository (`oracle/n7rt-vehicle/tests/`), and are compiled here only when N7RT_ORACLE_DIR names them at build
//! time (build.rs, cfg `n7rt_oracle`). A public build compiles this file to an empty test crate.
#![cfg(n7rt_oracle)]
#![recursion_limit = "512"]

/// Receiver-side E2E checks, state machine and reference consumer chain on a synthetic pack.
#[allow(dead_code, unused_imports)]
mod generic {
    include!(concat!(env!("N7RT_ORACLE_SRC"), "/tests/generic.rs"));
}

/// Variants and student packs against the reference consumer, judgement of run files.
#[allow(dead_code, unused_imports)]
mod distribution {
    include!(concat!(env!("N7RT_ORACLE_SRC"), "/tests/distribution.rs"));
}

/// The private APPS mission files (pack, golden vectors, chronograms) against the engine and the oracle.
#[allow(dead_code, unused_imports)]
mod restricted_apps {
    include!(concat!(env!("N7RT_ORACLE_SRC"), "/tests/restricted_apps.rs"));
}
#[allow(dead_code, unused_imports)]
mod restricted_wsf { include!(concat!(env!("N7RT_ORACLE_SRC"), "/tests/restricted_wsf.rs")); }
#[allow(dead_code, unused_imports)]
mod restricted_sas { include!(concat!(env!("N7RT_ORACLE_SRC"), "/tests/restricted_sas.rs")); }
#[allow(dead_code, unused_imports)]
mod restricted_bps { include!(concat!(env!("N7RT_ORACLE_SRC"), "/tests/restricted_bps.rs")); }
