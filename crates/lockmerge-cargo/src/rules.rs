//! Envelope checks, the verdict on conflicts the engine's base rules do not
//! settle, and validation of the merged set.

use crate::codes::{self, E1, V2, V3, V4, V5};
use crate::model::{Id, Lock, Package, compat_class};
use crate::moves;
use crate::parse::role_name;
use crate::shared::{ROLES, Shared};
use lockmerge_core::{Context, Draft, Findings, Proof, Refusal, Role};
use semver::Version;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn refusal(
    code: lockmerge_core::ReasonCode,
    subject: String,
    facts: Vec<(&'static str, String)>,
) -> Refusal {
    Refusal::new(code, vec![subject], facts)
}

/// What must agree across the three inputs. Checked in a fixed order: version
/// (R1), checksums (R2), sources (R12), prerelease changes (R11), versions both
/// sides bumped (M1, else R3), changes one side made to existing packages (D,
/// F, B1, C, else R14), crates only each side added (R15), and the sort order
/// of ties (V5).
pub fn envelope(models: [&Lock; 3], sh: &Shared, cx: &Context<'_>) -> Result<Vec<Proof>, Refusal> {
    let version = models[0].version;
    if let Some(i) = (1..3).find(|&i| models[i].version != version) {
        return Err(refusal(
            codes::R1,
            "Cargo.lock".into(),
            vec![
                ("base", format!("version {version}")),
                (
                    role_name(ROLES[i]),
                    format!("version {}", models[i].version),
                ),
            ],
        ));
    }
    checksums(&models)?;
    sources(&models)?;
    prereleases(&models)?;
    let mut proofs = vec![cx.proof(E1, vec![("version", version.to_string())])];
    moves::groups(sh, cx)?;
    moves::moves(sh, cx, &mut proofs)?;
    different_additions(&models, sh)?;
    tie_order(&models)?;
    Ok(proofs)
}

/// R2: an id that appears in several inputs has one checksum.
fn checksums(models: &[&Lock; 3]) -> Result<(), Refusal> {
    let ids: BTreeSet<&Id> = models.iter().flat_map(|m| m.packages.keys()).collect();
    for id in ids {
        let seen: Vec<(Role, &Option<String>)> = ROLES
            .iter()
            .zip(models)
            .filter_map(|(r, m)| m.packages.get(id).map(|p| (*r, &p.checksum)))
            .collect();
        if seen.windows(2).any(|w| w[0].1 != w[1].1) {
            let facts = seen
                .iter()
                .map(|(r, c)| (role_name(*r), c.as_deref().unwrap_or("none").to_string()))
                .collect();
            return Err(refusal(codes::R2, id.to_string(), facts));
        }
    }
    Ok(())
}

/// Per (name, version), the sources each input has.
fn sources_by_version(m: &Lock) -> BTreeMap<(&str, &Version), BTreeSet<&Option<String>>> {
    let mut out: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for id in m.packages.keys() {
        out.entry((id.name.as_str(), &id.version))
            .or_default()
            .insert(&id.source);
    }
    out
}

/// R12: no (name, version) has one set of sources in one input and a disjoint
/// set in another.
fn sources(models: &[&Lock; 3]) -> Result<(), Refusal> {
    let per: Vec<_> = models.iter().map(|m| sources_by_version(m)).collect();
    for (a, b) in [(0, 1), (0, 2), (1, 2)] {
        for (key, sa) in &per[a] {
            if let Some(sb) = per[b].get(key)
                && sa.is_disjoint(sb)
            {
                return Err(refusal(
                    codes::R12,
                    format!("{} {}", key.0, key.1),
                    vec![
                        (role_name(ROLES[a]), show(sa)),
                        (role_name(ROLES[b]), show(sb)),
                    ],
                ));
            }
        }
    }
    Ok(())
}

fn show(set: &BTreeSet<&Option<String>>) -> String {
    set.iter()
        .map(|s| s.as_deref().unwrap_or("path"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A source without the precise git commit (`#<commit>`), so that an update of
/// a git dependency, which changes the commit and maybe the version, is seen as
/// a move of one crate. Full sources stay in the ids.
pub(crate) fn crate_source(id: &Id) -> Option<String> {
    id.source
        .as_deref()
        .map(|s| s.split_once('#').map_or(s, |(head, _)| head).to_string())
}

pub(crate) type CrateKey<'a> = (&'a str, Option<String>);

/// The versions of each (name, source without commit) in one input.
pub(crate) fn versions_by_crate(m: &Lock) -> BTreeMap<CrateKey<'_>, BTreeSet<&Version>> {
    let mut out: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for id in m.packages.keys() {
        out.entry((id.name.as_str(), crate_source(id)))
            .or_default()
            .insert(&id.version);
    }
    out
}

pub(crate) fn crate_label(key: &CrateKey<'_>) -> String {
    match &key.1 {
        Some(s) => format!("{} ({s})", key.0),
        None => key.0.to_string(),
    }
}

/// R11: a side replaced a version of a crate by one that differs from it in
/// being a prerelease. The pairing of removed to added versions is not known,
/// so any such pair refuses.
fn prereleases(models: &[&Lock; 3]) -> Result<(), Refusal> {
    let base = versions_by_crate(models[0]);
    for i in [1, 2] {
        let side = versions_by_crate(models[i]);
        for (key, before) in &base {
            let Some(after) = side.get(key) else { continue };
            let removed = before.difference(after);
            let added: Vec<_> = after.difference(before).collect();
            for r in removed {
                if let Some(a) = added.iter().find(|a| a.pre.is_empty() != r.pre.is_empty()) {
                    return Err(refusal(
                        codes::R11,
                        crate_label(key),
                        vec![
                            ("from", r.to_string()),
                            (role_name(ROLES[i]), a.to_string()),
                        ],
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn join<T: std::fmt::Display>(items: impl IntoIterator<Item = T>) -> String {
    items
        .into_iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// R15: ours added an id that theirs lacks and base lacks, and theirs added a
/// different one. Two crates that were never resolved together may declare the
/// same native `links` library. For registry crates only the index can show
/// that, and for path crates only their manifests; this crate reads neither
/// yet, so the trigger always refuses, path packages included. Ids both sides
/// added are not affected, and neither is the lower version of a crate both
/// sides bumped (M1), which the merge drops and so never puts beside anything.
fn different_additions(models: &[&Lock; 3], sh: &Shared) -> Result<(), Refusal> {
    let dropped: Vec<Id> = sh.groups.borrow().iter().map(|g| g.low.clone()).collect();
    let only = |a: &Lock, b: &Lock, base: &Lock| -> Vec<String> {
        a.packages
            .keys()
            .filter(|id| {
                !b.packages.contains_key(id)
                    && !base.packages.contains_key(id)
                    && !dropped.contains(id)
            })
            .map(|id| id.to_string())
            .collect()
    };
    let ours = only(models[1], models[2], models[0]);
    let theirs = only(models[2], models[1], models[0]);
    if ours.is_empty() || theirs.is_empty() {
        return Ok(());
    }
    Err(Refusal::new(
        codes::R15,
        ours.iter().chain(&theirs).cloned().collect(),
        vec![("ours", ours.join(", ")), ("theirs", theirs.join(", "))],
    ))
}

/// V5, ties: Cargo sorts ids by name, version, then source, and how it orders
/// two sources of one name and version is not verified. Two such ids may be
/// written only if some input contains both, in the order this crate writes
/// (an input that is not in that order is refused at parse time).
fn tie_order(models: &[&Lock; 3]) -> Result<(), Refusal> {
    let ids: BTreeSet<&Id> = models.iter().flat_map(|m| m.packages.keys()).collect();
    let ids: Vec<&Id> = ids.into_iter().collect();
    for pair in ids.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if a.name == b.name
            && a.version == b.version
            && !models
                .iter()
                .any(|m| m.packages.contains_key(a) && m.packages.contains_key(b))
        {
            return Err(refusal(
                codes::UNWRITABLE,
                format!("{} {}", a.name, a.version),
                vec![("detail", "no input shows the order of its sources".into())],
            ));
        }
    }
    Ok(())
}

/// V2 to V5 on the merged set. Every check runs, so the report lists them all.
pub fn validate(draft: &Draft<Id, Package>, sh: &Shared, cx: &Context<'_>) -> Findings {
    let mut f = Findings::default();
    let items: Vec<(&Id, &Package)> = draft.iter().collect();

    // R5: a package removed on one side and changed on the other that M3 did
    // not prune is still there, so something still points at it.
    for id in sh.pending.borrow().iter() {
        if draft.contains_key(id) {
            f.refusals.push(refusal(
                codes::R5,
                id.to_string(),
                vec![("detail", "a package still depends on it".into())],
            ));
        }
    }

    // V2: every edge resolves to a package of the merged set.
    let dangling: Vec<Refusal> = items
        .iter()
        .flat_map(|(id, p)| {
            p.edges
                .iter()
                .filter(|e| !draft.contains_key(e))
                .map(|e| refusal(codes::R4, id.to_string(), vec![("edge", e.to_string())]))
        })
        .collect();
    if dangling.is_empty() {
        f.proofs.push(cx.proof(V2, Vec::new()));
    }
    f.refusals.extend(dangling);

    // V3: reachability from the path packages.
    let unreachable = unreachable(&items);
    if unreachable.is_empty() {
        f.proofs.push(cx.proof(V3, Vec::new()));
    }
    f.refusals.extend(
        unreachable
            .into_iter()
            .map(|id| refusal(codes::R7, id.to_string(), Vec::new())),
    );

    // V4: no two semver-compatible registry versions of one crate.
    // Name, source and compatibility class.
    type ClassKey<'a> = (&'a str, &'a str, (u64, u64, u64));
    let mut classes: BTreeMap<ClassKey<'_>, &Id> = BTreeMap::new();
    let mut clashes = Vec::new();
    for (id, _) in &items {
        if let (true, Some(source)) = (id.is_registry(), id.source.as_deref())
            && let Some(other) = classes.insert((&id.name, source, compat_class(&id.version)), id)
        {
            clashes.push(refusal(
                codes::R8,
                id.name.clone(),
                vec![
                    ("a", other.version.to_string()),
                    ("b", id.version.to_string()),
                ],
            ));
        }
    }
    if clashes.is_empty() {
        f.proofs.push(cx.proof(V4, Vec::new()));
    }
    f.refusals.extend(clashes);

    // V5: the set can be written, and every edge resolves back to its target.
    // The version number does not change whether the set can be written.
    if crate::write::render(3, &items).is_some() {
        f.proofs.push(cx.proof(V5, Vec::new()));
    } else {
        f.refusals.push(Refusal::new(
            codes::UNWRITABLE,
            vec!["Cargo.lock".into()],
            Vec::new(),
        ));
    }
    f
}

/// Non-path packages no path package reaches, through edges that exist.
fn unreachable<'a>(items: &[(&'a Id, &Package)]) -> Vec<&'a Id> {
    let by_id: BTreeMap<&Id, &Package> = items.iter().copied().collect();
    let mut seen: BTreeSet<&Id> = BTreeSet::new();
    let mut stack: Vec<&Id> = items
        .iter()
        .filter(|(id, _)| id.is_path())
        .map(|(id, _)| *id)
        .collect();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(p) = by_id.get(id) {
            stack.extend(p.edges.iter().filter(|e| by_id.contains_key(e)));
        }
    }
    items
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| !seen.contains(id))
        .collect()
}
