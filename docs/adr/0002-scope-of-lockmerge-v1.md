# 0002. Scope of lockmerge v1

- Status: Proposed
- Date: 2026-11-20

This ADR waits on the mergiraf run described in
[docs/00-prior-art.md](../00-prior-art.md#7-mergiraf-run), which is pending.
It is not accepted before that run is appended.

## Context

The charter lists eight lockfile formats. Research in
[docs/00-prior-art.md](../00-prior-art.md) found that npm and yarn have
existing drivers (npm-merge-driver, package-lock-merge-driver, derived-merge),
that pnpm repairs its own lockfile on install, and that Cargo has only one
driver, which is unlicensed, stale and discards its merge result. uv has no
known driver. Mergiraf (GPL-3.0) may already merge some of these files; that
has not been verified.

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
- If the mergiraf run shows it already produces valid `Cargo.lock` or
  `pnpm-lock.yaml` merges, this decision is revisited, and pnpm may be dropped.
- Using mergiraf only as an external comparison keeps the GPL out of the
  codebase; anything more needs the owner's sign-off.

## References

- [docs/00-prior-art.md](../00-prior-art.md)
- [Issue #8](https://github.com/ceebeelabs/lockmerge/issues/8)
