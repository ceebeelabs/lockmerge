//! Reading fixture cases in the ADR 0007 per-side layout (section 6.2).
//!
//! ```text
//! tests/fixtures/<ecosystem>/<owner>-<repo>/<merge-sha12>/
//!   base ours theirs expected        exact bytes, neutral names
//!   manifests/{base,ours,theirs}/<repo-relative path>
//!   case.toml
//! ```
//!
//! The rule fixtures of `tests/fixtures/rules/` use another layout (see #47)
//! and are not read here.

use crate::classify::{Kind, Waiver};
use crate::util::sha256_hex;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::DocumentMut;

/// Directory names under `tests/fixtures/` that hold benchmark cases.
pub const ECOSYSTEMS: [&str; 4] = ["cargo", "cargo-agent", "uv", "pnpm"];

/// Ecosystems that must exist: deleting the directory must not make the gate
/// check nothing.
pub const REQUIRED_ECOSYSTEMS: [&str; 1] = ["cargo"];

/// Licenses a fixture repository may have (standing rule 6).
pub const ALLOWED_LICENSES: [&str; 7] = [
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
];

/// Whether every term of an SPDX `A OR B` expression is allowed.
pub fn license_allowed(expr: &str) -> bool {
    !expr.trim().is_empty()
        && expr
            .split(" OR ")
            .all(|term| ALLOWED_LICENSES.contains(&term.trim()))
}

/// The three sides that carry manifests.
pub const SIDES: [&str; 3] = ["base", "ours", "theirs"];

pub struct ManifestFile {
    pub side: String,
    /// Repository-relative path, for example `crates/a/Cargo.toml`.
    pub rel: String,
    pub bytes: Vec<u8>,
}

/// One file of the case's registry index cache: `index/<index directory>/<path>`
/// is saved here as `rel` = `<index directory>/<path>`, the layout of the
/// testkit's `FakeIndex`.
pub struct IndexFile {
    pub rel: String,
    pub bytes: Vec<u8>,
}

pub struct Meta {
    pub repository: String,
    pub license: String,
    pub merge_commit: String,
    pub base_commit: String,
    pub parents: Vec<String>,
    pub lockfile_path: String,
    pub kind: Kind,
    pub foreign_count: Option<i64>,
    pub waiver: Option<Waiver>,
}

pub struct Case {
    /// `cargo/sharkdp-bat/02e6ff4183f6`
    pub id: String,
    pub ecosystem: String,
    pub meta: Meta,
    pub base: Vec<u8>,
    pub ours: Vec<u8>,
    pub theirs: Vec<u8>,
    pub expected: Vec<u8>,
    pub manifests: Vec<ManifestFile>,
    /// The registry index cache of the case; empty when the case has none.
    pub index: Vec<IndexFile>,
    /// Integrity failures found while loading: a hash that differs, a file
    /// that is not listed or is missing, a license outside the list.
    pub problems: Vec<String>,
}

fn is_sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Every file under `dir`, keyed by `/`-separated relative path.
fn walk(dir: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
    fn go(root: &Path, dir: &Path, out: &mut BTreeMap<String, PathBuf>) -> Result<(), String> {
        let rd = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in rd {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let ty = entry.file_type().map_err(|e| e.to_string())?;
            if ty.is_dir() {
                go(root, &path, out)?;
            } else if ty.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(rel, path);
            } else {
                return Err(format!(
                    "{}: not a regular file or directory",
                    path.display()
                ));
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    go(dir, dir, &mut out)?;
    Ok(out)
}

fn text(doc: &DocumentMut, key: &str) -> Option<String> {
    doc.get(key).and_then(|i| i.as_str()).map(str::to_string)
}

fn parse_meta(doc: &DocumentMut) -> Result<Meta, String> {
    let kind = text(doc, "kind")
        .as_deref()
        .and_then(Kind::parse)
        .ok_or("case.toml: `kind` must be \"reachable\" or \"re-resolved\"")?;
    let lockfile_path = text(doc, "lockfile_path").ok_or("case.toml: no `lockfile_path`")?;
    if text(doc, "ours_is").as_deref() != Some("parent1") {
        return Err("case.toml: `ours_is` must be \"parent1\"".into());
    }
    let parents = doc
        .get("parents")
        .and_then(|i| i.as_value())
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    // A waiver that does not parse or has no reason is ignored (section 5).
    let waiver = doc.get("waiver").and_then(|i| i.as_table()).and_then(|t| {
        let reason = t.get("reason")?.as_str()?.trim().to_string();
        let sha = t.get("result_sha256")?.as_str()?.to_string();
        (!reason.is_empty() && is_sha(&sha)).then_some(Waiver {
            reason,
            result_sha256: sha,
        })
    });
    Ok(Meta {
        repository: text(doc, "repository").unwrap_or_default(),
        license: text(doc, "license").unwrap_or_default(),
        merge_commit: text(doc, "merge_commit").unwrap_or_default(),
        base_commit: text(doc, "base_commit").unwrap_or_default(),
        parents,
        lockfile_path,
        kind,
        foreign_count: doc.get("foreign_ids_count").and_then(|i| i.as_integer()),
        waiver,
    })
}

/// Loads one case directory. `Err` means the case cannot be read at all.
pub fn load_case(ecosystem: &str, id: &str, dir: &Path) -> Result<Case, String> {
    let fail = |m: String| format!("{id}: {m}");
    let files = walk(dir).map_err(&fail)?;
    let toml_path = files
        .get("case.toml")
        .ok_or_else(|| fail("no case.toml".into()))?;
    let toml_text = fs::read_to_string(toml_path).map_err(|e| fail(format!("case.toml: {e}")))?;
    let doc: DocumentMut = toml_text
        .parse()
        .map_err(|e| fail(format!("case.toml does not parse: {e}")))?;
    let meta = parse_meta(&doc).map_err(&fail)?;

    let mut problems = Vec::new();
    let recorded: BTreeMap<String, String> = doc
        .get("sha256")
        .and_then(|i| i.as_table())
        .map(|t| {
            t.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.to_string(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let mut contents: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (rel, path) in &files {
        if rel == "case.toml" {
            continue;
        }
        let bytes = fs::read(path).map_err(|e| fail(format!("{rel}: {e}")))?;
        match recorded.get(rel) {
            None => problems.push(format!("{id}: file `{rel}` has no recorded SHA-256")),
            Some(want) if *want != sha256_hex(&bytes) => {
                problems.push(format!("{id}: `{rel}` does not match its recorded SHA-256"))
            }
            Some(_) => {}
        }
        contents.insert(rel.clone(), bytes);
    }
    for rel in recorded.keys() {
        if !contents.contains_key(rel) {
            problems.push(format!("{id}: `{rel}` is recorded but missing"));
        }
    }
    if !license_allowed(&meta.license) {
        problems.push(format!(
            "{id}: license `{}` is not in the allowed list",
            meta.license
        ));
    }

    let mut take = |name: &str| -> Result<Vec<u8>, String> {
        contents
            .remove(name)
            .ok_or_else(|| fail(format!("missing file `{name}`")))
    };
    let base = take("base")?;
    let ours = take("ours")?;
    let theirs = take("theirs")?;
    let expected = take("expected")?;

    let mut manifests = Vec::new();
    let mut index = Vec::new();
    for (rel, bytes) in contents {
        let mut parts = rel.splitn(3, '/');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("manifests"), Some(side), Some(path)) if SIDES.contains(&side) => {
                manifests.push(ManifestFile {
                    side: side.to_string(),
                    rel: path.to_string(),
                    bytes,
                })
            }
            (Some("index"), Some(_), Some(_)) => index.push(IndexFile {
                rel: rel["index/".len()..].to_string(),
                bytes,
            }),
            _ => problems.push(format!("{id}: unexpected file `{rel}`")),
        }
    }
    Ok(Case {
        id: id.to_string(),
        ecosystem: ecosystem.to_string(),
        meta,
        base,
        ours,
        theirs,
        expected,
        manifests,
        index,
        problems,
    })
}

/// The case directories of one ecosystem: `<eco>/<repo>/<sha>/`.
pub fn case_dirs(eco_dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut out = Vec::new();
    let eco = eco_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let list = |p: &Path| -> Result<Vec<PathBuf>, String> {
        let mut v: Vec<PathBuf> = fs::read_dir(p)
            .map_err(|e| format!("{}: {e}", p.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir())
            .collect();
        v.sort();
        Ok(v)
    };
    for repo in list(eco_dir)? {
        for case in list(&repo)? {
            let id = format!(
                "{eco}/{}/{}",
                repo.file_name().unwrap_or_default().to_string_lossy(),
                case.file_name().unwrap_or_default().to_string_lossy()
            );
            out.push((id, case));
        }
    }
    Ok(out)
}

/// The size of one corpus directory, two ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TreeSize {
    /// Bytes with each distinct file content counted once. Git stores
    /// identical contents once, so this is what the size budget bounds.
    pub unique: u64,
    /// Bytes of every file on disk.
    pub raw: u64,
}

/// Size of every file under `dir`: each distinct content (by SHA-256) counts
/// once in `unique`; `raw` counts every file.
pub fn tree_size(dir: &Path) -> Result<TreeSize, String> {
    let mut seen = std::collections::HashSet::new();
    let mut size = TreeSize::default();
    for path in walk(dir)?.values() {
        let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let len = bytes.len() as u64;
        size.raw += len;
        if seen.insert(sha256_hex(&bytes)) {
            size.unique += len;
        }
    }
    Ok(size)
}

/// The failure message for a corpus over `budget`, or `None`. The budget is
/// checked against the deduplicated size; the message names it and the raw size.
pub fn over_budget(what: &str, size: TreeSize, budget: u64) -> Option<String> {
    (size.unique > budget).then(|| {
        format!(
            "{what}: fixtures take {} bytes deduplicated ({} bytes on disk), over the budget of {budget}",
            size.unique, size.raw
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::TempDir;

    #[test]
    fn identical_contents_count_once_in_the_unique_size() {
        let tmp = TempDir::new("dedup").unwrap();
        fs::create_dir_all(tmp.path().join("a")).unwrap();
        fs::create_dir_all(tmp.path().join("b")).unwrap();
        fs::write(tmp.path().join("a/f"), b"12345").unwrap();
        fs::write(tmp.path().join("b/g"), b"12345").unwrap();
        let s = tree_size(tmp.path()).unwrap();
        assert_eq!(s, TreeSize { unique: 5, raw: 10 });
    }

    #[test]
    fn distinct_contents_of_equal_length_count_separately() {
        let tmp = TempDir::new("distinct").unwrap();
        fs::write(tmp.path().join("f"), b"12345").unwrap();
        fs::write(tmp.path().join("g"), b"12346").unwrap();
        fs::write(tmp.path().join("h"), b"123").unwrap();
        let s = tree_size(tmp.path()).unwrap();
        assert_eq!(
            s,
            TreeSize {
                unique: 13,
                raw: 13
            }
        );
    }

    #[test]
    fn the_budget_failure_names_the_deduplicated_size() {
        let size = TreeSize {
            unique: 60,
            raw: 900,
        };
        let msg = over_budget("cargo", size, 50).unwrap();
        assert!(msg.contains("60 bytes deduplicated"), "{msg}");
        assert!(msg.contains("900 bytes on disk"), "{msg}");
        assert!(msg.contains("budget of 50"), "{msg}");
        // The raw size alone never fails a corpus whose unique size fits.
        assert_eq!(
            over_budget(
                "cargo",
                TreeSize {
                    unique: 50,
                    raw: 900
                },
                50
            ),
            None
        );
    }

    pub(crate) fn write_case(dir: &Path, tamper: bool) {
        fs::create_dir_all(dir.join("manifests/base")).unwrap();
        let files: [(&str, &[u8]); 5] = [
            ("base", b"b"),
            ("ours", b"o"),
            ("theirs", b"t"),
            ("expected", b"e"),
            ("manifests/base/Cargo.toml", b"[package]\n"),
        ];
        let mut toml = String::from(
            "repository = \"https://example.invalid/r\"\nlicense = \"MIT OR Apache-2.0\"\nlockfile_path = \"Cargo.lock\"\nours_is = \"parent1\"\nkind = \"reachable\"\n[sha256]\n",
        );
        for (rel, bytes) in files {
            fs::write(dir.join(rel), bytes).unwrap();
            toml.push_str(&format!("\"{rel}\" = \"{}\"\n", sha256_hex(bytes)));
        }
        fs::write(dir.join("case.toml"), toml).unwrap();
        if tamper {
            fs::write(dir.join("ours"), b"changed").unwrap();
        }
    }

    #[test]
    fn loads_a_case_and_its_manifests() {
        let t = TempDir::new("case").unwrap();
        write_case(t.path(), false);
        let c = load_case("cargo", "cargo/r/x", t.path()).unwrap();
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!(c.ours, b"o");
        assert_eq!(c.manifests.len(), 1);
        assert_eq!(
            (c.manifests[0].side.as_str(), c.manifests[0].rel.as_str()),
            ("base", "Cargo.toml")
        );
    }

    #[test]
    fn a_changed_fixture_is_an_integrity_problem() {
        let t = TempDir::new("case").unwrap();
        write_case(t.path(), true);
        let c = load_case("cargo", "cargo/r/x", t.path()).unwrap();
        assert_eq!(c.problems.len(), 1);
        assert!(c.problems[0].contains("`ours` does not match"));
    }

    #[test]
    fn an_unlisted_file_is_an_integrity_problem() {
        let t = TempDir::new("case").unwrap();
        write_case(t.path(), false);
        fs::write(t.path().join("manifests/base/extra"), b"x").unwrap();
        let c = load_case("cargo", "cargo/r/x", t.path()).unwrap();
        assert!(c.problems.iter().any(|p| p.contains("no recorded SHA-256")));
    }

    #[test]
    fn waiver_without_reason_is_ignored() {
        let t = TempDir::new("case").unwrap();
        write_case(t.path(), false);
        let mut s = fs::read_to_string(t.path().join("case.toml")).unwrap();
        s.push_str(&format!(
            "[waiver]\nreason = \" \"\nresult_sha256 = \"{}\"\n",
            "a".repeat(64)
        ));
        fs::write(t.path().join("case.toml"), &s).unwrap();
        assert!(
            load_case("cargo", "x", t.path())
                .unwrap()
                .meta
                .waiver
                .is_none()
        );
        let s = s.replace("reason = \" \"", "reason = \"reviewed\"");
        fs::write(t.path().join("case.toml"), s).unwrap();
        assert!(
            load_case("cargo", "x", t.path())
                .unwrap()
                .meta
                .waiver
                .is_some()
        );
    }

    #[test]
    fn license_list() {
        assert!(license_allowed("MIT OR Apache-2.0"));
        assert!(!license_allowed("GPL-3.0-only"));
        assert!(!license_allowed("MIT OR MPL-2.0"));
        assert!(!license_allowed(""));
    }
}
