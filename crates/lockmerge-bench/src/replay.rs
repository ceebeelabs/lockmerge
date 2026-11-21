//! The replay (docs/02-oracle.md section 3.3): each case is rebuilt as a
//! throwaway git repository and merged with real `git merge`, without the
//! driver and with it, and again with the sides swapped.
//!
//! The result is read from git, never from the driver: an unmerged lockfile
//! is a refusal, a failed merge is an error, a clean merge gives the blob.

use crate::case::Case;
use crate::classify::{GitRun, Run};
use crate::driver;
use crate::util::{RunError, TempDir, git_isolated, run};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Per-merge limit (section 3.3).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const SETUP_TIMEOUT: Duration = Duration::from_secs(60);

pub struct ReplayOpts {
    /// The directory holding the `lockmerge` binary. The driver command is
    /// the bare name, found through `PATH`, as `lockmerge install` writes it.
    pub driver_dir: Option<PathBuf>,
    pub timeout: Duration,
}

pub struct Replayed {
    pub git: GitRun,
    pub lm: Run,
    pub lm_swapped: Run,
    pub millis: u128,
}

/// One merge, as git reports it.
enum Merged {
    Conflict,
    Clean(Vec<u8>),
    Error(String),
}

fn git(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut cmd = git_isolated(dir);
    cmd.args(args);
    match run(cmd, SETUP_TIMEOUT) {
        Ok(r) if r.ok() => Ok(r.stdout),
        Ok(r) => Err(format!("git {}: {}", args.join(" "), r.stderr_text())),
        Err(RunError::Timeout) => Err(format!("git {}: timeout", args.join(" "))),
        Err(RunError::Spawn(e)) => Err(format!("git {}: {e}", args.join(" "))),
    }
}

fn safe_rel(rel: &str) -> Result<(), String> {
    let p = Path::new(rel);
    let ok = !rel.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_)));
    ok.then_some(())
        .ok_or_else(|| format!("unsafe path `{rel}` in a fixture"))
}

fn write_file(root: &Path, rel: &str, bytes: &[u8]) -> Result<(), String> {
    safe_rel(rel)?;
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, bytes).map_err(|e| e.to_string())
}

/// Replaces the working tree with one side: its lockfile and its manifests.
fn write_side(dir: &Path, case: &Case, side: &str, lock: &[u8]) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.file_name().is_some_and(|n| n == ".git") {
            continue;
        }
        if path.is_dir() {
            fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
        } else {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    }
    write_file(dir, &case.meta.lockfile_path, lock)?;
    for m in case.manifests.iter().filter(|m| m.side == side) {
        write_file(dir, &m.rel, &m.bytes)?;
    }
    Ok(())
}

fn commit(dir: &Path, message: &str) -> Result<String, String> {
    git(dir, &["add", "-A", "-f"])?;
    git(dir, &["commit", "-q", "--allow-empty", "-m", message])?;
    let out = git(dir, &["rev-parse", "HEAD"])?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

/// Builds the repository of section 3.3 and returns the tips of `ours` and
/// `theirs`.
fn build_repo(dir: &Path, case: &Case) -> Result<(String, String), String> {
    git(dir, &["init", "-q", "-b", "base"])?;
    for kv in [
        ("core.autocrlf", "false"),
        ("core.safecrlf", "false"),
        ("core.eol", "lf"),
        ("commit.gpgsign", "false"),
        ("gc.auto", "0"),
        ("core.fsmonitor", "false"),
        ("merge.renames", "false"),
    ] {
        git(dir, &["config", kv.0, kv.1])?;
    }
    write_side(dir, case, "base", &case.base)?;
    commit(dir, "base")?;
    git(dir, &["checkout", "-q", "-b", "ours"])?;
    write_side(dir, case, "ours", &case.ours)?;
    let ours = commit(dir, "ours")?;
    git(dir, &["checkout", "-q", "base"])?;
    git(dir, &["checkout", "-q", "-b", "theirs"])?;
    write_side(dir, case, "theirs", &case.theirs)?;
    let theirs = commit(dir, "theirs")?;
    Ok((ours, theirs))
}

/// Configures the driver exactly as `lockmerge install` would.
fn configure_driver(dir: &Path, case: &Case) -> Result<(), String> {
    git(dir, &["config", "merge.lockmerge.name", driver::NAME_VALUE])?;
    git(dir, &["config", "merge.lockmerge.driver", driver::COMMAND])?;
    let file_name = Path::new(&case.meta.lockfile_path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("lockfile_path has no file name")?;
    let info = dir.join(".git").join("info");
    fs::create_dir_all(&info).map_err(|e| e.to_string())?;
    fs::write(info.join("attributes"), driver::attributes_line(file_name))
        .map_err(|e| e.to_string())
}

/// Merges `other` into `branch` (reset to `tip`) with real `git merge`.
fn merge(
    dir: &Path,
    case: &Case,
    opts: &ReplayOpts,
    (branch, tip): (&str, &str),
    (other, other_tip): (&str, &str),
    with_driver: bool,
) -> Merged {
    let prepared = (|| -> Result<(), String> {
        let _ = git(dir, &["merge", "--abort"]);
        git(dir, &["checkout", "-q", "-f", "-B", branch, tip])?;
        // An earlier clean merge moved the other branch; put it back so this
        // is a real merge and not a fast-forward.
        git(dir, &["branch", "-q", "-f", other, other_tip])?;
        git(dir, &["clean", "-fdxq"])?;
        Ok(())
    })();
    if let Err(e) = prepared {
        return Merged::Error(e);
    }
    let mut cmd: Command = git_isolated(dir);
    cmd.args(["merge", "--no-ff", "--no-edit", other]);
    if with_driver && let Some(d) = &opts.driver_dir {
        let mut paths = vec![d.clone()];
        if let Some(old) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&old));
        }
        match std::env::join_paths(paths) {
            Ok(p) => {
                cmd.env("PATH", p);
            }
            Err(e) => return Merged::Error(format!("PATH: {e}")),
        }
    }
    let ran = match run(cmd, opts.timeout) {
        Ok(r) => r,
        Err(RunError::Timeout) => return Merged::Error("timeout".into()),
        Err(RunError::Spawn(e)) => return Merged::Error(format!("git merge: {e}")),
    };
    match ran.code {
        Some(0) => {
            let spec = format!("HEAD:{}", case.meta.lockfile_path);
            match git(dir, &["cat-file", "blob", &spec]) {
                Ok(bytes) => Merged::Clean(bytes),
                Err(e) => Merged::Error(e),
            }
        }
        // Exit 1 is a conflict somewhere in the tree. Only the lockfile's own
        // index state says whether the lockfile was merged.
        Some(1) => match git(dir, &["ls-files", "-s", "--", &case.meta.lockfile_path]) {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out).into_owned();
                let stages: Vec<&str> = text
                    .lines()
                    .filter_map(|l| l.split_once('\t')?.0.split(' ').nth(2))
                    .collect();
                if stages.is_empty() {
                    Merged::Error(format!(
                        "git merge failed and the lockfile is not in the index: {}",
                        ran.stderr_text()
                    ))
                } else if stages.iter().all(|s| *s == "0") {
                    let spec = format!(":0:{}", case.meta.lockfile_path);
                    match git(dir, &["cat-file", "blob", &spec]) {
                        Ok(bytes) => Merged::Clean(bytes),
                        Err(e) => Merged::Error(e),
                    }
                } else {
                    Merged::Conflict
                }
            }
            Err(e) => Merged::Error(e),
        },
        Some(c) => Merged::Error(format!("git merge exited {c}: {}", ran.stderr_text())),
        None => Merged::Error(format!("git merge was killed: {}", ran.stderr_text())),
    }
}

/// Runs the plain merge, the driver merge and the swapped driver merge.
pub fn replay(case: &Case, opts: &ReplayOpts) -> Result<Replayed, String> {
    let started = Instant::now();
    let tmp = TempDir::new("replay").map_err(|e| e.to_string())?;
    let dir = tmp.path();
    let (ours, theirs) = build_repo(dir, case)?;

    let plain = match merge(dir, case, opts, ("ours", &ours), ("theirs", &theirs), false) {
        Merged::Conflict => GitRun::Conflict,
        Merged::Clean(b) => GitRun::Clean(b),
        Merged::Error(e) => GitRun::Error(e),
    };

    configure_driver(dir, case)?;
    let as_run = |m: Merged| match m {
        Merged::Conflict => Run::Refused,
        Merged::Clean(b) => Run::Clean(b),
        Merged::Error(e) => Run::Error(e),
    };
    let lm = as_run(merge(
        dir,
        case,
        opts,
        ("ours", &ours),
        ("theirs", &theirs),
        true,
    ));
    let lm_swapped = as_run(merge(
        dir,
        case,
        opts,
        ("theirs", &theirs),
        ("ours", &ours),
        true,
    ));
    Ok(Replayed {
        git: plain,
        lm,
        lm_swapped,
        millis: started.elapsed().as_millis(),
    })
}
