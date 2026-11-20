//! Pure types, traits and the three-way engine of lockmerge.
//!
//! lockmerge combines only what it can prove unambiguous and refuses
//! everything else. This crate makes that structural (see
//! `docs/03-architecture.md`):
//!
//! - A merged unit exists only with a [`Proof`] that names a rule the format
//!   registered in [`Pipeline::rules`]; the engine checks every proof.
//! - [`Pipeline::serialize`] takes a [`Sealed`] value, which only the engine can
//!   build, and only after every check passed.
//! - A case no rule accepts is refused with `core.UNPROVEN`.
//!
//! The crate depends on `std` only and does no I/O: no processes, files or
//! environment (enforced by `clippy.toml`).

#![forbid(unsafe_code)]

pub mod engine;
mod format;
mod ids;
mod proof;
mod report;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use format::{
    Conflict, Context, Findings, Format, Inputs, ManifestResult, Manifests, Pipeline, Removal,
    Role, Verdict, Why,
};
pub use ids::{CodeDef, FormatId, ReasonCode, RuleDef};
pub use proof::{Draft, Proof, Sealed};
pub use report::{Applied, Clean, Error, How, Outcome, Refusal, Report, Side};
