# 0005. What the benchmark counts as correct

- Status: Proposed
- Date: 2026-11-20

## Context

The benchmark replays real merges and must say whether lockmerge's result is
right. The charter allows "equals the committed resolution after
normalization, or passes the format's validity check, or both". A real
package-manager check needs the network, and a valid lockfile can still be the
wrong one (a dropped bump). The committed resolution can be non-canonical,
invalid, or contain package ids on neither side of the merge because the author
re-ran a resolver (5 of the 12 real Cargo.lock conflicts in the prior-art run).
No driver can reproduce those.

## Decision

Each case is classified at extraction as **reachable** (every package id of the
committed resolution is in base, ours or theirs) or **re-resolved** (otherwise).
The class is recomputed at every run.

For a reachable case, a result is correct when all three hold:

1. It parses and re-serializing it reproduces its bytes (a canonical fixed
   point).
2. It passes an offline structural validity predicate written independently of
   the merge code.
3. It equals the committed resolution after a per-format normalization that
   erases only differences the format document proves meaningless.

A reachable result that is valid but differs counts as wrong unless a person
records a reviewed waiver with a reason in `case.toml`; waived cases are
reported separately and never count as correct.

For a re-resolved case, a refusal is correct. A resolution that fails 1 or 2 is
wrong. A resolution that passes both is `resolved-unverified`: reported, never
counted as correct, limited by a baseline maximum, and each new one reviewed by
the owner. The scheduled real-tool audit must check every `resolved-unverified`
result; any failure there counts as a wrong merge (a false merge) and is
reported as one.

Re-resolved cases are excluded from the resolution-rate denominator and reported
separately. The headline metric is the resolution rate on reachable conflicts,
with its 95% Wilson lower bound; the rate over all conflicts is reported beside
it.

Real package-manager checks are not a per-PR gate. Fixtures whose committed
resolution fails the structural check are excluded and listed.

## Considered options

- Byte equality only: calls correct merges wrong when the committed file is
  non-canonical, and ignores validity.
- Validity only (real tool): accepts silently dropped changes and needs the
  network.
- Normalized equality plus offline validity for every case: treats unreachable
  resolutions as failures lockmerge cannot avoid, or rewards guessing.
- Reachable/re-resolved split with normalized equality and offline validity
  (chosen): the ground truth is a real commit, no network, and the headline
  measures what a driver can achieve.
- Fail every resolution of a re-resolved case as wrong: the safest, but blocks
  legitimate resolutions. Rejected by the owner in favor of `resolved-unverified`
  with the audit.

## Consequences

- Each format document must list its normalization, its validity predicate, and
  its id definition; the predicate must not share code with the merger.
- Some correct merges fail until waived. When unsure, fail.
- The headline excludes re-resolved cases, so the report must always show their
  count; otherwise the headline could be inflated by classifying hard cases away.
  The class is computed by rule, not chosen by hand.
- The package-manager audit needs a separate, non-required job that runs against
  every `resolved-unverified` result.

## References

- [docs/02-oracle.md](../02-oracle.md), sections 3 and 4
- [docs/00-prior-art.md](../00-prior-art.md) section 7
- [Issue #11](https://github.com/ceebeelabs/lockmerge/issues/11)
