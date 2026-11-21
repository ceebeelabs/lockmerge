# Architecture decision records

lockmerge records its significant decisions as architecture decision records
(ADRs). Each research document in `docs/` ends in the ADRs for the decisions it
makes, and the owner accepts or rejects them.

An ADR is a short note on one significant decision: context, decision,
considered options and consequences. Write one when a decision is hard to
reverse, shapes more than one format or crate, or is one a reviewer would
reasonably ask "why?" about. The ADR goes in the same pull request that makes
the decision.

## Files

- One file per decision, named `NNNN-short-title.md`, numbered in the order
  decided. Use the next free number.
- Start from [template.md](template.md).
- Titles never carry a status.

## Status

Status is one of `Proposed`, `Accepted`, `Rejected`, `Deprecated`, or
`Superseded by [NNNN](NNNN-title.md)`. Date is the date of the last status
change (YYYY-MM-DD).

## Who decides

- Authors only ever write `Proposed`, and only in an open pull request.
- The owner accepts, rejects or asks for a revision before that pull request
  merges. The status and date are then updated in the same pull request, so
  nothing merges as `Proposed`.
- Rejected ADRs are kept.
- An accepted ADR is not rewritten, except for its status, date and
  superseding link. A changed decision is a new ADR that supersedes the old
  one.

## Index

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions as ADRs in `docs/adr/` | Accepted |
| [0002](0002-scope-of-lockmerge-v1.md) | Scope of lockmerge v1: narrow (Cargo.lock, uv.lock, pnpm-lock.yaml) | Accepted |
| [0003](0003-merge-driver-contract.md) | The merge driver contract: inputs, exit codes, markers on refusal, manifests from git objects | Accepted |
| [0004](0004-server-side-merge-fallback.md) | Server-side merge fallback: detection with `lockmerge check` in CI | Accepted |
| [0005](0005-what-the-benchmark-counts-as-correct.md) | What the benchmark counts as correct | Accepted |
| [0006](0006-false-merge-gate-in-ci.md) | False-merge gate in CI | Accepted |
| [0007](0007-fixture-layout-and-extraction.md) | Fixture layout and extraction | Accepted |
| [0008](0008-safe-merge-and-refusal-rules-for-cargo-lock.md) | Safe-merge and refusal rules for Cargo.lock | Accepted |
| [0009](0009-opt-in-cargo-validation-mode.md) | Opt-in cargo validation mode (not v1) | Accepted |
| [0010](0010-how-the-driver-finds-the-commits.md) | How the driver finds the commits (amends ADR 0003) | Accepted |
| [0011](0011-crate-layout.md) | Crate layout | Accepted |
| [0012](0012-format-trait-and-merge-result-type.md) | Format trait and the merge result type | Accepted |
| [0013](0013-refusal-and-reporting-model.md) | Refusal and reporting model | Accepted |
| [0014](0014-parsers-are-read-only.md) | Parsers are read-only; `toml_edit` for TOML | Accepted |
| [0015](0015-r15-is-a-whole-file-refusal.md) | R15 is a whole-file refusal (corrects the context of ADR 0013) | Accepted |
| [0016](0016-how-the-cargo-manifest-rules-read-and-compare-manifests.md) | How the Cargo manifest rules read and compare manifests | Accepted |
| [0017](0017-v1-also-requires-few-regressions-against-plain-git.md) | v1 also requires few regressions against plain git (amends ADR 0005) | Accepted |
| [0018](0018-read-patch-exactly-and-accept-one-sided-path-crate-version-moves.md) | Read `[patch]` exactly and accept one-sided path-crate version moves (amends ADR 0016) | Proposed |
