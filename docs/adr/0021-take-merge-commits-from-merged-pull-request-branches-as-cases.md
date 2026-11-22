# 0021. Take merge commits from merged pull request branches as cases (amends ADR 0005 and ADR 0007)

- Status: Accepted
- Date: 2026-11-22

## Context

The real `Cargo.lock` corpus has 40 plain-git conflicts across 18 sources. ADR 0005 asks for at least 40 reachable conflicts with a 95% lower bound of at least 50%, and that needs more. A survey after #106 found 1,542 more squash-merged pull requests, all clean on the lockfile. On squash-merging projects the lockfile conflict is resolved earlier, by a "merge main into the branch" commit in the pull request's own history (`refs/pull/<n>/head`). Those merge commits are real conflicts with a real human resolution. The survey never scanned that history.

## Decision

With `pr_branch_merges = true` on a source in `sources.toml` (default off), `survey` and `extract` also count merge commits from the own history of merged pull requests as cases.

- **Candidate.** A merge commit `M` with two parents, reachable from `refs/pull/<n>/head` and not from the pinned revision. Ours and theirs are its parents in order, base is the single merge base of the parents, and expected is `M`'s lockfile. The existing candidate rules apply unchanged: one merge base, the lockfile differs on both sides, size limits, and reachable or re-resolved.
- **Merged pull requests only.** The pull request must have landed: either as the unique squash commit on the first-parent history, proven as in section 3.1 of docs/02-oracle.md (either proof), or with its head reachable from the pinned revision. Otherwise the merge is skipped as `pr-not-merged`.
- **Settled resolution.** The pull request head's lockfile must equal `M`'s byte for byte, else `skipped:pr-merge-unsettled`.
- **Dedup.** A merge commit is a case at most once, even when several pull requests contain it; a case from any of them wins over skips from the others. Merges reachable from the pinned revision are left to the existing first-parent scan.
- **Record.** `case.toml` gets `origin = "pr-branch"` and `pull_request = <n>`. `merge_commit` is `M`.
- **Reporting.** `survey` counts these cases as their own group, and `check` prints them as their own group. They are also counted in the headline.

## Limit

The expected file is the author's resolution on the branch. It is not a file that landed on the default branch as-is: a squash lands the pull request's final tree, whose lockfile may be re-resolved again, and reviewers may have asked for changes. The settled rule is what makes this acceptable ground truth: the author kept that exact lockfile until the pull request landed, so no later commit on the branch changed it. It is still not proof that the lockfile equals what the default branch would have held. A pull request whose head equals `M`'s lockfile only because a later commit restored it is accepted; we do not look for that.

## Considered options

- Take the merge regardless of later changes: simpler, but counts resolutions the author themselves later replaced.
- Require the squash lockfile to equal `M`'s: stricter, but a squash on a moved default branch re-resolves the lockfile, so it would drop nearly all of these cases.
- Leave the history unscanned: the corpus stays too small for the ADR 0005 target.

## Consequences

- ADR 0005: the headline denominator can now include author branch resolutions. The own group in `check` lets the owner read its rate apart; if it differs, the owner can decide to report the groups separately.
- ADR 0007: `case.toml` has `origin` and, for these cases, `pull_request` without `squash_proof`; the index lists the new skip reasons.
- ADR 0017: regressions and git-clean-correct counts include these cases; the group line shows only the reachable conflict rate.
- Pull request refs are fetched by `survey` and `extract` for a source that sets this, as for `squash_prs`. Workers never fetch.

## References

- Issue #107; docs/02-oracle.md sections 3.1 and 6.1; ADR 0005, 0007, 0017.
