# 00. Prior art

Research for charter section A. All projects were checked on 2026-11-20.
The decision that follows from it is
[ADR 0002](adr/0002-scope-of-lockmerge-v1.md), which is Proposed.

## Summary

The call is **narrow**: v1 covers `Cargo.lock`, then `uv.lock`, then
`pnpm-lock.yaml`. The rest is deferred.

- No project found does a three-way, parse-based, refuse-when-unsure merge for
  Cargo, uv or pnpm. The one project that follows that rule, derived-merge, was
  created 2026-09-25, has 0 stars, and covers only identity-keyed JSON and
  `yarn.lock`. It explicitly does not do YAML.
- Cargo is the best first target.
  [cargo-merge-driver](https://github.com/relrelb/cargo-merge-driver) is the
  only Cargo-specific driver. Its last push was 2024-08-31, it has no license,
  and its merge function overwrites its own output (see section 2).
- The package managers repair some lockfiles themselves (pnpm, yarn, old npm),
  so lockmerge adds less there. Cargo and uv have no documented self-repair of
  conflict markers that I could find.
- The mergiraf run on 17 real conflicts (see
  [Mergiraf run](#7-mergiraf-run)) found that it does not reliably produce
  valid lockfiles: with TOML forced it exited 0 with an invalid `Cargo.lock`
  in 5 of 12 cases, and with default settings it exited 0 with a wrong
  `pnpm-lock.yaml` in 1 of 5.

## Owner decisions

The owner answered the five questions of the research report (inbox item 475)
and accepted every recommendation.

1. **Scope is narrow:** `Cargo.lock`, then `uv.lock`, then `pnpm-lock.yaml`.
   npm, yarn, poetry, `go.sum` and `Gemfile.lock` are out of v1.
2. **The mergiraf run is done by hand by the owner side** on real
   `Cargo.lock` and `pnpm-lock.yaml` conflicts. Its results are in section 7.
3. **mergiraf (GPL-3.0) is only an external comparison tool,** run by hand or
   in CI. It is never a dependency or a source of code or fixtures.
4. **`pnpm-lock.yaml` stays third,** to be revisited with the data from the
   mergiraf run and the format research.
5. **`cargo-git` is dropped from the evaluation:** the only project found by
   that name is an unrelated helper crate for running git through cargo, not a
   merge driver.

## 1. Search

Sources checked: [awesome-merge-drivers](https://github.com/jelmer/awesome-merge-drivers),
a web search covering GitHub, lib.rs and docs.rs, and the primary
repositories. The crates.io and GitHub search UIs were not used directly.
Treat the list as good but not exhaustive.

The awesome list names four lockfile drivers: composer, npm-merge-driver,
cargo-merge-driver and pnpm-merge-driver. Of the all-in-one drivers, mergiraf
and Weave handle structured data such as JSON, YAML and TOML. None of the
all-in-one drivers is lockfile-specific.

New or recent projects (last 18 months, since 2025-05) that the awesome list
does not name:

| Project | Notes | Checked |
| --- | --- | --- |
| [ShawezA/derived-merge](https://github.com/ShawezA/derived-merge) | Created 2026-09-25 ([API](https://api.github.com/repos/ShawezA/derived-merge)). Semantic merge. See section 3. | 2026-11-20 |
| [boneskull/package-lock-merge-driver](https://github.com/boneskull/package-lock-merge-driver) | Created 2025-08-27, pushed 2026-11-19. Regenerate approach. | 2026-11-20 |
| [emosheeep/lockfile-conflicts](https://github.com/emosheeep/lockfile-conflicts) | MIT. pnpm only. Approach not stated in the README. 12 stars. | 2026-11-20 |
| [aube-lockfile `merge.rs`](https://docs.rs/crate/aube-lockfile/1.5.1/source/src/merge.rs) | Not a merge driver. A port of pnpm's `merge-git-branch-lockfiles`. No common ancestor; the higher semver wins on conflict. A contrast only: it guesses, which lockmerge must not. | 2026-11-20 |
| ddt (lib.rs) | A search result says it has a `resolve-conflict` command and a `merge=ddt-auto` gitattribute. The lib.rs page returned HTTP 403, so **unverified**. | 2026-11-20 |
| [GitWand](https://gitwand.app/fix/package-lock-json-merge-conflict) | A search summary says it tried semantic resolvers for npm, yarn, pnpm and Cargo and now declines to merge lockfiles by default. The reason is not given. **Secondary, unverified.** | 2026-11-20 |

I found no tool named `lockmerge`, `lock-merge` or `cargo-lock-merge`.

## 2. Project evaluation

| Project | Approach | Last push / release | License | Tests | Open issues |
| --- | --- | --- | --- | --- | --- |
| [derived-merge](https://github.com/ShawezA/derived-merge) | Semantic three-way merge of identity-keyed maps | 2026-09-25, no releases | MIT | 24 cases (README) | 0 |
| [cargo-merge-driver](https://github.com/relrelb/cargo-merge-driver) | `git merge-file`, then `cargo generate-lockfile` | 2024-08-31 ([API](https://api.github.com/repos/relrelb/cargo-merge-driver)), no releases seen | None declared (`license: null`) | Not visible | 0 |
| [pnpm/merge-driver](https://github.com/pnpm/merge-driver) | Merge driver for `pnpm-lock.yaml` (approach not checked) | 2023-10-19 ([API](https://api.github.com/repos/pnpm/merge-driver)) | MIT | Not checked | 19 |
| [npm/npm-merge-driver](https://github.com/npm/npm-merge-driver) | Re-runs the package manager on conflict (inferred from the README output); `-c` runs a custom command | Archived 2021-08-11 | ISC | Not checked | 11 (read-only) |
| [boneskull/package-lock-merge-driver](https://github.com/boneskull/package-lock-merge-driver) | Regenerate: trashes `node_modules`, re-runs `npm install`, checks with `npm ls` | 2026-11-19 ([API](https://api.github.com/repos/boneskull/package-lock-merge-driver)) | ISC per README; GitHub reports NOASSERTION | Not checked | 13 including PRs |
| [mergiraf](https://codeberg.org/mergiraf/mergiraf) | Syntax-aware (tree-sitter) three-way merge | Latest commit 2026-11-18, 26 releases | GPL-3.0 per README | Not checked | Not checked |

**cargo-merge-driver.** In its
[`merge` function](https://raw.githubusercontent.com/relrelb/cargo-merge-driver/main/src/main.rs)
it runs `git merge-file -p`, writes the output to the placeholder path, runs
`cargo generate-lockfile`, and then copies the "current" file over the
placeholder. That last step discards the merged output. It also ignores exit
statuses. The source has been read directly and this reading is confirmed. It
is a regenerate-without-proving design, which the charter rule forbids.

**Mergiraf and lockfiles.** Its [languages page](https://mergiraf.org/languages.html)
lists TOML, YAML, JSON and `go.mod`/`go.sum`, and says nothing about
`Cargo.lock`, `uv.lock` or `pnpm-lock.yaml`. Its
[conflicts page](https://mergiraf.org/conflicts.html) says that when git's line
merge succeeds, mergiraf's does too, except that it turns duplicate-signature
keys into conflicts. It also says commutative parents (children whose order
does not matter) are configured per language. **Unverified here; section 7
(the mergiraf run) tests it:**

- whether `[[package]]` arrays of tables in TOML count as commutative,
- how `pnpm-lock.yaml` mappings are treated,
- whether its output on real conflicts is a valid lockfile.

No lockfile-specific mergiraf issue turned up in my search, which is weak
evidence only.

## 3. derived-merge: provability rules

Source: [README](https://raw.githubusercontent.com/ShawezA/derived-merge/main/README.md).
The tool treats a lockfile as an identity-keyed map. Formats:
`package-lock.json`, `npm-shrinkwrap.json`, `yarn.lock` (v1 and v2+),
`deno.lock`, other identity-keyed JSON.

Rules, with a verdict on whether each generalizes to lockmerge (base = common
ancestor):

| # | Rule (per derived-merge) | Generalizes? |
| --- | --- | --- |
| 1 | Different keys added on each side: keep both | Yes for flat keyed sets (Cargo `[[package]]` by name, version and source; uv packages; pnpm `packages:`). Only valid when the entries are self-contained. See refusal R1. |
| 2 | Same key added identically on both sides: not a conflict | Yes. Requires deep, type-strict equality. |
| 3 | One side changed an entry, the other did not: take the change | Yes. |
| 4 | One side removed, the other left it: removal wins | Only with a reachability check. Cargo and pnpm entries can be referenced by other entries. See R2. |
| 5 | Different fields of one entry edited on each side: apply both | **Do not generalize in v1.** For Cargo, `version` and `checksum` are coupled, and `dependencies` lists must stay consistent. Refuse. |
| 6 | Same key, different values: refuse with the path | Yes. |
| 7 | Unparseable input: write nothing, exit non-zero | Yes. This is the charter rule. |
| 8 | Equality is deep and type-strict (an earlier `==` bug) | Yes. |
| 9 | Lists keyed by identity field; order follows ours, then theirs appends | Only where order is semantically irrelevant and the format sorts it. Cargo sorts, so the output would need re-sorting; otherwise refuse. |

Its own stated limit is "No semantic validation. A structurally clean merge
can still produce a dependency set that doesn't install." lockmerge must go
further: after merging, the result must be self-consistent (every referenced
package present, every entry referenced or a root).

**Self-reported result:** 150 merges rebuilt from npm/cli history. Git merged
126 cleanly and conflicted on 24. derived-merge merged 147 and conflicted on
3, so 21 of 24 resolved, with 0 values lost or invented per its verifier. The
three remaining conflicts were a nested dependency deleted on one side and
modified on the other, `10.0.0-pre.1` vs `10.0.0`, and two bumps of one
devDependency. **I did not reproduce this.** It is one author's reconstruction
on one repository, created in one day, so treat it as a hypothesis. Its 3
refusals match the cases lockmerge should refuse.

**Cases that must be refused (all formats):**

- R1. Both sides add an entry with the same key but different content
  (version, source, checksum, integrity).
- R2. One side removes an entry that the other side's changes now reference,
  or the other side modified it.
- R3. Both sides bump the same package to different versions. Refused unless
  a rule that proves the result applies; for Cargo that is only rule M1 in
  [cargo-lock.md section 4.2](formats/cargo-lock.md#42-rules) (ADR 0008).
  Nothing takes a higher version just because it is higher.
- R4. Either side, or the base, does not parse, or has an unknown lockfile
  version.
- R5. A manifest (`Cargo.toml`, `pyproject.toml`, `package.json`) disagrees
  with the merged lockfile, where the check is possible. The driver is handed
  only the three lockfile versions and never reads the working tree; it may
  read the manifests of the base and both sides from git objects when git
  provides the refs, and otherwise the check is left to `lockmerge check`
  (see [01 section 6](01-git-driver-mechanics.md#6-file-in-file-out-or-repository-access)).
- R6. The merged result has a dangling reference or a package with two
  different resolutions where the format allows only one.
- R7. A prerelease vs release difference on the same package.

## 4. What the package managers do alone

| Manager | Behavior | Evidence | lockmerge adds |
| --- | --- | --- | --- |
| Cargo | No documented handling of conflict markers that I found. The format was redesigned to reduce conflicts ([#7070](https://github.com/rust-lang/cargo/pull/7070)), and Cargo [still exhibits stale-entry bugs after text merges](https://github.com/rust-lang/cargo/issues/2302). The common fix is to regenerate with `cargo update`. | Not found in docs. **Unverified:** I did not read Cargo source for marker handling. | Most |
| uv | `uv lock` / `uv sync` re-create and update the file ([layout docs](https://docs.astral.sh/uv/concepts/projects/layout/)). The docs do not mention merge conflicts. Community advice is to delete and re-lock, which re-resolves everything and may upgrade unrelated packages. | Third-party blog only | Most (re-lock loses pins) |
| pnpm | [Resolves conflicts automatically on `pnpm install` if both sides are valid lockfiles](https://pnpm.io/git); otherwise a parse error. Offers branch lockfiles and `--merge-git-branch-lockfiles` ([CLI](https://pnpm.io/cli/install)). I could not confirm `--resolution-only` in the docs. | Official docs | Less. The value is merging without a network install and refusing instead of guessing. |
| yarn berry | Secondary sources say yarn auto-resolves conflict markers on install (from v1.0), and one thread reports it still left markers on 1.22.17. Berry specifically is **unverified**; my fetch of the official lockfile page was a 404. | Blogs and Stack Overflow only | Unknown |
| npm | The v6 docs say npm resolves lockfile conflicts on `npm install` and rewrites the file ([npm v6 docs](https://docs.npmjs.com/cli/v6/configuring-npm/package-locks)). Current npm behavior not verified. Drivers exist (npm-merge-driver, package-lock-merge-driver), and derived-merge covers it. | Old docs | Least |

Where install re-resolves (uv, npm, pnpm), it may need the network and can
change unrelated versions. A pure-text, no-network merge that proves its
result is a different product. That is the pitch, and no source found
contradicts it.

## 5. Licenses and reuse

lockmerge is MIT OR Apache-2.0. The compatible licenses are listed in
standing rule 6.

- **derived-merge (MIT):** code and tests may be reused with attribution. Its
  tests are npm and yarn JSON; test data derived from npm/cli would need its
  own provenance check before copying. Prefer re-deriving the rules and
  writing our own fixtures.
- **pnpm/merge-driver (MIT), lockfile-conflicts (MIT):** reusable in
  principle. Neither is semantic for our targets.
- **npm-merge-driver, package-lock-merge-driver (ISC):** compatible. Regenerate
  approach, so little worth reusing.
- **cargo-merge-driver:** no license declared, so all rights are reserved. Do
  not copy code or fixtures.
- **mergiraf (GPL-3.0):** not compatible. Do not copy code or test data, and
  do not link it. Per owner decision 3 it is used only as a separate program
  for comparison, run by hand or in CI.
- **aube-lockfile:** license not shown on the page checked; unknown.

## 6. Call: narrow

**Why not no-go:** Cargo and uv have no verified tool that merges
semantically and refuses safely. The only Cargo driver is stale, unlicensed
and discards its merge result. Parallel agents make conflicts more frequent
([A2-ai/miniextendr#1662](https://github.com/A2-ai/miniextendr/issues/1662)).

**Why not go on all eight formats:**

- npm and yarn have drivers, and derived-merge is a credible if new one.
- pnpm self-repairs on install.
- `go.sum` is already covered by mergiraf and is mostly a set of lines.
- `poetry.lock` and `Gemfile.lock` were not researched here, and I do not know
  their semantics.
- Breadth would dilute the "never guess" test coverage.

**Risk to the call:** mergiraf may already merge `Cargo.lock` and
`pnpm-lock.yaml` validly. If the run in section 7 had shown it does, the
Cargo-first case weakens, though the GPL and the missing validity checks
remain. Section 7 shows it does not: mergiraf exited 0 with invalid or wrong
lockfiles in several cases.

## 7. Mergiraf run

Owner side, 2026-11-20.

mergiraf 0.20.0 (installed with `cargo install --locked mergiraf`), run by hand
on real lockfile conflicts from public repositories. A case is a merge commit
whose two parents both changed the lockfile since their merge base, and where
plain `git merge-file` conflicts. For each case, base, ours, theirs and the
committed resolution were extracted, and mergiraf ran as
`mergiraf merge base ours theirs -p <lockfile name> -o out` in two modes:

- **As installed:** language detected from the file name. mergiraf 0.20.0 maps
  `*.toml` to TOML and `*.yaml`/`*.yml` to YAML, so `Cargo.lock` gets a line
  merge and `pnpm-lock.yaml` gets a structured YAML merge.
- **Forced:** `-L toml` for `Cargo.lock` (as a user would get by mapping
  `Cargo.lock` to mergiraf's TOML support), `-L yaml` for `pnpm-lock.yaml`.

Each clean result (exit 0, no markers) was parsed (duplicate YAML keys
rejected) and compared with the committed resolution. `Cargo.lock` results
were also checked structurally: no duplicate `[[package]]` (name, version,
source), and every `dependencies` entry resolves, with a bare name requiring
exactly one package of that name.

| Case (merge commit) | Lockfile | As installed | Forced language |
|---|---|---|---|
| rust-lang/cargo `d8f9130c6237` | Cargo.lock | conflict | conflict |
| sharkdp/bat `02e6ff4183f6` | Cargo.lock | conflict | **exit 0, invalid**: two `libc` and two `byteorder` packages, so bare `libc`/`byteorder` dependencies are ambiguous |
| sharkdp/bat `230abfd2bc14` | Cargo.lock | conflict | exit 0, same packages as committed; three entries say `syn 2.0.32` where cargo writes `syn` (valid, not cargo's canonical form) |
| sharkdp/bat `571970f8ff28` | Cargo.lock | conflict | **exit 0, invalid**: `bat 0.24.0` listed twice; dangling dependency `itertools 0.12.1` |
| sharkdp/bat `60693db73f53` | Cargo.lock | conflict | **exit 0, invalid**: duplicated packages make bare dependencies (`libc`, `nix`, `once_cell`, …) ambiguous |
| sharkdp/fd `8500c3193d83` | Cargo.lock | conflict | conflict |
| sharkdp/fd `c41d61eeda2f` | Cargo.lock | conflict | conflict |
| sharkdp/fd `cdc6a37ed6f1` | Cargo.lock | conflict | conflict |
| lapce/lapce `09b033f1937a` | Cargo.lock | correct | correct |
| lapce/lapce `155a1dea9a96` | Cargo.lock | conflict | conflict |
| lapce/lapce `b998c1f3a50f` | Cargo.lock | conflict | **exit 0, invalid**: 8+ packages listed twice (`alacritty_terminal`, `indexmap`, `lapce-core`, …) |
| lapce/lapce `e0739a060a94` | Cargo.lock | conflict | **exit 0, invalid**: `lapce-core 0.0.12` listed twice |
| vuejs/core `5afd04cbb850` | pnpm-lock.yaml | conflict | conflict |
| vuejs/core `5ec937723af3` | pnpm-lock.yaml | conflict | conflict |
| vuejs/core `982a145d3854` | pnpm-lock.yaml | **exit 0, wrong** (see below) | **exit 0, wrong** (same) |
| vuejs/core `e12b10ac3ea3` | pnpm-lock.yaml | conflict | conflict |
| vuejs/core `eaf5455d7745` | pnpm-lock.yaml | conflict | conflict |

vuejs/core `982a145d3854` ("Merge branch 'main' into minor"): one side
removed the `packages/reactivity-transform` importer, and the other kept it
and changed versions. mergiraf merged cleanly and dropped the importer, but
the merged tree still contains `packages/reactivity-transform`, so the
lockfile no longer matches the workspace. It also pinned `vite` 5.0.6 and
`@codspeed/vitest-plugin` for vite 5.0.6, where the committed resolution has
5.0.7. With no configuration beyond mergiraf's defaults, this is a silently
wrong `pnpm-lock.yaml`.

Caveat: the committed resolution of bat `571970f8ff28` itself lists
`itertools 0.13.0` twice, so that project committed a lockfile cargo would
rewrite. mergiraf's problems in that case are different ones.

### Findings

- **Cargo.lock, as installed:** 11 of 12 conflict and 1 is correct. mergiraf
  does no harm here, but it adds almost nothing over git.
- **Cargo.lock, TOML forced:** 5 of 12 merge "cleanly" into invalid lockfiles
  (duplicate packages, dangling or ambiguous dependencies), 1 is equivalent
  but not in cargo's form, 1 is correct, and 5 conflict. A generic structured
  merge does not know Cargo.lock's invariants, so it cannot be trusted with
  them.
- **pnpm-lock.yaml (YAML is detected by default):** 4 of 5 conflict and 1
  merges cleanly into a wrong lockfile.
- **Answer to the charter's question:** no. mergiraf does not reliably
  produce valid lockfiles, and when it fails it often exits 0, which is the
  outcome lockmerge exists to prevent. This supports the narrow scope and
  ADR 0002. mergiraf stays an external comparison tool in the benchmark, and
  these 17 cases are candidate benchmark fixtures. bat, fd, lapce and cargo
  are MIT and/or Apache-2.0, and vuejs/core is MIT; check each license when
  the fixtures are extracted.

Not done: running `cargo metadata --locked` or `pnpm install --frozen-lockfile`
on the merged trees. The structural checks above are enough to show the
invalid cases. The validity commands are for the benchmark (part D).

## 8. Verification gaps

- Whether Cargo handles conflict markers on its own: Cargo source not read.
- uv: only a third-party source for the delete-and-re-lock advice.
- pnpm `--resolution-only`: not found in the docs.
- yarn berry and current npm behavior: not verified from official sources.
- ddt and GitWand: secondary or inaccessible sources.
- The derived-merge result (21 of 24) was not reproduced.
- Test suites and open issues of pnpm/merge-driver, npm-merge-driver,
  package-lock-merge-driver and mergiraf were not checked.
- The search did not use the crates.io or GitHub search UIs directly.
