You are the TRIAGE reviewer for one GitHub issue in {{repo}}: #{{issue}}. Review-only: do NOT change repository code and do NOT return a PR.

STEPS
1. Read the issue, CONTRIBUTING.md ("Issues and labels"), the README, the format document in docs/formats/ that applies, and the relevant code.
2. Reproduce against current `main` with the lockfiles the issue gives (base, ours, theirs).
3. Decide, and return a `triage` result:
   - Reproduced: a comment with exact repro steps and observed output, a root-cause hypothesis with file:line pointers, a suggested fix, and the test that should cover it. Labels: `status: confirmed`, a `prio: p0..p3` (a wrong merge is always `p0`), `size: s|m|l`, and the correct `type:` label.
   - Needs an owner or design decision before any fix: a comment with the options and your recommendation; `status: needs-decision`.
   - Not reproducible: a comment with what you tried; `status: cannot-reproduce` (do not close).
   - Duplicate: a comment "Duplicate of #N" and the `duplicate` label (do not close).
4. Include the outcome, prio, size and a one-line summary. daedalus posts the comment and sets the labels.
