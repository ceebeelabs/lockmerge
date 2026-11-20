//! Read-only parsing of one Cargo.lock (ADR 0014).
//!
//! `toml_edit` is used to read the item tree only. Every key and value form
//! this crate does not handle is refused, never skipped. After the model is
//! built, the canonical writer must reproduce the input byte for byte;
//! otherwise the input is refused, so a file is merged only if its text is
//! exactly what Cargo writes.

use crate::codes;
use crate::model::{EdgeError, Id, Lock, Package, resolve_edge};
use crate::write::{is_plain, render};
use lockmerge_core::{Refusal, Role};
use semver::Version;
use std::collections::{BTreeMap, BTreeSet};
use toml_edit::{Document, Item, Table};

fn refuse(code: lockmerge_core::ReasonCode, role: Role, detail: impl Into<String>) -> Refusal {
    Refusal::new(
        code,
        vec![role_name(role).to_string()],
        vec![("detail", detail.into())],
    )
}

pub(crate) fn role_name(role: Role) -> &'static str {
    match role {
        Role::Base => "base",
        Role::Ours => "ours",
        Role::Theirs => "theirs",
    }
}

/// A name, version, source or checksum: plain text without whitespace or
/// characters that need escaping, so the edge grammar stays unambiguous.
fn is_token(s: &str) -> bool {
    crate::write::is_plain(s) && !s.chars().any(|c| c.is_whitespace() || c == '(' || c == ')')
}

pub fn parse(role: Role, bytes: &[u8]) -> Result<Lock, Refusal> {
    let text = std::str::from_utf8(bytes).map_err(|_| refuse(codes::R1, role, "not UTF-8"))?;
    let doc = Document::parse(text).map_err(|e| refuse(codes::R1, role, e.message()))?;
    let root = doc.as_table();

    let version = match root.get("version") {
        None => return Err(refuse(codes::R1, role, "no `version` key (version 1 or 2)")),
        Some(item) => match item.as_integer() {
            Some(v @ (3 | 4)) => v as u8,
            Some(v) => {
                return Err(refuse(
                    codes::R1,
                    role,
                    format!("lockfile version {v} is not supported"),
                ));
            }
            None => return Err(refuse(codes::R1, role, "`version` is not an integer")),
        },
    };
    for (key, _) in root.iter() {
        if key != "version" && key != "package" {
            return Err(refuse(
                codes::R10,
                role,
                format!("top-level key `{key}` is not handled"),
            ));
        }
    }
    let tables = root
        .get("package")
        .and_then(Item::as_array_of_tables)
        .ok_or_else(|| refuse(codes::R10, role, "no `[[package]]` entries"))?;

    let mut raw: Vec<(Id, Package, Vec<String>)> = Vec::new();
    let mut seen = BTreeSet::new();
    for table in tables.iter() {
        let (id, checksum, edges) = read_package(role, table)?;
        if !seen.insert(id.clone()) {
            return Err(refuse(codes::R9, role, format!("{id} appears twice")));
        }
        raw.push((
            id,
            Package {
                checksum,
                edges: BTreeSet::new(),
            },
            edges,
        ));
    }
    if raw.is_empty() {
        return Err(refuse(codes::R10, role, "no `[[package]]` entries"));
    }

    // Resolve every edge against this file's ids, per Cargo's lookup rules.
    let ids: Vec<Id> = raw.iter().map(|(id, _, _)| id.clone()).collect();
    let mut packages = BTreeMap::new();
    for (id, mut pkg, edges) in raw {
        for text in edges {
            let target = resolve_edge(ids.iter(), &text).map_err(|e| {
                let why = match e {
                    EdgeError::Malformed => "is not in a form Cargo writes",
                    EdgeError::Unresolved => "names no package",
                    EdgeError::Ambiguous => "names several packages",
                };
                Refusal::new(
                    codes::R9,
                    vec![id.to_string()],
                    vec![
                        ("input", role_name(role).to_string()),
                        ("edge", text.clone()),
                        ("detail", format!("the edge {why}")),
                    ],
                )
            })?;
            if !pkg.edges.insert(target.clone()) {
                return Err(refuse(
                    codes::R9,
                    role,
                    format!("{id} lists the edge `{text}` twice"),
                ));
            }
        }
        packages.insert(id, pkg);
    }

    // The round-trip rule: unless the canonical writer reproduces the input
    // exactly, the text holds something this crate does not model.
    let items: Vec<_> = packages.iter().collect();
    if render(version, &items).as_deref() != Some(bytes) {
        return Err(refuse(
            codes::CANON,
            role,
            "the file is not in the exact form Cargo writes",
        ));
    }
    Ok(Lock { version, packages })
}

fn read_package(role: Role, table: &Table) -> Result<(Id, Option<String>, Vec<String>), Refusal> {
    let (mut name, mut version, mut source, mut checksum, mut edges) =
        (None, None, None, None, Vec::new());
    for (key, item) in table.iter() {
        let string = |what: &str| -> Result<String, Refusal> {
            let s = item
                .as_str()
                .ok_or_else(|| refuse(codes::R10, role, format!("`{what}` is not a string")))?;
            if is_token(s) {
                Ok(s.to_string())
            } else {
                Err(refuse(
                    codes::R10,
                    role,
                    format!("`{what}` holds characters this crate does not handle"),
                ))
            }
        };
        match key {
            "name" => name = Some(string("name")?),
            "version" => version = Some(string("version")?),
            "source" => source = Some(string("source")?),
            "checksum" => checksum = Some(string("checksum")?),
            "dependencies" => {
                let array = item.as_array().ok_or_else(|| {
                    refuse(
                        codes::R10,
                        role,
                        "`dependencies` is not an array of strings",
                    )
                })?;
                for value in array.iter() {
                    let s = value.as_str().filter(|s| is_plain(s)).ok_or_else(|| {
                        refuse(codes::R10, role, "a dependency is not a plain string")
                    })?;
                    edges.push(s.to_string());
                }
            }
            other => {
                return Err(refuse(
                    codes::R10,
                    role,
                    format!("package key `{other}` is not handled"),
                ));
            }
        }
    }
    let (Some(name), Some(version)) = (name, version) else {
        return Err(refuse(
            codes::R10,
            role,
            "a package has no `name` or no `version`",
        ));
    };
    let parsed = Version::parse(&version)
        .ok()
        .filter(|v| v.to_string() == version)
        .ok_or_else(|| {
            refuse(
                codes::R11,
                role,
                format!("`{name} {version}`: not a semver version"),
            )
        })?;
    let id = Id {
        name,
        version: parsed,
        source,
    };
    Ok((id, checksum, edges))
}
