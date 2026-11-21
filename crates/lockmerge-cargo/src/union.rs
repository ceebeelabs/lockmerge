//! The manifest text conflict that is provably two insertions (ADR 0018).
//!
//! `git merge-file --diff3` conflicts when both branches add a line at the
//! same place, for example one dependency each at the end of the same
//! `[dependencies]` table. [`resolve`] reads that output and returns the text
//! with both insertions, ours first, only when every conflict hunk is exactly
//! that and nothing else; every other shape is an `Err` with the reason, and
//! the caller treats the manifest as unread (R14), as before.

use std::collections::BTreeSet;
use toml_edit::{Document, Item, TableLike};

const OURS: &str = "<<<<<<< ours";
const BASE: &str = "||||||| base";
const SEP: &str = "=======";
const THEIRS: &str = ">>>>>>> theirs";

fn no<T>(why: impl Into<String>) -> Result<T, String> {
    Err(why.into())
}

fn bare(line: &str) -> &str {
    line.strip_suffix('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .unwrap_or(line)
}

enum Part<'a> {
    Common(&'a str),
    Hunk {
        ours: Vec<&'a str>,
        base: Vec<&'a str>,
        theirs: Vec<&'a str>,
    },
}

/// Splits `git merge-file -p --diff3 -L ours -L base -L theirs` output.
fn parts(text: &str) -> Result<Vec<Part<'_>>, String> {
    #[derive(PartialEq)]
    enum In {
        Common,
        Ours,
        Base,
        Theirs,
    }
    let mut out = Vec::new();
    let mut state = In::Common;
    let (mut o, mut b, mut t) = (Vec::new(), Vec::new(), Vec::new());
    for line in text.split_inclusive('\n') {
        let plain = bare(line);
        let marker = [OURS, BASE, SEP, THEIRS].contains(&plain);
        match (&state, plain) {
            (In::Common, OURS) => state = In::Ours,
            (In::Ours, BASE) => state = In::Base,
            (In::Base, SEP) => state = In::Theirs,
            (In::Theirs, THEIRS) => {
                out.push(Part::Hunk {
                    ours: std::mem::take(&mut o),
                    base: std::mem::take(&mut b),
                    theirs: std::mem::take(&mut t),
                });
                state = In::Common;
            }
            _ if marker || plain.starts_with("<<<<<<<") || plain.starts_with(">>>>>>>") => {
                return no("a conflict marker is where it is not expected");
            }
            (In::Common, _) => out.push(Part::Common(line)),
            (In::Ours, _) => o.push(line),
            (In::Base, _) => b.push(line),
            (In::Theirs, _) => t.push(line),
        }
    }
    if state != In::Common {
        return no("a conflict hunk is not closed");
    }
    Ok(out)
}

/// Each line is one whole `key = value` entry that parses completely on its
/// own. Returns the keys.
fn entries(lines: &[&str]) -> Result<Vec<String>, String> {
    let mut keys = Vec::new();
    for line in lines {
        let doc = Document::parse(*line)
            .map_err(|_| format!("`{}` is not a whole TOML entry", bare(line)))?;
        let table = doc.as_table();
        let mut it = table.iter();
        match (it.next(), it.next()) {
            // A dotted key (`a.b = 1`) parses as a table, never as a value.
            (Some((key, item)), None) if item.is_value() => {
                keys.push(key.to_string());
            }
            _ => {
                return no(format!(
                    "`{}` is not one plain key = value entry",
                    bare(line)
                ));
            }
        }
    }
    Ok(keys)
}

/// The dependency table a header line names: `[dependencies]`,
/// `[dev-dependencies]`, `[build-dependencies]` or their
/// `[target.<cfg>.<kind>]` forms, as the path of keys.
fn dependency_table(header: &str) -> Result<Vec<String>, String> {
    let line = bare(header).trim();
    if !line.starts_with('[') || line.starts_with("[[") {
        return no(format!("`{line}` is not a table header"));
    }
    let doc =
        Document::parse(header).map_err(|_| format!("`{line}` is not read as a table header"))?;
    let mut path = Vec::new();
    let mut table: &dyn TableLike = doc.as_table();
    loop {
        let mut it = table.iter();
        let Some((key, item)) = it.next() else { break };
        if it.next().is_some() {
            return no(format!("`{line}` is not one table"));
        }
        path.push(key.to_string());
        match item.as_table_like() {
            Some(t) if item.is_table() => table = t,
            _ => return no(format!("`{line}` is not a table header")),
        }
    }
    let kind = |k: &str| {
        matches!(
            k,
            "dependencies" | "dev-dependencies" | "build-dependencies"
        )
    };
    match path.as_slice() {
        [k] if kind(k) => Ok(path),
        [t, _, k] if t == "target" && kind(k) => Ok(path),
        _ => no(format!("`{line}` is not a dependency table")),
    }
}

fn table_at<'a>(doc: &'a Document<&str>, path: &[String]) -> Option<&'a dyn TableLike> {
    let mut table: &dyn TableLike = doc.as_table();
    for key in path {
        table = table.get(key)?.as_table_like()?;
    }
    Some(table)
}

/// The text with both sides' insertions kept, ours first.
pub fn resolve(conflicted: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(conflicted).map_err(|_| "the manifest is not UTF-8")?;
    let parts = parts(text)?;
    if !parts.iter().any(|p| matches!(p, Part::Hunk { .. })) {
        return no("the text merge reported a conflict but shows no hunk");
    }

    let mut merged = String::new();
    // Where each side's inserted keys went: the table path of each hunk.
    let mut inserted: Vec<(Vec<String>, Vec<String>)> = Vec::new();
    let mut last_header: Option<&str> = None;
    for part in &parts {
        match part {
            Part::Common(line) => {
                if bare(line).trim_start().starts_with('[') {
                    last_header = Some(line);
                }
                merged.push_str(line);
            }
            Part::Hunk { ours, base, theirs } => {
                if !base.is_empty() {
                    return no(
                        "a conflict hunk changes lines of the base; only insertions are read",
                    );
                }
                if ours.is_empty() || theirs.is_empty() {
                    return no("a conflict hunk is not an insertion by both sides");
                }
                let ours_keys = entries(ours)?;
                let theirs_keys = entries(theirs)?;
                if let Some(k) = ours_keys.iter().find(|k| theirs_keys.contains(k)) {
                    return no(format!("both branches add the key `{k}` at the same place"));
                }
                let Some(header) = last_header else {
                    return no("a conflict hunk is not inside a table");
                };
                let path = dependency_table(header)?;
                for line in ours.iter().chain(theirs) {
                    merged.push_str(line);
                    if !line.ends_with('\n') {
                        merged.push('\n');
                    }
                }
                inserted.push((path, [ours_keys, theirs_keys].concat()));
            }
        }
    }

    // The whole result must parse (this also refuses a repeated key), and
    // each inserted key must sit in the table its header names.
    let doc = Document::parse(merged.as_str()).map_err(|e| {
        format!(
            "the manifest with both insertions is not TOML: {}",
            e.message()
        )
    })?;
    let base_text: String = parts
        .iter()
        .filter_map(|p| match p {
            Part::Common(l) => Some(*l),
            Part::Hunk { .. } => None,
        })
        .collect();
    let base_doc = Document::parse(base_text.as_str())
        .map_err(|_| "the unchanged lines of the manifest are not TOML on their own")?;
    let mut seen: BTreeSet<(Vec<String>, String)> = BTreeSet::new();
    for (path, keys) in &inserted {
        let Some(table) = table_at(&doc, path) else {
            return no("an inserted entry is not in the table the header names");
        };
        let base_table = table_at(&base_doc, path);
        for key in keys {
            if !table.get(key).is_some_and(Item::is_value) {
                return no(format!("`{key}` is not in the table the header names"));
            }
            if base_table.is_some_and(|t| t.contains_key(key)) {
                return no(format!("the key `{key}` is already in the table"));
            }
            if !seen.insert((path.clone(), key.clone())) {
                return no(format!("the key `{key}` is inserted twice"));
            }
        }
    }
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::resolve;

    /// `git merge-file --diff3` output for a manifest: `head`, then one hunk,
    /// then `tail`.
    fn hunk(head: &str, ours: &str, base: &str, theirs: &str, tail: &str) -> String {
        format!(
            "{head}<<<<<<< ours\n{ours}||||||| base\n{base}=======\n{theirs}>>>>>>> theirs\n{tail}"
        )
    }

    const HEAD: &str = "[package]\nname = \"app\"\n\n[dependencies]\nserde = \"1\"\n";
    const TAIL: &str = "\n[features]\nx = []\n";

    fn swap(ours: &str, theirs: &str) -> (String, String) {
        (theirs.to_string(), ours.to_string())
    }

    fn ok(text: &str) -> String {
        resolve(text.as_bytes()).unwrap_or_else(|e| panic!("refused: {e}\n{text}"))
    }

    fn refused(text: &str) -> String {
        match resolve(text.as_bytes()) {
            Ok(s) => panic!("accepted:\n{s}\nfrom:\n{text}"),
            Err(e) => e,
        }
    }

    #[test]
    fn two_insertions_in_one_dependency_table_are_both_kept_ours_first() {
        let text = hunk(HEAD, "itoa = \"1\"\n", "", "leaf = \"1\"\n", TAIL);
        assert_eq!(
            ok(&text),
            format!("{HEAD}itoa = \"1\"\nleaf = \"1\"\n{TAIL}")
        );
        // Swapped sides: the same two entries, theirs first.
        let (o, t) = swap("itoa = \"1\"\n", "leaf = \"1\"\n");
        assert_eq!(
            ok(&hunk(HEAD, &o, "", &t, TAIL)),
            format!("{HEAD}leaf = \"1\"\nitoa = \"1\"\n{TAIL}")
        );
    }

    #[test]
    fn several_whole_entries_per_side_are_read() {
        let text = hunk(
            HEAD,
            "a = \"1\"\nb = { version = \"2\", features = [\"x\"] }\n",
            "",
            "c = \"3\"\n",
            TAIL,
        );
        assert!(ok(&text).contains("c = \"3\"\n"));
    }

    #[test]
    fn dev_build_and_target_tables_are_read() {
        for header in [
            "[dev-dependencies]",
            "[build-dependencies]",
            "[target.'cfg(unix)'.dependencies]",
            "[target.x86_64-pc-windows-msvc.dev-dependencies]",
            "[target.'cfg(unix)'.build-dependencies]",
        ] {
            let head = format!("[package]\nname = \"app\"\n\n{header}\nserde = \"1\"\n");
            ok(&hunk(&head, "itoa = \"1\"\n", "", "leaf = \"1\"\n", TAIL));
        }
    }

    #[test]
    fn other_tables_are_refused() {
        for header in [
            "[features]",
            "[workspace.dependencies]",
            "[package.metadata.x]",
            "[dependencies.serde]",
            "[target.'cfg(unix)'.dependencies.serde]",
            "[target.dependencies]",
            "[[bin]]",
            "[lints]",
            "[patch.crates-io]",
        ] {
            let head = format!("[package]\nname = \"app\"\n\n{header}\nserde = \"1\"\n");
            refused(&hunk(&head, "itoa = \"1\"\n", "", "leaf = \"1\"\n", TAIL));
        }
        // Before any table header.
        refused(&hunk("", "itoa = \"1\"\n", "", "leaf = \"1\"\n", "\n[a]\n"));
    }

    #[test]
    fn an_edit_is_refused_where_an_insertion_is_read() {
        // Both sides changed the base line.
        let edit = hunk(
            "[dependencies]\n",
            "serde = \"1.0\"\n",
            "serde = \"1\"\n",
            "serde = \"1.1\"\n",
            "",
        );
        assert!(refused(&edit).contains("base"));
        // One side removed a line the other changed.
        refused(&hunk(
            "[dependencies]\n",
            "",
            "serde = \"1\"\n",
            "serde = \"2\"\n",
            "",
        ));
        // Insertion on one side only cannot be a conflict, and is refused.
        refused(&hunk(HEAD, "itoa = \"1\"\n", "", "", TAIL));
    }

    #[test]
    fn a_key_both_sides_insert_is_refused() {
        refused(&hunk(HEAD, "itoa = \"1\"\n", "", "itoa = \"2\"\n", TAIL));
        refused(&hunk(
            HEAD,
            "itoa = \"1\"\na = \"1\"\n",
            "",
            "b = \"1\"\nitoa = \"1\"\n",
            TAIL,
        ));
    }

    #[test]
    fn a_key_already_in_the_table_is_refused() {
        refused(&hunk(HEAD, "serde = \"2\"\n", "", "leaf = \"1\"\n", TAIL));
        refused(&hunk(HEAD, "itoa = \"1\"\n", "", "serde = \"2\"\n", TAIL));
    }

    #[test]
    fn a_value_over_several_lines_is_refused() {
        refused(&hunk(
            HEAD,
            "itoa = { version = \"1\",\n  features = [\"x\"] }\n",
            "",
            "leaf = \"1\"\n",
            TAIL,
        ));
        refused(&hunk(HEAD, "itoa = [\n", "", "leaf = \"1\"\n", TAIL));
    }

    #[test]
    fn anything_but_a_plain_entry_is_refused() {
        for bad in [
            "# a comment\n",
            "\n",
            "itoa.version = \"1\"\n",
            "[other]\n",
            "itoa = \n",
            "itoa = \"1\" extra\n",
        ] {
            refused(&hunk(HEAD, bad, "", "leaf = \"1\"\n", TAIL));
            refused(&hunk(HEAD, "leaf = \"1\"\n", "", bad, TAIL));
        }
    }

    #[test]
    fn every_hunk_must_be_an_insertion() {
        let two = |second_base: &str| {
            let first = hunk(HEAD, "itoa = \"1\"\n", "", "leaf = \"1\"\n", "");
            let second = hunk(
                "\n[dev-dependencies]\n",
                "a = \"1\"\n",
                second_base,
                "b = \"1\"\n",
                "",
            );
            format!("{first}{second}")
        };
        // Both hunks insertions, in different tables.
        let merged = ok(&two(""));
        assert!(merged.contains("itoa") && merged.contains("b = \"1\""));
        // The second is an edit.
        refused(&two("old = \"1\"\n"));
    }

    #[test]
    fn markers_out_of_order_or_unclosed_are_refused() {
        refused("[dependencies]\n<<<<<<< ours\na = \"1\"\n");
        refused("[dependencies]\n=======\n");
        refused("[dependencies]\n>>>>>>> theirs\n");
        refused("[dependencies]\n<<<<<<< ours\n<<<<<<< ours\n");
        // No hunk at all.
        refused("[dependencies]\na = \"1\"\n");
    }
}
