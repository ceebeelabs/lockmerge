//! The binary against real git in temporary repositories: `lockmerge install`
//! and then `git merge`, `git cherry-pick` and `git rebase`. The lockfiles are
//! the existing fixtures of `tests/fixtures/rules/cargo/E1`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_lockmerge");

fn fixture(case: &str, file: &str) -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/rules/cargo/E1")
        .join(case)
        .join(file);
    fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(tag: &str) -> Repo {
        let dir = std::env::temp_dir().join(format!("lockmerge-e2e-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("home")).unwrap();
        fs::create_dir_all(dir.join("repo")).unwrap();
        let r = Repo { dir };
        r.git(&["init", "-q", "-b", "main"]).ok();
        r
    }

    fn work(&self) -> PathBuf {
        self.dir.join("repo")
    }

    fn command(&self, program: &str) -> Command {
        let bin_dir = Path::new(BIN).parent().unwrap();
        let path = format!(
            "{}:{}",
            bin_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut c = Command::new(program);
        c.current_dir(self.work())
            .env("PATH", path)
            .env("HOME", self.dir.join("home"))
            .env("GIT_CONFIG_GLOBAL", self.dir.join("home/gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE");
        c
    }

    fn git(&self, args: &[&str]) -> Checked {
        let out = self.command("git").args(args).output().unwrap();
        Checked(out, args.join(" "))
    }

    fn lockmerge(&self, args: &[&str]) -> Checked {
        let out = self.command(BIN).args(args).output().unwrap();
        Checked(out, format!("lockmerge {}", args.join(" ")))
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        fs::write(self.work().join(name), bytes).unwrap();
    }

    fn read(&self, name: &str) -> Vec<u8> {
        fs::read(self.work().join(name)).unwrap()
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]).ok();
        self.git(&["commit", "-q", "-m", message]).ok();
    }

    /// `main` has base; `ours` and `theirs` branch from it with their lockfiles.
    fn branches(&self, case: &str) {
        self.write(".gitattributes", b"Cargo.lock merge=lockmerge\n");
        self.write("Cargo.lock", &fixture(case, "base"));
        self.commit("base");
        self.git(&["checkout", "-q", "-b", "theirs"]).ok();
        self.write("Cargo.lock", &fixture(case, "theirs"));
        self.commit("theirs");
        self.git(&["checkout", "-q", "main"]).ok();
        self.write("Cargo.lock", &fixture(case, "ours"));
        self.commit("ours");
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

struct Checked(Output, String);

impl Checked {
    fn ok(&self) -> &Output {
        assert!(self.0.status.success(), "{} failed: {}", self.1, self.err());
        &self.0
    }
    fn out(&self) -> String {
        String::from_utf8_lossy(&self.0.stdout).into_owned()
    }
    fn err(&self) -> String {
        String::from_utf8_lossy(&self.0.stderr).into_owned()
    }
    fn code(&self) -> i32 {
        self.0.status.code().unwrap_or(-1)
    }
}

const DRIVER: &str = "lockmerge merge-driver %O %A %B %L %P %S %X %Y";

#[test]
fn install_writes_name_and_driver_together_and_prints_them() {
    let r = Repo::new("install");
    let c = r.lockmerge(&["install"]);
    c.ok();
    assert!(c.out().contains(DRIVER), "{}", c.out());
    assert_eq!(
        r.git(&["config", "--local", "merge.lockmerge.driver"])
            .out()
            .trim(),
        DRIVER
    );
    assert_eq!(
        r.git(&["config", "--local", "merge.lockmerge.name"])
            .out()
            .trim(),
        "lockmerge lockfile merge driver"
    );
}

#[test]
fn install_print_changes_nothing() {
    let r = Repo::new("print");
    let c = r.lockmerge(&["install", "--print"]);
    c.ok();
    assert!(
        c.out().contains(DRIVER) && c.out().contains("merge.lockmerge.name"),
        "{}",
        c.out()
    );
    assert!(
        !r.git(&["config", "--local", "--list"])
            .out()
            .contains("lockmerge")
    );
}

#[test]
fn install_global_writes_the_global_file_only() {
    let r = Repo::new("global");
    r.lockmerge(&["install", "--global"]).ok();
    let global = fs::read_to_string(r.dir.join("home/gitconfig")).unwrap();
    assert!(global.contains(DRIVER), "{global}");
    assert!(
        !r.git(&["config", "--local", "--list"])
            .out()
            .contains("lockmerge")
    );
}

#[test]
fn install_rejects_unknown_options() {
    let r = Repo::new("badopt");
    assert_eq!(r.lockmerge(&["install", "--bogus"]).code(), 2);
}

#[test]
fn git_merge_without_the_driver_conflicts_on_the_same_files() {
    // The premise of the test below: plain git cannot merge these.
    let r = Repo::new("control");
    r.branches("accept-external-package-unchanged");
    r.write(".gitattributes", b"");
    r.commit("no driver");
    assert_ne!(r.git(&["merge", "theirs"]).code(), 0);
}

#[test]
fn git_merge_runs_the_driver_and_takes_a_proven_merge() {
    let r = Repo::new("merge-clean");
    r.branches("accept-external-package-unchanged");
    r.lockmerge(&["install"]).ok();
    let m = r.git(&["merge", "theirs"]);
    m.ok();
    assert_eq!(
        r.read("Cargo.lock"),
        fixture("accept-external-package-unchanged", "expected")
    );
}

#[test]
fn git_merge_with_a_refusal_conflicts_with_markers_and_the_reason() {
    let r = Repo::new("merge-refuse");
    r.branches("refuse-both-bumped-differently");
    r.lockmerge(&["install"]).ok();
    let m = r.git(&["merge", "theirs"]);
    assert_eq!(m.code(), 1, "{}{}", m.out(), m.err());
    let text = String::from_utf8(r.read("Cargo.lock")).unwrap();
    assert!(
        text.contains("<<<<<<<") && text.contains("=======") && text.contains(">>>>>>>"),
        "{text}"
    );
    let all = format!("{}{}", m.out(), m.err());
    assert!(all.contains("cargo.R3"), "{all}");
    assert!(
        all.contains("Both branches moved a package to different versions"),
        "{all}"
    );
}

#[test]
fn cherry_pick_runs_the_driver_too() {
    let r = Repo::new("pick");
    r.branches("accept-external-package-unchanged");
    r.lockmerge(&["install"]).ok();
    r.git(&["cherry-pick", "theirs"]).ok();
    assert_eq!(
        r.read("Cargo.lock"),
        fixture("accept-external-package-unchanged", "expected")
    );
}

#[test]
fn an_unsupported_file_name_conflicts_with_markers_through_git() {
    let r = Repo::new("unsupported");
    r.write(".gitattributes", b"*.lock merge=lockmerge\n");
    r.write("other.lock", b"a\nb\nc\n");
    r.commit("base");
    r.git(&["checkout", "-q", "-b", "theirs"]).ok();
    r.write("other.lock", b"a\nb\nC\n");
    r.commit("theirs");
    r.git(&["checkout", "-q", "main"]).ok();
    r.write("other.lock", b"A\nb\nc\n");
    r.commit("ours");
    r.lockmerge(&["install"]).ok();
    let m = r.git(&["merge", "theirs"]);
    assert_eq!(m.code(), 1);
    assert!(
        String::from_utf8(r.read("other.lock"))
            .unwrap()
            .contains("<<<<<<<")
    );
}

/// A path with non-UTF-8 bytes is a valid git path on Unix. The binary must
/// exit 255 (not 101) and leave %A unchanged.
#[cfg(unix)]
#[test]
fn a_non_utf8_argument_exits_255_and_leaves_a_unchanged() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let r = Repo::new("nonutf8");
    for (n, c) in [("O", "base\n"), ("A", "ours\n"), ("B", "theirs\n")] {
        r.write(n, c.as_bytes());
    }
    let bad = OsString::from_vec(b"caf\xe9/Cargo.lock".to_vec());
    let out = r
        .command(BIN)
        .arg("merge-driver")
        .args(["O", "A", "B", "7"])
        .arg(bad)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(255));
    assert_eq!(r.read("A"), b"ours\n");
}

/// The case `tests/fixtures/rules/cargo/M4/accept-both-insert-into-dependencies`
/// as a repository: both branches add a dependency line at the end of the same
/// table of `app/Cargo.toml`, so git's own merge of that manifest conflicts.
fn m4_case(r: &Repo) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/rules/cargo/M4/accept-both-insert-into-dependencies");
    let load = |role: &str| {
        for member in ["", "app/", "cli/", "lib/", "tool/"] {
            let from = dir.join("sides").join(role).join(member).join("Cargo.toml");
            fs::create_dir_all(r.work().join(member)).unwrap();
            r.write(&format!("{member}Cargo.toml"), &fs::read(from).unwrap());
        }
        r.write("Cargo.lock", &fs::read(dir.join(role)).unwrap());
    };
    r.write(".gitattributes", b"Cargo.lock merge=lockmerge\n");
    load("base");
    r.commit("base");
    r.git(&["checkout", "-q", "-b", "theirs"]).ok();
    load("theirs");
    r.commit("theirs");
    r.git(&["checkout", "-q", "main"]).ok();
    load("ours");
    r.commit("ours");
}

#[test]
fn a_manifest_conflict_of_two_insertions_merges_the_lockfile_and_says_so() {
    let r = Repo::new("m4");
    m4_case(&r);
    r.lockmerge(&["install"]).ok();
    let m = r.git(&["merge", "theirs"]);
    // Git leaves the manifest conflicted; the lockfile is merged.
    assert_eq!(m.code(), 1, "{}{}", m.out(), m.err());
    let expected =
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../tests/fixtures/rules/cargo/M4/accept-both-insert-into-dependencies/expected",
        ))
        .unwrap();
    assert_eq!(r.read("Cargo.lock"), expected);
    let manifest = String::from_utf8(r.read("app/Cargo.toml")).unwrap();
    assert!(manifest.contains("<<<<<<<"), "{manifest}");
    let all = format!("{}{}", m.out(), m.err());
    assert!(
        all.contains(
            "Cargo.toml has a merge conflict; the merged Cargo.lock assumes both added dependencies are kept."
        ),
        "{all}"
    );
}

#[test]
fn a_clean_merge_that_needs_no_manifest_conflict_prints_no_notice() {
    let r = Repo::new("m4-quiet");
    r.branches("accept-external-package-unchanged");
    r.lockmerge(&["install"]).ok();
    let m = r.git(&["merge", "theirs"]);
    m.ok();
    assert!(!format!("{}{}", m.out(), m.err()).contains("merge conflict; the merged"));
}
