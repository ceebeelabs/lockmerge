# Cargo.lock

Charter section C, first format. This document is the model for the other
formats. Its decisions are recorded in [ADR 0008](../adr/0008-safe-merge-and-refusal-rules-for-cargo-lock.md),
[ADR 0009](../adr/0009-opt-in-cargo-validation-mode.md) and
[ADR 0010](../adr/0010-how-the-driver-finds-the-commits.md), all Proposed.

Sources were checked on 2026-11-20. Cargo is MIT OR Apache-2.0. Claims about
runtime behavior that nobody has run are marked **unverified**. Claims about the
Cargo and Git sources come from a summarizing read, so they name functions, not
line numbers.

## 1. Source of truth

Cargo's parser and serializer, in rust-lang/cargo (paths current on `master`;
line links need a second pass):

- Version enum and defaults: [`src/resolver/resolve.rs`](https://github.com/rust-lang/cargo/blob/master/src/resolver/resolve.rs) (enum about lines 72 to 101, `with_rust_version` about 125 to 151; approximate).
- Decoding, edge lookup, `[metadata]`: [`src/resolver/encode.rs`](https://github.com/rust-lang/cargo/blob/master/src/resolver/encode.rs), functions `into_resolve`, `encodable_package_id`, `encodable_source_id`, `impl Serialize for Resolve`.
- Schema structs: [`crates/cargo-util-schemas/src/lockfile.rs`](https://github.com/rust-lang/cargo/blob/master/crates/cargo-util-schemas/src/lockfile.rs) (`TomlLockfile`, `TomlLockfileDependency`, `TomlLockfilePackageId`).
- Text writer, header, trimming, no rewrite if equal: [`src/ops/lockfile.rs`](https://github.com/rust-lang/cargo/blob/master/src/ops/lockfile.rs) (`serialize_resolve`, `are_equal_lockfiles`, `write_pkg_lockfile`).

Prose: [FAQ](https://doc.rust-lang.org/cargo/faq.html), [resolver](https://doc.rust-lang.org/cargo/reference/resolver.html), [`cargo metadata`](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html), [specifying dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html), [workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html).

## 2. Lockfile versions

| Version | Introduced | Recognized by | lockmerge |
| --- | --- | --- | --- |
| V1 | baseline | no `version` key; checksums in `[metadata]` as `"checksum <id>"` | Refuse |
| V2 | 1.38 ([#7070](https://github.com/rust-lang/cargo/pull/7070), merged 2019-07-23); default 1.41 to 1.52 | no `version` key; per-package `checksum`; edges may omit version or source | Refuse |
| V3 | 1.47; default 1.53 to 1.82 | `version = 3` | Support |
| V4 | 1.78 (#12852); default from 1.83 | `version = 4` | Support |
| V5 | unstable, needs `-Znext-lockfile-bump` | `version = 5` | Refuse |

Decoder: `Some(4) => V4`, `Some(5)` only when nightly features are allowed,
`None => V1`. V2 is inferred when an edge omits version or source
(`version.max(V2)` in `lookup_id`). V1 and V2 cannot be told apart by a header,
so both are refused. `max_stable()` is V4. V3 and V4 differ in one way: V4
writes the URL-encoded source (`as_encoded_url`), V3 the plain URL.

Cargo upgrades the version when it rewrites a file whose version is below the
default for the workspace's rust-version, and refuses versions above
`max_stable` without the flag (`write_pkg_lockfile`; **unverified by running
Cargo**). **Rule:** base, ours and theirs must declare the same version;
otherwise refuse.

## 3. Data model

Top-level fields in schema order: `version`, `package`, `root`, `metadata`,
`patch`.

- `[[package]]` fields in order: `name`, `version`, `source`, `checksum`, `dependencies`, `replace`. Only `name` and `version` are always present.
- `source` is absent for path packages (workspace members and path dependencies) (`encodable_source_id` returns `None`). Otherwise `registry+...`, `sparse+...` or `git+...#<commit>`.
- `checksum` appears for registry packages only.
- `dependencies` is a list of strings, absent when empty.
- **Edge format** (`TomlLockfilePackageId`): `name`, optional ` version`, optional ` (source)`. The writer (`encodable_package_id`) drops the source when only one source exists for that name and version in the file, and drops the version when only one version of the name exists in the file. **The textual form of an edge depends on every other package in the file.**
- Reading: `lookup_id` returns `None` silently when a version-less edge matches several versions, or a source-less edge matches several candidates. Cargo then ignores the edge.
- Duplicate (name, version, source) is a hard error.
- Workspace members are path packages and the roots of the graph. Stale path packages are skipped on read.
- Edges are by **package name** (the real crate name), not by the rename key a manifest may use (`foo = { package = "bar" }`).
- `[metadata]` holds only V1 checksums in practice; on V2 and later a `checksum` key in it makes Cargo drop all checksums. `[root]` is V1 only. `replace` is old-style `[replace]`.
- `[[patch.unused]]`: patches that matched nothing; package fields, no dependencies.
- The lockfile records every edge for all features and targets of workspace members (dev, build, optional, target-specific). **Unverified by running Cargo.** Rule M2 relies on this only in the safe direction: if it is wrong, the edge set and the manifest set will not match and M2 refuses.

## 4. What a provably safe merge is

### 4.1 Method

1. Parse base, ours and theirs with a parser that keeps text spans. All three must be v3 or v4 with the same version.
2. Resolve every edge in each file to a full id using Cargo's lookup rules, per file. Ambiguous or unresolved edge: refuse.
3. A package is `{id, checksum, set of resolved edges}`. Compare in this form, never as text.
4. Merge three-way per id (4.2), then apply M1 to M3 where their conditions hold.
5. Validate the merged set (4.3).
6. Re-emit canonical text (section 5), keeping original bytes for entries whose canonical form did not change.

**Invariant for the zero-wrong-merge gate:** the merged id set is a subset of
the union of the three id sets, every checksum is copied from an input, and
every edge in the output either exists in an input (re-pointed only by M1) or
is proven against the merged manifest (M2).

### 4.2 Rules

Per-id base rules (lockfile only):

| Case | Result |
| --- | --- |
| Same in ours and theirs (including both added the same) | take it |
| Changed on one side only | take the changed one |
| Added on one side only | take it |
| Removed on one side, unchanged on the other | remove, then validate |
| Both added the same id with different checksum or edges | refuse |
| Both changed the same id differently | refuse unless M1 or M2 applies |
| Removed on one side, changed on the other | refuse unless M3 applies |

**F. Dependency carry for version changes (lockfile only).** Features decide
which optional dependencies a crate has in the lockfile, features reach a crate
through other crates (`app` enables `rand/getrandom`, which forwards to
`rand_core/getrandom`), and registry manifests are not in git. So whenever the
merged set takes a different version of a crate than at least one input has
(M1, B1, or the base rules choosing between differing entries of one crate
name), the chosen entry's set of dependency names must contain the union of the
dependency names of that crate's entries in base, ours and theirs. If any is
missing: refuse. F needs no manifest, so it applies in the fallback too. It
refuses some legitimate bumps that drop a dependency; that cost is accepted.
Accept: rand_core 0.6.2 (base), 0.6.3 (ours), 0.6.4 (theirs), all with the same
dependency names. Refuse: ours has `getrandom` on rand_core 0.6.3 (feature
enabled), theirs' 0.6.4 has none; this holds with no manifests, with ours not
bumping rand_core (0.6.2 gaining `getrandom`), and with the feature requested
through a changed indirect dependent (`wrapper` 1.1.0 on ours newly enabling
`rand/getrandom`). A manifest-only feature change that the lockfile does not
show on one side is caught by M1 condition e where manifests exist, and in the
fallback by refusing M1 whenever a path package exists.

*What "equal" means in C(i) and M1 e* (as implemented). The `[features]`
tables, the workspace `members` and `exclude`, the resolver and the edition are equal, and so is every dependency
declaration of the root manifest and of every path package, compared without
its version requirement (that is B1's and M1 a's business): `package`, source,
`features`, `default-features`, `optional`, after `workspace = true`
inheritance. Two differences, which only the other branch can have made, are
allowed: a declaration the side lacks that selects nothing (no `features`,
default features, not optional), is a registry dependency and names a crate
the side's lockfile does not contain (the format document's own B1 accept case
adds such a dependency to another member); and a declaration the side has,
equal to base's, that the merged manifest lacks (the other branch removed it,
which only withdraws a request).

**D. No downgrades.** A registry or git crate whose chosen version is lower than
its version in base is refused. A lower version can lack a feature the merged
manifest enables on the other side (base rand 0.8.4 with `min_const_gen` enabled
by ours, theirs alone downgrading to 0.8.3, which lacks it), and the lockfile
and the registry metadata cannot prove otherwise. Upgrades within a semver
class are assumed to keep features, which is **unverified**; M1 also requires H
to be higher than base's version.
Accept: neither side changes a feature declaration, theirs bumps 0.8.4 to 0.8.5. Refuse: ours enables a feature and
theirs downgrades to 0.8.3.

**C. Feature coverage for every version change, across compatibility classes
too.** A version change taken from side S, whether one-sided or not and whether
inside a compatibility class or across one (rand 0.8.4 to 0.9), is taken only if
(i) for every path package, the declarations of every registry and git
dependency and every `[features]` entry naming an external crate in the merged
manifest equal those of S's manifest of that package (M1 condition e), and
(ii) every registry or git package with an edge into the changed crate exists
with the same id in S (M1 condition b), because its feature requests were
resolved there. If manifests are unavailable and any path package exists, or S's
manifest cannot be read: refuse. The check reaches indirect dependents because
(i) covers every path package's declarations, not only those naming the crate.
Cross-class feature selection: a version change of a registry or git crate
across semver compatibility classes (rand 0.8.x to 0.9.x) is refused if the
other side changed any feature selection in any path manifest between base and
that side: any `[features]` table entry, or any dependency's `features`,
`default-features` or `optional` key, in any path package (member or nonmember
path crate, at any depth of path dependencies). It is also refused when the
manifests needed to check this are unavailable (fallback mode with a path
package present). Forwarding chains, direct or through registry crates, are not
proven. Accept: theirs upgrades rand 0.8.5 to 0.9.0 and ours changed no feature
selection. Refuse: wrapper has `legacy = ["rand/min_const_gen"]`, ours enables
`legacy`, theirs upgrades rand 0.8 to 0.9 (direct forwarding); and wrapper has
`legacy = ["bridge/legacy"]` where registry `bridge` forwards `rand/min_const_gen`,
ours enables `legacy`, theirs moves rand 0.8.5 to 0.9.0 (bridge case).
Accept: base rand 0.8.4 with `default-features = false`; neither side changes
features; theirs upgrades to 0.9 and its lockfile has every dependent. Refuse:
ours enables `min_const_gen` and theirs alone upgrades to 0.9, which lacks the
feature: the merged manifest differs from theirs in rand's features. Refuse
also when manifests are unavailable.

**B1. Manifest agreement for version changes.** A one-sided change that moves a
crate present in base to another version (a bump or downgrade, with the other
side leaving it alone) is taken only if, for every path package with an edge into
that crate, the merged manifest (4.5) declares a requirement the new version
satisfies. If no path package has an edge into it, B1 holds trivially. If a
path package has such an edge and manifests are unavailable: refuse (R14). The
other side may have changed a manifest only, which the lockfile does not show.
Pure additions and removals are not affected.
Accept: ours bumps serde 1.0.100 to 1.0.101; theirs adds `anyhow` to another
member; `app` still says `serde = "1"`. Refuse: same, but theirs pins `app` to
`serde = "=1.0.100"` (manifest-only change): 1.0.101 fails the requirement.

"Changed" uses the resolved comparison, so a changed edge target counts even
when the edge text is identical (the bare `"serde"` case).

**M1. Both sides bumped the same registry crate to semver-compatible versions.**

Applies when, for a crate class K (same name, same registry source, same
compatibility class: same major, or same `0.minor`, or same `0.0.patch`), each
of ours and theirs has exactly one version in K, they differ (O and T), both
differ from base's, and neither is a prerelease. H is the higher of O and T,
taken from side S_H; L is the other. Accept only if **all** hold:

- a. For every path package in the merged set with an edge into K: its manifest (section 4.5) declares a dependency on that crate whose requirement H satisfies (semver requirement check with the `semver` crate, which Cargo itself uses). If any such path package exists and manifests are unavailable: refuse.
- b. Every non-path package in the merged set with an edge into K is present in S_H with the same id and checksum, with its K edge pointing at H there. This proves its own requirement admits H, because the same id and checksum means the same crate manifest. A registry or git package that exists only on the L side: refuse (its requirement is not in git and not provable).
- c. The merged set keeps only H in K. Edges into L or H are written as edges to H. H's entry and its edges come from S_H, and every edge must resolve in the merged set (V2). If H's edges name something that is itself a both-sides-bumped crate, M1 applies to it too; any unresolved chain: refuse.
- d. Checksum, source and version of H are copied unchanged from S_H.
- e. Features. H's edges from S_H cover only the features S_H's graph was resolved with, and features reach K indirectly (an app enabling a feature of `rand` that forwards to `rand_core/getrandom`). So the check is not limited to dependents of K: for **every** path package in the merged set, the declarations of every registry and git dependency in the merged manifest (`features`, `default-features`, `optional`, `workspace = true` inheritance) and every `[features]` entry naming an external crate (`C/feat`, `C?/feat`, `dep:C`) must equal the same in the S_H side's manifest of that package. Any difference, or an S_H manifest that cannot be read: refuse. This proves S_H's lockfile was resolved with the merged feature set. Without manifests, M1 refuses whenever any path package exists (fallback, 4.6), and F applies in every case.

The registry requirements of other crates are not in git, so M1 proves them by
identity (condition b), not by reading them. The merged manifests are used only
for path packages (condition a).

Accept case: base serde 1.0.100; ours 1.0.101; theirs 1.0.102; every dependent
exists in theirs with an edge to 1.0.102 and the member manifest says
`serde = "1"`. Merge keeps 1.0.102.
Neighbor refuse cases: ours enables `features = ["derive"]` on serde in the
member manifest (adding `serde_derive` to ours) and theirs bumps to the higher
1.0.102 without it: the merged manifest enables derive but S_H's graph has no
`serde_derive` (condition e), so M1 refuses and M3 must not prune it. The
indirect case: `app` depends on rand 0.8.5 with `default-features = false`;
ours enables rand's `getrandom` feature and bumps rand_core 0.6.2 to 0.6.3,
theirs bumps rand_core to 0.6.4 without the feature: the merged manifest differs
from S_H's in rand's features, so M1 refuses although `app` has no direct edge
into rand_core. Ours adds crate `y` depending on serde 1.0.101 and
theirs (higher) has no `y` (condition b); a member manifest says
`serde = "<1.0.102"` (condition a); versions 1.0.101 and 2.0.0 (different class,
so M1 does not apply and the base rules apply); either version is a prerelease.

**M2. Both sides changed the same path package's edges.**

Applies to a path package P (no source) whose resolved edges differ in ours and
theirs from base and from each other. Accept only if all hold:

- a. The merged edge set E is: base edges, plus edges each side added, minus edges each side removed, with the same-name target in ours versus theirs resolved only by M1. If both sides changed the target of the same name differently without M1: refuse.
- b. P is found in the merged manifest (4.5), and its dependency declarations are fully readable: names (using `package =` when renamed), requirement, `path`, `git` and `workspace = true` inheritance resolved against `[workspace.dependencies]` in the root manifest. Any form not read (unknown key, `[patch]` or `[replace]` in any workspace manifest, a dependency in several tables with different requirements): refuse.
- c. The set of distinct crate names in E equals the set declared in the manifest (all kinds: normal, dev, build, target-specific). One name with several resolved versions: refuse.
- d. Each registry edge's version satisfies the declared requirement. Each path edge points to a path package whose manifest path matches the declared `path`, and, when the declaration also has a `version` requirement, whose version (from the merged manifest of that package) satisfies it. Git edges are accepted only if identical in base, ours and theirs.
- e. Every edge target exists in the merged set.
- f. Only edges (not P's own name or version) differ. P's `version` must equal in ours, theirs and the merged manifest.

Accept case: base member `app` depends on serde; ours adds `anyhow`, theirs adds
`tracing`; the merged manifest lists serde, anyhow and tracing; all three
resolve in the merged set. Merge adds both edges.
Path-version pair: `app` declares `lib = { path = "lib", version = ">=1.0.0" }`
and both sides keep lib 1.0.0 (accept: edges combine). Refuse: ours bumps `lib`
to 2.0.0, theirs pins `=1.0.0` and adds `anyhow`; the path matches but 2.0.0
does not satisfy `=1.0.0` (condition d).
M2 also refuses a combination that makes a normal dependency cycle through
the package, and one whose merged `[features]`, membership or feature
selection differs from either branch's (comparison as in C(i), except that the
plain declaration the branch lacks may name any crate).
Neighbor refuse cases: the merged manifest lists a fourth dependency that
neither lockfile has an edge for (condition c); the merged manifest declares
`anyhow = "2"` but ours resolved 1.0.80 (d); the manifest of `app` cannot be
found, or its three-way text merge conflicts (b, fallback to base rules, which
refuse).

**M3. Prune packages that became unreachable.**

A non-path package X is dropped when all hold: X is removed on one side and
changed on the other, or X is left with no incoming edge after the merge of
every other id; X is reachable from a path package in each input file where it
appears (an input that already contains an unreachable non-path package:
refuse); and, after dropping X and re-checking, no remaining edge points at it.
Dependencies of X that then have no incoming edge and meet the same conditions
are dropped too, until stable. Needs no manifests.

Accept case: ours removes the only dependency on `old-crate` from a member;
theirs updates `old-crate`'s own dependency. `old-crate` is removed on one side
and changed on the other, and no remaining edge points to it. Merge drops it.
Neighbor refuse case: same shape, but another package in theirs still depends
on `old-crate`: the removal conflicts with a live edge (R5 stays).

### 4.3 Validation of the merged set (all must hold)

- V1. No duplicate (name, version, source).
- V2. Every edge resolves to exactly one package in the merged set.
- V3. Every non-path package is reachable from a path package.
- V4. No two registry packages of the same name and source are semver-compatible (owner decision). Rests on the [resolver docs](https://doc.rust-lang.org/cargo/reference/resolver.html#semver-compatibility); the 0.x rule is inferred from their examples. **Unverified by running Cargo.**
- V5. Output sorted and canonical (section 5).
- V6. Native links. Cargo allows one package per `links` value (for example `sqlite3`, declared by both `libsqlite3-sys` and `sqlite3-src`), and `links` is in registry manifests, which are not in git. When one registry or git id was added only by ours (not in base) and a different registry or git id only by theirs, they have never been resolved together, so the merge needs the `links` proof of 4.5 ("Registry index cache"). The proof covers registry pairs only; a pair with a git id on either side is always refused, since `links` cannot be read for git sources. For registry pairs: every registry id in the merged set has an index entry matching name, version and checksum, and no two packages share a `links` value. Without the proof: refuse (R15). Ids both sides added identically, and additions by one side only, are not affected.

### 4.4 Cases that must be refused

- R1. Any input fails to parse, has no `version` key or a version other than 3 or 4, or the three versions differ.
- R2. Same id, different checksum.
- R3. Both sides bumped the same package to different versions **and** M1 does not apply (different compatibility class, prerelease, M1 condition a or b fails, manifests missing where needed).
- R4. Dangling edge in the merged set (the cargo#2302 shape): ours bumps serde 100 to 101, theirs adds `foo` with an edge to serde 100, and M1 does not prove `foo` against 101.
- R5. Package removed on one side and changed on the other, unless M3 applies.
- R6. A path package whose edges both sides changed, unless M2 applies. Also when manifests are unavailable.
- R7. Unreachable non-path package that M3 does not accept (already unreachable in an input).
- R8. Two semver-compatible registry versions of one crate in the merged set.
- R9. Ambiguous or unresolvable edge in an input; two path packages with the same name and version.
- R10. Any difference in `[[patch.unused]]`, `[root]`, `[metadata]` or `replace`, or any key we do not know.
- R11. Prerelease versus release difference on one package; any non-semver version string.
- R12. A source string changes for the same name and version.
- R13. The merged lockfile satisfies the merged manifest for every part the rules above do not check (registry crates' requirements on each other, features). Not provable here; it is the job of the benchmark and CI check (section 6).
- R15. A registry or git id added only by ours and a different registry or git id added only by theirs, and either side's id is a git source, or the `links` proof (4.5) fails: the cache is absent, unreadable or in a format we cannot read exactly; an id has no entry, or its checksum differs from the entry's; any git source id is in the merged set; a path package's `links` cannot be read; or two packages share a `links` value. Examples: ours adds git crate `native-a` to member a and theirs git crate `native-b` to member b, both `links = "sqlite3"` (always refused); ours adds `libgit2-sys` 0.11 and theirs 0.12 (both `links = "git2"`), or ours adds `libsqlite3-sys` and theirs `sqlite3-src` (both `links = "sqlite3"`).
- Codes of the manifest rules in `lockmerge-cargo` that the list above does not number: `DOWNGRADE` (rule D), `FEATURES` (rules C and F: the merged manifests may select features the taken entry was not resolved with, or the entry lacks a dependency another version has) and `REQUIREMENT` (rule B1: a merged path manifest does not admit the version taken). When M1's conditions fail the refusal is R3; when M2's fail, R6.
- R14. A manifest is needed by a rule and any of the following holds: the commits are not identifiable (4.5), a manifest is missing on any side, the text merge of a manifest conflicts, a path package in the lockfile cannot be mapped to exactly one manifest with the same name and version, or the manifest uses a form we do not read. Refuse the rule (B1 refuses the merge when a version change needs a manifest) and apply the base rules.

Neighbors that **merge** (each needs its own test): both sides add the same
crates; one side adds crates and the other none; both add the same entry byte-identically; one side bumps and the other
does not touch it; one side removes an unused crate and the other did not touch
it; two versions of one crate that are not semver-compatible (0.6.5 and 0.7.3
of rand) when base already has both, or when only one side adds one of them
(V6 accept; the neighboring refusal is the `libgit2-sys` 0.11 and 0.12 case, and
the `libsqlite3-sys` and `sqlite3-src` case, R15).

### 4.5 Reading manifests from git objects

The charter and the owner's decision (ADR 0003) allow reading `Cargo.toml` from
git objects, never from the working tree. The driver stays one step: it runs
once per conflicted file, when git invokes it. ADR 0010 amends ADR 0003 on how
the commits are found.

**What exists at driver time.** `MERGE_HEAD`, `REBASE_HEAD`,
`CHERRY_PICK_HEAD` and `REVERT_HEAD` are not written yet when git runs the
driver (git writes them after the merge attempt: `builtin/merge.c`,
`sequencer.c`). The owner probed this on git 2.50.1. The driver instead reads
the arguments `%S %X %Y` and the environment that git gives it. The driver is
started with the repository top level as its working directory and `GIT_DIR`
unset (probed on git 2.50.1).

**Identifying the commits, per operation** (probed on git 2.50.1):

| Operation | ours | theirs | base | Evidence |
| --- | --- | --- | --- | --- |
| `git merge` | `HEAD` (`%X` is `HEAD`) | the full sha in the environment variable `GITHEAD_<full sha>=<branch name>` whose value equals `%Y` | `%S`, the short sha of the merge base | exactly one `GITHEAD_` variable matches `%Y` |
| `git cherry-pick` | `HEAD` | `%Y` is `<short sha> (<subject or ref>)`: the commit being applied | the first parent of theirs; `%S` is `parent of <short sha> (<subject or ref>)` | the sha in `%S` must equal the sha in `%Y`; theirs must have exactly one parent (a merge commit picked with `-m N` names no selected parent in the label, so it is unidentifiable) |
| `git rebase` (merge backend) | `HEAD` is the upstream plus commits replayed so far (rebase swaps the sides, so `%A` holds it) | as cherry-pick: the commit being replayed | as cherry-pick | same shape as cherry-pick |
| `git revert` | not probed; handled only if a test establishes the label shape | | | otherwise unidentifiable |
| `git merge-tree --write-tree`, `git merge-file`, `git stash`, `--apply` rebase backend, anything else | not identifiable | | | no manifest rules |

**Proof of ours.** `HEAD` is ours only if it is what git actually merged
into. `git cherry-pick -n` and similar start from the index, which can differ
from `HEAD` (a staged manifest change that the lockfile does not reflect). The
driver enables manifest rules only if the lockfile blob of `HEAD` at `%P` equals
the bytes of `%A` and `git diff --cached --quiet HEAD` reports no staged
difference (also the manifests it reads). If either check fails or cannot be
run, there are no manifest rules (4.6). Whether the on-disk index reflects the
starting state at driver time is **unverified**; a test must establish it.

The driver resolves each sha with `git rev-parse --verify` and refuses an
ambiguous short sha. Ours is `HEAD`, resolved the same way. A merge that has
several merge bases (a virtual ancestor), a label that does not have exactly the
expected shape, an unknown operation, or any object that cannot be read:
nothing is identified, and there are no manifest rules for that file (4.6).
When the commits are identified, the base and theirs commits come from the
labels, not from a search of history, so nothing is guessed.

Git does not document these label formats as stable. ADR 0010 therefore
requires tests that run real `git merge`, `rebase`, `cherry-pick` and `revert`
in a temporary repository, pinned to several git versions in CI (at least the
oldest supported and the newest), and a driver that refuses manifest rules when
a label does not match the expected shape.

Sources: [gitattributes, custom merge driver](https://git-scm.com/docs/gitattributes) (the driver gets `%O %A %B %L %P %S %X %Y`, writes its result into the `%A` file), [git-rebase](https://git-scm.com/docs/git-rebase), [builtin/merge.c](https://github.com/git/git/blob/master/builtin/merge.c), [sequencer.c](https://github.com/git/git/blob/master/sequencer.c), and the owner's probe on git 2.50.1.

**The merged manifest.** For each manifest a rule needs, the driver reads the
blobs of base, ours and theirs with `git cat-file` and runs `git merge-file -p`
on the three. A clean result is the merged manifest. A conflict, or a manifest
missing on any side, means no manifest rules for the whole lockfile (R14). This
is exactly the text that git itself produces for those manifests when its merge
is clean, so it is not a dependency-level merge of our own. The index content is not
read (stage 0 does not exist at driver time); the one use of the index is the comparison `git diff --cached --quiet HEAD` of "Proof of ours", and the working tree is never
read: the result depends only on the three inputs and the git objects.

**Finding manifests.** Take the directory of the lockfile (`%P`). Read its
`Cargo.toml` from each of the three trees. A virtual workspace has `[workspace]`
without `[package]`. Expand `members` and `exclude` globs against
`git ls-tree -r` of the same tree. Add every `path = ` dependency target read
from those manifests. Map each path package (no `source`) in each lockfile to
exactly one manifest with the same `name` and `version` (`version.workspace =
true` resolves against `[workspace.package]`; anything unreadable is R14). All
members must map, in all three lockfiles; else R14 for the whole file.

Implementation (`lockmerge-cargo`, #37). The format reads manifests through
`lockmerge_core::Manifests`, which has no directory listing, so a `members`
entry is read as a literal path or as `<directory>/*`, where each candidate is
the directory named like a path package of the lockfiles (its manifest must
hold that package name); `exclude` must be literal. Any other glob, an
unlisted member, a `[patch]` or `[replace]` table, a dependency key not read
(`registry`, `artifact`, `default_features`, ...) and a path that leaves the
repository are unread: R14. A member removed on one side has no merged
manifest, so the manifest rules refuse for that file (R14). Rules that compare
against what a side was resolved with (M1 e, C) read that side's own manifest
through `Manifests::side`, which the driver answers from the side's commit.

**Registry index cache (links proof).** The one input besides git objects. Only
when R15's trigger holds (4.4), the driver reads, read-only and offline, Cargo's
local registry index cache under `CARGO_HOME` (default `~/.cargo`),
`registry/index/<registry>/...`, for every registry id in the merged set. It
never fetches, never writes the cache and never runs Cargo. It covers the
sparse index cache (`.cache/<prefix>/<name>`, a binary file whose header and
per-version records the code task must verify against real files) and the git
index (JSON lines per crate), and refuses for any layout or header it cannot
read exactly. An entry counts only if its name and version match the id **and**
its `cksum` equals the lockfile's checksum; that makes it proof, not a guess.
All registry directories are searched, so no registry-name hash is computed;
disagreeing `links` between directories refuses. The entry's `links` value (or
its absence) is read. The `links` of path packages is read from their manifests
(4.5); git source ids refuse. The merge is accepted only if every id has a
matching entry and no two packages share a `links` value. This widens the
"manifests from git objects only" rule of ADR 0003 and ADR 0010 for this one
input. The result depends on the cache being present, but a missing cache only
causes refusals, never a different merge. Tests: cache present and links-free
(accept); a git id on either side (refuse); shared `links` (refuse); checksum mismatch (refuse); cache absent
(refuse); unknown cache format (refuse).

**Tools.** Shell out to the `git` executable (`rev-parse`, `cat-file`,
`ls-tree`, `merge-file`); no git library. Parse with `toml_edit` and `semver`,
both approved by the owner. Both are, to my knowledge, MIT OR Apache-2.0; the
code task states each crate and its license in its PR (standing rule 6).

### 4.6 Fallback

Whenever the commits are not identified (4.5), or a manifest cannot be read,
is missing on a side, or does not merge cleanly as text, run the base rules of
4.2 only, for the whole file. M3 and the registry-dependents part of M1
(conditions b to d, when no path package exists at all, since features can come
from any member, see condition e) still apply, with D, F and V6, since they need
no manifest (V6's proof reads the registry index cache; without it, R15 refuses). M2, the path-dependents part of M1 and B1 (for a crate with a path dependent) refuse (R14).

### 4.7 Accept and refuse tests (one pair per rule)

| Rule | Accept test | Neighbor refuse test |
| --- | --- | --- |
| M1 | serde bumped 100 to 101 and 102, dependents all in the 102 side | derive enabled only on the lower side; extra dependent only on the 101 side; or member requirement `<1.0.102`; or 1.0.101 vs 2.0.0 |
| M2 | member gets `anyhow` from ours and `tracing` from theirs; merged manifest lists both | merged manifest lists an extra dependency; or requirement not met by the resolved version, or a path target outside its `version` requirement; or manifest unavailable or conflicting |
| D | neither side changes a feature declaration, one-sided upgrade 0.8.4 to 0.8.5 (with another branch's feature change C refuses even an upgrade, as C(i) says) | one-sided downgrade to 0.8.3 |
| F | rand_core bumped on both sides, same dependency names | 0.6.4 lacks `getrandom` that ours' 0.6.3 has (also with manifests unavailable; also one-sided; also via a changed dependent) |
| V6 | cache present, both added crates links-free; or only one side adds a crate | shared `links` (`libsqlite3-sys` and `sqlite3-src`); checksum mismatch; cache absent; unknown cache format |
| C | cross-class upgrade when the other side changed no feature selection | other side enables a feature the new class lacks; direct forwarding (`legacy = ["rand/min_const_gen"]`) and bridge forwarding (`legacy = ["bridge/legacy"]`) with rand 0.8 to 0.9; manifests unavailable |
| B1 | one-sided serde bump; other side adds a crate to another member | other side pins `=1.0.100` in a manifest only |
| M3 | only dependent removed by ours, entry changed by theirs | another package still depends on it |

Manifest handling has its own tests (4.5): a manifest whose three-way text
merge conflicts falls back to lockfile-only rules; a working tree that differs
from the git objects does not change the result; a label that does not match
the expected shape, several merge bases and an unknown operation give no
manifest rules; `cherry-pick -m 2` of a merge commit (manifest rules must not
run); `cherry-pick -n` with a staged manifest pin that `HEAD` lacks (manifest
rules must not run).

### 4.8 Estimates per rule (the owner's 7 reachable conflicts)

The estimates are bounds from the numbers the owner gave for the 12 real
conflicts of the mergiraf run (5 were re-resolved by the merger with ids on
neither side and are refused by design; 7 are reachable), not measurements.

| Step | Resolved of 7 | Rate | Needs manifests |
| --- | --- | --- | --- |
| Base rules (refuse anything both sides touched) | 2 | 29% | no |
| M1 plus M2 together | +2, so 4 | 57% | M1 only for path dependents; M2 always |
| M1 alone | 0 to 2 | not separable from the data given | partly |
| M2 alone | 0 to 2 (M1 plus M2 is 2) | not separable | yes |
| M3 | +1 to 2, so 5 to 6 | 71% to 86% | no |

- 4 of 7 is 57%, below the 60% target, and with 7 cases the target means at least 5. **M3 is required; M1 plus M2 are not enough.**
- When manifests cannot be found, the guaranteed part is the base rules, M3 and the lockfile-only part of M1: 3 to 6 of 7 (43% to 86%). With the commits identified from `%S %X %Y` and `GITHEAD_<sha>` (4.5), the manifest rules run in the same step, so this range is the fallback case only.
- The 90% bar for two branches each adding different crates depends on M2, because the typical case has both branches adding a dependency to the same member. Lockfile-only rules refuse it (R6). Even when M2 succeeds, V6/R15 refuses it unless the registry index cache proves the added crates links-free (4.5): with a cold or absent cache (a fresh CI runner, a first merge) most such merges are refused, so the agent-style rate depends on the cache being warm and must be reported with and without it. No data exists for this; it needs its own corpus.
- Each rule shows its own row in the benchmark so the split can be measured.
- With n = 7 and 12, the rates carry large uncertainty; one case moves a rate by 14 points.

**How the bars are judged (owner decision; the corpus is defined in #11,
docs/02-oracle.md and ADRs 0005 to 0007).** v1 is declared at 60% or more of at
least 40 reachable real Cargo.lock conflicts, extracted by hand from public
repositories and classified reachable or re-resolved, with a 95% lower bound of
at least 50%, and at 90% or more on the agent-style corpus (a hand-built set in
`tests/fixtures/cargo-agent/`, made owner-side). Zero wrong merges is the hard
gate. The 7 reachable cases stay as a regression set with per-rule attribution.

## 5. Exact serialization

From `serialize_resolve` and the schema:

- Header: `# This file is automatically @generated by Cargo.` then `# It is not intended for manual editing.` Other leading `#` comment lines are preserved.
- Then `version = N`, then a blank line (V1 and V2 emit no `version` line).
- Each entry: `[[package]]`, fields in the order `name`, `version`, `source`, `checksum`, `dependencies`, then a blank line. `dependencies` is a multi-line array: one quoted string per line, one-space indent, each ending in a comma, closing `]`. Basic quotes.
- Packages sorted by `PackageId` order: name, then version (semver), then source. The source-ordering rule is unverified. Our output rule: sort by (name, semver version); if two ids tie, refuse unless their relative order is the same in an input that has both.
- Each package's `dependencies` is sorted by the fields of the edge text (name, then version as text, then source), not by semver: `rand 0.10.0` comes before `rand 0.8.5`, while the packages themselves are in semver order (checked against 460 real version 3 and 4 lockfiles packaged in crates.io releases; nine are kept in `tests/fixtures/roundtrip/cargo/`).
- `[[patch.unused]]` entries, if any, follow, sorted.
- Trailing blank lines are trimmed on V2 and later: the file ends with exactly one `\n`.
- Cargo does not rewrite a file whose lines equal the output (`are_equal_lockfiles`).

**Round-trip rule:** return unchanged files byte for byte. Do not re-serialize
through a TOML library; keep each entry's original text and replace only
entries that changed, writing edges in canonical (shortest) form for the merged
set. Fixtures must come from real repositories (standing rule 4). These rules
have not been diffed against real Cargo output.

## 6. Validity check

- **Driver:** offline structural checks V1 to V5. No Cargo.
- **Benchmark and CI:** `cargo metadata --locked --offline --format-version 1` in a checkout of the merged tree. `--locked` fails if Cargo would change the lockfile; `--offline` forbids the network ([docs](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html)). A warm cache (`cargo fetch` beforehand) is the documented route; the docs do not say what must be cached. **Unverified: not run.** Without `--no-deps` it needs downloaded crate sources, and Cargo reads target files (such as `src/lib.rs`), so it needs the full tree, not only manifests. `--no-deps` is cheaper and proves less.
- `cargo generate-lockfile` is not a check: it re-resolves and may change versions.
- Opt-in runtime use: ADR 0009 (not v1).

## 7. Known pitfalls

- [cargo#2302](https://github.com/rust-lang/cargo/issues/2302): a merge left `matches 0.1.1` referenced with no package entry. Closed 2019-02-06 as fixed by #5831, which makes Cargo recover. Cargo recovering is not proof for us; R4 refuses.
- [cargo#7070](https://github.com/rust-lang/cargo/pull/7070) (V2): moved checksums out of `[metadata]` and shortened edges. The shortened edges are what make text merges unsafe.
- Mergiraf with TOML forced produced duplicate packages and dangling or ambiguous edges in 5 of 12 real conflicts (prior-art section 7).
- The owner's data on those 12 (2026-11-20): 5 were re-resolved by the merger (the committed lockfile has ids on neither side, for example fd's clap 4.0.18, lapce b998c1f3a5 with 47 new ids). No driver can reproduce them, so refusing is correct. 7 are reachable.
- The [FAQ](https://doc.rust-lang.org/cargo/faq.html) recommends resetting to "yours" and running `cargo check` or `cargo tree`; that needs Cargo, a manifest and often the network (this corrects prior-art section 4).
- Never emit `[metadata]` on V2 and later.
- Merged-manifest trap: the lockfile proof is relative to the text merge of the manifests. If the user later edits `Cargo.toml` differently, the lockfile may no longer match; CI's `--locked` check catches it.
- The `%S`, `%X` and `%Y` label formats are not documented as stable by git (4.5); a git release can change them. The driver then finds no commits and falls back.

## 8. Verification gaps

- Cargo and Git sources were read through a summarizing tool, not line by line.
- Unrun: `cargo metadata --offline` cache behavior; whether the lockfile lists optional and dev edges (assumed, M2 refuses if wrong); the 0.x unification rule (V4).
- Source sort order, array formatting and V4 URL encoding were not checked against real files.
- The label shapes in 4.5 were probed once by the owner, on git 2.50.1, for merge, cherry-pick and rebase (merge backend). `git revert`, the `--apply` rebase backend and `git stash` are not probed; other git versions are unchecked.
- The per-rule estimates in 4.8 are bounds, not measurements.

## 9. Open decisions

Decided by the owner on 2026-11-20: one-step driver (no `lockmerge resolve` in
v1), merged manifest by `git merge-file`, M1 proof by identity for registry and
git dependents, M3 accepted, shell out to `git`, `toml_edit` and `semver`
approved, and the corpus for the 60% and 90% bars deferred to #11. Still open:
the label shapes of `git revert` and other git versions (tests in CI settle
them), and the verification gaps in section 8.
