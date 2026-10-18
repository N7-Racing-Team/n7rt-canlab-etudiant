//! Physical benches and expected results are held in the restricted instructor repository.
#![cfg(n7rt_lab_oracle_tests)]
mod wsf { include!(concat!(env!("N7RT_LAB_ORACLE_TESTS"), "/wsf.rs")); }
mod sas { include!(concat!(env!("N7RT_LAB_ORACLE_TESTS"), "/sas.rs")); }
mod sas_output { include!(concat!(env!("N7RT_LAB_ORACLE_TESTS"), "/sas_output.rs")); }
mod bps { include!(concat!(env!("N7RT_LAB_ORACLE_TESTS"), "/bps.rs")); }
mod four { include!(concat!(env!("N7RT_LAB_ORACLE_TESTS"), "/four.rs")); }
mod workflow { include!(concat!(env!("N7RT_LAB_ORACLE_TESTS"), "/workflow.rs")); }
mod workflow_flow { include!(concat!(env!("N7RT_LAB_ORACLE_TESTS"), "/workflow_flow.rs")); }
mod adversarial { include!(concat!(env!("N7RT_LAB_ORACLE_TESTS"), "/adversarial.rs")); }
