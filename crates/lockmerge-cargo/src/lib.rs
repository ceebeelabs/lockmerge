//! The Cargo.lock format of lockmerge (`docs/formats/cargo-lock.md`, ADR 0008
//! and ADR 0014).
//!
//! - [`parse`] reads a lockfile with `toml_edit` read-only and refuses every
//!   form it does not handle (R1, R9 to R11, `CANON`).
//! - [`model`] keys packages by (name, version, source) and resolves every
//!   dependency edge to a full id per input file.
//! - [`write`] is the canonical writer for versions 3 and 4. No TOML library
//!   writes output (the `display` feature of `toml_edit` is off).
//! - [`CargoFormat`] applies the base rules of the engine to resolved
//!   packages, refuses what they cannot settle, validates the merged set (V2
//!   to V5) and writes it.
//!
//! Not part of this crate yet: the manifest rules (M1 to M3, B1, C, D, F) and
//! the native-links proof (R15). Where those rules would decide, this crate
//! refuses instead of guessing: a version move of a crate present in base is
//! refused with R14, different packages (registry, git or path) added by each side with
//! R15, and two changes to one package with R5, R6 or `core.UNPROVEN`.
//!
//! Unchanged inputs are returned byte for byte by the engine (`core.S1`)
//! before any parsing.

#![forbid(unsafe_code)]

mod codes;
pub mod model;
pub mod parse;
mod rules;
pub mod write;

pub use codes::{CARGO, CODES, RULES};
pub use model::{Id, Lock, Package};

use lockmerge_core::{
    CodeDef, Conflict, Context, Draft, Findings, Format, FormatId, Pipeline, Proof, Refusal, Role,
    RuleDef, Sealed, Verdict,
};
use std::collections::BTreeMap;

/// The Cargo.lock format. Register it with the CLI through [`format`].
#[derive(Clone, Copy, Debug, Default)]
pub struct CargoFormat;

/// The format as the CLI and the benchmark see it.
pub fn format() -> &'static dyn Format {
    static CARGO_FORMAT: CargoFormat = CargoFormat;
    &CARGO_FORMAT
}

impl Pipeline for CargoFormat {
    type Model = Lock;
    type Key = Id;
    type Unit = Package;

    fn id(&self) -> FormatId {
        CARGO
    }

    fn rules(&self) -> &'static [RuleDef] {
        RULES
    }

    fn codes(&self) -> &'static [CodeDef] {
        CODES
    }

    fn matches(&self, file_name: &str) -> bool {
        file_name == "Cargo.lock"
    }

    fn parse(&self, role: Role, bytes: &[u8]) -> Result<Lock, Refusal> {
        parse::parse(role, bytes)
    }

    fn envelope(&self, models: [&Lock; 3], cx: &Context<'_>) -> Result<Vec<Proof>, Refusal> {
        rules::envelope(models, cx)
    }

    fn units(&self, model: &Lock) -> BTreeMap<Id, Package> {
        model.packages.clone()
    }

    fn decide(&self, conflict: &Conflict<'_, Id, Package>, _cx: &Context<'_>) -> Verdict<Package> {
        rules::decide(conflict)
    }

    fn validate(&self, draft: &Draft<Id, Package>, cx: &Context<'_>) -> Findings {
        rules::validate(draft, cx)
    }

    fn serialize(&self, sealed: &Sealed<Id, Package>, models: [&Lock; 3]) -> Vec<u8> {
        let items: Vec<(&Id, &Package)> = sealed.iter().collect();
        // The envelope guarantees the three versions agree, and `validate`
        // (V5) that the set can be written; a panic here is caught by the
        // driver and leaves the file untouched (ADR 0013).
        write::render(models[0].version, &items)
            .expect("V5 checked that the merged set can be written")
    }
}
