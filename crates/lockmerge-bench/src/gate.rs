//! The gate (docs/02-oracle.md section 5, ADR 0006): `baseline.toml`, the
//! per-ecosystem checks, the size budget and the `--release v1` checks.

use crate::metrics::Metrics;
use std::fs;
use std::path::Path;
use toml_edit::DocumentMut;

/// Raw bytes of committed fixtures per corpus. This settles #24: the first
/// estimate of docs/02-oracle.md section 6.4 was about 198 MB for the three
/// planned corpora against a 100 MB budget. The budgets below add up to
/// 100 MB, and the per-repository caps and `max_file_bytes` of the corpora
/// follow from them. A corpus over its budget fails `check`.
pub const MEGABYTE: u64 = 1_000_000;
pub const BUDGETS: [(&str, u64); 4] = [
    ("cargo", 50 * MEGABYTE),
    ("cargo-agent", 15 * MEGABYTE),
    ("uv", 35 * MEGABYTE),
    // pnpm has no oracle and no budget yet; its corpus needs the owner to
    // set one (and to revisit the others).
    ("pnpm", 0),
];
pub const TOTAL_BUDGET: u64 = 100 * MEGABYTE;

pub fn budget_of(ecosystem: &str) -> u64 {
    BUDGETS
        .iter()
        .find(|(e, _)| *e == ecosystem)
        .map_or(0, |(_, b)| *b)
}

/// `tests/fixtures/<ecosystem>/baseline.toml`.
#[derive(Debug, PartialEq, Eq)]
pub struct Baseline {
    pub min_cases: usize,
    pub min_reachable: usize,
    /// The ratchet: the reachable rate must be at least
    /// `min_rate_resolved / min_rate_of`, compared exactly.
    pub min_rate_resolved: usize,
    pub min_rate_of: usize,
    pub max_unverified: usize,
    /// The regression ratchet (ADR 0017): git-clean-correct cases that
    /// lockmerge refuses. It may only go down.
    pub max_regressions: usize,
}

const KEYS: [&str; 6] = [
    "min_cases",
    "min_reachable",
    "min_rate_resolved",
    "min_rate_of",
    "max_unverified",
    "max_regressions",
];

pub fn parse_baseline(text: &str) -> Result<Baseline, String> {
    let doc: DocumentMut = text
        .parse()
        .map_err(|e| format!("baseline.toml does not parse: {e}"))?;
    for (k, _) in doc.as_table().iter() {
        if !KEYS.contains(&k) {
            return Err(format!("baseline.toml: unknown key `{k}`"));
        }
    }
    let get = |k: &str| -> Result<usize, String> {
        doc.get(k)
            .and_then(|i| i.as_integer())
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| format!("baseline.toml: `{k}` must be a non-negative integer"))
    };
    let b = Baseline {
        min_cases: get("min_cases")?,
        min_reachable: get("min_reachable")?,
        min_rate_resolved: get("min_rate_resolved")?,
        min_rate_of: get("min_rate_of")?,
        max_unverified: get("max_unverified")?,
        max_regressions: get("max_regressions")?,
    };
    if b.min_rate_of == 0 || b.min_rate_resolved > b.min_rate_of {
        return Err("baseline.toml: the rate must be `min_rate_resolved` of `min_rate_of`, with `min_rate_of` at least 1".into());
    }
    Ok(b)
}

pub fn load_baseline(path: &Path) -> Result<Baseline, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_baseline(&text)
}

/// The per-PR gate for one ecosystem, given its metrics. Returns every
/// failure. Integrity failures (hashes, classes, budget) are added by the
/// caller, which has the files.
pub fn gate(
    ecosystem: &str,
    m: &Metrics,
    b: &Baseline,
    case_ids_bad: &[(String, String)],
) -> Vec<String> {
    let mut f = Vec::new();
    for (id, why) in case_ids_bad {
        f.push(format!("{ecosystem}: {id}: {why}"));
    }
    if m.false_merges > 0 {
        f.push(format!("{ecosystem}: {} wrong merge(s)", m.false_merges));
    }
    if m.errors > 0 {
        f.push(format!("{ecosystem}: {} error(s)", m.errors));
    }
    // Exact integer comparison: s/c >= num/den.
    if m.s_r * b.min_rate_of < b.min_rate_resolved * m.c_r
        || (m.c_r == 0 && b.min_rate_resolved > 0)
    {
        f.push(format!(
            "{ecosystem}: the reachable resolution rate {}/{} is below the baseline {}/{}",
            m.s_r, m.c_r, b.min_rate_resolved, b.min_rate_of
        ));
    }
    if m.n < b.min_cases {
        f.push(format!(
            "{ecosystem}: {} cases, the baseline minimum is {}",
            m.n, b.min_cases
        ));
    }
    if m.reachable < b.min_reachable {
        f.push(format!(
            "{ecosystem}: {} reachable cases, the baseline minimum is {}",
            m.reachable, b.min_reachable
        ));
    }
    if m.unverified > b.max_unverified {
        f.push(format!(
            "{ecosystem}: {} resolved-unverified results, the baseline maximum is {}",
            m.unverified, b.max_unverified
        ));
    }
    if m.regressions > b.max_regressions {
        f.push(format!(
            "{ecosystem}: {} regressions against plain git, the baseline maximum is {}",
            m.regressions, b.max_regressions
        ));
    }
    f
}

/// The checks of `--release v1` (section 5). `cargo` is the real corpus.
/// A missing corpus is a failure: nothing is assumed.
pub fn release_v1(cargo: Option<&Metrics>, agent: Option<&Metrics>) -> Vec<String> {
    let mut f = Vec::new();
    match cargo {
        None => f.push("release v1: there is no cargo corpus".to_string()),
        Some(m) => {
            if m.c_r < 40 {
                f.push(format!(
                    "release v1: cargo has {} reachable conflicts, at least 40 are required",
                    m.c_r
                ));
            }
            // S_r / C_r >= 60%, exactly.
            if m.c_r == 0 || m.s_r * 5 < m.c_r * 3 {
                f.push(format!(
                    "release v1: cargo reachable rate {}/{} is below 60%",
                    m.s_r, m.c_r
                ));
            }
            if m.reachable_lower_bound() < 0.5 {
                f.push(format!(
                    "release v1: cargo 95% Wilson lower bound {:.1}% is below 50%",
                    m.reachable_lower_bound() * 100.0
                ));
            }
            // ADR 0017: regressions / git-clean-correct <= 5%, exactly.
            if m.regressions * 20 > m.git_clean_correct {
                f.push(format!(
                    "release v1: cargo has {} regressions against plain git out of {} git-clean-correct cases, more than 5%",
                    m.regressions, m.git_clean_correct
                ));
            }
        }
    }
    match agent {
        None => f.push("release v1: there is no cargo-agent corpus".to_string()),
        Some(m) => {
            if m.c_r == 0 || m.s_r * 10 < m.c_r * 9 {
                f.push(format!(
                    "release v1: cargo-agent reachable rate {}/{} is below 90%",
                    m.s_r, m.c_r
                ));
            }
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_BASELINE: &str = "min_cases = 0\nmin_reachable = 0\nmin_rate_resolved = 0\nmin_rate_of = 1\nmax_unverified = 0\nmax_regressions = 0\n";

    fn metrics(s_r: usize, c_r: usize) -> Metrics {
        Metrics {
            n: c_r,
            reachable: c_r,
            c_r,
            s_r,
            ..Metrics::default()
        }
    }

    #[test]
    fn budgets_add_up_to_the_total() {
        assert_eq!(BUDGETS.iter().map(|(_, b)| b).sum::<u64>(), TOTAL_BUDGET);
    }

    #[test]
    fn baseline_parses_and_refuses_malformed() {
        let b = parse_baseline(EMPTY_BASELINE).unwrap();
        assert_eq!((b.min_cases, b.min_rate_of), (0, 1));
        assert!(parse_baseline("min_cases = 0\n").is_err());
        // The regression ratchet is required: a baseline without it is refused.
        assert!(parse_baseline(&EMPTY_BASELINE.replace("max_regressions = 0\n", "")).is_err());
        assert!(parse_baseline(&format!("{EMPTY_BASELINE}extra = 1\n")).is_err());
        assert!(
            parse_baseline(&EMPTY_BASELINE.replace("min_rate_of = 1", "min_rate_of = 0")).is_err()
        );
        assert!(
            parse_baseline(&EMPTY_BASELINE.replace("min_cases = 0", "min_cases = -1")).is_err()
        );
    }

    #[test]
    fn empty_corpus_passes_the_empty_baseline() {
        let b = parse_baseline(EMPTY_BASELINE).unwrap();
        assert!(gate("cargo", &Metrics::default(), &b, &[]).is_empty());
    }

    #[test]
    fn each_condition_fails_the_gate() {
        let b = parse_baseline(
            "min_cases = 4\nmin_reachable = 4\nmin_rate_resolved = 1\nmin_rate_of = 2\nmax_unverified = 1\nmax_regressions = 1\n",
        )
        .unwrap();
        assert!(gate("cargo", &metrics(2, 4), &b, &[]).is_empty());
        // Wrong merges and errors, unconditionally.
        let mut m = metrics(2, 4);
        m.false_merges = 1;
        assert_eq!(gate("cargo", &m, &b, &[]).len(), 1);
        let mut m = metrics(2, 4);
        m.errors = 1;
        assert_eq!(gate("cargo", &m, &b, &[]).len(), 1);
        // Below the ratchet: 1 of 4 against 1 of 2.
        assert_eq!(gate("cargo", &metrics(1, 4), &b, &[]).len(), 1);
        // Below the minimums.
        assert!(gate("cargo", &metrics(1, 2), &b, &[]).len() >= 2);
        // Too many unverified.
        let mut m = metrics(2, 4);
        m.unverified = 2;
        assert_eq!(gate("cargo", &m, &b, &[]).len(), 1);
        // Regressions: one is allowed, two are not.
        let mut m = metrics(2, 4);
        m.regressions = 1;
        assert!(gate("cargo", &m, &b, &[]).is_empty());
        m.regressions = 2;
        let f = gate("cargo", &m, &b, &[]);
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].contains("regressions"));
        // An integrity problem.
        let bad = [("cargo/r/x".to_string(), "hash".to_string())];
        assert_eq!(gate("cargo", &metrics(2, 4), &b, &bad).len(), 1);
    }

    #[test]
    fn a_positive_baseline_fails_when_nothing_conflicts() {
        let b = parse_baseline(
            "min_cases = 0\nmin_reachable = 0\nmin_rate_resolved = 1\nmin_rate_of = 2\nmax_unverified = 0\nmax_regressions = 0\n",
        )
        .unwrap();
        assert_eq!(gate("cargo", &Metrics::default(), &b, &[]).len(), 1);
    }

    #[test]
    fn release_v1_caps_regressions_at_five_percent_of_git_clean_correct() {
        let agent = metrics(9, 10);
        let with = |regressions, git_clean_correct| Metrics {
            regressions,
            git_clean_correct,
            ..metrics(30, 40)
        };
        // 3 of 60 is 5%: accepted. 4 of 60 is above: refused.
        assert!(release_v1(Some(&with(3, 60)), Some(&agent)).is_empty());
        let f = release_v1(Some(&with(4, 60)), Some(&agent));
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].contains("regressions"));
        // The observed state of main: 68 of 75.
        assert!(!release_v1(Some(&with(68, 75)), Some(&agent)).is_empty());
        // No git-clean-correct case and no regression: nothing to fail.
        assert!(release_v1(Some(&with(0, 0)), Some(&agent)).is_empty());
        // The agent corpus is not subject to it.
        assert!(
            release_v1(
                Some(&metrics(30, 40)),
                Some(&Metrics {
                    regressions: 9,
                    git_clean_correct: 9,
                    ..metrics(9, 10)
                })
            )
            .is_empty()
        );
    }

    #[test]
    fn release_v1_requires_sample_rate_and_lower_bound() {
        let agent = metrics(9, 10);
        // 60% of 40 with a lower bound below 50%.
        let weak = metrics(24, 40);
        let f = release_v1(Some(&weak), Some(&agent));
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].contains("Wilson"));
        // 30 of 40 = 75%: lower bound about 60%.
        assert!(release_v1(Some(&metrics(30, 40)), Some(&agent)).is_empty());
        // Too few conflicts, even at 100%.
        assert!(!release_v1(Some(&metrics(10, 10)), Some(&agent)).is_empty());
        // Below 60% exactly: 23 of 40.
        assert!(!release_v1(Some(&metrics(23, 40)), Some(&agent)).is_empty());
        // The agent bar is 90%.
        assert!(!release_v1(Some(&metrics(30, 40)), Some(&metrics(8, 10))).is_empty());
        // Missing corpora fail.
        assert_eq!(release_v1(None, None).len(), 2);
        assert!(!release_v1(Some(&Metrics::default()), Some(&Metrics::default())).is_empty());
    }
}
