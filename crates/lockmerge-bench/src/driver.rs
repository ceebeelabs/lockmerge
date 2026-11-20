//! How the merge driver is configured in a replay.
//!
//! The replay must configure the driver exactly as `lockmerge install` does,
//! from one source in code (docs/02-oracle.md section 3.3). `lockmerge-cli`
//! (#34) does not exist yet, so these constants live here for now. When it
//! lands, `install` and the bench must read the same definition; that move is
//! a follow-up of #35.
//!
//! The command is the bare name `lockmerge`, found through `PATH` (ADR 0003),
//! with the labels `%S %X %Y` that the driver uses to identify the commits
//! (ADR 0010).

/// The `merge.lockmerge.name` value.
pub const NAME_VALUE: &str = "lockmerge lockfile merge driver";

/// The `merge.lockmerge.driver` value.
pub const COMMAND: &str = "lockmerge merge-driver %O %A %B %L %P %S %X %Y";

/// The `.git/info/attributes` line that routes a lockfile to the driver.
pub fn attributes_line(file_name: &str) -> String {
    format!("{file_name} merge=lockmerge\n")
}
