//! The verdicts on conflicts the engine's base rules do not settle (M1, M2 and
//! the first half of M3) and the removals after the per-key pass (M1's lower
//! version, M3), `docs/formats/cargo-lock.md` section 4.2.

use crate::codes::{self, M1, M2, M3};
use crate::manifest::{Decl, Source, dir_of};
use crate::model::{Id, Package};
use crate::shared::{Shared, admits, names, targets};
use lockmerge_core::{Conflict, Context, Draft, Refusal, Removal, Role, Verdict};
use std::collections::{BTreeMap, BTreeSet};

const CRATES_IO: [&str; 2] = [
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
];

/// A conflict the base rules did not settle.
pub fn decide(sh: &Shared, cx: &Context<'_>, c: &Conflict<'_, Id, Package>) -> Verdict<Package> {
    let id = c.key;
    match (c.base, c.ours, c.theirs) {
        (Some(_), Some(changed), None) | (Some(_), None, Some(changed)) => {
            removed_and_changed(sh, cx, id, changed)
        }
        (Some(b), Some(o), Some(t)) => both_changed(sh, cx, id, b, o, t),
        _ => Verdict::NotApplicable,
    }
}

/// M3, first half: a non-path package one side removed and the other changed
/// is kept for now and pruned afterwards if nothing points at it any more. It
/// must be reachable in every input it appears in, so that a package that was
/// already orphaned is never dropped by it.
fn removed_and_changed(
    sh: &Shared,
    cx: &Context<'_>,
    id: &Id,
    changed: &Package,
) -> Verdict<Package> {
    let refuse = |why: &str| {
        Verdict::Refuse(Refusal::new(
            codes::R5,
            vec![id.to_string()],
            vec![("detail", why.to_string())],
        ))
    };
    if id.is_path() {
        return refuse("a path package cannot be dropped");
    }
    for role in [Role::Base, Role::Ours, Role::Theirs] {
        if sh.model(role).packages.contains_key(id) && !sh.reachable(role).contains(id) {
            return refuse("it is not reachable from a path package in every input");
        }
    }
    sh.pending.borrow_mut().insert(id.clone());
    Verdict::Accept(changed.clone(), cx.proof(M3, Vec::new()))
}

fn both_changed(
    sh: &Shared,
    cx: &Context<'_>,
    id: &Id,
    base: &Package,
    ours: &Package,
    theirs: &Package,
) -> Verdict<Package> {
    // M1: the two sides differ only in edges into a crate both bumped.
    let (no, nt) = (sh.normalized(&ours.edges), sh.normalized(&theirs.edges));
    if ours.checksum == theirs.checksum && no == nt {
        return Verdict::Accept(
            Package {
                checksum: ours.checksum.clone(),
                edges: no,
            },
            cx.proof(M1, vec![("package", id.to_string())]),
        );
    }
    if !id.is_path() {
        return Verdict::NotApplicable;
    }
    match m2(sh, cx, id, base, ours, theirs) {
        Ok(p) => Verdict::Accept(p, cx.proof(M2, vec![("package", id.to_string())])),
        Err(why) => Verdict::Refuse(Refusal::new(
            codes::R6,
            vec![id.to_string()],
            vec![("detail", why)],
        )),
    }
}

/// M2: both sides changed a path package's edges. The merged edge set is the
/// base's plus what each side added minus what each side removed, and must
/// agree with the merged manifest in names, requirements and paths.
fn m2(
    sh: &Shared,
    cx: &Context<'_>,
    id: &Id,
    base: &Package,
    ours: &Package,
    theirs: &Package,
) -> Result<Package, String> {
    let all: BTreeSet<&str> = names(&base.edges)
        .into_iter()
        .chain(names(&ours.edges))
        .chain(names(&theirs.edges))
        .collect();
    let mut edges: BTreeSet<Id> = BTreeSet::new();
    for name in all {
        let (b, o, t) = (
            targets(&base.edges, name),
            targets(&ours.edges, name),
            targets(&theirs.edges, name),
        );
        let chosen: BTreeSet<&Id> = if o == t || t == b {
            o
        } else if o == b {
            t
        } else {
            // Both changed this dependency's target. Only a crate both sides
            // bumped (M1) has a proven answer.
            let (no, nt) = (sh.normalized(&to_set(&o)), sh.normalized(&to_set(&t)));
            if no != nt {
                return Err(format!("both branches changed the target of {name}"));
            }
            edges.extend(no);
            continue;
        };
        edges.extend(chosen.into_iter().cloned());
    }
    // No normal dependency cycle: Cargo rejects them, and the lockfile does
    // not say which edges are dev-dependencies. A cycle through `id` in the
    // graph of both branches' edges, with `id`'s edges as merged, is not
    // proven valid, so it refuses (also when an input has such a cycle).
    if let Some(via) = cycle_through(sh, id, &edges) {
        return Err(format!(
            "the combined edges make a dependency cycle through {via}"
        ));
    }
    // One resolved version per name.
    let edge_names = names(&edges);
    if edge_names.len() != edges.len() {
        return Err("a dependency resolves to several versions".into());
    }

    let found = sh.found(cx).map_err(|f| f.text().to_string())?;
    let Some((_, manifest)) = found.packages.get(id) else {
        return Err(format!("no manifest for {id}"));
    };
    if manifest.workspace.is_some() && manifest.name.is_none() {
        return Err("the package is a virtual workspace".into());
    }
    // b and c: the declared crate names are the names of the edges.
    let mut declared: BTreeMap<&str, &Decl> = BTreeMap::new();
    for d in manifest.deps.values() {
        match declared.get(d.package.as_str()) {
            None => {
                declared.insert(d.package.as_str(), d);
            }
            Some(first) if first.req == d.req && first.source == d.source => {}
            Some(_) => {
                return Err(format!(
                    "{} is declared with different requirements",
                    d.package
                ));
            }
        }
    }
    if declared.keys().copied().collect::<BTreeSet<_>>() != edge_names {
        return Err("the manifest's dependencies are not the merged edges' names".into());
    }
    // d: every edge matches its declaration.
    for t in &edges {
        let d = declared[t.name.as_str()];
        match (&t.source, &d.source) {
            (None, Source::Path(dir)) => {
                let Some((path, _)) = found.packages.get(t) else {
                    return Err(format!("no manifest for {t}"));
                };
                if dir_of(path) != dir {
                    return Err(format!("{t} is not at the declared path {dir}"));
                }
                if let Some(req) = &d.req
                    && !admits(req, &t.version)
                {
                    return Err(format!("{t} does not satisfy {req}"));
                }
            }
            (Some(src), Source::Git(_)) if src.starts_with("git+") => {
                if let Some(req) = &d.req
                    && !admits(req, &t.version)
                {
                    return Err(format!("{t} does not satisfy {req}"));
                }
                let same = |p: &Package| targets(&p.edges, &t.name) == BTreeSet::from([t]);
                if !(same(base) && same(ours) && same(theirs)) {
                    return Err(format!(
                        "the git dependency {} is not the same in all inputs",
                        t.name
                    ));
                }
            }
            (Some(src), Source::Registry) if CRATES_IO.contains(&src.as_str()) => match &d.req {
                Some(req) if admits(req, &t.version) => {}
                _ => return Err(format!("{t} does not satisfy the declared requirement")),
            },
            _ => {
                return Err(format!(
                    "{t} is not the kind of dependency the manifest declares"
                ));
            }
        }
    }
    // Features: the combination must be covered by each branch's resolution.
    for role in [Role::Ours, Role::Theirs] {
        sh.covers_combined(role, cx).map_err(|f| {
            format!(
                "the combined manifests are not proven to select what {} was resolved with: {}",
                crate::parse::role_name(role),
                f.text()
            )
        })?;
    }
    Ok(Package {
        checksum: ours.checksum.clone(),
        edges,
    })
}

/// The first package of the graph of ours' and theirs' edges, with `id`'s
/// edges replaced by `edges`, that is on a cycle through `id`.
fn cycle_through(sh: &Shared, id: &Id, edges: &BTreeSet<Id>) -> Option<Id> {
    let mut seen: BTreeSet<&Id> = BTreeSet::new();
    let mut stack: Vec<&Id> = edges.iter().collect();
    while let Some(next) = stack.pop() {
        if next == id {
            return Some(id.clone());
        }
        if !seen.insert(next) {
            continue;
        }
        for m in [&sh.models[1], &sh.models[2]] {
            if let Some(p) = m.packages.get(next) {
                stack.extend(p.edges.iter());
            }
        }
    }
    None
}

fn to_set(ids: &BTreeSet<&Id>) -> BTreeSet<Id> {
    ids.iter().map(|i| (*i).clone()).collect()
}

/// Removals after the per-key pass: the lower version of each crate both sides
/// bumped (M1), then M3: packages removed on one side and changed on the other
/// that nothing points at any more, and the dependencies they leave unused.
pub fn prune(sh: &Shared, draft: &Draft<Id, Package>, cx: &Context<'_>) -> Vec<Removal<Id>> {
    let mut out: Vec<Removal<Id>> = Vec::new();
    let mut removed: BTreeSet<Id> = BTreeSet::new();
    for g in sh.groups.borrow().iter() {
        if draft.contains_key(&g.low) {
            removed.insert(g.low.clone());
            out.push(Removal {
                key: g.low.clone(),
                proof: cx.proof(M1, vec![("kept", g.high.to_string())]),
            });
        }
    }
    let pending = sh.pending.borrow();
    loop {
        let mut incoming: BTreeSet<&Id> = BTreeSet::new();
        let mut lost: BTreeSet<&Id> = BTreeSet::new();
        for (id, p) in draft.iter() {
            if removed.contains(id) {
                lost.extend(p.edges.iter());
            } else {
                incoming.extend(p.edges.iter());
            }
        }
        let next: Vec<Id> = draft
            .iter()
            .map(|(id, _)| id)
            .filter(|id| !removed.contains(*id) && !id.is_path() && !incoming.contains(*id))
            .filter(|id| {
                // A pending package nothing points at, or a package the removals
                // left unused that every input reached.
                pending.contains(*id)
                    || (lost.contains(*id)
                        && [Role::Base, Role::Ours, Role::Theirs].iter().all(|r| {
                            !sh.model(*r).packages.contains_key(*id)
                                || sh.reachable(*r).contains(*id)
                        }))
            })
            .cloned()
            .collect();
        if next.is_empty() {
            return out;
        }
        for id in next {
            out.push(Removal {
                key: id.clone(),
                proof: cx.proof(M3, vec![("package", id.to_string())]),
            });
            removed.insert(id);
        }
    }
}
