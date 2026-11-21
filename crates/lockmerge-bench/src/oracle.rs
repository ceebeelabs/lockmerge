//! The correctness oracle (docs/02-oracle.md section 4, ADR 0005).
//!
//! An oracle is written independently of the merge code: it shares nothing
//! with any format crate, and uses only a TOML or YAML parser. When it cannot
//! tell, it says "no", which can only make the gate stricter.

use std::collections::BTreeSet;

mod cargo;

pub use cargo::{CargoOracle, path_package_names, registry_package_names};

pub trait Oracle {
    /// The package ids of a lockfile (section 3.2).
    fn ids(&self, bytes: &[u8]) -> Result<BTreeSet<String>, String>;
    /// L0: the bytes parse and are their own canonical serialization.
    fn canonical(&self, bytes: &[u8]) -> Result<(), String>;
    /// L1: the independent structural validity predicate.
    fn valid(&self, bytes: &[u8]) -> Result<(), String>;
    /// L2: a deterministic text of the lockfile after normalization. Two
    /// lockfiles are equal when their texts are. It erases only what the
    /// format document proves meaningless.
    fn normalize(&self, bytes: &[u8]) -> Result<String, String>;
}

/// The oracle for a fixture directory name, or `None` when the ecosystem has
/// none yet. A corpus without an oracle fails the gate; it is never counted.
pub fn for_ecosystem(ecosystem: &str) -> Option<Box<dyn Oracle>> {
    match ecosystem {
        "cargo" | "cargo-agent" => Some(Box::new(CargoOracle)),
        _ => None,
    }
}
