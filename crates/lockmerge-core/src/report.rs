use crate::{FormatId, ReasonCode};
use std::fmt;

/// A refusal to merge, with the facts that explain it. Its text for the user
/// comes from the format's [`CodeDef`](crate::CodeDef) table, never from here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub code: ReasonCode,
    /// For example `serde 1.0.200 (registry)`.
    pub subjects: Vec<String>,
    /// For example `("ours", "1.0.201")`.
    pub facts: Vec<(&'static str, String)>,
}

impl Refusal {
    pub fn new(
        code: ReasonCode,
        subjects: Vec<String>,
        facts: Vec<(&'static str, String)>,
    ) -> Self {
        Self {
            code,
            subjects,
            facts,
        }
    }
}

/// A rule that accepted something, and what it accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    /// A registered rule id: a format rule or a core rule (`core.S1`,
    /// `core.B1` to `core.B4`).
    pub rule: &'static str,
    pub subjects: Vec<String>,
}

/// Which input was returned unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Ours,
    Theirs,
}

/// How a clean result was produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// One input, byte for byte (`core.S1`).
    Unchanged(Side),
    /// Built from the merged units and written by the format's serializer.
    Merged,
}

/// A merged result. Only the engine can build one.
///
/// ```compile_fail,E0451
/// let _ = lockmerge_core::Clean { bytes: Vec::new(), how: lockmerge_core::How::Merged };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clean {
    bytes: Vec<u8>,
    how: How,
}

impl Clean {
    pub(crate) fn new(bytes: Vec<u8>, how: How) -> Self {
        Self { bytes, how }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn how(&self) -> How {
        self.how
    }
}

/// Exactly one of two results, never mixed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Exit 0.
    Clean(Clean),
    /// Exit 1. The primary refusal: the first fatal one in the fixed order
    /// (parse, envelope, keys in sorted order, validators).
    Refused(Refusal),
}

/// The result of a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub format: FormatId,
    pub outcome: Outcome,
    /// One entry per accepted key, envelope check, removal and validator, in
    /// the order they ran.
    pub applied: Vec<Applied>,
    /// Non-fatal refusals: a rule declined but did not end the merge. They are
    /// reported and counted but never change the outcome.
    pub downgrades: Vec<Refusal>,
    /// Fatal refusals other than the primary one, in the same fixed order.
    pub also_refused: Vec<Refusal>,
}

/// A failure that is neither a refusal nor "unavailable": a broken format
/// registration or proof. The CLI exits 255 and leaves the file alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A `Proof` names a rule that is not in `Pipeline::rules()`.
    UnregisteredRule { format: FormatId, rule: String },
    /// A `Refusal` has a code that is neither in `Pipeline::codes()` nor a
    /// core code.
    UnregisteredCode { format: FormatId, code: String },
    /// `rules()` or `codes()` is malformed: an id outside the format's prefix
    /// (a format may not register a `core.*` rule) or a duplicate.
    BadRegistry { format: FormatId, detail: String },
    /// `prune` named a key that is not in the draft, or named it twice.
    BadRemoval { format: FormatId, key: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnregisteredRule { format, rule } => {
                write!(
                    f,
                    "internal error: format {format} used unregistered rule {rule}"
                )
            }
            Error::UnregisteredCode { format, code } => {
                write!(
                    f,
                    "internal error: format {format} used unregistered code {code}"
                )
            }
            Error::BadRegistry { format, detail } => {
                write!(
                    f,
                    "internal error: format {format} has a bad registry: {detail}"
                )
            }
            Error::BadRemoval { format, key } => {
                write!(
                    f,
                    "internal error: format {format} pruned unknown key {key}"
                )
            }
        }
    }
}

impl std::error::Error for Error {}
