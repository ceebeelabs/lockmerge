Standing rules for all work in this repository. They come before the brief.

1. Commits, docs and code comments carry no `Co-authored-by` trailer for an AI tool, no "Generated with" line and no claude.ai links. Descriptions say what changed and why, and nothing else about credit.
2. Write in American English. Treat the repository as open source: no secrets, tokens or machine-specific absolute paths; public-quality commit messages.
3. Never guess. Merge logic combines only what it can prove unambiguous and refuses everything else. Every new merge rule comes with a test for the case it accepts and a test for the neighboring case it must refuse. If you can't tell whether a case is safe, refuse it and say so in your result.
4. Fixtures are exact bytes. Lockfiles under `tests/fixtures/` are test data: write them exactly, byte for byte, and never regenerate them with a package manager. Say where each fixture came from (a real repository and commit, or a reduction of one).
5. Adding a crate: add it to `Cargo.toml` with a version requirement (an exact or caret version of a release; never a git or path dependency from outside the repository) and let `cargo build` or `cargo update -p <crate>` update `Cargo.lock`. Commit both. Never hand-edit the root `Cargo.lock`. Your network reaches only `index.crates.io` and `static.crates.io`, so git dependencies and other registries fail.
6. A PR that adds a crate names it, why it's needed, and its license. The license must be compatible with MIT OR Apache-2.0 (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0); anything else (GPL, LGPL, MPL and the like) needs the owner's sign-off first. Prefer maintained, widely used crates that don't pull in a large tree for a small need.
7. Never fetch the public repositories the benchmark replays. Fixture extraction runs in CI or by hand, and its output is committed as extracted base, ours, theirs and expected files.
8. Good / Better / Best: build the simplest solid version that works end to end. Don't gold-plate. Return better and best ideas as follow-ups.
9. You work in your worktree and commit there. You do not push, open PRs, comment on GitHub or start reviews; daedalus does that from your structured result. Your PR description follows .github/pull_request_template.md: Summary, Test evidence, Follow-ups, and the checklist answered explicitly.
10. "Part of #N" for an intermediate piece of an issue; "Fixes #N" only on the piece that completes it. Say which in your result.
11. Self-review before you finish a turn: re-read the whole diff against CONTRIBUTING.md "Reviewing". Fix what you find first.
12. Review fixes: take every required change and the optional notes you agree with in one round. For each finding answer `fixed` or `disputed` with a reason; for each optional note `taken` or `deferred` with a reason.
13. Once the Cargo workspace exists, run `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` and the tests of the crates you touched before you finish. Run cargo commands in the foreground with the shell tool's longest timeout, not in the background.
14. Keep context lean: read the sections your brief points to, not whole documents.
