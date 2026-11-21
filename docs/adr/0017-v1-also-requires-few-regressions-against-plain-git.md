# 0017. v1 also requires few regressions against plain git (amends ADR 0005)

- Status: Proposed
- Date: 2026-11-21

## Context

ADR 0005 defines the regression count: cases where plain git merges cleanly and correctly and lockmerge refuses. It reports the count with a target of 0, but neither the per-PR gate nor the v1 release check gates it. On the corpus, plain git merged 75 of 75 clean cases correctly and lockmerge (main after PR #71) refused 68 of them. A user who installs lockmerge today gets worse merges than with plain git, and every gate still passes.

## Decision

1. `lockmerge-bench check --release v1` additionally requires, on each Cargo corpus, regressions to be at most 5% of `git-clean-correct` cases, compared exactly with integers (regressions times 20 is at most the `git-clean-correct` count). A corpus with no `git-clean-correct` case has no regression and passes this check.
2. The per-PR gate is unchanged (zero wrong merges, zero errors, the rate ratchet), and gains a regression ratchet: `baseline.toml` has a required integer `max_regressions`, and the gate fails if the regression count exceeds it. The value may only go down; raising it needs the owner's review, like the other baseline values.
3. The first value for a real corpus is the count measured when it is extracted, as for the rate.
4. docs/02-oracle.md section 5 is updated to match.

## Considered options

- Gate the release at 0 regressions: the strictest, but one refusal of an unusual clean case would block a release; 5% leaves room while the rules grow.
- Leave regressions as a report: the status quo, which lets a release be worse than plain git.
- Make `max_regressions` optional in `baseline.toml`: a missing key would silently remove the ratchet, so it is required.

## Consequences

The v1 release is blocked until lockmerge merges at least 95% of the cases plain git merges correctly. The rules that accept git-clean cases (or a fast path that defers to git's result when it is provably the same) become part of the v1 work. Existing `baseline.toml` files must add `max_regressions`. The agent-style corpus is not subject to the release check, as its cases are conflicts by construction.

## References

- ADR 0005, docs/02-oracle.md section 5, issue #77.
