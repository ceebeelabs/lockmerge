# 0018. Read `[patch]` exactly and accept one-sided path-crate version moves (amends ADR 0016)

- Status: Proposed
- Date: 2026-11-21

## Context

The R14 diagnosis of PR #89 (issue #76) listed 30 refusals for an unavailable input on the two Cargo corpora. 13 came from ADR 0016 decision 2, which leaves a `[patch]` table unread, and 8 from decision 5, which takes a one-sided version move only for registry crates. A `[patch]` table is common in real workspaces, and a path crate that bumps its own version (a release commit, or `[workspace.package] version`) is one of the commonest changes to a lockfile.

## Decision

This ADR amends ADR 0016 decisions 2 and 5. ADR 0016 is not edited.

1. **`[patch]` is read exactly.** The workspace root's `[patch.<source>]` tables are parsed: each source as written, each entry a table that has only the keys `version`, `path`, `git`, `branch`, `tag`, `rev`, `package`, `registry`, `features`, `default-features` and `optional`, with values of the types Cargo expects, and exactly one source (`path`, `git` or `registry`; not `path` with `git`). A `path` is normalized to a repository path and may not leave the repository, and `features` are compared as a set. A `[patch]` in a member manifest, an entry that is not a table, an unknown key and a wrong type are unread and refuse (R14), as before. `[replace]` stays unread. The manifests of the packages that `path` entries point at are found like path dependencies.
2. **The manifest rules run only if the three `[patch]` tables agree.** The merged root manifest's `[patch]`, after this normalization, must equal ours' and equal theirs': the same sources, the same crates and the same declarations. Any difference refuses (R14). Both lockfiles were then resolved under the same patches, so the lockfile depends on `[patch]` only through what both sides already resolved with. A patch that points at a registry package by a path (a registry declaration resolving to a path package) is never proved by B1, which requires a path declaration for a path target, so a version move of such a patched crate refuses.
3. **A one-sided version move of a path package is accepted** when all of these hold: the merged manifest of the package (from `Manifests`, after `version.workspace = true` inheritance) declares exactly the new version, and no manifest declares the old one; the other branch left the package's entry as in base (F); the new entry keeps every dependency name of the old one (F); the new version is not lower (D); every path package with an edge into it declares it as a path dependency at the package's directory, with a `version` requirement, if any, that the new version satisfies (B1); and C holds. The package is identified as the pair of one path id of base the branch lacks and one path id the branch adds, of one name. Anything else that is not an unambiguous pair refuses as before.
4. **A one-sided version move of a git crate stays refused (R14).** Its version comes from another repository at the locked commit, which lockmerge cannot read.

## Considered options

- Read `[patch]` and let it differ between the branches when the lockfiles agree: needs a proof that a patch change cannot change a resolution, which the lockfile alone does not give.
- Accept path moves without the manifest check, trusting the lockfile: a version the manifest does not declare would be taken on faith.
- Also accept a path package that loses a dependency in the move: the new entry is the branch's own resolution, but nothing proves the other branch's manifests do not still need the dependency; refused for now.

## Consequences

More merges are accepted: the 13 and the 8 refusals above move on to later rules. Some of them now reach R15, because a path id added by one branch and a registry id added by the other are never resolved together, and a path package's `links` is not read (a follow-up may read it). A change of `[patch]` on one branch only still refuses, even if it cannot affect the lockfile. The `[patch]` of `.cargo/config.toml` is outside the repository's manifests and is not read, as before.

## References

ADR 0016 decisions 2 and 5; `docs/formats/cargo-lock.md` sections 4.2 and 4.5; issues #76 and #95; PR #89.
