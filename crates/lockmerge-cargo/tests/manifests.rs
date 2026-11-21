//! The manifest reader, the manifest finder, the feature-coverage comparison
//! and the symmetry of the manifest rules (ADR 0008 decisions 4 to 6 and 10,
//! `docs/formats/cargo-lock.md` section 4.5).

use lockmerge_cargo::find::{Side, covers, find, read_side};
use lockmerge_cargo::manifest::{Kind, Source, Unread, join, parse};
use lockmerge_cargo::model::Id;
use lockmerge_core::testkit::{Case, FakeManifests};
use lockmerge_core::{Context, Inputs, ManifestResult, Outcome, Role};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/rules/cargo")
}

fn path_id(name: &str, version: &str) -> Id {
    Id {
        name: name.into(),
        version: version.parse().unwrap(),
        source: None,
    }
}

fn unread(text: &str) -> Unread {
    parse(text, "", None).unwrap_err()
}

#[test]
fn reads_dependency_forms_renames_and_targets() {
    let m = parse(
        r#"
[package]
name = "app"
version = "0.1.0"
edition = "2021"

[features]
legacy = ["wrapper/legacy"]

[dependencies]
serde = "1"
rand = { version = "0.8", default-features = false, features = ["std"], optional = true }
old = { package = "new-name", version = "2" }
lib = { path = "lib", version = ">=1" }
g = { git = "https://example.test/g", branch = "main" }

[dev-dependencies]
tempfile = "3"

[target.'cfg(unix)'.dependencies]
libc = "0.2"
"#,
        "",
        None,
    )
    .unwrap();
    assert_eq!(m.name.as_deref(), Some("app"));
    assert_eq!(m.version.as_ref().unwrap().to_string(), "0.1.0");
    assert_eq!(m.deps.len(), 7);
    let rand = &m.deps[&(Kind::Normal, None, "rand".to_string())];
    assert!(!rand.default_features && rand.optional && !rand.is_plain());
    assert_eq!(m.decls_of("new-name").count(), 1);
    assert_eq!(
        m.deps[&(Kind::Normal, None, "lib".to_string())].source,
        Source::Path("lib".into())
    );
    assert!(matches!(
        m.deps[&(Kind::Normal, None, "g".to_string())].source,
        Source::Git(ref s) if s.ends_with("?branch=main")
    ));
    assert!(m.deps.contains_key(&(
        Kind::Normal,
        Some("cfg(unix)".to_string()),
        "libc".to_string()
    )));
    assert!(
        m.deps
            .contains_key(&(Kind::Dev, None, "tempfile".to_string()))
    );
    assert!(m.deps[&(Kind::Normal, None, "serde".to_string())].is_plain());
}

#[test]
fn inherits_from_the_workspace_and_refuses_extra_keys() {
    let root = parse(
        r#"
[workspace]
members = ["app"]

[workspace.package]
version = "2.3.4"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
lib = { path = "crates/lib", version = "1" }
"#,
        "",
        None,
    )
    .unwrap();
    let ws = root.workspace.as_ref().unwrap();
    assert_eq!(ws.dep_paths().unwrap(), vec!["crates/lib".to_string()]);
    let m = parse(
        r#"
[package]
name = "app"
version.workspace = true

[dependencies]
serde = { workspace = true, features = ["std"] }
lib = { workspace = true }
"#,
        "app",
        Some(ws),
    )
    .unwrap();
    assert_eq!(m.version.as_ref().unwrap().to_string(), "2.3.4");
    let serde = &m.deps[&(Kind::Normal, None, "serde".to_string())];
    assert_eq!(
        serde.features.iter().collect::<Vec<_>>(),
        vec!["derive", "std"]
    );
    // A path inherited from the workspace is relative to the workspace root.
    assert_eq!(
        m.deps[&(Kind::Normal, None, "lib".to_string())].source,
        Source::Path("crates/lib".into())
    );
    // Other keys beside `workspace = true`, and a crate the workspace lacks.
    for dep in [
        r#"serde = { workspace = true, default-features = false }"#,
        r#"anyhow = { workspace = true }"#,
    ] {
        let text = format!("[package]\nname = \"app\"\n[dependencies]\n{dep}\n");
        assert!(parse(&text, "app", Some(ws)).is_err(), "{dep}");
    }
}

#[test]
fn refuses_what_it_does_not_read() {
    let refused = [
        "[patch.crates-io]\nserde = { path = \"serde\" }\n",
        "[replace]\n",
        "[package]\nname = \"a\"\n[dev_dependencies]\nx = \"1\"\n",
        "[package]\nname = \"a\"\n[dependencies]\nx = { version = \"1\", registry = \"other\" }\n",
        "[package]\nname = \"a\"\n[dependencies]\nx = { version = \"1\", default_features = false }\n",
        "[package]\nname = \"a\"\n[dependencies]\nx = { version = \"1\", artifact = \"bin\" }\n",
        "[package]\nname = \"a\"\n[dependencies]\nx = { path = \"p\", git = \"g\" }\n",
        "[package]\nname = \"a\"\n[dependencies]\nx = { git = \"g\", branch = \"a\", tag = \"b\" }\n",
        "[package]\nname = \"a\"\n[dependencies]\nx = { path = \"../../outside\" }\n",
        "[package]\nname = \"a\"\n[dependencies]\nx = \"not a requirement\"\n",
        "[package]\nname = \"a\"\n[dependencies]\nx = { features = [\"a\"] }\n",
        "[package]\nname = \"a\"\nversion.workspace = true\n",
        "[package]\nname = \"a\"\n[target.'cfg(unix)'.other]\n",
        "this is not toml [",
    ];
    for text in refused {
        assert!(parse(text, "", None).is_err(), "{text}");
    }
    assert!(unread("[patch.x]\n").0.contains("patch"));
}

#[test]
fn joins_and_normalizes_repository_paths() {
    assert_eq!(join("a/b", "../c").unwrap(), "a/c");
    assert_eq!(join("", "./x/y").unwrap(), "x/y");
    assert_eq!(join("a", "..").unwrap(), "");
    for bad in ["../x", "/abs", "a\\b", "C:/x"] {
        assert!(join("", bad).is_err(), "{bad}");
    }
}

const ROOT_GLOB: &str =
    "[workspace]\nmembers = [\"crates/*\", \"tools/one\"]\nexclude = [\"crates/skipped\"]\n";

fn member(name: &str, deps: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[dependencies]\n{deps}")
}

fn ids(names: &[&str]) -> BTreeSet<Id> {
    names.iter().map(|n| path_id(n, "0.1.0")).collect()
}

fn merged(files: &[(&str, String)]) -> FakeManifests {
    files.iter().fold(FakeManifests::new(), |m, (p, t)| {
        m.with(p, ManifestResult::Merged(t.clone().into_bytes()))
    })
}

#[test]
fn finds_literal_members_globs_and_path_dependencies() {
    let m = merged(&[
        ("Cargo.toml", ROOT_GLOB.to_string()),
        (
            "crates/a/Cargo.toml",
            member("a", "d = { path = \"../../deps/d\" }\n"),
        ),
        ("crates/b/Cargo.toml", member("b", "")),
        ("tools/one/Cargo.toml", member("one", "")),
        ("deps/d/Cargo.toml", member("d", "")),
        ("crates/skipped/Cargo.toml", member("skipped", "")),
    ]);
    let cx = Context::new(&m);
    let found = find(&ids(&["a", "b", "one", "d"]), "Cargo.lock", &cx).unwrap();
    let paths: Vec<&str> = found.packages.values().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "crates/a/Cargo.toml",
            "crates/b/Cargo.toml",
            "deps/d/Cargo.toml",
            "tools/one/Cargo.toml"
        ]
    );
    // An excluded directory is not a member, even when a glob matches it.
    assert!(find(&ids(&["skipped"]), "Cargo.lock", &cx).is_err());
}

#[test]
fn finding_refuses_what_it_cannot_map_or_read() {
    let base = [
        ("Cargo.toml", ROOT_GLOB.to_string()),
        ("crates/a/Cargo.toml", member("a", "")),
        ("tools/one/Cargo.toml", member("one", "")),
    ];
    // A path package no manifest has.
    let m = merged(&base);
    assert!(
        find(
            &ids(&["a", "one", "ghost"]),
            "Cargo.lock",
            &Context::new(&m)
        )
        .is_err()
    );
    // A glob other than `<directory>/*`, a glob in `exclude`.
    for root in [
        "[workspace]\nmembers = [\"crates/**\"]\n",
        "[workspace]\nmembers = [\"c*/a\"]\n",
        "[workspace]\nmembers = [\"crates/a\"]\nexclude = [\"crates/*\"]\n",
    ] {
        let m = merged(&[("Cargo.toml", root.to_string()), base[1].clone()]);
        assert!(
            find(&ids(&["a"]), "Cargo.lock", &Context::new(&m)).is_err(),
            "{root}"
        );
    }
    // A manifest whose text merge conflicts, one that is missing, a patch.
    let m = FakeManifests::new()
        .with(
            "Cargo.toml",
            ManifestResult::Merged(ROOT_GLOB.as_bytes().to_vec()),
        )
        .with("crates/a/Cargo.toml", ManifestResult::Conflict);
    assert!(find(&ids(&["a"]), "Cargo.lock", &Context::new(&m)).is_err());
    let m = merged(&[(
        "Cargo.toml",
        "[workspace]\nmembers = [\"x\"]\n[patch.crates-io]\n".to_string(),
    )]);
    assert!(find(&ids(&["x"]), "Cargo.lock", &Context::new(&m)).is_err());
    // Two manifests that are the same package.
    let m = merged(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"a\", \"b\"]\n".to_string(),
        ),
        ("a/Cargo.toml", member("same", "")),
        ("b/Cargo.toml", member("same", "")),
    ]);
    assert!(find(&ids(&["same"]), "Cargo.lock", &Context::new(&m)).is_err());
}

#[test]
fn a_lockfile_in_a_subdirectory_reads_that_directory_s_manifest() {
    let m = merged(&[
        (
            "sub/Cargo.toml",
            "[workspace]\nmembers = [\"m\"]\n".to_string(),
        ),
        ("sub/m/Cargo.toml", member("m", "")),
    ]);
    let found = find(&ids(&["m"]), "sub/Cargo.lock", &Context::new(&m)).unwrap();
    assert_eq!(found.root_dir, "sub");
    assert_eq!(
        found.packages.values().next().unwrap().0,
        "sub/m/Cargo.toml"
    );
}

fn side_of(files: &[(&str, &str)]) -> FakeManifests {
    files.iter().fold(FakeManifests::new(), |m, (p, t)| {
        m.with_side(Role::Ours, p, t.as_bytes())
            .with_side(Role::Base, p, t.as_bytes())
    })
}

/// Coverage of the merged manifests against ours, with one member `m` whose
/// merged manifest is `merged_m` and whose own (and base's) is `own_m`.
fn coverage(merged_m: &str, own_m: &str, ours_names: &[&str]) -> Result<(), String> {
    let root = "[workspace]\nmembers = [\"m\"]\n";
    let mm = merged(&[
        ("Cargo.toml", root.to_string()),
        ("m/Cargo.toml", merged_m.to_string()),
    ]);
    let cx = Context::new(&mm);
    let found = find(&ids(&["m"]), "Cargo.lock", &cx).unwrap();
    let sides = side_of(&[("Cargo.toml", root), ("m/Cargo.toml", own_m)]);
    let cx = Context::new(&sides);
    let ours: Side = read_side(&found, Role::Ours, &cx).unwrap();
    let base: Side = read_side(&found, Role::Base, &cx).unwrap();
    let names: BTreeSet<&str> = ours_names.iter().copied().collect();
    covers(&found, &ours, &base, &names)
}

#[test]
fn coverage_accepts_equal_selection_and_ignores_requirements() {
    let own = member(
        "m",
        "rand = { version = \"0.8\", default-features = false }\n",
    );
    let merged_m = member(
        "m",
        "rand = { version = \"0.9\", default-features = false }\n",
    );
    assert_eq!(coverage(&merged_m, &own, &["rand"]), Ok(()));
}

#[test]
fn coverage_refuses_a_changed_feature_selection() {
    let own = member(
        "m",
        "rand = { version = \"0.8\", default-features = false }\n",
    );
    for merged_m in [
        member("m", "rand = { version = \"0.8\" }\n"),
        member(
            "m",
            "rand = { version = \"0.8\", default-features = false, features = [\"std\"] }\n",
        ),
        member(
            "m",
            "rand = { version = \"0.8\", default-features = false, optional = true }\n",
        ),
        format!("{}\n[features]\nx = []\n", own.trim_end()),
        own.replace(
            "version = \"0.1.0\"",
            "version = \"0.1.0\"\nedition = \"2021\"",
        ),
    ] {
        assert!(coverage(&merged_m, &own, &["rand"]).is_err(), "{merged_m}");
    }
}

#[test]
fn coverage_allows_only_the_other_branch_s_plain_new_dependency_of_an_unused_crate() {
    let own = member("m", "rand = \"0.8\"\n");
    let plain = member("m", "rand = \"0.8\"\nanyhow = \"1\"\n");
    // Plain, registry, and the side's lockfile has no anyhow.
    assert_eq!(coverage(&plain, &own, &["rand"]), Ok(()));
    // The side's lockfile already has it: features of it may unify.
    assert!(coverage(&plain, &own, &["rand", "anyhow"]).is_err());
    // It selects something, or is optional, or is not a registry dependency.
    for new in [
        "anyhow = { version = \"1\", features = [\"std\"] }",
        "anyhow = { version = \"1\", default-features = false }",
        "anyhow = { version = \"1\", optional = true }",
        "anyhow = { git = \"https://example.test/anyhow\" }",
    ] {
        let merged_m = member("m", &format!("rand = \"0.8\"\n{new}\n"));
        assert!(coverage(&merged_m, &own, &["rand"]).is_err(), "{new}");
    }
}

#[test]
fn coverage_allows_a_removed_dependency_only_if_base_had_it_unchanged() {
    let root = "[workspace]\nmembers = [\"m\"]\n";
    let with = member("m", "rand = \"0.8\"\nitoa = \"1\"\n");
    let without = member("m", "rand = \"0.8\"\n");
    let mm = merged(&[
        ("Cargo.toml", root.to_string()),
        ("m/Cargo.toml", without.clone()),
    ]);
    let found = find(&ids(&["m"]), "Cargo.lock", &Context::new(&mm)).unwrap();
    let side_with = |base_text: &str| {
        let f = FakeManifests::new()
            .with_side(Role::Ours, "Cargo.toml", root.as_bytes())
            .with_side(Role::Ours, "m/Cargo.toml", with.as_bytes())
            .with_side(Role::Base, "Cargo.toml", root.as_bytes())
            .with_side(Role::Base, "m/Cargo.toml", base_text.as_bytes());
        let cx = Context::new(&f);
        (
            read_side(&found, Role::Ours, &cx).unwrap(),
            read_side(&found, Role::Base, &cx).unwrap(),
        )
    };
    let names = BTreeSet::new();
    // The other branch removed it: base had it as the side does.
    let (ours, base) = side_with(&with);
    assert_eq!(covers(&found, &ours, &base, &names), Ok(()));
    // The side itself added it, and the merge lacks it.
    let (ours, base) = side_with(&without);
    assert!(covers(&found, &ours, &base, &names).is_err());
    // Base had a different declaration of it.
    let (ours, base) = side_with(&member(
        "m",
        "rand = \"0.8\"\nitoa = { version = \"1\", features = [\"x\"] }\n",
    ));
    assert!(covers(&found, &ours, &base, &names).is_err());
}

fn load(rule: &str, name: &str) -> Case {
    Case::load(&fixtures().join(rule).join(name)).unwrap()
}

/// Merges a case with its two branches exchanged and returns the report.
fn merge_swapped(case: &Case) -> lockmerge_core::Report {
    let case = case.swapped();
    lockmerge_cargo::format()
        .merge(
            &Inputs {
                base: &case.base,
                ours: &case.ours,
                theirs: &case.theirs,
                path: &case.path,
            },
            &Context::new(&case.manifests),
        )
        .unwrap()
}

/// The same bytes, and the rule that made the case accepted among those applied.
fn assert_same_merge(rule: &str, name: &str) {
    let case = load(rule, name);
    let report = merge_swapped(&case);
    let Outcome::Clean(clean) = &report.outcome else {
        panic!("{rule}/{name} with the sides swapped was refused: {report:?}");
    };
    assert_eq!(
        Some(clean.bytes()),
        case.expected.as_deref(),
        "{rule}/{name}"
    );
    let id = format!("cargo.{rule}");
    assert!(
        report.applied.iter().any(|a| a.rule == id),
        "{rule}/{name}: {id} was not applied"
    );
}

#[test]
fn m1_keeps_the_higher_version_whichever_side_has_it() {
    assert_same_merge("M1", "accept-both-bumped-higher-kept");
}

#[test]
fn the_one_sided_rules_apply_to_either_side() {
    // The accept cases bump on ours (B1, F) or on theirs (D, C); exchanging
    // the branches must give the same bytes.
    for (rule, name) in [
        ("D", "accept-upgrade-from-theirs"),
        ("B1", "accept-bump-while-other-member-drops-a-crate"),
        ("F", "accept-bump-keeps-dependencies"),
        ("C", "accept-cross-class-upgrade"),
        ("M3", "accept-removed-and-changed-package-unused"),
        ("M2", "accept-both-add-an-edge-to-the-same-member"),
    ] {
        assert_same_merge(rule, name);
    }
}

#[test]
fn the_refusals_hold_with_the_sides_swapped() {
    for (rule, name, code) in [
        ("B1", "refuse-manifest-only-pin", "cargo.REQUIREMENT"),
        ("D", "refuse-downgrade", "cargo.DOWNGRADE"),
        ("C", "refuse-workspace-membership-changed", "cargo.FEATURES"),
        ("C", "refuse-direct-forwarding", "cargo.FEATURES"),
        ("C", "refuse-bridge-forwarding", "cargo.FEATURES"),
        (
            "C",
            "refuse-feature-enabled-on-other-branch",
            "cargo.FEATURES",
        ),
        ("F", "refuse-bump-drops-dependency", "cargo.FEATURES"),
        (
            "M1",
            "refuse-feature-enabled-on-lower-side-without-edge",
            "cargo.R3",
        ),
        ("M3", "refuse-another-package-still-depends", "cargo.R5"),
    ] {
        let case = load(rule, name);
        let report = merge_swapped(&case);
        let Outcome::Refused(r) = &report.outcome else {
            panic!(
                "{rule}/{name} with the sides swapped was merged: {:?}",
                report.applied
            );
        };
        assert_eq!(r.code.to_string(), code, "{rule}/{name}");
    }
}

/// Both orders of a case refuse with R6 and a detail that names the reason.
#[test]
fn m2_refuses_cycles_and_unproven_feature_combinations_for_the_stated_reason() {
    for (name, why) in [
        ("refuse-dependency-cycle", "dependency cycle"),
        ("refuse-forwarded-features-combined", "not proven to select"),
        ("refuse-git-requirement-not-met", "does not satisfy"),
    ] {
        let case = load("M2", name);
        for case in [case.swapped(), case] {
            let report = lockmerge_cargo::format()
                .merge(
                    &Inputs {
                        base: &case.base,
                        ours: &case.ours,
                        theirs: &case.theirs,
                        path: &case.path,
                    },
                    &Context::new(&case.manifests),
                )
                .unwrap();
            let Outcome::Refused(r) = &report.outcome else {
                panic!("{name} was merged");
            };
            assert_eq!(r.code.to_string(), "cargo.R6", "{name}");
            let detail: Vec<&str> = r.facts.iter().map(|(_, v)| v.as_str()).collect();
            assert!(detail.iter().any(|d| d.contains(why)), "{name}: {detail:?}");
        }
    }
}
