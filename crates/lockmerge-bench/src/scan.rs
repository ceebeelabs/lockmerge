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
    /// Opt-in (`squash_prs = true`): also recover squash-merged pull requests
    /// from `refs/pull/<n>/head` as cases (docs/02-oracle.md section 3.1).
    /// Default false.
    pub squash_prs: bool,
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
        let squash_prs = match t.get("squash_prs") {
            None => false,
            Some(i) => i.as_bool().ok_or_else(|| {
                format!(
                    "sources.toml: source {} `squash_prs` must be true or false",
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
            squash_prs,
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
    if source.squash_prs {
        // The pull request heads are not part of a plain clone. The refspec
        // names them under the same refs/pull/ names. Owner side, network.
        git_out(
            dir,
            &[
                "fetch",
                "--quiet",
                &source.url,
                "+refs/pull/*/head:refs/pull/*/head",
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
    /// The pull request number when this is a squash-merged pull request:
    /// `merge` is then the squash commit, `parents` are its parent and the
    /// pull request head.
    pub pull_request: Option<u64>,
    /// How a squash commit was proven to be the pull request: `"disjoint"` or
    /// `"three-way"` (docs/02-oracle.md section 3.1). `None` for merge commits.
    pub squash_proof: Option<&'static str>,
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
        pull_request: None,
        squash_proof: None,
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
    let full = |taken: usize| max_taken.is_some_and(|m| taken >= m);
    'merges: for line in listing.lines() {
        let mut parts = line.split_whitespace();
        let Some(merge) = parts.next() else { continue };
        let parents: Vec<&str> = parts.collect();
        for path in &src.lockfiles {
            if full(taken) {
                break 'merges;
            }
            let result = candidate(repo, src, oracle, merge, &parents, path)?;
            offer(&mut out, &mut taken, select, merge, path, result);
        }
    }
    if src.squash_prs && !full(taken) {
        scan_squash(repo, src, oracle, max_taken, &mut taken, select, &mut out)?;
    }
    Ok(out)
}

/// Records the result for one merge and lockfile path.
fn offer(
    out: &mut Vec<Considered>,
    taken: &mut usize,
    select: &mut dyn FnMut(&Candidate) -> Selection,
    merge: &str,
    path: &str,
    result: Result<Candidate, &'static str>,
) {
    match result {
        Err(why) => out.push(skip(merge, path, why)),
        Ok(c) => match select(&c) {
            Selection::Skip(why) => out.push(Considered {
                plain_git: Some(c.plain_git),
                ..skip(merge, path, why)
            }),
            Selection::Take => {
                *taken += 1;
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

/// The paths that differ between two commits, without the lockfiles of the
/// source (they have cases of their own).
fn changed(repo: &Path, src: &Source, a: &str, b: &str) -> Result<BTreeSet<String>, String> {
    let raw = git_out(repo, &["diff", "--name-only", "--no-renames", "-z", a, b])?;
    Ok(raw
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .filter(|p| !src.lockfiles.contains(p))
        .collect())
}

/// The tree of a three-way merge of `ours` and `head` from `base`, if git
/// merges every path outside the lockfiles cleanly. Conflicts in the
/// lockfiles are expected and ignored. `None` when any other path conflicts.
fn merge_tree_outside_lockfiles(
    repo: &Path,
    src: &Source,
    base: &str,
    ours: &str,
    head: &str,
) -> Result<Option<String>, String> {
    let args = [
        "merge-tree",
        "--write-tree",
        "--name-only",
        "-z",
        "--merge-base",
        base,
        ours,
        head,
    ];
    let mut cmd = git_plain(repo);
    cmd.args(args);
    let r = match run(cmd, GIT_TIMEOUT) {
        Ok(r) => r,
        Err(RunError::Timeout) => return Err("git merge-tree: timeout".into()),
        Err(RunError::Spawn(e)) => return Err(format!("git merge-tree: {e}")),
    };
    // Exit 0 is a clean merge, 1 a merge with conflicts; anything else is an
    // error. The output is the tree id, then (with -z, when conflicted) the
    // conflicted paths, each ended by NUL.
    if !r.ok() && r.code != Some(1) {
        return Err(format!("git merge-tree: {}", r.stderr_text()));
    }
    let mut fields = r.stdout.split(|b| *b == 0);
    let tree = String::from_utf8_lossy(fields.next().unwrap_or_default())
        .trim()
        .to_string();
    if tree.is_empty() {
        return Err("git merge-tree: no tree in the output".into());
    }
    let is_lockfile = |p: &[u8]| {
        src.lockfiles
            .contains(&String::from_utf8_lossy(p).into_owned())
    };
    // Conflicted file entries, ended by an empty field.
    for path in fields.by_ref().take_while(|p| !p.is_empty()) {
        if !is_lockfile(path) {
            return Ok(None);
        }
    }
    // Then the messages: a path count, the paths, a type and the text. Some
    // conflicts (a directory rename split) have no conflicted file entry and
    // show only here, so every CONFLICT message must name only lockfiles. A
    // message that cannot be read, or names no path, is refused.
    let rest: Vec<&[u8]> = fields.collect();
    let mut i = 0;
    while i < rest.len() {
        if rest[i].is_empty() && i + 1 == rest.len() {
            break;
        }
        let count = std::str::from_utf8(rest[i])
            .ok()
            .and_then(|n| n.parse::<usize>().ok());
        let Some(count) = count else { return Ok(None) };
        let (paths, kind) = (rest.get(i + 1..i + 1 + count), rest.get(i + 1 + count));
        let (Some(paths), Some(kind), Some(_text)) = (paths, kind, rest.get(i + 2 + count)) else {
            return Ok(None);
        };
        if kind.starts_with(b"CONFLICT") && (count == 0 || !paths.iter().all(|p| is_lockfile(p))) {
            return Ok(None);
        }
        i += count + 3;
    }
    Ok(Some(tree))
}

/// Proves that the squash commit `squash` is the pull request applied to
/// `ours` (its parent), apart from the lockfiles. With `pr` the paths the pull
/// request changed from `base`, and `main` those the default branch changed
/// from `base`:
///
/// - If they are disjoint, the squash must change exactly the pull request's
///   paths, and each of them must equal the pull request head (`"disjoint"`).
/// - Otherwise git must merge `ours` and `head` from `base` cleanly outside the
///   lockfiles, and that tree must equal the squash tree outside the lockfiles
///   (`"three-way"`).
///
/// Returns the proof used, or the reason the squash is skipped.
fn squash_proof(
    repo: &Path,
    src: &Source,
    c: &Candidate,
) -> Result<Result<&'static str, &'static str>, String> {
    let (base, ours, head, squash) = (&c.base, &c.parents[0], &c.parents[1], &c.merge);
    let pr = changed(repo, src, base, head)?;
    let main = changed(repo, src, base, ours)?;
    if !pr.is_disjoint(&main) {
        let Some(tree) = merge_tree_outside_lockfiles(repo, src, base, ours, head)? else {
            return Ok(Err("squash-overlap-unproven"));
        };
        if !changed(repo, src, &tree, squash)?.is_empty() {
            return Ok(Err("squash-overlap-unproven"));
        }
        return Ok(Ok("three-way"));
    }
    let landed = changed(repo, src, ours, squash)?;
    let differs = changed(repo, src, squash, head)?;
    if landed != pr || !differs.is_disjoint(&pr) {
        return Ok(Err("squash-not-the-pull-request"));
    }
    Ok(Ok("disjoint"))
}

/// `Some(n)` for a subject that ends with GitHub's `(#n)`.
fn pull_number(subject: &str) -> Option<u64> {
    let inner = subject.trim_end().strip_suffix(')')?;
    let digits = &inner[inner.rfind("(#")? + 2..];
    let n: u64 = digits.parse().ok()?;
    (n.to_string() == digits).then_some(n)
}

/// Squash-merged pull requests (docs/02-oracle.md section 3.1). The squash
/// commit S of pull request n is a one-parent commit on the first-parent
/// history of the pinned revision whose subject ends with `(#n)`, for a pull
/// request whose head is in `refs/pull/<n>/head`. It must be the only such
/// commit, and `squash_proof` must show that its tree is the pull request
/// applied. Ours is S's parent, theirs the pull request head.
fn scan_squash(
    repo: &Path,
    src: &Source,
    oracle: &dyn Oracle,
    max_taken: Option<usize>,
    taken: &mut usize,
    select: &mut dyn FnMut(&Candidate) -> Selection,
    out: &mut Vec<Considered>,
) -> Result<(), String> {
    let mut heads: std::collections::BTreeMap<u64, String> = Default::default();
    let refs = text(git_out(
        repo,
        &[
            "for-each-ref",
            "--format=%(objecttype) %(objectname) %(refname)",
            "refs/pull/",
        ],
    )?);
    for line in refs.lines() {
        let mut p = line.split(' ');
        let (Some("commit"), Some(sha), Some(name)) = (p.next(), p.next(), p.next()) else {
            continue;
        };
        let n = name
            .strip_prefix("refs/pull/")
            .and_then(|r| r.strip_suffix("/head"))
            .filter(|d| d.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|d| d.parse::<u64>().ok());
        if let Some(n) = n {
            heads.insert(n, sha.to_string());
        }
    }
    let log = text(git_out(
        repo,
        &[
            "log",
            "--first-parent",
            "--format=%H%x09%P%x09%s",
            &src.revision,
        ],
    )?);
    // Newest first; the commits that name each pull request.
    let mut order: Vec<u64> = Vec::new();
    let mut named: std::collections::BTreeMap<u64, Vec<(String, String)>> = Default::default();
    for line in log.lines() {
        let mut f = line.splitn(3, '\t');
        let (Some(sha), Some(parents), Some(subject)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let parents: Vec<&str> = parents.split_whitespace().collect();
        let (Some(n), [parent]) = (pull_number(subject), parents.as_slice()) else {
            continue;
        };
        if !heads.contains_key(&n) {
            continue;
        }
        if !named.contains_key(&n) {
            order.push(n);
        }
        named
            .entry(n)
            .or_default()
            .push((sha.to_string(), parent.to_string()));
    }
    for n in order {
        let head = &heads[&n];
        let squashes = &named[&n];
        let [(squash, parent)] = squashes.as_slice() else {
            for path in &src.lockfiles {
                out.push(skip(head, path, "squash-ambiguous-commit"));
            }
            continue;
        };
        let mut proof: Option<Result<&'static str, &'static str>> = None;
        for path in &src.lockfiles {
            if max_taken.is_some_and(|m| *taken >= m) {
                return Ok(());
            }
            let parents = [parent.as_str(), head.as_str()];
            match candidate(repo, src, oracle, squash, &parents, path)? {
                Err(why) => out.push(skip(squash, path, why)),
                Ok(mut c) => {
                    let result = match proof {
                        Some(r) => r,
                        None => *proof.insert(squash_proof(repo, src, &c)?),
                    };
                    match result {
                        Err(why) => out.push(Considered {
                            plain_git: Some(c.plain_git),
                            ..skip(squash, path, why)
                        }),
                        Ok(used) => {
                            c.pull_request = Some(n);
                            c.squash_proof = Some(used);
                            offer(out, taken, select, squash, path, Ok(c));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// `survey`: counts the candidates of one source without writing fixtures.
pub fn survey(repo: &Path, src: &Source, oracle: &dyn Oracle) -> Result<String, String> {
    let all = scan(repo, src, oracle, None, &mut |_| Selection::Take)?;
    let (mut reachable, mut re_resolved, mut conflicts) = (0, 0, 0);
    let mut skips: std::collections::BTreeMap<String, usize> = Default::default();
    let (mut disjoint, mut three_way) = (0, 0);
    for c in &all {
        match &c.outcome {
            Outcome::Case(b) => {
                match b.kind {
                    Kind::Reachable => reachable += 1,
                    Kind::ReResolved => re_resolved += 1,
                }
                match b.squash_proof {
                    Some("disjoint") => disjoint += 1,
                    Some(_) => three_way += 1,
                    None => {}
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
    if disjoint + three_way > 0 {
        s.push_str(&format!(
            "; squash proofs disjoint {disjoint}, three-way {three_way}"
        ));
    }
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
            squash_prs: false,
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

    #[test]
    fn squash_prs_is_opt_in_and_a_boolean() {
        let ok = "[[source]]\nurl = \"https://github.com/sharkdp/bat\"\necosystem = \"cargo\"\nlockfiles = [\"Cargo.lock\"]\nrevision = \"0123456789012345678901234567890123456789\"\nmax_cases = 8\nmax_file_bytes = 250000\n";
        assert!(!parse_sources(ok).unwrap()[0].squash_prs);
        let with = |v: &str| format!("{ok}squash_prs = {v}\n");
        assert!(parse_sources(&with("true")).unwrap()[0].squash_prs);
        assert!(!parse_sources(&with("false")).unwrap()[0].squash_prs);
        assert!(
            parse_sources(&with("\"yes\""))
                .unwrap_err()
                .contains("squash_prs")
        );
    }

    #[test]
    fn pull_numbers_come_only_from_a_trailing_reference() {
        assert_eq!(pull_number("Fix it (#12)"), Some(12));
        assert_eq!(pull_number("Fix it (#12) "), Some(12));
        for no in [
            "Fix it",
            "Fix #12",
            "Fix (#12) now",
            "Fix (#012)",
            "Fix (#)",
            "Revert \"Fix (#12)\"",
            "Fix (#1a)",
        ] {
            assert_eq!(pull_number(no), None, "{no}");
        }
    }
}
