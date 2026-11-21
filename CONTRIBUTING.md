# Contributing to lockmerge

Thanks for your interest. lockmerge is pre-alpha. The plan is in
[docs/charter.md](docs/charter.md). The Cargo workspace has the library crates
`crates/lockmerge-core` and `crates/lockmerge-cargo`; there is no binary yet.
Issues, discussion and pull requests are welcome.

## The rule every change keeps

lockmerge combines what is provably unambiguous and refuses everything else.
A change that could make lockmerge write a wrong lockfile instead of refusing
will not be merged, however rare the case. When in doubt, refuse.

## Issues and labels

- `type:` `bug`, `feature`, `docs`, `research` or `chore`.
- `status: needs-decision`: an open design question; nobody starts work until
  it is answered.
- `status: confirmed`: reproduced, with a cause and a suggested test.
- `size: s`, `size: m`, `size: l`, and `prio: p0` (urgent) to `prio: p3`.

## Pull requests

- Every change has an issue and a pull request. Say "Fixes #N" when the PR
  completes the issue and "Part of #N" when it is one piece of it.
- The `issue link` check fails unless the description references an open
  issue of this repository with `Fixes #N`, `Closes #N`, `Resolves #N` or
  `Part of #N`. A number that is a pull request, is closed, does not exist or
  belongs to another repository does not count. To fix a failure, correct the
  reference in the description; editing it re-runs the check. To test the
  logic locally, run `.github/scripts/check-issue-link.sh --self-test`. The
  check runs the default branch's copy of the workflow and script, so a change
  to either takes effect after it merges; a separate `issue link self-test`
  workflow runs the pull request's copy of the script with `--self-test`.
- Fill in the pull request template: what changed and why, the commands you
  ran and their results, and anything left for later.
- Use American English.
- Before you ask for review, run:

  ```sh
  cargo fmt --all --check
  cargo clippy --all-targets -- -D warnings
  cargo test
  ```

  The `rust` CI job runs the same three checks on every pull request, over the
  whole workspace.

### Test fixtures

Lockfiles under `tests/fixtures/` are test data. Write them exactly, byte for
byte, and never regenerate them with a package manager. `.gitattributes`
stops git from changing their line endings.

### Dependencies

Add a crate by editing `Cargo.toml` and letting cargo update `Cargo.lock`;
never edit the root `Cargo.lock` by hand. A PR that adds a crate names it, why
it is needed, and its license. The license must be compatible with
MIT OR Apache-2.0 (for example MIT, Apache-2.0, BSD, ISC, Zlib or
Unicode-3.0).

## Decisions

Significant decisions are recorded as architecture decision records in
[docs/adr/](docs/adr/README.md), in the pull request that makes them. The
owner accepts or rejects each one before that pull request merges. The
`adr lint` check enforces numbering, titles, statuses, dates and the index; run
`.github/scripts/check-adrs.sh` locally to check `docs/adr/`, or
`.github/scripts/check-adrs.sh --self-test` to test the check itself.

## Reviewing

A reviewer checks, in order:

1. **No wrong merges.** Every path that can't prove a merge correct refuses.
   Each new merge rule has a test for the case it accepts and for the
   neighboring case it must refuse.
2. **Correctness and edge cases**, with a concrete failing input for every
   problem raised.
3. **Byte-exact output.** Unchanged input round-trips byte for byte; output
   matches the package manager's own formatting.
4. **Tests** for every behavior changed, and fixtures that are real or
   faithfully reduced from real lockfiles.
5. **Docs in step**: the format documents, the README and the charter.
6. **Dependencies**: licenses compatible, maintained, and not a large tree
   for a small need.

## License

By contributing, you agree that your contributions are dual licensed under
MIT OR Apache-2.0, as described in the [README](README.md#license), with no
additional terms or conditions.
