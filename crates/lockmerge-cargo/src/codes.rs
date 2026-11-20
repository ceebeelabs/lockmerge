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
/// A package was removed on one side and changed on the other.
pub const R5: ReasonCode = code("R5");
/// Both sides changed a path package.
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
/// A version change needs the manifest rules, which this crate does not have.
pub const R14: ReasonCode = code("R14");
/// Different registry or git ids added by each side need the native-links
/// proof, which this crate does not have.
pub const R15: ReasonCode = code("R15");
/// An input is not in the exact form Cargo writes (the round-trip rule).
pub const CANON: ReasonCode = code("CANON");
/// The merged set cannot be written in Cargo's form with certainty (V5).
pub const UNWRITABLE: ReasonCode = code("UNWRITABLE");

pub const E1: &str = "cargo.E1";
pub const V2: &str = "cargo.V2";
pub const V3: &str = "cargo.V3";
pub const V4: &str = "cargo.V4";
pub const V5: &str = "cargo.V5";

pub static RULES: &[RuleDef] = &[
    RuleDef {
        id: E1,
        summary: "All three inputs are version 3 or 4 lockfiles of the same version, in Cargo's exact form, with no conflicting checksum, source, version change or id order.",
        adr: "0008#1",
    },
    RuleDef {
        id: V2,
        summary: "Every dependency edge of the merged set resolves to exactly one package.",
        adr: "0008#8",
    },
    RuleDef {
        id: V3,
        summary: "Every non-path package of the merged set is reachable from a path package.",
        adr: "0008#8",
    },
    RuleDef {
        id: V4,
        summary: "No two registry packages of the merged set share a name, a source and a semver compatibility class.",
        adr: "0008#8",
    },
    RuleDef {
        id: V5,
        summary: "The merged set is written in Cargo's canonical form, and every edge resolves back to its target.",
        adr: "0008#8",
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
        "One branch removed a package the other changed",
        "{subjects} was removed on one branch and changed on the other.",
        BY_HAND,
    ),
    def(
        R6,
        "Both branches changed a package of this project",
        "{subjects} is a path package that both branches changed.",
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
        "A version change needs the manifests",
        "{subjects} changed version on a branch; lockmerge cannot yet check Cargo.toml to prove that safe.",
        BY_HAND,
    ),
    def(
        R15,
        "Both branches added different crates",
        "{subjects} were added by one branch and different crates by the other; two crates may link the same native library, which lockmerge cannot yet rule out.",
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
];
