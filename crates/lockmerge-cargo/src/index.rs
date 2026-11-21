//! Pure functions over the bytes of Cargo's registry index cache, for the
//! native-links proof (R15, V6). The layout read here is pinned in
//! `docs/formats/cargo-registry-index.md`; anything not pinned there is
//! refused, never guessed. No I/O: `lockmerge-env` reads the files.

use serde_json::Value;

/// The directory of an index directory that holds the cached entry files.
pub const CACHE_DIR: &str = ".cache";

/// The cache version byte at the start of a cache file (Cargo 1.96.0).
const CACHE_VERSION: u8 = 3;
/// The highest index schema version recorded in the header (Cargo 1.96.0).
const INDEX_V_MAX: [u8; 4] = [2, 0, 0, 0];

/// The path of a crate's entry file in the cache of one index directory, from
/// the crate name: `.cache/` and then the index path by name length (`1/n`,
/// `2/nn`, `3/n/nnn`, `ab/cd/abcdef`). `None` for a name that is not made of
/// ASCII lowercase letters, digits, `-` and `_`, whose file name was not
/// verified.
pub fn entry_path(name: &str) -> Option<String> {
    let ok = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if !ok {
        return None;
    }
    let rel = match name.len() {
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", &name[..1]),
        _ => format!("{}/{}/{name}", &name[..2], &name[2..4]),
    };
    Some(format!("{CACHE_DIR}/{rel}"))
}

/// Why a cache file is not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryError(pub String);

fn bad<T>(why: &str) -> Result<T, EntryError> {
    Err(EntryError(why.to_string()))
}

/// One record of a cache file: the version text and the JSON of that version,
/// not yet parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record<'a> {
    pub version: &'a str,
    pub json: &'a str,
}

/// Splits a cache file into its records, if it has exactly the layout of the
/// format document: the header, the index version string and NUL-ended pairs.
pub fn parse_entry(bytes: &[u8]) -> Result<Vec<Record<'_>>, EntryError> {
    let Some((&version, rest)) = bytes.split_first() else {
        return bad("the file is empty");
    };
    if version != CACHE_VERSION {
        return bad("the cache version is not 3");
    }
    let Some(rest) = rest.strip_prefix(&INDEX_V_MAX) else {
        return bad("the highest index version is not 2");
    };
    // After the header: the index version string, then pairs, each piece ended
    // by a NUL, and nothing after the last NUL.
    let Some(body) = rest.strip_suffix(&[0]) else {
        return bad("the file does not end with a NUL");
    };
    let pieces: Vec<&[u8]> = body.split(|&b| b == 0).collect();
    // The index version string, then a version text and a JSON per record.
    if pieces.len().is_multiple_of(2) {
        return bad("a record has no JSON");
    }
    let mut records = Vec::new();
    for pair in pieces[1..].chunks(2) {
        let (Ok(version), Ok(json)) = (std::str::from_utf8(pair[0]), std::str::from_utf8(pair[1]))
        else {
            return bad("a record is not UTF-8");
        };
        records.push(Record { version, json });
    }
    Ok(records)
}

/// The fields of one version the proof reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexVersion {
    pub name: String,
    pub vers: String,
    pub cksum: String,
    pub links: Option<String>,
}

impl Record<'_> {
    /// Parses the record's JSON. Exact: the fields of the format document, no
    /// others assumed, and a `links` that is anything but absent, `null` or a
    /// non-empty string is refused.
    pub fn summary(&self) -> Result<IndexVersion, EntryError> {
        let Ok(Value::Object(o)) = serde_json::from_str::<Value>(self.json) else {
            return bad("the JSON is not an object");
        };
        let string = |key: &str| match o.get(key) {
            Some(Value::String(s)) => Ok(s.clone()),
            _ => bad(&format!("`{key}` is not a string")),
        };
        let vers = string("vers")?;
        if vers != self.version {
            return bad("`vers` differs from the version text in front of it");
        }
        match o.get("v") {
            None => {}
            Some(Value::Number(n)) if n.as_u64().is_some_and(|n| n == 1 || n == 2) => {}
            _ => return bad("`v` is not 1 or 2"),
        }
        let links = match o.get("links") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => return bad("`links` is not a non-empty string"),
        };
        Ok(IndexVersion {
            name: string("name")?,
            vers,
            cksum: string("cksum")?,
            links,
        })
    }
}

/// What the cache says about one registry package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexLookup {
    /// An entry matches name, version and `cksum`; its `links`, if any.
    Found { links: Option<String> },
    /// Nothing proves it; the reason.
    Unavailable(String),
}

/// Looks `name` `version` up in the entry files of every index directory
/// (`files`, one per directory that has one). A file that is not read exactly
/// contributes nothing. The lookup is `Found` only if some file has exactly one
/// record of that version whose name, `vers` and `cksum` equal the arguments,
/// and every matching record of every file has the same `links`.
pub fn lookup(files: &[Vec<u8>], name: &str, version: &str, cksum: &str) -> IndexLookup {
    let mut found: Vec<Option<String>> = Vec::new();
    for bytes in files {
        let Ok(records) = parse_entry(bytes) else {
            continue;
        };
        let mut same: Vec<&Record<'_>> = records.iter().filter(|r| r.version == version).collect();
        if same.len() != 1 {
            continue;
        }
        let Ok(v) = same.remove(0).summary() else {
            continue;
        };
        if v.name == name && v.vers == version && v.cksum == cksum {
            found.push(v.links);
        }
    }
    match found.split_first() {
        None => {
            IndexLookup::Unavailable("no index entry matches name, version and checksum".into())
        }
        Some((first, rest)) if rest.iter().all(|l| l == first) => IndexLookup::Found {
            links: first.clone(),
        },
        Some(_) => IndexLookup::Unavailable("index directories disagree on `links`".into()),
    }
}
