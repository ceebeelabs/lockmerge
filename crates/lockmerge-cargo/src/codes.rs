//! The refusal codes and rules of the Cargo.lock format that need no manifest.
//! Codes follow the numbering of `docs/formats/cargo-lock.md` section 4.4; a
//! shipped code is never renamed.

use lockmerge_core::{CodeDef, FormatId, ReasonCode, RuleDef};

pub const CARGO: FormatId = FormatId("cargo");

const fn code(c: &'static str) -> ReasonCode {
    ReasonCode::new(CARGO, c)
}

/// An input does not parse, has no `version` key or one other than 3 or 4, or
/// the three versions differ.
pub const R1: ReasonCode = code("R1");
/// The same id has different checksums.
pub const R2: ReasonCode = code("R2");
/// Both sides moved the same package to different versions.
pub const R3: ReasonCode = code("R3");
/// An edge in the merged set points at no package.
pub const R4: ReasonCode = code("R4");
/// A package one branch removed and the other changed that M3 cannot drop.
pub const R5: ReasonCode = code("R5");
/// Both branches changed a path package's dependencies and M2 cannot combine
/// them.
pub const R6: ReasonCode = code("R6");
/// A non-path package cannot be reached from a path package.
pub const R7: ReasonCode = code("R7");
/// Two semver-compatible versions of a registry crate in the merged set.
pub const R8: ReasonCode = code("R8");
/// An edge is ambiguous or unresolvable, or an id appears twice.
pub const R9: ReasonCode = code("R9");
/// A key or form this crate does not handle.
pub const R10: ReasonCode = code("R10");
/// A prerelease against a release on one package, or a non-semver version.
pub const R11: ReasonCode = code("R11");
/// A source string changed for the same name and version.
pub const R12: ReasonCode = code("R12");
/// A rule needs the manifests and they are not available, not identifiable or
/// not read exactly.
pub const R14: ReasonCode = code("R14");
/// Different registry or git ids added by each side, and the native-links
/// proof (V6) does not hold: a git or path package among them, no matching
/// index entry, or a `links` value two packages share.
pub const R15: ReasonCode = code("R15");
/// An input is not in the exact form Cargo writes (the round-trip rule).
pub const CANON: ReasonCode = code("CANON");
/// The merged set cannot be written in Cargo's form with certainty (V5).
pub const UNWRITABLE: ReasonCode = code("UNWRITABLE");
/// A branch moved a crate to a lower version (rule D).
pub const DOWNGRADE: ReasonCode = code("DOWNGRADE");
/// The merged manifests may request features the taken entry was not resolved
/// with, or the entry lacks a dependency another version has (rules C and F).
pub const FEATURES: ReasonCode = code("FEATURES");
/// A path package's merged manifest does not admit the version taken (rule B1).
pub const REQUIREMENT: ReasonCode = code("REQUIREMENT");

pub const E1: &str = "cargo.E1";
pub const M1: &str = "cargo.M1";
pub const M2: &str = "cargo.M2";
pub const M3: &str = "cargo.M3";
pub const M4: &str = "cargo.M4";
pub const B1: &str = "cargo.B1";
pub const C: &str = "cargo.C";
pub const D: &str = "cargo.D";
pub const F: &str = "cargo.F";
pub const V2: &str = "cargo.V2";
pub const V3: &str = "cargo.V3";
pub const V4: &str = "cargo.V4";
pub const V5: &str = "cargo.V5";
pub const V6: &str = "cargo.V6";

pub static RULES: &[RuleDef] = &[
    RuleDef {
        id: E1,
        summary: "All three inputs are version 3 or 4 lockfiles of the same version, in Cargo's exact form, with no conflicting checksum, source, version change or id order.",
        notice: None,
        adr: "0008#1",
    },
    RuleDef {
        id: M1,
        summary: "Both branches bumped a registry crate to different versions of one compatibility class: keep the higher one, with every dependent and the requirements and features proven.",
        notice: None,
        adr: "0008#4",
    },
    RuleDef {
        id: M2,
        summary: "Both branches changed a path package's dependencies: combine them when the merged manifest declares exactly the merged set and every version satisfies its requirement.",
        notice: None,
        adr: "0008#5",
    },
    RuleDef {
        id: M3,
        summary: "Drop a registry or git package that one branch removed and the other changed, or that the removals left unused, when nothing points at it.",
        notice: None,
        adr: "0008#6",
    },
    RuleDef {
        id: M4,
        summary: "A Cargo.toml text conflict that is two insertions of whole entries at one place in one dependency table is read as both insertions, ours first.",
        notice: Some(
            "Cargo.toml has a merge conflict; the merged Cargo.lock assumes both added dependencies are kept.",
        ),
        adr: "0019",
    },
    RuleDef {
        id: B1,
        summary: "A one-sided version change is taken when every path package that depends on the crate still admits the new version in the merged manifest.",
        notice: None,
        adr: "0008#4",
    },
    RuleDef {
        id: C,
        summary: "A one-sided change to a package's dependencies is taken when the merged manifests select the features the changing branch was resolved with.",
        notice: None,
        adr: "0008#4",
    },
    RuleDef {
        id: D,
        summary: "A one-sided version change is not a downgrade.",
        notice: None,
        adr: "0008#4",
    },
    RuleDef {
        id: F,
        summary: "The new entry of a moved crate keeps every dependency name the old one had.",
        notice: None,
        adr: "0008#4",
    },
    RuleDef {
        id: V2,
        summary: "Every dependency edge of the merged set resolves to exactly one package.",
        notice: None,
        adr: "0008#8",
    },
    RuleDef {
        id: V3,
        summary: "Every non-path package of the merged set is reachable from a path package.",
        notice: None,
        adr: "0008#8",
    },
    RuleDef {
        id: V4,
        summary: "No two registry packages of the merged set share a name, a source and a semver compatibility class.",
        notice: None,
        adr: "0008#8",
    },
    RuleDef {
        id: V5,
        summary: "The merged set is written in Cargo's canonical form, and every edge resolves back to its target.",
        notice: None,
        adr: "0008#8",
    },
    RuleDef {
        id: V6,
        summary: "When each branch added different registry crates, every registry crate of the merged set has a matching registry index cache entry and no two packages share a `links` value.",
        notice: None,
        adr: "0008#15",
    },
];

const fn def(
    c: ReasonCode,
    title: &'static str,
    explain: &'static str,
    next_step: &'static str,
) -> CodeDef {
    CodeDef {
        code: c,
        title,
        explain,
        next_step,
        fatal: true,
    }
}

const BY_HAND: &str = "Resolve Cargo.lock by hand in the conflict markers.";

pub static CODES: &[CodeDef] = &[
    def(
        R1,
        "Not a supported Cargo.lock",
        "{subjects} is not a Cargo.lock of version 3 or 4, or the three versions differ.",
        "Merge by hand, or make all three branches use the same lockfile version first.",
    ),
    def(
        R2,
        "Checksums differ for one package",
        "{subjects} has a different checksum in two of the files.",
        BY_HAND,
    ),
    def(
        R3,
        "Both branches moved a package to different versions",
        "Both branches replaced {subjects} with different versions.",
        BY_HAND,
    ),
    def(
        R4,
        "The merged file would point at a missing package",
        "{subjects} depends on a package the merged file does not contain.",
        BY_HAND,
    ),
    def(
        R5,
        "A package was removed on one branch and changed on the other",
        "{subjects} was removed on one branch and changed on the other, and something still depends on it or it was not in use before.",
        BY_HAND,
    ),
    def(
        R6,
        "Both branches changed the dependencies of one package",
        "Both branches changed the dependencies of {subjects}, and its Cargo.toml cannot prove they combine.",
        BY_HAND,
    ),
    def(
        R7,
        "A package would be left unused",
        "{subjects} cannot be reached from any package of this project.",
        BY_HAND,
    ),
    def(
        R8,
        "Two compatible versions of one crate",
        "{subjects} would appear in two semver-compatible versions.",
        BY_HAND,
    ),
    def(
        R9,
        "A dependency is ambiguous",
        "{subjects} has a dependency that matches no package or several, or appears twice.",
        BY_HAND,
    ),
    def(
        R10,
        "A part of Cargo.lock is not handled",
        "{subjects} has a key or form that lockmerge does not read, such as `[metadata]` or `[[patch.unused]]`.",
        BY_HAND,
    ),
    def(
        R11,
        "A prerelease changed against a release",
        "{subjects} has a prerelease version on one side of a change, or a version that is not semver.",
        BY_HAND,
    ),
    def(
        R12,
        "A package changed its source",
        "{subjects} has the same name and version but a different source.",
        BY_HAND,
    ),
    def(
        R14,
        "A change needs the manifests, which cannot be used",
        "{subjects} changed on a branch in a way that is only safe if Cargo.toml says so, and the manifests are not available or not in a form lockmerge reads.",
        BY_HAND,
    ),
    def(
        R15,
        "Both branches added different crates",
        "{subjects} were added by one branch and different crates by the other, and lockmerge could not prove from Cargo's local registry index cache that no two of the crates link the same native library.",
        BY_HAND,
    ),
    def(
        CANON,
        "Cargo.lock is not in Cargo's exact form",
        "{subjects} differs from what Cargo itself would write, so lockmerge will not rewrite it.",
        BY_HAND,
    ),
    def(
        UNWRITABLE,
        "The merged file cannot be written with certainty",
        "{subjects} cannot be written in Cargo's form without guessing.",
        BY_HAND,
    ),
    def(
        DOWNGRADE,
        "A branch moved a crate to a lower version",
        "{subjects} was moved to a lower version, which may lack a feature the other branch enables.",
        BY_HAND,
    ),
    def(
        FEATURES,
        "The features a version was resolved with may not be the merged ones",
        "{subjects} changed on a branch, and the other branch changes which features or dependencies apply, so the taken entry may miss a dependency.",
        BY_HAND,
    ),
    def(
        REQUIREMENT,
        "A requirement in Cargo.toml does not admit the version",
        "{subjects} was moved to a version that a merged Cargo.toml no longer admits.",
        BY_HAND,
    ),
];
