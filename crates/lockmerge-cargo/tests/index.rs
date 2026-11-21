//! The pure registry index functions (docs/formats/cargo-registry-index.md)
//! and the reasons R15 gives, over the V6 fixtures.

use lockmerge_cargo::index::{IndexLookup, entry_path, lookup, parse_entry};
use lockmerge_core::testkit::Case;
use lockmerge_core::{Context, Inputs, Outcome};
use std::path::{Path, PathBuf};

const SUM: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HEADER: &[u8] = b"\x03\x02\0\0\0";

fn record(vers: &str, rest: &str) -> Vec<u8> {
    format!("{vers}\0{{\"name\":\"alpha\",\"vers\":\"{vers}\",{rest}}}\0").into_bytes()
}

fn file(header: &[u8], records: &[Vec<u8>]) -> Vec<u8> {
    let mut out = header.to_vec();
    out.extend_from_slice(b"etag: \"x\"\0");
    for r in records {
        out.extend_from_slice(r);
    }
    out
}

fn good(links: Option<&str>) -> Vec<u8> {
    let l = links.map_or(String::new(), |l| format!(",\"links\":\"{l}\""));
    file(
        HEADER,
        &[
            record("0.9.0", &format!("\"cksum\":\"{OTHER}\"")),
            record("1.0.0", &format!("\"cksum\":\"{SUM}\"{l}")),
        ],
    )
}

fn found(links: Option<&str>) -> IndexLookup {
    IndexLookup::Found {
        links: links.map(str::to_string),
    }
}

fn unavailable(files: &[Vec<u8>]) -> bool {
    matches!(
        lookup(files, "alpha", "1.0.0", SUM),
        IndexLookup::Unavailable(_)
    )
}

#[test]
fn entry_paths_by_name_length() {
    for (name, path) in [
        ("a", ".cache/1/a"),
        ("ab", ".cache/2/ab"),
        ("abc", ".cache/3/a/abc"),
        ("abcd", ".cache/ab/cd/abcd"),
        ("libz-sys", ".cache/li/bz/libz-sys"),
        ("a_b-c1", ".cache/a_/b-/a_b-c1"),
    ] {
        assert_eq!(entry_path(name).as_deref(), Some(path), "{name}");
    }
}

#[test]
fn entry_paths_refuse_names_that_are_not_pinned() {
    for name in ["", "Serde", "a.b", "a/b", "../x", "naïve", "a b"] {
        assert_eq!(entry_path(name), None, "{name:?}");
    }
}

#[test]
fn a_matching_entry_gives_its_links_or_none() {
    assert_eq!(lookup(&[good(None)], "alpha", "1.0.0", SUM), found(None));
    assert_eq!(
        lookup(&[good(Some("z"))], "alpha", "1.0.0", SUM),
        found(Some("z"))
    );
}

#[test]
fn name_version_and_checksum_must_all_match() {
    let f = [good(Some("z"))];
    assert!(matches!(
        lookup(&f, "alpha", "1.0.0", OTHER),
        IndexLookup::Unavailable(_)
    ));
    assert!(matches!(
        lookup(&f, "alpha", "1.0.1", SUM),
        IndexLookup::Unavailable(_)
    ));
    assert!(matches!(
        lookup(&f, "beta", "1.0.0", SUM),
        IndexLookup::Unavailable(_)
    ));
    // The record of 0.9.0 has the other checksum: that one matches 0.9.0 only.
    assert_eq!(lookup(&f, "alpha", "0.9.0", OTHER), found(None));
}

#[test]
fn a_file_that_is_not_read_exactly_proves_nothing() {
    let rec = record("1.0.0", &format!("\"cksum\":\"{SUM}\""));
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        (
            "cache version 4",
            file(b"\x04\x02\0\0\0", std::slice::from_ref(&rec)),
        ),
        (
            "cache version 2",
            file(b"\x02\x02\0\0\0", std::slice::from_ref(&rec)),
        ),
        (
            "index version 3",
            file(b"\x03\x03\0\0\0", std::slice::from_ref(&rec)),
        ),
        ("header only", b"\x03\x02\0\0\0".to_vec()),
        ("no final NUL", {
            let mut r = rec.clone();
            r.pop();
            file(HEADER, &[r])
        }),
        ("extra byte", {
            let mut f = file(HEADER, std::slice::from_ref(&rec));
            f.push(b'x');
            f
        }),
        ("version without JSON", {
            let mut f = file(HEADER, &[]);
            f.extend_from_slice(b"1.0.0\0");
            f
        }),
        (
            "JSON not an object",
            file(HEADER, &[b"1.0.0\0[]\0".to_vec()]),
        ),
        (
            "links is a number",
            file(
                HEADER,
                &[record("1.0.0", &format!("\"cksum\":\"{SUM}\",\"links\":1"))],
            ),
        ),
        (
            "links is empty",
            file(
                HEADER,
                &[record(
                    "1.0.0",
                    &format!("\"cksum\":\"{SUM}\",\"links\":\"\""),
                )],
            ),
        ),
        (
            "schema v 3",
            file(
                HEADER,
                &[record("1.0.0", &format!("\"cksum\":\"{SUM}\",\"v\":3"))],
            ),
        ),
        (
            "two records of one version",
            file(HEADER, &[rec.clone(), rec.clone()]),
        ),
    ];
    for (what, bytes) in cases {
        assert!(unavailable(&[bytes]), "{what}");
    }
}

#[test]
fn a_record_whose_vers_differs_from_its_text_is_not_read() {
    let wrong = b"1.0.0\0{\"name\":\"alpha\",\"vers\":\"1.0.1\",\"cksum\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}\0".to_vec();
    assert!(unavailable(&[file(HEADER, &[wrong])]));
}

#[test]
fn an_unreadable_file_does_not_stop_a_good_one() {
    let bad = file(b"\x04\x02\0\0\0", &[]);
    assert_eq!(
        lookup(&[bad, good(Some("z"))], "alpha", "1.0.0", SUM),
        found(Some("z"))
    );
}

#[test]
fn directories_that_disagree_on_links_prove_nothing() {
    assert!(unavailable(&[good(Some("z")), good(None)]));
    assert_eq!(
        lookup(&[good(Some("z")), good(Some("z"))], "alpha", "1.0.0", SUM),
        found(Some("z"))
    );
}

#[test]
fn parse_entry_splits_the_records() {
    let f = good(None);
    let records = parse_entry(&f).unwrap();
    let versions: Vec<_> = records.iter().map(|r| r.version).collect();
    assert_eq!(versions, ["0.9.0", "1.0.0"]);
}

// ---- the reason of each refusal, over the V6 fixtures ---------------------

fn v6(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/rules/cargo/V6")
        .join(name)
}

/// The primary refusal's detail text, or `None` if the case merged.
fn detail(name: &str) -> Option<String> {
    let case = Case::load(&v6(name)).unwrap();
    let cx = Context::new(&case.manifests).with_index(&case.index);
    let report = lockmerge_cargo::format()
        .merge(
            &Inputs {
                base: &case.base,
                ours: &case.ours,
                theirs: &case.theirs,
                path: "Cargo.lock",
            },
            &cx,
        )
        .unwrap();
    match report.outcome {
        Outcome::Clean(_) => None,
        Outcome::Refused(r) => Some(
            r.facts
                .iter()
                .filter(|(k, _)| *k == "detail")
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>()
                .join("; "),
        ),
    }
}

#[test]
fn each_refusal_names_its_reason() {
    for (name, expect) in [
        ("refuse-shared-links", "share the native library `sqlite3`"),
        (
            "refuse-shared-links-with-path-member",
            "share the native library `z`",
        ),
        ("refuse-checksum-mismatch", "no index entry matches"),
        ("refuse-entry-missing", "no index entry matches"),
        ("refuse-cache-absent", "no index entry matches"),
        ("refuse-unknown-cache-format", "no index entry matches"),
        ("refuse-git-package-already-in-the-lockfile", "a git source"),
        ("refuse-git-id-on-ours", "not a registry package"),
        ("refuse-git-id-on-theirs", "not a registry package"),
    ] {
        let got = detail(name).unwrap_or_else(|| panic!("{name} merged"));
        assert!(got.contains(expect), "{name}: {got}");
    }
}

#[test]
fn the_neighbors_that_merge_do() {
    for name in [
        "accept-links-free",
        "accept-different-links",
        "accept-several-index-directories",
        "accept-path-member-links-different",
    ] {
        assert!(detail(name).is_none(), "{name}");
    }
}

/// Without a cache in the context at all (a caller that provides none), the
/// registry pair is refused as well.
#[test]
fn a_context_without_a_cache_refuses() {
    let case = Case::load(&v6("accept-links-free")).unwrap();
    let cx = Context::new(&case.manifests);
    let report = lockmerge_cargo::format()
        .merge(
            &Inputs {
                base: &case.base,
                ours: &case.ours,
                theirs: &case.theirs,
                path: "Cargo.lock",
            },
            &cx,
        )
        .unwrap();
    let Outcome::Refused(r) = report.outcome else {
        panic!("merged without a cache");
    };
    assert_eq!(r.code.to_string(), "cargo.R15");
}
