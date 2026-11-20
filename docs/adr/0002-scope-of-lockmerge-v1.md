# 0002. Scope of lockmerge v1

- Status: Accepted
- Date: 2026-11-20

## Context

The charter lists eight lockfile formats. Research in
[docs/00-prior-art.md](../00-prior-art.md) found that npm and yarn have
existing drivers (npm-merge-driver, package-lock-merge-driver, derived-merge),
that pnpm repairs its own lockfile on install, and that Cargo has only one
driver, which is unlicensed, stale and discards its merge result. uv has no
known driver. Mergiraf (GPL-3.0) might already merge some of these files, so the
owner ran mergiraf 0.20.0 by hand on 17 real conflicts (12 `Cargo.lock`, 5
`pnpm-lock.yaml`; [section 7](../00-prior-art.md#7-mergiraf-run)). With TOML
forced for `Cargo.lock`, it exited 0 with an invalid lockfile in 5 of 12 cases
(duplicate packages, dangling or ambiguous dependencies); as installed, 11 of
12 conflicted and 1 was correct. For `pnpm-lock.yaml` with default settings, it
exited 0 with a wrong lockfile in 1 of 5 cases (a removed importer still
present in the workspace, and a stale `vite` version). It does not reliably
produce valid lockfiles, and when it fails it often exits 0.

## Decision

The call is **narrow**. v1 supports these formats, in this order:

1. `Cargo.lock`
2. `uv.lock`
3. `pnpm-lock.yaml`, which stays third and is revisited with the data from the
   mergiraf run and the format research.

`package-lock.json`, `yarn.lock`, `poetry.lock`, `go.sum` and `Gemfile.lock`
are out of scope for v1 and become follow-ups once the benchmark exists.
`cargo-git` is dropped from the evaluation: the only project found by that
name is an unrelated helper crate, not a merge driver.

For every format, lockmerge merges only what it can prove (the refusal cases
R1 to R7 in the prior art document) and otherwise exits non-zero and leaves
git's conflict markers.

mergiraf is used only as an external comparison tool, run by hand or in CI. It
is never a dependency or a source of code or fixtures.

## Considered options

- Go (all eight formats): matches the charter, but spreads the "never guess"
  tests thin and duplicates existing npm and yarn tools.
- Narrow (Cargo, uv, pnpm): targets the gap where no safe tool is known, and
  keeps each format's tests thorough.
- No-go: rejected, because Cargo and uv have no verified tool that merges
  semantically and refuses safely.

## Consequences

- v1 effort goes to Cargo first; the format docs and the benchmark start there.
- npm, yarn, poetry, `go.sum` and `Gemfile.lock` users get nothing from v1.
- The mergiraf results support this scope: a generic structured merge does not
  know the lockfile invariants. pnpm stays third and may still be dropped if
  the format research shows `pnpm install` already gives a safe offline result.
- The 17 cases are candidate benchmark fixtures; each source license is
  checked at extraction.
- Using mergiraf only as an external comparison keeps the GPL out of the
  codebase; anything more needs the owner's sign-off.

## References

- [docs/00-prior-art.md](../00-prior-art.md)
- [Issue #8](https://github.com/ceebeelabs/lockmerge/issues/8)
