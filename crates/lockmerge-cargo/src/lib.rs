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
//! - The manifest rules (M1 to M3, B1, C, D, F) read `Cargo.toml` through
//!   [`lockmerge_core::Manifests`] ([`manifest`], [`find`]) and refuse with R14
//!   when a manifest is unavailable or not read exactly.
//! - The native-links proof (V6, refused as R15) reads Cargo's registry index
//!   cache through [`lockmerge_core::IndexCache`] ([`index`], pinned in
//!   `docs/formats/cargo-registry-index.md`) when each side added different
//!   registry crates. A git or path package among the added ones, an absent
//!   cache or an entry not read exactly is refused with R15.
//!
//! Unchanged inputs are returned byte for byte by the engine (`core.S1`)
//! before any parsing.

#![forbid(unsafe_code)]

mod codes;
pub mod find;
pub mod index;
pub mod manifest;
mod merge;
pub mod model;
mod moves;
pub mod parse;
mod rules;
mod shared;
pub mod write;

pub use codes::{CARGO, CODES, RULES};
pub use model::{Id, Lock, Package};

use lockmerge_core::{
    CodeDef, Conflict, Context, Draft, Error, Findings, Format, FormatId, Inputs, Pipeline, Proof,
    Refusal, Removal, Report, Role, RuleDef, Sealed, Verdict, engine,
};
use shared::Shared;
use std::cell::OnceCell;
use std::collections::BTreeMap;

/// The Cargo.lock format. Register it with the CLI through [`format`].
#[derive(Clone, Copy, Debug, Default)]
pub struct CargoFormat;

/// The format as the CLI and the benchmark see it.
pub fn format() -> &'static dyn Format {
    static CARGO_FORMAT: CargoFormat = CargoFormat;
    &CARGO_FORMAT
}

impl Format for CargoFormat {
    fn id(&self) -> FormatId {
        CARGO
    }

    fn matches(&self, file_name: &str) -> bool {
        file_name == "Cargo.lock"
    }

    fn rules(&self) -> &'static [RuleDef] {
        RULES
    }

    fn codes(&self) -> &'static [CodeDef] {
        CODES
    }

    fn merge(&self, inputs: &Inputs<'_>, cx: &Context<'_>) -> Result<Report, Error> {
        engine::run(&Run::new(inputs.path), inputs, cx)
    }
}

/// One merge run. The manifest rules need the three parsed lockfiles in the
/// per-key and prune steps, which the engine hands to the format only in the
/// envelope step, so the run keeps them from there on.
struct Run {
    path: String,
    shared: OnceCell<Shared>,
}

impl Run {
    fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            shared: OnceCell::new(),
        }
    }

    fn shared(&self) -> &Shared {
        self.shared
            .get()
            .expect("the engine runs the envelope before any other step")
    }
}

impl Pipeline for Run {
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
        let sh = self.shared.get_or_init(|| Shared::new(models, &self.path));
        rules::envelope(models, sh, cx)
    }

    fn units(&self, model: &Lock) -> BTreeMap<Id, Package> {
        model.packages.clone()
    }

    fn decide(&self, conflict: &Conflict<'_, Id, Package>, cx: &Context<'_>) -> Verdict<Package> {
        merge::decide(self.shared(), cx, conflict)
    }

    fn prune(&self, draft: &Draft<Id, Package>, cx: &Context<'_>) -> Vec<Removal<Id>> {
        merge::prune(self.shared(), draft, cx)
    }

    fn validate(&self, draft: &Draft<Id, Package>, cx: &Context<'_>) -> Findings {
        rules::validate(draft, self.shared(), cx)
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
