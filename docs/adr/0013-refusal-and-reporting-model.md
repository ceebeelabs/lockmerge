# 0013. Refusal and reporting model

- Status: Proposed
- Date: 2026-11-20

## Context

Refusals must be explained to the user in plain language and counted by the benchmark by rule and by reason (ADR 0005, 0008). Some rules are refused while the base rules still merge the file (R14, R15).

## Decision

A run returns a `Report` with an `Outcome` (`Clean` with merged bytes or an unchanged input; or `Refused` with a `Refusal`), a list of `Applied` (rule id and subjects, one per accepted unit) and a list of `downgrades` (rule-level refusals that did not end the merge; they are reported and counted by the benchmark but are not whole-file refusals). A `Refusal` is a `ReasonCode` (`<format>.<code>`, for example `cargo.R3`, with a `core.*` family for engine-level reasons), subjects and structured facts. A shipped code is never renamed; a new code is added instead. The user-facing text comes only from the format's `CodeDef` table (title, explanation template, next step) and is printed to stderr. The primary refusal is the first fatal one in a fixed order; the count of others is printed. `Error` (exit 255) is a separate type for failures that are neither a refusal nor "unavailable"; failing to run git or find an object is "unavailable" and leads to R14, not to an error. A panic is caught at the top of `main` with `catch_unwind` and exits 255. Messages say what could not be proven and never suggest an unproven shortcut.

Because `catch_unwind` only works with unwinding, the release profile keeps `panic = "unwind"`, and `panic = "abort"` is never set for the `lockmerge` binary. A test runs the driver with a format that panics and asserts exit code 255 and that `%A` is unchanged.

## Considered options

- Free-form error strings: easy to write, impossible to count or translate into stable benchmark rows.
- String codes plus message tables (chosen): stable keys, one place for wording, zero-filled benchmark tables from the registry.
- Only the first refusal: simpler, but hides downgrades that explain why a rule did not fire.
- `panic = "abort"` for a smaller binary: a panic would then abort the process with a signal exit code instead of 255, and the driver could not guarantee `%A` is left alone.

## Consequences

Benchmark tables are stable across versions as long as codes are not renamed. Each new code needs a `CodeDef` and a refusal fixture. Wording is reviewed once per code. The binary is slightly larger because of unwinding support.

## References

docs/03-architecture.md section 3; ADR 0003, 0005, 0008, 0010.
