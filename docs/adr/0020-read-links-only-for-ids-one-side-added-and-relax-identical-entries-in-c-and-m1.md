# 0020. Read `links` only for ids one side added, and use the identical-entry relaxation in C and M1 e (amends ADR 0008 decision 15 and ADR 0016 decision 3)

- Status: Accepted
- Date: 2026-11-21

## Context

On the agent-style corpus (#88, #101) eight refusals with code R15 (alacritty cases al-01, 02, 03, 04, 12, 17, 19 and 20) have one cause. A git package, `x11-clipboard`, sits unchanged in base, ours, theirs and the expected merge. ADR 0008 decision 15 refuses whenever a git source id is in the merged set, so the V6 proof fails, although the package is not new.

Each side's lockfile is a Cargo resolution of that side's whole package set, git packages included. Two packages that share a `links` value cannot both be in one such set. So a collision in the merged set can only be between an id that only ours added and an id that only theirs added. Ids both sides hold cannot be one of the two.

ADR 0016 decision 3 lets M2 accept a plain registry declaration that the side lacks, for a crate whose entries (id, checksum and resolved dependency edges) are identical in both lockfiles, because the branch that added the declaration was resolved with it in force. C(i) and M1 e compare manifests the same way, but refuse the same declaration. Case al-08 (both branches add `hex`, to different members, with an identical entry) refuses for that reason alone.

## Decision

1. V6 (rule R15) reads `links` for registry ids (as today, from the checksum-matched index cache; the proof runs only when each side added a different registry id) and for path packages (from their merged manifests). A git id that is present in both ours and theirs is not read. A git id that only one side holds, including every git id a side added, still refuses as today.
2. The identical-entry relaxation of ADR 0016 decision 3 applies to C(i) and M1 e as well as M2: a plain registry declaration the side lacks (no features, default features, not optional), for a crate the side's lockfile holds, passes if the crate's entries, and every package reachable from them, are identical in both lockfiles, and refuses if any differ: a feature the declaration forwards can reach a transitive crate that one branch upgraded.

## Considered options

- Option A: keep ADR 0008 decision 15 and ADR 0016 decision 3. Nothing changes; nine cases of the corpus stay refused.
- Option B (chosen): the two narrow changes above. Each is an argument from what a resolution already proves, and each keeps its refusal for the neighbor.
- Option C: read the `links` of a git source from its manifest. This needs a download or a vendored copy, which a merge driver does not have.

## Consequences

- Merges with a git dependency that no branch touched are no longer refused by R15 when the branches add different registry crates. Added git packages still are.
- Declarations of held crates with identical entries stop refusing in C and M1 e. Entries that differ still refuse with FEATURES.
- Cases that stay refused: R3 (al-22, al-23, al-28), R8 (digest, al-05, bat-03, fd-05), FEATURES with differing entries (bat-13, bat-14, al-10, al-13) and the R14 dependency-target cases. They are correct refusals under the accepted ADRs.

## References

- Issue #101 and the owner's decision on it.
- ADR 0008 decision 15, ADR 0016 decision 3; `docs/formats/cargo-lock.md` sections 4.5 and 4.6.
