//! What one merge run knows across the engine's phases: the three parsed
//! lockfiles, the manifests (read at most once, and only when a rule needs
//! them), the both-sides-bumped groups the envelope proved (M1) and the
//! removals pending for the prune step (M3).

use crate::find::{Found, Side, check_patch, covers_combined, find_with, read_side};
use crate::manifest::Unread;
use crate::model::{Id, Lock};
use lockmerge_core::{Context, Role};
use semver::Version;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::BTreeSet;

pub const ROLES: [Role; 3] = [Role::Base, Role::Ours, Role::Theirs];

/// Why a manifest rule cannot be applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fail {
    /// The manifests are not available or not read exactly (R14).
    Manifests(String),
    /// The manifests are read and show that the rule does not hold.
    Rule(String),
}

impl Fail {
    pub fn text(&self) -> &str {
        match self {
            Fail::Manifests(s) | Fail::Rule(s) => s,
        }
    }
}

impl From<Unread> for Fail {
    fn from(u: Unread) -> Self {
        Fail::Manifests(u.0)
    }
}

/// Both sides bumped one registry crate to different versions of its
/// compatibility class, and M1 proved the higher one can be kept.
#[derive(Clone, Debug)]
pub struct Group {
    pub low: Id,
    pub high: Id,
}

pub struct Shared {
    /// Base, ours, theirs.
    pub models: [Lock; 3],
    lock_path: String,
    found: OnceCell<Result<Found, Unread>>,
    sides: [OnceCell<Result<Side, Unread>>; 3],
    reach: OnceCell<[BTreeSet<Id>; 3]>,
    pub groups: RefCell<Vec<Group>>,
    /// Packages removed on one side and changed on the other that M3 may
    /// still prune; one left in the merged set is an R5 refusal.
    pub pending: RefCell<BTreeSet<Id>>,
    /// The envelope found registry crates added by each side that were never
    /// resolved together: `validate` must then prove their `links` (V6).
    pub links_proof: Cell<bool>,
}

fn other(role: Role) -> Role {
    match role {
        Role::Ours => Role::Theirs,
        _ => Role::Ours,
    }
}

pub fn index(role: Role) -> usize {
    match role {
        Role::Base => 0,
        Role::Ours => 1,
        Role::Theirs => 2,
    }
}

impl Shared {
    pub fn new(models: [&Lock; 3], lock_path: &str) -> Self {
        Self {
            models: models.map(Clone::clone),
            lock_path: lock_path.to_string(),
            found: OnceCell::new(),
            sides: [OnceCell::new(), OnceCell::new(), OnceCell::new()],
            reach: OnceCell::new(),
            groups: RefCell::new(Vec::new()),
            pending: RefCell::new(BTreeSet::new()),
            links_proof: Cell::new(false),
        }
    }

    pub fn model(&self, role: Role) -> &Lock {
        &self.models[index(role)]
    }

    /// The path packages of the three lockfiles.
    pub fn path_ids(&self) -> BTreeSet<Id> {
        self.models
            .iter()
            .flat_map(|m| m.packages.keys())
            .filter(|id| id.is_path())
            .cloned()
            .collect()
    }

    /// The one-sided version moves of path packages (ADR 0018): per branch, a
    /// path package of base that the branch lacks, and the one path package
    /// of that name the branch has and base lacks. Only an unambiguous pair
    /// (one of each by name) counts.
    pub fn path_moves(&self, role: Role) -> Vec<(Id, Id)> {
        let (base, side) = (&self.models[0], self.model(role));
        let paths =
            |m: &Lock| -> Vec<Id> { m.packages.keys().filter(|i| i.is_path()).cloned().collect() };
        let (pb, ps) = (paths(base), paths(side));
        let mut out = Vec::new();
        for b in pb.iter().filter(|b| !ps.contains(b)) {
            let removed = pb.iter().filter(|i| i.name == b.name && !ps.contains(i));
            let added: Vec<&Id> = ps
                .iter()
                .filter(|i| i.name == b.name && !pb.contains(i))
                .collect();
            if let ([n], 1) = (added.as_slice(), removed.count()) {
                out.push((b.clone(), (*n).clone()));
            }
        }
        out
    }

    fn moved_from(&self) -> BTreeSet<Id> {
        [Role::Ours, Role::Theirs]
            .into_iter()
            .flat_map(|r| self.path_moves(r))
            .map(|(b, _)| b)
            .collect()
    }

    /// The merged manifests of every path package, or why they are not
    /// available. With no path package there is nothing to read.
    pub fn found(&self, cx: &Context<'_>) -> Result<&Found, Fail> {
        self.found
            .get_or_init(|| {
                let found = find_with(&self.path_ids(), &self.moved_from(), &self.lock_path, cx)?;
                check_patch(&found, cx)?;
                Ok(found)
            })
            .as_ref()
            .map_err(|u| Fail::Manifests(u.0.clone()))
    }

    /// The manifests whose conflicting text merge was read as two insertions
    /// (ADR 0019), among those the rules have read so far.
    pub fn kept_both(&self) -> &[String] {
        match self.found.get() {
            Some(Ok(found)) => &found.kept_both,
            _ => &[],
        }
    }

    fn side(&self, role: Role, cx: &Context<'_>) -> Result<&Side, Fail> {
        let found = self.found(cx)?;
        self.sides[index(role)]
            .get_or_init(|| read_side(found, role, cx))
            .as_ref()
            .map_err(|u| Fail::Manifests(u.0.clone()))
    }

    /// Feature coverage against `role`'s manifests (rules M1 e and C). With no
    /// path package in any lockfile there is no manifest that selects
    /// features, and the check holds.
    pub fn covers(&self, role: Role, cx: &Context<'_>) -> Result<(), Fail> {
        self.covers_as(role, cx)
    }

    /// Coverage for a merge that combines both branches' changes (M2).
    pub fn covers_combined(&self, role: Role, cx: &Context<'_>) -> Result<(), Fail> {
        self.covers_as(role, cx)
    }

    fn covers_as(&self, role: Role, cx: &Context<'_>) -> Result<(), Fail> {
        if self.path_ids().is_empty() {
            return Ok(());
        }
        let found = self.found(cx)?;
        let side = self.side(role, cx)?;
        let base = self.side(Role::Base, cx)?;
        let names: BTreeSet<&str> = self
            .model(role)
            .packages
            .keys()
            .map(|id| id.name.as_str())
            .collect();
        // The other branch added the declarations; a held crate is proven by
        // its entries being the same in both lockfiles.
        let (mine, adder) = (self.model(role), self.model(other(role)));
        fn entries<'a>(m: &'a Lock, name: &str) -> Vec<(&'a Id, &'a crate::model::Package)> {
            m.packages
                .iter()
                .filter(|(id, _)| id.name == name)
                .collect()
        }
        let identical = |name: &str| entries(mine, name) == entries(adder, name);
        covers_combined(found, side, base, &names, &identical).map_err(Fail::Rule)
    }

    /// Per input, the packages reachable from its path packages.
    pub fn reachable(&self, role: Role) -> &BTreeSet<Id> {
        let all = self.reach.get_or_init(|| {
            [0, 1, 2].map(|i| {
                let m = &self.models[i];
                let mut seen: BTreeSet<Id> = BTreeSet::new();
                let mut stack: Vec<&Id> = m.packages.keys().filter(|id| id.is_path()).collect();
                while let Some(id) = stack.pop() {
                    if !seen.insert(id.clone()) {
                        continue;
                    }
                    if let Some(p) = m.packages.get(id) {
                        stack.extend(p.edges.iter());
                    }
                }
                seen
            })
        });
        &all[index(role)]
    }

    /// Replaces the low id of every group by its high id.
    pub fn normalized(&self, edges: &BTreeSet<Id>) -> BTreeSet<Id> {
        let groups = self.groups.borrow();
        edges
            .iter()
            .map(|e| {
                groups
                    .iter()
                    .find(|g| g.low == *e)
                    .map_or_else(|| e.clone(), |g| g.high.clone())
            })
            .collect()
    }
}

/// Whether `req` (a Cargo requirement) admits `version`.
pub fn admits(req: &str, version: &Version) -> bool {
    semver::VersionReq::parse(req).is_ok_and(|r| r.matches(version))
}

/// The targets named `name` among `edges`.
pub fn targets<'a>(edges: &'a BTreeSet<Id>, name: &str) -> BTreeSet<&'a Id> {
    edges.iter().filter(|e| e.name == name).collect()
}

/// Every name an edge set depends on.
pub fn names(edges: &BTreeSet<Id>) -> BTreeSet<&str> {
    edges.iter().map(|e| e.name.as_str()).collect()
}

/// The path packages of `models` with an edge into an id in `ids`.
pub fn path_dependents<'a>(models: &'a [Lock; 3], ids: &BTreeSet<&Id>) -> BTreeSet<&'a Id> {
    let mut out = BTreeSet::new();
    for m in models {
        for (id, p) in &m.packages {
            if id.is_path() && p.edges.iter().any(|e| ids.contains(e)) {
                out.insert(id);
            }
        }
    }
    out
}
