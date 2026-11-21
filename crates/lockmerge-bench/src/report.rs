//! Printing the metrics (docs/02-oracle.md section 5). The rate and its lower
//! bound are always printed side by side.

use crate::check::{CheckOutcome, EcoReport};
use crate::classify::{Class, GitOutcome};
use lockmerge_core::Format;
use lockmerge_core::engine::{CORE_CODES, CORE_RULES};
use std::collections::BTreeMap;
use std::fmt::Write;

fn pct(x: Option<f64>) -> String {
    x.map_or("n/a".to_string(), |v| format!("{:.1}%", v * 100.0))
}

/// The formats an ecosystem directory uses: `cargo` and `cargo-agent` both
/// use the format `cargo`.
fn format_of(ecosystem: &str) -> &str {
    ecosystem.strip_suffix("-agent").unwrap_or(ecosystem)
}

/// Rows of `counts`, zero-filled with `registry` and with any key that the
/// registry does not know listed last.
fn zero_filled(
    registry: Vec<String>,
    counts: &BTreeMap<String, usize>,
) -> Vec<(String, usize, bool)> {
    let mut rows: Vec<(String, usize, bool)> = registry
        .iter()
        .map(|k| (k.clone(), counts.get(k).copied().unwrap_or(0), true))
        .collect();
    for (k, n) in counts {
        if !registry.contains(k) {
            rows.push((k.clone(), *n, false));
        }
    }
    rows
}

fn table(out: &mut String, title: &str, rows: &[(String, usize, bool)]) {
    let _ = writeln!(out, "  {title}:");
    if rows.is_empty() {
        let _ = writeln!(out, "    (no registered entries)");
    }
    for (k, n, known) in rows {
        let note = if *known {
            ""
        } else {
            "  (not in the registry)"
        };
        let _ = writeln!(out, "    {k:<28} {n}{note}");
    }
}

fn eco_section(out: &mut String, e: &EcoReport, formats: &[&dyn Format]) {
    let m = &e.metrics;
    let _ = writeln!(out, "== {} ==", e.name);
    let _ = writeln!(
        out,
        "  cases {} (reachable {}, re-resolved {}); fixtures {} bytes deduplicated (checked against the budget), {} bytes on disk",
        m.n, m.reachable, m.re_resolved, e.size.unique, e.size.raw
    );
    let _ = writeln!(
        out,
        "  Reachable resolution rate (headline): {}/{} = {}, 95% Wilson lower bound {:.1}%",
        m.s_r,
        m.c_r,
        pct(m.reachable_rate()),
        m.reachable_lower_bound() * 100.0
    );
    let _ = writeln!(
        out,
        "  Rate over all git conflicts: {}/{} = {}",
        m.s_r,
        m.c_r + m.c_x,
        pct(m.all_conflicts_rate())
    );
    let _ = writeln!(
        out,
        "  Re-resolved: refused-correct {}, resolved-unverified {}, resolved-wrong {}",
        m.re_refused_correct, m.re_unverified, m.re_wrong
    );
    let _ = writeln!(
        out,
        "  Coverage: {}/{} reachable resolved correctly; git-clean-wrong {}",
        m.resolved_correct_reachable, m.reachable, m.git_clean_wrong
    );
    let _ = writeln!(
        out,
        "  False merges {} (must be 0); errors {} (must be 0); regressions {} of {} git-clean-correct (target 0; v1 allows 5%); exact {}",
        m.false_merges, m.errors, m.regressions, m.git_clean_correct, m.exact
    );
    let _ = writeln!(
        out,
        "  Runtime per case: median {} ms, p95 {} ms, max {} ms",
        m.millis_median, m.millis_p95, m.millis_max
    );
    if let Some(b) = &e.baseline {
        let _ = writeln!(
            out,
            "  Baseline: rate >= {}/{}, cases >= {}, reachable >= {}, unverified <= {}, regressions <= {}",
            b.min_rate_resolved,
            b.min_rate_of,
            b.min_cases,
            b.min_reachable,
            b.max_unverified,
            b.max_regressions
        );
    }
    let _ = writeln!(out, "  lockmerge classes:");
    for c in Class::ALL {
        let _ = writeln!(
            out,
            "    {:<22} {}",
            c.name(),
            m.classes.get(c.name()).copied().unwrap_or(0)
        );
    }
    let _ = writeln!(out, "  plain git outcomes:");
    for g in [
        GitOutcome::Conflict,
        GitOutcome::CleanCorrect,
        GitOutcome::CleanWrong,
        GitOutcome::CleanUnverified,
        GitOutcome::Error,
    ] {
        let _ = writeln!(
            out,
            "    {:<22} {}",
            g.name(),
            m.git_outcomes.get(g.name()).copied().unwrap_or(0)
        );
    }
    if !m.waived.is_empty() {
        let _ = writeln!(
            out,
            "  Waived (reviewed, not counted as correct): {}",
            m.waived.join(", ")
        );
    }

    let fmt_id = format_of(&e.name);
    let mut rules: Vec<String> = CORE_RULES.iter().map(|r| r.id.to_string()).collect();
    let mut codes: Vec<String> = CORE_CODES.iter().map(|c| c.code.to_string()).collect();
    for f in formats.iter().filter(|f| f.id().0 == fmt_id) {
        rules.extend(f.rules().iter().map(|r| r.id.to_string()));
        codes.extend(f.codes().iter().map(|c| c.code.to_string()));
    }
    let has_format = formats.iter().any(|f| f.id().0 == fmt_id);
    if !has_format {
        let _ = writeln!(
            out,
            "  (no `{fmt_id}` format is registered in this build: rule and code counts cover the core registry only)"
        );
    }
    table(
        out,
        "Rules applied (zero-filled from the registries)",
        &zero_filled(rules, &m.applied),
    );
    table(
        out,
        "Refusals by code (zero-filled)",
        &zero_filled(codes.clone(), &m.refusals),
    );
    table(
        out,
        "Downgrades by code (zero-filled)",
        &zero_filled(codes, &m.downgrades),
    );

    // Why each case was refused for an unavailable input (R14), so the
    // causes can be counted and the common ones fixed.
    let unavailable: Vec<_> = e
        .results
        .iter()
        .filter_map(|r| {
            let a = r.attribution.as_ref()?;
            let code = a.refusal.as_deref()?;
            code.ends_with(".R14")
                .then_some((r, a.refusal_detail.as_deref()))
        })
        .collect();
    if !unavailable.is_empty() {
        let _ = writeln!(
            out,
            "  Refusals for an unavailable input (R14), with the cause:"
        );
        for (r, detail) in unavailable {
            let _ = writeln!(out, "    {}: {}", r.id, detail.unwrap_or("(no detail)"));
        }
    }

    let notable: Vec<_> = e
        .results
        .iter()
        .filter(|r| matches!(r.class, Class::ResolvedWrong | Class::Error))
        .collect();
    if !notable.is_empty() {
        let _ = writeln!(out, "  Failing cases:");
        for r in notable {
            let _ = writeln!(out, "    {} {}: {}", r.id, r.class.name(), r.detail);
        }
    }
}

pub fn render(outcome: &CheckOutcome, formats: &[&dyn Format]) -> String {
    let mut out = String::new();
    for e in &outcome.ecosystems {
        eco_section(&mut out, e, formats);
    }
    let _ = writeln!(
        out,
        "oracle self-test: {}",
        match &outcome.selftest {
            Ok(()) => "passed".to_string(),
            Err(e) => format!("FAILED: {e}"),
        }
    );
    if outcome.passed() {
        let _ = writeln!(out, "bench gate: PASSED");
    } else {
        let _ = writeln!(out, "bench gate: FAILED");
        for f in &outcome.failures {
            let _ = writeln!(out, "  - {f}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_zero_filled_and_strangers_flagged() {
        let mut counts = BTreeMap::new();
        counts.insert("a.X".to_string(), 2);
        counts.insert("z.Y".to_string(), 1);
        let rows = zero_filled(vec!["a.X".into(), "a.W".into()], &counts);
        assert_eq!(
            rows,
            vec![
                ("a.X".to_string(), 2, true),
                ("a.W".to_string(), 0, true),
                ("z.Y".to_string(), 1, false)
            ]
        );
    }

    #[test]
    fn r14_refusals_are_listed_with_their_cause_and_others_are_not() {
        use crate::attribution::Attribution;
        use crate::classify::Kind;
        use crate::metrics::compute;
        let result = |id: &str, code: &str, detail: &str| crate::metrics::CaseResult {
            id: id.into(),
            kind: Kind::Reachable,
            git: GitOutcome::Conflict,
            class: Class::Refused,
            detail: String::new(),
            millis: 1,
            exact: false,
            attribution: Some(Attribution {
                format: "toy".into(),
                applied: vec![],
                refusal: Some(code.into()),
                refusal_detail: Some(detail.into()),
                downgrades: vec![],
                merged: None,
            }),
            waived: false,
        };
        let results = vec![
            result("a", "toy.R14", "x [detail: no manifest]"),
            result("b", "toy.R15", "y [detail: two ids]"),
        ];
        let e = EcoReport {
            name: "toy".into(),
            metrics: compute(&results),
            baseline: None,
            results,
            size: crate::case::TreeSize::default(),
        };
        let mut out = String::new();
        eco_section(&mut out, &e, &[]);
        assert!(out.contains("    a: x [detail: no manifest]"), "{out}");
        assert!(!out.contains("two ids"), "{out}");
    }

    #[test]
    fn cargo_agent_uses_the_cargo_format() {
        assert_eq!(format_of("cargo-agent"), "cargo");
        assert_eq!(format_of("uv"), "uv");
    }
}
