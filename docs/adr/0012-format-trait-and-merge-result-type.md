# 0012. Format trait and the merge result type

- Status: Proposed
- Date: 2026-11-20

## Context

"Never guess" (charter, standing rule 3) must hold for every format, written by different workers. Reviewing every code path for an unproven merge does not scale.

## Decision

The CLI and benchmark use an object-safe `Format` trait. A format author implements the typed `Pipeline` trait (`parse`, `envelope`, `units`, `decide`, `validate`, `serialize`); a blanket impl supplies `Format` through `engine::run`. The engine applies generic base rules, then asks the format's `decide` for the remaining cases, whose default is `NotApplicable`, which refuses. `Proof`, `Draft`, `Sealed` and `Report::Clean` have private fields and crate-private constructors. `serialize` takes only a `Sealed` value, which exists only after every unit has a `Proof` naming a rule in `Pipeline::rules()` and all `validate` checks pass. Unchanged inputs are returned byte for byte under core rule `core.S1`. Each `RuleDef` needs an accept and a refuse fixture directory under `tests/fixtures/rules/<format>/<rule>/`, enforced by a lint in the testkit.

## Considered options

- One `merge(bytes) -> bytes` method per format: simplest, but nothing forces a rule or a proof, and any format can skip validation.
- Typed pipeline with sealed results (chosen): a little more ceremony per format; accepted merges always name a rule, and refusal is the default.
- A rule DSL shared by all formats: too constraining for Cargo's resolved-domain rules (M1, C, F).

## Consequences

Every accepted merge is attributable to a rule, which the benchmark counts. A rule can still be wrong; the type system does not prove correctness, so the pair tests and the zero-wrong-merge gate (ADR 0006) stay required. A format whose structure does not fit "keyed units" can use one unit for the whole file, at the cost of fewer accepted merges.

## References

docs/03-architecture.md section 2; ADR 0008 decisions 3, 8, 9, 11, 13.
