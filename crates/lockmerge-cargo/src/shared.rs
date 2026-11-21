//! What one merge run knows across the engine's phases: the three parsed
//! lockfiles, the manifests (read at most once, and only when a rule needs
//! them), the both-sides-bumped groups the envelope proved (M1) and the
//! removals pending for the prune step (M3).

use crate::find::{Found, Side, covers, find, read_side};
use crate::manifest::Unread;
use crate::model::{Id, Lock};
use lockmerge_core::{Context, Role};
use semver::Version;
use std::cell::{OnceCell, RefCell};
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

    /// The merged manifests of every path package, or why they are not
    /// available. With no path package there is nothing to read.
    pub fn found(&self, cx: &Context<'_>) -> Result<&Found, Fail> {
        self.found
            .get_or_init(|| find(&self.path_ids(), &self.lock_path, cx))
            .as_ref()
            .map_err(|u| Fail::Manifests(u.0.clone()))
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
        covers(found, side, base, &names).map_err(Fail::Rule)
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
