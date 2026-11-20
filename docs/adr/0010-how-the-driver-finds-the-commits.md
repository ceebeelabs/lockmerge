# 0010. How the driver finds the commits (amends ADR 0003)

- Status: Proposed
- Date: 2026-11-20

## Context

ADR 0003 (Accepted) says the driver may read manifests from git objects using
`MERGE_HEAD`, `REBASE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD` and `HEAD`.
The Cargo.lock research (docs/formats/cargo-lock.md section 4.5) found that
git writes the first four only after the merge attempt, so they do not exist
when git runs the driver. The owner probed this on git 2.50.1 and found what
git does pass: the arguments `%S %X %Y` and, for `git merge`, an environment
variable. This ADR supersedes the "Manifests from git objects" ref list of
ADR 0003 and the driver command line in its Decision and Consequences. ADR
0003 itself is not edited.

Probed on git 2.50.1:

- `git merge`: the environment variable `GITHEAD_<full sha of theirs>=<branch
  name>` is set; `%S` is the short sha of the merge base; `%X` is `HEAD`; `%Y`
  is the branch name.
- `git cherry-pick` and `git rebase` (merge backend): `%Y` is
  `<short sha> (<subject or ref>)`, for example `844242e (side)`, naming the
  commit being applied; `%S` is `parent of 844242e (side)`, so its first
  parent is the base. Rebase swaps ours and theirs.
- `GIT_DIR` is unset; the working directory is the repository top level.

The index stage 0 of a manifest is not available at driver time either.

## Decision

The driver stays one step. It has no second step; a `lockmerge resolve`
command is not part of v1.

1. `lockmerge install` writes the driver line as
   `lockmerge merge-driver %O %A %B %L %P %S %X %Y`. The driver uses `%S %X %Y`
   for identification as well as for marker labels.
2. Commits are identified from `%S`, `%X`, `%Y` and `GITHEAD_<sha>`, not from
   `MERGE_HEAD`, `REBASE_HEAD`, `CHERRY_PICK_HEAD` or `REVERT_HEAD`. Ours is
   `HEAD`. For a merge, theirs is the full sha of the `GITHEAD_` variable whose
   value equals `%Y`, and base is `%S`. For cherry-pick and the rebase merge
   backend, theirs is the sha in `%Y` and base is its first parent, which must
   agree with `%S`. A cherry-pick target with more than one parent is
   unidentifiable (the label does not name the selected parent, `-m N`). Each sha is resolved with `git rev-parse --verify`; an
   ambiguous short sha is refused.
   Ours is proven, not assumed: manifest rules need the `HEAD` lockfile blob at
   `%P` to equal `%A` and `git diff --cached --quiet HEAD` to report no staged
   difference, because `cherry-pick -n` starts from the index, not `HEAD`.
3. The driver finds no commits, and so applies no manifest rules (lockfile-only
   fallback; a rule that needs a manifest refuses, R14), when: the merge has
   several merge bases or a virtual ancestor; a label does not match the
   expected shape; the operation is unknown; the run is `git merge-tree`,
   `git merge-file` or standalone; or any object cannot be read. It never
   guesses.
4. The merged manifest is the output of `git merge-file -p` on the three
   manifest blobs (base, ours, theirs) of each manifest a rule needs, read
   with `git cat-file`. If that merge reports a conflict, or the manifest is
   missing on any side, the driver uses lockfile-only rules for the whole
   file. This is the text git itself produces for those manifests when its
   merge is clean, not a dependency-level merge of our own. The working tree
   and the index are never read.
5. Git does not document these label formats as stable. Tests must run real
   `git merge`, `rebase`, `cherry-pick` and `revert` in a temporary
   repository, pinned to several git versions in CI (at least the oldest
   supported and the newest). The driver refuses manifest rules when a label
   does not match the expected shape.

## Considered options

- A second step, `lockmerge resolve`, run after the merge stops, reading
  `MERGE_HEAD` and the index stages: it sees the refs, but adds a step users
  must remember. Not in v1; it may come later as an extra.
- Re-running the driver through `git checkout -m`: unverified, and also a
  second step. Rejected.
- Index stage 0 as the merged manifest: not available at driver time.
  Rejected.
- Our own dependency-level three-way merge of manifests: proves something
  about a manifest that git may not produce. Rejected.
- Find the other side by searching history, for example for a commit whose
  lockfile equals `%B`: ambiguous when commits touch only `Cargo.toml`.
  Rejected.
- Lockfile-only rules everywhere: simplest, but M2 and part of M1 never run,
  and the 60% target is not assured (docs/formats/cargo-lock.md 4.8).

## Consequences

- Manifest rules work in one step for merge, cherry-pick and the rebase merge
  backend, with no new ref reading.
- The driver depends on label formats git does not promise. A git release that
  changes them makes the driver fall back to lockfile-only rules, never merge
  wrongly; CI on the oldest and newest git versions finds such a change.
- `git revert`, the `--apply` rebase backend and `git stash` are not probed.
  They stay unidentifiable (fallback) until a test establishes their labels.
- A manifest that conflicts as text costs the whole file its manifest rules.
- The driver shells out to `git` (`rev-parse`, `cat-file`, `ls-tree`,
  `merge-file`) and links no git library.
- Tests required: a manifest that conflicts falls back; the result never
  depends on the working tree; a label of the wrong shape, `cherry-pick -m 2` of a merge commit, `cherry-pick -n` with a staged manifest pin, several merge bases
  and an unknown operation give no manifest rules.

## References

- [ADR 0003](0003-merge-driver-contract.md), [ADR 0008](0008-safe-merge-and-refusal-rules-for-cargo-lock.md)
- docs/formats/cargo-lock.md sections 4.5 and 4.6
- [gitattributes: defining a custom merge driver](https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver)
- [builtin/merge.c](https://github.com/git/git/blob/master/builtin/merge.c), [sequencer.c](https://github.com/git/git/blob/master/sequencer.c)
