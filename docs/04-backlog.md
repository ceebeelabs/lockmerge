# 04. Backlog

Later work and known risks, grouped. This is a list, not a plan: each item is
one or two sentences, and the owner orders it. Each group links its source
issues; an item's detail lives in the document it names.

## Driver

Source: [#22](https://github.com/ceebeelabs/lockmerge/issues/22), [#23](https://github.com/ceebeelabs/lockmerge/issues/23)

- Server-side fallback (ADR 0004): hosted merges run no driver, so a `lockmerge check` path must catch what the driver would have refused.
- Which of `HEAD`-relative commits the driver can name for rebase and cherry-pick is known only from the probe in ADR 0010 (git 2.50.1); other git versions are unverified.

## Cargo rules

Source: [#24](https://github.com/ceebeelabs/lockmerge/issues/24)

- Pruning packages no longer reachable from a path package may resolve one or two more of the seven reachable prior-art cases; not yet specified as a rule.
- Cargo behavior noted as unverified by running Cargo (for example version upgrade on rewrite, `docs/formats/cargo-lock.md` section 2) needs a check before a rule depends on it.

## Benchmark

Source: [#25](https://github.com/ceebeelabs/lockmerge/issues/25)

- Extraction tool and fixture set from public repositories (`docs/02-oracle.md`); the resolution-rate numbers so far come from 12 prior-art cases only.
- A lower bound printed beside every rate, so a lucky run is not read as proof.

## Architecture

Source: [#26](https://github.com/ceebeelabs/lockmerge/issues/26)

- A shared span-splicing helper in core once two formats need byte-exact output, and a pipeline helper for the common keyed-packages shape.
- Risks, unverified: toml_edit span support for read-only documents, whether saphyr spans are byte offsets, and the exact registry index entry layouts.

## Other formats

Source: [#28](https://github.com/ceebeelabs/lockmerge/issues/28)

- npm, Yarn, pnpm, Poetry, uv, Go and Bundler lockfiles each need a format document and rules before any code; the YAML library choice waits for the pnpm research.
- Opt-in cargo validation mode (ADR 0009) and a machine-readable CLI report for CI.
