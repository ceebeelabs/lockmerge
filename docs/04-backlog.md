# 04. Backlog

A seed backlog of 33 entries, as the charter asks. Each has a short
description, acceptance criteria tied to tests or the benchmark, dependencies
and a size (S, M or L). Entries 1 to 9 are already filed as issues; their
title, dependencies and size come from the issues, and their scope is not
rewritten here. Entries 10 to 33 are marked "to file". The benchmark harness
and the core trait come first, so the per-format work can then run in
parallel. Dependencies name an entry by number and title or by issue.

## Filed

### 1. Workspace and lockmerge-core: types, traits, engine and Rust CI ([#31](https://github.com/ceebeelabs/lockmerge/issues/31))

Size M. Depends on: none (settles #29 itself).
The Cargo workspace and `lockmerge-core` per `docs/03-architecture.md`
sections 1 to 3.
Acceptance: unit tests with a toy format give every core rule an accept and a
refuse case; an unregistered rule in a `Proof` is an internal error; the
`rust` CI job (fmt, clippy, tests) passes.

### 2. lockmerge-core testkit: fixture runner and the rule-pair lint ([#32](https://github.com/ceebeelabs/lockmerge/issues/32))

Size S. Depends on: #31.
A fixture runner for the ADR 0007 case layout and a lint that every rule and
code has accept and refuse fixtures.
Acceptance: the toy format's fixtures pass the runner; removing one fixture
of a rule makes the lint fail.

### 3. lockmerge-env: commit identification and manifests from git ([#33](https://github.com/ceebeelabs/lockmerge/issues/33))

Size M. Depends on: #31.
Commit identification per ADR 0010 and a `Git` implementation of `Manifests`.
Acceptance: integration tests with real merge, rebase, cherry-pick and revert
find the right commits or give `Unavailable`; CI runs on the oldest and
newest supported git.

### 4. lockmerge-cli: the merge-driver and install commands ([#34](https://github.com/ceebeelabs/lockmerge/issues/34))

Size M. Depends on: #31, #33.
`lockmerge merge-driver` with exit codes 0, 1 and 255, markers on refusal,
and `lockmerge install`.
Acceptance: tests with a fake format cover each exit path, symmetry of
`(O, A, B)` and `(O, B, A)`, and a panic leaving `%A` unchanged; an
end-to-end real `git merge` runs through the installed driver.

### 5. lockmerge-bench: replay harness, metrics and the gate ([#35](https://github.com/ceebeelabs/lockmerge/issues/35))

Size L. Depends on: #31, #32, and #24 (settled inside this entry).
`lockmerge-bench check`, `extract` and `survey`, the metrics and the gate of
`docs/02-oracle.md`.
Acceptance: the oracle self-test with stub mergers passes; the `bench` CI job
passes offline on an empty corpus; `extract` is tested on a small temporary
repository.

### 6. lockmerge-cargo: parser, model, canonical writer and base rules ([#36](https://github.com/ceebeelabs/lockmerge/issues/36))

Size L. Depends on: #31, #32.
Read-only `toml_edit` parsing, the resolved model, the canonical writer for
versions 3 and 4, base rules and refusals R1 to R14.
Acceptance: every fixture lockfile round-trips byte for byte or is refused;
every rule and code has an accept and a refuse fixture pair that the testkit
lint checks.

### 7. lockmerge-cargo: manifest-assisted rules M1 to M3, C, D, F and B1 ([#37](https://github.com/ceebeelabs/lockmerge/issues/37))

Size L. Depends on: #36, #33.
The manifest-assisted rules of ADR 0008 decisions 4 to 6 and 10, with
manifest finding per `docs/formats/cargo-lock.md` section 4.5.
Acceptance: one accept and one refuse fixture pair per rule, including the
regression pairs written in ADR 0008; `Conflict` or `Unavailable` manifests
fall back as decision 10 says.

### 8. lockmerge-cargo: native-links proof R15 from the registry index cache ([#38](https://github.com/ceebeelabs/lockmerge/issues/38))

Size M. Depends on: #37.
Write `docs/formats/cargo-registry-index.md`, then implement R15 over the
registry index cache.
Acceptance: the fake `CARGO_HOME` tests cover cache present and links-free,
a git id, shared `links`, checksum mismatch, cache absent, unknown cache
format and several index directories.

### 9. Owner-side: extract the real Cargo.lock corpus and build the agent-style corpus ([#39](https://github.com/ceebeelabs/lockmerge/issues/39))

Size M. Depends on: #35. Owner only, never by workers.
Pin public repositories, run `survey` and `extract`, and hand-build the
agent-style cases.
Acceptance: at least 40 reachable `Cargo.lock` conflicts, about 100 cases in
all and at least 30 agent-style cases are committed unchanged; the first
`baseline.toml` is set from the measured rate.

## To file

### 10. L3 audit job: real Cargo on every resolved-unverified result (to file; [#23](https://github.com/ceebeelabs/lockmerge/issues/23), [#22](https://github.com/ceebeelabs/lockmerge/issues/22))

Size L. Depends on: 5 (lockmerge-bench), 9 (corpus).
The scheduled, networked L3 audit of `docs/02-oracle.md` section 4: real Cargo
checks each merged lockfile against the manifests of the merged tree, for
every `resolved-unverified` result. It must rebuild enough project files for
Cargo (restored source trees from the original commits, or recorded target
stubs, since manifests omit implicitly discovered targets) and list cases
whose manifests do not merge cleanly as unaudited for the owner to review.
Acceptance: a Cargo failure on a correctly prepared tree is reported as
`resolved-wrong`; a preparation failure is `audit-prep-failed` and never a
wrong merge; unaudited results count against `max_unverified`.

### 11. Measure the cost of rule C on both corpora (to file; [#25](https://github.com/ceebeelabs/lockmerge/issues/25))

Size S. Depends on: 7 (M1 to M3 rules), 9 (corpus).
Count rule C refusals under their own reason code on the real and the
agent-style corpus. If C blocks the 60% or 90% target, propose a narrower
proven rule as a new ADR amending ADR 0008.
Acceptance: the per-code refusal table shows C's count on each corpus; the
rate with and without C is printed side by side.

### 12. `lockmerge check` command (to file)

Size M. Depends on: 4 (CLI), 6 (cargo base rules).
A command that runs the same rules over a lockfile and its two parents after
the fact, for hosted merges that ran no driver (ADR 0004) and for the R5
manifest check when git objects were not available.
Acceptance: tests show it accepts a correct merge result and reports a
wrong one with the reason code, using the driver's fixtures.

### 13. `lockmerge doctor` (to file)

Size S. Depends on: 4 (CLI).
Check that the driver is installed in the repository, that `.gitattributes`
names it, and that git is new enough.
Acceptance: tests in temporary repositories cover installed, missing
attribute, missing config and old git.

### 14. Release packaging (to file)

Size M. Depends on: 4 (CLI).
A single static binary per platform, install instructions and a README quick
start.
Acceptance: CI builds the release binary and a clean-machine install test
runs `lockmerge install` and a merge; the README quick start is run as
written.

### 15. uv.lock format research (to file)

Size M. Depends on: 1 (core).
A format document with a rule set and refusal list, like
`docs/formats/cargo-lock.md`, plus ADRs as Proposed.
Acceptance: the owner accepts the document; every rule names an accept and a
refuse case; unverified points are marked.

### 16. lockmerge-uv: format crate (to file)

Size L. Depends on: 15 (uv.lock research), 2 (testkit).
Parser, model, writer and rules for `uv.lock`.
Acceptance: fixtures round-trip byte for byte; each rule has an accept and a
refuse fixture pair checked by the testkit lint.

### 17. pnpm-lock.yaml format research, including the YAML library choice (to file; YAML choice from [#28](https://github.com/ceebeelabs/lockmerge/issues/28))

Size M. Depends on: 1 (core).
A format document and rule set, and a decision on a span-preserving YAML
parser (whether saphyr spans are byte offsets is unverified,
`docs/03-architecture.md`).
Acceptance: the owner accepts the document and the library ADR; the parser's
license and tree size are stated.

### 18. lockmerge-pnpm: format crate (to file)

Size L. Depends on: 17 (pnpm research), 2 (testkit).
Parser, model, writer and rules for `pnpm-lock.yaml`.
Acceptance: fixtures round-trip byte for byte; each rule has an accept and a
refuse fixture pair checked by the testkit lint.

### 19. uv corpus (to file)

Size M. Depends on: 16 (uv crate), 5 (lockmerge-bench). Owner only.
Extract real `uv.lock` conflicts under the ADR 0005 and 0007 layout.
Acceptance: the corpus is committed unchanged with sources noted, and a
baseline is set.

### 20. pnpm corpus (to file; [#22](https://github.com/ceebeelabs/lockmerge/issues/22))

Size M. Depends on: 18 (pnpm crate), 5 (lockmerge-bench). Owner only.
Extract real `pnpm-lock.yaml` conflicts under the same layout.
Acceptance: as entry 19.

### 21. Fuzz the parse and engine paths (to file; [#28](https://github.com/ceebeelabs/lockmerge/issues/28))

Size M. Depends on: 6 (cargo base rules).
Fuzz parsers and the engine against the zero-wrong-merge gate.
Acceptance: a fuzz target runs in CI for a fixed time and finds no panic,
wrong merge or output that fails to re-parse; found inputs become fixtures.

### 22. Server-side merge fallback workflow (to file; ADR 0004)

Size M. Depends on: 12 (`lockmerge check`).
An example CI job for hosted merges (ADR 0004) that runs `lockmerge check`.
Acceptance: the example runs in a test repository and fails on a wrong
lockfile.

### 23. Check the Cargo behaviors marked unverified (to file; [#26](https://github.com/ceebeelabs/lockmerge/issues/26))

Size S. Depends on: 6 (cargo base rules). Owner only.
Run Cargo owner-side to confirm points the docs mark unverified: version
upgrade on rewrite (`docs/formats/cargo-lock.md` section 2), the 0.x
unification rule and the source sort order; also add line numbers to the
cargo and git source links.
Acceptance: each point is confirmed or corrected in the document, and a
fixture or test is added where a rule depends on it.

### 24. Shared span-splicing helper in core (to file; [#28](https://github.com/ceebeelabs/lockmerge/issues/28))

Size M. Depends on: 6 (cargo crate), 16 or 18 (a second format).
Extract byte-exact splicing once two formats need it.
Acceptance: both formats use it with unchanged round-trip and benchmark
results.

### 25. Pipeline helper for the keyed-packages shape (to file; [#28](https://github.com/ceebeelabs/lockmerge/issues/28))

Size S. Depends on: 6 (cargo crate), 16 or 18 (a second format).
A shared helper for formats whose lockfile is a keyed list of packages.
Acceptance: formats using it keep identical fixture and benchmark results.

### 26. Opt-in cargo validation mode, ADR 0009 (to file; [#26](https://github.com/ceebeelabs/lockmerge/issues/26), [#28](https://github.com/ceebeelabs/lockmerge/issues/28))

Size M. Depends on: 7 (M1 to M3 rules); needs a charter change first.
An opt-in mode that runs Cargo offline on the merged result; never on by
default.
Acceptance: the owner amends the charter; tests show a result Cargo rejects
is refused when the mode is on, and behavior is unchanged when it is off.

### 27. Machine-readable CLI report (to file; [#28](https://github.com/ceebeelabs/lockmerge/issues/28))

Size S. Depends on: 4 (CLI).
Output the `Report` as JSON for CI.
Acceptance: a test checks the fields for a merge and for a refusal with its
code.

### 28. Format research for the remaining formats (to file)

Size L. Depends on: 15 and 17 (the first two researched formats).
Research documents for `package-lock.json`, `go.sum`, `yarn.lock`,
`poetry.lock` and `Gemfile.lock`; the owner may split it per format when
filing.
Acceptance: each document has a rule set and a refusal list, and the owner
accepts it before any crate work starts.

### 29. `lockmerge resolve`: a second step (to file; [#26](https://github.com/ceebeelabs/lockmerge/issues/26))

Size L. Depends on: 7 (M1 to M3 rules); needs a charter change first.
A later extra step for conflicts the driver refuses; the charter currently
puts re-running a resolver out of scope for v1.
Acceptance: the owner decides the scope and amends the charter; then a design
document is accepted before any code.

### 30. Cargo.lock version 5 support (to file; [#26](https://github.com/ceebeelabs/lockmerge/issues/26))

Size M. Depends on: 6 (cargo base rules).
Add version 5 when Cargo stabilizes it; until then it is refused (ADR 0008
decision 1).
Acceptance: the format document is updated from Cargo's stable behavior, and
each changed rule has an accept and a refuse fixture pair.

### 31. Recover squash-merged cases from `refs/pull/*/head` (to file; [#22](https://github.com/ceebeelabs/lockmerge/issues/22))

Size M. Depends on: 5 (lockmerge-bench). Owner only.
Fetch pull request refs in `extract` so squash-merged conflicts, which have no
merge commit, can become cases.
Acceptance: `extract` is tested on a small temporary repository with a
squash-merged pull request ref.

### 32. Replay rebase and cherry-pick in the harness (to file; [#22](https://github.com/ceebeelabs/lockmerge/issues/22))

Size M. Depends on: 5 (lockmerge-bench), 3 (lockmerge-env).
Replay cases with `git rebase` and `git cherry-pick` as well as `git merge`,
including the unprobed revert, `rebase --apply` and stash paths, which fall
back (the `%S`, `%X` and `%Y` label shapes are not a stable git API).
Acceptance: the harness reports each operation separately and a case that
merges under one operation but differs under another is flagged.

### 33. Mergiraf as a comparison column in the benchmark (to file; [#22](https://github.com/ceebeelabs/lockmerge/issues/22))

Size S. Depends on: 5 (lockmerge-bench). Owner only.
Add mergiraf's result per case as a comparison column. It is GPL-3.0, so run
it as an external tool only and copy none of its code or data (`docs/00-prior-art.md` section 5).
Acceptance: the report shows the column when the tool is installed and omits
it otherwise; no mergiraf code or data is in the repository.
