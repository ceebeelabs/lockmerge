# lockmerge charter

What lockmerge is for, the rule it never breaks, and the research that comes
before any code. The work is planned as GitHub issues sized for parallel
workers and built with Daedalus, a GitHub-first orchestrator for coding
agents.

## Goal

A single Rust binary, installed as a git merge driver, that merges lockfiles
by meaning rather than by lines:

- `Cargo.lock`
- `uv.lock`, `poetry.lock`
- `pnpm-lock.yaml`, `package-lock.json`, `yarn.lock` (classic and berry)
- `go.sum`
- `Gemfile.lock`

## The rule

**Combine what is provably unambiguous, refuse everything else, never
guess.** When the driver can't prove a merge is correct, it exits non-zero
and leaves git's normal conflict markers, so the outcome is no worse than
today. A silently wrong lockfile is the one unacceptable outcome.

## Why

- A long-standing pain:
  [rust-lang/cargo#2302](https://github.com/rust-lang/cargo/issues/2302)
  shows a clean textual merge leaving `Cargo.lock` with a dependency entry
  but no matching `[[package]]` section.
  [rust-lang/cargo#7070](https://github.com/rust-lang/cargo/pull/7070)
  redesigned the lockfile format partly to reduce merge conflicts.
- Projects still work around it by hand:
  [A2-ai/miniextendr#1662](https://github.com/A2-ai/miniextendr/issues/1662)
  (2026) proposes shipping a hand-written shell merge driver for
  `Cargo.lock` in project templates.
- Parallel coding agents make it worse: workers adding dependencies at the
  same time collide on lockfiles constantly.
- The result is measurable: replaying real merge histories gives a numeric
  score that CI can gate on.

## Research phase

No implementation until this phase is done. Each part below ends in a
document in `docs/`. Each document ends in the ADRs (see
[docs/adr/](adr/README.md)) for the decisions it makes.

### A. Prior art: `docs/00-prior-art.md`

- Search crates.io, lib.rs, GitHub and awesome-merge-drivers for any
  lockfile-aware merge driver from the last 18 months, in any language.
- Evaluate the known projects: derived-merge, cargo-merge-driver,
  pnpm/merge-driver, npm-merge-driver and its forks, cargo-git, mergiraf.
- Run mergiraf on 3 to 5 real `Cargo.lock` and `pnpm-lock.yaml` conflicts.
  Does it produce valid lockfiles?
- Write down derived-merge's "provability" rules precisely. Which ones
  generalize beyond npm?
- What do the package managers already do on their own (pnpm, yarn berry, uv,
  Cargo)? Where a manager already repairs its own lockfile, lockmerge adds
  less there.
- End with a go, narrow or no-go call, with reasons.

### B. Git merge driver mechanics: `docs/01-git-driver-mechanics.md`

- The driver contract: the `%O %A %B %L %P` placeholders, exit codes,
  `.gitattributes` and `merge.<name>.driver`, and behavior during rebase,
  cherry-pick and `git merge-file`.
- Installing per repository and globally, ideally as one command.
- Server-side merges (the GitHub web UI, merge queues) ignore custom drivers:
  what is the fallback?
- Whether the driver can be pure file-in, file-out, with no libgit2 or
  gitoxide.

### C. Per-format semantics: `docs/formats/<format>.md`

One document per format, in this order: `Cargo.lock`, `uv.lock`,
`pnpm-lock.yaml`, `package-lock.json`, `go.sum`, `yarn.lock`, `poetry.lock`,
`Gemfile.lock`. Each covers:

- The source of truth: the parser in the package manager's own source.
- Format versions in use (for example Cargo lockfile v3 and v4, pnpm
  lockfile 6 and 9, yarn classic and berry, uv's schema versions).
- The data model: packages, versions, sources, checksums, dependency edges,
  workspace members, metadata and hash fields.
- What a provably safe merge means for the format, and the list of conflicts
  that must be refused (for example the same package at different resolved
  versions on each side, or a checksum mismatch).
- Exact serialization: ordering, quoting, indentation, trailing newline,
  header comments. Unchanged files must round-trip byte for byte.
- The command that checks a merged lockfile is valid, and whether it needs
  network access.
- Known pitfalls, such as the dangling reference in cargo#2302.

### D. Benchmark: `docs/02-oracle.md`

- 10 to 20 public repositories per ecosystem with heavy lockfile churn.
- A replay harness: find merge commits whose parents both changed the
  lockfile, merge again with plain git and with lockmerge, and classify each
  result as resolved-correct, resolved-wrong, refused or git-clean.
- "Correct" means the merged lockfile equals the committed resolution after
  normalization, or passes the format's validity check, or both.
- Metrics: resolution rate, false-merge rate (must be 0) and runtime.
- How it runs in CI without hammering registries, the dataset size, and how
  fixtures are stored (as extracted base, ours, theirs and expected files).

### E. Architecture: `docs/03-architecture.md`

- The crate layout. A starting point: `lockmerge-core` (the three-way engine
  and the rules for what is provable), one crate or module per format
  implementing a common trait (parse, model, merge three-way, serialize),
  `lockmerge-cli` and `lockmerge-bench`.
- The format trait boundary, so each format can be built independently.
- How a refusal is explained to the user in plain language.
- Parsing libraries that preserve formatting for TOML, YAML and JSON.

### Backlog: `docs/04-backlog.md`

A seed backlog of 25 to 35 issues, each with a title, a short description,
acceptance criteria tied to tests or the benchmark, dependencies and a size
(S, M or L). The benchmark harness and the core trait come first, so the
per-format work can then run in parallel.

## Constraints

- Rust, a single static binary, and no runtime dependency on any package
  manager for merging. The benchmark's validity checks may call the real
  tools.
- Conservative by default: refuse and fall back to git.
- Out of scope for v1: resolving genuine version conflicts by re-running a
  resolver, editing manifests, and network access during a merge.
- License: MIT OR Apache-2.0. Any code or test fixtures taken from other
  projects must have a compatible license.

## Done when

- The prior-art decision is made and justified with links.
- Every in-scope format has a semantics document with a concrete rule set for
  a safe merge and a list of refusals.
- The benchmark has a defined dataset, procedure and metrics.
- The backlog is ready to file as GitHub issues as written.
