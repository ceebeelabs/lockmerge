//! The command line, on the fixtures committed in this repository.

use lockmerge_bench::cli::main;

fn fixtures() -> String {
    format!("{}/../../tests/fixtures", env!("CARGO_MANIFEST_DIR"))
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_committed_corpus_passes_the_per_pr_gate() {
    // The `bench` CI job runs this same command. It passes on the empty corpus
    // and needs no `lockmerge` binary, because no case is replayed.
    assert_eq!(main(args(&["check", "--fixtures", &fixtures()])), 0);
}

#[test]
fn the_v1_release_check_fails_until_the_targets_are_met() {
    assert_eq!(
        main(args(&[
            "check",
            "--fixtures",
            &fixtures(),
            "--release",
            "v1"
        ])),
        1
    );
}

#[test]
fn bad_usage_is_exit_2() {
    assert_eq!(main(args(&[])), 2);
    assert_eq!(main(args(&["frobnicate"])), 2);
    assert_eq!(main(args(&["check", "--bogus"])), 2);
    assert_eq!(main(args(&["extract"])), 2);
}
