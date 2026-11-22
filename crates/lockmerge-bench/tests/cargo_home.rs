//! The registry cache of a replay comes only from the case (docs/02-oracle.md
//! section 3.3): a case without `index/` refuses R15 even when the outer
//! `CARGO_HOME` holds a matching entry.
//!
//! The outer `CARGO_HOME` has to be set in the environment of the process that
//! runs the merge, and a test cannot set it safely in its own, so the test
//! runs itself again as a child with the variable set.

use lockmerge_bench::attribution::attribute;
use lockmerge_bench::case::{Case, IndexFile, ManifestFile, Meta};
use lockmerge_bench::classify::Kind;
use lockmerge_bench::replay::case_cargo_home;
use lockmerge_bench::util::TempDir;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CHILD: &str = "LOCKMERGE_TEST_CARGO_HOME_CHILD";

fn v6(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/rules/cargo/V6")
        .join(name)
}

/// Every file under `dir` as (path relative to `dir` with `/`, bytes).
fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn go(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                go(root, &p, out);
            } else {
                let rel = p.strip_prefix(root).unwrap();
                let rel: Vec<_> = rel.iter().map(|c| c.to_string_lossy()).collect();
                out.push((rel.join("/"), fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    if dir.is_dir() {
        go(dir, dir, &mut out);
    }
    out
}

/// The V6 case `accept-links-free` as a benchmark case, with or without its
/// index cache.
fn case(with_index: bool) -> Case {
    let dir = v6("accept-links-free");
    let manifests = ["base", "ours", "theirs"]
        .iter()
        .flat_map(|side| {
            files(&dir.join("sides").join(side))
                .into_iter()
                .map(|(rel, bytes)| ManifestFile {
                    side: side.to_string(),
                    rel,
                    bytes,
                })
        })
        .collect();
    let index = if with_index {
        files(&dir.join("index"))
            .into_iter()
            .map(|(rel, bytes)| IndexFile { rel, bytes })
            .collect()
    } else {
        Vec::new()
    };
    Case {
        id: "cargo/test/v6".into(),
        ecosystem: "cargo".into(),
        meta: Meta {
            repository: String::new(),
            license: "MIT".into(),
            merge_commit: String::new(),
            base_commit: String::new(),
            parents: Vec::new(),
            lockfile_path: "Cargo.lock".into(),
            kind: Kind::Reachable,
            foreign_count: None,
            waiver: None,
            pr_branch: false,
        },
        base: fs::read(dir.join("base")).unwrap(),
        ours: fs::read(dir.join("ours")).unwrap(),
        theirs: fs::read(dir.join("theirs")).unwrap(),
        expected: fs::read(dir.join("expected")).unwrap(),
        manifests,
        index,
        problems: Vec::new(),
    }
}

fn refusal(case: &Case) -> Option<String> {
    let formats = [lockmerge_cargo::format()];
    attribute(&formats, case).unwrap().unwrap().refusal
}

#[test]
fn the_case_home_holds_exactly_the_case_index() {
    let with = case_cargo_home(&case(true)).unwrap();
    let got = files(&with.path().join("registry/index"));
    let want = files(&v6("accept-links-free").join("index"));
    assert!(!want.is_empty());
    assert_eq!(got, want);
    let without = case_cargo_home(&case(false)).unwrap();
    assert!(files(without.path()).is_empty());
}

/// Runs inside the child only.
#[test]
fn child_checks() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    // The outer CARGO_HOME holds the matching entries of the V6 case.
    let outer = std::env::var_os("CARGO_HOME").unwrap();
    assert!(!files(&Path::new(&outer).join("registry/index")).is_empty());
    assert_eq!(refusal(&case(false)).as_deref(), Some("cargo.R15"));
    // With the case's own index the same merge is accepted.
    assert_eq!(refusal(&case(true)), None);
}

#[test]
fn a_case_without_index_refuses_r15_even_when_the_outer_cargo_home_has_the_entry() {
    let outer = TempDir::new("outer-cargo-home").unwrap();
    for (rel, bytes) in files(&v6("accept-links-free").join("index")) {
        let p = outer.path().join("registry/index").join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_checks", "--nocapture"])
        .env(CHILD, "1")
        .env("CARGO_HOME", outer.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("1 passed"), "the child did not run: {text}");
}
