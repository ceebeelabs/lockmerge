//! Fixture runner, fakes and the rule-pair lint for format tests
//! (`docs/03-architecture.md` section 2, ADR 0007 layout).
//!
//! This module is test support behind the `testkit` feature, which the release
//! binary does not enable. It is the one place in this crate that reads files,
//! so it lifts the purity lints of `clippy.toml` for itself.
//!
//! A case directory holds `base`, `ours`, `theirs`, optionally `expected`,
//! `manifests/` and `case.toml`:
//!
//! ```toml
//! source = "hand-built"            # where the bytes came from (standing rule 4)
//! path = "toy.lock"                # repository path of the file
//! outcome = "merged"               # or "refused"
//! applied = ["toy.E1", "core.B3"]  # merged: every applied rule, in order
//! code = "toy.R3"                  # refused: the primary refusal's code
//! downgrades = ["toy.R14"]         # optional: non-fatal refusals, in order
//! ```
//!
//! `manifests/<repo path>` holds the merged manifest the fake returns for that
//! path; `manifests/<repo path>.conflict` (any content) makes it a
//! [`ManifestResult::Conflict`]; any other path is unavailable. The `index/`
//! directory of a case is reserved for the `FakeIndex` of `lockmerge-cargo`.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use crate::{Context, Format, Inputs, ManifestResult, Manifests, Outcome, Why};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A [`Manifests`] built from a map or from a case's `manifests/` directory.
#[derive(Clone, Debug, Default)]
pub struct FakeManifests {
    by_path: BTreeMap<String, ManifestResult>,
}

impl FakeManifests {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the answer for `repo_path`.
    pub fn with(mut self, repo_path: &str, result: ManifestResult) -> Self {
        self.by_path.insert(repo_path.to_string(), result);
        self
    }

    /// Reads a `manifests/` directory. A missing directory is an empty fake.
    pub fn from_dir(dir: &Path) -> Result<Self, String> {
        let mut fake = Self::new();
        if dir.is_dir() {
            fake.read_dir(dir, "")?;
        }
        Ok(fake)
    }

    fn read_dir(&mut self, dir: &Path, prefix: &str) -> Result<(), String> {
        for entry in sorted_entries(dir)? {
            let name = file_name(&entry)?;
            let rel = format!("{prefix}{name}");
            if entry.is_dir() {
                self.read_dir(&entry, &format!("{rel}/"))?;
            } else if let Some(path) = rel.strip_suffix(".conflict") {
                self.by_path
                    .insert(path.to_string(), ManifestResult::Conflict);
            } else {
                self.by_path
                    .insert(rel, ManifestResult::Merged(read(&entry)?));
            }
        }
        Ok(())
    }
}

impl Manifests for FakeManifests {
    fn merged(&self, repo_path: &str) -> ManifestResult {
        self.by_path
            .get(repo_path)
            .cloned()
            .unwrap_or_else(|| ManifestResult::Unavailable(Why("not in the fixture".into())))
    }
}

/// What a case expects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expect {
    /// A clean result: `expected` bytes, and every applied rule in order.
    Merged { applied: Vec<String> },
    /// A refusal whose primary code is `code` (for example `toy.R3`).
    Refused { code: String },
}

/// One loaded fixture case.
#[derive(Debug)]
pub struct Case {
    pub dir: PathBuf,
    pub source: String,
    pub path: String,
    pub expect: Expect,
    /// Codes of the non-fatal refusals the report must carry, in order.
    pub downgrades: Vec<String>,
    pub base: Vec<u8>,
    pub ours: Vec<u8>,
    pub theirs: Vec<u8>,
    /// Present exactly for merged cases.
    pub expected: Option<Vec<u8>>,
    pub manifests: FakeManifests,
}

impl Case {
    /// Loads a case directory. Anything unexpected is an error.
    pub fn load(dir: &Path) -> Result<Case, String> {
        let at = |e: String| format!("{}: {e}", dir.display());
        let toml = read_text(&dir.join("case.toml")).map_err(at)?;
        let mut t = parse_toml(&toml).map_err(|e| at(format!("case.toml: {e}")))?;
        let source = t.take_string("source").map_err(at)?;
        let path = t.take_string("path").map_err(at)?;
        let outcome = t.take_string("outcome").map_err(at)?;
        let downgrades = t.take_list_opt("downgrades").map_err(at)?;
        let (expect, expected) = match outcome.as_str() {
            "merged" => {
                let applied = t.take_list("applied").map_err(at)?;
                let expected = read(&dir.join("expected")).map_err(at)?;
                (Expect::Merged { applied }, Some(expected))
            }
            "refused" => {
                let code = t.take_string("code").map_err(at)?;
                if dir.join("expected").exists() {
                    return Err(at("a refused case has no `expected` file".into()));
                }
                (Expect::Refused { code }, None)
            }
            other => {
                return Err(at(format!(
                    "outcome must be merged or refused, not {other:?}"
                )));
            }
        };
        t.finish().map_err(at)?;
        Ok(Case {
            dir: dir.to_path_buf(),
            source,
            path,
            expect,
            downgrades,
            base: read(&dir.join("base")).map_err(at)?,
            ours: read(&dir.join("ours")).map_err(at)?,
            theirs: read(&dir.join("theirs")).map_err(at)?,
            expected,
            manifests: FakeManifests::from_dir(&dir.join("manifests")).map_err(at)?,
        })
    }
}

/// Runs `case` through `format` and checks the outcome exactly: bytes,
/// applied rules and downgrades for a merge, the primary code for a refusal.
pub fn run_case(format: &dyn Format, case: &Case) -> Result<(), String> {
    let cx = Context::new(&case.manifests);
    let inputs = Inputs {
        base: &case.base,
        ours: &case.ours,
        theirs: &case.theirs,
        path: &case.path,
    };
    let report = format
        .merge(&inputs, &cx)
        .map_err(|e| format!("{}: internal error: {e}", case.dir.display()))?;
    let fail = |msg: String| Err(format!("{}: {msg}", case.dir.display()));
    let downgrades: Vec<String> = report
        .downgrades
        .iter()
        .map(|r| r.code.to_string())
        .collect();
    if downgrades != case.downgrades {
        return fail(format!(
            "downgrades are {downgrades:?}, the case says {:?}",
            case.downgrades
        ));
    }
    match (&case.expect, &report.outcome) {
        (Expect::Merged { applied }, Outcome::Clean(clean)) => {
            let got: Vec<&str> = report.applied.iter().map(|a| a.rule).collect();
            if got != *applied {
                return fail(format!(
                    "applied rules are {got:?}, the case says {applied:?}"
                ));
            }
            if Some(clean.bytes()) != case.expected.as_deref() {
                return fail("merged bytes differ from `expected`".into());
            }
            Ok(())
        }
        (Expect::Merged { .. }, Outcome::Refused(r)) => {
            fail(format!("expected a merge, got refusal {}", r.code))
        }
        (Expect::Refused { code }, Outcome::Refused(r)) => {
            if r.code.to_string() == *code {
                Ok(())
            } else {
                fail(format!("refused with {}, the case says {code}", r.code))
            }
        }
        (Expect::Refused { code }, Outcome::Clean(_)) => {
            fail(format!("expected refusal {code}, got a clean result"))
        }
    }
}

/// Loads and runs the case in `dir`.
pub fn run_dir(format: &dyn Format, dir: &Path) -> Result<(), String> {
    run_case(format, &Case::load(dir)?)
}

/// The rule-pair lint over `<root>/<format id>/<rule>/{accept,refuse}-*`.
/// Returns every problem found; an empty list means the lint passes.
///
/// - every `RuleDef` has an `accept-*` and a `refuse-*` directory;
/// - every `accept-*` case is a merge whose `applied` names its rule, and every
///   `refuse-*` case is a refusal (so a case cannot pass through another rule);
/// - every `CodeDef` has a refusal case with that code, or, for a non-fatal
///   code (which never ends a merge), a case that lists it in `downgrades`;
/// - every case runs and matches (see [`run_case`]);
/// - no directory is left over that names no rule, or is neither accept nor
///   refuse (a typo would otherwise hide a fixture from the lint).
pub fn lint(format: &dyn Format, root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let id = format.id().to_string();
    let base = root.join(&id);
    let mut covered: Vec<String> = Vec::new();
    let mut seen_dirs: Vec<String> = Vec::new();
    for rule in format.rules() {
        let Some(short) = rule.id.strip_prefix(&format!("{id}.")) else {
            problems.push(format!("rule {} does not start with `{id}.`", rule.id));
            continue;
        };
        seen_dirs.push(short.to_string());
        let rule_dir = base.join(short);
        let (mut accepts, mut refuses) = (0, 0);
        for entry in sorted_entries(&rule_dir).unwrap_or_default() {
            let Ok(name) = file_name(&entry) else {
                continue;
            };
            let accept = name.starts_with("accept-");
            if !(accept || name.starts_with("refuse-")) || !entry.is_dir() {
                problems.push(format!(
                    "{}: not an accept-* or refuse-* directory",
                    entry.display()
                ));
                continue;
            }
            let case = match Case::load(&entry) {
                Ok(c) => c,
                Err(e) => {
                    problems.push(e);
                    continue;
                }
            };
            if let Err(e) = run_case(format, &case) {
                problems.push(e);
            }
            match (&case.expect, accept) {
                (Expect::Merged { applied }, true) => {
                    accepts += 1;
                    if !applied.iter().any(|r| r == rule.id) {
                        problems.push(format!(
                            "{}: `applied` does not name {}",
                            entry.display(),
                            rule.id
                        ));
                    }
                }
                (Expect::Refused { .. }, false) => refuses += 1,
                _ => problems.push(format!(
                    "{}: an accept case must merge and a refuse case must refuse",
                    entry.display()
                )),
            }
            if let Expect::Refused { code } = &case.expect {
                covered.push(code.clone());
            }
            covered.extend(case.downgrades.iter().cloned());
        }
        if accepts == 0 {
            problems.push(format!(
                "rule {}: no accept-* case in {}",
                rule.id,
                rule_dir.display()
            ));
        }
        if refuses == 0 {
            problems.push(format!(
                "rule {}: no refuse-* case in {}",
                rule.id,
                rule_dir.display()
            ));
        }
    }
    for code in format.codes() {
        let name = code.code.to_string();
        let ok = covered.contains(&name);
        // The engine records only non-fatal codes as downgrades, so a fatal
        // code is covered by a refusal case alone.
        if !ok {
            problems.push(format!("code {name}: no refusal case"));
        }
    }
    for entry in sorted_entries(&base).unwrap_or_default() {
        if let Ok(name) = file_name(&entry)
            && !seen_dirs.contains(&name)
        {
            problems.push(format!(
                "{}: no rule of format {id} has this name",
                entry.display()
            ));
        }
    }
    problems
}

// ---- files -----------------------------------------------------------

fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_text(path: &Path) -> Result<String, String> {
    String::from_utf8(read(path)?).map_err(|_| format!("{}: not UTF-8", path.display()))
}

fn file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or_else(|| format!("{}: not a UTF-8 name", path.display()))
}

fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        out.push(e.map_err(|e| format!("{}: {e}", dir.display()))?.path());
    }
    out.sort();
    Ok(out)
}

// ---- case.toml: the small subset we write ------------------------------

enum Value {
    Str(String),
    List(Vec<String>),
}

struct Table(BTreeMap<String, Value>);

impl Table {
    fn take(&mut self, key: &str) -> Option<Value> {
        self.0.remove(key)
    }

    fn take_string(&mut self, key: &str) -> Result<String, String> {
        match self.take(key) {
            Some(Value::Str(s)) if !s.is_empty() => Ok(s),
            Some(_) => Err(format!("`{key}` must be a non-empty string")),
            None => Err(format!("missing `{key}`")),
        }
    }

    fn take_list(&mut self, key: &str) -> Result<Vec<String>, String> {
        match self.take(key) {
            Some(Value::List(l)) => Ok(l),
            Some(_) => Err(format!("`{key}` must be a list of strings")),
            None => Err(format!("missing `{key}`")),
        }
    }

    fn take_list_opt(&mut self, key: &str) -> Result<Vec<String>, String> {
        if self.0.contains_key(key) {
            self.take_list(key)
        } else {
            Ok(Vec::new())
        }
    }

    fn finish(self) -> Result<(), String> {
        match self.0.keys().next() {
            Some(k) => Err(format!("unexpected key `{k}`")),
            None => Ok(()),
        }
    }
}

/// Reads `key = "string"` and `key = ["a", "b"]` lines, comments and blank
/// lines. Anything else (tables, escapes, multi-line arrays, duplicates) is an
/// error, never a guess.
fn parse_toml(text: &str) -> Result<Table, String> {
    let mut map = BTreeMap::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let err = |m: &str| format!("line {}: {m}", n + 1);
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| err("expected key = value"))?;
        let key = key.trim();
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(err("bad key"));
        }
        let value = strip_comment(value.trim());
        let parsed = if let Some(inner) = value.strip_prefix('[') {
            let inner = inner
                .strip_suffix(']')
                .ok_or_else(|| err("array must end on its line"))?;
            let mut items = Vec::new();
            for part in inner.split(',') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                items.push(quoted(part).ok_or_else(|| err("bad array item"))?);
            }
            Value::List(items)
        } else {
            Value::Str(quoted(value).ok_or_else(|| err("value must be a quoted string or array"))?)
        };
        if map.insert(key.to_string(), parsed).is_some() {
            return Err(err("duplicate key"));
        }
    }
    Ok(Table(map))
}

fn quoted(s: &str) -> Option<String> {
    let inner = s.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.contains(['"', '\\'])).then(|| inner.to_string())
}

/// Drops a trailing `# comment` that sits outside quotes.
fn strip_comment(s: &str) -> &str {
    let mut in_str = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return s[..i].trim_end(),
            _ => {}
        }
    }
    s
}
