What a reviewer checks in this repository. daedalus renders your structured
result into the review format; you do not write Markdown for the review.

- **No wrong merges (blocking).** Any change that could make lockmerge write
  a lockfile it can't prove correct, instead of refusing, is a required
  finding. Give the concrete base, ours and theirs inputs that produce the
  wrong output.
- Every new merge rule has a test for the case it accepts and one for the
  neighboring case it must refuse.
- Byte-exact output: unchanged input round-trips byte for byte, and output
  matches the package manager's own formatting.
- Fixtures under `tests/fixtures/` are real or faithfully reduced from real
  lockfiles, say where they came from, and were not regenerated.
- The benchmark's definition of "correct" and its false-merge check are not
  weakened.
- Correctness and edge cases of the change, with a concrete failing input for
  every required finding.
- New crates: license compatible with MIT OR Apache-2.0, maintained, and not
  a large tree for a small need. The root `Cargo.lock` changed only through
  cargo.
- Tests for every behavior changed; docs (the format documents, README,
  charter) and `.daedalus/policy.toml` in step.

Required findings block the merge. Anything else goes in `optional`.

ADRs (`docs/adr/`) in the diff:

- An ADR the author adds or changes is `Proposed`, has no status in its
  title, and is in the index in `docs/adr/README.md`.
- A status change to Accepted, Rejected, Deprecated or Superseded is valid
  when the owner made it (an owner commit, or the owner decision recorded on
  the pull request or issue); it is not a finding.
- A significant decision (hard to reverse, shaping more than one format or
  crate, or one a reviewer would ask "why?" about) without an ADR is a
  finding.
