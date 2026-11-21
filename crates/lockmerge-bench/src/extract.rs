//! `lockmerge-bench extract` (docs/02-oracle.md section 6.1): saves each
//! selected merge as raw bytes, with its manifests, license and `case.toml`.
//!
//! Run by hand, by the owner side, with network access. Never a package
//! manager: every byte is `git cat-file blob`.

use crate::case::license_allowed;
use crate::classify::Kind;
use crate::oracle::{CargoOracle, Oracle, path_package_names};
use crate::scan::{Candidate, Considered, Outcome, Selection, Source, ensure_clone, git_out, scan};
use crate::util::sha256_hex;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::DocumentMut;

pub const EXTRACTOR_VERSION: &str = env!("CARGO_PKG_VERSION");

fn q(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn normalized(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The SPDX terms the text of one license file shows, among MIT and
/// Apache-2.0. Anything else is not detected, and needs the owner's word.
fn detect(text: &str) -> BTreeSet<&'static str> {
    let t = normalized(text);
    let mut found = BTreeSet::new();
    if t.contains("apache license") && t.contains("version 2.0") {
        found.insert("Apache-2.0");
    }
    if t.contains("permission is hereby granted, free of charge, to any person obtaining a copy") {
        found.insert("MIT");
    }
    found
}

/// The license files at the pinned commit and the SPDX expression.
/// A file name and its bytes.
type NamedFile = (String, Vec<u8>);

fn license(repo: &Path, src: &Source) -> Result<(String, Vec<NamedFile>), String> {
    let listing =
        String::from_utf8_lossy(&git_out(repo, &["ls-tree", "--name-only", &src.revision])?)
            .into_owned();
    let mut files = Vec::new();
    let mut found = BTreeSet::new();
    for name in listing.lines() {
        let lower = name.to_lowercase();
        if !(lower.starts_with("license")
            || lower.starts_with("licence")
            || lower.starts_with("copying"))
        {
            continue;
        }
        let spec = format!("{}:{name}", src.revision);
        let bytes = git_out(repo, &["cat-file", "blob", &spec])?;
        found.extend(detect(&String::from_utf8_lossy(&bytes)));
        files.push((name.to_string(), bytes));
    }
    let detected = match (found.contains("MIT"), found.contains("Apache-2.0")) {
        (true, true) => Some("MIT OR Apache-2.0".to_string()),
        (true, false) => Some("MIT".to_string()),
        (false, true) => Some("Apache-2.0".to_string()),
        (false, false) => None,
    };
    let expr = match (&src.license, detected) {
        (Some(d), Some(f)) => {
            let set = |e: &str| {
                e.split(" OR ")
                    .map(|t| t.trim().to_string())
                    .collect::<BTreeSet<_>>()
            };
            if set(d) != set(&f) {
                return Err(format!(
                    "{}: sources.toml says {d} but the license files at the pin show {f}",
                    src.name
                ));
            }
            d.clone()
        }
        (Some(d), None) => d.clone(),
        (None, Some(f)) => f,
        (None, None) => {
            return Err(format!(
                "{}: no license recognized at the pin; this needs the owner's sign-off, then `license` in sources.toml",
                src.name
            ));
        }
    };
    if !license_allowed(&expr) {
        return Err(format!(
            "{}: license `{expr}` is outside the allowed list; this needs the owner's sign-off",
            src.name
        ));
    }
    if files.is_empty() {
        return Err(format!("{}: no license file at the pin to copy", src.name));
    }
    Ok((expr, files))
}

/// The manifests of one side: the root manifest next to the lockfile, every
/// `Cargo.toml` whose package is a path package of that side's lockfile, and
/// for each of those the nearest ancestor manifest that has a `[workspace]`
/// table: a member inherits from it, and a virtual root has no package name to
/// select it by.
fn side_manifests(
    repo: &Path,
    commit: &str,
    lock_path: &str,
    lock: &[u8],
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let names = path_package_names(lock)?;
    let listing =
        String::from_utf8_lossy(&git_out(repo, &["ls-tree", "-r", "--name-only", commit])?)
            .into_owned();
    let lock_dir = Path::new(lock_path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let root = if lock_dir.is_empty() {
        "Cargo.toml".to_string()
    } else {
        format!("{lock_dir}/Cargo.toml")
    };
    // Every manifest at the commit, parsed once. `None` if it does not parse.
    let mut all: BTreeMap<String, (Vec<u8>, Option<DocumentMut>)> = BTreeMap::new();
    for path in listing.lines() {
        if Path::new(path).file_name().and_then(|n| n.to_str()) != Some("Cargo.toml") {
            continue;
        }
        let bytes = git_out(repo, &["cat-file", "blob", &format!("{commit}:{path}")])?;
        let doc = String::from_utf8(bytes.clone())
            .ok()
            .and_then(|t| t.parse::<DocumentMut>().ok());
        all.insert(path.to_string(), (bytes, doc));
    }
    let mut keep: BTreeSet<String> = BTreeSet::new();
    for (path, (_, doc)) in &all {
        let named = doc
            .as_ref()
            .and_then(|d| {
                d.get("package")?
                    .as_table_like()?
                    .get("name")?
                    .as_str()
                    .map(str::to_string)
            })
            .is_some_and(|n| names.contains(&n));
        if path == &root || named {
            keep.insert(path.clone());
        }
    }
    // The nearest ancestor with a `[workspace]` table, as cargo finds it.
    let mut roots = BTreeSet::new();
    for path in keep.iter().filter(|p| **p != root) {
        let mut dir = Path::new(path).parent().and_then(Path::parent);
        while let Some(d) = dir {
            let candidate = if d.as_os_str().is_empty() {
                "Cargo.toml".to_string()
            } else {
                format!("{}/Cargo.toml", d.to_string_lossy())
            };
            if all
                .get(&candidate)
                .and_then(|(_, doc)| doc.as_ref())
                .is_some_and(|doc| doc.get("workspace").is_some())
            {
                roots.insert(candidate);
                break;
            }
            dir = d.parent();
        }
    }
    keep.extend(roots);
    Ok(keep
        .into_iter()
        .filter_map(|p| all.remove(&p).map(|(bytes, _)| (p, bytes)))
        .collect())
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn case_toml(
    src: &Source,
    c: &Candidate,
    license: &str,
    shas: &[(String, String)],
    date: &str,
) -> String {
    let mut s = String::new();
    s.push_str(&format!("repository = {}\n", q(&src.url)));
    s.push_str(&format!("license = {}\n", q(license)));
    s.push_str(&format!("merge_commit = {}\n", q(&c.merge)));
    s.push_str(&format!(
        "parents = [{}, {}]\n",
        q(&c.parents[0]),
        q(&c.parents[1])
    ));
    s.push_str(&format!("base_commit = {}\n", q(&c.base)));
    s.push_str(&format!("lockfile_path = {}\n", q(&c.path)));
    s.push_str("ours_is = \"parent1\"\n");
    s.push_str(&format!("kind = {}\n", q(c.kind.name())));
    s.push_str(&format!("foreign_ids_count = {}\n", c.foreign.len()));
    let first: Vec<String> = c.foreign.iter().take(10).map(|i| q(i)).collect();
    s.push_str(&format!("foreign_ids = [{}]\n", first.join(", ")));
    s.push_str(&format!("extractor_version = {}\n", q(EXTRACTOR_VERSION)));
    s.push_str(&format!("extracted_on = {}\n", q(date)));
    s.push_str("\n[sha256]\n");
    for (rel, sha) in shas {
        s.push_str(&format!("{} = {}\n", q(rel), q(sha)));
    }
    s
}

/// Writes one case. Returns its directory.
fn write_case(
    repo: &Path,
    src: &Source,
    c: &Candidate,
    license: &str,
    dir: &Path,
    date: &str,
) -> Result<(), String> {
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("base".into(), c.base_bytes.clone()),
        ("ours".into(), c.ours.clone()),
        ("theirs".into(), c.theirs.clone()),
        ("expected".into(), c.expected.clone()),
    ];
    for (side, commit, lock) in [
        ("base", c.base.as_str(), &c.base_bytes),
        ("ours", c.parents[0].as_str(), &c.ours),
        ("theirs", c.parents[1].as_str(), &c.theirs),
    ] {
        for (rel, bytes) in side_manifests(repo, commit, &c.path, lock)? {
            files.push((format!("manifests/{side}/{rel}"), bytes));
        }
    }
    let mut shas = Vec::new();
    for (rel, bytes) in &files {
        write(&dir.join(rel), bytes)?;
        shas.push((rel.clone(), sha256_hex(bytes)));
    }
    write(
        &dir.join("case.toml"),
        case_toml(src, c, license, &shas, date).as_bytes(),
    )
}

/// Merges this run's entries into `<eco>/INDEX.toml`, keeping the entries of
/// other repositories.
fn write_index(path: &Path, src: &Source, considered: &[Considered]) -> Result<(), String> {
    let mut entries: Vec<(String, String, String, String)> = Vec::new();
    if let Ok(existing) = fs::read_to_string(path) {
        let doc: DocumentMut = existing
            .parse()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some(list) = doc.get("merge").and_then(|i| i.as_array_of_tables()) {
            for t in list.iter() {
                let s = |k: &str| t.get(k).and_then(|i| i.as_str()).unwrap_or("").to_string();
                if s("repository") != src.url {
                    entries.push((
                        s("repository"),
                        s("merge_commit"),
                        s("lockfile_path"),
                        s("result"),
                    ));
                }
            }
        }
    }
    for c in considered {
        let result = match &c.outcome {
            Outcome::Skipped(why) => why.clone(),
            Outcome::Case(b) => format!("case:{}", b.kind.name()),
        };
        entries.push((src.url.clone(), c.merge.clone(), c.path.clone(), result));
    }
    entries.sort();
    let mut s = String::from(
        "# Every merge considered by `lockmerge-bench extract`, with the result: a case\n# or the reason it was skipped (docs/02-oracle.md section 3.1).\n",
    );
    for (repo, merge, lock, result) in entries {
        s.push_str(&format!(
            "\n[[merge]]\nrepository = {}\nmerge_commit = {}\nlockfile_path = {}\nresult = {}\n",
            q(&repo),
            q(&merge),
            q(&lock),
            q(&result)
        ));
    }
    write(path, s.as_bytes())
}

pub struct ExtractOpts {
    pub out: PathBuf,
    pub cache: PathBuf,
    pub date: String,
}

/// Extracts one source. Returns a one-line summary.
pub fn extract(src: &Source, opts: &ExtractOpts) -> Result<String, String> {
    if src.ecosystem != "cargo" {
        return Err(format!(
            "{}: extraction of `{}` needs its oracle, which does not exist yet",
            src.name, src.ecosystem
        ));
    }
    let oracle = CargoOracle;
    let repo = opts.cache.join(format!("{}.git", src.name));
    ensure_clone(src, &repo)?;
    let (license_expr, license_files) = license(&repo, src)?;

    let mut seen: BTreeSet<BTreeSet<String>> = BTreeSet::new();
    let mut re_resolved = 0usize;
    // Half of `max_cases` at most, so the headline denominator is not starved.
    let re_quota = src.max_cases / 2;
    let considered = scan(
        &repo,
        src,
        &oracle as &dyn Oracle,
        Some(src.max_cases),
        &mut |c| {
            if c.kind == Kind::ReResolved && re_resolved >= re_quota {
                return Selection::Skip("re-resolved-quota");
            }
            if !seen.insert(c.change_set.clone()) {
                return Selection::Skip("duplicate-change-set");
            }
            if c.kind == Kind::ReResolved {
                re_resolved += 1;
            }
            Selection::Take
        },
    )?;

    let repo_dir = opts.out.join(&src.ecosystem).join(&src.name);
    let mut taken = 0;
    let mut selected: BTreeSet<String> = BTreeSet::new();
    let mut per_merge: std::collections::BTreeMap<&str, usize> = Default::default();
    for c in &considered {
        let Outcome::Case(cand) = &c.outcome else {
            continue;
        };
        let n = per_merge.entry(c.merge.as_str()).or_default();
        *n += 1;
        let short = &c.merge[..12.min(c.merge.len())];
        let name = if *n == 1 {
            short.to_string()
        } else {
            format!("{short}-{n}")
        };
        selected.insert(name.clone());
        // Rewrite from scratch, so no file of an earlier run survives.
        let _ = fs::remove_dir_all(repo_dir.join(&name));
        write_case(
            &repo,
            src,
            cand,
            &license_expr,
            &repo_dir.join(name),
            &opts.date,
        )?;
        taken += 1;
    }
    // Reconcile: case directories of this source that are not in the selected
    // set are from an earlier run and no longer cases. Other sources live in
    // other directories and are untouched.
    if let Ok(entries) = fs::read_dir(&repo_dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() && !selected.contains(&name) {
                fs::remove_dir_all(e.path()).map_err(|e| e.to_string())?;
            }
        }
    }
    for (name, bytes) in &license_files {
        write(&repo_dir.join(name), bytes)?;
    }
    write_index(
        &opts.out.join(&src.ecosystem).join("INDEX.toml"),
        src,
        &considered,
    )?;
    Ok(format!(
        "{}: {} considered, {taken} extracted ({license_expr})",
        src.name,
        considered.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn license_detection_reads_the_texts() {
        let mit = "MIT License\n\nPermission is hereby granted, free of charge, to any person obtaining a copy\nof this software";
        let apache = "Apache License\nVersion 2.0, January 2004";
        assert_eq!(detect(mit), BTreeSet::from(["MIT"]));
        assert_eq!(detect(apache), BTreeSet::from(["Apache-2.0"]));
        assert!(detect("GNU GENERAL PUBLIC LICENSE Version 3").is_empty());
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(q("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }
}
