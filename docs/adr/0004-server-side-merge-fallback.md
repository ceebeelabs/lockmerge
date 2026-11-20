# 0004. Server-side merge fallback

- Status: Proposed
- Date: 2026-11-20

## Context

Hosted merges (the GitHub web UI, "Update branch", merge queues) are computed on the server. Git's driver definition lives in config, which is not shipped with the repository. GitLab states that custom drivers are unsupported on GitLab.com; Bitbucket Cloud's tracker says the same; for GitHub I found no official statement, only third-party ones, so lockmerge assumes no driver runs there ([docs/01-git-driver-mechanics.md](../01-git-driver-mechanics.md#4-server-side-merges)). The real risk is a clean textual merge that yields an invalid lockfile. The driver also does not run for trivial merges, so it cannot be the only protection.

## Decision

The server-side fallback is **detection, not merging**. lockmerge will provide:

1. A read-only command `lockmerge check [<lockfile>...]` that exits 0 only if each lockfile parses, has a known version, is internally consistent (R6), and agrees with its manifests where the format allows (R5). It prints the failed rule on exit 1. The check is the same code the driver uses to validate its own output. It runs where the full tree exists, and is where manifest agreement is checked whenever the driver could not read the manifests from git objects (see [ADR 0003](0003-merge-driver-contract.md)).
2. Documentation for running `lockmerge check` as a required CI check on pull requests and on the `merge_group` event, with a sample GitHub Actions workflow, and an explanation that the check, not the driver, is what protects merges made on the server.
3. Optionally, a sample `pre-push` script calling `lockmerge check`, documented as a local convenience that is not a guarantee.

lockmerge will not, in v1, ship a published GitHub Action, an auto-fix bot, or anything that pushes to a branch.

## Considered options

- **Documentation only:** free, but nothing catches a bad merge. Kept as part of the decision, not alone.
- **Pre-push hook only:** local, not installed by cloning, can be bypassed, and does not see merges created on the server. Optional extra.
- **CI check (chosen):** sees the final tree on the PR and the merge queue's temporary branch, and can be a required check. Detects but does not repair.
- **Auto-fix Action that re-merges and pushes:** outward-facing, needs write tokens and interacts with protected branches. Deferred; needs its own ADR and owner sign-off.

## Consequences

- Adds a `check` command to the v1 interface; its first version covers only what the per-format documents (charter section C) define.
- Teams get protection only if they make the check required in branch protection and listen to `merge_group`; the documentation must say so.
- A teammate without the local driver gets git's text merge (a missing `merge.<name>` section falls back silently), and `lockmerge check` in CI is what catches the result.
- Users on GitHub still resolve conflicting lockfiles locally; the web editor cannot use lockmerge.

## References

- [docs/01-git-driver-mechanics.md](../01-git-driver-mechanics.md)
- [GitLab: git attributes](https://docs.gitlab.com/user/project/repository/files/git_attributes), [BCLOUD-20234](https://jira.atlassian.com/browse/BCLOUD-20234)
- [GitHub: managing a merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)
- [githooks: pre-push](https://git-scm.com/docs/githooks#_pre_push)
- [ADR 0002](0002-scope-of-lockmerge-v1.md), [ADR 0003](0003-merge-driver-contract.md), [Issue #9](https://github.com/ceebeelabs/lockmerge/issues/9)
