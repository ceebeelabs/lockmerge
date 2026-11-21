//! What the sides changed in packages that exist in base, before the per-key
//! pass: the both-sides-bumped crates M1 may keep, and the one-sided version
//! moves and dependency changes that rules D, F, B1 and C must prove
//! (`docs/formats/cargo-lock.md` section 4.2).

use crate::codes::{self, B1, C, D, F};
use crate::manifest::Source;
use crate::model::{Id, compat_class};
use crate::parse::role_name;
use crate::rules::{CrateKey, crate_label, crate_source, join, refusal, versions_by_crate};
use crate::shared::{Fail, Group, ROLES, Shared, admits, names, path_dependents, targets};
use lockmerge_core::{Context, Proof, ReasonCode, Refusal, Role};
use semver::Version;
use std::collections::{BTreeMap, BTreeSet};

/// A refusal for a manifest rule that failed: R14 when the manifests could not
/// be read, `code` when they were read and show the rule does not hold.
pub(crate) fn from_fail(
    code: ReasonCode,
    subject: String,
    fail: &Fail,
    mut facts: Vec<(&'static str, String)>,
) -> Refusal {
    let code = match fail {
        Fail::Manifests(_) => codes::R14,
        Fail::Rule(_) => code,
    };
    facts.push(("detail", fail.text().to_string()));
    Refusal::new(code, vec![subject], facts)
}

fn ids_by_crate(m: &crate::model::Lock) -> BTreeMap<CrateKey<'_>, BTreeSet<&Id>> {
    let mut out: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for id in m.packages.keys() {
        out.entry((id.name.as_str(), crate_source(id)))
            .or_default()
            .insert(id);
    }
    out
}

/// A path package's merged manifest declares `name` only with requirements
/// that `version` satisfies.
pub fn requirement(
    sh: &Shared,
    cx: &Context<'_>,
    package: &Id,
    name: &str,
    version: &Version,
) -> Result<(), Fail> {
    let found = sh.found(cx)?;
    let Some((_, manifest)) = found.packages.get(package) else {
        return Err(Fail::Manifests(format!("no manifest for {package}")));
    };
    let decls: Vec<_> = manifest.decls_of(name).collect();
    if decls.is_empty() {
        return Err(Fail::Manifests(format!(
            "the manifest of {package} does not declare {name}"
        )));
    }
    for d in decls {
        if d.source != Source::Registry {
            return Err(Fail::Manifests(format!(
                "{name} is not a registry dependency of {package}"
            )));
        }
        match &d.req {
            Some(req) if admits(req, version) => {}
            Some(req) => {
                return Err(Fail::Rule(format!(
                    "{package} requires {name} {req}, which {version} does not satisfy"
                )));
            }
            None => return Err(Fail::Manifests(format!("{name} has no requirement"))),
        }
    }
    Ok(())
}

/// R3 or a group for every crate both sides bumped to different versions.
pub fn groups(sh: &Shared, cx: &Context<'_>) -> Result<(), Refusal> {
    let [base, ours, theirs] = &sh.models;
    let (vb, vo, vt) = (
        versions_by_crate(base),
        versions_by_crate(ours),
        versions_by_crate(theirs),
    );
    for (key, before) in &vb {
        let (Some(o), Some(t)) = (vo.get(key), vt.get(key)) else {
            continue;
        };
        let removed_o: BTreeSet<_> = before.difference(o).collect();
        let removed_t: BTreeSet<_> = before.difference(t).collect();
        let added_o: BTreeSet<_> = o.difference(before).collect();
        let added_t: BTreeSet<_> = t.difference(before).collect();
        if removed_o.is_disjoint(&removed_t)
            || added_o.is_empty()
            || added_t.is_empty()
            || added_o == added_t
        {
            continue;
        }
        match group_for(sh, cx, key) {
            Ok(g) => sh.groups.borrow_mut().push(g),
            Err(fail) => {
                return Err(refusal(
                    codes::R3,
                    crate_label(key),
                    vec![
                        ("ours", join(&added_o)),
                        ("theirs", join(&added_t)),
                        ("detail", fail.text().to_string()),
                    ],
                ));
            }
        }
    }
    Ok(())
}

fn crate_ids<'a>(m: &'a crate::model::Lock, key: &CrateKey<'_>) -> Vec<&'a Id> {
    m.packages
        .keys()
        .filter(|id| id.name == key.0 && crate_source(id) == key.1)
        .collect()
}

/// M1: both sides bumped the registry crate `key` to different versions of one
/// compatibility class. Conditions a, b and e and the dependency carry F.
fn group_for(sh: &Shared, cx: &Context<'_>, key: &CrateKey<'_>) -> Result<Group, Fail> {
    let rule = |s: &str| Fail::Rule(s.to_string());
    let (b, o, t) = (
        crate_ids(&sh.models[0], key),
        crate_ids(&sh.models[1], key),
        crate_ids(&sh.models[2], key),
    );
    let ([b], [o], [t]) = (b.as_slice(), o.as_slice(), t.as_slice()) else {
        return Err(rule("a side has several versions of the crate"));
    };
    let (b, o, t) = (*b, *o, *t);
    if !(b.is_registry() && o.is_registry() && t.is_registry()) {
        return Err(rule("not a registry crate"));
    }
    let class = compat_class(&b.version);
    if compat_class(&o.version) != class || compat_class(&t.version) != class {
        return Err(rule("the versions are in different compatibility classes"));
    }
    if [b, o, t].iter().any(|i| !i.version.pre.is_empty()) {
        return Err(rule("a version is a prerelease"));
    }
    let (high, low, high_role) = if o.version > t.version {
        (o, t, Role::Ours)
    } else {
        (t, o, Role::Theirs)
    };
    if high.version <= b.version {
        return Err(rule("the higher version is not above the base version"));
    }
    let kids: BTreeSet<&Id> = [b, o, t].into_iter().collect();

    // a: every path package with an edge into the crate declares a
    // requirement the higher version satisfies.
    for p in path_dependents(&sh.models, &kids) {
        requirement(sh, cx, p, &high.name, &high.version)?;
    }

    // b: every registry or git package with an edge into the crate is in the
    // higher side with the same checksum and its edge there at the higher id.
    let high_model = sh.model(high_role);
    for role in [Role::Ours, Role::Theirs] {
        for (x, xp) in &sh.model(role).packages {
            if x.is_path() || !xp.edges.iter().any(|e| kids.contains(e)) {
                continue;
            }
            let Some(hp) = high_model.packages.get(x) else {
                return Err(Fail::Rule(format!("{x} is not in the side with {high}")));
            };
            let into: BTreeSet<&Id> = hp.edges.iter().filter(|e| kids.contains(*e)).collect();
            if hp.checksum != xp.checksum || into != BTreeSet::from([high]) {
                return Err(Fail::Rule(format!("{x} does not depend on {high} there")));
            }
        }
    }
    // The dependents the two sides share differ only in their edges into the
    // crate, so taking the higher side's loses nothing.
    let rest = |edges: &BTreeSet<Id>| -> BTreeSet<Id> {
        edges
            .iter()
            .filter(|e| !kids.contains(*e))
            .cloned()
            .collect()
    };
    for (x, xo) in &sh.models[1].packages {
        if let Some(xt) = sh.models[2].packages.get(x)
            && !x.is_path()
            && rest(&xo.edges) != rest(&xt.edges)
        {
            return Err(Fail::Rule(format!(
                "{x} differs in more than {}",
                high.name
            )));
        }
    }

    // F: the higher entry keeps every dependency name any entry has.
    let mut wanted: BTreeSet<&str> = BTreeSet::new();
    for (m, id) in [(0, b), (1, o), (2, t)] {
        wanted.extend(names(&sh.models[m].packages[id].edges));
    }
    let have = names(&high_model.packages[high].edges);
    if let Some(missing) = wanted.difference(&have).next() {
        return Err(Fail::Rule(format!(
            "{high} lacks the dependency {missing} that another version has"
        )));
    }

    // e: the merged manifests request the features the higher side was
    // resolved with.
    sh.covers(high_role, cx)?;
    Ok(Group {
        low: low.clone(),
        high: high.clone(),
    })
}

fn is_group(sh: &Shared, key: &CrateKey<'_>) -> bool {
    sh.groups
        .borrow()
        .iter()
        .any(|g| g.high.name == key.0 && crate_source(&g.high) == key.1)
}

/// Checks every change a side made to a package of base that no other rule
/// settles. Version moves need D, F, B1 and C. A dependency a package gained
/// or, for a path package, lost needs C. A registry or git package that lost a
/// dependency fails F. Anything else that moved is refused (R14).
pub fn moves(sh: &Shared, cx: &Context<'_>, proofs: &mut Vec<Proof>) -> Result<(), Refusal> {
    // Whatever refuses without reading a manifest refuses first, on either
    // branch, so that the reason does not depend on which branch is ours.
    check(sh, cx, &mut Vec::new(), false)?;
    check(sh, cx, proofs, true)
}

/// One pass over both branches. Without `manifests` it skips the steps that
/// read them.
fn check(
    sh: &Shared,
    cx: &Context<'_>,
    proofs: &mut Vec<Proof>,
    manifests: bool,
) -> Result<(), Refusal> {
    let base = &sh.models[0];
    let vb = ids_by_crate(base);
    for i in [1usize, 2] {
        let role = ROLES[i];
        let (side, other) = (&sh.models[i], &sh.models[3 - i]);
        let (vs, vj) = (ids_by_crate(side), ids_by_crate(other));
        let mut explained: Vec<(&Id, &Id)> = Vec::new();
        for (key, before) in &vb {
            let Some(after) = vs.get(key) else { continue };
            let removed: Vec<&Id> = before.difference(after).copied().collect();
            let added: Vec<&Id> = after.difference(before).copied().collect();
            if removed.is_empty() || added.is_empty() || is_group(sh, key) {
                continue;
            }
            let other_after = vj.get(key);
            if other_after != Some(before) {
                if other_after == Some(after) {
                    continue;
                }
                return Err(refusal(
                    codes::R14,
                    crate_label(key),
                    vec![
                        ("branch", role_name(role).to_string()),
                        ("before", join(before.iter().map(|i| &i.version))),
                        ("after", join(after.iter().map(|i| &i.version))),
                    ],
                ));
            }
            one_sided_move(sh, cx, proofs, manifests, role, &removed, &added)?;
            explained.push((removed[0], added[0]));
        }
        dependency_changes(sh, cx, proofs, manifests, role, &explained)?;
    }
    Ok(())
}

/// A registry crate moved from `removed` to `added` on `role`'s side while the
/// other side left the crate alone.
fn one_sided_move(
    sh: &Shared,
    cx: &Context<'_>,
    proofs: &mut Vec<Proof>,
    manifests: bool,
    role: Role,
    removed: &[&Id],
    added: &[&Id],
) -> Result<(), Refusal> {
    let side_name = role_name(role);
    let facts = |b: &Id, n: &Id| {
        vec![
            ("branch", side_name.to_string()),
            ("from", b.version.to_string()),
            ("to", n.version.to_string()),
        ]
    };
    let ([b], [n]) = (removed, added) else {
        return Err(refusal(
            codes::R14,
            removed[0].name.clone(),
            vec![
                ("branch", side_name.to_string()),
                ("detail", "several versions of the crate changed".into()),
            ],
        ));
    };
    let (b, n) = (*b, *n);
    if !(b.is_registry() && n.is_registry()) {
        return Err(refusal(
            codes::R14,
            b.name.clone(),
            vec![
                ("branch", side_name.to_string()),
                (
                    "detail",
                    "only moves of registry crates are read; the order of path or git versions is not known".into(),
                ),
            ],
        ));
    }
    let subject = b.name.clone();

    // D: no downgrade.
    if n.version < b.version {
        return Err(Refusal::new(codes::DOWNGRADE, vec![subject], facts(b, n)));
    }
    if manifests {
        proofs.push(cx.proof(D, facts(b, n)));
    }

    // F: the new entry keeps every dependency name of the old one. The other
    // branch left the old version in place, and must not have changed what it
    // depends on: that change (a feature enabled there) would be lost.
    let other_entry = sh.models[if role == Role::Ours { 2 } else { 1 }]
        .packages
        .get(b);
    if other_entry != sh.models[0].packages.get(b) {
        let mut f = facts(b, n);
        f.push((
            "detail",
            format!("the other branch changed the dependencies of {b}"),
        ));
        return Err(Refusal::new(codes::FEATURES, vec![subject], f));
    }
    let have = names(&sh.model(role).packages[n].edges);
    let wanted = names(&sh.models[0].packages[b].edges);
    if let Some(missing) = wanted.difference(&have).next() {
        let mut f = facts(b, n);
        f.push(("detail", format!("{n} lacks the dependency {missing}")));
        return Err(Refusal::new(codes::FEATURES, vec![subject], f));
    }
    if manifests {
        proofs.push(cx.proof(F, facts(b, n)));
    }
    if !manifests {
        return Ok(());
    }

    // B1: every path package with an edge into the crate still declares a
    // requirement the new version satisfies.
    let ids: BTreeSet<&Id> = [b, n].into_iter().collect();
    for p in path_dependents(&sh.models, &ids) {
        requirement(sh, cx, p, &n.name, &n.version)
            .map_err(|f| from_fail(codes::REQUIREMENT, subject.clone(), &f, facts(b, n)))?;
    }
    proofs.push(cx.proof(B1, facts(b, n)));

    // C: the dependents were resolved in this side, and its features are the
    // merged ones.
    let side = sh.model(role);
    let other = &sh.models[if role == Role::Ours { 2 } else { 1 }];
    for (x, xp) in &other.packages {
        if x.is_path() || !xp.edges.iter().any(|e| ids.contains(e)) {
            continue;
        }
        if !side.packages.contains_key(x) && sh.models[0].packages.get(x) != Some(xp) {
            let mut f = facts(b, n);
            f.push(("detail", format!("{x} exists only on the other branch")));
            return Err(Refusal::new(codes::FEATURES, vec![subject], f));
        }
    }
    sh.covers(role, cx)
        .map_err(|f| from_fail(codes::FEATURES, subject.clone(), &f, facts(b, n)))?;
    proofs.push(cx.proof(C, facts(b, n)));
    Ok(())
}

/// A package of base that `role`'s side changed while the other side left it
/// alone: a dependency target that moved, a dependency name lost or gained.
fn dependency_changes(
    sh: &Shared,
    cx: &Context<'_>,
    proofs: &mut Vec<Proof>,
    manifests: bool,
    role: Role,
    explained: &[(&Id, &Id)],
) -> Result<(), Refusal> {
    let i = if role == Role::Ours { 1 } else { 2 };
    let (side, other) = (&sh.models[i], &sh.models[3 - i]);
    let mut needs_cover = None;
    for (id, before) in &sh.models[0].packages {
        let Some(after) = side.packages.get(id) else {
            continue;
        };
        if other.packages.get(id) != Some(before) || after.edges == before.edges {
            continue;
        }
        let all: BTreeSet<&str> = before
            .edges
            .iter()
            .chain(&after.edges)
            .map(|e| e.name.as_str())
            .collect();
        for name in all {
            let (b, a) = (targets(&before.edges, name), targets(&after.edges, name));
            let detail = |why: &str| {
                vec![
                    ("branch", role_name(role).to_string()),
                    ("dependency", name.to_string()),
                    ("before", join(&b)),
                    ("after", join(&a)),
                    ("detail", why.to_string()),
                ]
            };
            if !b.is_empty() && !a.is_empty() && a != b {
                // A move of one target onto another is explained when the
                // crate's own move was checked above. Any other change of
                // target (onto a version that already existed, say) is not.
                let gone: BTreeSet<&Id> = b.difference(&a).copied().collect();
                let new: BTreeSet<&Id> = a.difference(&b).copied().collect();
                let ok = explained
                    .iter()
                    .any(|(o, n)| gone == BTreeSet::from([*o]) && new == BTreeSet::from([*n]));
                if !ok {
                    return Err(Refusal::new(
                        codes::R14,
                        vec![id.to_string()],
                        detail("the target changed without a version move of the crate"),
                    ));
                }
            } else if !b.is_empty() && a.is_empty() {
                if !id.is_path() {
                    // F: features decide a registry package's dependencies, and
                    // the manifests of the other branch may still enable them.
                    return Err(Refusal::new(
                        codes::FEATURES,
                        vec![id.to_string()],
                        detail("a dependency that the unchanged branch still has is dropped"),
                    ));
                }
                needs_cover = Some(id);
            } else if b.is_empty() && !a.is_empty() {
                needs_cover = Some(id);
            }
        }
    }
    if let (Some(id), true) = (needs_cover, manifests) {
        sh.covers(role, cx).map_err(|f| {
            from_fail(
                codes::FEATURES,
                id.to_string(),
                &f,
                vec![("branch", role_name(role).to_string())],
            )
        })?;
        proofs.push(cx.proof(C, vec![("branch", role_name(role).to_string())]));
    }
    Ok(())
}
