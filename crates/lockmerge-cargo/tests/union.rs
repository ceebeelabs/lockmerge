//! ADR 0019: a manifest text conflict that is two insertions. The cases are
//! the `accept-*` and `refuse-*` directories of `tests/fixtures/rules/cargo/M4`
//! (run once by the lint in `rules.rs`); here each runs again with the two
//! branches exchanged, which must give the same bytes or the same refusal.

use lockmerge_core::testkit::Case;
use lockmerge_core::{Context, Inputs, Outcome};
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/rules/cargo/M4")
}

fn merge(case: &Case) -> lockmerge_core::Report {
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

#[test]
fn every_case_gives_the_same_result_with_the_sides_swapped() {
    let mut accepts = 0;
    let mut refuses = 0;
    let names = [
        "accept-both-insert-into-build-dependencies",
        "accept-both-insert-into-dependencies",
        "accept-both-insert-into-dev-dependencies",
        "accept-both-insert-into-target-dependencies",
        "refuse-both-insert-the-same-key",
        "refuse-edit-and-insertion",
        "refuse-insertion-into-another-table",
        "refuse-key-already-in-the-table",
        "refuse-table-style-section",
        "refuse-two-hunks-one-is-an-edit",
        "refuse-value-over-several-lines",
    ];
    for name in names {
        let path = dir().join(name);
        let case = Case::load(&path).unwrap();
        let report = merge(&case.swapped());
        match (&report.outcome, name.starts_with("accept-")) {
            (Outcome::Clean(clean), true) => {
                accepts += 1;
                assert_eq!(Some(clean.bytes()), case.expected.as_deref(), "{name}");
                assert!(
                    report.applied.iter().any(|a| a.rule == "cargo.M4"),
                    "{name}"
                );
            }
            (Outcome::Refused(r), false) => {
                refuses += 1;
                assert_eq!(r.code.to_string(), "cargo.R6", "{name}");
            }
            (other, _) => panic!("{name} with the sides swapped: {other:?}"),
        }
    }
    assert_eq!((accepts, refuses), (4, 7));
}
