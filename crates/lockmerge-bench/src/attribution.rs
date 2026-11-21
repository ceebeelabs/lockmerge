//! Per-rule and per-code attribution (docs/03-architecture.md section 3).
//!
//! The classification of a case comes from real `git merge` and the oracle.
//! To say *which rule* applied or *which code* refused, the bench also runs
//! the registered [`Format`] in process on the same bytes and reads its
//! [`Report`]. The two must agree on the outcome; if they do not, the case is
//! an `error`. That also catches a driver that misidentified the commits.

use crate::case::Case;
use crate::classify::Run;
use crate::util::{TempDir, git_isolated, run};
use lockmerge_core::{Context, Format, Inputs, ManifestResult, Manifests, Outcome, Report, Why};
use std::fs;
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribution {
    pub format: String,
    /// Every applied rule id, one entry per accepted unit.
    pub applied: Vec<String>,
    /// The primary refusal's code, when the format refused.
    pub refusal: Option<String>,
    /// Codes of non-fatal refusals that did not end the merge.
    pub downgrades: Vec<String>,
    /// The merged bytes, when the format merged.
    pub merged: Option<Vec<u8>>,
}

/// The merged manifest of a path, from the case's saved manifests: the three
/// sides run through `git merge-file`, as the driver's `Git` implementation
/// does. Anything it cannot establish is `Unavailable`, the safe direction.
struct SavedManifests<'a> {
    case: &'a Case,
}

impl SavedManifests<'_> {
    fn side(&self, side: &str, path: &str) -> Option<&[u8]> {
        self.case
            .manifests
            .iter()
            .find(|m| m.side == side && m.rel == path)
            .map(|m| m.bytes.as_slice())
    }
}

impl Manifests for SavedManifests<'_> {
    fn merged(&self, repo_path: &str) -> ManifestResult {
        let (Some(b), Some(o), Some(t)) = (
            self.side("base", repo_path),
            self.side("ours", repo_path),
            self.side("theirs", repo_path),
        ) else {
            return ManifestResult::Unavailable(Why(format!(
                "`{repo_path}` is not saved on all three sides"
            )));
        };
        if o == t || b == t {
            return ManifestResult::Merged(o.to_vec());
        }
        if b == o {
            return ManifestResult::Merged(t.to_vec());
        }
        let unavailable = |m: String| ManifestResult::Unavailable(Why(m));
        let tmp = match TempDir::new("manifest") {
            Ok(t) => t,
            Err(e) => return unavailable(e.to_string()),
        };
        for (name, bytes) in [("ours", o), ("base", b), ("theirs", t)] {
            if let Err(e) = fs::write(tmp.path().join(name), bytes) {
                return unavailable(e.to_string());
            }
        }
        let mut cmd = git_isolated(tmp.path());
        cmd.args(["merge-file", "-p", "ours", "base", "theirs"]);
        match run(cmd, Duration::from_secs(30)) {
            Ok(r) if r.code == Some(0) => ManifestResult::Merged(r.stdout),
            Ok(r) if matches!(r.code, Some(c) if c > 0 && c < 128) => ManifestResult::Conflict,
            Ok(r) => unavailable(format!("git merge-file: {}", r.stderr_text())),
            Err(_) => unavailable("git merge-file did not finish".into()),
        }
    }
}

fn from_report(report: &Report) -> Attribution {
    let (refusal, merged) = match &report.outcome {
        Outcome::Refused(r) => (Some(r.code.to_string()), None),
        Outcome::Clean(c) => (None, Some(c.bytes().to_vec())),
    };
    Attribution {
        format: report.format.to_string(),
        applied: report.applied.iter().map(|a| a.rule.to_string()).collect(),
        refusal,
        downgrades: report
            .downgrades
            .iter()
            .map(|r| r.code.to_string())
            .collect(),
        merged,
    }
}

/// Runs the format that claims the case's lockfile name. `Ok(None)` when no
/// registered format does (the tables then have nothing to count).
pub fn attribute(formats: &[&dyn Format], case: &Case) -> Result<Option<Attribution>, String> {
    let name = Path::new(&case.meta.lockfile_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let Some(format) = formats.iter().find(|f| f.matches(name)) else {
        return Ok(None);
    };
    let manifests = SavedManifests { case };
    let cx = Context::new(&manifests);
    let inputs = Inputs {
        base: &case.base,
        ours: &case.ours,
        theirs: &case.theirs,
        path: &case.meta.lockfile_path,
    };
    match format.merge(&inputs, &cx) {
        Ok(report) => Ok(Some(from_report(&report))),
        Err(e) => Err(format!("format {} failed: {e}", format.id())),
    }
}

/// `Some(reason)` when the in-process run and the git run disagree.
pub fn disagreement(attr: &Attribution, run: &Run) -> Option<String> {
    match (run, &attr.merged, &attr.refusal) {
        (Run::Error(_), _, _) => None,
        (Run::Refused, None, Some(_)) => None,
        (Run::Clean(b), Some(m), _) if b == m => None,
        (Run::Clean(_), Some(_), _) => {
            Some("the driver's result differs from the in-process merge".to_string())
        }
        (Run::Clean(_), None, _) => {
            Some("git merged cleanly but the format refused in process".to_string())
        }
        (Run::Refused, Some(_), _) => {
            Some("git left the file unmerged but the format merged in process".to_string())
        }
        (Run::Refused, None, None) => Some("the in-process run has no outcome".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(merged: Option<&[u8]>, refusal: Option<&str>) -> Attribution {
        Attribution {
            format: "toy".into(),
            applied: vec![],
            refusal: refusal.map(str::to_string),
            downgrades: vec![],
            merged: merged.map(<[u8]>::to_vec),
        }
    }

    #[test]
    fn agreement_and_each_kind_of_disagreement() {
        assert!(disagreement(&attr(None, Some("toy.R1")), &Run::Refused).is_none());
        assert!(disagreement(&attr(Some(b"x"), None), &Run::Clean(b"x".to_vec())).is_none());
        assert!(disagreement(&attr(Some(b"x"), None), &Run::Clean(b"y".to_vec())).is_some());
        assert!(disagreement(&attr(None, Some("toy.R1")), &Run::Clean(b"y".to_vec())).is_some());
        assert!(disagreement(&attr(Some(b"x"), None), &Run::Refused).is_some());
    }
}
