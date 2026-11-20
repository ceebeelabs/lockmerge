# 0006. False-merge gate in CI

- Status: Proposed
- Date: 2026-11-20

## Context

The one unacceptable outcome is a silently wrong lockfile. The owner's targets
(2026-11-20): zero wrong merges is a hard gate; v1 ships at 60% or more of
reachable Cargo.lock conflicts (stretch 80%), from at least 40 reachable
conflicts with a 95% Wilson lower bound of at least 50%; agent-style conflicts
have a separate bar of 90% or more. A corpus gives only a bound (about 3/N at
95% for zero failures in N cases), and a gate that can be weakened or can
silently check nothing gives false comfort. Early pull requests will be below
the targets.

## Decision

`lockmerge-bench check` runs as a required CI job on every pull request and
fails when any of these hold:

1. any case is `resolved-wrong` or `error` (crash, timeout, bad output, or a
   swapped-order disagreement), unconditionally. A waiver applies only during
   classification, to a reachable result that passes L0 and L1 but differs at
   L2; it never exempts an `error` or a `resolved-wrong`;
2. the reachable resolution rate (`resolved-correct` among reachable cases where
   plain git conflicted, over all such cases) is below the committed `baseline.toml` for the
   ecosystem. The first baseline is the rate measured when the benchmark first
   runs; the baseline only ratchets up;
3. the case count or reachable count is below the baseline's minimums, a
   fixture's SHA-256 differs from its `case.toml`, or a recomputed
   reachable/re-resolved class differs from `case.toml`;
4. `resolved-unverified` exceeds the baseline's `max_unverified`; each new one
   needs the owner's review;
5. the built-in oracle self-test fails: stub mergers that return `ours` or
   `base` must be classed `resolved-wrong` on a case where that is wrong.

The gate is the count of false merges, not a rate. Runtime is reported, not
gated. Changes to `baseline.toml`, waivers, or the check need the owner's
review through CODEOWNERS.

The v1 targets are checked only by `lockmerge-bench check --release v1` before
a release: for real Cargo, at least 40 reachable conflicts, a reachable rate of
60% or more and a 95% Wilson lower bound of 50% or more; for agent-style, a
rate of 90% or more. It is not a per-PR check.

The scheduled real-tool audit is not a required check, but it is part of the
safety argument: it checks every `resolved-unverified` result, and an audit
failure on one counts as a wrong merge (a false merge) and is reported as such.

## Considered options

- Gate on a false-merge rate threshold: lets one wrong merge through at scale.
- Gate on the targets on every PR: blocks all early work.
- Count gate plus ratcheting baseline plus integrity checks plus a release check
  (chosen): any wrong merge fails, progress is locked in, and deleting or
  editing fixtures or the classifier cannot raise the score unnoticed.
- Also gate runtime: rejected for v1, CI timing is noisy.

## Consequences

- A genuine wrong merge blocks every pull request until fixed, by design.
- Raising the rate is a two-file change (code and baseline).
- The corpus is a sample; the per-rule accept and refuse tests (standing rule 3)
  remain the evidence of safety.
- Reaching a target on a small reachable sample is weak evidence; the report
  prints a confidence lower bound beside each rate, and the release check
  enforces a minimum sample.
- Needs a CODEOWNERS file and a required-check setting on the repository.

## References

- [docs/02-oracle.md](../02-oracle.md), section 5
- [ADR 0005](0005-what-the-benchmark-counts-as-correct.md)
- [Issue #11](https://github.com/ceebeelabs/lockmerge/issues/11)
