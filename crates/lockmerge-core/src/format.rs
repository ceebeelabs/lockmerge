use crate::{CodeDef, Draft, Error, FormatId, Proof, Refusal, Report, RuleDef, Sealed, engine};
use std::collections::BTreeMap;
use std::fmt::Display;

/// Which of the three inputs is meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Base,
    Ours,
    Theirs,
}

/// The three versions of one file, as bytes.
#[derive(Clone, Copy, Debug)]
pub struct Inputs<'a> {
    pub base: &'a [u8],
    pub ours: &'a [u8],
    pub theirs: &'a [u8],
    /// The repository path of the file, for messages and manifest lookup.
    pub path: &'a str,
}

/// Why a merged manifest is not available. Any reason leads to the format's
/// safe direction (ADR 0008 R14), never to an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Why(pub String);

/// The result of asking for the merged manifest of a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestResult {
    Merged(Vec<u8>),
    Unavailable(Why),
    Conflict,
}

/// Merged manifests, from git objects. Implemented by `lockmerge-env`, and by
/// fakes in tests.
pub trait Manifests {
    fn merged(&self, repo_path: &str) -> ManifestResult;

    /// The manifest at `repo_path` as one input has it, unmerged. A format
    /// uses it to ask whether the merged manifest still says what the side's
    /// lockfile was resolved with. The default provides none, which is the
    /// safe direction: the format then refuses what needs it.
    fn side(&self, _role: Role, _repo_path: &str) -> Result<Vec<u8>, Why> {
        Err(Why("side manifests are not provided".to_string()))
    }
}

/// What the engine hands to a format besides the inputs.
#[derive(Clone, Copy)]
pub struct Context<'a> {
    pub manifests: &'a dyn Manifests,
}

impl<'a> Context<'a> {
    pub fn new(manifests: &'a dyn Manifests) -> Self {
        Self { manifests }
    }

    /// Makes a [`Proof`] for `rule` with the facts it relied on. This is the
    /// only way a format obtains a `Proof`. The engine checks `rule` against
    /// `Pipeline::rules()` when it sees the proof.
    pub fn proof(&self, rule: &'static str, facts: Vec<(&'static str, String)>) -> Proof {
        Proof::new(rule, facts)
    }
}

/// A key whose three versions the base rules did not settle.
#[derive(Debug)]
pub struct Conflict<'a, K, U> {
    pub key: &'a K,
    pub base: Option<&'a U>,
    pub ours: Option<&'a U>,
    pub theirs: Option<&'a U>,
}

/// What a format says about a [`Conflict`].
#[derive(Debug)]
pub enum Verdict<U> {
    /// Use this unit, for the rule the proof names.
    Accept(U, Proof),
    /// Refuse with a format-specific code (for example `cargo.R3`). A fatal
    /// code ends the merge. A non-fatal code is recorded as a downgrade and
    /// the key is then refused as unproven, because nothing merged it.
    Refuse(Refusal),
    /// No rule of this format applies. The default; the engine refuses the key
    /// with `core.UNPROVEN`.
    NotApplicable,
}

/// A unit a format removes after the per-key pass (ADR 0008 M3 pruning).
/// Removal needs a proof like an acceptance does.
#[derive(Debug)]
pub struct Removal<K> {
    pub key: K,
    pub proof: Proof,
}

/// What a whole-set check found.
#[derive(Debug, Default)]
pub struct Findings {
    /// One per check that passed.
    pub proofs: Vec<Proof>,
    /// Failed checks. Whether one ends the merge is decided by its code's
    /// [`CodeDef::fatal`].
    pub refusals: Vec<Refusal>,
}

/// Object-safe; what the CLI and the benchmark see.
pub trait Format: Sync {
    fn id(&self) -> FormatId;
    fn matches(&self, file_name: &str) -> bool;
    /// For lint and zero-filled tables.
    fn rules(&self) -> &'static [RuleDef];
    fn codes(&self) -> &'static [CodeDef];
    /// Merges three inputs. A refusal is an `Ok` report; `Err` is an internal
    /// error (exit 255).
    fn merge(&self, inputs: &Inputs<'_>, cx: &Context<'_>) -> Result<Report, Error>;
}

/// The typed contract a format implements. A blanket impl provides [`Format`]
/// through [`engine::run`], so a format has no way to return a clean result
/// except through the engine.
pub trait Pipeline {
    /// The parsed file.
    type Model;
    /// Identifies a unit across the three inputs.
    type Key: Ord + Clone + Display;
    /// The merge unit.
    type Unit: Clone + Eq;

    fn id(&self) -> FormatId;
    /// Every rule a [`Proof`] of this format may name. Ids start with
    /// `<format>.`.
    fn rules(&self) -> &'static [RuleDef];
    /// Every code a [`Refusal`] of this format may carry. Codes belong to this
    /// format; `core.*` codes need no registration.
    fn codes(&self) -> &'static [CodeDef];
    fn matches(&self, file_name: &str) -> bool;

    /// Parses one input. Refuse what is not read exactly.
    fn parse(&self, role: Role, bytes: &[u8]) -> Result<Self::Model, Refusal>;

    /// Checks what must agree across the three inputs (version, unknown
    /// keys). The proofs are recorded as applied.
    fn envelope(&self, models: [&Self::Model; 3], cx: &Context<'_>) -> Result<Vec<Proof>, Refusal>;

    fn units(&self, model: &Self::Model) -> BTreeMap<Self::Key, Self::Unit>;

    /// Called only for keys the engine's base rules (`core.B1` to `core.B4`)
    /// do not settle. The default of every implementation must be
    /// [`Verdict::NotApplicable`].
    fn decide(
        &self,
        conflict: &Conflict<'_, Self::Key, Self::Unit>,
        cx: &Context<'_>,
    ) -> Verdict<Self::Unit>;

    /// Removals after the per-key pass and before `validate`, each with a
    /// proof. The engine checks the rule and that the key is in the draft.
    fn prune(
        &self,
        _draft: &Draft<Self::Key, Self::Unit>,
        _cx: &Context<'_>,
    ) -> Vec<Removal<Self::Key>> {
        Vec::new()
    }

    /// Whole-set checks (ADR 0008 decision 8) on the pruned draft.
    fn validate(&self, draft: &Draft<Self::Key, Self::Unit>, cx: &Context<'_>) -> Findings;

    /// The canonical writer; never a TOML or YAML library's writer. It takes
    /// only a [`Sealed`] value, so it cannot run before the engine's checks
    /// pass. `models` are the parsed base, ours and theirs, for what is not a
    /// unit (headers, for example).
    fn serialize(
        &self,
        sealed: &Sealed<Self::Key, Self::Unit>,
        models: [&Self::Model; 3],
    ) -> Vec<u8>;
}

impl<P: Pipeline + Sync> Format for P {
    fn id(&self) -> FormatId {
        Pipeline::id(self)
    }
    fn matches(&self, file_name: &str) -> bool {
        Pipeline::matches(self, file_name)
    }
    fn rules(&self) -> &'static [RuleDef] {
        Pipeline::rules(self)
    }
    fn codes(&self) -> &'static [CodeDef] {
        Pipeline::codes(self)
    }
    fn merge(&self, inputs: &Inputs<'_>, cx: &Context<'_>) -> Result<Report, Error> {
        engine::run(self, inputs, cx)
    }
}
