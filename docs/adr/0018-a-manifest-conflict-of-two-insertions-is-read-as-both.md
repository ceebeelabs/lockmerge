# 0018. A manifest conflict of two insertions is read as both (amends ADR 0008 decision 10 and ADR 0010)

- Status: Proposed
- Date: 2026-11-21

## Context

ADR 0008 decision 10 says the merged manifest is the `git merge-file` result of the three manifest blobs, and that a manifest whose text merge conflicts means no manifest rules for the whole lockfile (R14; rule M2 then refuses with R6). ADR 0010 repeats the merged-manifest rule.

On the agent-style corpus (#88) this is the largest single cause of refusals. In 24 of the 29 R6 refusals each branch added one dependency line at the same place in the same member manifest (for example at the end of `[dependencies]`). Git cannot tell the order of two insertions at one position, so `git merge-file` conflicts, although the intent is plain: keep both. Of the 24, 21 have intent "merge".

The lockfile proof of M2 needs a merged manifest to compare against. For this one shape the merged manifest can be stated exactly, so no rule has to guess.

## Decision

1. When `git merge-file` conflicts on a manifest, the manifest is treated as merged only if every conflict hunk meets all of these. Otherwise today's behavior stands: no manifest rules, R14 (R6 for M2).
   - It is an insertion by both sides at the same base position: the base has no lines in the hunk, and neither side's part is empty. The hunks are read from `git merge-file --diff3` output.
   - Each side's inserted lines are whole TOML key-value entries: each line parses completely on its own as exactly one `key = value` entry. A comment, a blank line, a table header, a dotted key or a value that runs over several lines is not one.
   - The hunk is inside one dependency table header, which is `[dependencies]`, `[dev-dependencies]` or `[build-dependencies]`, or the same with a `[target.<cfg>.` prefix. Any other table (`[features]`, `[workspace.dependencies]`, `[package.metadata]`, `[patch]`, `[lints]` and the like), a table-style section such as `[dependencies.<name>]`, an array of tables, and a hunk before any header are refused.
   - No key is inserted by both sides, and no inserted key is already in that table in the base.
2. The merged manifest for the rules is the unchanged text with each hunk replaced by ours' insertions followed by theirs'. It must parse as TOML, with each inserted key in the table its header names. All hunks of a file must pass; one that does not keeps today's behavior for the whole file.
3. Git still leaves `Cargo.toml` conflicted, because the lockfile driver runs only on `Cargo.lock`. The merged manifest exists only in lockmerge's reading. The merged `Cargo.lock` is proven for the resolution that keeps both insertions. Any other resolution is caught by CI's `cargo --locked` check, as ADR 0008 already accepts for later manifest edits.
4. When this reading is used and the merge completes, the driver prints on stderr: "Cargo.toml has a merge conflict; the merged Cargo.lock assumes both added dependencies are kept." The text is in the rule's entry of the format's rule table (`notice`), not in the driver.
5. The rule is registered as `cargo.M4`. It changes what the manifest rules read, not what they accept: M1, M2, M3, B1, C, D and F, with their conditions, run on the merged manifest as before.

## Considered options

- Option A (chosen): the narrow shape above. Exact and testable, and it covers the observed cause.
- Option B: leave decision 10 as it is. Nothing changes, and the agent-style target is not reachable by this route.
- Option C: a union merge of manifests. It guesses at edits, removals and other tables, which the standing rules forbid.

## Consequences

- Both-sides-add-a-dependency conflicts in one member manifest stop refusing when the rest of M2 holds.
- The user still resolves `Cargo.toml` by hand and must keep both lines. A resolution that drops one is caught by `cargo --locked`, not by lockmerge.
- A text conflict of any other shape is unchanged.
- The benchmark and the driver supply the `--diff3` text through `ManifestResult::Conflicted`; a source that gives only `Conflict` behaves as before.

## References

- Issue #88 and the owner's decision on it.
- ADR 0008 decisions 5 and 10, ADR 0010, ADR 0016; `docs/formats/cargo-lock.md` section 4.5.
