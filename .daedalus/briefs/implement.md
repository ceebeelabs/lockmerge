Implementation of one piece of #{{issue}} in {{repo}}, from an owner-approved design or document.

READ FIRST: the design report or document and the owner's decisions linked from the issue, CONTRIBUTING.md and AGENTS.md.

{{prompt}}

HARD RULES
- Never guess (standing rule 3): every new merge rule has a test for what it accepts and a test for the neighboring case it refuses.
- Tests first: a test that fails on `main` and passes with your change.
- Existing behavior stays identical unless the design changes it; pin it in tests before changing anything.
- Committing a document from an accepted research report: commit it as the owner accepted it, plus the owner's decisions, and nothing else.
- One PR for your piece, "Part of #{{issue}}" (or "Fixes #{{issue}}" if your prompt says it is the last piece). Title as given in your prompt. Description per the template, with a table of what is done and what is deliberately not.
