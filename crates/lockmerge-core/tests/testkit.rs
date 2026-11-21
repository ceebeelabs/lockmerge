//! Tests of the testkit itself: the lint must fail on every gap it promises to
//! find, and the runner must fail on every mismatch. Each test builds a small
//! fixture tree in the target directory, copying cases from the toy fixtures.
//!
//! The tests write files, so they lift the purity lints of `clippy.toml`, as
//! the testkit does.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

mod common;

use common::Toy;
use lockmerge_core::testkit::{self, Case, Expect, FakeManifests};
use lockmerge_core::{ManifestResult, Manifests};
use std::fs;
use std::path::{Path, PathBuf};

fn toy_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/rules/toy")
}

/// A fresh copy of the toy fixtures under `<target tmp>/<name>/toy`.
fn copy_of_toy(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&root);
    copy_dir(&toy_fixtures(), &root.join("toy"));
    root
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            fs::copy(e.path(), target).unwrap();
        }
    }
}

fn lint(root: &Path) -> Vec<String> {
    testkit::lint(&Toy::default(), root)
}

fn has(problems: &[String], needle: &str) -> bool {
    problems.iter().any(|p| p.contains(needle))
}

#[test]
fn the_copy_of_the_toy_fixtures_passes() {
    // The control for every test below: they differ from this by one change.
    let root = copy_of_toy("kit-control");
    assert_eq!(lint(&root), Vec::<String>::new());
}

#[test]
fn lint_refuses_a_rule_without_an_accept_case() {
    let root = copy_of_toy("kit-no-accept");
    fs::remove_dir_all(root.join("toy/M1/accept-both-raised")).unwrap();
    let p = lint(&root);
    assert!(has(&p, "rule toy.M1: no accept-* case"), "{p:?}");
    assert!(!has(&p, "no refuse-* case"), "{p:?}");
}

#[test]
fn lint_refuses_a_rule_without_a_refuse_case() {
    let root = copy_of_toy("kit-no-refuse");
    for d in [
        "refuse-added-on-both-without-base",
        "refuse-bad-value",
        "refuse-non-fatal-downgrade",
    ] {
        fs::remove_dir_all(root.join("toy/M1").join(d)).unwrap();
    }
    let p = lint(&root);
    assert!(has(&p, "rule toy.M1: no refuse-* case"), "{p:?}");
    assert!(!has(&p, "no accept-* case"), "{p:?}");
}

#[test]
fn lint_refuses_a_code_nobody_refuses_with() {
    let root = copy_of_toy("kit-no-code");
    fs::remove_dir_all(root.join("toy/M1/refuse-bad-value")).unwrap();
    let p = lint(&root);
    // M1 still has refuse cases; only the code R3 lost its case.
    assert_eq!(p, ["code toy.R3: no refusal case"]);
}

#[test]
fn lint_covers_a_non_fatal_code_only_through_downgrades() {
    let root = copy_of_toy("kit-downgrade");
    let case = root.join("toy/M1/refuse-non-fatal-downgrade/case.toml");
    let text = fs::read_to_string(&case).unwrap();
    fs::write(&case, text.replace("downgrades = [\"toy.R14\"]\n", "")).unwrap();
    let p = lint(&root);
    // The run now disagrees with the case, and R14 is no longer covered.
    assert!(
        has(&p, "downgrades are [\"toy.R14\"], the case says []"),
        "{p:?}"
    );
    assert!(has(&p, "code toy.R14: no refusal case"), "{p:?}");
}

#[test]
fn lint_refuses_an_accept_case_that_does_not_name_its_rule() {
    // A case that merges through other rules must not count for M1.
    let root = copy_of_toy("kit-wrong-rule");
    let m1 = root.join("toy/M1/accept-both-raised");
    fs::remove_dir_all(&m1).unwrap();
    copy_dir(&root.join("toy/V1/accept-no-empty-value"), &m1);
    let p = lint(&root);
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(p[0].contains("`applied` does not name toy.M1"), "{p:?}");
}

#[test]
fn lint_refuses_a_refuse_case_that_merges() {
    let root = copy_of_toy("kit-refuse-merges");
    let d = root.join("toy/M1/refuse-bad-value");
    fs::remove_dir_all(&d).unwrap();
    copy_dir(&root.join("toy/M1/accept-both-raised"), &d);
    let p = lint(&root);
    assert!(
        has(
            &p,
            "an accept case must merge and a refuse case must refuse"
        ),
        "{p:?}"
    );
}

#[test]
fn lint_refuses_a_stray_directory_and_a_misnamed_case() {
    let root = copy_of_toy("kit-stray");
    fs::create_dir_all(root.join("toy/M9")).unwrap();
    fs::create_dir_all(root.join("toy/M1/acept-typo")).unwrap();
    let p = lint(&root);
    assert!(
        has(&p, "toy/M9: no rule of format toy has this name"),
        "{p:?}"
    );
    assert!(
        has(&p, "acept-typo: not an accept-* or refuse-* directory"),
        "{p:?}"
    );
}

#[test]
fn run_case_fails_on_different_bytes() {
    let root = copy_of_toy("kit-bytes");
    let d = root.join("toy/M1/accept-both-raised");
    fs::write(d.join("expected"), b"toy 1\na=5\n").unwrap();
    let e = testkit::run_dir(&Toy::default(), &d).unwrap_err();
    assert!(e.contains("merged bytes differ"), "{e}");
}

#[test]
fn run_case_fails_on_a_different_code() {
    let root = copy_of_toy("kit-code");
    let d = root.join("toy/M1/refuse-bad-value");
    let case = d.join("case.toml");
    let text = fs::read_to_string(&case).unwrap();
    fs::write(&case, text.replace("toy.R3", "toy.R15")).unwrap();
    let e = testkit::run_dir(&Toy::default(), &d).unwrap_err();
    assert!(
        e.contains("refused with toy.R3, the case says toy.R15"),
        "{e}"
    );
}

#[test]
fn case_toml_refuses_what_it_cannot_read_exactly() {
    let root = copy_of_toy("kit-toml");
    let d = root.join("toy/M1/accept-both-raised");
    let case = d.join("case.toml");
    let good = fs::read_to_string(&case).unwrap();
    for (bad, why) in [
        (
            format!("{good}surprise = \"x\"\n"),
            "unexpected key `surprise`",
        ),
        (format!("{good}path = \"again\"\n"), "duplicate key"),
        (good.replace("\"hand-built\"", "\"a\\nb\""), "quoted string"),
        (
            good.replace("[\"toy.E1\",", "[\"toy.E1\",\n"),
            "array must end on its line",
        ),
        (
            good.replace("outcome = \"merged\"", "outcome = \"maybe\""),
            "merged or refused",
        ),
        (good.replace("path = \"toy.lock\"\n", ""), "missing `path`"),
        (
            good.replace("source = \"hand-built\"\n", ""),
            "missing `source`",
        ),
        (format!("[table]\n{good}"), "expected key = value"),
    ] {
        fs::write(&case, &bad).unwrap();
        let e = Case::load(&d).unwrap_err();
        assert!(e.contains(why), "{why}: got {e}");
    }
    fs::write(&case, &good).unwrap();
    assert!(matches!(
        Case::load(&d).unwrap().expect,
        Expect::Merged { .. }
    ));
}

/// A manifests directory built from `(relative path, bytes)` entries.
fn manifests_dir(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    for (rel, bytes) in files {
        let f = root.join(rel);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(f, bytes).unwrap();
    }
    root
}

#[test]
fn fake_manifests_read_merged_conflict_and_unavailable() {
    let root = manifests_dir(
        "kit-manifests",
        &[
            ("merged/Cargo.toml", b"[package]\n"),
            ("merged/sub/Cargo.toml", b"[member]\n"),
            ("merged/sub/Other.toml.conflict", b""),
        ],
    );
    let m = FakeManifests::from_dir(&root).unwrap();
    assert_eq!(
        m.merged("Cargo.toml"),
        ManifestResult::Merged(b"[package]\n".to_vec())
    );
    assert_eq!(
        m.merged("sub/Cargo.toml"),
        ManifestResult::Merged(b"[member]\n".to_vec())
    );
    assert_eq!(m.merged("sub/Other.toml"), ManifestResult::Conflict);
    assert!(matches!(
        m.merged("elsewhere/Cargo.toml"),
        ManifestResult::Unavailable(_)
    ));
    // A missing directory is an empty fake, and the builder overrides by path.
    let none = FakeManifests::from_dir(&root.join("absent")).unwrap();
    assert!(matches!(
        none.merged("Cargo.toml"),
        ManifestResult::Unavailable(_)
    ));
    let one = FakeManifests::new().with("x", ManifestResult::Conflict);
    assert_eq!(one.merged("x"), ManifestResult::Conflict);
}

#[test]
fn fake_manifests_do_not_read_files_outside_merged() {
    // The old layout (manifests/<path>) is no longer read: a file beside
    // `merged/` is an error, never silently ignored or taken as a manifest.
    let root = manifests_dir("kit-man-old", &[("Cargo.toml", b"[package]\n")]);
    let e = FakeManifests::from_dir(&root).unwrap_err();
    assert!(e.contains("Cargo.toml") && e.contains("merged"), "{e}");
}

#[test]
fn fake_manifests_reject_the_per_side_benchmark_layout() {
    for side in ["base", "ours", "theirs"] {
        let rel = format!("{side}/Cargo.toml");
        let root = manifests_dir(
            &format!("kit-man-side-{side}"),
            &[("merged/Cargo.toml", b"x"), (&rel, b"y")],
        );
        let e = FakeManifests::from_dir(&root).unwrap_err();
        assert!(
            e.contains("per-side") && e.contains(side) && e.contains("manifests/merged"),
            "{side}: {e}"
        );
    }
}

#[test]
fn fake_manifests_reject_a_conflict_marker_with_content_or_a_twin() {
    let root = manifests_dir("kit-man-full", &[("merged/a.toml.conflict", b"oops")]);
    let e = FakeManifests::from_dir(&root).unwrap_err();
    assert!(e.contains("must be empty"), "{e}");
    let root = manifests_dir(
        "kit-man-twin",
        &[("merged/a.toml", b"x"), ("merged/a.toml.conflict", b"")],
    );
    let e = FakeManifests::from_dir(&root).unwrap_err();
    assert!(e.contains("both"), "{e}");
}

#[test]
fn a_case_with_a_per_side_manifests_directory_does_not_load() {
    let root = copy_of_toy("kit-case-side");
    let d = root.join("toy/M1/accept-both-raised");
    fs::create_dir_all(d.join("manifests/ours")).unwrap();
    fs::write(d.join("manifests/ours/Cargo.toml"), b"x").unwrap();
    let e = Case::load(&d).unwrap_err();
    assert!(e.contains("per-side"), "{e}");
}

#[test]
fn a_case_reads_its_manifests_from_merged() {
    let root = copy_of_toy("kit-case-merged");
    let d = root.join("toy/M1/accept-both-raised");
    fs::create_dir_all(d.join("manifests/merged")).unwrap();
    fs::write(d.join("manifests/merged/Cargo.toml"), b"m").unwrap();
    let c = Case::load(&d).unwrap();
    assert_eq!(
        c.manifests.merged("Cargo.toml"),
        ManifestResult::Merged(b"m".to_vec())
    );
}

// ---- also_refused ---------------------------------------------------------

/// The toy fixture whose run refuses several keys: three keys conflict, so
/// the primary refusal is the first and the rest are `also_refused`.
fn also_refused_case(name: &str, extra: &str) -> PathBuf {
    let root = copy_of_toy(name);
    let d = root.join("toy/M1/refuse-bad-value");
    let case = d.join("case.toml");
    let text = fs::read_to_string(&case).unwrap();
    fs::write(&case, format!("{text}{extra}")).unwrap();
    d
}

#[test]
fn run_case_checks_also_refused() {
    // Control: the toy refusal has no further fatal refusals.
    let d = also_refused_case("kit-also-empty", "also_refused = []\n");
    testkit::run_dir(&Toy::default(), &d).unwrap();
    // Declaring one that does not happen fails.
    let d = also_refused_case("kit-also-extra", "also_refused = [\"toy.R3\"]\n");
    let e = testkit::run_dir(&Toy::default(), &d).unwrap_err();
    assert!(
        e.contains("also_refused are [], the case says [\"toy.R3\"]"),
        "{e}"
    );
}

#[test]
fn run_case_fails_on_an_unlisted_also_refused() {
    // Two unparsable inputs: ours is primary, theirs is also refused.
    let root = copy_of_toy("kit-also-unlisted");
    let d = root.join("toy/E1/refuse-unparsable-theirs");
    fs::write(d.join("ours"), b"\xff").unwrap();
    let e = testkit::run_dir(&Toy::default(), &d).unwrap_err();
    assert!(e.contains("also_refused are [\""), "{e}");
}
