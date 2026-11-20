# 0011. Crate layout

- Status: Proposed
- Date: 2026-11-20

## Context

lockmerge starts with Cargo.lock and adds uv.lock and pnpm-lock.yaml (ADR 0002). Formats are built by separate workers. Git access and the Cargo registry cache are side effects (ADR 0003, 0010, 0008 decision 15), and merge logic must be testable on fixtures without them.

## Decision

The repository is a Cargo workspace with these crates: `lockmerge-core` (types, traits, engine, refusal model, testkit feature that is off in the release binary; depends on std only), one crate per format (`lockmerge-cargo`, later `lockmerge-uv` and `lockmerge-pnpm`; depend on core and their parsers), `lockmerge-env` (the only crate that runs git or reads the registry cache), `lockmerge-cli` and `lockmerge-bench`. Core and format crates use `#![forbid(unsafe_code)]` and a clippy `disallowed-methods` list for `std::process`, `std::fs` and `std::env`.

The Cargo registry index format knowledge stays in `lockmerge-cargo` as pure functions over bytes: the relative path of an entry, parsing sparse and git index entries, matching name, version and cksum against the lockfile checksum, and reading `links`. `lockmerge-cargo` defines the `RegistryIndex` trait; `lockmerge-env` implements it by only listing directories and reading files, and hands the bytes over. It depends on `lockmerge-cargo` for that trait and the path helper only. The proof logic is thus testable on fixtures without a `CARGO_HOME`.

The index cache directory name `index.crates.io-<hash>` is not computed, because the hash depends on the Cargo version (it changed in Cargo 1.85) and several such directories can exist. `lockmerge-env` lists `registry/index/` under `CARGO_HOME` for candidates. An entry from any candidate counts only if name, version and cksum match the lockfile exactly; this is safe because a published version's index entry, including `links`, is immutable (only `yanked` changes). If no directory gives a matching entry, R15 refuses. This extends ADR 0008 decision 15 without changing it.

## Considered options

- Modules per format in one crate: fewer manifests, but parser dependencies leak across formats, workers' changes collide, and nothing stops cross-format coupling.
- Crate per format: one more manifest each, but disjoint directories, isolated dependencies and compiler-enforced boundaries.
- Side effects inside format crates: simpler, but the core is no longer pure and fixture tests need a real git and a real cache.
- Index parsing inside `lockmerge-env`: fewer trait calls, but the proof logic could then only be tested with a real cache.

## Consequences

Formats build and test independently. Purity is enforced by lints. Registering a format is one line in `lockmerge-cli`. `lockmerge-env` depends on one format crate for a trait; moving the trait to core would remove that, at the cost of a Cargo-specific type in core. Listing several cache directories means a lockfile can be proven from an entry cached by another Cargo version, which is sound because entries are immutable once published.

## References

docs/03-architecture.md section 1; ADR 0002, 0003, 0008, 0010.
