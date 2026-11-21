//! Read-only reading of one `Cargo.toml` for the manifest rules
//! (`docs/formats/cargo-lock.md` section 4.5).
//!
//! Only what the rules need is read, and every form this module does not read
//! exactly is [`Unread`], never skipped: an unknown key in a dependency, a
//! `[replace]` table, a `[patch]` entry that is not read exactly (ADR 0018), a dependency that inherits from the
//! workspace with extra keys, a path that leaves the repository. The callers
//! turn `Unread` into a refusal (R14).

use semver::{Version, VersionReq};
use std::collections::{BTreeMap, BTreeSet};
use toml_edit::{Document, Item, TableLike, Value};

/// A manifest or a part of one that is not read exactly, with the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unread(pub String);

fn unread<T>(why: impl Into<String>) -> Result<T, Unread> {
    Err(Unread(why.into()))
}

/// Which dependency table a declaration is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Normal,
    Dev,
    Build,
}

/// Where a dependency comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Registry,
    /// A path dependency, as a normalized repository path of its directory.
    Path(String),
    /// A git dependency: the url and the branch, tag or revision, as written.
    Git(String),
}

/// One dependency declaration, after resolving `workspace = true`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decl {
    /// The real crate name (`package = ...` when renamed, else the key).
    pub package: String,
    /// The version requirement text, when the declaration has one.
    pub req: Option<String>,
    pub source: Source,
    pub features: BTreeSet<String>,
    pub default_features: bool,
    pub optional: bool,
}

impl Decl {
    /// Declares no feature selection: no `features`, default features on, not
    /// optional.
    pub fn is_plain(&self) -> bool {
        self.features.is_empty() && self.default_features && !self.optional
    }
}

/// The key of a declaration: its table, the target it is limited to, and the
/// key it is written under.
pub type DepKey = (Kind, Option<String>, String);

/// The parts of a `[workspace]` table that other manifests inherit.
#[derive(Clone, Debug, Default)]
pub struct Workspace {
    /// The directory of the workspace root, as a repository path.
    pub dir: String,
    pub members: Vec<String>,
    pub exclude: Vec<String>,
    deps: BTreeMap<String, Raw>,
    version: Option<String>,
    edition: Option<String>,
}

/// One manifest.
#[derive(Clone, Debug, Default)]
pub struct Manifest {
    pub name: Option<String>,
    /// The package version; `0.0.0` when the manifest has none, as Cargo
    /// writes it in the lockfile.
    pub version: Option<Version>,
    pub deps: BTreeMap<DepKey, Decl>,
    /// The `[features]` table, entries as written.
    pub features: BTreeMap<String, Vec<String>>,
    pub resolver: Option<String>,
    pub edition: Option<String>,
    /// The `[package] links` value, for the native-links proof (R15).
    pub links: Option<String>,
    /// Present for the workspace root.
    pub workspace: Option<Workspace>,
    /// The `[patch]` tables of the root manifest, normalized (ADR 0018): by
    /// source as written, then by the key each entry is written under.
    pub patch: Patch,
}

/// One `[patch]` entry: every key it has, with a canonical value text. The
/// `path` is normalized to a repository path and `features` is sorted.
pub type PatchEntry = BTreeMap<String, String>;

/// `[patch.<source>]` tables: source as written, then entry key.
pub type Patch = BTreeMap<String, BTreeMap<String, PatchEntry>>;

/// A dependency as written, before inheritance.
#[derive(Clone, Debug, Default)]
struct Raw {
    workspace: bool,
    version: Option<String>,
    path: Option<String>,
    git: Option<String>,
    git_ref: Option<String>,
    features: Option<Vec<String>>,
    default_features: Option<bool>,
    optional: Option<bool>,
    package: Option<String>,
    /// Keys other than `workspace`, `features` and `optional`.
    other_keys: bool,
}

/// Joins `rel` to the repository directory `dir` and normalizes `.` and `..`.
/// A path that is absolute, uses a backslash or leaves the repository is
/// unread.
pub fn join(dir: &str, rel: &str) -> Result<String, Unread> {
    if rel.starts_with('/') || rel.contains('\\') || rel.contains(':') {
        return unread(format!("path {rel:?} is not a relative repository path"));
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return unread(format!("path {rel:?} leaves the repository"));
                }
            }
            p => parts.push(p),
        }
    }
    Ok(parts.join("/"))
}

/// The directory part of a repository path ("" for the top level).
pub fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

fn string(item: &Item, what: &str) -> Result<String, Unread> {
    match item.as_str() {
        Some(s) => Ok(s.to_string()),
        None => unread(format!("{what} is not a string")),
    }
}

fn string_list(item: &Item, what: &str) -> Result<Vec<String>, Unread> {
    let Some(array) = item.as_array() else {
        return unread(format!("{what} is not an array"));
    };
    array
        .iter()
        .map(|v| match v {
            Value::String(s) => Ok(s.value().clone()),
            _ => unread(format!("{what} has an entry that is not a string")),
        })
        .collect()
}

fn raw_decl(item: &Item, key: &str) -> Result<Raw, Unread> {
    if let Some(req) = item.as_str() {
        return Ok(Raw {
            version: Some(req.to_string()),
            ..Raw::default()
        });
    }
    let Some(t) = item.as_table_like() else {
        return unread(format!("dependency {key} is neither a string nor a table"));
    };
    let mut raw = Raw::default();
    let (mut branch, mut tag, mut rev) = (None, None, None);
    for (k, v) in t.iter() {
        let what = format!("dependency {key}, key {k}");
        match k {
            "workspace" => match v.as_bool() {
                Some(b) => raw.workspace = b,
                None => return unread(format!("{what} is not a boolean")),
            },
            "version" => raw.version = Some(string(v, &what)?),
            "path" => raw.path = Some(string(v, &what)?),
            "git" => raw.git = Some(string(v, &what)?),
            "branch" => branch = Some(string(v, &what)?),
            "tag" => tag = Some(string(v, &what)?),
            "rev" => rev = Some(string(v, &what)?),
            "features" => raw.features = Some(string_list(v, &what)?),
            "default-features" => match v.as_bool() {
                Some(b) => raw.default_features = Some(b),
                None => return unread(format!("{what} is not a boolean")),
            },
            "optional" => match v.as_bool() {
                Some(b) => raw.optional = Some(b),
                None => return unread(format!("{what} is not a boolean")),
            },
            "package" => raw.package = Some(string(v, &what)?),
            // `registry`, `public`, `artifact`, `lib`, `target`, the
            // underscore spelling of `default-features` and anything else.
            _ => return unread(format!("{what} is not read")),
        }
        if !matches!(k, "workspace" | "features" | "optional") {
            raw.other_keys = true;
        }
    }
    let refs: Vec<String> = [("branch", branch), ("tag", tag), ("rev", rev)]
        .into_iter()
        .filter_map(|(n, v)| v.map(|v| format!("{n}={v}")))
        .collect();
    if refs.len() > 1 {
        return unread(format!("dependency {key} names several git references"));
    }
    raw.git_ref = refs.into_iter().next();
    Ok(raw)
}

fn resolve(key: &str, raw: &Raw, dir: &str, ws: Option<&Workspace>) -> Result<Decl, Unread> {
    let (base, ws_features, base_dir) = if raw.workspace {
        if raw.other_keys {
            return unread(format!(
                "dependency {key} inherits from the workspace and has other keys"
            ));
        }
        let Some(ws) = ws else {
            return unread(format!("dependency {key} inherits from no workspace"));
        };
        let Some(w) = ws.deps.get(key) else {
            return unread(format!("workspace has no dependency {key}"));
        };
        if w.workspace {
            return unread(format!("workspace dependency {key} inherits itself"));
        }
        (w, w.features.clone().unwrap_or_default(), ws.dir.as_str())
    } else {
        (raw, Vec::new(), dir)
    };
    // Inherited: the workspace's features plus the member's own. Else the
    // declaration's own (`ws_features` is empty then).
    let mut features: BTreeSet<String> = ws_features.into_iter().collect();
    features.extend(raw.features.clone().unwrap_or_default());
    let source = match (&base.path, &base.git) {
        (Some(_), Some(_)) => return unread(format!("dependency {key} has a path and a git url")),
        (Some(p), None) => Source::Path(join(base_dir, p)?),
        (None, Some(g)) => Source::Git(match &base.git_ref {
            Some(r) => format!("{g}?{r}"),
            None => g.clone(),
        }),
        (None, None) => Source::Registry,
    };
    if source == Source::Registry && base.version.is_none() {
        return unread(format!("dependency {key} has no version"));
    }
    if let Some(req) = &base.version
        && VersionReq::parse(req).is_err()
    {
        return unread(format!("dependency {key} has the requirement {req:?}"));
    }
    let optional = raw.optional.unwrap_or(false);
    Ok(Decl {
        package: base.package.clone().unwrap_or_else(|| key.to_string()),
        req: base.version.clone(),
        source,
        features,
        default_features: base.default_features.unwrap_or(true),
        optional,
    })
}

fn dep_tables(
    t: &dyn TableLike,
    kind_names: &[(&str, Kind)],
    target: Option<&str>,
    dir: &str,
    ws: Option<&Workspace>,
    out: &mut BTreeMap<DepKey, Decl>,
) -> Result<(), Unread> {
    for (name, kind) in kind_names {
        let Some(item) = t.get(name) else { continue };
        let Some(table) = item.as_table_like() else {
            return unread(format!("{name} is not a table"));
        };
        for (key, value) in table.iter() {
            let raw = raw_decl(value, key)?;
            let decl = resolve(key, &raw, dir, ws)?;
            let k = (*kind, target.map(str::to_string), key.to_string());
            if out.insert(k, decl).is_some() {
                return unread(format!("dependency {key} is declared twice"));
            }
        }
    }
    Ok(())
}

fn workspace_table(t: &dyn TableLike, dir: &str) -> Result<Workspace, Unread> {
    let list = |key: &str| -> Result<Vec<String>, Unread> {
        match t.get(key) {
            None => Ok(Vec::new()),
            Some(i) => string_list(i, &format!("workspace {key}")),
        }
    };
    let mut ws = Workspace {
        dir: dir.to_string(),
        members: list("members")?,
        exclude: list("exclude")?,
        ..Workspace::default()
    };
    if let Some(deps) = t.get("dependencies") {
        let Some(deps) = deps.as_table_like() else {
            return unread("workspace dependencies is not a table");
        };
        for (key, value) in deps.iter() {
            ws.deps.insert(key.to_string(), raw_decl(value, key)?);
        }
    }
    if let Some(p) = t.get("package") {
        let Some(p) = p.as_table_like() else {
            return unread("workspace package is not a table");
        };
        if let Some(v) = p.get("version") {
            ws.version = Some(string(v, "workspace package version")?);
        }
        if let Some(v) = p.get("edition") {
            ws.edition = Some(string(v, "workspace package edition")?);
        }
    }
    Ok(ws)
}

/// The keys a `[patch]` entry may have: those of a dependency declaration that
/// name a source or select what is built.
const PATCH_KEYS: [&str; 11] = [
    "version",
    "path",
    "git",
    "branch",
    "tag",
    "rev",
    "package",
    "registry",
    "features",
    "default-features",
    "optional",
];

/// Reads the `[patch]` table exactly: every source's table of entries, each
/// entry a table with only the keys of [`PATCH_KEYS`], values of the expected
/// types, and a source (`path`, `git` or `registry`). Anything else is unread.
fn patch_tables(item: &Item, dir: &str) -> Result<Patch, Unread> {
    let Some(sources) = item.as_table_like() else {
        return unread("patch is not a table");
    };
    let mut out = Patch::new();
    for (source, entries) in sources.iter() {
        let Some(entries) = entries.as_table_like() else {
            return unread(format!("patch source {source} is not a table"));
        };
        let mut table = BTreeMap::new();
        for (name, entry) in entries.iter() {
            let what = format!("patch {source} entry {name}");
            let Some(t) = entry.as_table_like() else {
                return unread(format!("{what} is not a table"));
            };
            let mut canon = PatchEntry::new();
            for (k, v) in t.iter() {
                if !PATCH_KEYS.contains(&k) {
                    return unread(format!("{what}, key {k} is not read"));
                }
                let text = match k {
                    "path" => format!("s:{}", join(dir, &string(v, &what)?)?),
                    "features" => {
                        let mut f = string_list(v, &what)?;
                        f.sort();
                        f.dedup();
                        format!("l:{}", f.join(","))
                    }
                    "default-features" | "optional" => match v.as_bool() {
                        Some(b) => format!("b:{b}"),
                        None => return unread(format!("{what}, key {k} is not a boolean")),
                    },
                    _ => format!("s:{}", string(v, &what)?),
                };
                canon.insert(k.to_string(), text);
            }
            let sources = ["path", "git", "registry"]
                .iter()
                .filter(|k| canon.contains_key(**k))
                .count();
            if sources == 0 || (canon.contains_key("path") && canon.contains_key("git")) {
                return unread(format!("{what} does not name exactly one source"));
            }
            if table.insert(name.to_string(), canon).is_some() {
                return unread(format!("{what} is declared twice"));
            }
        }
        out.insert(source.to_string(), table);
    }
    Ok(out)
}

impl Manifest {
    /// The repository directories of the `path` entries of `[patch]`.
    pub fn patch_paths(&self) -> Vec<String> {
        self.patch
            .values()
            .flat_map(|t| t.values())
            .filter_map(|e| e.get("path"))
            .filter_map(|p| p.strip_prefix("s:"))
            .map(str::to_string)
            .collect()
    }
}

/// A field of `[package]` that is a string or `{ workspace = true }`.
fn inheritable(
    p: &dyn TableLike,
    key: &str,
    from_ws: impl Fn(&Workspace) -> Option<String>,
    ws: Option<&Workspace>,
) -> Result<Option<String>, Unread> {
    let Some(item) = p.get(key) else {
        return Ok(None);
    };
    if let Some(s) = item.as_str() {
        return Ok(Some(s.to_string()));
    }
    if let Some(t) = item.as_table_like()
        && t.len() == 1
        && t.get("workspace").and_then(Item::as_bool) == Some(true)
    {
        return match ws.and_then(from_ws) {
            Some(v) => Ok(Some(v)),
            None => unread(format!(
                "package {key} inherits a value the workspace lacks"
            )),
        };
    }
    unread(format!("package {key} is not read"))
}

/// Reads one manifest located in the repository directory `dir`. `root_ws` is
/// the workspace of the root manifest, for inheritance; the root manifest
/// itself passes `None` and uses its own `[workspace]` table.
pub fn parse(text: &str, dir: &str, root_ws: Option<&Workspace>) -> Result<Manifest, Unread> {
    let doc = Document::parse(text).map_err(|e| Unread(format!("not TOML: {}", e.message())))?;
    let root = doc.as_table();
    for key in ["replace", "dev_dependencies", "build_dependencies"] {
        if root.contains_key(key) {
            return unread(format!("the table `{key}` is not read"));
        }
    }
    // Only the workspace root's `[patch]` applies. One in a member manifest
    // is not read.
    let patch = match root.get("patch") {
        None => Patch::new(),
        Some(_) if root_ws.is_some() => return unread("a member manifest has a [patch] table"),
        Some(item) => patch_tables(item, dir)?,
    };
    let own_ws = match root.get("workspace") {
        None => None,
        Some(item) => match item.as_table_like() {
            Some(t) => {
                if root_ws.is_some() {
                    return unread("a member manifest has its own [workspace]");
                }
                Some(workspace_table(t, dir)?)
            }
            None => return unread("workspace is not a table"),
        },
    };
    let ws = own_ws.as_ref().or(root_ws);
    let mut m = Manifest {
        patch,
        ..Manifest::default()
    };
    if let Some(item) = root.get("package") {
        let Some(p) = item.as_table_like() else {
            return unread("package is not a table");
        };
        let Some(name) = p.get("name") else {
            return unread("package has no name");
        };
        m.name = Some(string(name, "package name")?);
        let version = inheritable(p, "version", |w| w.version.clone(), ws)?;
        m.version = Some(match version {
            None => Version::new(0, 0, 0),
            Some(v) => match Version::parse(&v) {
                Ok(v) => v,
                Err(_) => return unread(format!("package version {v:?} is not semver")),
            },
        });
        m.edition = inheritable(p, "edition", |w| w.edition.clone(), ws)?;
        if let Some(r) = p.get("resolver") {
            m.resolver = Some(string(r, "package resolver")?);
        }
        if let Some(l) = p.get("links") {
            m.links = Some(string(l, "package links")?);
        }
    }
    if let Some(item) = root.get("features") {
        let Some(t) = item.as_table_like() else {
            return unread("features is not a table");
        };
        for (k, v) in t.iter() {
            m.features
                .insert(k.to_string(), string_list(v, &format!("feature {k}"))?);
        }
    }
    let kinds = [
        ("dependencies", Kind::Normal),
        ("dev-dependencies", Kind::Dev),
        ("build-dependencies", Kind::Build),
    ];
    dep_tables(root, &kinds, None, dir, ws, &mut m.deps)?;
    if let Some(item) = root.get("target") {
        let Some(targets) = item.as_table_like() else {
            return unread("target is not a table");
        };
        for (cfg, value) in targets.iter() {
            let Some(t) = value.as_table_like() else {
                return unread(format!("target {cfg} is not a table"));
            };
            if let Some(k) = t
                .iter()
                .map(|(k, _)| k)
                .find(|k| !kinds.iter().any(|(n, _)| n == k))
            {
                return unread(format!("target {cfg} has the key {k}"));
            }
            dep_tables(t, &kinds, Some(cfg), dir, ws, &mut m.deps)?;
        }
    }
    if let Some(w) = &own_ws {
        // Workspace dependencies that nothing inherits still count as
        // declarations of the root: they decide nothing by themselves, so
        // only their path targets matter, for finding manifests.
        m.resolver = match root["workspace"].get("resolver") {
            Some(r) => Some(string(r, "workspace resolver")?),
            None => m.resolver,
        };
        m.workspace = Some(w.clone());
    }
    Ok(m)
}

impl Workspace {
    /// The path targets of the `[workspace.dependencies]`, resolved.
    pub fn dep_paths(&self) -> Result<Vec<String>, Unread> {
        let mut out = Vec::new();
        for raw in self.deps.values() {
            if let Some(p) = &raw.path {
                out.push(join(&self.dir, p)?);
            }
        }
        Ok(out)
    }
}

/// What decides which features reach which dependencies, in one manifest.
/// Two manifests with the same `Selection` request the same features of the
/// same dependencies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub features: BTreeMap<String, Vec<String>>,
    pub resolver: Option<String>,
    pub edition: Option<String>,
    /// Declarations without their version requirement.
    pub decls: BTreeMap<DepKey, Decl>,
    /// The workspace's `members` and `exclude` as written, sorted, for a root
    /// manifest. Cargo resolves the features of every member, so a package
    /// that joins or leaves the workspace changes which features apply.
    pub members: Option<(Vec<String>, Vec<String>)>,
}

impl Manifest {
    pub fn selection(&self) -> Selection {
        Selection {
            features: self.features.clone(),
            resolver: self.resolver.clone(),
            edition: self.edition.clone(),
            members: self.workspace.as_ref().map(|w| {
                let sorted = |v: &Vec<String>| {
                    let mut v = v.clone();
                    v.sort();
                    v
                };
                (sorted(&w.members), sorted(&w.exclude))
            }),
            decls: self
                .deps
                .iter()
                .map(|(k, d)| {
                    let mut d = d.clone();
                    d.req = None;
                    (k.clone(), d)
                })
                .collect(),
        }
    }

    /// Every declaration of the crate named `package`.
    pub fn decls_of<'a>(&'a self, package: &'a str) -> impl Iterator<Item = &'a Decl> {
        self.deps.values().filter(move |d| d.package == package)
    }
}
