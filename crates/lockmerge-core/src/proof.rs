use std::collections::BTreeMap;

/// The label under which a unit was accepted.
///
/// A proof names a rule and records the facts that made the rule apply. It is
/// a label, not a theorem: the compiler guarantees every accepted unit has a
/// named rule, not that the rule is right (that is the job of the accept and
/// refuse tests of the rule).
///
/// Formats cannot build a `Proof` directly. They get one from
/// [`Context::proof`](crate::Context::proof). The engine checks the rule
/// against [`Pipeline::rules`](crate::Pipeline::rules) and treats an
/// unregistered rule as an internal error, never as a merge.
///
/// ```compile_fail,E0624
/// let _ = lockmerge_core::Proof::new("x.R1", Vec::new());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proof {
    rule: &'static str,
    facts: Vec<(&'static str, String)>,
}

impl Proof {
    pub(crate) fn new(rule: &'static str, facts: Vec<(&'static str, String)>) -> Self {
        Self { rule, facts }
    }

    /// The rule id this proof names.
    pub fn rule(&self) -> &'static str {
        self.rule
    }

    /// The facts the rule relied on.
    pub fn facts(&self) -> &[(&'static str, String)] {
        &self.facts
    }
}

#[derive(Clone, Debug)]
struct Entry<U> {
    unit: U,
    proof: Proof,
}

/// The merged units so far. Read-only for formats; only the engine adds or
/// removes units.
///
/// ```compile_fail,E0451
/// let _ = lockmerge_core::Draft::<String, String> { entries: Default::default() };
/// ```
#[derive(Clone, Debug)]
pub struct Draft<K, U> {
    entries: BTreeMap<K, Entry<U>>,
}

impl<K: Ord, U> Draft<K, U> {
    pub(crate) fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub(crate) fn insert(&mut self, key: K, unit: U, proof: Proof) {
        self.entries.insert(key, Entry { unit, proof });
    }

    pub(crate) fn remove(&mut self, key: &K) -> bool {
        self.entries.remove(key).is_some()
    }

    pub(crate) fn seal(self) -> Sealed<K, U> {
        Sealed {
            entries: self.entries,
        }
    }

    pub fn get(&self, key: &K) -> Option<&U> {
        self.entries.get(key).map(|e| &e.unit)
    }

    /// The proof under which `key` was accepted.
    pub fn proof(&self, key: &K) -> Option<&Proof> {
        self.entries.get(key).map(|e| &e.proof)
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    /// Units in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &U)> {
        self.entries.iter().map(|(k, e)| (k, &e.unit))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A merged set that passed every check. [`Pipeline::serialize`] takes only
/// this type, so bytes can be written only for a set the engine sealed.
///
/// ```compile_fail,E0451
/// let _ = lockmerge_core::Sealed::<String, String> { entries: Default::default() };
/// ```
///
/// [`Pipeline::serialize`]: crate::Pipeline::serialize
#[derive(Clone, Debug)]
pub struct Sealed<K, U> {
    entries: BTreeMap<K, Entry<U>>,
}

impl<K: Ord, U> Sealed<K, U> {
    pub fn get(&self, key: &K) -> Option<&U> {
        self.entries.get(key).map(|e| &e.unit)
    }

    /// Units in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &U)> {
        self.entries.iter().map(|(k, e)| (k, &e.unit))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
