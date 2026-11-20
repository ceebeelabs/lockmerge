# 0014. Parsers are read-only; `toml_edit` for TOML

- Status: Accepted
- Date: 2026-11-20

## Context

ADR 0008 forbids re-serializing Cargo.lock through a TOML library. The same risk applies to YAML and JSON. Documented behavior of `toml_edit` (docs.rs, checked 2026-11-20) preserves comments, spacing and order, but not the order of dotted keys or a missing trailing newline, so it is not byte-exact. `serde_yaml` is deprecated (0.9.34+deprecated, 2024-03-25). `yaml-rust2` and `saphyr` are maintained, but none of the candidates is known to round-trip byte for byte.

## Decision

Parsing libraries are used to read inputs only. Output is either an input returned byte for byte, or bytes written by code in the repository (a canonical writer or a splice of input byte ranges). Clippy forbids the libraries' writers in format crates. For TOML (`Cargo.lock`, `uv.lock`, manifests) the parser is `toml_edit` (MIT OR Apache-2.0), walked as an item tree so that any form we do not handle is refused rather than skipped. The YAML library for `pnpm-lock.yaml` is not decided here; the pnpm format research chooses it. This ADR fixes only the read-only policy and the shortlist: span-carrying parsers, with `saphyr-parser` as the first candidate; `serde_yaml` is excluded. JSON is not needed in v1.

## Considered options

- Serialize through `toml_edit` or a serde model: short code, but not byte-exact and unverifiable against the exact-bytes gate.
- Hand-written parsers for each format: full control, but more code to get wrong and test.
- Library parser, own writer (chosen): reuses mature parsing, keeps output under our tests.

## Consequences

Each format needs its own writer or splice code. Parser upgrades cannot change output bytes. Adding a crate follows standing rules 5 and 6; `toml_edit` and `semver` are MIT OR Apache-2.0.

## References

docs/03-architecture.md section 4; ADR 0008 decision 11; https://crates.io/crates/toml_edit; https://docs.rs/toml_edit/latest/toml_edit/; https://crates.io/crates/serde_yaml; https://crates.io/crates/saphyr.
