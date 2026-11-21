# 0016. How the Cargo manifest rules read and compare manifests

- Status: Proposed
- Date: 2026-11-20

## Context

ADR 0008 decisions 4 to 6 and 10 define the manifest-assisted rules M1 to M3, B1, C, D and F, and `docs/formats/cargo-lock.md` section 4.5 says how manifests are found. Implementing them (#37) met points the documents leave open or settle in two ways:

- M1 condition e and rule C compare the merged manifests with what one side's lockfile was resolved with, but `Manifests` returned only the merged manifest.
- `Manifests` lists no directories, so the `git ls-tree` glob expansion of section 4.5 has nothing to run on.
- C(i) says the merged declarations "equal" the side's. Read literally, it refuses the format document's own B1 accept case (theirs adds `anyhow` to another member) and every merge where the other branch removes a dependency.
- R15's trigger, read on the raw lockfiles, fires on the two ids M1 exists to reconcile (serde 1.0.101 only in ours, 1.0.102 only in theirs), so M1 could never accept.
- The engine hands the three parsed lockfiles to a format only in the envelope step, while M1, M2 and M3 decide per key and prune later.

## Decision

1. `Manifests` gains `side(role, path)`, the manifest as one input has it, with a default that returns an error. The default is the safe direction: a rule that needs it refuses (R14). `GitManifests` answers from the side's commit; the testkit and the benchmark answer from saved files.
2. A `members` entry is read as a literal path or `<directory>/*`, expanded by the directories named like the lockfiles' path packages, whose manifests must hold that name. `exclude` must be literal. Everything else (other globs, `[patch]`, `[replace]`, unknown dependency keys, paths leaving the repository) is unread and refused with R14. A follow-up may add a directory listing to `Manifests`.
3. "Equal" in C(i) and M1 e compares, for the root manifest and every path package, the `[features]` table, the workspace `members` and `exclude` (a package that joins or leaves the workspace changes which features apply), the resolver, the edition and each dependency declaration without its version requirement, after `workspace = true` inheritance. Two differences are allowed because only the other branch can make them: a plain registry declaration (no `features`, default features, not optional) the side lacks for a crate the side's lockfile does not contain, and a declaration the side has equal to base's that the merge lacks (a removal withdraws a request). Anything else refuses.

   M2 compares the combination with each branch in turn under the same rule, except that the plain registry declaration the branch lacks may name a crate its lockfile does not hold or one with an identical entry in both lockfiles (below). It also refuses a combination that makes a dependency cycle through the package (Cargo rejects normal cycles and the lockfile does not show dev-dependencies). When one branch adds a plain registry declaration of a crate the other branch's lockfile already holds, the default features it enables can change that crate's dependencies. The combination is accepted only if the crate's entries (id, checksum and resolved dependency edges) are identical in both lockfiles, because the branch that added the declaration was resolved with it in force and its entry already includes what the default features pull in; otherwise it is refused with `FEATURES`.
4. R15's trigger ignores the lower id of a crate M1 keeps one version of, since that id never reaches the merged set.
5. A dependency a registry package loses, and a moved crate whose new entry lacks a dependency name, refuse by F without reading manifests; a path package that gains or loses a dependency needs C. A one-sided move is taken only for registry crates (path and git versions have no known order), only as one version for one, and only if the other branch left the old entry's dependencies as in base.
6. New refusal codes: R5 and R6 (registered again), `DOWNGRADE` (D), `FEATURES` (C and F) and `REQUIREMENT` (B1). M1 failures are R3 and M2 failures R6, as the format document says. Checks that read no manifest run before those that do, so the reason does not depend on which branch is ours.
7. `lockmerge-cargo` implements `Format` directly and runs the engine with a per-run `Pipeline` that keeps the parsed lockfiles from the envelope step on. No change to `Pipeline` is needed.

## Considered options

- Keep the literal "equal" of C(i): the B1 accept case of the format document and most merges where the other branch drops a dependency would refuse, and M3 could not accept.
- Compare only the declarations the side has: a removal by the other branch is then free, but a new declaration that selects features would pass.
- Add `Pipeline::prepare` to core so every phase sees the models: a larger change to a settled contract for what a per-run struct does.
- Add a directory listing to `Manifests` now: exact glob expansion, but a larger change to core and env than this piece needs.

## Consequences

The accepted merges are fewer than the rules' maximum: manifests with other globs, members removed on one side, and git or path version moves refuse. Each refusal is the safe direction. Decision 3 widens the literal text of C(i); the format document records the two allowances. The documents' R15 text still applies to every other pair of added ids.

## References

ADR 0008 decisions 4 to 6, 10 and 15; ADR 0012; ADR 0013; `docs/formats/cargo-lock.md` sections 4.2 and 4.5; issue #37.
