//! `lockmerge-bench extract` (docs/02-oracle.md section 6.1): saves each
//! selected merge as raw bytes, with its manifests, license and `case.toml`.
//!
//! Run by hand, by the owner side, with network access. Never a package
//! manager: every byte is `git cat-file blob`.

use crate::case::license_allowed;
use crate::classify::Kind;
use crate::oracle::{CargoOracle, Oracle, path_package_names, registry_packages};
use crate::scan::{
    Candidate, Considered, Outcome, PlainGit, Selection, Source, ensure_clone, git_out, scan,
};
use crate::util::sha256_hex;
use lockmerge_cargo::index::parse_entry;
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

/// The path of a crate's entry under an index directory's `.cache/`
/// (docs/formats/cargo-registry-index.md section 2), or `None` for a name the
/// document does not pin: only ASCII lowercase letters, digits, `-` and `_`.
fn cache_path(name: &str) -> Option<String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return None;
    }
    Some(match name.len() {
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", &name[..1]),
        _ => format!("{}/{}/{name}", &name[..2], &name[2..4]),
    })
}

/// How a saved index file was reduced from the original cache file: the
/// SHA-256 of the original bytes and the versions whose records were kept.
struct Reduction {
    /// `<index directory>/.cache/<path>`, the key of the file in `[sha256]`
    /// (without the leading `index/`).
    rel: String,
    original_sha256: String,
    versions: Vec<String>,
}

/// Reduces one cache file to its header and the records of `wanted` versions
/// (docs/formats/cargo-registry-index.md section 3). The header bytes, up to
/// and including the NUL that ends the index version string, are kept as they
/// are, and so is each kept record, byte for byte, in its original order. A
/// record is never added or edited. Returns the new bytes and the versions
/// kept, or `None` if the file does not have exactly the pinned layout or no
/// record is kept: such a file is not saved, and R15 refuses its packages.
fn trim_cache_file(bytes: &[u8], wanted: &BTreeSet<String>) -> Option<(Vec<u8>, Vec<String>)> {
    const MAGIC: &[u8] = b"\x03\x02\0\0\0";
    // The reader R15 uses must accept the original, so a file it would refuse
    // is not reduced into one it might accept.
    parse_entry(bytes).ok()?;
    let rest = bytes.strip_prefix(MAGIC)?;
    let nul = rest.iter().position(|b| *b == 0)?;
    let header_len = MAGIC.len() + nul + 1;
    let mut out = bytes[..header_len].to_vec();
    let mut kept: Vec<String> = Vec::new();
    let mut pos = header_len;
    while pos < bytes.len() {
        let v_end = pos + bytes[pos..].iter().position(|b| *b == 0)?;
        let j_end = v_end + 1 + bytes[v_end + 1..].iter().position(|b| *b == 0)?;
        // The version text and the JSON must be UTF-8 (section 3).
        let vers = std::str::from_utf8(&bytes[pos..v_end]).ok()?;
        std::str::from_utf8(&bytes[v_end + 1..j_end]).ok()?;
        if wanted.contains(vers) {
            out.extend_from_slice(&bytes[pos..=j_end]);
            if !kept.iter().any(|k| k == vers) {
                kept.push(vers.to_string());
            }
        }
        pos = j_end + 1;
    }
    if kept.is_empty() {
        return None;
    }
    Some((out, kept))
}

/// The registry index cache files the native-links proof (R15) needs for the
/// registry packages of base, ours and theirs, read from
/// `<cargo_home>/registry/index/<index directory>/.cache/<path>` and reduced by
/// `trim_cache_file` to the exact versions of those lockfiles. Returns
/// `index/<index directory>/.cache/<path>` and the reduced bytes, one
/// `Reduction` per file, and how many package names had no saved file in any
/// index directory (R15 refuses those). A name the document does not pin, a
/// file that does not have the pinned layout, and a file with no record of a
/// lockfile version are not saved and count as missing.
fn index_files(
    cargo_home: &Path,
    locks: &[&[u8]],
) -> Result<(Vec<NamedFile>, Vec<Reduction>, usize), String> {
    let mut versions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for lock in locks {
        for (name, version) in registry_packages(lock)? {
            versions.entry(name).or_default().insert(version);
        }
    }
    let root = cargo_home.join("registry/index");
    let mut dirs: Vec<String> = match fs::read_dir(&root) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    };
    dirs.sort();
    let mut files = Vec::new();
    let mut reductions = Vec::new();
    let mut missing = 0;
    for (name, wanted) in versions {
        let Some(path) = cache_path(&name) else {
            missing += 1;
            continue;
        };
        let before = files.len();
        for d in &dirs {
            let file = root.join(d).join(".cache").join(&path);
            if file.is_file() {
                let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
                if let Some((trimmed, kept)) = trim_cache_file(&bytes, &wanted) {
                    let rel = format!("{d}/.cache/{path}");
                    reductions.push(Reduction {
                        rel: rel.clone(),
                        original_sha256: sha256_hex(&bytes),
                        versions: kept,
                    });
                    files.push((format!("index/{rel}"), trimmed));
                }
            }
        }
        if files.len() == before {
            missing += 1;
        }
    }
    Ok((files, reductions, missing))
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
    reductions: &[Reduction],
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
    if let Some(n) = c.pull_request {
        // A squash-merged pull request: `merge_commit` is the squash commit,
        // `parents` its parent (ours) and the pull request head (theirs).
        s.push_str(&format!("pull_request = {n}\n"));
    }
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
    // Each saved index file is a reduction of the owner's cache file: the
    // SHA-256 of the original and the versions kept (section 6.2).
    for r in reductions {
        let versions: Vec<String> = r.versions.iter().map(|v| q(v)).collect();
        s.push_str(&format!(
            "\n[[index_reduction]]\nfile = {}\noriginal_sha256 = {}\nversions = [{}]\n",
            q(&format!("index/{}", r.rel)),
            q(&r.original_sha256),
            versions.join(", ")
        ));
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
    cargo_home: Option<&Path>,
) -> Result<usize, String> {
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
    let mut missing = 0;
    let mut reductions = Vec::new();
    if let Some(home) = cargo_home {
        let (index, red, m) = index_files(home, &[&c.base_bytes, &c.ours, &c.theirs])?;
        files.extend(index);
        reductions = red;
        missing = m;
    }
    let mut shas = Vec::new();
    for (rel, bytes) in &files {
        write(&dir.join(rel), bytes)?;
        shas.push((rel.clone(), sha256_hex(bytes)));
    }
    write(
        &dir.join("case.toml"),
        case_toml(src, c, license, &shas, &reductions, date).as_bytes(),
    )?;
    Ok(missing)
}

/// Merges this run's entries into `<eco>/INDEX.toml`, keeping the entries of
/// other repositories.
fn write_index(path: &Path, src: &Source, considered: &[Considered]) -> Result<(), String> {
    // repository, merge, lockfile path, result, plain git (empty if unknown)
    let mut entries: Vec<(String, String, String, String, String)> = Vec::new();
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
                        s("plain_git"),
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
        let plain = c
            .plain_git
            .map(|g| g.name().to_string())
            .unwrap_or_default();
        entries.push((
            src.url.clone(),
            c.merge.clone(),
            c.path.clone(),
            result,
            plain,
        ));
    }
    entries.sort();
    let mut s = String::from(
        "# Every merge considered by `lockmerge-bench extract`, with the result: a case\n# or the reason it was skipped (docs/02-oracle.md section 3.1), and what plain\n# git does with the lockfile where that was checked.\n",
    );
    for (repo, merge, lock, result, plain) in entries {
        s.push_str(&format!(
            "\n[[merge]]\nrepository = {}\nmerge_commit = {}\nlockfile_path = {}\nresult = {}\n",
            q(&repo),
            q(&merge),
            q(&lock),
            q(&result)
        ));
        if !plain.is_empty() {
            s.push_str(&format!("plain_git = {}\n", q(&plain)));
        }
    }
    write(path, s.as_bytes())
}

/// Picks at most `max_cases` of the candidates (docs/02-oracle.md section
/// 6.1) and turns the others into skips with a reason. Plain-git conflicts
/// come first, newest first; then clean merges, newest first, up to
/// `git_clean_percent` of `max_cases`. Re-resolved cases are at most half of
/// `max_cases`, so the headline denominator is not starved, and a candidate
/// with the same set of changed ids as one already taken is a duplicate.
pub fn choose(mut considered: Vec<Considered>, src: &Source) -> Vec<Considered> {
    let clean_cap = src.max_cases * src.git_clean_percent as usize / 100;
    let re_cap = src.max_cases / 2;
    let mut order: Vec<usize> = (0..considered.len())
        .filter(|i| matches!(considered[*i].outcome, Outcome::Case(_)))
        .collect();
    // Stable: newest first within each group.
    order.sort_by_key(|i| considered[*i].plain_git == Some(PlainGit::Clean));
    let mut seen: BTreeSet<BTreeSet<String>> = BTreeSet::new();
    let (mut taken, mut clean, mut re_resolved) = (0usize, 0usize, 0usize);
    for i in order {
        let Outcome::Case(c) = &considered[i].outcome else {
            continue;
        };
        let is_clean = c.plain_git == PlainGit::Clean;
        let why = if c.kind == Kind::ReResolved && re_resolved >= re_cap {
            Some("re-resolved-quota")
        } else if is_clean && clean >= clean_cap {
            Some("git-clean-quota")
        } else if taken >= src.max_cases {
            Some("over-max-cases")
        } else if !seen.insert(c.change_set.clone()) {
            Some("duplicate-change-set")
        } else {
            None
        };
        match why {
            Some(why) => considered[i].outcome = Outcome::Skipped(format!("skipped:{why}")),
            None => {
                taken += 1;
                clean += usize::from(is_clean);
                re_resolved += usize::from(c.kind == Kind::ReResolved);
            }
        }
    }
    considered
}

pub struct ExtractOpts {
    pub out: PathBuf,
    pub cache: PathBuf,
    pub date: String,
    /// The `CARGO_HOME` whose registry index cache is copied into each case's
    /// `index/`. `None` saves no index entries.
    pub cargo_home: Option<PathBuf>,
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

    // Every merge is scanned; the choice among the candidates comes after, so
    // plain-git conflicts can be preferred over newer clean merges.
    let considered = choose(
        scan(&repo, src, &oracle as &dyn Oracle, None, &mut |_| {
            Selection::Take
        })?,
        src,
    );

    let repo_dir = opts.out.join(&src.ecosystem).join(&src.name);
    let mut taken = 0;
    let mut missing = 0;
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
        missing += write_case(
            &repo,
            src,
            cand,
            &license_expr,
            &repo_dir.join(name),
            &opts.date,
            opts.cargo_home.as_deref(),
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
    let note = if opts.cargo_home.is_some() && missing > 0 {
        format!("; {missing} registry package entries not in the index cache (R15 refuses them)")
    } else {
        String::new()
    };
    Ok(format!(
        "{}: {} considered, {taken} extracted ({license_expr}){note}",
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
    fn cache_paths_follow_the_name_length_and_refuse_unpinned_names() {
        assert_eq!(cache_path("a").as_deref(), Some("1/a"));
        assert_eq!(cache_path("az").as_deref(), Some("2/az"));
        assert_eq!(cache_path("aes").as_deref(), Some("3/a/aes"));
        assert_eq!(cache_path("libz-sys").as_deref(), Some("li/bz/libz-sys"));
        assert_eq!(cache_path("a_b-1").as_deref(), Some("a_/b-/a_b-1"));
        assert_eq!(cache_path("Serde"), None);
        assert_eq!(cache_path("a.b"), None);
        assert_eq!(cache_path(""), None);
    }

    #[test]
    fn only_registry_sources_need_index_entries() {
        let lock = "version = 3\n\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"x\"\n\n[[package]]\nname = \"b\"\nversion = \"1.0.0\"\nsource = \"git+https://example.com/b#abc\"\n\n[[package]]\nname = \"c\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"d\"\nversion = \"1.0.0\"\nsource = \"sparse+https://index.crates.io/\"\nchecksum = \"y\"\n";
        let pkgs = registry_packages(lock.as_bytes()).unwrap();
        let want = [("a", "1.0.0"), ("d", "1.0.0")].map(|(n, v)| (n.to_string(), v.to_string()));
        assert_eq!(pkgs, BTreeSet::from(want));
    }

    #[test]
    fn trimming_keeps_header_and_wanted_records_in_order_and_refuses_the_rest() {
        let file = b"\x03\x02\0\0\0etag: x\0" as &[u8];
        let recs: [(&str, &str); 4] = [
            ("1.0.0", "{a}"),
            ("2.0.0", "{b}"),
            ("1.0.0", "{c}"),
            ("3.0.0", "{d}"),
        ];
        let mut full = file.to_vec();
        for (v, j) in recs {
            full.extend_from_slice(format!("{v}\0{j}\0").as_bytes());
        }
        let want = |vs: &[&str]| vs.iter().map(|v| v.to_string()).collect::<BTreeSet<_>>();
        // Both records of a version are kept, in their original order.
        let (out, kept) = trim_cache_file(&full, &want(&["3.0.0", "1.0.0"])).unwrap();
        assert_eq!(
            out,
            [file, b"1.0.0\0{a}\0", b"1.0.0\0{c}\0", b"3.0.0\0{d}\0"].concat()
        );
        assert_eq!(kept, ["1.0.0", "3.0.0"]);
        // No record of a wanted version: nothing is saved.
        assert!(trim_cache_file(&full, &want(&["9.9.9"])).is_none());
        // A header other than the pinned one, a missing last NUL and a
        // record without JSON are not reduced.
        let mut other = full.clone();
        other[0] = 2;
        assert!(trim_cache_file(&other, &want(&["1.0.0"])).is_none());
        assert!(trim_cache_file(&full[..full.len() - 1], &want(&["1.0.0"])).is_none());
        let mut odd = full.clone();
        odd.extend_from_slice(b"4.0.0\0");
        assert!(trim_cache_file(&odd, &want(&["1.0.0"])).is_none());
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(q("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }
}
