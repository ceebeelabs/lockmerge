# 0007. Fixture layout and extraction

- Status: Accepted
- Date: 2026-11-20

## Context

Standing rule 7 says workers never download repositories and tests run offline;
a separate extraction step saves each real merge as four files. Fixtures must be
exact bytes with recorded provenance and a compatible license, and the dataset
must stay small. The driver reads manifests from git objects (ADR 0003), so a
case needs more than four files: the harness replays each case in a throwaway
git repository that holds the manifests too. The owner also wants an
agent-style corpus built from public repositories, which needs cargo to write
lockfiles.

## Decision

Fixtures live at `tests/fixtures/<ecosystem>/<owner>-<repo>/<merge-sha12>/` as
`base`, `ours`, `theirs`, `expected` (neutral names, exact bytes), a
`manifests/` directory with the manifests of base, ours and theirs (always,
including workspace members and path-dependency manifests), and `case.toml`.
`case.toml` records the repository URL, SPDX license, full SHAs of the merge,
its parents and the base, the lockfile path, the class (reachable or
re-resolved, with evidence), SHA-256 of each file, the extractor version and an
optional reviewed waiver. `tests/fixtures/.gitattributes` sets `-text -diff`.
License texts sit next to the repository directory. `INDEX.toml` lists every
considered merge with skip reasons.

Extraction is `lockmerge-bench extract`, run by hand by the owner side with
network access, from a `sources.toml` that pins each repository to a commit. It
uses the `git` binary. `survey` counts candidates first. The corpus is capped
per repository and by file size, with a total byte budget enforced by a test.
A repository whose license is outside the standing-rule list is not extracted
without the owner's sign-off. The orchestrator's repository and its history are
never used as benchmark data.

The agent-style corpus (`tests/fixtures/cargo-agent/`) is built owner-side by
`lockmerge-bench synth` from public MIT or Apache-2.0 repositories only, never
from a private or orchestrator repository: two branches from one base each add
a different crate with cargo; `expected` is the lockfile after both adds. Its
`case.toml` records the commands, cargo version, crate versions and date.

**Exception to standing rule 4 (owner-approved, 2026-11-20).** Standing rule 4
forbids regenerating fixtures with a package manager. For
`tests/fixtures/cargo-agent/` only, the bytes are cargo's output, written once
by an owner-side run and committed unchanged. Workers never run cargo to write
fixtures, and the exception covers no other directory.

## Considered options

- Store patches against base: smaller, but not exact bytes.
- Download in CI: violates rule 7.
- Git LFS: extra infrastructure; offline tests need the content.
- Committed exact files with caps (chosen).
- Agent-style cases from private repositories: rejected by the owner.

## Consequences

- Clone size grows by an estimated 15 to 35 MB; the byte budget keeps it
  visible.
- Neutral names keep scanners and Cargo from treating fixtures as live
  lockfiles.
- Rule 4 has a recorded, narrowly scoped exception for one corpus.
- Reducing large fixtures is deferred until the validity oracle is trusted.
- Extracting manifests for workspace members and path dependencies is part of
  the extractor, and its test must cover it.

## References

- [docs/02-oracle.md](../02-oracle.md), section 6
- [ADR 0003](0003-merge-driver-contract.md)
- [Issue #11](https://github.com/ceebeelabs/lockmerge/issues/11)
