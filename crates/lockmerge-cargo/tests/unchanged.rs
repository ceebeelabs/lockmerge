//! Unchanged inputs come back byte for byte, parsed or not (ADR 0008
//! decision 11, `core.S1`), and the format reports what it is.

use lockmerge_core::testkit::FakeManifests;
use lockmerge_core::{Context, How, Inputs, Outcome, Side};

fn merge(base: &[u8], ours: &[u8], theirs: &[u8]) -> lockmerge_core::Report {
    let manifests = FakeManifests::new();
    lockmerge_cargo::format()
        .merge(
            &Inputs {
                base,
                ours,
                theirs,
                path: "Cargo.lock",
            },
            &Context::new(&manifests),
        )
        .unwrap()
}

// Not canonical Cargo text on purpose: no parser or writer may touch it.
const ODD: &[u8] = b"# edited by hand\r\nversion = 3\r\n\r\n\r\n[[package]]\r\nname = 'x'\r\n";
const OTHER: &[u8] = b"not a lockfile at all";

#[test]
fn theirs_equal_to_base_returns_ours_unchanged() {
    let r = merge(OTHER, ODD, OTHER);
    let Outcome::Clean(c) = r.outcome else {
        panic!("refused")
    };
    assert_eq!(c.bytes(), ODD);
    assert_eq!(c.how(), How::Unchanged(Side::Ours));
}

#[test]
fn ours_equal_to_base_returns_theirs_unchanged() {
    let r = merge(OTHER, OTHER, ODD);
    let Outcome::Clean(c) = r.outcome else {
        panic!("refused")
    };
    assert_eq!(c.bytes(), ODD);
    assert_eq!(c.how(), How::Unchanged(Side::Theirs));
}

#[test]
fn identical_sides_return_ours_unchanged() {
    let r = merge(OTHER, ODD, ODD);
    let Outcome::Clean(c) = r.outcome else {
        panic!("refused")
    };
    assert_eq!(c.bytes(), ODD);
}

#[test]
fn three_different_unparsable_inputs_are_refused_not_returned() {
    let r = merge(b"a", b"b", b"c");
    assert!(matches!(r.outcome, Outcome::Refused(ref x) if x.code.to_string() == "cargo.R1"));
}

#[test]
fn the_format_matches_cargo_lock_only() {
    let f = lockmerge_cargo::format();
    assert_eq!(f.id().to_string(), "cargo");
    assert!(f.matches("Cargo.lock"));
    assert!(!f.matches("Cargo.toml"));
    assert!(!f.matches("cargo.lock"));
}
