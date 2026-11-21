//! Finding the manifests of a lockfile's path packages (section 4.5 of the
//! format document), and comparing feature selection between the merged
//! manifests and one side's.
//!
//! Everything comes through [`lockmerge_core::Manifests`]: the merged manifest
//! of a repository path, and one input's own manifest of a path. A trait that
//! lists no directories cannot expand a glob, so a `members` entry is read only
//! as a literal path or as `<directory>/*`, where each candidate is the
//! directory named like a path package of the lockfiles. Anything else is
//! unread and the manifest rules refuse (R14).

use crate::manifest::{
    DepKey, Manifest, Selection, Source, Unread, Workspace, dir_of, join, parse,
};
use crate::model::Id;
use crate::union;
use lockmerge_core::{Context, ManifestResult, Role};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

/// The merged manifests of every path package of the three lockfiles.
#[derive(Debug)]
pub struct Found {
    /// The repository directory of the lockfile and of the root manifest.
    pub root_dir: String,
    pub root: Manifest,
    /// Each path package of the lockfiles with its manifest's repository path
    /// and the merged manifest.
    pub packages: BTreeMap<Id, (String, Manifest)>,
    /// Manifests whose text merge conflicted and was read as two insertions
    /// (ADR 0019).
    pub kept_both: Vec<String>,
    /// Path packages of the lockfiles that no manifest declares any more
    /// because a branch moved the package to another version (ADR 0018): the
    /// merged manifest of that directory declares the new version.
    pub superseded: BTreeSet<Id>,
}

fn manifest_path(dir: &str) -> Result<String, Unread> {
    join(dir, "Cargo.toml")
}

fn has_glob(s: &str) -> bool {
    s.contains(['*', '?', '[', ']', '{', '}'])
}

/// The merged text of `path`. A conflict that is two insertions (ADR 0019) is
/// read as both, and the path is noted in `kept`.
fn read_merged(
    cx: &Context<'_>,
    path: &str,
    kept: &RefCell<Vec<String>>,
) -> Result<String, Unread> {
    match cx.manifests.merged(path) {
        ManifestResult::Merged(bytes) => match String::from_utf8(bytes) {
            Ok(s) => Ok(s),
            Err(_) => Err(Unread(format!("{path} is not UTF-8"))),
        },
        ManifestResult::Conflict => Err(Unread(format!("the text merge of {path} conflicts"))),
        ManifestResult::Conflicted(text) => match union::resolve(&text) {
            Ok(s) => {
                kept.borrow_mut().push(path.to_string());
                Ok(s)
            }
            Err(why) => Err(Unread(format!("the text merge of {path} conflicts: {why}"))),
        },
        ManifestResult::Unavailable(why) => Err(Unread(format!("{path}: {}", why.0))),
    }
}

/// Locates and reads the merged manifest of each id in `wanted` (the path
/// packages of the three lockfiles). `lock_path` is the repository path of the
/// lockfile.
pub fn find(wanted: &BTreeSet<Id>, lock_path: &str, cx: &Context<'_>) -> Result<Found, Unread> {
    find_with(wanted, &BTreeSet::new(), lock_path, cx)
}

/// [`find`], where each id of `superseded` may have no manifest: a branch
/// moved that path package to another version, which `wanted` holds too.
pub fn find_with(
    wanted: &BTreeSet<Id>,
    superseded: &BTreeSet<Id>,
    lock_path: &str,
    cx: &Context<'_>,
) -> Result<Found, Unread> {
    let root_dir = dir_of(lock_path).to_string();
    let root_path = manifest_path(&root_dir)?;
    let kept = RefCell::new(Vec::new());
    let text = read_merged(cx, &root_path, &kept)?;
    let root = parse(&text, &root_dir, None)?;
    let ws = root.workspace.clone();

    let mut loaded: BTreeMap<String, Manifest> = BTreeMap::new();
    loaded.insert(root_dir.clone(), root.clone());

    if let Some(ws) = &ws {
        if ws.exclude.iter().any(|e| has_glob(e)) {
            return Err(Unread("a workspace `exclude` has a glob".into()));
        }
        let excluded: BTreeSet<String> = ws
            .exclude
            .iter()
            .map(|e| join(&ws.dir, e))
            .collect::<Result<_, _>>()?;
        for member in &ws.members {
            members_of(member, ws, &excluded, wanted, cx, &kept, &mut loaded)?;
        }
    }

    // Path dependencies, at any depth. A manifest that cannot be read is an
    // error: the dependency is part of the graph.
    let mut queue: Vec<String> = Vec::new();
    if let Some(ws) = &ws {
        queue.extend(ws.dep_paths()?);
    }
    queue.extend(path_targets(loaded.values()));
    // The packages a `[patch]` entry points at are part of the graph.
    queue.extend(root.patch_paths());
    while let Some(dir) = queue.pop() {
        if loaded.contains_key(&dir) {
            continue;
        }
        let path = manifest_path(&dir)?;
        let text = read_merged(cx, &path, &kept)?;
        let m = parse(&text, &dir, ws.as_ref())?;
        queue.extend(path_targets(std::iter::once(&m)));
        loaded.insert(dir, m);
    }

    let mut by_id: BTreeMap<Id, (String, Manifest)> = BTreeMap::new();
    for (dir, m) in &loaded {
        let (Some(name), Some(version)) = (&m.name, &m.version) else {
            continue;
        };
        let id = Id {
            name: name.clone(),
            version: version.clone(),
            source: None,
        };
        if by_id
            .insert(id.clone(), (manifest_path(dir)?, m.clone()))
            .is_some()
        {
            return Err(Unread(format!("two manifests are the path package {id}")));
        }
    }
    let mut packages = BTreeMap::new();
    let mut gone = BTreeSet::new();
    for id in wanted {
        match by_id.remove(id) {
            Some(found) => {
                packages.insert(id.clone(), found);
            }
            None if superseded.contains(id) => {
                gone.insert(id.clone());
            }
            None => return Err(Unread(format!("no manifest for the path package {id}"))),
        }
    }
    Ok(Found {
        root_dir,
        root,
        packages,
        kept_both: kept.into_inner(),
        superseded: gone,
    })
}

fn path_targets<'a>(manifests: impl Iterator<Item = &'a Manifest>) -> Vec<String> {
    manifests
        .flat_map(|m| m.deps.values())
        .filter_map(|d| match &d.source {
            Source::Path(p) => Some(p.clone()),
            _ => None,
        })
        .collect()
}

fn members_of(
    member: &str,
    ws: &Workspace,
    excluded: &BTreeSet<String>,
    wanted: &BTreeSet<Id>,
    cx: &Context<'_>,
    kept: &RefCell<Vec<String>>,
    loaded: &mut BTreeMap<String, Manifest>,
) -> Result<(), Unread> {
    if !has_glob(member) {
        let dir = join(&ws.dir, member)?;
        if excluded.contains(&dir) || loaded.contains_key(&dir) {
            return Ok(());
        }
        let path = manifest_path(&dir)?;
        let text = read_merged(cx, &path, kept)?;
        loaded.insert(dir.clone(), parse(&text, &dir, Some(ws))?);
        return Ok(());
    }
    let Some(prefix) = member.strip_suffix("/*").filter(|p| !has_glob(p)) else {
        return Err(Unread(format!("the workspace member glob {member:?}")));
    };
    // Candidates: a directory named like each path package. One that does not
    // exist, or holds another package, is not a member this lockfile has.
    let names: BTreeSet<&str> = wanted.iter().map(|i| i.name.as_str()).collect();
    for name in names {
        let dir = join(&ws.dir, &format!("{prefix}/{name}"))?;
        if excluded.contains(&dir) || loaded.contains_key(&dir) {
            continue;
        }
        let path = manifest_path(&dir)?;
        match cx.manifests.merged(&path) {
            ManifestResult::Merged(bytes) => {
                let text =
                    String::from_utf8(bytes).map_err(|_| Unread(format!("{path} is not UTF-8")))?;
                let m = parse(&text, &dir, Some(ws))?;
                if m.name.as_deref() == Some(name) {
                    loaded.insert(dir, m);
                }
            }
            ManifestResult::Conflict => {
                return Err(Unread(format!("the text merge of {path} conflicts")));
            }
            ManifestResult::Conflicted(text) => {
                let text = union::resolve(&text)
                    .map_err(|why| Unread(format!("the text merge of {path} conflicts: {why}")))?;
                let m = parse(&text, &dir, Some(ws))?;
                if m.name.as_deref() == Some(name) {
                    kept.borrow_mut().push(path.clone());
                    loaded.insert(dir, m);
                }
            }
            ManifestResult::Unavailable(_) => {}
        }
    }
    Ok(())
}

/// ADR 0018: the manifest rules run only if the merged `[patch]` equals the
/// one in ours' root manifest and in theirs', so both lockfiles were resolved
/// under the patches the merge keeps.
pub fn check_patch(found: &Found, cx: &Context<'_>) -> Result<(), Unread> {
    let root_path = manifest_path(&found.root_dir)?;
    for role in [Role::Ours, Role::Theirs] {
        let bytes = cx
            .manifests
            .side(role, &root_path)
            .map_err(|why| Unread(format!("{root_path}: {}", why.0)))?;
        let text =
            String::from_utf8(bytes).map_err(|_| Unread(format!("{root_path} is not UTF-8")))?;
        let own = parse(&text, &found.root_dir, None)?;
        if own.patch != found.root.patch {
            return Err(Unread(format!(
                "the [patch] of the merged manifest differs from the one in {}'s",
                match role {
                    Role::Ours => "ours",
                    _ => "theirs",
                }
            )));
        }
    }
    Ok(())
}

/// One input's own manifests, at the paths [`find`] found.
#[derive(Debug)]
pub struct Side {
    /// By manifest path, root included.
    pub manifests: BTreeMap<String, Manifest>,
}

/// Reads the manifests of `role` at the paths of `found`.
pub fn read_side(found: &Found, role: Role, cx: &Context<'_>) -> Result<Side, Unread> {
    let get = |path: &str| -> Result<String, Unread> {
        match cx.manifests.side(role, path) {
            Ok(bytes) => {
                String::from_utf8(bytes).map_err(|_| Unread(format!("{path} is not UTF-8")))
            }
            Err(why) => Err(Unread(format!("{path}: {}", why.0))),
        }
    };
    let root_path = manifest_path(&found.root_dir)?;
    let root = parse(&get(&root_path)?, &found.root_dir, None)?;
    let ws = root.workspace.clone();
    let mut manifests = BTreeMap::new();
    manifests.insert(root_path.clone(), root);
    for (path, _) in found.packages.values() {
        if *path == root_path {
            continue;
        }
        let m = parse(&get(path)?, dir_of(path), ws.as_ref())?;
        manifests.insert(path.clone(), m);
    }
    Ok(Side { manifests })
}

/// Why the merged manifests do not request what `side` was resolved with.
pub type Mismatch = String;

/// Whether every request for features in the merged manifests is the one the
/// side's lockfile was resolved with (rules M1 e and C): the `[features]`
/// tables, the workspace's `members` and `exclude`, the resolver and edition, and every dependency declaration without
/// its version requirement are equal, for the root manifest and every path
/// package. Two differences are allowed, because they come from the other
/// branch alone and cannot change what the side's lockfile was resolved with:
///
/// - a declaration the side lacks that selects nothing (no `features`, default
///   features, not optional), is a registry dependency, and names a crate that
///   the side's lockfile does not contain: it can only add a package nothing
///   else of the side's lockfile depends on;
/// - a declaration the side has, equal to base's, that the merged manifest
///   lacks: the other branch removed it, which only withdraws a request.
///
/// `side_names` are the package names of the side's lockfile.
pub fn covers(
    found: &Found,
    side: &Side,
    base: &Side,
    side_names: &BTreeSet<&str>,
) -> Result<(), Mismatch> {
    covers_with(found, side, base, side_names, None)
}

/// [`covers`] for a merge that combines both branches' changes to a path
/// package (M2). The combination is compared with each branch's manifests in
/// turn: no feature table, membership, resolver, edition or feature selection
/// may differ, so no feature forwarded by one branch can reach what the other
/// branch resolved. The other branch's added plain registry declaration (no
/// features, default features, not optional) of a crate the branch's lockfile
/// already holds can unify default features; it is accepted only if
/// `identical(name)` says the crate's entries (id, checksum and resolved
/// dependency edges) are the same in both lockfiles, because the branch that
/// added the declaration was resolved with it in force (ADR 0016).
pub fn covers_combined(
    found: &Found,
    side: &Side,
    base: &Side,
    side_names: &BTreeSet<&str>,
    identical: &dyn Fn(&str) -> bool,
) -> Result<(), Mismatch> {
    covers_with(found, side, base, side_names, Some(identical))
}

/// Marks a refusal whose cause is a plain declaration of a held crate whose
/// entry differs between the lockfiles.
pub const ENTRY_DIFFERS: &str = "its entry differs between the branches' lockfiles";

fn covers_with(
    found: &Found,
    side: &Side,
    base: &Side,
    side_names: &BTreeSet<&str>,
    identical: Option<&dyn Fn(&str) -> bool>,
) -> Result<(), Mismatch> {
    let root_path = manifest_path(&found.root_dir).map_err(|e| e.0)?;
    let mut paths: BTreeMap<&str, &Manifest> = BTreeMap::new();
    paths.insert(root_path.as_str(), &found.root);
    for (path, m) in found.packages.values() {
        paths.insert(path.as_str(), m);
    }
    for (path, merged) in paths {
        let (Some(own), Some(first)) = (side.manifests.get(path), base.manifests.get(path)) else {
            return Err(format!("{path} is not in the side's manifests"));
        };
        let (a, b, c) = (merged.selection(), own.selection(), first.selection());
        same_selection(path, &a, &b, &c, side_names, identical)?;
    }
    Ok(())
}

fn same_selection(
    path: &str,
    merged: &Selection,
    side: &Selection,
    base: &Selection,
    side_names: &BTreeSet<&str>,
    identical: Option<&dyn Fn(&str) -> bool>,
) -> Result<(), Mismatch> {
    if merged.features != side.features {
        return Err(format!("{path}: the [features] table differs"));
    }
    if merged.members != side.members {
        return Err(format!("{path}: the workspace members or exclude differ"));
    }
    if merged.resolver != side.resolver || merged.edition != side.edition {
        return Err(format!("{path}: the resolver or edition differs"));
    }
    let key = |k: &DepKey| format!("{} in {path}", k.2);
    for (k, decl) in &merged.decls {
        match side.decls.get(k) {
            Some(d) if d == decl => {}
            Some(_) => return Err(format!("the declaration of {} differs", key(k))),
            None => {
                let plain = decl.is_plain() && decl.source == Source::Registry;
                let name = decl.package.as_str();
                if !plain {
                    return Err(format!("the declaration of {} is new and selects", key(k)));
                }
                if side_names.contains(name) {
                    match identical {
                        Some(same) if same(name) => {}
                        Some(_) => {
                            return Err(format!(
                                "the declaration of {} is new, {name} is in this branch's lockfile and {ENTRY_DIFFERS}",
                                key(k)
                            ));
                        }
                        None => {
                            return Err(format!(
                                "the declaration of {} is new and selects",
                                key(k)
                            ));
                        }
                    }
                }
            }
        }
    }
    for (k, decl) in &side.decls {
        if !merged.decls.contains_key(k) && base.decls.get(k) != Some(decl) {
            return Err(format!("the declaration of {} is gone", key(k)));
        }
    }
    Ok(())
}
