# 01. Git merge driver mechanics

Research for charter section B. Sources were read on 2026-11-20 (git documentation as published at git-scm.com, git `master` source on GitHub). The decisions that follow from it are [ADR 0003](adr/0003-merge-driver-contract.md) and [ADR 0004](adr/0004-server-side-merge-fallback.md), both Proposed. The owner's answers to the open questions are recorded in [section 9](#9-the-owners-answers).

**How the sources were read.** The git manual pages were fetched and quoted. The C source (`merge-ll.c`, `merge-ort.c`) was read through a page summarizer that could not give line numbers, so claims about source are cited to the file and function, not to a line, and are marked "source (summarized)". Claims I could not verify are listed in section 8.

## TL;DR

- **The contract is small.** Git writes the three versions to temp files and runs `merge.<name>.driver` through a shell with `%O` (base), `%A` (ours), `%B` (theirs), `%L` (marker size) and `%P` (path) replaced. The driver overwrites the `%A` file with the result and exits 0 for a clean merge, non-zero for a conflict. Exit above 128 is an error, not a conflict ([gitattributes](https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver)).
- **Git does not put conflict markers into `%A` for us.** Whatever the driver leaves in `%A` is the result. So "refuse and leave conflict markers" (ADR 0002) means the driver has to write the markers itself. A refusal that leaves `%A` untouched would leave a clean-looking file marked as conflicted. See ADR 0003.
- **The driver is not called for trivial merges.** If only one side changed a file, or both made identical changes, git resolves it without calling the driver. lockmerge never sees those, so the check that a result is valid cannot live only in the driver (see section 5).
- **The driver needs no library and never touches the working tree.** The lockfile merge is file-in, file-out. For the rules that need manifests, v1 may read the manifests of the base and both sides from git objects with `git cat-file` or `git show`, on the refs git provides for the operation, and refuses or falls back to lockfile-only rules when those refs are absent (section 6).
- **Install is one command.** `lockmerge install [--global]` sets two config keys (`merge.lockmerge.name`, `merge.lockmerge.driver`) and adds three lines to an attributes file. `lockmerge uninstall` removes exactly those.
- **Server-side merges do not run drivers.** Confirmed in the docs of GitLab and reported for Bitbucket; for GitHub I found no official statement (third-party only). The fallback is detection, not merging: a `lockmerge check` command run in CI (including the `merge_group` event), plus documentation. A pre-push hook is an optional local extra. See ADR 0004.

## 1. The driver contract

### Configuration keys

From [gitattributes, "Defining a custom merge driver"](https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver) and [git-config](https://git-scm.com/docs/git-config) (the `merge.*` keys):

```
[merge "filfre"]
	name = feel-free merge driver
	driver = filfre %O %A %B %L %P
	recursive = binary
```

- `merge.<name>.name`: human-readable name only.
- `merge.<name>.driver`: the command line.
- `merge.<name>.recursive`: "what other merge driver to use when the merge driver is called for an internal merge between common ancestors, when there are more than one. When left unspecified, the driver itself is used for both."
- The driver definition lives in config, not in `.gitattributes`; the attribute only names the driver.

### Placeholders

| Placeholder | Meaning | Source |
| --- | --- | --- |
| `%O` | Temp file with the common ancestor's version | [gitattributes](https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver) |
| `%A` | Temp file with the current ("ours") version. The result goes here | same |
| `%B` | Temp file with the other branch's version | same |
| `%L` | Conflict marker size | same |
| `%P` | Path where the merged result will be stored | same |
| `%S`, `%X`, `%Y` | Conflict labels for the ancestor, ours and theirs | same |
| `%%` | A literal `%` | source (summarized): `ll_ext_merge` in [merge-ll.c](https://github.com/git/git/blob/master/merge-ll.c) |

Details from `ll_ext_merge` in `merge-ll.c` (source, summarized):

- `%O`, `%A`, `%B` expand to the **basenames** of temp files created with the template `.merge_file_XXXXXX`. The directory is not set in that function, so I could not establish where they live, nor the working directory of the child (not verified).
- `%P`, `%S`, `%X`, `%Y` are shell-quoted and are empty if git has no value.
- The command is run through the shell.
- The path argument passed to the driver call is the target location, **except** in rename/rename (1 to 2) conflicts, where `merge-ort.c` passes the original source path (source, summarized). A format detector must therefore not depend on `%P` being the final name; lockfiles are rarely renamed, but the driver must refuse when `%P` is not a name it knows (ADR 0003).
- I did not find which git version added `%P`, `%S`, `%X`, `%Y`. The manual documents them today; the minimum supported git version is an open item (section 8).

### What git expects back

Documented ([gitattributes](https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver)):

> The merge driver is expected to leave the result of the merge in the file named with `%A` by overwriting it, and exit with zero status if it managed to merge them cleanly, or non-zero if there were conflicts. When the driver crashes (e.g. killed by SEGV), it is expected to exit with non-zero status that are higher than 128, and in such a case, the merge results in a failure (which is different from producing a conflict).

Source (summarized, `ll_ext_merge`): exit 0 is `LL_MERGE_OK`, 1 to 128 is `LL_MERGE_CONFLICT`, above 128 is `LL_MERGE_ERROR`; the `%A` file is read back after the run, and a read failure turns the result into `LL_MERGE_ERROR`. Git's own test shows the error path: a driver killed with a signal makes `git merge` exit 2 with `error: failed to execute internal merge` and leave no unmerged index entries or working-tree changes ([t6406-merge-attr.sh](https://github.com/git/git/blob/master/t/t6406-merge-attr.sh), read summarized).

Consequences for lockmerge:

- **A conflict result is whatever is in `%A`.** The external-driver path adds no markers (source, summarized: it reads `%A` back and nothing else). Git's built-in text driver is the one that writes `<<<<<<<` markers ([gitattributes, built-in merge drivers](https://git-scm.com/docs/gitattributes#_built_in_merge_drivers)).
- **Exit 1 with `%A` untouched** gives the user a path git calls conflicted, whose working-tree file is the "ours" lockfile with no markers. `git add` on it would silently take one side. This is the worst failure mode for a tool whose rule is "never guess", so the driver must write markers on refusal (ADR 0003).
- Exit code `1` for every refusal; the reason goes to stderr. Exit above 128 only for internal errors (unreadable input, I/O failure), where git aborts the merge without changing anything.

### The `merge` attribute and fallbacks

From [gitattributes, "The merge attribute"](https://git-scm.com/docs/gitattributes#_the_merge_attribute): a string value selects a custom driver; `merge=text` and `merge=binary` select built-ins; unspecified uses the text driver unless `merge.default` names another.

- **A driver name that has no `merge.<name>` section silently falls back to the built-in text merge.** Source (summarized): `find_ll_merge_driver` checks user drivers, then built-ins, then text. This is what happens for a teammate who has the committed `.gitattributes` line but has not run `lockmerge install`. They get git's normal behavior, with no error. A mitigation (a CI check) is in ADR 0004.
- **A `merge.<name>` section with no `driver` line makes git die** with a "lacks command line" error (source, summarized). `lockmerge install` must write both keys in one step, and `uninstall` must remove the whole section.

### When git calls the driver at all

Source (summarized, `merge-ort.c`: `collect_merge_info_callback`, `handle_content_merge`):

- If all three versions are equal, or the two sides are equal, or one side equals the base, git picks the result without a content merge. **No driver call.**
- The driver is reached only for regular files whose three versions differ in the necessary way (`merge_3way()` then `ll_merge()`).
- Symlinks and submodules never reach `ll_merge`. Delete/modify conflicts: I could not read that part of the file, so whether a driver runs there is **not verified**; git's own documentation of the `merge` attribute describes only "a file-level merge" ([gitattributes](https://git-scm.com/docs/gitattributes#_the_merge_attribute)). lockmerge must not assume it is called when one side deleted the lockfile.
- GitLab's documentation says the same in its own words: "A custom merge driver is invoked only in the case of a non-trivial merge conflict" ([GitLab docs](https://docs.gitlab.com/user/project/repository/files/git_attributes)).

Both-sides-added (no base): `merge-ort` calls the content merge with a null base ID (source, summarized). What git then writes into the `%O` file for a null base is **not verified** (I expect an empty file). lockmerge treats an empty `%O` as "no ancestor" and refuses (ADR 0003), which is safe either way.

## 2. Behavior by command

| Command | Does it run the driver? | Notes |
| --- | --- | --- |
| `git merge` | Yes | The default `ort` strategy; `recursive` is now a synonym for `ort` ([git-merge](https://git-scm.com/docs/git-merge#_merge_strategies)). |
| `git rebase` (merge backend, the default) | Yes | "Using merging strategies to rebase (default)", with strategy `ort` ([git-rebase](https://git-scm.com/docs/git-rebase)). **Ours and theirs are swapped:** `%A` is the so-far rebased series, `%B` is the commit being replayed ([git-rebase](https://git-scm.com/docs/git-rebase#_merge_strategies)). |
| `git rebase --apply` | **Not verified** | The apply backend works by `format-patch` then `am`, and does not support merge strategies ([git-rebase](https://git-scm.com/docs/git-rebase)). I did not find a source stating whether `am --3way` honors custom drivers. Treat as unsupported until tested. |
| `git cherry-pick`, `git revert` | Yes | The `merge` attribute "affects ... `git revert` and `git cherry-pick`" ([gitattributes](https://git-scm.com/docs/gitattributes#_the_merge_attribute)). For cherry-pick the base is the picked commit's parent; for revert the roles of base and theirs are swapped. That follows from the definition of the operations; I did not find it stated on those manual pages. |
| `git merge-file` | **No** (not verified in source) | It takes three explicit file names and has no path to look up attributes with ([git-merge-file](https://git-scm.com/docs/git-merge-file)). Its exit value is the number of conflicts (capped at 127) or negative on error, which is a different contract from a driver's. |
| `git merge-tree --write-tree` | Documented as using "the same features as the 'real' git-merge", including content merges; the page never mentions drivers or attributes ([git-merge-tree](https://git-scm.com/docs/git-merge-tree)). **Needs a test.** | It "does not read from or write to either the working tree or index", exit 1 on conflict, so a driver cannot rely on a working tree being there. The older trivial-merge form cannot do content merges, so no driver. |
| Multiple merge bases | Yes, via a virtual ancestor | `ort` "creates a merged tree of the common ancestors and uses that as the reference tree" ([git-merge](https://git-scm.com/docs/git-merge#_merge_strategies)). The driver is called to merge the ancestors unless `merge.<name>.recursive` names another driver. During that inner merge `virtual_ancestor` is set, and git's built-in binary driver keeps the ancestor's content (source, summarized, `ll_merge` and `ll_binary_merge` in `merge-ll.c`). |

### Design consequences

1. **Symmetry.** Because rebase swaps sides, and `%A`/`%B` mean different things per command, every rule must give the same result for `(O, A, B)` and `(O, B, A)`, byte for byte. Refusal messages must say "side A / side B", not "ours / theirs". The code task should add a property test for it.
2. **Multiple bases.** Leave `merge.lockmerge.recursive` unset, so lockmerge also merges the ancestors. If it refuses there, git puts markers in the virtual ancestor, which lockmerge's parser then rejects (R4) when it does the final merge. The result is a refusal, never a guess. Setting `recursive = binary` would give a virtual base that is just one of the real bases, which is valid input but is not git's merge of the bases; I would not choose it without evidence. I could not establish whether `%S` identifies a virtual ancestor (not verified); the driver must not depend on it.
3. **Standalone CLI.** `lockmerge merge --base O --ours A --theirs B [--stdout]` gives the same engine for `merge-file`-style use and for the benchmark. It has no refs, so it uses lockfile-only rules.

## 3. Installation

Config scopes, lowest to highest precedence: system, global, local (`$GIT_DIR/config`), worktree (only if `extensions.worktreeConfig` is set), command line ([git-config](https://git-scm.com/docs/git-config)). Linked worktrees share the local config unless `extensions.worktreeConfig` is on.

Attribute files, highest precedence first ([gitattributes](https://git-scm.com/docs/gitattributes)): `$GIT_DIR/info/attributes`, `.gitattributes` in the path's directory and its parents, then the global file (`core.attributesFile`, default `$XDG_CONFIG_HOME/git/attributes`, else `$HOME/.config/git/attributes`), then the system file.

### `lockmerge install` (proposal)

Per repository, run inside a repository:

| What | Where | Content |
| --- | --- | --- |
| Driver definition | `git config --local` (`.git/config`) | `merge.lockmerge.name = lockmerge lockfile merge driver`, `merge.lockmerge.driver = lockmerge merge-driver %O %A %B %L %P` |
| Attributes | `.gitattributes` in the repository root (committed, shared by the team) | `Cargo.lock merge=lockmerge`, `uv.lock merge=lockmerge`, `pnpm-lock.yaml merge=lockmerge`, between marker comments `# BEGIN lockmerge` and `# END lockmerge` |

Globally, `lockmerge install --global`:

| What | Where | Content |
| --- | --- | --- |
| Driver definition | `git config --global` | same two keys |
| Attributes | the file `core.attributesFile` names, or its default location if unset (the key is not set by install) | same marked block |

Options worth having from day one: `--attributes=info` to write `$GIT_DIR/info/attributes` instead of the committed file (local-only, nothing for teammates to see), and `--print` to show what would change.

Rules for the installer:

- The driver command is the bare name `lockmerge`, found through `PATH` when git runs it through the shell. Absolute, machine-specific paths do not belong in anything that can be committed (standing rule 2). The config file is not committed, but a bare name keeps config portable across dotfile repositories. An `--absolute-path` option is a follow-up.
- Patterns without a slash match at any depth ([gitattributes pattern rules follow gitignore](https://git-scm.com/docs/gitattributes)), so one line per file name covers workspaces with several lockfiles.
- Idempotent: a second run changes nothing.
- Write config with `git config`, never by editing the file, so includes and scopes behave.
- Write both config keys in one step (a section without `driver` makes git die, section 1).

### `lockmerge uninstall`

- `git config --remove-section merge.lockmerge` in the same scope.
- Remove only the marked block from the attributes file; delete the file only if it is then empty and install created it. Anything outside the markers is left alone.
- Idempotent, and prints what it removed.

A committed `.gitattributes` line with no local driver falls back silently to text merge (section 1), so uninstalling in one clone cannot break another clone, and a teammate who skips install is no worse off than today.

Windows: git runs `driver` through its own shell, and quoting of `%P` differs. **Not verified.** Test on Windows in CI before claiming support.

## 4. Server-side merges

The question: GitHub's web UI, "Update branch" and merge queues compute merges on the server. Do they run drivers?

- **GitLab:** no. "Custom merge drivers are not supported on GitLab.com or GitLab Dedicated"; self-managed administrators can define them in server configuration ([GitLab docs](https://docs.gitlab.com/user/project/repository/files/git_attributes)).
- **Bitbucket Cloud:** a Jira request records that it "doesn't allow the use of GIT native or custom merge drivers configured via .gitattributes" ([BCLOUD-20234](https://jira.atlassian.com/browse/BCLOUD-20234)); Bitbucket Server has an open request too ([BSERV-4566](https://jira.atlassian.com/browse/BSERV-4566)).
- **GitHub:** I found **no official statement** either way. A Stack Overflow answer (mirrored [here](https://kiwix.lema.org/stackoverflow.com_en_all_2023-11/questions/65309426/how-to-enable-custom-merge-driver-gitattributes-in-github)) says GitHub does not support custom drivers because the driver command lives in config, which is not shipped with the repository. That reasoning is sound and matches git's design (the driver definition is in config, not in the repository), but it is third-party. The [merge queue documentation](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue) does not mention merge drivers.

Planning rule: **assume no custom driver runs server-side**. The danger is not conflicts (GitHub shows those and blocks the merge); it is a *clean* textual merge that is an invalid lockfile, such as the ones mergiraf produced in [00-prior-art section 7](00-prior-art.md#7-mergiraf-run). A plain `git merge` text merge can do the same: git's text driver has no idea about lockfile invariants.

### Options compared

| Option | Catches a bad server-made merge? | Who it covers | Cost | Weakness |
| --- | --- | --- | --- | --- |
| **Documentation only** | No | Readers who follow it | None | Relies on people. |
| **Pre-push hook** running `lockmerge check` | Only for what you push. Not for a merge GitHub creates | The person who installed it | Small: a script, `core.hooksPath` or `.git/hooks/pre-push` | Local only; a hook can be skipped, and hooks are not installed by cloning. The hook gets `<local-ref> <local-oid> <remote-ref> <remote-oid>` lines on stdin and blocks the push with a non-zero exit ([githooks](https://git-scm.com/docs/githooks#_pre_push)). Whether `--no-verify` skips it: I did not confirm from the pages read. |
| **CI check** (GitHub Action or any CI) running `lockmerge check` on the PR and on `merge_group` | **Yes**: sees the final tree, with all manifests present | Everyone, if made a required check | Medium: a workflow file, a released binary or `cargo install` | Detects, does not repair. A bad merge is blocked, the fix is manual. Needs the check to be required in branch protection. |
| **Auto-fix Action** (re-merge and push) | Yes, and repairs | Everyone | High: needs write tokens, pushes to PR branches, interacts with protected branches | Outward-facing and hard to make safe. Not for v1. |

The merge queue is the case where the check matters most: the queue builds temporary branches `gh-readonly-queue/{base_branch}` that combine the base and every PR ahead, and checks run only if the workflow listens to the `merge_group` event; "required checks must report on merge group builds, or the merge fails" ([merge queue docs](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)). That is where two PRs that each touched the lockfile get merged by the server.

Recommendation (ADR 0004): detection in CI is the real fallback. v1 ships `lockmerge check` (a read-only command that validates a lockfile against its own invariants and, where possible, its manifests: refusals R5 and R6 from [00-prior-art](00-prior-art.md#3-derived-merge-provability-rules)) plus documentation for CI and a sample workflow. A pre-push hook is an optional local example. A published GitHub Action wrapper and anything that pushes are follow-ups.

Note the dependency: `lockmerge check` needs the per-format validity rules from charter section C, so its first version covers what those documents define.

## 5. What the driver cannot guarantee

Because git skips the driver for trivial merges (section 1) and server merges skip it entirely (section 4), the driver can only vouch for the merges it performs. A lockfile that came out of a trivial merge, a server merge or a hand edit is checked only by `lockmerge check`. Documentation must say this plainly and must not describe the driver as a guarantee that committed lockfiles are valid.

## 6. File-in, file-out, or repository access?

**The driver needs no libgit2 or gitoxide, and never reads the working tree.** Everything the contract gives it is: three file paths, a marker size, a path string and three labels. The lockfile merge itself needs nothing else. The one thing that needs more is the manifests (`Cargo.toml` and the like), which some rules need to prove a result (R5 in [00-prior-art](00-prior-art.md#3-derived-merge-provability-rules); the Cargo rule M1, which takes a higher version only where it can be proven, and the rule "combine added dependencies" need to know what each side's manifest allows; R3 refuses different bumps of one package unless M1 applies, see [cargo-lock.md section 4.2](formats/cargo-lock.md#42-rules) and ADR 0008).

### Why not the working tree

The child is run through a shell from git's working directory (source summarized, not verified for the cwd); `git merge-tree --write-tree` has no working tree at all ([git-merge-tree](https://git-scm.com/docs/git-merge-tree)); and during `git merge` the working tree is not yet in the merged state, so files read from it could be stale or from one side. A decision based on them would not be reproducible. The driver must not read the working tree, the index, or any file other than its three inputs and the objects described next.

### Manifests from git objects (owner decision)

The owner decided that v1 may read the manifests of the base and both sides **from git objects**, never from the working tree. The contract:

- **Allowed:** run `git cat-file` or `git show` to read a manifest blob at the same directory as the lockfile (derived from `%P`) in the commits git provides for the operation. No libgit2, no gitoxide, no `git` crate; the driver only runs the `git` executable, which is present because git started the driver.
- **Amended by ADR 0010:** the ref list below is superseded. Refs such as `MERGE_HEAD` do not exist while the driver runs; ADR 0010 says what git passes instead.
- **Refs it may use (as ADR 0003 stated them):** `HEAD`, `MERGE_HEAD`, `REBASE_HEAD`, `CHERRY_PICK_HEAD` and `REVERT_HEAD`. Which of them stands for base, side A and side B depends on the operation, as in section 2: for `merge`, `HEAD` and `MERGE_HEAD` are the two sides; for `rebase`, `HEAD` is the series rebased so far and `REBASE_HEAD` is the commit being replayed; for `cherry-pick` the base is the parent of `CHERRY_PICK_HEAD`; for `revert` base and theirs are swapped relative to that. These follow from the definitions of the operations. I did not verify them against the source, nor that each ref exists at the moment git runs the driver (section 8).
- **Absent refs:** when the refs for the operation are absent (`git merge-tree`, `git merge-file`, an unknown operation, a standalone run), or when the commits cannot be read, or the base commit cannot be identified without guessing (for example several merge bases), the driver must either **refuse**, or **fall back to lockfile-only rules**, which are the rules provable from the three lockfile versions alone. A rule that needs a manifest it cannot read refuses; it never assumes the manifest is unchanged.
- **Deterministic:** the result depends only on the three inputs and the objects named by the refs. It must be the same on every run with the same objects, and must not depend on the working tree, the index, the time, or the environment. Reading from objects keeps this true even when the working tree is dirty or stale.
- **Symmetric:** assigning a manifest to a side follows the lockfile's `%A`/`%B` assignment, so the result is the same for `(O, A, B)` and `(O, B, A)`.
- **Self-check:** a manifest blob that is missing at one of the three commits, or does not parse, is a refusal for rules that need it. It is not read as "empty".

Manifest agreement (R5) is therefore checked **in the driver when the objects are available**, and by `lockmerge check` otherwise (and always in CI, where the whole tree exists).

### Other repository questions

- **Conflict-marker fallback (ADR 0003)** needs a three-way text merge. Options: shell out to `git merge-file -p` with the labels `%S %X %Y` and marker size `%L` (git is present, since git started the driver; it needs no repository, only three files), or write a diff3 in Rust. Recommended: `git merge-file`, with a whole-file marker block if it is unavailable. No new crate.
- **`merge.conflictStyle` (`diff3`/`zdiff3`)**: git's built-in driver honors it; an external driver is not told. Follow-up: the fallback could read it with `git config`. Out of v1.

`lockmerge install`/`uninstall` call the `git config` command, so the install side also needs no library.

## 7. Good / Better / Best, and the PR split

Per the owner's rule to build the simplest solid version, **Good** is what v1 needs.

- **Good**
  1. PR 1: `lockmerge merge-driver` subcommand implementing the ADR 0003 contract with no format knowledge yet: argument parsing, input reading, marker fallback, exit codes, atomic write of `%A`. Tests build a small repository and run `git merge`, `git rebase`, `git cherry-pick` and `git merge-tree --write-tree` against it (offline), for a "formats refused" stub that always refuses, and for a stub that merges.
  2. PR 2: `lockmerge install` / `uninstall` with idempotency tests on a temporary `HOME` and repository.
  3. PR 3: reading manifests from git objects on the operation's refs (section 6), with tests for each operation and for absent refs.
  4. PR 4: docs: server-side guidance (ADR 0004) and a sample CI workflow.
  5. PR 5 (after section C): `lockmerge check`.
- **Better:** `install --attributes=info`, `--print`, `doctor` (shows whether the driver resolves, which git version, whether the attributes line is active via `git check-attr`); the `merge.conflictStyle` fallback; a test matrix on Windows and macOS.
- **Best:** a published GitHub Action wrapping `lockmerge check`; a pre-push hook installer; a bot-safe repair flow (outward-facing, needs its own ADR and owner sign-off).

## 8. Verification gaps

- Source line numbers: not obtained; `merge-ll.c` and `merge-ort.c` were read through a summarizer. The "git `master`" source was read on 2026-11-20 and may differ from older releases.
- The first file in the `ll_merge` source (`ll-merge.c`) is now `merge-ll.c`; the old name returns 404.
- Temp-file directory and child working directory of the driver: not established. Also not established: whether the driver's environment lets `git cat-file` find the repository (`GIT_DIR` and the like). If it cannot, the driver falls back to lockfile-only rules.
- Whether `MERGE_HEAD`, `REBASE_HEAD`, `CHERRY_PICK_HEAD` and `REVERT_HEAD` exist while git runs the driver, in every operation and for every step (for example the first conflicting step of a rebase): not established. The mapping of refs to base and sides in section 6 is derived from the operations' definitions, not verified.
- Content of `%O` for files added on both sides: not established.
- Whether a driver runs for delete/modify: not established (the part of `merge-ort.c` was not read).
- `git rebase --apply` / `git am -3` with drivers: not established.
- GitHub's server-side behavior: only third-party statements; no official one found. Also not verified: that GitHub's `pull_request` workflows check out a server-made merge commit.
- Whether `pre-push` is skipped by `git push --no-verify`: not in the pages read.
- Minimum git version for `%P`, `%S`, `%X`, `%Y`: unknown.
- Windows shell quoting: not tested.
- `merge-tree --write-tree` honoring drivers: documented as same features, not tested here; the code task must test it.
- No experiment was run: this task is research only, so none of the behavior above was executed.

## 9. The owner's answers

The owner accepted the recommended answers to questions 1 to 6 and changed question 7 because of a later decision (the driver may read manifests from git objects).

| # | Question | Answer |
| --- | --- | --- |
| 1 | When lockmerge refuses but git's text merge would have been clean, what does the driver leave? | Accepted: write a whole-file marker block (ours / theirs) and exit 1. |
| 2 | Where does `lockmerge install` write attributes by default? | Accepted: the committed `.gitattributes`; `--attributes=info` for local-only; `--global` writes the global attributes file. |
| 3 | Is the v1 server-side fallback `lockmerge check` plus documentation and a sample CI workflow? | Accepted: yes. No published Action, no auto-fix bot; the pre-push hook is only an example. |
| 4 | Is the driver command the bare name `lockmerge`? | Accepted: yes; an opt-in `--absolute-path` is a later follow-up. |
| 5 | Is `merge.lockmerge.recursive` left unset? | Accepted: yes. |
| 6 | Do internal errors exit 255 and ordinary refusals exit 1? | Accepted: yes. |
| 7 | Is the R5 check moved out of the driver to `lockmerge check`? | **Changed.** The driver may read the manifests of the base and both sides from git objects, never from the working tree. Manifest agreement is checked in the driver when the objects are available and in `lockmerge check` otherwise. Rule M1 (R3 refuses different bumps of one package unless M1 applies; ADR 0008) and "combine added dependencies" can then be proven. See section 6 and ADR 0003. |
