//! Finding the merges that are cases (docs/02-oracle.md section 3.1), for
//! `survey` and `extract`. Both shell out to the `git` binary on a bare clone
//! and never use a package manager. They run by hand on the owner side, with
//! network access, never against real repositories from a worker.

use crate::classify::{Kind, foreign_ids, kind_of};
use crate::oracle::Oracle;
use crate::util::{RunError, git_plain, run};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;
use toml_edit::DocumentMut;

const GIT_TIMEOUT: Duration = Duration::from_secs(600);

/// The default for `Source::git_clean_percent`.
pub const DEFAULT_GIT_CLEAN_PERCENT: u64 = 25;

/// One entry of `sources.toml`.
#[derive(Clone, Debug)]
pub struct Source {
    pub name: String,
    pub url: String,
    pub ecosystem: String,
    pub lockfiles: Vec<String>,
    /// The pinned tip, a full commit SHA.
    pub revision: String,
    pub max_cases: usize,
    pub max_file_bytes: u64,
    /// The most `max_cases` that may be cases where plain git merges the
    /// lockfile cleanly, in percent (docs/02-oracle.md section 6.1). The rest
    /// is filled with plain-git conflicts first. Default 25.
    pub git_clean_percent: u64,
    /// The SPDX expression the owner states; checked against the license
    /// files at the pinned commit.
    pub license: Option<String>,
}

fn slug(url: &str) -> String {
    let trimmed = url.trim_end_matches('/').trim_end_matches(".git");
    let parts: Vec<&str> = trimmed.rsplit(['/', ':']).take(2).collect();
    let name: Vec<&str> = parts.into_iter().rev().collect();
    name.join("-")
}

pub fn parse_sources(text: &str) -> Result<Vec<Source>, String> {
    let doc: DocumentMut = text
        .parse()
        .map_err(|e| format!("sources.toml does not parse: {e}"))?;
    let list = doc
        .get("source")
        .and_then(|i| i.as_array_of_tables())
        .ok_or("sources.toml needs `[[source]]` tables")?;
    let mut out = Vec::new();
    for (n, t) in list.iter().enumerate() {
        let s = |k: &str| t.get(k).and_then(|i| i.as_str()).map(str::to_string);
        let ctx = |k: &str| format!("sources.toml: source {} needs `{k}`", n + 1);
        let url = s("url").ok_or_else(|| ctx("url"))?;
        let revision = s("revision").ok_or_else(|| ctx("revision"))?;
        if revision.len() != 40 || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!(
                "sources.toml: source {} `revision` must be a full 40-character commit SHA",
                n + 1
            ));
        }
        let lockfiles: Vec<String> = t
            .get("lockfiles")
            .and_then(|i| i.as_value())
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if lockfiles.is_empty() {
            return Err(ctx("lockfiles"));
        }
        let int = |k: &str| {
            t.get(k)
                .and_then(|i| i.as_integer())
                .and_then(|v| u64::try_from(v).ok())
                .ok_or_else(|| ctx(k))
        };
        let git_clean_percent = match t.get("git_clean_percent") {
            None => DEFAULT_GIT_CLEAN_PERCENT,
            Some(_) => int("git_clean_percent").ok().filter(|p| *p <= 100).ok_or_else(|| {
                format!(
                    "sources.toml: source {} `git_clean_percent` must be an integer from 0 to 100",
                    n + 1
                )
            })?,
        };
        out.push(Source {
            name: s("name").unwrap_or_else(|| slug(&url)),
            ecosystem: s("ecosystem").ok_or_else(|| ctx("ecosystem"))?,
            lockfiles,
            revision,
            max_cases: int("max_cases")? as usize,
            max_file_bytes: int("max_file_bytes")?,
            git_clean_percent,
            license: s("license"),
            url,
        });
    }
    Ok(out)
}

/// Runs `git` in `dir` (a bare clone) and returns stdout on success.
pub fn git_out(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut cmd = git_plain(dir);
    cmd.args(args);
    match run(cmd, GIT_TIMEOUT) {
        Ok(r) if r.ok() => Ok(r.stdout),
        Ok(r) => Err(format!("git {}: {}", args.join(" "), r.stderr_text())),
        Err(RunError::Timeout) => Err(format!("git {}: timeout", args.join(" "))),
        Err(RunError::Spawn(e)) => Err(format!("git {}: {e}", args.join(" "))),
    }
}

/// Like [`git_out`], but exit code 1 is `Ok(None)` (`rev-parse --verify -q`
/// for a missing path, `merge-base` for no common ancestor, `cat-file -e`).
fn git_try(dir: &Path, args: &[&str]) -> Result<Option<Vec<u8>>, String> {
    let mut cmd = git_plain(dir);
    cmd.args(args);
    match run(cmd, GIT_TIMEOUT) {
        Ok(r) if r.ok() => Ok(Some(r.stdout)),
        Ok(r) if r.code == Some(1) => Ok(None),
        Ok(r) => Err(format!("git {}: {}", args.join(" "), r.stderr_text())),
        Err(RunError::Timeout) => Err(format!("git {}: timeout", args.join(" "))),
        Err(RunError::Spawn(e)) => Err(format!("git {}: {e}", args.join(" "))),
    }
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).trim().to_string()
}

/// Clones `source.url` bare into `dir` unless it is already there, and checks
/// that the pinned revision exists in it.
pub fn ensure_clone(source: &Source, dir: &Path) -> Result<(), String> {
    if !dir.join("HEAD").exists() {
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let here = dir.parent().unwrap_or(Path::new("."));
        // The subprocess runs in `here`, so pass only the final component.
        let target = dir
            .file_name()
            .ok_or("the clone path has no final component")?
            .to_string_lossy()
            .into_owned();
        git_out(
            here,
            &[
                "clone",
                "--bare",
                "--filter=blob:none",
                &source.url,
                &target,
            ],
        )?;
    }
    let spec = format!("{}^{{commit}}", source.revision);
    git_try(dir, &["cat-file", "-e", &spec])?
        .map(|_| ())
        .ok_or_else(|| {
            format!(
                "{}: revision {} is not in the clone",
                source.name, source.revision
            )
        })
}

/// What plain `git merge` does with the lockfile of a case, with no driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlainGit {
    Conflict,
    Clean,
}

impl PlainGit {
    pub fn name(self) -> &'static str {
        match self {
            PlainGit::Conflict => "conflict",
            PlainGit::Clean => "clean",
        }
    }
}

/// Runs git's three-way file merge on the three blobs, the content merge a
/// plain `git merge` of this path does (the replay in `check` merges the
/// fixture alone, so the repository's own attributes do not apply). `None`
/// when git fails rather than reports conflicts.
fn plain_git(base: &[u8], ours: &[u8], theirs: &[u8]) -> Option<PlainGit> {
    let tmp = crate::util::TempDir::new("plain").ok()?;
    for (name, bytes) in [("ours", ours), ("base", base), ("theirs", theirs)] {
        let p = tmp.path().join(name);
        std::fs::write(&p, bytes).ok()?;
    }
    let mut cmd = git_plain(tmp.path());
    cmd.args(["merge-file", "-p", "ours", "base", "theirs"]);
    match run(cmd, GIT_TIMEOUT) {
        Ok(r) => match r.code {
            Some(0) => Some(PlainGit::Clean),
            // The exit code is the number of conflicts, capped at 127.
            Some(n) if (1..=127).contains(&n) => Some(PlainGit::Conflict),
            _ => None,
        },
        Err(_) => None,
    }
}

/// A merge that is a case.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub merge: String,
    pub parents: [String; 2],
    pub base: String,
    pub path: String,
    pub base_bytes: Vec<u8>,
    pub ours: Vec<u8>,
    pub theirs: Vec<u8>,
    pub expected: Vec<u8>,
    pub kind: Kind,
    pub foreign: Vec<String>,
    pub plain_git: PlainGit,
    /// Ids that changed between base and either side, to diversify cases.
    pub change_set: BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub enum Outcome {
    Skipped(String),
    Case(Box<Candidate>),
}

#[derive(Clone, Debug)]
pub struct Considered {
    pub merge: String,
    pub path: String,
    pub outcome: Outcome,
    /// What plain git does, known once the merge is a candidate (also when it
    /// is skipped afterwards, say as a duplicate).
    pub plain_git: Option<PlainGit>,
}

/// What the caller says about a candidate.
pub enum Selection {
    Take,
    Skip(&'static str),
}

fn skip(merge: &str, path: &str, why: &str) -> Considered {
    Considered {
        merge: merge.to_string(),
        path: path.to_string(),
        outcome: Outcome::Skipped(format!("skipped:{why}")),
        plain_git: None,
    }
}

fn blob_of(repo: &Path, commit: &str, path: &str) -> Result<Option<String>, String> {
    let spec = format!("{commit}:{path}");
    Ok(git_try(repo, &["rev-parse", "--verify", "-q", &spec])?.map(text))
}

fn candidate(
    repo: &Path,
    src: &Source,
    oracle: &dyn Oracle,
    merge: &str,
    parents: &[&str],
    path: &str,
) -> Result<Result<Candidate, &'static str>, String> {
    if parents.len() != 2 {
        return Ok(Err("octopus"));
    }
    let bases =
        text(git_try(repo, &["merge-base", "--all", parents[0], parents[1]])?.unwrap_or_default());
    let bases: Vec<&str> = bases.lines().collect();
    let base = match bases.as_slice() {
        [] => return Ok(Err("no-base")),
        [one] => *one,
        _ => return Ok(Err("multi-base")),
    };
    let mut blobs = Vec::new();
    for c in [base, parents[0], parents[1], merge] {
        match blob_of(repo, c, path)? {
            Some(b) => blobs.push(b),
            None => return Ok(Err("not-modify-modify")),
        }
    }
    if blobs[0] == blobs[1] || blobs[0] == blobs[2] {
        return Ok(Err("not-modify-modify"));
    }
    for b in &blobs {
        let size: u64 = text(git_out(repo, &["cat-file", "-s", b])?)
            .parse()
            .map_err(|_| "unreadable blob size".to_string())?;
        if size > src.max_file_bytes {
            return Ok(Err("too-large"));
        }
    }
    let mut bytes = Vec::new();
    for b in &blobs {
        bytes.push(git_out(repo, &["cat-file", "blob", b])?);
    }
    let [base_bytes, ours, theirs, expected]: [Vec<u8>; 4] = bytes
        .try_into()
        .map_err(|_| "internal: four blobs".to_string())?;
    if [&base_bytes, &ours, &theirs]
        .iter()
        .any(|b| oracle.ids(b).is_err())
    {
        return Ok(Err("side-unparseable"));
    }
    if oracle.ids(&expected).is_err() || oracle.valid(&expected).is_err() {
        return Ok(Err("expected-invalid"));
    }
    let Some(plain) = plain_git(&base_bytes, &ours, &theirs) else {
        return Ok(Err("plain-git-error"));
    };
    let foreign = foreign_ids(oracle, &base_bytes, &ours, &theirs, &expected)?;
    let ids = |b: &[u8]| oracle.ids(b).unwrap_or_default();
    let (ib, io, it) = (ids(&base_bytes), ids(&ours), ids(&theirs));
    let change_set: BTreeSet<String> = ib
        .symmetric_difference(&io)
        .chain(ib.symmetric_difference(&it))
        .cloned()
        .collect();
    Ok(Ok(Candidate {
        merge: merge.to_string(),
        parents: [parents[0].to_string(), parents[1].to_string()],
        base: base.to_string(),
        path: path.to_string(),
        base_bytes,
        ours,
        theirs,
        expected,
        kind: kind_of(&foreign),
        foreign,
        plain_git: plain,
        change_set,
    }))
}

/// Walks the merge commits reachable from the pinned revision, newest first,
/// and lists every one considered with its result. `select` decides whether a
/// candidate is taken; the walk stops once `max_taken` are (or never, for
/// `None`, as `survey` does).
pub fn scan(
    repo: &Path,
    src: &Source,
    oracle: &dyn Oracle,
    max_taken: Option<usize>,
    select: &mut dyn FnMut(&Candidate) -> Selection,
) -> Result<Vec<Considered>, String> {
    let listing = text(git_out(
        repo,
        &["rev-list", "--merges", "--parents", &src.revision],
    )?);
    let mut out = Vec::new();
    let mut taken = 0usize;
    'merges: for line in listing.lines() {
        let mut parts = line.split_whitespace();
        let Some(merge) = parts.next() else { continue };
        let parents: Vec<&str> = parts.collect();
        for path in &src.lockfiles {
            if max_taken.is_some_and(|m| taken >= m) {
                break 'merges;
            }
            match candidate(repo, src, oracle, merge, &parents, path)? {
                Err(why) => out.push(skip(merge, path, why)),
                Ok(c) => match select(&c) {
                    Selection::Skip(why) => out.push(Considered {
                        plain_git: Some(c.plain_git),
                        ..skip(merge, path, why)
                    }),
                    Selection::Take => {
                        taken += 1;
                        out.push(Considered {
                            merge: merge.to_string(),
                            path: path.to_string(),
                            plain_git: Some(c.plain_git),
                            outcome: Outcome::Case(Box::new(c)),
                        });
                    }
                },
            }
        }
    }
    Ok(out)
}

/// `survey`: counts the candidates of one source without writing fixtures.
pub fn survey(repo: &Path, src: &Source, oracle: &dyn Oracle) -> Result<String, String> {
    let all = scan(repo, src, oracle, None, &mut |_| Selection::Take)?;
    let (mut reachable, mut re_resolved, mut conflicts) = (0, 0, 0);
    let mut skips: std::collections::BTreeMap<String, usize> = Default::default();
    for c in &all {
        match &c.outcome {
            Outcome::Case(b) => {
                match b.kind {
                    Kind::Reachable => reachable += 1,
                    Kind::ReResolved => re_resolved += 1,
                }
                if b.plain_git == PlainGit::Conflict {
                    conflicts += 1;
                }
            }
            Outcome::Skipped(why) => *skips.entry(why.clone()).or_default() += 1,
        }
    }
    let mut s = format!(
        "{}: {} merge/path pairs considered, {} candidates (reachable {reachable}, re-resolved {re_resolved}; plain git conflicts {conflicts})",
        src.name,
        all.len(),
        reachable + re_resolved
    );
    for (why, n) in skips {
        s.push_str(&format!("; {why} {n}"));
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_relative_clone_path_is_cloned_where_it_is_looked_for() {
        let src = crate::util::TempDir::new("origin").unwrap();
        let run = |d: &Path, a: &[&str]| {
            let mut c = crate::util::git_isolated(d);
            c.args(a);
            assert!(crate::util::run(c, Duration::from_secs(60)).unwrap().ok());
        };
        run(src.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(src.path().join("f"), "x").unwrap();
        run(src.path(), &["add", "f"]);
        run(src.path(), &["commit", "-q", "-m", "c"]);
        let mut c = crate::util::git_isolated(src.path());
        c.args(["rev-parse", "HEAD"]);
        let rev = String::from_utf8(crate::util::run(c, Duration::from_secs(60)).unwrap().stdout)
            .unwrap()
            .trim()
            .to_string();
        let source = Source {
            name: "t".into(),
            url: src.path().to_string_lossy().into_owned(),
            ecosystem: "cargo".into(),
            lockfiles: vec!["Cargo.lock".into()],
            revision: rev,
            max_cases: 1,
            max_file_bytes: 1,
            git_clean_percent: 25,
            license: None,
        };
        // Relative to the process directory, as the default `bench-cache/`.
        let cwd = std::env::current_dir().unwrap();
        let tmp = crate::util::TempDir::new("cache").unwrap();
        let mut rel = PathBuf::new();
        for _ in cwd.components().skip(1) {
            rel.push("..");
        }
        rel.push(tmp.path().strip_prefix("/").unwrap());
        let dir = rel.join("sub").join("t.git");
        ensure_clone(&source, &dir).unwrap();
        assert!(tmp.path().join("sub/t.git/HEAD").is_file());
        assert!(!tmp.path().join("sub/sub").exists());
    }

    #[test]
    fn sources_parse_and_refuse_a_short_revision() {
        let ok = "[[source]]\nurl = \"https://github.com/sharkdp/bat\"\necosystem = \"cargo\"\nlockfiles = [\"Cargo.lock\"]\nrevision = \"0123456789012345678901234567890123456789\"\nmax_cases = 8\nmax_file_bytes = 250000\n";
        let s = parse_sources(ok).unwrap();
        assert_eq!((s[0].name.as_str(), s[0].max_cases), ("sharkdp-bat", 8));
        let short = ok.replace("0123456789012345678901234567890123456789", "main");
        assert!(parse_sources(&short).unwrap_err().contains("40-character"));
        assert!(parse_sources(&ok.replace("max_cases = 8\n", "")).is_err());
        // The share of git-clean cases defaults to 25 and is a percentage.
        assert_eq!(s[0].git_clean_percent, 25);
        let with = |v: &str| format!("{ok}git_clean_percent = {v}\n");
        assert_eq!(parse_sources(&with("0")).unwrap()[0].git_clean_percent, 0);
        assert_eq!(
            parse_sources(&with("100")).unwrap()[0].git_clean_percent,
            100
        );
        for bad in ["101", "-1", "\"25\"", "12.5"] {
            assert!(
                parse_sources(&with(bad))
                    .unwrap_err()
                    .contains("git_clean_percent"),
                "{bad}"
            );
        }
    }
}
