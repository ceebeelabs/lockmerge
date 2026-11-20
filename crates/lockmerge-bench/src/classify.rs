//! Classification of one case (docs/02-oracle.md sections 3.2, 3.3 and 4).
//!
//! Everything here is a pure function of bytes. The replay (real `git
//! merge`) produces a [`Run`]; the oracle self-test feeds stub mergers
//! through the same functions.

use crate::oracle::Oracle;
use crate::util::sha256_hex;

/// Whether some merge could in principle have produced `expected`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Reachable,
    ReResolved,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Reachable => "reachable",
            Kind::ReResolved => "re-resolved",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "reachable" => Some(Kind::Reachable),
            "re-resolved" => Some(Kind::ReResolved),
            _ => None,
        }
    }
}

/// A reviewed waiver, bound to one normalized result (section 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiver {
    pub reason: String,
    pub result_sha256: String,
}

/// The ids of `expected` that are in none of the three inputs, sorted. The
/// class is by rule, never by hand (ADR 0005).
pub fn foreign_ids(
    o: &dyn Oracle,
    base: &[u8],
    ours: &[u8],
    theirs: &[u8],
    expected: &[u8],
) -> Result<Vec<String>, String> {
    let known: std::collections::BTreeSet<String> = [base, ours, theirs]
        .iter()
        .map(|b| o.ids(b))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    Ok(o.ids(expected)?
        .into_iter()
        .filter(|id| !known.contains(id))
        .collect())
}

pub fn kind_of(foreign: &[String]) -> Kind {
    if foreign.is_empty() {
        Kind::Reachable
    } else {
        Kind::ReResolved
    }
}

/// What lockmerge's merge of one case did, read from git.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Run {
    /// The path was left unmerged (driver exit 1).
    Refused,
    /// Git completed the merge; this is the lockfile it wrote.
    Clean(Vec<u8>),
    /// Crash, timeout, unexpected state or no output.
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    ResolvedCorrect,
    ResolvedWrong,
    ResolvedUnverified,
    ResolvedWaived,
    RefusedCorrect,
    Refused,
    Error,
}

impl Class {
    pub const ALL: [Class; 7] = [
        Class::ResolvedCorrect,
        Class::ResolvedWrong,
        Class::ResolvedUnverified,
        Class::ResolvedWaived,
        Class::RefusedCorrect,
        Class::Refused,
        Class::Error,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Class::ResolvedCorrect => "resolved-correct",
            Class::ResolvedWrong => "resolved-wrong",
            Class::ResolvedUnverified => "resolved-unverified",
            Class::ResolvedWaived => "resolved-waived",
            Class::RefusedCorrect => "refused-correct",
            Class::Refused => "refused",
            Class::Error => "error",
        }
    }
}

/// The plain `git merge` outcome, shown beside lockmerge's class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitRun {
    Conflict,
    Clean(Vec<u8>),
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitOutcome {
    Conflict,
    CleanCorrect,
    CleanWrong,
    CleanUnverified,
    Error,
}

impl GitOutcome {
    pub fn name(self) -> &'static str {
        match self {
            GitOutcome::Conflict => "git-conflict",
            GitOutcome::CleanCorrect => "git-clean-correct",
            GitOutcome::CleanWrong => "git-clean-wrong",
            GitOutcome::CleanUnverified => "git-clean-unverified",
            GitOutcome::Error => "git-error",
        }
    }
}

/// Everything the classifier knows about a case.
pub struct CaseView<'a> {
    pub kind: Kind,
    pub expected: &'a [u8],
    pub waiver: Option<&'a Waiver>,
}

/// A class with the reason, for the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub class: Class,
    pub detail: String,
}

fn verdict(class: Class, detail: impl Into<String>) -> Verdict {
    Verdict {
        class,
        detail: detail.into(),
    }
}

/// L0, L1 and (for reachable cases) L2 on one result. `Ok(true)` is correct;
/// `Ok(false)` is valid but unprovable (re-resolved); `Err` is the reason it
/// is wrong, with `waived` set when a waiver covers it.
enum Judged {
    Correct,
    Unverified,
    Waived,
    Wrong(String),
}

fn judge(o: &dyn Oracle, case: &CaseView, bytes: &[u8]) -> Judged {
    if let Err(e) = o.canonical(bytes) {
        return Judged::Wrong(format!("L0 canonical: {e}"));
    }
    if let Err(e) = o.valid(bytes) {
        return Judged::Wrong(format!("L1 structural: {e}"));
    }
    match case.kind {
        Kind::ReResolved => Judged::Unverified,
        Kind::Reachable => {
            let got = match o.normalize(bytes) {
                Ok(g) => g,
                Err(e) => return Judged::Wrong(format!("L2 normalize result: {e}")),
            };
            let want = match o.normalize(case.expected) {
                Ok(w) => w,
                Err(e) => return Judged::Wrong(format!("L2 normalize expected: {e}")),
            };
            if got == want {
                return Judged::Correct;
            }
            match case.waiver {
                Some(w) if w.result_sha256 == sha256_hex(got.as_bytes()) => Judged::Waived,
                _ => Judged::Wrong("L2: differs from expected".to_string()),
            }
        }
    }
}

/// The swap check (section 3.3): both refuse, or both give equal results.
fn swap_disagreement(o: &dyn Oracle, run: &Run, swapped: &Run) -> Option<String> {
    match (run, swapped) {
        (Run::Error(e), _) | (_, Run::Error(e)) => Some(e.clone()),
        (Run::Refused, Run::Refused) => None,
        (Run::Clean(a), Run::Clean(b)) => {
            if a == b {
                return None;
            }
            match (o.normalize(a), o.normalize(b)) {
                // Equality after normalization must not hide a result that
                // fails L0 or L1: both must pass before they count as equal.
                (Ok(x), Ok(y)) if x == y => [a, b]
                    .iter()
                    .any(|r| o.canonical(r).is_err() || o.valid(r).is_err())
                    .then(|| "swap check: the results are equal only after normalization and one fails L0 or L1".to_string()),
                _ => Some("swap check: the two orders give different results".to_string()),
            }
        }
        _ => Some("swap check: one order refuses and the other merges".to_string()),
    }
}

/// Classifies lockmerge's run of one case, given the swapped run.
pub fn classify(o: &dyn Oracle, case: &CaseView, run: &Run, swapped: &Run) -> Verdict {
    if let Some(why) = swap_disagreement(o, run, swapped) {
        return verdict(Class::Error, why);
    }
    match run {
        Run::Error(e) => verdict(Class::Error, e.clone()),
        Run::Refused => match case.kind {
            Kind::Reachable => verdict(Class::Refused, "refused"),
            Kind::ReResolved => verdict(Class::RefusedCorrect, "re-resolved case refused"),
        },
        Run::Clean(bytes) => match judge(o, case, bytes) {
            Judged::Correct => verdict(Class::ResolvedCorrect, "equals expected"),
            Judged::Unverified => verdict(Class::ResolvedUnverified, "valid; nothing proves it"),
            Judged::Waived => verdict(Class::ResolvedWaived, "reviewed waiver"),
            Judged::Wrong(why) => verdict(Class::ResolvedWrong, why),
        },
    }
}

/// Classifies the plain `git merge` run.
pub fn classify_git(o: &dyn Oracle, case: &CaseView, run: &GitRun) -> GitOutcome {
    match run {
        GitRun::Conflict => GitOutcome::Conflict,
        GitRun::Error(_) => GitOutcome::Error,
        GitRun::Clean(bytes) => match judge(o, case, bytes) {
            Judged::Correct => GitOutcome::CleanCorrect,
            Judged::Unverified => GitOutcome::CleanUnverified,
            Judged::Waived | Judged::Wrong(_) => GitOutcome::CleanWrong,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::CargoOracle;

    const HEAD: &str = "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n";

    fn lock(names: &[&str]) -> Vec<u8> {
        // `app` depends on every name; each is a registry package.
        let mut s = String::from(HEAD);
        s.push_str("[[package]]\nname = \"app\"\nversion = \"0.1.0\"\n");
        if !names.is_empty() {
            s.push_str("dependencies = [\n");
            for n in names {
                s.push_str(&format!(" \"{n}\",\n"));
            }
            s.push_str("]\n");
        }
        for n in names {
            s.push_str(&format!(
                "\n[[package]]\nname = \"{n}\"\nversion = \"1.0.0\"\nsource = \"registry+x\"\nchecksum = \"{n}\"\n"
            ));
        }
        s.into_bytes()
    }

    fn view(expected: &[u8], kind: Kind) -> CaseView<'_> {
        CaseView {
            kind,
            expected,
            waiver: None,
        }
    }

    #[test]
    fn reachable_and_re_resolved_by_rule() {
        let (b, o, t) = (lock(&["pa"]), lock(&["pa", "pb"]), lock(&["pa", "pc"]));
        let both = lock(&["pa", "pb", "pc"]);
        assert!(
            foreign_ids(&CargoOracle, &b, &o, &t, &both)
                .unwrap()
                .is_empty()
        );
        let fresh = lock(&["pa", "pb", "pc", "pd"]);
        assert_eq!(
            foreign_ids(&CargoOracle, &b, &o, &t, &fresh).unwrap(),
            vec!["pd 1.0.0 (registry+x)".to_string()]
        );
        assert_eq!(kind_of(&[]), Kind::Reachable);
        assert_eq!(kind_of(&["pd".to_string()]), Kind::ReResolved);
    }

    #[test]
    fn reachable_correct_wrong_refused() {
        let expected = lock(&["pa", "pb", "pc"]);
        let c = view(&expected, Kind::Reachable);
        let same = Run::Clean(expected.clone());
        assert_eq!(
            classify(&CargoOracle, &c, &same, &same).class,
            Class::ResolvedCorrect
        );
        // Valid and canonical, but it dropped `c`: wrong, by L2.
        let dropped = Run::Clean(lock(&["pa", "pb"]));
        let v = classify(&CargoOracle, &c, &dropped, &dropped);
        assert_eq!(
            (v.class, v.detail.as_str()),
            (Class::ResolvedWrong, "L2: differs from expected")
        );
        // Not canonical: wrong, by L0.
        let mut sloppy = expected.clone();
        sloppy.push(b'\n');
        let v = classify(
            &CargoOracle,
            &c,
            &Run::Clean(sloppy.clone()),
            &Run::Clean(sloppy),
        );
        assert_eq!(v.class, Class::ResolvedWrong);
        assert!(v.detail.starts_with("L0"));
        // A refusal on a reachable case is a missed opportunity, not wrong.
        assert_eq!(
            classify(&CargoOracle, &c, &Run::Refused, &Run::Refused).class,
            Class::Refused
        );
    }

    #[test]
    fn re_resolved_refusal_is_correct_and_resolution_is_unverified() {
        let expected = lock(&["pa", "pb", "pc", "pd"]);
        let c = view(&expected, Kind::ReResolved);
        assert_eq!(
            classify(&CargoOracle, &c, &Run::Refused, &Run::Refused).class,
            Class::RefusedCorrect
        );
        let merged = Run::Clean(lock(&["pa", "pb", "pc"]));
        assert_eq!(
            classify(&CargoOracle, &c, &merged, &merged).class,
            Class::ResolvedUnverified
        );
        // An invalid resolution of a re-resolved case is wrong.
        let dangling = Run::Clean(b"<<<<<<< x\n".to_vec());
        assert_eq!(
            classify(&CargoOracle, &c, &dangling, &dangling).class,
            Class::ResolvedWrong
        );
    }

    #[test]
    fn waiver_covers_exactly_one_normalized_result() {
        let expected = lock(&["pa", "pb", "pc"]);
        let other = lock(&["pa", "pb"]);
        let digest = sha256_hex(CargoOracle.normalize(&other).unwrap().as_bytes());
        let w = Waiver {
            reason: "reviewed".into(),
            result_sha256: digest,
        };
        let c = CaseView {
            kind: Kind::Reachable,
            expected: &expected,
            waiver: Some(&w),
        };
        let r = Run::Clean(other);
        assert_eq!(
            classify(&CargoOracle, &c, &r, &r).class,
            Class::ResolvedWaived
        );
        // Any other mismatch on the same case stays wrong.
        let third = Run::Clean(lock(&["pa", "pc"]));
        assert_eq!(
            classify(&CargoOracle, &c, &third, &third).class,
            Class::ResolvedWrong
        );
        // A waiver never exempts an error.
        let e = Run::Error("boom".into());
        assert_eq!(classify(&CargoOracle, &c, &e, &r).class, Class::Error);
    }

    #[test]
    fn swap_check_catches_order_dependence() {
        let expected = lock(&["pa", "pb", "pc"]);
        let c = view(&expected, Kind::Reachable);
        let good = Run::Clean(expected.clone());
        // One order merges, the other refuses.
        assert_eq!(
            classify(&CargoOracle, &c, &good, &Run::Refused).class,
            Class::Error
        );
        // Both merge, with different results.
        let other = Run::Clean(lock(&["pa", "pb"]));
        assert_eq!(
            classify(&CargoOracle, &c, &good, &other).class,
            Class::Error
        );
    }

    #[test]
    fn swap_equality_after_normalization_needs_both_results_canonical() {
        let expected = lock(&["pa", "pb", "pc"]);
        let c = view(&expected, Kind::Reachable);
        let good = Run::Clean(expected.clone());
        let mut sloppy = expected.clone();
        sloppy.push(b'\n');
        let bad = Run::Clean(sloppy);
        for (a, b) in [(&good, &bad), (&bad, &good)] {
            let v = classify(&CargoOracle, &c, a, b);
            assert_eq!(v.class, Class::Error, "{}", v.detail);
        }
    }

    #[test]
    fn plain_git_outcomes() {
        let expected = lock(&["pa", "pb", "pc"]);
        let c = view(&expected, Kind::Reachable);
        assert_eq!(
            classify_git(&CargoOracle, &c, &GitRun::Conflict),
            GitOutcome::Conflict
        );
        assert_eq!(
            classify_git(&CargoOracle, &c, &GitRun::Clean(expected.clone())),
            GitOutcome::CleanCorrect
        );
        assert_eq!(
            classify_git(&CargoOracle, &c, &GitRun::Clean(lock(&["pa", "pb"]))),
            GitOutcome::CleanWrong
        );
        let rr = view(&expected, Kind::ReResolved);
        assert_eq!(
            classify_git(&CargoOracle, &rr, &GitRun::Clean(lock(&["pa"]))),
            GitOutcome::CleanUnverified
        );
    }
}
