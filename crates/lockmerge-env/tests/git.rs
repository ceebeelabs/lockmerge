//! Real `git merge`, `rebase`, `cherry-pick` and `revert` in temporary
//! repositories, with `env-probe` as the merge driver (ADR 0010 decision 5).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use lockmerge_env::{DriverArgs, Git, GitManifests, Identified, ManifestResult, Manifests};

static N: AtomicU64 = AtomicU64::new(0);

struct Repo {
    dir: PathBuf,
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn base_cmd(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_EDITOR", "true")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com");
    c
}

impl Repo {
    fn new() -> Self {
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("lockmerge-env-test-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let r = Repo { dir };
        r.git(&["init", "-q"]);
        r.git(&["symbolic-ref", "HEAD", "refs/heads/main"]);
        r.git(&["config", "commit.gpgsign", "false"]);
        r.write(".gitattributes", "Cargo.lock merge=lm\n");
        let exe = env!("CARGO_BIN_EXE_env-probe");
        r.git(&[
            "config",
            "merge.lm.driver",
            &format!("'{exe}' %A %S %X %Y %P"),
        ]);
        r
    }

    fn output(&self, args: &[&str]) -> Output {
        base_cmd(&self.dir)
            .env("LOCKMERGE_PROBE_LOG", self.log_path())
            .args(args)
            .output()
            .unwrap()
    }

    fn git(&self, args: &[&str]) -> String {
        let out = self.output(args);
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn log_path(&self) -> PathBuf {
        self.dir.join(".git").join("probe.log")
    }

    fn write(&self, name: &str, text: &str) {
        fs::write(self.dir.join(name), text).unwrap();
    }

    fn commit(&self, msg: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
        self.git(&["rev-parse", "HEAD"])
    }

    /// The lines the driver logged, one per driver run.
    fn log(&self) -> Vec<String> {
        fs::read_to_string(self.log_path())
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The single line the driver logged.
    fn only_log(&self) -> String {
        let log = self.log();
        assert_eq!(log.len(), 1, "driver runs: {log:?}");
        log[0].clone()
    }
}

/// A manifest of ten lines, so that edits at the top and at the bottom merge
/// cleanly as text.
fn manifest(first: &str, last: &str) -> String {
    let mut s = format!("# {first}\n");
    for i in 1..9 {
        s.push_str(&format!("line {i}\n"));
    }
    s.push_str(&format!("# {last}\n"));
    s
}

struct Fixture {
    repo: Repo,
    base: String,
    ours: String,
    theirs: String,
}

/// main: base -> ours; side: base -> theirs. The lockfile conflicts, the
/// manifest merges cleanly. HEAD is main.
fn diverged() -> Fixture {
    let repo = Repo::new();
    repo.write("Cargo.toml", &manifest("a", "z"));
    repo.write("Cargo.lock", "lock base\n");
    let base = repo.commit("base");
    repo.git(&["branch", "side"]);
    repo.write("Cargo.toml", &manifest("ours", "z"));
    repo.write("Cargo.lock", "lock ours\n");
    let ours = repo.commit("main change");
    repo.git(&["checkout", "-q", "side"]);
    repo.write("Cargo.toml", &manifest("a", "theirs"));
    repo.write("Cargo.lock", "lock theirs\n");
    let theirs = repo.commit("side change");
    repo.git(&["checkout", "-q", "main"]);
    Fixture {
        repo,
        base,
        ours,
        theirs,
    }
}

fn merged_manifest() -> String {
    format!("merged {:?}", manifest("ours", "theirs"))
}

fn identified(f: &Fixture, manifest: &str) -> String {
    format!(
        "identified base={} ours={} theirs={} manifest={manifest}",
        f.base, f.ours, f.theirs
    )
}

#[test]
fn merge_identifies_the_commits_and_merges_the_manifest() {
    let f = diverged();
    f.repo.output(&["merge", "side"]);
    assert_eq!(f.repo.only_log(), identified(&f, &merged_manifest()));
}

#[test]
fn merge_by_sha_identifies_theirs() {
    let f = diverged();
    f.repo.output(&["merge", &f.theirs]);
    assert_eq!(f.repo.only_log(), identified(&f, &merged_manifest()));
}

#[test]
fn cherry_pick_identifies_the_commits() {
    let f = diverged();
    f.repo.output(&["cherry-pick", "side"]);
    assert_eq!(f.repo.only_log(), identified(&f, &merged_manifest()));
}

#[test]
fn rebase_merge_backend_identifies_the_commits() {
    let f = diverged();
    f.repo.git(&["checkout", "-q", "side"]);
    f.repo.output(&["rebase", "main"]);
    // Rebase replays `side change` onto main: HEAD is main's tip.
    assert_eq!(f.repo.only_log(), identified(&f, &merged_manifest()));
    let explicit = diverged();
    explicit.repo.git(&["checkout", "-q", "side"]);
    explicit.repo.output(&["rebase", "-m", "main"]);
    assert_eq!(
        explicit.repo.only_log(),
        identified(&explicit, &merged_manifest())
    );
}

#[test]
fn rebase_apply_backend_is_unavailable() {
    let f = diverged();
    f.repo.git(&["checkout", "-q", "side"]);
    f.repo.output(&["rebase", "--apply", "main"]);
    let line = f.repo.only_log();
    assert!(line.contains("constructed fake ancestor"), "{line}");
    assert!(line.starts_with("unavailable "), "{line}");
}

#[test]
fn revert_is_unavailable() {
    let f = diverged();
    // Reverting `side change` on main conflicts in the lockfile.
    f.repo.output(&["revert", "--no-edit", &f.theirs]);
    let line = f.repo.only_log();
    assert!(line.contains("revert"), "{line}");
    assert!(line.starts_with("unavailable "), "{line}");
}

#[test]
fn a_manifest_that_conflicts_as_text_is_a_conflict() {
    let f = diverged();
    // Both sides edit the first manifest line differently.
    f.repo.git(&["checkout", "-q", "side"]);
    f.repo
        .write("Cargo.toml", &manifest("theirs-first", "theirs"));
    f.repo.commit("side edits the first line");
    f.repo.git(&["checkout", "-q", "main"]);
    f.repo.output(&["merge", "side"]);
    let line = f.repo.only_log();
    assert!(line.starts_with("identified "), "{line}");
    assert!(line.ends_with("manifest=conflict"), "{line}");
}

#[test]
fn a_manifest_missing_on_a_side_is_unavailable() {
    let f = diverged();
    f.repo.git(&["checkout", "-q", "side"]);
    f.repo.git(&["rm", "-q", "Cargo.toml"]);
    f.repo.commit("side removes the manifest");
    f.repo.git(&["checkout", "-q", "main"]);
    f.repo.output(&["merge", "side"]);
    let line = f.repo.only_log();
    assert!(line.contains("manifest=unavailable "), "{line}");
    assert!(line.contains("missing on theirs"), "{line}");
}

#[test]
fn several_merge_bases_are_unavailable() {
    // A criss-cross merge: two merge bases, so git builds a virtual ancestor.
    let repo = Repo::new();
    repo.write("Cargo.toml", &manifest("a", "z"));
    repo.write("Cargo.lock", "0\n");
    repo.commit("base");
    repo.git(&["branch", "other"]);
    repo.write("x", "1\n");
    repo.commit("main 1");
    repo.git(&["checkout", "-q", "other"]);
    repo.write("y", "1\n");
    repo.commit("other 1");
    repo.git(&["checkout", "-q", "-b", "other2"]);
    repo.git(&["merge", "-q", "--no-edit", "main"]);
    repo.git(&["checkout", "-q", "main"]);
    repo.git(&["merge", "-q", "--no-edit", "other"]);
    // Both tips have the other's earlier commit as a merge base; change the
    // lockfile on each so that the final merge conflicts there.
    repo.write("Cargo.lock", "main\n");
    repo.commit("main lock");
    repo.git(&["checkout", "-q", "other2"]);
    repo.write("Cargo.lock", "other\n");
    repo.commit("other lock");
    repo.git(&["checkout", "-q", "main"]);
    let bases = repo.git(&["merge-base", "--all", "main", "other2"]);
    assert_eq!(bases.lines().count(), 2, "setup must give two merge bases");
    repo.output(&["merge", "other2"]);
    let line = repo.log().pop().expect("the driver ran");
    assert!(line.contains("merged common ancestors"), "{line}");
    assert!(line.starts_with("unavailable "), "{line}");
}

#[test]
fn cherry_pick_of_a_merge_commit_is_unavailable() {
    let repo = Repo::new();
    repo.write("Cargo.toml", &manifest("a", "z"));
    repo.write("Cargo.lock", "base\n");
    repo.commit("base");
    repo.git(&["branch", "side1"]);
    repo.git(&["branch", "side2"]);
    repo.write("Cargo.lock", "main\n");
    repo.commit("main lock");
    repo.git(&["checkout", "-q", "side1"]);
    repo.write("x", "1\n");
    repo.commit("side1");
    repo.git(&["checkout", "-q", "side2"]);
    repo.write("y", "1\n");
    repo.commit("side2");
    repo.git(&["merge", "-q", "--no-edit", "side1"]);
    repo.write("Cargo.lock", "merge result\n");
    repo.git(&["commit", "-q", "-a", "--amend", "--no-edit"]);
    repo.git(&["checkout", "-q", "main"]);
    repo.output(&["cherry-pick", "-m", "1", "side2"]);
    let line = repo.only_log();
    assert!(line.starts_with("unavailable "), "{line}");
    assert!(line.contains("2 parents"), "{line}");
}

/// A single pick of a commit that changes only the manifest, then a pick that
/// conflicts in the lockfile. With `-n` the first change is staged.
fn staged_fixture() -> (Repo, String) {
    let repo = Repo::new();
    repo.write("Cargo.toml", &manifest("a", "z"));
    repo.write("Cargo.lock", "base\n");
    let base = repo.commit("base");
    repo.git(&["branch", "side"]);
    repo.write("Cargo.lock", "main\n");
    repo.commit("main lock");
    repo.git(&["checkout", "-q", "side"]);
    repo.write("Cargo.toml", &manifest("a", "pick1"));
    repo.commit("pick 1 changes the manifest");
    repo.write("Cargo.lock", "theirs\n");
    repo.commit("pick 2 changes the lockfile");
    repo.git(&["checkout", "-q", "main"]);
    (repo, base)
}

#[test]
fn cherry_pick_of_two_commits_without_n_identifies_the_second() {
    let (repo, _) = staged_fixture();
    repo.output(&["cherry-pick", "side~1", "side"]);
    let line = repo.only_log();
    assert!(line.starts_with("identified "), "{line}");
}

#[test]
fn cherry_pick_n_with_a_staged_manifest_is_unavailable() {
    let (repo, _) = staged_fixture();
    repo.output(&["cherry-pick", "-n", "side~1", "side"]);
    let line = repo.only_log();
    assert!(line.starts_with("unavailable "), "{line}");
    assert!(line.contains("staged"), "{line}");
}

// The remaining tests call the API without a real driver run.

fn pick_args<'a>(label: &'a str, parent_label: &'a str, lock: &'a [u8]) -> DriverArgs<'a> {
    DriverArgs {
        base_label: parent_label,
        ours_label: "HEAD",
        theirs_label: label,
        lockfile_path: "Cargo.lock",
        ours_lockfile: lock,
    }
}

#[test]
fn ours_is_proven_against_the_lockfile_of_head() {
    let f = diverged();
    let git = Git::new().in_dir(&f.repo.dir);
    let short = &f.theirs[..7];
    let y = format!("{short} (side change)");
    let s = format!("parent of {y}");
    let found = git.identify(&pick_args(&y, &s, b"lock ours\n"), &[]);
    assert_eq!(
        found,
        Ok(Identified {
            base: f.base.clone(),
            ours: f.ours.clone(),
            theirs: f.theirs.clone()
        })
    );
    // The neighbor: %A is not the lockfile of HEAD.
    assert!(
        git.identify(&pick_args(&y, &s, b"lock something else\n"), &[])
            .is_err()
    );
}

#[test]
fn the_label_must_agree_with_the_commit() {
    let f = diverged();
    let git = Git::new().in_dir(&f.repo.dir);
    // Applying the commit `ours`, a child of base, but labeled as theirs'
    // base: the base label is derived from the commit, never trusted.
    let y = format!("{} (side change)", &f.theirs[..7]);
    let s = format!("parent of {} (side change)", &f.ours[..7]);
    assert!(
        git.identify(&pick_args(&y, &s, b"lock ours\n"), &[])
            .is_err()
    );
}

#[test]
fn a_ref_that_looks_like_an_abbreviation_is_refused() {
    let f = diverged();
    let git = Git::new().in_dir(&f.repo.dir);
    // A branch named like a short sha, pointing elsewhere.
    f.repo.git(&["branch", "abcdef1", &f.base]);
    assert!(git.resolve_abbrev("abcdef1").is_err());
    assert_eq!(
        git.resolve_abbrev(&f.theirs[..7]).as_deref(),
        Ok(f.theirs.as_str())
    );
}

#[test]
fn an_ambiguous_short_sha_is_refused() {
    let repo = Repo::new();
    repo.write("a", "a\n");
    repo.commit("a");
    let tree = repo.git(&["rev-parse", "HEAD^{tree}"]);
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut pair = None;
    for i in 0..70_000 {
        let out = base_cmd(&repo.dir)
            .env("GIT_COMMITTER_DATE", "1700000000 +0000")
            .env("GIT_AUTHOR_DATE", "1700000000 +0000")
            .args(["commit-tree", &tree, "-m", &format!("c{i}")])
            .output()
            .unwrap();
        let sha = String::from_utf8(out.stdout).unwrap().trim().to_string();
        if let Some(prev) = seen.insert(sha[..4].to_string(), sha.clone()) {
            pair = Some((prev, sha));
            break;
        }
    }
    let (a, b) = pair.expect("pigeonhole: 4 hex digits have 65536 values");
    assert_ne!(a, b);
    let git = Git::new().in_dir(&repo.dir);
    assert!(git.resolve_abbrev(&a[..4]).is_err());
    // A longer abbreviation that tells them apart resolves.
    let n = (4..40).find(|&n| a[..n] != b[..n]).unwrap();
    assert_eq!(git.resolve_abbrev(&a[..n]).as_deref(), Ok(a.as_str()));
}

#[test]
fn git_that_does_not_start_is_unavailable() {
    let f = diverged();
    let git = Git::new()
        .in_dir(&f.repo.dir)
        .with_program("/nonexistent/git");
    let args = pick_args("f96df54 (s)", "parent of f96df54 (s)", b"");
    let m = git.manifests(&args, &[]);
    assert!(matches!(
        m.merged("Cargo.toml"),
        ManifestResult::Unavailable(_)
    ));
    // With the commits known, a git that does not start is still Unavailable.
    let m = GitManifests::with_commits(
        git,
        Identified {
            base: f.base.clone(),
            ours: f.ours.clone(),
            theirs: f.theirs.clone(),
        },
    );
    assert!(matches!(
        m.merged("Cargo.toml"),
        ManifestResult::Unavailable(_)
    ));
}

#[test]
fn standalone_and_unknown_runs_are_unavailable() {
    let f = diverged();
    let git = Git::new().in_dir(&f.repo.dir);
    let args = DriverArgs {
        base_label: "base",
        ours_label: "ours",
        theirs_label: "theirs",
        lockfile_path: "Cargo.lock",
        ours_lockfile: b"lock ours\n",
    };
    let m = git.manifests(&args, &[]);
    assert!(matches!(
        m.merged("Cargo.toml"),
        ManifestResult::Unavailable(_)
    ));
}

#[test]
fn the_merged_manifest_ignores_the_working_tree() {
    let f = diverged();
    let git = Git::new().in_dir(&f.repo.dir);
    let m = GitManifests::with_commits(
        git,
        Identified {
            base: f.base.clone(),
            ours: f.ours.clone(),
            theirs: f.theirs.clone(),
        },
    );
    let expected = ManifestResult::Merged(manifest("ours", "theirs").into_bytes());
    assert_eq!(m.merged("Cargo.toml"), expected);
    f.repo.write("Cargo.toml", "garbage in the working tree\n");
    assert_eq!(m.merged("Cargo.toml"), expected);
}

#[test]
fn paths_that_leave_the_top_level_are_unavailable() {
    let f = diverged();
    let git = Git::new().in_dir(&f.repo.dir);
    let m = GitManifests::with_commits(
        git,
        Identified {
            base: f.base.clone(),
            ours: f.ours.clone(),
            theirs: f.theirs.clone(),
        },
    );
    for bad in [
        "",
        "./Cargo.toml",
        "../Cargo.toml",
        "a/../Cargo.toml",
        "/Cargo.toml",
        "a//b",
    ] {
        assert!(
            matches!(m.merged(bad), ManifestResult::Unavailable(_)),
            "{bad:?}"
        );
    }
    assert!(matches!(
        m.merged("nope/Cargo.toml"),
        ManifestResult::Unavailable(_)
    ));
}

#[test]
fn a_partial_clone_never_fetches_a_missing_manifest() {
    let f = diverged();
    let src = &f.repo.dir;
    f.repo.git(&["config", "uploadpack.allowFilter", "true"]);
    f.repo
        .git(&["config", "uploadpack.allowAnySHA1InWant", "true"]);
    let clone = Repo::new();
    let url = format!("file://{}", src.display());
    let out = base_cmd(&clone.dir)
        .args([
            "fetch",
            "-q",
            "--no-tags",
            "--filter=blob:none",
            &url,
            "+refs/heads/*:refs/remotes/o/*",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    clone.git(&["config", "remote.o.url", &url]);
    clone.git(&["config", "remote.o.promisor", "true"]);
    clone.git(&["config", "remote.o.partialclonefilter", "blob:none"]);
    clone.git(&["config", "extensions.partialclone", "o"]);
    let missing = |r: &Repo| r.git(&["rev-list", "--objects", "--missing=print", "--all"]);
    let before = missing(&clone);
    assert!(before.contains('?'), "setup: blobs must be missing");
    let m = GitManifests::with_commits(
        Git::new().in_dir(&clone.dir),
        Identified {
            base: f.base.clone(),
            ours: f.ours.clone(),
            theirs: f.theirs.clone(),
        },
    );
    assert!(matches!(
        m.merged("Cargo.toml"),
        ManifestResult::Unavailable(_)
    ));
    assert_eq!(missing(&clone), before, "no object may be fetched");
}

#[test]
fn old_git_placeholders_left_literal_are_unavailable() {
    // git before the %S %X %Y placeholders passes them unexpanded.
    let f = diverged();
    let git = Git::new().in_dir(&f.repo.dir);
    let args = DriverArgs {
        base_label: "%S",
        ours_label: "%X",
        theirs_label: "%Y",
        lockfile_path: "Cargo.lock",
        ours_lockfile: b"lock ours\n",
    };
    assert!(git.identify(&args, &[]).is_err());
}
