use std::fmt;

/// Identifies a format, for example `FormatId("cargo")`. Rule ids and reason
/// codes of a format are prefixed with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FormatId(pub &'static str);

impl fmt::Display for FormatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// A stable refusal key such as `cargo.R3`. A shipped code is never renamed;
/// a new code is added instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReasonCode {
    pub format: FormatId,
    pub code: &'static str,
}

impl ReasonCode {
    pub const fn new(format: FormatId, code: &'static str) -> Self {
        Self { format, code }
    }
}

impl fmt::Display for ReasonCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.format, self.code)
    }
}

/// A merge rule a format may apply, for example `cargo.M1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleDef {
    /// `<format>.<rule>`, for example `cargo.M1`.
    pub id: &'static str,
    /// One sentence, for reports.
    pub summary: &'static str,
    /// The ADR that decides the rule, for example `0008#4`.
    pub adr: &'static str,
}

/// A refusal code and its user-facing text. Text for a refusal comes only
/// from this table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeDef {
    pub code: ReasonCode,
    /// Plain language, no jargon.
    pub title: &'static str,
    /// A template with `{placeholders}` filled from subjects and facts.
    pub explain: &'static str,
    /// What the user can do. Never an unproven shortcut.
    pub next_step: &'static str,
    /// Whether a refusal with this code ends the merge of the whole file.
    ///
    /// A fatal refusal (for example ADR 0008's R15, the native links proof) is
    /// always a whole-file refusal and is never reported as a downgrade. A
    /// non-fatal one (for example R14, manifests unavailable) is recorded in
    /// `Report::downgrades` by the engine and does not end the merge by itself.
    pub fatal: bool,
}
