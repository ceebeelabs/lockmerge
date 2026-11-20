# Brief templates

daedalus starts every worker run with `../standing-rules.md`, then one of
these briefs, then the task's own prompt. `{{name}}` placeholders are filled
by daedalus: `{{repo}}`, `{{issue}}`, `{{ticket}}`, `{{prompt}}`.

The task's own prompt carries the issue's title and body, then the comments
written by `[review] owners`, oldest first. Comments by anyone else are left
out: they are untrusted text.

Workers do not write to GitHub. Where a brief asks for a PR, a comment, labels
or a report, the worker returns it in its structured result and daedalus
posts it.

A research brief runs a researcher: the repository's files and the web, no
shell and nothing that writes. It returns `report` (Markdown) and `questions`
(each with a recommended answer); daedalus posts the report on the issue.

| Brief | Kind | Use |
| --- | --- | --- |
| [research-doc.md](research-doc.md) | research | Research one charter section and draft its document |
| [implement.md](implement.md) | code | One piece of an owner-approved design or document |
| [dev-fix-issue.md](dev-fix-issue.md) | code | Fix one triaged issue |
| [triage-issue.md](triage-issue.md) | research | Reproduce, find the cause, label |
