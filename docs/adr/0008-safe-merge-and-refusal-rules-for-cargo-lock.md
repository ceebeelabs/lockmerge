# 0008. Safe-merge and refusal rules for Cargo.lock

- Status: Proposed
- Date: 2026-11-20

## Context

lockmerge merges lockfiles only when it can prove the result valid, and
refuses otherwise (ADR 0002, standing rule 3). Cargo.lock edges are written in
a shortened form: the version is omitted when a name has one version in the
file, and the source when a name and version has one source. A line-based or
generic-TOML merge can produce a file that parses but points an edge at a
different package, or has duplicate or dangling entries (cargo#2302; the
mergiraf run in docs/00-prior-art.md section 7).

Targets (owner, 2026-11-20): zero wrong merges is a hard gate. The headline
metric is the resolution rate on reachable conflicts, those whose committed
resolution uses only package ids present in base, ours or theirs. v1 ships at
60% or more of reachable Cargo.lock conflicts, stretch 80%. Agent-style
conflicts (two branches each adding different crates) have a separate bar of
90% or more. In the 12 real conflicts of the mergiraf run, 5 were re-resolved
by the merger with ids on neither side and are refused by design; 7 are
reachable. Rules that refuse everything both sides touched resolve 2 of 7.

ADR 0003 lets the driver read Cargo.toml files from git objects, never from
the working tree. How the driver finds the commits and the merged manifest is
decided in ADR 0010, which amends ADR 0003.

## Decision

1. Support lockfile versions 3 and 4 only. Refuse 1, 2, 5, a missing or
   unrecognized `version`, and inputs whose versions differ.
2. Merge in a resolved domain: key packages by (name, version, source),
   resolve every edge to a full id per input file, merge three-way per id,
   then write canonical text. Compare edges by what they resolve to, never by
   text.
3. Base rules (lockfile only): accept identical entries, entries changed on
   one side, entries added on one side, and removals of entries the other side
   left unchanged. Refuse a checksum difference, both sides changing or adding
   the same id differently, and removal against modification, except where
   rules M1 to M3 apply.
4. M1, both sides bumped one registry crate to semver-compatible versions:
   keep the higher version only if (a) every path package with an edge into the
   crate declares a requirement the higher version satisfies in the merged
   manifest, and (b) every non-path package with an edge into the crate exists
   with the same id and checksum in the side that has the higher version, with
   its edge to the higher version there, and (c) the declarations of the crate
   in those path packages' merged manifests, including features and
   `default-features`, equal those of the higher side, since its edges cover
   only the features it was resolved with; this covers every registry and git
   dependency of every path package, not only the bumped crate, because
   features reach it indirectly. Otherwise refuse. Prereleases and
   different compatibility classes are refused.
   B1, any one-sided version change of a crate present in base is taken only
   if every path package with an edge into it has a merged-manifest
   requirement the new version satisfies; if manifests are unavailable and such
   a package exists, refuse. This covers a manifest-only pin on the other side.
5. M2, both sides changed the same path package's edges: combine only if the
   merged manifest is fully readable and its declared dependency names equal
   the merged edge names, each registry edge satisfies its declared
   requirement, each path edge matches its declared path and satisfies its declared `version`
   requirement if it has one, git edges are
   identical in all inputs, and every target exists in the merged set.
   Otherwise refuse.
6. M3, pruning: drop a non-path package that is removed on one side and
   changed on the other, or left without incoming edges, only if no remaining
   edge points to it and it was reachable in every input where it appears.
7. Refuse also: any ambiguous or unresolvable edge; two path packages with the
   same name and version; any difference in `[[patch.unused]]`, `[root]`,
   `[metadata]`, `replace` or unknown keys; a source change for the same name
   and version; prerelease versus release differences.
8. Validate the merged set and refuse on failure: no duplicate ids; every edge
   resolves to exactly one package; every non-path package reachable from a
   path package; no two semver-compatible registry versions of one crate;
   output sorted and canonical.
9. The merged id set is a subset of the union of the inputs' ids; checksums
   are copied from inputs; no id is invented.
10. Manifests come from git objects only. The merged manifest is the
    `git merge-file` result of the three manifest blobs, and the commits are
    identified as ADR 0010 decides. When the commits cannot be identified, a
    manifest is missing on a side or conflicts, or a manifest form is not
    read, use the base rules plus M3 and the manifest-free part of M1 for the
    whole file. See docs/formats/cargo-lock.md section 4.5.
11. Return unchanged inputs byte for byte; never re-serialize through a TOML
    library.
12. The driver does not run Cargo. `cargo metadata --locked --offline` runs
    in the benchmark and CI only.
13. Each rule is tested with a case it accepts and a neighboring case it
    refuses.
14. The v1 bars are judged on the corpus of issue #11 (docs/02-oracle.md and
    ADRs 0005 to 0007): at least 60% of at least 40 reachable real
    Cargo.lock conflicts with a 95% lower bound of at least 50%, and at least
    90% on the agent-style corpus. The owner's 7 reachable cases are kept as
    a regression set with per-rule attribution.

## Considered options

- Line merge plus `cargo generate-lockfile` (cargo-merge-driver): needs the
  network, may upgrade unrelated crates.
- Generic structured TOML merge (mergiraf): produced invalid lockfiles with
  exit 0 in 5 of 12 real conflicts.
- Base rules only: 2 of 7 reachable conflicts (29%), below the 60% target.
- Resolved-domain merge with M1 to M3 (this decision): estimated 5 to 6 of 7
  (71% to 86%) with manifests; needs a manifest reader.
- Guess the other side's commit (for example by lockfile blob match):
  ambiguous when commits touch only Cargo.toml. Rejected.

## Consequences

- Easier: every accepted merge has a stated proof and a counter-test.
- Harder: Cargo's edge shortening, sort order and requirement semantics must
  be implemented exactly; a manifest reader (workspace members, inheritance)
  is needed; fixtures stay byte-exact.
- M2 and part of M1 run only where the commits are identified and the
  manifests merge cleanly as text; elsewhere the fallback applies and the 60%
  target is not assured.
- The proof is relative to the text merge of the manifests; a different later
  edit of Cargo.toml is caught by the CI check, not by the driver.
- Open: source ordering, array formatting, the 0.x unification rule and
  whether the lockfile lists optional and dev edges are unverified.

## References

- docs/formats/cargo-lock.md
- [ADR 0003](0003-merge-driver-contract.md), [ADR 0010](0010-how-the-driver-finds-the-commits.md)
- https://github.com/rust-lang/cargo/issues/2302
- https://github.com/rust-lang/cargo/pull/7070
- https://github.com/rust-lang/cargo/blob/master/src/resolver/encode.rs
- https://doc.rust-lang.org/cargo/faq.html
- https://git-scm.com/docs/gitattributes
