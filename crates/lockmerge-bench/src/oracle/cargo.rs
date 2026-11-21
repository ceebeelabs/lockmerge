//! The `Cargo.lock` oracle. Written from `docs/formats/cargo-lock.md`
//! sections 2, 3 and 5 and from `docs/02-oracle.md` section 4, with only
//! `toml_edit` as a dependency. It shares no code with any merge rule.
//!
//! Where the format document marks a point unverified (the order of two
//! packages that differ only in source, an edge written with a source but no
//! version), this oracle answers "not canonical". A false "no" makes the gate
//! stricter and sends the case to the owner; a false "yes" would hide a bug.

use super::Oracle;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use toml_edit::{DocumentMut, Item, Table};

pub struct CargoOracle;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pkg {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
    dependencies: Vec<String>,
    replace: Option<String>,
}

impl Pkg {
    /// `name version` or `name version (source)`: the package id.
    fn id(&self) -> String {
        match &self.source {
            Some(s) => format!("{} {} ({s})", self.name, self.version),
            None => format!("{} {}", self.name, self.version),
        }
    }
}

struct Lock {
    version: Option<i64>,
    packages: Vec<Pkg>,
    patch_unused: Vec<Pkg>,
    metadata: BTreeMap<String, String>,
}

fn string_of(item: &Item, what: &str) -> Result<String, String> {
    item.as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("{what} is not a string"))
}

fn parse_pkg(t: &Table) -> Result<Pkg, String> {
    let mut name = None;
    let mut version = None;
    let mut source = None;
    let mut checksum = None;
    let mut replace = None;
    let mut dependencies = Vec::new();
    for (k, v) in t.iter() {
        match k {
            "name" => name = Some(string_of(v, "name")?),
            "version" => version = Some(string_of(v, "version")?),
            "source" => source = Some(string_of(v, "source")?),
            "checksum" => checksum = Some(string_of(v, "checksum")?),
            "replace" => replace = Some(string_of(v, "replace")?),
            "dependencies" => {
                let arr = v
                    .as_value()
                    .and_then(|v| v.as_array())
                    .ok_or("dependencies is not an array")?;
                for e in arr.iter() {
                    dependencies.push(
                        e.as_str()
                            .ok_or("a dependency is not a string")?
                            .to_string(),
                    );
                }
            }
            other => return Err(format!("unknown package key `{other}`")),
        }
    }
    Ok(Pkg {
        name: name.ok_or("package without name")?,
        version: version.ok_or("package without version")?,
        source,
        checksum,
        dependencies,
        replace,
    })
}

fn parse(bytes: &[u8]) -> Result<Lock, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "not UTF-8".to_string())?;
    let doc: DocumentMut = text.parse().map_err(|e| format!("not valid TOML: {e}"))?;
    let mut lock = Lock {
        version: None,
        packages: Vec::new(),
        patch_unused: Vec::new(),
        metadata: BTreeMap::new(),
    };
    for (k, v) in doc.as_table().iter() {
        match k {
            "version" => {
                lock.version = Some(v.as_integer().ok_or("version is not an integer")?);
            }
            "package" => {
                let aot = v
                    .as_array_of_tables()
                    .ok_or("package is not an array of tables")?;
                for t in aot.iter() {
                    lock.packages.push(parse_pkg(t)?);
                }
            }
            "patch" => {
                let t = v.as_table().ok_or("patch is not a table")?;
                for (pk, pv) in t.iter() {
                    if pk != "unused" {
                        return Err(format!("unknown patch key `{pk}`"));
                    }
                    let aot = pv
                        .as_array_of_tables()
                        .ok_or("patch.unused is not an array of tables")?;
                    for t in aot.iter() {
                        lock.patch_unused.push(parse_pkg(t)?);
                    }
                }
            }
            "metadata" => {
                let t = v.as_table().ok_or("metadata is not a table")?;
                for (mk, mv) in t.iter() {
                    lock.metadata
                        .insert(mk.to_string(), string_of(mv, "a metadata value")?);
                }
            }
            other => return Err(format!("unknown top-level key `{other}`")),
        }
    }
    Ok(lock)
}

/// The index of the one package an edge names, or why there is not exactly one.
fn resolve(entry: &str, pkgs: &[Pkg]) -> Result<usize, String> {
    let (name, rest) = match entry.split_once(' ') {
        Some((n, r)) => (n, Some(r)),
        None => (entry, None),
    };
    let (version, source) = match rest {
        None => (None, None),
        Some(r) => match r.split_once(" (") {
            Some((v, s)) => (
                Some(v),
                Some(
                    s.strip_suffix(')')
                        .ok_or_else(|| format!("malformed edge `{entry}`"))?,
                ),
            ),
            None => (Some(r), None),
        },
    };
    let hits: Vec<usize> = pkgs
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            p.name == name
                && version.is_none_or(|v| p.version == v)
                && source.is_none_or(|s| p.source.as_deref() == Some(s))
        })
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!("edge `{entry}` names no package")),
        _ => Err(format!("edge `{entry}` is ambiguous")),
    }
}

fn check_unique(lock: &Lock) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for p in &lock.packages {
        if !seen.insert(p.id()) {
            return Err(format!("duplicate package {}", p.id()));
        }
    }
    Ok(())
}

// ---- version order (semver 2.0.0) ---------------------------------------

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Ident {
    Num(u64),
    Alpha(String),
}

struct Ver {
    core: [u64; 3],
    pre: Vec<Ident>,
    build: String,
}

fn parse_ver(s: &str) -> Option<Ver> {
    let (rest, build) = match s.split_once('+') {
        Some((r, b)) => (r, b.to_string()),
        None => (s, String::new()),
    };
    let (core, pre) = match rest.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (rest, None),
    };
    let mut it = core.split('.');
    let mut nums = [0u64; 3];
    for n in nums.iter_mut() {
        *n = it.next()?.parse().ok()?;
    }
    if it.next().is_some() {
        return None;
    }
    let pre = match pre {
        None => Vec::new(),
        Some(p) => p
            .split('.')
            .map(|i| match i.parse::<u64>() {
                Ok(n) => Ident::Num(n),
                Err(_) => Ident::Alpha(i.to_string()),
            })
            .collect(),
    };
    Some(Ver {
        core: nums,
        pre,
        build,
    })
}

fn cmp_ver(a: &Ver, b: &Ver) -> Ordering {
    a.core
        .cmp(&b.core)
        .then_with(|| match (a.pre.is_empty(), b.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => a.pre.cmp(&b.pre),
        })
        .then_with(|| a.build.cmp(&b.build))
}

// ---- L0: the canonical writer, used only to compare -----------------------

fn quote(s: &str) -> Result<String, String> {
    if s.chars().any(|c| c == '"' || c == '\\' || c.is_control()) {
        return Err("a string needs escaping, which the oracle does not verify".into());
    }
    Ok(format!("\"{s}\""))
}

/// How an edge to `pkgs[idx]` is written: the shortest form that names it
/// among `pkgs` (document section 3).
fn edge_text(pkgs: &[Pkg], idx: usize) -> Result<String, String> {
    let p = &pkgs[idx];
    let same_name: Vec<&Pkg> = pkgs.iter().filter(|q| q.name == p.name).collect();
    if same_name.len() == 1 {
        return Ok(p.name.clone());
    }
    let versions: BTreeSet<&str> = same_name.iter().map(|q| q.version.as_str()).collect();
    if versions.len() == 1 {
        return Err(format!(
            "`{}` has one version from several sources: the edge form is unverified",
            p.name
        ));
    }
    let same_version = same_name.iter().filter(|q| q.version == p.version).count();
    if same_version == 1 {
        Ok(format!("{} {}", p.name, p.version))
    } else {
        Ok(format!(
            "{} {} ({})",
            p.name,
            p.version,
            p.source.as_deref().unwrap_or("")
        ))
    }
}

fn sort_key_cmp(a: &Pkg, b: &Pkg) -> Result<Ordering, String> {
    let (va, vb) = (
        parse_ver(&a.version).ok_or_else(|| format!("version `{}` is not semver", a.version))?,
        parse_ver(&b.version).ok_or_else(|| format!("version `{}` is not semver", b.version))?,
    );
    Ok(a.name
        .cmp(&b.name)
        .then_with(|| cmp_ver(&va, &vb))
        .then_with(|| a.source.cmp(&b.source)))
}

fn sorted(pkgs: &[Pkg]) -> Result<Vec<Pkg>, String> {
    // Validate every version first so the comparator cannot fail.
    for p in pkgs {
        parse_ver(&p.version).ok_or_else(|| format!("version `{}` is not semver", p.version))?;
    }
    let mut v = pkgs.to_vec();
    v.sort_by(|a, b| sort_key_cmp(a, b).unwrap_or(Ordering::Equal));
    Ok(v)
}

fn block(heading: &str, p: &Pkg, deps: &[String]) -> Result<String, String> {
    let mut s = format!(
        "{heading}\nname = {}\nversion = {}\n",
        quote(&p.name)?,
        quote(&p.version)?
    );
    if let Some(src) = &p.source {
        s.push_str(&format!("source = {}\n", quote(src)?));
    }
    if let Some(c) = &p.checksum {
        s.push_str(&format!("checksum = {}\n", quote(c)?));
    }
    if !deps.is_empty() {
        s.push_str("dependencies = [\n");
        for d in deps {
            s.push_str(&format!(" {},\n", quote(d)?));
        }
        s.push_str("]\n");
    }
    if let Some(r) = &p.replace {
        s.push_str(&format!("replace = {}\n", quote(r)?));
    }
    Ok(s)
}

const HEADER: [&str; 2] = [
    "# This file is automatically @generated by Cargo.",
    "# It is not intended for manual editing.",
];

/// The leading `#` lines of a lockfile. Cargo preserves them when it rewrites
/// a file; the header must come first. Where extra comments sit relative to
/// the header is unverified, so only the header-first form is accepted.
fn leading_comments(bytes: &[u8]) -> Result<Vec<&str>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "not UTF-8".to_string())?;
    let lines: Vec<&str> = text.lines().take_while(|l| l.starts_with('#')).collect();
    if lines.len() < 2 || lines[..2] != HEADER {
        return Err("the Cargo header is missing".into());
    }
    Ok(lines)
}

/// What Cargo would write for this lockfile's content, given the leading
/// comments of the input.
fn render(lock: &Lock, comments: &[&str]) -> Result<String, String> {
    let version = match lock.version {
        Some(v @ (3 | 4)) => v,
        other => return Err(format!("lockfile version {other:?} is not 3 or 4")),
    };
    if lock.patch_unused.iter().any(|p| !p.dependencies.is_empty()) {
        return Err("a `patch.unused` entry with dependencies is unverified".into());
    }
    let pkgs = sorted(&lock.packages)?;
    let mut blocks = Vec::new();
    for p in &pkgs {
        let mut idxs = Vec::new();
        for d in &p.dependencies {
            idxs.push(resolve(d, &pkgs)?);
        }
        idxs.sort();
        for w in idxs.windows(2) {
            if w[0] == w[1] {
                return Err("a package lists one edge twice".into());
            }
        }
        // Cargo orders the encoded ids (name, then version as a string, then
        // source), independently of the order of package blocks.
        type EdgeKey = (String, Option<String>, Option<String>);
        let mut keyed: Vec<(EdgeKey, String)> = Vec::new();
        for i in &idxs {
            let text = edge_text(&pkgs, *i)?;
            let q = &pkgs[*i];
            let key = (
                q.name.clone(),
                text.contains(' ').then(|| q.version.clone()),
                text.ends_with(')')
                    .then(|| q.source.clone().unwrap_or_default()),
            );
            keyed.push((key, text));
        }
        keyed.sort();
        let deps: Vec<String> = keyed.into_iter().map(|(_, t)| t).collect();
        blocks.push(block("[[package]]", p, &deps)?);
    }
    // Cargo's writer emits `[[patch.unused]]` before `[metadata]`. The key
    // quoting of metadata is unverified; a mismatch makes the file not
    // canonical, which is the strict direction.
    for p in sorted(&lock.patch_unused)? {
        blocks.push(block("[[patch.unused]]", &p, &[])?);
    }
    if !lock.metadata.is_empty() {
        let mut m = String::from("[metadata]\n");
        for (k, v) in &lock.metadata {
            let bare = !k.is_empty()
                && k.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            let key = if bare { k.clone() } else { quote(k)? };
            m.push_str(&format!("{key} = {}\n", quote(v)?));
        }
        blocks.push(m);
    }
    let mut out = String::new();
    for c in comments {
        out.push_str(c);
        out.push('\n');
    }
    out.push_str(&format!("version = {version}\n"));
    if !blocks.is_empty() {
        out.push('\n');
        out.push_str(&blocks.join("\n"));
    }
    Ok(out)
}

/// Names of the packages that have no `source`: workspace members and path
/// dependencies, whose manifests the extractor saves.
pub fn path_package_names(bytes: &[u8]) -> Result<BTreeSet<String>, String> {
    Ok(parse(bytes)?
        .packages
        .into_iter()
        .filter(|p| p.source.is_none())
        .map(|p| p.name)
        .collect())
}

impl Oracle for CargoOracle {
    fn ids(&self, bytes: &[u8]) -> Result<BTreeSet<String>, String> {
        Ok(parse(bytes)?.packages.iter().map(Pkg::id).collect())
    }

    fn canonical(&self, bytes: &[u8]) -> Result<(), String> {
        let lock = parse(bytes)?;
        let want = render(&lock, &leading_comments(bytes)?)?;
        if want.as_bytes() == bytes {
            Ok(())
        } else {
            Err("re-serializing does not reproduce the bytes".into())
        }
    }

    fn valid(&self, bytes: &[u8]) -> Result<(), String> {
        let lock = parse(bytes)?;
        if !matches!(lock.version, Some(3 | 4)) {
            return Err(format!("unknown lockfile version {:?}", lock.version));
        }
        check_unique(&lock)?;
        let mut referenced = BTreeSet::new();
        for p in &lock.packages {
            for d in &p.dependencies {
                referenced.insert(resolve(d, &lock.packages)?);
            }
        }
        // A replacement is a package with no dependencies of its own, whose
        // target is exactly one package of the file.
        for p in lock.packages.iter().chain(&lock.patch_unused) {
            if let Some(r) = &p.replace {
                if !p.dependencies.is_empty() {
                    return Err(format!("{} has both dependencies and replace", p.id()));
                }
                if !r.contains(" (") {
                    return Err(format!("replace `{r}` is not a full package id"));
                }
                referenced.insert(resolve(r, &lock.packages)?);
            }
        }
        for (i, p) in lock.packages.iter().enumerate() {
            if p.source.is_some() && !referenced.contains(&i) {
                return Err(format!("{} is not referenced by any package", p.id()));
            }
        }
        Ok(())
    }

    fn normalize(&self, bytes: &[u8]) -> Result<String, String> {
        let lock = parse(bytes)?;
        check_unique(&lock)?;
        let mut lines = vec![format!(
            "version={}",
            lock.version.map_or("none".to_string(), |v| v.to_string())
        )];
        let mut pkgs: Vec<String> = Vec::new();
        for p in &lock.packages {
            let mut deps = BTreeSet::new();
            for d in &p.dependencies {
                deps.insert(lock.packages[resolve(d, &lock.packages)?].id());
            }
            pkgs.push(format!(
                "pkg {}|checksum={:?}|replace={:?}|deps={:?}",
                p.id(),
                p.checksum,
                p.replace,
                deps
            ));
        }
        pkgs.sort();
        lines.extend(pkgs);
        let mut unused: Vec<String> = lock
            .patch_unused
            .iter()
            .map(|p| {
                let mut d = p.dependencies.clone();
                d.sort();
                format!(
                    "unused {}|checksum={:?}|replace={:?}|deps={:?}",
                    p.id(),
                    p.checksum,
                    p.replace,
                    d
                )
            })
            .collect();
        unused.sort();
        lines.extend(unused);
        for (k, v) in &lock.metadata {
            lines.push(format!("meta {k:?}={v:?}"));
        }
        Ok(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\n";

    fn lock(version: u32, body: &str) -> Vec<u8> {
        format!("{HEAD}version = {version}\n\n{body}").into_bytes()
    }

    const GOOD: &str = "[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\n \"dep\",\n]\n\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"aa\"\n";

    #[test]
    fn canonical_valid_file_passes_all_layers() {
        let b = lock(4, GOOD);
        CargoOracle.canonical(&b).unwrap();
        CargoOracle.valid(&b).unwrap();
        assert_eq!(CargoOracle.ids(&b).unwrap().len(), 2);
    }

    #[test]
    fn l0_refuses_what_cargo_would_rewrite() {
        // Two trailing newlines.
        let mut extra = lock(4, GOOD);
        extra.push(b'\n');
        assert!(CargoOracle.canonical(&extra).is_err());
        // Packages out of order.
        let swapped = "[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = \"s\"\nchecksum = \"aa\"\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\n \"dep\",\n]\n";
        assert!(CargoOracle.canonical(&lock(4, swapped)).is_err());
        // A long edge where the short one is enough.
        let long = GOOD.replace("\"dep\",", "\"dep 1.0.0\",");
        assert!(CargoOracle.canonical(&lock(4, &long)).is_err());
        // Not TOML at all: conflict markers.
        assert!(CargoOracle.canonical(b"<<<<<<< ours\n").is_err());
    }

    #[test]
    fn l0_orders_edges_by_encoded_id_string() {
        let blocks = "\n[[package]]\nname = \"x\"\nversion = \"1.9.0\"\nsource = \"s\"\nchecksum = \"a\"\n\n[[package]]\nname = \"x\"\nversion = \"1.10.0\"\nsource = \"s\"\nchecksum = \"b\"\n";
        let app = |first: &str, second: &str| {
            format!(
                "[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\n \"{first}\",\n \"{second}\",\n]\n{blocks}"
            )
        };
        // Package blocks are in semver order (1.9.0 first), but the edges are
        // ordered as encoded ids: "x 1.10.0" before "x 1.9.0".
        CargoOracle
            .canonical(&lock(4, &app("x 1.10.0", "x 1.9.0")))
            .unwrap();
        assert!(
            CargoOracle
                .canonical(&lock(4, &app("x 1.9.0", "x 1.10.0")))
                .is_err()
        );
    }

    #[test]
    fn l0_keeps_extra_leading_comments_but_needs_the_header() {
        let with = format!("{HEAD}# Licensed under MIT.\nversion = 4\n\n{GOOD}");
        CargoOracle.canonical(with.as_bytes()).unwrap();
        // No header: not Cargo's output.
        let none = format!("# Licensed under MIT.\nversion = 4\n\n{GOOD}");
        assert!(CargoOracle.canonical(none.as_bytes()).is_err());
    }

    #[test]
    fn l0_renders_metadata_and_replace() {
        let unused = "[[patch.unused]]\nname = \"zz\"\nversion = \"1.0.0\"\n";
        let both = format!("{GOOD}\n{unused}\n[metadata]\ncustom = \"x\"\n");
        CargoOracle.canonical(&lock(4, &both)).unwrap();
        // Neighbor: metadata before the unused patches is not Cargo's order.
        let wrong = format!("{GOOD}\n[metadata]\ncustom = \"x\"\n\n{unused}");
        assert!(CargoOracle.canonical(&lock(4, &wrong)).is_err());
        let with_replace = GOOD.replace(
            "checksum = \"aa\"\n",
            "checksum = \"aa\"\nreplace = \"dep 1.0.0 (git+x)\"\n",
        );
        CargoOracle.canonical(&lock(4, &with_replace)).unwrap();
        // Neighbor: `replace` before `checksum` is the wrong field order.
        let swapped = GOOD.replace(
            "checksum = \"aa\"\n",
            "replace = \"dep 1.0.0 (git+x)\"\nchecksum = \"aa\"\n",
        );
        assert!(CargoOracle.canonical(&lock(4, &swapped)).is_err());
    }

    #[test]
    fn l1_validates_replacements() {
        const SRC: &str = "registry+https://github.com/rust-lang/crates.io-index";
        let target = format!("dep 1.0.0 ({SRC})");
        // Accepted: a dependency-free package replaced by an existing one.
        let ok = format!(
            "{GOOD}\n[[package]]\nname = \"old\"\nversion = \"1.0.0\"\nreplace = \"{target}\"\n"
        );
        CargoOracle.valid(&lock(4, &ok)).unwrap();
        // Both dependencies and replace.
        let both = GOOD.replace(
            "dependencies = [\n \"dep\",\n]\n",
            &format!("dependencies = [\n \"dep\",\n]\nreplace = \"{target}\"\n"),
        );
        assert!(
            CargoOracle
                .valid(&lock(4, &both))
                .unwrap_err()
                .contains("both dependencies and replace")
        );
        // Missing target.
        let missing = ok.replace("dep 1.0.0 (", "gone 1.0.0 (");
        assert!(
            CargoOracle
                .valid(&lock(4, &missing))
                .unwrap_err()
                .contains("names no package")
        );
        // Ambiguous target: a bare name matching two packages.
        let ambiguous = format!(
            "{GOOD}\n[[package]]\nname = \"dep\"\nversion = \"2.0.0\"\nsource = \"{SRC}\"\nchecksum = \"bb\"\n\n[[package]]\nname = \"old\"\nversion = \"1.0.0\"\nreplace = \"dep 1.0.0\"\n"
        );
        assert!(CargoOracle.valid(&lock(4, &ambiguous)).is_err());
    }

    #[test]
    fn l1_refuses_each_structural_fault() {
        // Duplicate id.
        let dup = format!(
            "{GOOD}\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n"
        );
        assert!(
            CargoOracle
                .valid(&lock(4, &dup))
                .unwrap_err()
                .contains("duplicate")
        );
        // Dangling edge.
        let dangling = GOOD.replace("\"dep\",", "\"gone\",");
        assert!(
            CargoOracle
                .valid(&lock(4, &dangling))
                .unwrap_err()
                .contains("names no package")
        );
        // Ambiguous edge.
        let two = format!(
            "{GOOD}\n[[package]]\nname = \"dep\"\nversion = \"2.0.0\"\nsource = \"s\"\nchecksum = \"bb\"\n"
        );
        assert!(
            CargoOracle
                .valid(&lock(4, &two))
                .unwrap_err()
                .contains("ambiguous")
        );
        // A registry package nobody references.
        let orphan = format!(
            "{GOOD}\n[[package]]\nname = \"zzz\"\nversion = \"1.0.0\"\nsource = \"s\"\nchecksum = \"cc\"\n"
        );
        assert!(
            CargoOracle
                .valid(&lock(4, &orphan))
                .unwrap_err()
                .contains("not referenced")
        );
        // Version 2 and 5 are unknown to this oracle.
        assert!(CargoOracle.valid(&lock(5, GOOD)).is_err());
        assert!(CargoOracle.valid(GOOD.as_bytes()).is_err());
    }

    #[test]
    fn normalization_ignores_layout_and_edge_order_but_not_content() {
        let a = lock(4, GOOD);
        // Same content, other quoting, extra comment, and spaces.
        let b = format!(
            "{HEAD}# note\nversion = 4\n\n[[package]]\nname='app'\nversion = '0.1.0'\ndependencies = ['dep 1.0.0']\n\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"aa\"\n"
        );
        assert_eq!(
            CargoOracle.normalize(&a).unwrap(),
            CargoOracle.normalize(b.as_bytes()).unwrap()
        );
        // A changed checksum is a difference.
        let c = GOOD.replace("\"aa\"", "\"ab\"");
        assert_ne!(
            CargoOracle.normalize(&a).unwrap(),
            CargoOracle.normalize(&lock(4, &c)).unwrap()
        );
        // A dropped edge is a difference.
        let d = GOOD.replace("dependencies = [\n \"dep\",\n]\n", "");
        assert_ne!(
            CargoOracle.normalize(&a).unwrap(),
            CargoOracle.normalize(&lock(4, &d)).unwrap()
        );
    }

    #[test]
    fn unknown_keys_and_duplicate_keys_do_not_parse() {
        assert!(
            CargoOracle
                .ids(&lock(
                    4,
                    "[[package]]\nname = \"a\"\nversion = \"1.0.0\"\nfoo = \"x\"\n"
                ))
                .is_err()
        );
        assert!(
            CargoOracle
                .ids(&lock(
                    4,
                    "[[package]]\nname = \"a\"\nname = \"b\"\nversion = \"1.0.0\"\n"
                ))
                .is_err()
        );
    }
}
