//! Real lockfiles through the parser and the canonical writer, and through the
//! engine (`tests/fixtures/roundtrip/cargo/`, provenance in its README).
#![allow(clippy::disallowed_methods)]

use lockmerge_cargo::model::{Id, Package};
use lockmerge_cargo::parse::parse;
use lockmerge_cargo::write::render;
use lockmerge_core::testkit::FakeManifests;
use lockmerge_core::{Context, Inputs, Outcome, Role};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/roundtrip/cargo")
}

fn real_files() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "lock") {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            out.push((name, std::fs::read(&path).unwrap()));
        }
    }
    out.sort();
    out
}

#[test]
fn the_corpus_has_both_lockfile_versions() {
    let versions: BTreeSet<u8> = real_files()
        .iter()
        .map(|(_, b)| parse(Role::Base, b).unwrap().version)
        .collect();
    assert_eq!(versions, BTreeSet::from([3, 4]));
    assert!(real_files().len() >= 9);
}

#[test]
fn every_real_lockfile_parses_and_writes_the_same_bytes() {
    for (name, bytes) in real_files() {
        let lock = parse(Role::Base, &bytes).unwrap_or_else(|r| panic!("{name}: {r:?}"));
        let items: Vec<(&Id, &Package)> = lock.packages.iter().collect();
        let written = render(lock.version, &items).expect("writable");
        assert!(
            written == bytes,
            "{name}: parse then write changed the bytes"
        );
    }
}

/// The engine on real shapes: ours and theirs each add a different workspace
/// member (path packages are never the subject of R15). The result is the
/// canonical text of base plus both members.
#[test]
fn real_lockfiles_merge_two_added_members() {
    for (name, bytes) in real_files() {
        let base = parse(Role::Base, &bytes).unwrap();
        let with = |member: &str| {
            let mut lock = base.clone();
            let id = Id {
                name: member.to_string(),
                version: "0.1.0".parse().unwrap(),
                source: None,
            };
            // The member depends on the first package of the file, if any.
            let first = lock.packages.keys().next().unwrap().clone();
            let edges = BTreeSet::from([first]);
            lock.packages.insert(
                id,
                Package {
                    checksum: None,
                    edges,
                },
            );
            lock
        };
        let write = |lock: &lockmerge_cargo::Lock| {
            let items: Vec<_> = lock.packages.iter().collect();
            render(lock.version, &items).unwrap()
        };
        let (ours, theirs) = (with("zz-ours"), with("zz-theirs"));
        let mut both = ours.clone();
        both.packages.extend(theirs.packages.clone());
        let (o, t) = (write(&ours), write(&theirs));
        let manifests = FakeManifests::new();
        let report = lockmerge_cargo::format()
            .merge(
                &Inputs {
                    base: &bytes,
                    ours: &o,
                    theirs: &t,
                    path: "Cargo.lock",
                },
                &Context::new(&manifests),
            )
            .unwrap();
        match report.outcome {
            Outcome::Clean(clean) => assert!(
                clean.bytes() == write(&both).as_slice(),
                "{name}: merged bytes differ"
            ),
            Outcome::Refused(r) => panic!("{name}: refused: {r:?}"),
        }
    }
}
