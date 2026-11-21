//! How the merge driver is configured in a replay.
//!
//! The replay must configure the driver exactly as `lockmerge install` does,
//! from one source in code (docs/02-oracle.md section 3.3): the constants of
//! `lockmerge_core::driver_config`, which `lockmerge install` writes too.

pub use lockmerge_core::driver_config::{DRIVER as COMMAND, NAME as NAME_VALUE};

/// The `.git/info/attributes` line that routes a lockfile to the driver.
pub fn attributes_line(file_name: &str) -> String {
    format!("{file_name} merge=lockmerge\n")
}
