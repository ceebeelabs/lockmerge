//! Envelope checks, the verdict on conflicts the engine's base rules do not
//! settle, and validation of the merged set.

use crate::codes::{self, E1, V2, V3, V4, V5, V6};
use crate::index::{self, IndexLookup};
use crate::model::{Id, Lock, Package, compat_class};
use crate::moves;
use crate::parse::role_name;
use crate::shared::{ROLES, Shared};
use lockmerge_core::{Context, Draft, Findings, IndexCache, Proof, Refusal, Role};
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
    different_additions(&models, sh, cx)?;
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

/// R15's trigger: ours added an id that theirs lacks and base lacks, and
/// theirs added a different one. Two crates that were never resolved together
/// may declare the same native `links` library. A git or path package among
/// them refuses here: `links` of a git source cannot be read, and a path
/// package that a side added is not covered by the proof. Registry crates
/// only set `links_proof`, and `validate` proves their `links` over the merged
/// set (V6). Ids both sides added are not affected, and neither is the lower
/// version of a crate both sides bumped (M1), which the merge drops and so
/// never puts beside anything.
fn different_additions(models: &[&Lock; 3], sh: &Shared, cx: &Context<'_>) -> Result<(), Refusal> {
    let dropped: Vec<Id> = sh.groups.borrow().iter().map(|g| g.low.clone()).collect();
    let only = |a: &Lock, b: &Lock, base: &Lock| -> Vec<Id> {
        a.packages
            .keys()
            .filter(|id| {
                !b.packages.contains_key(id)
                    && !base.packages.contains_key(id)
                    && !dropped.contains(id)
            })
            .cloned()
            .collect()
    };
    let ours = only(models[1], models[2], models[0]);
    let theirs = only(models[2], models[1], models[0]);
    if ours.is_empty() || theirs.is_empty() {
        return Ok(());
    }
    let show = |ids: &[Id]| join(ids);
    let mut facts = vec![("ours", show(&ours)), ("theirs", show(&theirs))];
    if let Some(id) = ours.iter().chain(&theirs).find(|id| !id.is_registry()) {
        facts.push((
            "detail",
            format!("{id} is not a registry package, so its `links` cannot be read from the index"),
        ));
        return Err(Refusal::new(
            codes::R15,
            ours.iter()
                .chain(&theirs)
                .map(|id| id.to_string())
                .collect(),
            facts,
        ));
    }
    // The entries of the added ids come first, as decision 15 says, so that a
    // missing cache refuses before the later steps. The full proof over the
    // merged set is V6.
    let fail = |id: &Id, why: String| {
        Refusal::new(
            codes::R15,
            vec![id.to_string()],
            vec![
                ("ours", show(&ours)),
                ("theirs", show(&theirs)),
                ("detail", why),
            ],
        )
    };
    let Some(cache) = cx.index else {
        let id = &ours[0];
        return Err(fail(id, "the registry index cache is not available".into()));
    };
    for (id, side) in ours
        .iter()
        .map(|i| (i, 1))
        .chain(theirs.iter().map(|i| (i, 2)))
    {
        let sum = models[side].packages[id].checksum.as_deref();
        registry_links(id, sum, cache).map_err(|why| fail(id, why))?;
    }
    sh.links_proof.set(true);
    Ok(())
}

/// The `links` of a registry package from the index cache: `Ok(None)` if its
/// entry matches name, version and checksum and has none.
fn registry_links(
    id: &Id,
    checksum: Option<&str>,
    cache: &dyn IndexCache,
) -> Result<Option<String>, String> {
    let Some(sum) = checksum else {
        return Err("it has no checksum to match an index entry against".into());
    };
    let Some(path) = index::entry_path(&id.name) else {
        return Err("the cache file of its name is not pinned".into());
    };
    match index::lookup(&cache.files(&path), &id.name, &id.version.to_string(), sum) {
        IndexLookup::Found { links } => Ok(links),
        IndexLookup::Unavailable(why) => Err(why),
    }
}

/// V6: the native-links proof over the merged set, from the registry index
/// cache and the merged manifests of the path packages. Every package must
/// give its `links` (or the absence of one) and no value may repeat. A git
/// source, a registry package without a matching index entry and a path
/// package whose manifest is not read all refuse, and so does an absent cache.
fn native_links(
    items: &[(&Id, &Package)],
    sh: &Shared,
    cx: &Context<'_>,
) -> Result<Vec<(&'static str, String)>, Refusal> {
    let all = || {
        items
            .iter()
            .map(|(id, _)| id.to_string())
            .collect::<Vec<_>>()
    };
    let fail = |subjects: Vec<String>, detail: String| {
        Refusal::new(codes::R15, subjects, vec![("detail", detail)])
    };
    let Some(cache) = cx.index else {
        return Err(fail(
            all(),
            "the registry index cache is not available".into(),
        ));
    };
    let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut registry = 0;
    for (id, pkg) in items {
        let links = if id.is_registry() {
            registry += 1;
            registry_links(id, pkg.checksum.as_deref(), cache)
                .map_err(|why| fail(vec![id.to_string()], why))?
        } else if id.is_path() {
            let manifest = sh
                .found(cx)
                .map_err(|f| fail(vec![id.to_string()], f.text().to_string()))?
                .packages
                .get(*id)
                .map(|(_, m)| m);
            match manifest {
                Some(m) => m.links.clone(),
                None => {
                    return Err(fail(
                        vec![id.to_string()],
                        "its manifest was not found".into(),
                    ));
                }
            }
        } else {
            return Err(fail(
                vec![id.to_string()],
                "a git source: its `links` cannot be read".into(),
            ));
        };
        if let Some(l) = links {
            owners.entry(l).or_default().push(id.to_string());
        }
    }
    if let Some((value, ids)) = owners.iter().find(|(_, ids)| ids.len() > 1) {
        return Err(fail(
            ids.clone(),
            format!("they share the native library `{value}`"),
        ));
    }
    Ok(vec![
        ("registry packages", registry.to_string()),
        ("links", join(owners.keys())),
    ])
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

    // M4: a manifest the rules read had a text conflict read as two insertions.
    if !sh.kept_both().is_empty() {
        f.proofs.push(cx.proof(codes::M4, Vec::new()));
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

    // V6: the native links of the merged set, when each side added different
    // registry crates.
    if sh.links_proof.get() {
        match native_links(&items, sh, cx) {
            Ok(facts) => f.proofs.push(cx.proof(V6, facts)),
            Err(r) => f.refusals.push(r),
        }
    }

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
