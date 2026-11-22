//! The metrics of docs/02-oracle.md section 5, over the results of one
//! ecosystem.

use crate::attribution::Attribution;
use crate::classify::{Class, GitOutcome, Kind};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct CaseResult {
    pub id: String,
    pub kind: Kind,
    pub git: GitOutcome,
    pub class: Class,
    pub detail: String,
    pub millis: u128,
    /// The merged bytes equal `expected` byte for byte.
    pub exact: bool,
    pub attribution: Option<Attribution>,
    /// The case holds a reviewed waiver.
    pub waived: bool,
    /// The case is a merge from a pull request's own history (ADR 0021).
    pub pr_branch: bool,
}

/// The 95% Wilson score lower bound of `successes` out of `n`. Zero when
/// `n` is zero.
pub fn wilson_lower(successes: usize, n: usize) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let z = 1.959_963_984_540_054_f64;
    let (s, n) = (successes as f64, n as f64);
    let p = s / n;
    let centre = p + z * z / (2.0 * n);
    let spread = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt();
    ((centre - spread) / (1.0 + z * z / n)).max(0.0)
}

#[derive(Debug, Default)]
pub struct Metrics {
    pub n: usize,
    pub reachable: usize,
    pub re_resolved: usize,
    /// Reachable cases where plain git conflicts.
    pub c_r: usize,
    /// Re-resolved cases where plain git conflicts.
    pub c_x: usize,
    /// Reachable, plain git conflicted, lockmerge resolved correctly.
    pub s_r: usize,
    pub classes: BTreeMap<&'static str, usize>,
    pub git_outcomes: BTreeMap<&'static str, usize>,
    pub false_merges: usize,
    pub errors: usize,
    pub regressions: usize,
    /// Cases where plain git is clean and correct: the denominator of the
    /// regression rate (ADR 0017).
    pub git_clean_correct: usize,
    pub git_clean_wrong: usize,
    /// Resolved-correct over all reachable cases, git-clean included.
    pub resolved_correct_reachable: usize,
    pub exact: usize,
    pub unverified: usize,
    pub waived: Vec<String>,
    pub re_refused_correct: usize,
    pub re_unverified: usize,
    pub re_wrong: usize,
    pub millis_median: u128,
    pub millis_p95: u128,
    pub millis_max: u128,
    pub applied: BTreeMap<String, usize>,
    pub refusals: BTreeMap<String, usize>,
    pub downgrades: BTreeMap<String, usize>,
    /// The PR-branch group (ADR 0021): cases, reachable cases, reachable
    /// cases where plain git conflicts, and of those resolved correctly.
    pub pb_n: usize,
    pub pb_reachable: usize,
    pub pb_c_r: usize,
    pub pb_s_r: usize,
}

impl Metrics {
    /// `S_r / C_r`, with the 95% Wilson lower bound.
    pub fn reachable_rate(&self) -> Option<f64> {
        (self.c_r > 0).then(|| self.s_r as f64 / self.c_r as f64)
    }

    pub fn reachable_lower_bound(&self) -> f64 {
        wilson_lower(self.s_r, self.c_r)
    }

    /// `S_r / (C_r + C_x)`.
    pub fn all_conflicts_rate(&self) -> Option<f64> {
        let d = self.c_r + self.c_x;
        (d > 0).then(|| self.s_r as f64 / d as f64)
    }
}

pub fn compute(results: &[CaseResult]) -> Metrics {
    let mut m = Metrics {
        n: results.len(),
        ..Metrics::default()
    };
    let mut times: Vec<u128> = Vec::new();
    for r in results {
        *m.classes.entry(r.class.name()).or_default() += 1;
        *m.git_outcomes.entry(r.git.name()).or_default() += 1;
        times.push(r.millis);
        let conflict = r.git == GitOutcome::Conflict;
        if r.pr_branch {
            m.pb_n += 1;
            if r.kind == Kind::Reachable {
                m.pb_reachable += 1;
                if conflict {
                    m.pb_c_r += 1;
                    m.pb_s_r += usize::from(r.class == Class::ResolvedCorrect);
                }
            }
        }
        match r.kind {
            Kind::Reachable => {
                m.reachable += 1;
                if conflict {
                    m.c_r += 1;
                    if r.class == Class::ResolvedCorrect {
                        m.s_r += 1;
                    }
                }
                if r.class == Class::ResolvedCorrect {
                    m.resolved_correct_reachable += 1;
                }
            }
            Kind::ReResolved => {
                m.re_resolved += 1;
                if conflict {
                    m.c_x += 1;
                }
                match r.class {
                    Class::RefusedCorrect => m.re_refused_correct += 1,
                    Class::ResolvedUnverified => m.re_unverified += 1,
                    Class::ResolvedWrong => m.re_wrong += 1,
                    _ => {}
                }
            }
        }
        match r.class {
            Class::ResolvedWrong => m.false_merges += 1,
            Class::Error => m.errors += 1,
            Class::ResolvedUnverified => m.unverified += 1,
            Class::ResolvedWaived => m.waived.push(r.id.clone()),
            _ => {}
        }
        if r.git == GitOutcome::CleanWrong {
            m.git_clean_wrong += 1;
        }
        if r.git == GitOutcome::CleanCorrect {
            m.git_clean_correct += 1;
            if r.class == Class::Refused {
                m.regressions += 1;
            }
        }
        if r.exact {
            m.exact += 1;
        }
        if let Some(a) = &r.attribution {
            for rule in &a.applied {
                *m.applied.entry(rule.clone()).or_default() += 1;
            }
            if let Some(code) = &a.refusal {
                *m.refusals.entry(code.clone()).or_default() += 1;
            }
            for code in &a.downgrades {
                *m.downgrades.entry(code.clone()).or_default() += 1;
            }
        }
    }
    times.sort_unstable();
    if let Some(max) = times.last() {
        m.millis_max = *max;
        m.millis_median = times[times.len() / 2];
        let idx = ((times.len() as f64) * 0.95).ceil() as usize;
        m.millis_p95 = times[idx.clamp(1, times.len()) - 1];
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(kind: Kind, git: GitOutcome, class: Class) -> CaseResult {
        CaseResult {
            id: "x".into(),
            kind,
            git,
            class,
            detail: String::new(),
            millis: 10,
            exact: false,
            attribution: None,
            waived: false,
            pr_branch: false,
        }
    }

    #[test]
    fn wilson_matches_known_values() {
        // 5 of 7: the document says the interval reaches from about 36%.
        let lb = wilson_lower(5, 7);
        assert!((lb - 0.359).abs() < 0.005, "{lb}");
        assert_eq!(wilson_lower(0, 0), 0.0);
        // Zero successes has a zero lower bound; all successes of 40 stays below 1.
        assert_eq!(wilson_lower(0, 10), 0.0);
        let all = wilson_lower(40, 40);
        assert!(all > 0.91 && all < 1.0, "{all}");
    }

    #[test]
    fn denominators_follow_section_five() {
        let results = vec![
            r(
                Kind::Reachable,
                GitOutcome::Conflict,
                Class::ResolvedCorrect,
            ),
            r(Kind::Reachable, GitOutcome::Conflict, Class::Refused),
            // Git-clean cases are never in S_r or C_r.
            r(
                Kind::Reachable,
                GitOutcome::CleanCorrect,
                Class::ResolvedCorrect,
            ),
            r(Kind::Reachable, GitOutcome::CleanCorrect, Class::Refused),
            r(
                Kind::ReResolved,
                GitOutcome::Conflict,
                Class::RefusedCorrect,
            ),
            r(
                Kind::ReResolved,
                GitOutcome::Conflict,
                Class::ResolvedUnverified,
            ),
            r(
                Kind::Reachable,
                GitOutcome::CleanWrong,
                Class::ResolvedWrong,
            ),
        ];
        let m = compute(&results);
        assert_eq!((m.n, m.reachable, m.re_resolved), (7, 5, 2));
        assert_eq!((m.c_r, m.c_x, m.s_r), (2, 2, 1));
        assert_eq!(m.reachable_rate(), Some(0.5));
        assert_eq!(m.all_conflicts_rate(), Some(0.25));
        assert_eq!(m.false_merges, 1);
        assert_eq!((m.regressions, m.git_clean_correct), (1, 2));
        assert_eq!(m.git_clean_wrong, 1);
        assert_eq!(m.resolved_correct_reachable, 2);
        assert_eq!(
            (m.re_refused_correct, m.re_unverified, m.re_wrong),
            (1, 1, 0)
        );
    }

    #[test]
    fn empty_corpus_has_no_rate() {
        let m = compute(&[]);
        assert_eq!(m.reachable_rate(), None);
        assert_eq!(m.reachable_lower_bound(), 0.0);
    }

    #[test]
    fn runtime_percentiles() {
        let mut v = Vec::new();
        for ms in 1..=20u128 {
            let mut c = r(Kind::Reachable, GitOutcome::Conflict, Class::Refused);
            c.millis = ms;
            v.push(c);
        }
        let m = compute(&v);
        assert_eq!((m.millis_max, m.millis_p95), (20, 19));
    }

    #[test]
    fn the_pr_branch_group_is_counted_apart() {
        let mut a = r(
            Kind::Reachable,
            GitOutcome::Conflict,
            Class::ResolvedCorrect,
        );
        a.pr_branch = true;
        let mut b = r(Kind::Reachable, GitOutcome::Conflict, Class::Refused);
        b.pr_branch = true;
        let c = r(
            Kind::Reachable,
            GitOutcome::Conflict,
            Class::ResolvedCorrect,
        );
        let m = compute(&[a, b, c]);
        assert_eq!((m.pb_n, m.pb_reachable, m.pb_c_r, m.pb_s_r), (2, 2, 2, 1));
        assert_eq!((m.c_r, m.s_r), (3, 2));
    }
}
