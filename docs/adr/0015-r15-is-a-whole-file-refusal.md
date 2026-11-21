# 0015. R15 is a whole-file refusal

- Status: Accepted
- Date: 2026-11-21

## Context

ADR 0013 says some rules are refused while the base rules still merge the file, and names R14 and R15 as examples. That is true of R14 and wrong for R15.

ADR 0008 decision 8 makes the `links` proof a requirement of the merged set: when ours and theirs each add a different registry or git id, the proof of decision 15 must hold, and a failure refuses with R15. Decision 15 lists the cases that refuse with R15 (cache absent or unreadable, entry missing, checksum differs, git source present, a `links` value shared). In each of them the base rules cannot show that the merged set builds, so merging the file anyway would be a guess. The code agrees with ADR 0008: the Cargo code table marks R15 with `CodeDef::fatal`, and the engine treats a fatal code as a whole-file refusal (docs/03-architecture.md, "Which refusals are fatal"; PR #41).

## Decision

R15 is a whole-file refusal. A format returns it as a fatal code, and the run ends in `Outcome::Refused` wherever it is returned. It is never a downgrade.

Only a non-fatal rule refusal, such as R14 (a rule's input is unavailable), can be a downgrade: it is recorded and reported, and the base rules go on merging the file.

This corrects the Context of ADR 0013. ADR 0013 is not edited; its Decision (the `Report`, `Applied` and `downgrades` shape) is unchanged.

## Considered options

- R15 as a downgrade, as ADR 0013's Context reads: the base rules would write a lockfile whose `links` conflicts were never ruled out. This breaks "never guess" and ADR 0008.
- R15 as a whole-file refusal (chosen): matches ADR 0008 decisions 8 and 15 and the code. The cost is that a file with an unprovable `links` case is left to git's conflict markers, which is today's outcome.

## Consequences

Readers of ADR 0013 should take R15 as fatal; this ADR is the correction. A refusal fixture for R15 expects `Refused`, not a clean merge with a downgrade. No code changes: the code already behaves this way.

## References

ADR 0008 decisions 8 and 15; ADR 0013; docs/03-architecture.md; issue #44; PR #41.
