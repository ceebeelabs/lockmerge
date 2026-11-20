//! The fixture contract of the toy format (ADR 0007 layout under
//! `tests/fixtures/rules/toy/`), run by the testkit. This is what a format
//! crate's own `tests/rules.rs` looks like.

mod common;

use common::Toy;
use lockmerge_core::testkit;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/rules")
}

#[test]
fn every_rule_and_code_has_its_fixtures_and_every_case_matches() {
    let problems = testkit::lint(&Toy::default(), &fixtures());
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
