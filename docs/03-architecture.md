# 03. Architecture and the format trait

Status: draft for ADRs 0011 to 0014 (all Proposed). Research for issue #12,
charter section E. Crate metadata was checked on crates.io on 2026-11-20. No
crate source code was read. Points that could not be verified are marked
"unverified".

## TL;DR

**A Cargo workspace with one pure core and one crate per format, plus a
separate crate for side effects.**

| Crate | Role | Side effects |
|---|---|---|
| `lockmerge-core` | Types, the `Pipeline` and `Format` traits, the three-way engine, the refusal model, the rendering of reasons, `testkit` (feature) | none |
| `lockmerge-cargo`, later `lockmerge-uv` and `lockmerge-pnpm` | One format each: parser, model, rules, canonical writer. For Cargo this includes the registry index entry format, as pure functions over bytes | none |
| `lockmerge-env` | Runs `git` (identify commits, `cat-file`, `merge-file`); lists directories and reads files of the Cargo registry index cache and hands the bytes over | the only crate that spawns processes or reads files for the driver |
| `lockmerge-cli` | The driver command line (ADR 0003 and 0010), exit codes, printing | calls `lockmerge-env` |
| `lockmerge-bench` | Replay harness (ADR 0005 to 0007), per-rule and per-reason tables | temp git repos, real tools |

**Never guess is a type-level rule.**

- A merge result can only be built by the engine. Each unit of the output
  carries a `Proof`, which names a registered rule.
- `serialize` accepts only a `Sealed` value. The engine produces `Sealed` only
  after the named whole-file validators pass.
- Anything the rules do not accept is refused. That is the default path, not an
  extra check.

**The proof is a label, not a theorem.**

- The compiler guarantees that every accepted byte has a named rule and that
  nothing skips validation.
- It does not guarantee that a rule is correct. That comes from the accept and
  refuse test pair of every rule and from the zero-wrong-merge gate (ADR 0006).

**Parsers.**

- `toml_edit` is used read-only for `Cargo.lock`, `uv.lock` and manifests.
- No library ever writes lockfile bytes (ADR 0014).
- The YAML choice for `pnpm-lock.yaml` is deferred to its format research. The
  shortlist and constraints are in section 4.

## 1. Crate layout

```
crates/
  lockmerge-core/    pure: types, traits, engine, reasons, testkit (feature)
  lockmerge-cargo/   pure: Cargo.lock (+ manifests) format, registry index entry format
  lockmerge-uv/      later, same shape
  lockmerge-pnpm/    later, same shape
  lockmerge-env/     side effects: git subprocess, registry cache directory listing and file reads
  lockmerge-cli/     binary `lockmerge`: merge-driver, install, doctor
  lockmerge-bench/   binary `lockmerge-bench`: replay harness, tables, extractor
tests/fixtures/      exact-byte fixtures (ADR 0007)
```

**A crate per format, not a module.**

- Disjoint directories let workers build formats in parallel, and their PRs
  rarely touch the same files. The one shared line is the registration in
  `lockmerge-cli/src/formats.rs`.
- Parser dependencies stay inside the format that needs them. The YAML library
  never enters the Cargo build.
- A format cannot reach another format's internals or the side-effect code. It
  sees only `lockmerge-core` traits.
- The cost is one more `Cargo.toml` per format. That is small.

**Dependency direction.**

- `core` depends on nothing but `std`.
- `cargo`, `uv` and `pnpm` depend on `core` and their parsers.
- `env` depends on `core` only. The `IndexCache` trait that `CargoHome`
  implements lives in `core`, next to `Manifests`, so `env` needs no format
  crate (this replaces the first plan, a `RegistryIndex` trait in
  `lockmerge-cargo`).
- `cli` and `bench` depend on everything.
- `core` and the format crates use `#![forbid(unsafe_code)]`. Clippy
  `disallowed-methods` forbids `std::process`, `std::fs` and `std::env` in
  them. This is how purity is enforced.
- The `testkit` feature of `lockmerge-core` is off in the release binary.

**Side-effect boundaries.**

- Git. The `Manifests` trait lives in `core`. Its method is
  `merged(path) -> ManifestResult`, where `ManifestResult` is
  `Merged(bytes) | Unavailable(Why) | Conflict`. The `Git` implementation in
  `lockmerge-env`:
  - identifies the commits from `%S/%X/%Y` and `GITHEAD_<sha>` (ADR 0010);
  - runs `git cat-file` for each blob;
  - runs `git merge-file` over the three manifest blobs.
  - Any failure, including git not starting, is `Unavailable`. The format then
    applies R14, which is the safe direction.
- Registry cache. The knowledge of the index format stays in `lockmerge-cargo`
  as pure functions over bytes:
  - the relative path of a crate's entry inside an index directory, from the
    crate name;
  - parsing a cached index entry file (the cache layout is pinned in
    `docs/formats/cargo-registry-index.md`; a git index's JSON files are git
    objects and are not read);
  - matching name, version and `cksum` against the lockfile's checksum;
  - reading `links`.

  `lockmerge-env::CargoHome` only lists directories, reads files and hands the
  bytes over. It decides nothing. The proof logic is therefore testable on
  fixtures without a `CARGO_HOME`. The sketch:

  ```rust
  // In core. Implemented by lockmerge-env (real files) and the testkit.
  pub trait IndexCache {
      /// The bytes of the file at `rel_path` in each directory of
      /// registry/index/ that has it; empty if the cache is absent.
      fn files(&self, rel_path: &str) -> Vec<Vec<u8>>;
  }

  // Pure, in lockmerge-cargo (`index`), pinned in docs/formats/cargo-registry-index.md:
  pub fn entry_path(name: &str) -> Option<String>;       // ".cache/ab/cd/abcdef"
  pub fn parse_entry(bytes: &[u8]) -> Result<Vec<Record>, EntryError>;
  pub fn lookup(files: &[Vec<u8>], name: &str, version: &str, cksum: &str) -> IndexLookup;
  // IndexLookup: Found { links: Option<String> } | Unavailable(reason)
  ```

- Registry directory selection. The cache directory name
  `index.crates.io-<hash>` is not computed. The hash depends on the Cargo
  version (it changed in Cargo 1.85), so several such directories can exist.
  `lockmerge-env` finds candidate index directories by listing
  `registry/index/` under `CARGO_HOME`. An entry from any candidate directory
  counts only if name, version and `cksum` match the lockfile exactly. This is
  safe because a published version's index entry, including `links`, is
  immutable (only `yanked` changes). If no directory gives a matching entry,
  R15 refuses. This extends ADR 0008 decision 15 without changing it. An absent
  or unreadable cache, or an entry format that cannot be parsed exactly, is
  `IndexLookup::Unavailable`, which is R15.
- The conflict markers for a refusal are written by the CLI through `env`
  (`git merge-file` on the three lockfiles, ADR 0003). The core never produces
  them.
- Tests and the benchmark replace both boundaries with fakes (`FakeManifests`
  and `FakeIndex` in `testkit`, built from a rule fixture's `manifests/merged/` directory).
  `FakeIndex` returns fixture bytes through the same pure functions the real
  implementation uses.

## 2. The format trait

Two levels. `Format` is the object-safe boundary used by the CLI and the bench.
`Pipeline` is the typed contract a format author implements. A blanket impl
provides `Format` for every `Pipeline` through `engine::run`. A format
therefore has no way to return `Merged` except through the engine.

```rust
// ---- lockmerge-core ------------------------------------------------
pub struct FormatId(pub &'static str);                 // "cargo"
pub struct ReasonCode { pub format: FormatId, pub code: &'static str } // cargo.R3

pub struct RuleDef { pub id: &'static str,            // "cargo.M1"
                     pub summary: &'static str,       // one sentence, for reports
                     pub adr: &'static str }          // "0008#4"
pub struct CodeDef { pub code: ReasonCode,
                     pub title: &'static str,         // plain language, no jargon
                     pub explain: &'static str,       // template with {placeholders}
                     pub next_step: &'static str,     // what the user can do
                     pub fatal: bool }                // whole-file refusal? (R15: true, R14: false)

pub struct Inputs<'a> { pub base: &'a [u8], pub ours: &'a [u8], pub theirs: &'a [u8],
                        pub path: &'a str }
pub trait Manifests { fn merged(&self, repo_path: &str) -> ManifestResult; }
pub struct Context<'a> { pub manifests: &'a dyn Manifests }
impl Context<'_> { pub fn proof(&self, rule: &'static str, facts: Vec<(&'static str, String)>) -> Proof; }

/// Object-safe; what the CLI and bench see.
pub trait Format: Sync {
    fn id(&self) -> FormatId;
    fn matches(&self, file_name: &str) -> bool;
    fn rules(&self) -> &'static [RuleDef];            // for lint and zero-filled tables
    fn codes(&self) -> &'static [CodeDef];
    fn merge(&self, inputs: &Inputs, cx: &Context) -> Result<Report, Error>;  // Err: internal error, exit 255
}

/// Typed contract a format implements.
pub trait Pipeline {
    type Model;                                       // parsed file
    type Key: Ord + Clone + std::fmt::Display;        // cargo: (name, version, source)
    type Unit: Clone + Eq;                            // cargo: entry with resolved edges
    fn id(&self) -> FormatId;
    fn rules(&self) -> &'static [RuleDef];
    fn codes(&self) -> &'static [CodeDef];
    fn matches(&self, file_name: &str) -> bool;

    fn parse(&self, role: Role, bytes: &[u8]) -> Result<Self::Model, Refusal>;       // R1, R9
    fn envelope(&self, m: [&Self::Model; 3], cx: &Context) -> Result<Vec<Proof>, Refusal>; // version, R10
    fn units(&self, m: &Self::Model) -> std::collections::BTreeMap<Self::Key, Self::Unit>;
    /// Called only for the cases the engine's base rules do not accept.
    /// The default of every implementation must be `NotApplicable`.
    fn decide(&self, c: &Conflict<Self::Key, Self::Unit>, cx: &Context) -> Verdict<Self::Unit>;
    /// After the per-key pass, before `validate`: units to remove (M3), each with a Proof.
    /// Default: none.
    fn prune(&self, d: &Draft<Self::Key, Self::Unit>, cx: &Context) -> Vec<Removal<Self::Key>>;
    /// Whole-set checks (ADR 0008 decision 8, including R15) on the pruned draft.
    fn validate(&self, d: &Draft<Self::Key, Self::Unit>, cx: &Context) -> Findings;
    /// Takes only a Sealed value. Canonical writer; never a TOML/YAML library's writer.
    fn serialize(&self, s: &Sealed<Self::Key, Self::Unit>, m: [&Self::Model; 3]) -> Vec<u8>;
}

pub enum Verdict<U> { Accept(U, Proof), Refuse(Refusal), NotApplicable }
pub struct Removal<K> { pub key: K, pub proof: Proof }
pub struct Findings { pub proofs: Vec<Proof>, pub refusals: Vec<Refusal> } // all failures, collected
```

**What the engine does** (`engine::run`):

1. If two inputs are byte-identical, or one equals base, return
   `Outcome::Clean` with `How::Unchanged(side)`, under core rule `core.S1`.
   These are the unchanged inputs of ADR 0008 decision 11. They are never
   re-serialized.
2. `parse` the three inputs and apply `envelope`.
3. For each key, apply the core base rules `core.B1` to `core.B4` (identical;
   one-sided change; one-sided add; removal of an unchanged entry). They are
   generic and live in `core`, which is the three-way engine of the charter.
4. For every other case, ask `decide`. `NotApplicable` for every case ends in
   refusal `core.UNPROVEN` with the key. A format may refine this to its own
   code from `decide` (`Verdict::Refuse`, for example `cargo.R3`).
5. Ask `prune` for removals (M3). Each carries a `Proof`; the engine checks the
   rule and that the key is in the draft, removes it and records it as applied.
6. Run `validate` on the pruned draft. All failures are collected. Any fatal
   one refuses.
7. Mint `Sealed`. Only now may `serialize` be called.

**How "never guess" is enforced structurally.**

- `Proof`, `Draft`, `Sealed` and `Clean` (the payload of `Outcome::Clean`) have
  private fields. Their constructors are `pub(crate)` in `core`. A format gets
  a `Proof` only from `Context::proof(rule, facts)` and hands it back in
  `Verdict::Accept`, `Removal`, `Findings` or the `envelope` result.
- `Context::proof` is the one place a rule enters. The engine checks that
  `rule` is in `Pipeline::rules()` when it sees the proof. If it is not, the
  result is an `Error` (exit 255), never a merge. The engine also refuses a
  registry that lists a `core.*` rule, so a format cannot forge a core rule.
- `serialize` takes `&Sealed`. So there is no code path from parsed input to
  written bytes that skips the engine, a `Proof` per unit and the validators.
- `Clean` has no public constructor, so no code outside the engine can produce
  `Outcome::Clean`.
- Refusal is the default. Adding a rule can only add accepted cases. A
  forgotten case refuses.
- Limits: a rule author can write a wrong rule. A format can mint a `Proof`
  through `Context::proof` with a true-sounding rule. The defenses are review, the pair
  test below, and the zero-wrong-merge gate with the benchmark's `applied`
  table, which shows which rule made a wrong merge if one happens.

**Settled in issue #29.** Three gaps in the first sketch, closed without
changing an ADR.

- *Who builds a `Proof`.* Format crates cannot call a crate-private
  constructor, yet `decide` must return a `Proof`. The constructor stays
  crate-private (ADR 0012). `Context::proof` is the public way to get one; it
  is a label maker with no power on its own, because the engine checks every
  proof against `Pipeline::rules()` and `Sealed` exists only after the checks.
  `Proof::new(rule, facts)` is therefore not public API.
- *Removing units.* `validate` only reads the draft. M3 pruning is the explicit
  `prune` step, run after the per-key pass and before `validate`. It returns
  `Removal { key, proof }` values. The engine checks the rule and the key
  (an unknown or repeated key is an `Error`) and records each as `Applied`.
- *Which refusals are fatal.* `CodeDef::fatal` says it, per code. A fatal code
  (R15, parse and envelope refusals, `core.UNPROVEN`) is a whole-file refusal
  wherever it is returned. Only a non-fatal code (R14) can be a downgrade, and
  only where the engine can continue: from `decide` (recorded, and the key is
  then refused as `core.UNPROVEN`, because nothing merged it) or from
  `validate`. `parse` and `envelope` refusals always end the run.

**Where the accept and refuse tests live.**

- Each rule and each reason code has a pair of fixture directories in the
  ADR 0007 layout (`base`, `ours`, `theirs`, `expected`,
  `manifests/merged/`, `case.toml`):
  - `tests/fixtures/rules/<format>/<rule>/accept-<slug>/`
  - `tests/fixtures/rules/<format>/<rule>/refuse-<slug>/`
- `case.toml` of a refusal case holds `outcome = "refused"` and
  `code = "cargo.R3"`. The `refuse-` case must be the neighboring case that
  fails the rule's condition. Hand-minimized cases are labeled "reduction of
  <repo>@<sha>" or "hand-built" (standing rule 4).
- The `testkit` runner (feature of `lockmerge-core`, used by each format's
  `tests/rules.rs`) has a lint test: for every `RuleDef` there is at least one
  `accept-*` and one `refuse-*` directory. For every `CodeDef` there is at
  least one refusal case with that code. The accept cases must name the rule in
  their `applied` list, so a case cannot pass by accident through another rule.
  `case.toml` also carries `source` (where the bytes came from) and `path`
  (the repository path of the file). It may list `downgrades`: a non-fatal
  code never ends a merge, so such a code is covered by a case that lists it
  there. It may also list `also_refused`: the codes of the fatal refusals
  after the primary one, in order; the runner checks it exactly, like
  `downgrades`.
- **Two manifest layouts, so they cannot be confused.** A rule fixture
  (hand-built, `tests/fixtures/rules/`) holds already-merged manifests:
  `manifests/merged/<repo path>` is the merged manifest the fake returns;
  `manifests/merged/<repo path>.conflict`, an empty file, means the manifest
  merge conflicted; no file means unavailable. A benchmark case
  (`tests/fixtures/<ecosystem>/`) keeps the per-side layout of ADR 0007,
  `manifests/{base,ours,theirs}/<repo path>`, which the bench replays through
  real git. The testkit rejects a per-side directory in a rule fixture, and
  anything else beside `merged/`, with an error.
- Unit tests of a validator or hook may sit next to the code. The pair
  fixtures are the contract.
- The pure index functions of `lockmerge-cargo` (path, entry parsing, matching,
  `links`) are tested on index entry fixtures, with no `CARGO_HOME`.

## 3. Refusal and reporting model

**Three result kinds, never mixed.**

- `Outcome::Clean` (exit 0): merged bytes or one input returned unchanged.
- `Outcome::Refused` (exit 1): a `Refusal` with a `ReasonCode`. The CLI writes
  conflict markers into `%A` (ADR 0003).
- `Error` (exit 255): git or I/O failure that the format cannot treat as
  "unavailable", or an internal invariant failure. A panic is caught at the top
  of `main` with `catch_unwind` and mapped to 255. A Rust panic would otherwise
  exit 101, so this needs the explicit catch.

**The release profile keeps `panic = "unwind"`.** `panic = "abort"` is never
set in any profile that builds the `lockmerge` binary, because `catch_unwind`
does nothing under abort and the exit code would no longer be 255. A test in
`lockmerge-cli` runs the driver with a fake format that panics and asserts
that the exit code is 255 and that `%A` is unchanged, byte for byte.

```rust
pub struct Refusal { pub code: ReasonCode,
                     pub subjects: Vec<String>,                  // "serde 1.0.200 (registry)"
                     pub facts: Vec<(&'static str, String)> }    // ("ours", "1.0.201"), ("theirs","1.0.203")
pub struct Applied { pub rule: &'static str, pub subjects: Vec<String> }
pub struct Report  { pub format: FormatId,                 // Outcome::Clean(Clean) | Outcome::Refused(Refusal)
                     pub outcome: Outcome,
                     pub applied: Vec<Applied>,        // one per accepted unit, by rule
                     pub downgrades: Vec<Refusal>,     // non-fatal refusals that did not end the merge (R14)
                     pub also_refused: Vec<Refusal> }  // fatal refusals after the primary one
```

**What each field is for.**

- The primary refusal is the first fatal one in a deterministic order: parse,
  then envelope, then keys in sorted order, then validators. The CLI prints the
  primary one and a count of the others.
- `downgrades` records refusals with a non-fatal code (R14 style) while the
  merge went on. They are part of the report, and the benchmark counts them,
  so it can show why a rule did not fire. They are not whole-file refusals and
  do not change the exit code. R15 is not a downgrade: it is a validator
  failure that refuses the whole file (ADR 0008 decision 8 and 15).
- `also_refused` holds the fatal refusals after the primary one, in the same
  fixed order, so the CLI can print their count.
- Every `Applied` entry names a registered rule. The benchmark sums `applied`
  by `rule`, `downgrades` and `Refused` by `code`. `Format::rules()` and
  `codes()` zero-fill the rows, so the refusal breakdown shows codes that never
  occurred.
- Reason codes are stable keys. A shipped code is never renamed; a new code is
  added instead.

**Plain-language output.**

- Text comes only from the `CodeDef` table of the format: `title`, `explain`
  (filled from `subjects` and `facts`) and `next_step`. Free-form strings in
  rule code are not printed.
- The CLI prints to stderr, where git shows driver output:

```
lockmerge: Cargo.lock was not merged automatically (cargo.R3).
Both branches upgraded `serde` to different versions: ours 1.0.201, theirs 1.0.203.
lockmerge could not prove that one of them is safe for everything that depends on it.
Git's conflict markers are in Cargo.lock. To resolve: take one side, then run
`cargo update -p serde`, and commit the result.
```

- Rules for wording: say what could not be proven, never "your change is
  wrong". `next_step` never recommends an unproven shortcut. Wording is
  American English.
- A JSON report exists as a library type (`Report` is serializable by
  hand-written code, no extra crate) for the bench. A JSON option on the driver
  line is not needed in v1, because the driver command line is fixed by
  ADR 0003.
- Exit-code and marker behavior are tested in the CLI crate with a fake
  `Format`, and end to end in the bench (ADR 0006).

## 4. Parsers

All parsers are used read-only. Output is always written by lockmerge or
returned byte for byte (ADR 0008 decision 11, generalized by ADR 0014). Checked
2026-11-20 on crates.io.

| Crate | Version, last release | License | Notes |
|---|---|---|---|
| [`toml_edit`](https://crates.io/crates/toml_edit) | 0.25.19, 2026-11-06 | MIT OR Apache-2.0 | In the `toml-rs/toml` repository; about 300M recent downloads. [docs.rs](https://docs.rs/toml_edit/latest/toml_edit/) says it preserves comments, spaces and order, and lists two exceptions: the order of dotted keys and a missing trailing newline. That is not byte-exact, so we do not write with it. Whether it keeps spans for read-only documents is unverified (I recall `ImDocument` but did not check). |
| [`serde_json`](https://crates.io/crates/serde_json) | 1.0.154, 2026-10-11 | MIT OR Apache-2.0 | `preserve_order` keeps key order only; whitespace and number formatting are not kept. No JSON lockfile is in v1 scope (ADR 0002). Defer. |
| [`serde_yaml`](https://crates.io/crates/serde_yaml) | 0.9.34+deprecated, 2024-03-25 | MIT OR Apache-2.0 | Deprecated. Rejected. |
| [`yaml-rust2`](https://crates.io/crates/yaml-rust2) | 0.13.1, 2026-11-20 | MIT OR Apache-2.0 | Maintained; a plain value tree; loses comments and layout. |
| [`saphyr`](https://crates.io/crates/saphyr) / `saphyr-parser` | 0.3.4, 2026-11-11 | MIT OR Apache-2.0 | docs.rs lists `Marker` and `Span` types and calls it a YAML 1.2 parser, so source ranges are probably available. Not confirmed that they are byte offsets on scalars. |

**Decisions.**

- TOML: `toml_edit`, read-only, for `Cargo.lock`, `uv.lock` and manifests. It
  gives explicit control over the forms we read and the forms we refuse as
  unread (R14, R10): we walk its item tree and refuse any form we do not
  handle, instead of deserializing into structs. Using `toml_edit` read-only
  also keeps one TOML crate in the tree. A clippy `disallowed-methods` entry
  forbids `ToString` on its documents and `DocumentMut::to_string`, as a guard.
  We also need the `semver` crate for requirement matching (MIT OR Apache-2.0,
  the crate Cargo itself uses; unverified for the exact version and tree size,
  check at add time).
- YAML: not decided here; the choice is deferred to the pnpm format research.
  No Rust YAML library round-trips byte for byte, as far as is known
  (unverified). The constraint for that research: parse with a span-carrying
  parser (`saphyr-parser` is the first candidate), merge in a keyed model, and
  write the output by splicing byte ranges of the input text, or by a writer we
  own. Never write with the library. `serde_yaml` is excluded.
- JSON: not needed in v1; same constraint when it is.
- Per-format serialization strategy (canonical writer like Cargo, or splice)
  belongs to each format's document. The trait allows both, because
  `serialize` receives the `Sealed` units and the format's own model.

**Refusals specific to parsing, to carry into each format's codes.**

- The input is not valid in the chosen parser, or has duplicate keys: refuse
  (Cargo R1).
- A form the reader does not handle, an unknown key, or an unsupported version:
  refuse (Cargo R1 and R10). Never skip it silently.
- Non-UTF-8 bytes, or CRLF where the format's writer would have to guess line
  endings: refuse unless the format document says exactly how to preserve them.
  This is a proposal that the format research must confirm.

## 5. Keeping "never guess" enforceable: summary

| Question | Answer |
|---|---|
| Where are decisions made? | Only in `Pipeline::decide` and `validate` of a format crate, plus the generic base rules in `core`. |
| How can the CLI see a merge that did not pass a rule? | It cannot: `Clean` needs a `Sealed` value from the engine. |
| How does a new rule get tested? | A `RuleDef` entry; the lint demands an accept and a refuse fixture directory. |
| How does a new reason code get tested? | A `CodeDef` entry; the lint demands a refusal fixture with that code. |
| How does the benchmark attribute results? | `applied` by rule, `downgrades` and `Refused` by code, zero-filled from the registries. |
| What stays a human responsibility? | The rules themselves, which review and the gate (ADR 0006) check. |

## Good / Better / Best, with a PR-sized split

**Good (v1, build this).** The crates as above, with `lockmerge-cargo` only.

1. PR 1: workspace, `core` types (`ReasonCode`, `RuleDef`, `Proof`, `Report`),
   `Format` and `Pipeline`, the engine with the core base rules, and unit tests
   with a toy format.
2. PR 2: `testkit` (fixture runner and the rule-pair lint).
3. PR 3: `lockmerge-env` with the `Git` implementation (ADR 0010) and
   `FakeManifests`.
4. PR 4: `lockmerge-cli`: driver, exit codes, stderr rendering, marker writing,
   `catch_unwind` and the panic test.
5. PR 5: `lockmerge-bench` replay harness reading `Report`.
6. Then per-format PRs: `lockmerge-cargo` base rules, then M1 to M3, C, D, F,
   B1, then the pure registry index functions, `CargoHome` and R15.

**Better (follow-up).** A shared span-splicing helper in `core` for byte-exact
output, once two formats need it. A `Pipeline` helper for the common "keyed
packages" shape.

**Best (follow-up).** Opt-in cargo validation mode (ADR 0009). A
machine-readable report on the CLI for CI. Fuzzing the parse and engine paths
against the zero-wrong-merge gate.

## Not verified

- `toml_edit` span support for read-only documents.
- Whether `saphyr` spans are byte offsets.
- That no Rust YAML library round-trips byte for byte.
- JSON round-trip behavior beyond `preserve_order` semantics.
- The `semver` crate and any parser source were not read.
- The exact entry layouts of the sparse and git index caches; the Cargo
  registry format document must pin them before `lockmerge-cargo` parses them.
