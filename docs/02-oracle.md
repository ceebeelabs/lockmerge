# 02. Benchmark and replay harness

Research for charter section D. Checked on 2026-11-20. The decisions are
ADR 0005 (what the benchmark counts as correct), ADR 0006 (the false-merge
gate in CI) and ADR 0007 (fixture layout and extraction), all Proposed. Links:
[0005](adr/0005-what-the-benchmark-counts-as-correct.md),
[0006](adr/0006-false-merge-gate-in-ci.md),
[0007](adr/0007-fixture-layout-and-extraction.md).

## 1. Summary

- **Dataset:** two corpora. The *real* corpus is merge commits from public
  repositories, each saved as exact bytes (`base`, `ours`, `theirs`,
  `expected`, saved manifests, `case.toml`) in `tests/fixtures/`. The
  *agent-style* corpus is built by hand from public MIT or Apache repositories
  (section 6.3). Candidate repositories are in section 2. Targets are about 100
  real Cargo cases, 60 uv cases, and 30 agent-style Cargo cases. No repository
  of the orchestrator that drives this project, and no history from it, is ever
  benchmark data.
- **Reachable or re-resolved.** Every case is classified at extraction. It is
  *reachable* if every package id in `expected` appears in at least one of
  base, ours or theirs. Otherwise it is *re-resolved*: the person or tool
  that made the commit ran a resolver and picked ids neither side had. No
  driver can reproduce a re-resolved case, so refusing it is correct
  (section 3).
- **Replay:** for each case, build a throwaway git repository from the fixture
  and run real `git merge` without and with the lockmerge driver, then classify
  both (section 3). The replay covers `git merge` only; rebase and cherry-pick
  are covered by the driver's own tests.
- **Correct:** the result parses, is canonical, is structurally valid, and (for
  reachable cases) equals `expected` after a proof-based normalization
  (section 4). Real package-manager checks need the network, so they are a
  scheduled audit, not the per-PR gate. The audit checks every
  `resolved-unverified` result, and a failure there counts as a false merge.
- **Headline metric:** the resolution rate on *reachable* git conflicts, with a
  95% Wilson lower bound. The rate over *all* git conflicts is reported beside
  it (section 5).
- **Targets (owner, 2026-11-20):** v1 ships at 60% or more of reachable Cargo
  conflicts (stretch 80%), with at least 40 reachable real Cargo.lock conflicts
  and a 95% Wilson lower bound of at least 50%; and 90% or more on the
  agent-style corpus. Zero wrong merges is a hard gate. Per-PR CI enforces zero
  wrong merges, zero errors and a ratchet starting at the measured rate. The
  targets are release criteria, checked by `check --release v1` (section 5).
- **Extraction:** a subcommand of `lockmerge-bench`, run by hand with network
  access, never by workers (section 6). CI runs the benchmark offline
  (section 7).
- **Limit:** N cases with zero false merges bounds the true rate loosely
  (about 3/N at 95%). The gate is a regression tripwire. The proof of safety is
  the per-rule accept and refuse tests of standing rule 3.

## 2. Repositories

Method: GitHub API (`api.github.com/repos/<owner>/<repo>`), 2026-11-20, for
license (SPDX id as GitHub reports it), last push and archived flag; file sizes
from the contents API at the default branch. GitHub reports one license for
dual-licensed projects, so extraction must read the license files itself.
**Not measured:** lockfile churn (merge commits where both parents changed the
lockfile). The `survey` step (section 6) measures it; selection is by that
count, not by this list.

**Cargo candidates.** All were pushed between 2026-11-17 and 2026-11-20; none
is archived.

| Repository | GitHub license | `Cargo.lock` size | Notes |
| --- | --- | --- | --- |
| sharkdp/bat | Apache-2.0 | 72,881 B | Confirmed. 4 cases in the mergiraf run. |
| sharkdp/fd | Apache-2.0 | not measured | 3 cases in the mergiraf run. |
| lapce/lapce | Apache-2.0 | not measured | 4 cases in the mergiraf run. |
| rust-lang/cargo | Apache-2.0 | not measured | 1 case in the mergiraf run. |
| astral-sh/ruff | MIT | 137,594 B | Confirmed. |
| denoland/deno | MIT | 296,018 B | Confirmed. |
| nushell/nushell | MIT | not measured | |
| zellij-org/zellij | MIT | not measured | |
| alacritty/alacritty | Apache-2.0 | not measured | |
| oxc-project/oxc | MIT | not measured | |
| biomejs/biome | MIT | not measured | |
| swc-project/swc | Apache-2.0 | not measured | |
| tauri-apps/tauri | Apache-2.0 | not measured | |
| paradigmxyz/reth | NOASSERTION | 310,309 B | Lockfile confirmed. **License unclear** from the API; owner sign-off after reading its license files. Skipped for now. |

Excluded: meilisearch/meilisearch (NOASSERTION; license files not read).
Not examined: helix-editor/helix and zed-industries/zed (believed MPL-2.0 and
mixed; not verified). Both are outside the accepted licenses without sign-off
(standing rule 6).

**uv candidates.** All were pushed between 2026-11-17 and 2026-11-20; none is
archived.

| Repository | GitHub license | `uv.lock` size | Notes |
| --- | --- | --- | --- |
| pydantic/pydantic | MIT | 590,042 B | Confirmed. |
| pydantic/pydantic-ai | MIT | 1,852,742 B | Confirmed. Large: skipped by the size cap unless raised. |
| run-llama/llama_index | MIT | not measured | |
| crewAIInc/crewAI | MIT | 1,861,903 B | Confirmed. Large. |
| browser-use/browser-use | MIT | not measured | |
| docling-project/docling | MIT | not measured | |
| modelcontextprotocol/python-sdk | MIT | not measured | |
| openai/openai-agents-python | MIT | not measured | |
| apache/airflow | Apache-2.0 | not measured | |
| dagster-io/dagster | Apache-2.0 | not measured | |
| PrefectHQ/prefect | Apache-2.0 | not measured | |
| jlowin/fastmcp | Apache-2.0 | not measured | |
| langchain-ai/langchain | MIT | not measured | Monorepo with several lockfiles; the survey takes a path list. |
| astral-sh/ruff | MIT | 334,090 B | Confirmed. |

A committed `uv.lock` was verified only for the four marked "Confirmed".

**pnpm** (third in ADR 0002): not researched beyond the five vuejs/core cases
in the mergiraf run. Pick its repositories when pnpm work starts.

**Merge-style caveat.** Replaying merge commits only finds cases in
repositories whose history keeps them. Squash-merge repositories (common among
agent-driven uv projects) will have few. The survey ranks repositories by actual
count. Fetching `refs/pull/<n>/head` may recover squashed merges; this is
**not verified**, so it is a follow-up. The agent-style corpus (section 6.3)
exists partly because real history has few cases of the "two branches each add
a crate" shape.

**Seed cases.** The 17 cases of
[00-prior-art section 7](00-prior-art.md#7-mergiraf-run) (12 `Cargo.lock`,
5 `pnpm-lock.yaml`) are included by hash. Owner-side data on the 12 Cargo cases:
**5 are re-resolved** (the committed lockfile has ids on neither side, for
example fd's `clap 4.0.18`, and lapce `b998c1f3a5` with 47 new ids), and **7
are reachable**. The harness reproduces this split by rule (section 3); it is
not recorded by hand.

## 3. Replay procedure

### 3.1 Which merges are cases

A case is a merge commit `M` with exactly two parents `P1` and `P2`, where:

1. `git merge-base --all P1 P2` returns exactly one commit `B`. Criss-cross
   histories are skipped as `skipped:multi-base`: git merges them recursively,
   which a three-way driver cannot reproduce.
2. The lockfile path exists in `B`, `P1`, `P2` and `M`, and its blob differs
   between `B` and `P1`, and between `B` and `P2`. Otherwise
   `skipped:not-modify-modify`.
3. The `expected` blob passes the independent structural check (L1,
   section 4). Otherwise `skipped:expected-invalid`. Example: bat
   `571970f8ff28` lists `itertools 0.13.0` twice in the committed resolution.

Octopus merges are skipped. Every skip is listed with its reason in
`tests/fixtures/<ecosystem>/INDEX.toml`.

Mapping: `ours = P1`, `theirs = P2`, `base = B`, `expected = M`.

### 3.2 Reachable or re-resolved

Computed at extraction from the lockfile bytes, and recomputed by `check`.
A mismatch with `case.toml` fails the gate, so a case cannot be moved to the
easier class by editing.

- Let `ids(X)` be the set of package ids in lockfile `X`. The id is the key from
  section 4 (Cargo: name, version, source; uv: name, version, source; pnpm: the
  `packages` key).
- **Reachable:** `ids(expected)` is a subset of `ids(base) ∪ ids(ours) ∪
  ids(theirs)`.
- **Re-resolved:** otherwise. `case.toml` records the count of foreign ids and
  the first ten.

The test is on the id only. A package that keeps its id but changes `checksum`
or its dependency list is still reachable; those differences are checked by
equality in section 4. Reachable does **not** mean "some merge rule can produce
it". It only means the committed resolution is not out of reach by
construction. Cases the rules cannot yet produce are the refusal backlog.

### 3.3 The two merges

The driver (ADR 0003, Accepted) never reads the working tree or the index. It
reads the manifests of the base and both sides from git objects, using
`git cat-file` or `git show`, and refuses or falls back to lockfile-only rules
when it cannot identify the commits. Four loose files cannot supply that, so
the harness does not call `git merge-file` or the driver by hand. It replays
each case in a throwaway repository:

1. `git init` in a temp directory.
2. Commit `base`: the lockfile at its real path (`lockfile_path` in
   `case.toml`) and the saved base manifests at their repo-relative paths
   (section 6.2). Then create the `ours` and `theirs` branches from it, each
   commit holding the lockfile and the saved manifests of that side.
3. Check out `ours` and run `git merge theirs`, twice: once with no driver
   (git's text merge), and once with the lockmerge driver. For the driver run,
   the harness writes the `merge.lockmerge.*` keys to the repository config and
   `<lockfile name> merge=lockmerge` to `.git/info/attributes`. The driver
   command line is the one `lockmerge install` writes, taken from the same
   source in code and not retyped, including `%S %X %Y`. This matters because
   the driver identifies the base and the two sides from the `%S`, `%X` and
   `%Y` labels and the `GITHEAD_<sha>` environment variable that `git merge`
   sets: `MERGE_HEAD`, `REBASE_HEAD` and `CHERRY_PICK_HEAD` do not exist while
   the driver runs (probed on git 2.50.1). A decision under way to amend
   ADR 0003 on this point (issue #10) is the reason; the replay exercises that
   identification end to end and so fails if it breaks.
4. Repeat both runs with the sides swapped: check out `theirs` and merge
   `ours` (the swap check).

Real git decides which paths reach a driver, so a lockfile git resolves without
a content merge never reaches lockmerge, and the harness sees that too.

**Scope.** The replay covers `git merge` only. Rebase and cherry-pick put the
sides in different roles; the driver's own tests cover them.

The result of a lockmerge run is read from git, not from the driver: a path
git leaves unmerged is a refusal (exit 1, markers in the file, ADR 0003); a
failed merge (driver exit above 128) is an `error`; a clean merge gives the
lockfile blob from the merge result.

Costs: a few `git` processes per case. About 160 real cases at four merges each
is a few hundred merges on files up to 2 MB. I estimate seconds to a minute;
this is not measured.

Classification of lockmerge's result, exactly one per case:

| Class | Definition |
| --- | --- |
| `resolved-correct` | Reachable case. Exit 0 and the result is correct (section 4). |
| `resolved-wrong` | Exit 0 and the result is not correct (reachable), or fails L0 or L1 (re-resolved), or fails the L3 audit (`resolved-unverified` only). A false merge. |
| `resolved-unverified` | Re-resolved case. Exit 0 and the result passes L0 and L1. Nothing proves it right or wrong offline. Never counted as correct. |
| `resolved-waived` | Reachable case. Valid, differs from `expected`, and has a reviewed waiver (section 4). Not counted as correct. |
| `refused-correct` | Re-resolved case, and lockmerge refused. Correct by the owner's rule. |
| `refused` | Reachable case, and lockmerge refused with a reason code (R1 to R7 of the prior-art doc, plus new ones). A missed opportunity, not an error. |
| `error` | Crash, timeout (default 30 s), unexpected exit code, or no output. A gate failure, not counted as a false merge. |

Plain git outcome, reported as a column beside lockmerge's class:

| Plain git outcome | Meaning |
| --- | --- |
| `git-conflict` | Textual conflict. These cases form the denominators of the rates. |
| `git-clean-correct` | Clean, and correct by the same test. |
| `git-clean-wrong` | Clean but not correct: the cargo#2302 case, a dangling dependency after a "successful" text merge. This is where lockmerge adds most. For a re-resolved case, git-clean is reported as `git-clean-unverified`. |

**Regression count:** `git-clean-correct` cases that lockmerge refused. Target 0.

**Swap check.** The merge is also run with the sides swapped. Both runs must
refuse, or both must give results equal after normalization. A difference is
`error`. It catches order-dependent guessing.

## 4. What "correct" means

| Layer | Check | Needs | Gate? |
| --- | --- | --- | --- |
| L0 canonical | The result parses in lockmerge's model and re-serializing reproduces the bytes (a fixed point). | Nothing | Yes |
| L1 structural | Independent offline validity predicate (below). | Nothing | Yes |
| L2 equivalence | The result equals `expected` after normalization. **Reachable cases only.** | Nothing | Yes |
| L3 real tool | The real package manager accepts the merged lockfile against the manifests of the merged tree. | Network or warm cache, the tool | No per-PR gate; scheduled audit of every `resolved-unverified` result |

**Why L2 is the primary oracle for reachable cases.** `expected` is what a
person or package manager committed. L1 alone is not enough: a lockfile that
silently drops a version bump is valid. L3 alone is not enough: it checks
against the manifests, not against what the two branches intended, and needs
the network. **Why it does not apply to re-resolved cases:** `expected` there
contains ids no merge could know, so equality would only reward luck. For those
cases the only provable good outcome is a refusal; a resolution can be checked
for L0 and L1 only (`resolved-unverified`).

**The L3 audit of `resolved-unverified` (owner, 2026-11-20).** The scheduled
real-tool audit must check every `resolved-unverified` result. Any L3 failure
on one is a wrong merge: it is reported as `resolved-wrong` (a false merge) and
not as a softer class. A `resolved-unverified` result that the audit has not
yet checked stays `resolved-unverified`, counts against `max_unverified`
(section 5), and is listed as unaudited. If the manifests of the merged tree do
not merge cleanly in the replay, the audit cannot judge the case; it is listed
as unaudited and the owner reviews it.

**Strict rule (owner, 2026-11-20).** On a reachable case, a result that is
valid but differs from `expected` after normalization is `resolved-wrong`
unless a reviewed waiver exists. The strict rule is applied to reachable cases
only.

**Why not byte equality.** The committed file may be non-canonical (bat
`230abfd2bc14` writes `syn 2.0.32` where cargo writes `syn`). Byte equality is
reported as an `exact` rate. L0 enforces canonical output independently of
`expected`.

**Normalization.** It may erase only a difference the format document proves
meaningless; anything unproved stays significant.

- *All formats:* decode UTF-8 (otherwise not equal) and parse. Ignore comments,
  whitespace, quoting style, and key order in unordered maps. Duplicate keys
  are a parse failure. Compare scalars by type and value. Lists keep their order
  unless the format document proves the tool sorts them.
- *`Cargo.lock`:* compare the top-level `version`, the set of packages keyed by
  (name, version, source) with `checksum`, `source` and `replace`,
  `[[patch.unused]]` and `[metadata]`. Expand each `dependencies` entry to a full
  (name, version, source) id (a bare `syn` becomes the one package named `syn`;
  an ambiguous bare name equals nothing) and compare as a set.
- *`uv.lock`:* compare `version`, `revision`, `requires-python`,
  `resolution-markers` and the set of `[[package]]` keyed by (name, version,
  source), field by field. Until the uv format document says which lists uv
  sorts, every list is order-sensitive.
- *`pnpm-lock.yaml`:* compare `lockfileVersion` as a string, settings, and
  `importers`, `packages` and `snapshots` as unordered maps.

**Structural validity (L1).** Written independently of the merge code, using
only the TOML and YAML parsers.

- `Cargo.lock`: no two packages share (name, version, source); every
  `dependencies` entry resolves to exactly one package; each root (no `source`)
  or package is referenced by another; unknown top-level `version` fails.
- `uv.lock`: no two packages share (name, version, source); each dependency
  resolves to a package in the file; unknown `version` fails.
- `pnpm-lock.yaml`: every `snapshots` and importer dependency resolves; no
  duplicate keys.

**Waivers (reachable cases only).** `expected` can contain a change no merge
could derive from the three inputs even though its ids are present (for
example a bump to a version that exists on the other side, made on purpose in
the merge commit). If lockmerge's result is valid, differs from `expected`, and
a person judges the difference justified, a `[waiver]` table with a reason is
added to that case's `case.toml`. A waived case is `resolved-waived`, is listed
in every report, and does not count as correct. The owner reviews waivers
(ADR 0006).

**Why L3 is not the per-PR gate.** `cargo metadata --locked`
([docs](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html)) fails if
Cargo would change the lock file. Without `--no-deps` it needs the dependencies
(`--offline` restricts Cargo to crates already downloaded); `--no-deps` skips
the resolve and checks nothing about the lockfile. `uv lock --check` verifies
the lockfile against the project metadata
([docs](https://docs.astral.sh/uv/concepts/projects/sync/)); those docs do not
say whether it works with `--offline`. L3 runs as a separate manual or
scheduled job with network. The pnpm command
(`pnpm install --frozen-lockfile`) is **unverified** here.

## 5. Metrics, targets and the CI gate

Definitions. Over the real corpus of one ecosystem, after skips:

- `C_r` = reachable cases where plain git conflicts.
- `C_x` = re-resolved cases where plain git conflicts.
- `N` = all cases.

| Metric | Definition |
| --- | --- |
| **Reachable resolution rate (headline)** | `resolved-correct` / `C_r`, with a 95% Wilson lower bound beside it. |
| Rate over all git conflicts | `resolved-correct` / (`C_r` + `C_x`). Reported beside the headline. |
| Re-resolved report | Counts of `refused-correct`, `resolved-unverified`, and `resolved-wrong` among re-resolved cases. Reported separately; excluded from the headline denominator. |
| Coverage | `resolved-correct` over reachable cases, with the `git-clean-wrong` count beside it. |
| False-merge count | `resolved-wrong` over all N, including git-clean cases. **Must be 0.** |
| Regression count | `git-clean-correct` cases that lockmerge refused. Target 0. |
| Error count | `error` cases. **Must be 0.** |
| Exact rate | Resolved-correct cases whose bytes equal what the package manager writes. Reported. |
| Refusal breakdown | `refused` by reason code. The largest bucket is the next rule to prove. |
| Runtime | Wall time per case (median, p95, max). Reported, **not gated**; a more than 2x regression against the baseline warns. |

**Targets (owner, 2026-11-20).**

| Corpus | v1 target | Stretch |
| --- | --- | --- |
| Real Cargo.lock, reachable conflicts | 60% or more, from at least 40 reachable conflicts, with a 95% Wilson lower bound of at least 50% | 80% |
| Agent-style Cargo.lock (section 6.3) | 90% or more | none set |

Zero wrong merges is the hard gate for both. The rate and its lower bound are
printed side by side, so nobody reads a luck-driven number as proof.

The only numbers measured so far are from the owner's run of 12 prior-art
Cargo cases: 7 reachable. Rules that refuse anything both sides touched
resolve 2 of 7 (29%). Adding "take the higher semver-compatible version when
both sides bumped" and "combine both sides' edge changes on a path package"
resolves 4 of 7 (57%). Pruning packages no longer reachable from a path
package probably adds 1 or 2, which would give 5 or 6 of 7 (71% to 86%). These
are the owner's figures, not re-derived here.

**The sample is too small to confirm a target.** At 7 cases, 4 of 7 is below 60%
and 5 of 7 is above it, and the 95% Wilson interval of 5 of 7 reaches from about
36% to 92%. Hence the minimum of 40 reachable conflicts and the lower bound of
50%. If about 40% of cases are re-resolved (5 of 12 here, a small sample), 100
real cases give about 60 reachable ones: a lower bound near 50% when the rate is
about 65%.

**The per-PR gate.** `lockmerge-bench check` exits non-zero if:

1. any case is `resolved-wrong` or `error` (a waiver exempts a reachable case
   only if it parses and carries a reason);
2. the reachable resolution rate is below `tests/fixtures/<ecosystem>/baseline.toml`.
   The first baseline is **the measured rate when the benchmark first runs** (it
   may be low); the ratchet only goes up;
3. `N` or the reachable count is below the baseline's minimums, a fixture fails
   its recorded SHA-256, or a recomputed reachable or re-resolved class differs
   from `case.toml`;
4. `resolved-unverified` exceeds `max_unverified` in the baseline (starts at
   the measured count; each new one needs owner review);
5. the oracle self-test fails: the classifier is run against two built-in stub
   mergers (return `ours`, return `base`) and must class a case where those are
   wrong as `resolved-wrong`.

**The release check.** `lockmerge-bench check --release v1` additionally
requires, for the real Cargo corpus, at least 40 reachable conflicts, a
reachable rate of at least 60% and a 95% Wilson lower bound of at least 50%;
and for the agent-style corpus, a rate of at least 90%. It is run before a v1
tag, not on every PR (early PRs are below target by design). Making it a
required check once the targets are met is a later choice.

The `bench` job is a required status check, like the issue-link check. A PR
that raises the rate may raise the baseline in the same PR. Lowering the
baseline, adding a waiver, or changing the check needs the owner's review
(CODEOWNERS on `**/baseline.toml` and `**/case.toml`).

## 6. Extraction

### 6.1 Tool and steps

`lockmerge-bench extract` shells out to the `git` binary. It runs by hand, by
the owner side, with network access (standing rule 7). Workers never run it
against real repositories. Its test builds a small repository with `git` in a
temp directory and covers: a modify/modify case, a multi-base skip, an
added-lockfile skip, an octopus skip, byte exactness, a reachable case, a
re-resolved case (a merge commit that carries an id neither side had), and the
saved manifests.

1. `sources.toml` lists repositories: `url`, `ecosystem`, lockfile path(s), a
   pinned tip `revision`, `max_cases`, `max_file_bytes`. Only public
   repositories with an accepted license; never the orchestrator's repository
   or any history from it.
2. `survey`: `git clone --bare --filter=blob:none`, list merge commits
   reachable from the pin, count candidates per section 3.1 and the
   reachable/re-resolved split. Writes no fixtures.
3. `extract`: for each candidate (newest first, diversified by distinct sets of
   changed packages) write the four blobs with `git cat-file blob <sha>:<path>`:
   raw bytes, never a package manager. Compute the reachable class, and copy
   the manifests (6.2). Keep a quota of reachable cases: stop on re-resolved
   ones at half of `max_cases` per repository, so the headline denominator is
   not starved.
4. License: read the license files at the pinned commit, write the SPDX
   expression to `case.toml`, copy the license text. A repository outside MIT,
   Apache-2.0, BSD, ISC, Zlib or Unicode-3.0 is not extracted without the
   owner's sign-off.

### 6.2 Layout

```
tests/fixtures/
  .gitattributes                  # "* -text -diff linguist-generated"
  cargo/
    INDEX.toml                    # every considered merge, with skip reasons
    baseline.toml                 # minimums, min reachable rate, max_unverified
    sharkdp-bat/
      LICENSE-APACHE, LICENSE-MIT
      02e6ff4183f6/
        base ours theirs expected # exact bytes, no extension
        manifests/{base,ours,theirs}/<repo-relative path>
        case.toml
  cargo-agent/                    # section 6.3
  uv/ ...
  pnpm/ ...
```

File names are neutral (no `Cargo.lock`), so Dependabot, Renovate and Cargo
ignore them. The real name is `lockfile_path` in `case.toml`.

`manifests/` is always saved, because the replay (section 3.3) commits them
into the temporary repository for the driver to read. It holds, for each of
base, ours and theirs, the `Cargo.toml` files (including workspace members and
path-dependency manifests) found by the extractor from the lockfile's path
packages and the workspace tables. Manifests are small, and git deduplicates
identical blobs across sides. `manifests/` under `expected` is not needed.

`case.toml` records: `repository`, `license` (SPDX), `merge_commit`, `parents`,
`base_commit` (full SHAs), `lockfile_path`, `ours_is = "parent1"`, `kind =
"reachable" | "re-resolved"` with the foreign-id evidence, SHA-256 of every
file, `extractor_version`, `extracted_on`, and an optional `[waiver]`
(reachable only). Hand-reduced fixtures that workers write for unit tests live
in `tests/fixtures/unit/` with a README naming the commit each reduces.

### 6.3 Agent-style corpus (owner, 2026-11-20)

Real history has few cases of the shape that agent-driven work produces: two
branches from one base, each adding a different crate. They are built.

**Sources.** Public repositories that are MIT or Apache-2.0 only, with a
committed `Cargo.lock`, picked from the section 2 list. Never a private or
orchestrator repository.

**Recipe** (owner-side, by `lockmerge-bench synth`, with network and cargo):

1. Check out the repository at a pinned commit; that lockfile is `base`.
2. Pick two crates `A` and `B` that are not in the manifest, from a recorded
   list, pinned to exact versions (`cargo add A@=x.y.z`).
3. Branch `a`: run `cargo add A@=...` and let cargo update the lockfile: `ours`.
   Branch `b`: the same with `B`: `theirs`.
4. `expected`: on `base`, run both adds and let cargo update the lockfile. This
   is what a person would commit after merging both branches.
5. Save the four lockfiles as bytes, the manifests of each side, and a
   `case.toml` recording the commands, the exact `cargo --version`, the crate
   versions, and the date. Cargo is run once at extraction; the files are then
   committed unchanged.
6. Classify reachable or re-resolved by section 3.2. Cargo prefers locked
   versions, so a shared transitive dependency normally keeps its id, but this
   is not assumed: cases cargo re-resolved are labeled `re-resolved` and
   excluded from the agent-style denominator, like real ones.

Target: 90% or more of the reachable agent-style cases resolved correctly.
Shapes to include (with a neighboring case that must be refused for each): the
two crates share no dependency; they share a dependency at the same version;
they share a dependency that each side resolved to a different version (must
be refused unless a rule proves the merge); one side adds a crate and the other
bumps a package; both add the same crate (identical, must merge) and the same
crate at different versions (must be refused).

**Rule 4 exception (owner, 2026-11-20).** Standing rule 4 says never
regenerate fixtures with a package manager. This corpus is the exception,
because its `ours`, `theirs` and `expected` exist only as cargo output. The
exception is scoped to `tests/fixtures/cargo-agent/` and to owner-side runs.
Cargo is run once at extraction; the files are then committed unchanged.
Workers never run cargo to write fixtures; the recipe is in `case.toml`.
ADR 0007 records the exception.

### 6.4 Size

Measured at the default branch (section 2): `Cargo.lock` 73 to 310 KB,
`uv.lock` 334 KB to 1.9 MB. Everything below is an estimate.

| Corpus | Cases | Mean file | Raw | Packed in git |
| --- | --- | --- | --- | --- |
| Cargo, real | 100 (14 repos, up to 8 each) | about 150 KB | about 60 MB | about 5 to 12 MB |
| Cargo, agent-style | 30 | about 150 KB | about 18 MB | about 1 to 3 MB |
| uv, real | 60 (14 repos, up to 6 each) | about 500 KB | about 120 MB | about 8 to 20 MB |

The packed figures assume zlib shrinks lockfiles about 5x and delta packing
removes most of the near-copies; I measured neither. Manifests add a small
amount. The agent-style cases share a base per repository, so the base is
stored once per repository (a `base` per case, deduplicated by git).
To keep it small: cap cases per repository and `max_file_bytes` (300 KB Cargo,
700 KB uv); prefer conflict cases and varied kinds; a test sums fixture bytes
and fails over a budget (100 MB raw total, as before, plus the agent corpus);
never store derived files.

## 7. How CI runs it offline

`cargo test -p lockmerge-bench` and `lockmerge-bench check --fixtures
tests/fixtures` read only committed files and spawn only `git` and the
`lockmerge` binary in temp repositories. No package manager, registry or
network. The online audit (L3) is a separate `workflow_dispatch` or nightly
workflow, not a required check, so a registry outage cannot block merging.

## 8. Cases the benchmark must catch

From the prior-art doc: R1 same key with different content; R2 removal against
a reference or a modification; R3 both sides bump one package to different
versions (resolved only by a rule that proves it, such as the semver-compatible
higher-version rule, with the incompatible-major neighbor refused); R4
unparseable or unknown-version input; R6 a merge that leaves a dangling
reference or two resolutions of one package; R7 prerelease against release.
Each needs a fixture whose right class is `refused` and, where possible, a
neighboring case that merges (standing rule 3).

New from the owner's data: **a re-resolved case must be refused.** The five
re-resolved seed cases (fd `clap 4.0.18`, lapce `b998c1f3a5`, and three
others) are the test. The tripwire is `resolved-unverified` and `resolved-wrong`
on a re-resolved case. Real examples from the prior-art run: bat
`571970f8ff28` (duplicate and dangling entries) and vuejs/core `982a145d3854`
(removed importer).

## 9. Not verified

- Churn counts per repository (the survey measures them); lockfile presence for
  repositories not marked "Confirmed"; sizes marked "not measured".
- Whether the share of re-resolved cases (5 of 12) holds at scale.
- Whether cargo keeps locked versions in all agent-style cases (labeled
  re-resolved if not).
- That `refs/pull/*/head` recovers squashed merges.
- Packed-size and runtime numbers: estimates.
- Whether Cargo rewrites a non-canonical but valid lockfile under `--locked`.
- `uv lock --check` offline behavior; the pnpm check command.
- That the identification of base and sides from `%S %X %Y` and
  `GITHEAD_<sha>` behaves in the replay as probed on git 2.50.1 for other git
  versions; the amending ADR is not yet written.

## 10. Good / Better / Best

**Good (four PRs, each "Part of #11" follow-up work except the commit of this
document).**

1. *Classifier and replay core:* `lockmerge-bench run` builds the temp
   repository from a fixture, runs both merges and the swap, classifies
   (including reachable/re-resolved), prints the table, runs the stub-merger
   self-test.
2. *Cargo oracle and gate:* the independent Cargo validity predicate and
   normalization, `check` with `baseline.toml` (first value = measured rate),
   `--release`, the 12 prior-art Cargo cases as the first fixtures, the required
   CI job, and `survey` and `extract` with their temp-repository test.
3. *Owner-side Cargo extraction* (about 100 cases) and the uv oracle (same
   shape as 2).
4. *Agent-style corpus:* `synth`, its test on a tiny repository built in the
   test, and the first owner-side run.

**Better (follow-ups).** Fetch `refs/pull/*/head`; replay rebases and
cherry-picks in the harness; mergiraf as a comparison column; the scheduled L3
audit; the pnpm corpus; a sample-size minimum for the agent-style corpus.

**Best.** Fixture reduction by delta-debugging; mutation and property tests
(apply real dependency bumps to each side of real lockfiles); a public score
page produced by CI.
