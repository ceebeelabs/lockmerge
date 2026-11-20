# 0003. The merge driver contract lockmerge implements

- Status: Proposed
- Date: 2026-11-20

## Context

Git's external merge driver contract is documented in [gitattributes](https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver): the driver gets `%O %A %B %L %P` (plus `%S %X %Y`), overwrites `%A` with the result, and exits 0 for clean, non-zero for conflict, above 128 for failure. The external-driver path adds no conflict markers; it reads `%A` back as the result (`ll_ext_merge` in [merge-ll.c](https://github.com/git/git/blob/master/merge-ll.c), read summarized). Git skips the driver for trivial merges. The driver cannot rely on the working tree, because `git merge-tree --write-tree` has none and files in it are not reliably current. Some rules need the manifests (`Cargo.toml` and the like), which the owner decided may be read from git objects. ADR 0002 says lockmerge exits non-zero and "leaves git's conflict markers" when it cannot prove a merge. See [docs/01-git-driver-mechanics.md](../01-git-driver-mechanics.md).

## Decision

lockmerge implements the driver as `lockmerge merge-driver %O %A %B %L %P` and installs it with that command line (see the consequences for `install`). The contract:

**Inputs.** The three files named by `%O`, `%A`, `%B`, the marker size `%L`, and the path `%P` (used for its file name, to choose the format, and its directory, to locate the manifests). Optional `%S %X %Y` are accepted as marker labels. The driver never reads the working tree or the index.

**Manifests from git objects.** For rules that need the manifests of the base and both sides, the driver may run `git cat-file` or `git show` on the refs git provides for the operation: `MERGE_HEAD`, `REBASE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD` and `HEAD`. It uses no libgit2, gitoxide or git crate. When those refs are absent (`git merge-tree`, `git merge-file`, an unknown operation, a standalone run), or the objects cannot be read, or the base commit cannot be identified without guessing, the driver refuses or falls back to lockfile-only rules; a rule that needs a manifest it cannot read refuses. The result is deterministic given the same objects: it depends on nothing but the three inputs and those objects. Manifest agreement (R5) is checked in the driver when the objects are available, and by `lockmerge check` otherwise.

**Output on success.** The driver writes the proven merge result into the `%A` file, by writing a temporary file in the same directory and renaming it over `%A`, and exits **0**. The result is byte-identical for `(O, A, B)` and `(O, B, A)`.

**Refusal.** The driver refuses, writes the reason to stderr (naming the rule, such as R1, and the package), and exits **1**. It never exits 0 with a result it did not prove. Refusal covers at least: an empty or missing `%O`; any of the three inputs not parsing, or having an unknown lockfile version (R4); a `%P` file name that is not a supported format; and every case in the refusal list of [00-prior-art](../00-prior-art.md#3-derived-merge-provability-rules) and the per-format documents.

**What `%A` holds after a refusal.** The result must be unambiguously conflicted, never a clean-looking file. The driver writes conflict markers of size `%L` into `%A`: the output of `git merge-file -p` (with labels `%X`, `%S`, `%Y`) if that run reports conflicts; if it reports none, or `git merge-file` is unavailable, a single whole-file block (`<<<<<<<` ours, `=======` theirs, `>>>>>>>`). A lockfile with such a block does not parse as TOML or YAML, so the package manager fails loudly. A refusal never leaves `%A` unchanged without markers.

**Errors.** An internal error (unreadable input, failure to write) exits **255** with `%A` unchanged. Git treats an exit above 128 as a failed merge, not a conflict, and the merge stops with no change to the index or working tree (observed in [t6406-merge-attr.sh](https://github.com/git/git/blob/master/t/t6406-merge-attr.sh)). Panics are caught and mapped to 255. A manifest that cannot be read because its refs are absent is a refusal (exit 1) or a lockfile-only merge, not an error.

**Side and virtual-ancestor handling.** No rule depends on which of `%A`/`%B` is "ours" (rebase swaps them). `merge.lockmerge.recursive` is left unset, so ancestors are merged by lockmerge itself, and a refusal there yields markers that the final merge rejects as unparseable.

## Considered options

- **Exit 1 with `%A` untouched on refusal.** Simple, but leaves a clean-looking "ours" file that `git add` accepts as resolved. Rejected: it guesses in favor of one side.
- **Exit 0 with git's text-merge result when lockmerge cannot prove the merge.** Preserves git's old behavior, but is the failure mode this project exists to prevent (a clean textual merge that is an invalid lockfile). Rejected.
- **Refuse by exiting above 128.** Aborts the whole merge, so the user cannot resolve the other files. Rejected for ordinary refusals; used for internal errors only.
- **Read manifests from the working tree.** Not reliable (no working tree for `merge-tree`, stale during `merge`). Rejected.
- **Never read manifests in the driver; check them only in `lockmerge check`.** Reproducible, but the rules "take the higher compatible version" and "combine added dependencies" could never be proven in the driver. Rejected by the owner in favor of reading git objects.
- **Read manifests through libgit2 or gitoxide.** Adds a large dependency for what `git cat-file` does. Rejected.
- **Write markers with a Rust diff3 crate.** Adds a dependency for a fallback path; `git merge-file` is present whenever git runs the driver. Deferred.

## Consequences

- A merge that git would have completed silently as clean text may now stop with a conflict. That is intended (standing rule 3) and costs users a manual step.
- The code tasks need tests for: success writes `%A` and exits 0; each refusal exits 1 and leaves markers; an empty `%O` is refused; symmetry of `(O, A, B)` and `(O, B, A)`; an internal error exits 255 with `%A` unchanged; real `git merge`, `rebase`, `cherry-pick` and `merge-tree --write-tree` in a temporary repository.
- Manifest reading needs tests for each operation's refs, for absent refs (lockfile-only rules or refusal, never a guess), for an unreadable or unparseable manifest, and for a working tree that differs from the objects (the result must not change).
- Which ref is the base and which are the sides for each operation, and whether each ref exists when git runs the driver, is not yet verified; the code task must establish it by test and refuse where it cannot.
- `lockmerge install` writes the keys `merge.lockmerge.name` and `merge.lockmerge.driver` together (a section without `driver` makes git die) and uses the bare command name `lockmerge`, found through `PATH`.
- The driver does not run for trivial merges or server-side merges, so it is never a guarantee that committed lockfiles are valid.

## References

- [docs/01-git-driver-mechanics.md](../01-git-driver-mechanics.md)
- [gitattributes: defining a custom merge driver](https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver)
- [merge-ll.c](https://github.com/git/git/blob/master/merge-ll.c), [merge-ort.c](https://github.com/git/git/blob/master/merge-ort.c)
- [t6406-merge-attr.sh](https://github.com/git/git/blob/master/t/t6406-merge-attr.sh)
- [ADR 0002](0002-scope-of-lockmerge-v1.md), [Issue #9](https://github.com/ceebeelabs/lockmerge/issues/9)
