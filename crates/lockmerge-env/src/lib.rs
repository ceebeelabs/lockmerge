//! The side effects of the lockmerge driver that concern git (ADR 0010, ADR
//! 0011, `docs/03-architecture.md` section 1 "Side-effect boundaries").
//!
//! - [`labels`] is pure: it reads the arguments `%S %X %Y` and the
//!   `GITHEAD_<sha>` variables and says which operation git is running and
//!   which commit is theirs.
//! - [`Git`] shells out to the `git` executable (no git library) to resolve
//!   commits, to prove that ours is `HEAD`, and to read and merge manifests.
//! - [`CargoHome`] reads Cargo's registry index cache for the native-links
//!   proof (read-only, offline).
//! - [`GitManifests`] implements core's [`lockmerge_core::Manifests`].
//!
//! Nothing here guesses. Whatever cannot be proven is
//! [`ManifestResult::Unavailable`], which makes the format apply its safe
//! direction (ADR 0008 R14).

#![forbid(unsafe_code)]

mod cargo_home;
mod git;
pub mod labels;

pub use cargo_home::CargoHome;
pub use git::{DriverArgs, Git, GitManifests, Identified, githead_from_env};
pub use lockmerge_core::{IndexCache, ManifestResult, Manifests, Why};
