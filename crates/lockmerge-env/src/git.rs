//! The `Git` implementation: shells out to the `git` executable, never to a
//! library (ADR 0003, 0008), and never reads the working tree.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use lockmerge_core::{ManifestResult, Manifests, Why};

use crate::labels::{Labels, is_full_sha, parse_labels};

fn why(text: impl Into<String>) -> Why {
    Why(text.into())
}

/// What git passed to the driver, as far as identification needs it.
#[derive(Clone, Copy, Debug)]
pub struct DriverArgs<'a> {
    /// `%S`
    pub base_label: &'a str,
    /// `%X`
    pub ours_label: &'a str,
    /// `%Y`
    pub theirs_label: &'a str,
    /// `%P`: the repository path of the lockfile.
    pub lockfile_path: &'a str,
    /// The bytes of `%A`, the lockfile of ours.
    pub ours_lockfile: &'a [u8],
}

/// The three commits of a merge, as full object names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identified {
    pub base: String,
    pub ours: String,
    pub theirs: String,
}

/// The `GITHEAD_<full sha>=<value>` variables of `vars`, as `(sha, value)`.
/// A value that is not UTF-8 is converted lossily, so that it still counts as
/// a variable and can only fail to match.
pub fn githead_from_env(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(String, String)> {
    vars.into_iter()
        .filter_map(|(k, v)| {
            let k = k.to_string_lossy();
            let sha = k.strip_prefix("GITHEAD_")?;
            Some((sha.to_string(), v.to_string_lossy().into_owned()))
        })
        .collect()
}

/// A path that names a file below the repository top level, and nothing
/// else: `<rev>:<path>` is relative to the working directory when it starts
/// with `./` or `../`.
fn is_repo_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\0', '\n'])
        && path.split('/').all(|c| !matches!(c, "" | "." | ".."))
}

/// Runs `git`. Cheap to build; every call starts a process.
#[derive(Clone, Debug)]
pub struct Git {
    program: OsString,
    dir: Option<PathBuf>,
}

impl Default for Git {
    fn default() -> Self {
        Self::new()
    }
}

impl Git {
    /// `git` from `PATH`, in the current directory (the driver runs at the
    /// repository top level).
    pub fn new() -> Self {
        Self {
            program: "git".into(),
            dir: None,
        }
    }

    /// Runs git in `dir` instead of the current directory.
    pub fn in_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dir = Some(dir.into());
        self
    }

    /// Uses another executable than `git`.
    pub fn with_program(mut self, program: impl Into<OsString>) -> Self {
        self.program = program.into();
        self
    }

    fn command(&self) -> Command {
        let mut c = Command::new(&self.program);
        if let Some(d) = &self.dir {
            c.current_dir(d);
        }
        // Reading must not touch the index, and must never use the network:
        // in a partial clone a missing object would be fetched lazily from
        // the promisor remote. GIT_NO_LAZY_FETCH exists since git 2.44; for
        // older git, GIT_ALLOW_PROTOCOL=none makes any fetch fail, which is
        // then Unavailable.
        c.env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_ALLOW_PROTOCOL", "none")
            .stdin(Stdio::null());
        c
    }

    fn run<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<Output, Why> {
        self.command()
            .args(args)
            .output()
            .map_err(|e| why(format!("could not run git: {e}")))
    }

    /// Standard output of a git command that must succeed.
    fn stdout<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<Vec<u8>, Why> {
        let out = self.run(args)?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            let first = args
                .first()
                .map(|a| a.as_ref().to_string_lossy().into_owned());
            Err(why(format!(
                "git {} failed ({})",
                first.unwrap_or_default(),
                out.status
            )))
        }
    }

    fn text<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<String, Why> {
        String::from_utf8(self.stdout(args)?).map_err(|_| why("git printed text that is not UTF-8"))
    }

    /// `git rev-parse --verify <rev>^{commit}`, as a full object name.
    pub fn resolve_commit(&self, rev: &str) -> Result<String, Why> {
        let sha = self.text(&["rev-parse", "--verify", &format!("{rev}^{{commit}}")])?;
        let sha = sha.trim_end();
        if is_full_sha(sha) {
            Ok(sha.to_string())
        } else {
            Err(why(format!(
                "git resolved {rev:?} to {sha:?}, not an object name"
            )))
        }
    }

    /// Resolves an abbreviated sha. An ambiguous abbreviation is refused by
    /// git; a ref that happens to look like the abbreviation is refused here,
    /// because the result must start with the abbreviation.
    pub fn resolve_abbrev(&self, abbrev: &str) -> Result<String, Why> {
        let full = self.resolve_commit(abbrev)?;
        if full.starts_with(abbrev) {
            Ok(full)
        } else {
            Err(why(format!(
                "{abbrev:?} names a ref, not an abbreviated commit"
            )))
        }
    }

    /// The blob at `<commit>:<path>`; `Ok(None)` when it does not exist.
    fn blob(&self, commit: &str, path: &str) -> Result<Option<Vec<u8>>, Why> {
        let spec = format!("{commit}:{path}");
        // Tell "no such path" from a failure, so a failure is never read as a
        // missing manifest (both are Unavailable, but the reason differs).
        let exists = self.run(&["cat-file", "-e", &spec])?;
        if !exists.status.success() {
            return match self.run(&["cat-file", "-t", commit])?.status.success() {
                true => Ok(None),
                false => Err(why(format!("commit {commit} cannot be read"))),
            };
        }
        let out = self.run(&["cat-file", "blob", &spec])?;
        if out.status.success() {
            Ok(Some(out.stdout))
        } else {
            Err(why(format!("{spec} is not a blob")))
        }
    }

    /// The first parents of `commit`: all its parents.
    fn parents(&self, commit: &str) -> Result<Vec<String>, Why> {
        let line = self.text(&["rev-list", "--parents", "-n", "1", commit])?;
        let mut it = line.split_whitespace();
        if it.next() != Some(commit) {
            return Err(why("git rev-list printed an unexpected line"));
        }
        Ok(it.map(str::to_string).collect())
    }

    /// Ours is proven, not assumed (ADR 0010 decision 2): the lockfile blob of
    /// `ours` equals `%A`, and the index has no difference from `ours`
    /// (`git cherry-pick -n` starts from the index).
    fn prove_ours(&self, ours: &str, args: &DriverArgs) -> Result<(), Why> {
        if !is_repo_path(args.lockfile_path) {
            return Err(why(format!(
                "lockfile path {:?} is not a repository path",
                args.lockfile_path
            )));
        }
        match self.blob(ours, args.lockfile_path)? {
            Some(b) if b == args.ours_lockfile => {}
            Some(_) => return Err(why("the lockfile of HEAD differs from %A")),
            None => return Err(why("the lockfile does not exist in HEAD")),
        }
        let out = self.run(&["diff", "--cached", "--quiet", "--no-ext-diff", ours, "--"])?;
        match out.status.code() {
            Some(0) => Ok(()),
            Some(1) => Err(why("the index differs from HEAD (staged changes)")),
            _ => Err(why("git diff --cached failed")),
        }
    }

    /// Identifies base, ours and theirs (ADR 0010). `githead` is the
    /// `GITHEAD_` variables, see [`githead_from_env`].
    pub fn identify(
        &self,
        args: &DriverArgs,
        githead: &[(String, String)],
    ) -> Result<Identified, Why> {
        let labels = parse_labels(args.base_label, args.ours_label, args.theirs_label, githead)?;
        let ours = self.resolve_commit("HEAD")?;
        self.prove_ours(&ours, args)?;
        match labels {
            Labels::Merge { base, theirs } => {
                let theirs_full = self.resolve_commit(&theirs)?;
                if theirs_full != theirs {
                    return Err(why("GITHEAD_ names a different object"));
                }
                let base = self.resolve_abbrev(&base)?;
                // Several merge bases mean a virtual ancestor: not identified.
                let bases = self.text(&["merge-base", "--all", &ours, &theirs_full])?;
                if bases.lines().collect::<Vec<_>>() != [base.as_str()] {
                    return Err(why(
                        "the merge does not have exactly the labeled merge base",
                    ));
                }
                Ok(Identified {
                    base,
                    ours,
                    theirs: theirs_full,
                })
            }
            Labels::Pick { theirs } => {
                let theirs = self.resolve_abbrev(&theirs)?;
                match self.parents(&theirs)?.as_slice() {
                    [base] => Ok(Identified {
                        base: base.clone(),
                        ours,
                        theirs,
                    }),
                    // A merge commit picked with -m N: the label does not
                    // name the selected parent. A root commit has no base.
                    other => Err(why(format!(
                        "the commit being applied has {} parents, not exactly one",
                        other.len()
                    ))),
                }
            }
        }
    }

    /// Manifests of one driver run, identified from what git passed. When
    /// nothing can be identified, every lookup is `Unavailable` with the
    /// reason.
    pub fn manifests(&self, args: &DriverArgs, githead: &[(String, String)]) -> GitManifests {
        GitManifests {
            git: self.clone(),
            commits: self.identify(args, githead),
        }
    }
}

/// The `Manifests` implementation over git objects (ADR 0010 decision 4).
#[derive(Clone, Debug)]
pub struct GitManifests {
    git: Git,
    commits: Result<Identified, Why>,
}

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A scratch directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> std::io::Result<Self> {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("lockmerge-{}-{n}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl GitManifests {
    /// Manifests of commits that are already identified.
    pub fn with_commits(git: Git, commits: Identified) -> Self {
        Self {
            git,
            commits: Ok(commits),
        }
    }

    /// `git merge-file -p` over the three blobs. Exit status 0 is a clean
    /// merge, 1 to 127 is that many conflicts, anything else is an error.
    fn text_merge(&self, ours: &[u8], base: &[u8], theirs: &[u8]) -> Result<ManifestResult, Why> {
        let io = |e: std::io::Error| why(format!("scratch files: {e}"));
        let dir = Scratch::new().map_err(io)?;
        let paths = ["ours", "base", "theirs"].map(|n| dir.path().join(n));
        for (p, bytes) in paths.iter().zip([ours, base, theirs]) {
            fs::write(p, bytes).map_err(io)?;
        }
        // Run outside the repository, so that no repository configuration
        // and no working tree file can matter.
        let out = Command::new(&self.git.program)
            .current_dir(dir.path())
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null())
            .args([
                "merge-file",
                "-p",
                "-L",
                "ours",
                "-L",
                "base",
                "-L",
                "theirs",
                "ours",
                "base",
                "theirs",
            ])
            .output()
            .map_err(|e| why(format!("could not run git: {e}")))?;
        Ok(match out.status.code() {
            Some(0) => ManifestResult::Merged(out.stdout),
            Some(1..=127) => ManifestResult::Conflict,
            _ => return Err(why("git merge-file failed")),
        })
    }

    fn try_merged(&self, repo_path: &str) -> Result<ManifestResult, Why> {
        let c = self.commits.as_ref().map_err(Clone::clone)?;
        if !is_repo_path(repo_path) {
            return Err(why(format!("{repo_path:?} is not a repository path")));
        }
        let mut blobs = Vec::new();
        for (side, commit) in [("ours", &c.ours), ("base", &c.base), ("theirs", &c.theirs)] {
            match self.git.blob(commit, repo_path)? {
                Some(b) => blobs.push(b),
                None => return Err(why(format!("{repo_path} is missing on {side}"))),
            }
        }
        self.text_merge(&blobs[0], &blobs[1], &blobs[2])
    }
}

impl Manifests for GitManifests {
    fn merged(&self, repo_path: &str) -> ManifestResult {
        self.try_merged(repo_path)
            .unwrap_or_else(ManifestResult::Unavailable)
    }
}
