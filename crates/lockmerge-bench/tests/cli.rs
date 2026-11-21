//! The command line, on the fixtures committed in this repository.

use lockmerge_bench::cli::main;

fn fixtures() -> String {
    format!("{}/../../tests/fixtures", env!("CARGO_MANIFEST_DIR"))
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// The `lockmerge` binary that `cargo test --workspace` builds (lockmerge-cli's
/// tests need it), next to this test's `deps/` directory.
fn built_driver() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let bin = exe.parent()?.parent()?.join("lockmerge");
    bin.is_file().then(|| bin.to_string_lossy().into_owned())
}

#[test]
fn the_committed_corpus_passes_the_per_pr_gate() {
    // The `bench` CI job runs this same check with the built driver. Replaying
    // the committed corpus needs the `lockmerge` binary; it is built by a
    // workspace test run, not by `cargo test -p lockmerge-bench` alone.
    let Some(driver) = built_driver() else {
        eprintln!("skipped: no built `lockmerge` binary; run `cargo test --workspace`");
        return;
    };
    assert_eq!(
        main(args(&[
            "check",
            "--fixtures",
            &fixtures(),
            "--driver-bin",
            &driver
        ])),
        0
    );
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
