//! `lockmerge-bench check`: replay every case, classify, compute the metrics
//! and enforce the gate.

use crate::attribution::{attribute, disagreement};
use crate::case::{
    ECOSYSTEMS, REQUIRED_ECOSYSTEMS, TreeSize, case_dirs, load_case, over_budget, tree_size,
};
use crate::classify::{
    CaseView, Class, GitOutcome, Run, classify, classify_git, foreign_ids, kind_of,
};
use crate::gate::{Baseline, TOTAL_BUDGET, budget_of, gate, load_baseline, release_v1};
use crate::metrics::{CaseResult, Metrics, compute};
use crate::oracle::for_ecosystem;
use crate::replay::{DEFAULT_TIMEOUT, ReplayOpts, replay};
use crate::selftest;
use lockmerge_core::Format;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use toml_edit::DocumentMut;

pub struct CheckOpts {
    pub fixtures: PathBuf,
    /// `Some("v1")` adds the release targets.
    pub release: Option<String>,
    /// The `lockmerge` binary the driver runs. Default: next to this binary.
    pub driver_bin: Option<PathBuf>,
    pub timeout: Duration,
}

impl CheckOpts {
    pub fn new(fixtures: PathBuf) -> Self {
        CheckOpts {
            fixtures,
            release: None,
            driver_bin: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

pub struct EcoReport {
    pub name: String,
    pub metrics: Metrics,
    pub baseline: Option<Baseline>,
    pub results: Vec<CaseResult>,
    /// Deduplicated (checked against the budget) and raw on-disk bytes.
    pub size: TreeSize,
}

pub struct CheckOutcome {
    pub ecosystems: Vec<EcoReport>,
    /// Every reason the gate fails; empty means it passes.
    pub failures: Vec<String>,
    pub selftest: Result<(), String>,
}

impl CheckOutcome {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// The directory holding a `lockmerge` binary: the given path's directory, or
/// the one this executable sits in.
fn driver_dir(opts: &CheckOpts) -> Result<PathBuf, String> {
    let exe_name = format!("lockmerge{}", std::env::consts::EXE_SUFFIX);
    let bin = match &opts.driver_bin {
        Some(p) => p.clone(),
        None => std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name(&exe_name),
    };
    if bin.file_name().and_then(|n| n.to_str()) != Some(exe_name.as_str()) {
        return Err(format!(
            "the driver binary must be named `{exe_name}`: {}",
            bin.display()
        ));
    }
    if !bin.is_file() {
        return Err(format!(
            "no `lockmerge` binary at {}; build it or pass --driver-bin",
            bin.display()
        ));
    }
    // A file that cannot run would let the shell skip it and find another
    // `lockmerge` on PATH, or let git report the launch failure as a refusal.
    if !is_executable(&bin) {
        return Err(format!(
            "the driver binary {} is not an executable file",
            bin.display()
        ));
    }
    // Absolute, so the replay's PATH entry does not depend on the directory
    // of the temporary repository.
    // Only the directory is resolved: the selected basename stays, so a
    // symlink to a differently named executable still runs as `lockmerge`
    // from its own directory, never as its target's neighbor.
    let parent = bin
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::canonicalize(parent).map_err(|e| format!("{}: {e}", parent.display()))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    // `test -x` asks the system whether this process may execute the file
    // (access(2) with X_OK), so owner, group and other bits are applied to the
    // current user, which a mode mask cannot do.
    std::fs::metadata(p).is_ok_and(|m| m.is_file())
        && std::process::Command::new("/bin/sh")
            .args(["-c", "test -x \"$1\"", "sh"])
            .arg(p)
            .status()
            .is_ok_and(|s| s.success())
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

fn check_index(eco_dir: &Path) -> Result<(), String> {
    let path = eco_dir.join("INDEX.toml");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.parse::<DocumentMut>()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", path.display()))
}

pub fn check(opts: &CheckOpts, formats: &[&dyn Format]) -> CheckOutcome {
    let mut failures = Vec::new();
    let selftest = selftest::run();
    if let Err(e) = &selftest {
        failures.push(format!("oracle self-test failed: {e}"));
    }

    let mut ecosystems = Vec::new();
    let mut driver: Option<Result<PathBuf, String>> = None;
    let mut total = TreeSize::default();

    for eco in REQUIRED_ECOSYSTEMS {
        if !opts.fixtures.join(eco).is_dir() {
            failures.push(format!(
                "{} has no `{eco}` directory: an absent corpus would check nothing",
                opts.fixtures.display()
            ));
        }
    }

    for eco in ECOSYSTEMS {
        let eco_dir = opts.fixtures.join(eco);
        if !eco_dir.is_dir() {
            continue;
        }
        let mut bad: Vec<(String, String)> = Vec::new();
        let baseline = match load_baseline(&eco_dir.join("baseline.toml")) {
            Ok(b) => Some(b),
            Err(e) => {
                failures.push(format!("{eco}: {e}"));
                None
            }
        };
        if let Err(e) = check_index(&eco_dir) {
            failures.push(format!("{eco}: {e}"));
        }
        let size = match tree_size(&eco_dir) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("{eco}: {e}"));
                TreeSize::default()
            }
        };
        total.unique += size.unique;
        total.raw += size.raw;
        failures.extend(over_budget(eco, size, budget_of(eco)));
        let Some(oracle) = for_ecosystem(eco) else {
            failures.push(format!("{eco}: there is no oracle for this ecosystem yet"));
            continue;
        };
        let dirs = match case_dirs(&eco_dir) {
            Ok(d) => d,
            Err(e) => {
                failures.push(format!("{eco}: {e}"));
                Vec::new()
            }
        };

        let mut results = Vec::new();
        for (id, dir) in dirs {
            let case = match load_case(eco, &id, &dir) {
                Ok(c) => c,
                Err(e) => {
                    failures.push(e);
                    continue;
                }
            };
            for p in &case.problems {
                failures.push(p.clone());
            }
            // The class is recomputed from the bytes, never trusted.
            let foreign = match foreign_ids(
                oracle.as_ref(),
                &case.base,
                &case.ours,
                &case.theirs,
                &case.expected,
            ) {
                Ok(f) => f,
                Err(e) => {
                    failures.push(format!("{id}: cannot compute the class: {e}"));
                    continue;
                }
            };
            let kind = kind_of(&foreign);
            if kind != case.meta.kind {
                bad.push((
                    id.clone(),
                    format!(
                        "case.toml says {} but the bytes say {}",
                        case.meta.kind.name(),
                        kind.name()
                    ),
                ));
            }
            if let Some(n) = case.meta.foreign_count
                && usize::try_from(n).ok() != Some(foreign.len())
            {
                bad.push((
                    id.clone(),
                    format!(
                        "case.toml records {n} foreign ids but there are {}",
                        foreign.len()
                    ),
                ));
            }
            let driver_dir = driver.get_or_insert_with(|| driver_dir(opts)).clone();
            let result = match driver_dir {
                Err(e) => {
                    if !failures.contains(&e) {
                        failures.push(e);
                    }
                    continue;
                }
                Ok(dir) => run_case(
                    &case,
                    kind,
                    oracle.as_ref(),
                    formats,
                    &ReplayOpts {
                        driver_dir: Some(dir),
                        timeout: opts.timeout,
                    },
                ),
            };
            results.push(result);
        }

        let metrics = compute(&results);
        if let Some(b) = &baseline {
            failures.extend(gate(eco, &metrics, b, &bad));
        } else {
            failures.extend(bad.iter().map(|(i, w)| format!("{eco}: {i}: {w}")));
        }
        ecosystems.push(EcoReport {
            name: eco.to_string(),
            metrics,
            baseline,
            results,
            size,
        });
    }
    failures.extend(over_budget("total", total, TOTAL_BUDGET));

    if let Some(release) = &opts.release {
        if release == "v1" {
            let find = |n: &str| ecosystems.iter().find(|e| e.name == n).map(|e| &e.metrics);
            failures.extend(release_v1(find("cargo"), find("cargo-agent")));
        } else {
            failures.push(format!("unknown release `{release}`; the only one is `v1`"));
        }
    }
    CheckOutcome {
        ecosystems,
        failures,
        selftest,
    }
}

fn run_case(
    case: &crate::case::Case,
    kind: crate::classify::Kind,
    oracle: &dyn crate::oracle::Oracle,
    formats: &[&dyn Format],
    opts: &ReplayOpts,
) -> CaseResult {
    let view = CaseView {
        kind,
        expected: &case.expected,
        waiver: case.meta.waiver.as_ref(),
    };
    let failed = |detail: String, millis: u128| CaseResult {
        id: case.id.clone(),
        kind,
        git: GitOutcome::Error,
        class: Class::Error,
        detail,
        millis,
        exact: false,
        attribution: None,
        waived: false,
        pr_branch: case.meta.pr_branch,
    };
    let replayed = match replay(case, opts) {
        Ok(r) => r,
        Err(e) => return failed(e, 0),
    };
    let git = classify_git(oracle, &view, &replayed.git);
    let mut verdict = classify(oracle, &view, &replayed.lm, &replayed.lm_swapped);
    if let crate::classify::GitRun::Error(e) = &replayed.git {
        verdict.class = Class::Error;
        verdict.detail = format!("plain git merge: {e}");
    }
    let attribution = match attribute(formats, case) {
        Ok(a) => a,
        Err(e) => {
            verdict.class = Class::Error;
            verdict.detail = e;
            None
        }
    };
    if let Some(a) = &attribution
        && let Some(why) = disagreement(a, &replayed.lm)
    {
        verdict.class = Class::Error;
        verdict.detail = why;
    }
    let exact = verdict.class == Class::ResolvedCorrect
        && matches!(&replayed.lm, Run::Clean(b) if *b == case.expected);
    CaseResult {
        id: case.id.clone(),
        kind,
        git,
        class: verdict.class,
        detail: verdict.detail,
        millis: replayed.millis,
        exact,
        attribution,
        waived: case.meta.waiver.is_some(),
        pr_branch: case.meta.pr_branch,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::util::TempDir;

    fn make_exe(p: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(p, "").unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn a_non_executable_driver_is_rejected_even_with_a_competitor_on_path() {
        let (sel, other) = (TempDir::new("sel").unwrap(), TempDir::new("other").unwrap());
        let bin = sel.path().join("lockmerge");
        std::fs::write(&bin, "").unwrap(); // mode 0644
        make_exe(&other.path().join("lockmerge"));
        let mut opts = CheckOpts::new(PathBuf::from("unused"));
        opts.driver_bin = Some(bin);
        // Rejected up front, whatever PATH holds: no fallback is possible.
        let err = driver_dir(&opts).unwrap_err();
        assert!(err.contains("not an executable file"), "{err}");
        // Mode 0645: execute bits exist, but not for the owner, who is the
        // current user. Another user's bits do not make it runnable.
        {
            use std::os::unix::fs::PermissionsExt;
            let p = opts.driver_bin.clone().unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o645)).unwrap();
            let root = std::process::Command::new("id")
                .arg("-u")
                .output()
                .unwrap()
                .stdout
                == b"0\n";
            if !root {
                assert!(driver_dir(&opts).is_err());
            }
        }
        // The same file made executable is accepted.
        make_exe(&opts.driver_bin.clone().unwrap());
        assert!(driver_dir(&opts).is_ok());
    }

    #[test]
    fn a_relative_driver_bin_becomes_an_absolute_directory() {
        // The replay puts this directory on PATH inside a temporary
        // repository, so a relative one would find another `lockmerge`.
        let tmp = TempDir::new("rel").unwrap();
        let bin = tmp.path().join("lockmerge");
        make_exe(&bin);
        let cwd = std::env::current_dir().unwrap();
        let mut rel = PathBuf::new();
        for _ in cwd.components().skip(1) {
            rel.push("..");
        }
        rel.push(bin.strip_prefix("/").unwrap());
        assert!(rel.is_relative());
        let mut opts = CheckOpts::new(PathBuf::from("unused"));
        opts.driver_bin = Some(rel);
        let dir = driver_dir(&opts).unwrap();
        assert!(dir.is_absolute());
        assert_eq!(dir, std::fs::canonicalize(tmp.path()).unwrap());
    }

    #[test]
    fn a_symlinked_driver_keeps_its_own_directory() {
        use std::os::unix::fs::symlink;
        let (here, elsewhere) = (TempDir::new("here").unwrap(), TempDir::new("else").unwrap());
        // The selected `lockmerge` is a link to a differently named file in
        // another directory, which also holds a competing `lockmerge`.
        let target = elsewhere.path().join("real-driver");
        make_exe(&target);
        make_exe(&elsewhere.path().join("lockmerge"));
        symlink(&target, here.path().join("lockmerge")).unwrap();
        let mut opts = CheckOpts::new(PathBuf::from("unused"));
        opts.driver_bin = Some(here.path().join("lockmerge"));
        let dir = driver_dir(&opts).unwrap();
        assert_eq!(dir, std::fs::canonicalize(here.path()).unwrap());
    }
}
