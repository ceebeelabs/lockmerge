//! The fixture contract of the Cargo.lock format (ADR 0007 layout under
//! `tests/fixtures/rules/cargo/`), run by the testkit lint: every rule and
//! every refusal code has an accept or a refuse fixture, and every case
//! matches exactly.

use lockmerge_core::testkit;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/rules")
}

#[test]
fn every_rule_and_code_has_its_fixtures_and_every_case_matches() {
    let problems = testkit::lint(lockmerge_cargo::format(), &fixtures());
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
