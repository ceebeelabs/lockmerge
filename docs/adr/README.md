# Architecture decision records

lockmerge records its significant decisions as architecture decision records
(ADRs). Each research document in `docs/` ends in the ADRs for the decisions it
makes, and the owner accepts or rejects them.

An ADR is a short note on one significant decision: context, decision,
considered options and consequences. Write one when a decision is hard to
reverse, shapes more than one format or crate, or is one a reviewer would
reasonably ask "why?" about. The ADR goes in the same pull request that makes
the decision.

## Files

- One file per decision, named `NNNN-short-title.md`, numbered in the order
  decided. Use the next free number.
- Start from [template.md](template.md).
- Titles never carry a status.

## Status

Status is one of `Proposed`, `Accepted`, `Rejected`, `Deprecated`, or
`Superseded by [NNNN](NNNN-title.md)`. Date is the date of the last status
change (YYYY-MM-DD).

## Who decides

- Authors only ever write `Proposed`, and only in an open pull request.
- The owner accepts, rejects or asks for a revision before that pull request
  merges. The status and date are then updated in the same pull request, so
  nothing merges as `Proposed`.
- Rejected ADRs are kept.
- An accepted ADR is not rewritten, except for its status, date and
  superseding link. A changed decision is a new ADR that supersedes the old
  one.

## Index

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions as ADRs in `docs/adr/` | Accepted |
