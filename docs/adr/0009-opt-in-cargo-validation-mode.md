# 0009. Opt-in cargo validation mode

- Status: Accepted
- Date: 2026-11-20

## Context

ADR 0008 relies on offline structural checks and manifest-assisted rules.
Some risk remains that the lockfile proofs miss (requirements of registry
crates on each other, features). `cargo metadata --locked --offline
--format-version 1` fails if Cargo would change the lockfile, and does not
use the network. The charter forbids a runtime dependency on any package
manager, so this mode needs a charter change. The command has not been run:
what `--offline` needs in the local cache is unverified. Without `--no-deps`
it needs downloaded crate sources, and Cargo reads target files (such as
`src/lib.rs`), so it needs the full tree, not only manifests.

## Decision

1. Not part of v1. Off by default.
2. When the user enables it (flag or config), lockmerge keeps a candidate
   merge only if `cargo metadata --locked --offline --format-version 1`
   exits 0 in a scratch directory populated from git objects (the merged
   tree with the candidate lockfile), never in the working tree.
3. The candidate always comes from ADR 0008's rules. The mode can only turn
   an acceptance into a refusal; it never turns a refusal into a merge. Any
   non-zero exit, a missing `cargo`, or a timeout means refuse, and the
   message says which. A cold cache therefore refuses merges the rules
   proved; this is documented.
4. No network access: `--offline` is always passed. lockmerge never runs
   `cargo update`, `cargo generate-lockfile` or `cargo fetch`.
5. The charter must first be changed to allow an optional, off-by-default
   runtime use of Cargo; the owner makes that change.

## Considered options

- Never run Cargo in the driver (ADR 0008, the default): simplest, keeps the
  charter; accepts the residual risk to CI.
- Opt-in veto (this decision): extra safety for users who have a warm cache;
  costs time and a charter change.
- Use Cargo to validate candidates that the rules refused (for example a text
  merge): raises the resolution rate, but a valid lockfile is not necessarily
  the intended merge (a text merge can re-point an edge to a newer, valid
  version). It changes what "never guess" means. Not proposed here.

## Consequences

- Needs `cargo` on the path, a warm local cache and a checkout of the full
  tree, so merges are slower and may be refused for environment reasons.
- Does not raise the resolution rate; it only lowers the chance of a wrong
  merge.
- The charter text changes.

## References

- docs/formats/cargo-lock.md section 6
- https://doc.rust-lang.org/cargo/commands/cargo-metadata.html
- docs/charter.md ("no runtime dependency on any package manager")
