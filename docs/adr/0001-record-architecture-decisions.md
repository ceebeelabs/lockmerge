# 0001. Record architecture decisions

- Status: Proposed
- Date: 2026-11-20

## Context

lockmerge's rule is to combine only what is provably unambiguous and refuse
everything else. Decisions about which cases to refuse, which formats to
support and how to serialize them are hard to reverse and shape several
formats and crates. The research phase will produce many of them, and a
reviewer or later contributor needs to find out why one was made without
reading old pull request threads.

## Decision

We keep architecture decision records in `docs/adr/`, in the lightweight MADR
format of [template.md](template.md), under the process in
[README.md](README.md). Each research document ends in the ADRs for the
decisions it makes. Authors write them as `Proposed`; the owner accepts or
rejects them before the pull request merges.

## Considered options

- No records: the reasons live in pull request threads and documents. Cheapest,
  but hard to search and easy to lose.
- Decisions only inside the research documents: they sit next to the evidence,
  but a document covers many decisions and is rewritten as it is revised, so
  no single decision has a stable status or number.
- ADRs, one file per decision: a stable number and status per decision, a
  small file to review, and rejected options are kept. Costs a short note per
  decision.

## Consequences

- Every significant decision gets a file in the pull request that makes it,
  and a reviewer can ask for one when it is missing.
- The owner holds the accept/reject decision, so nothing merges as `Proposed`.
- Changing a decision means writing a new ADR that supersedes the old one.
- Numbering, titles and the index are checked by hand for now; an automated
  lint can come with the Cargo workspace.

## References

- [Issue #7](https://github.com/ceebeelabs/lockmerge/issues/7)
- [Architectural Decision Records](https://adr.github.io/) and
  [MADR](https://adr.github.io/madr/)
