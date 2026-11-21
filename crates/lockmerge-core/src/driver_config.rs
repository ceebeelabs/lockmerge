//! How the merge driver is registered in git: the one definition that
//! `lockmerge install` writes and the benchmark replays (docs/02-oracle.md
//! section 3.3).
//!
//! The command is the bare name `lockmerge`, found through `PATH` (ADR 0003),
//! with the labels `%S %X %Y` that the driver uses to identify the commits
//! (ADR 0010).

/// The `merge.lockmerge.name` value.
pub const NAME: &str = "lockmerge lockfile merge driver";

/// The `merge.lockmerge.driver` value.
pub const DRIVER: &str = "lockmerge merge-driver %O %A %B %L %P %S %X %Y";
