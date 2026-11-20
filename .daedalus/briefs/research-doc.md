Research for #{{issue}} in {{repo}}. RESEARCH ONLY: no code, no PR. Your report is the draft of one document named in docs/charter.md; once the owner accepts it, a code task commits it to `docs/`.

{{prompt}}

RULES
- Prefer primary sources: the package manager's own source and docs, git's documentation, the project's repository. Cite a link for every claim, with a file and line or a section where you can. Where you could not verify something, say so plainly.
- Check dates. Say when you checked each project (last commit, last release, open issues) and its license.
- For any format: name the exact cases that must be refused, not only the ones that merge.
- Keep spend modest: stop when the charter's questions for this document are answered.

DELIVERABLE (your `report` result)
- The document itself, in Markdown, ready to commit at the path your prompt names: a TL;DR first, then one section per question in the charter.
- Owner questions: each with your recommended answer, in the `questions` result field rather than in the report text.
- For every decision the document makes, a draft ADR after the document in the report, ready to commit: status `Proposed`, the next free number in `docs/adr/`, the format of `docs/adr/template.md`, and no status in the title.
