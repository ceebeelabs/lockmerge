//! The resolved model of one Cargo.lock: packages keyed by (name, version,
//! source), every dependency edge resolved to a full id.

use semver::Version;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A package id: name, version and source. A path package (workspace member or
/// path dependency) has no source. The derived order is Cargo's `PackageId`
/// order: name, then semver version, then source.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Id {
    pub name: String,
    pub version: Version,
    pub source: Option<String>,
}

impl Id {
    /// A path package has no `source` in the lockfile.
    pub fn is_path(&self) -> bool {
        self.source.is_none()
    }

    /// A registry package: its source is a `registry+` or `sparse+` index.
    pub fn is_registry(&self) -> bool {
        self.source
            .as_deref()
            .is_some_and(|s| s.starts_with("registry+") || s.starts_with("sparse+"))
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.name, self.version)?;
        if let Some(s) = &self.source {
            write!(f, " ({s})")?;
        }
        Ok(())
    }
}

/// The merge unit: what a package carries besides its id. Edges are resolved
/// ids, so two packages with the same edge text but different targets differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    pub checksum: Option<String>,
    pub edges: BTreeSet<Id>,
}

/// One parsed input.
#[derive(Clone, Debug)]
pub struct Lock {
    /// The `version` key: 3 or 4.
    pub version: u8,
    pub packages: BTreeMap<Id, Package>,
}

/// Why an edge text does not name exactly one package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EdgeError {
    /// Not `name`, `name version` or `name version (source)`.
    Malformed,
    /// Matches no package.
    Unresolved,
    /// Matches several packages.
    Ambiguous,
}

/// Splits an edge into name, optional version and optional source, in the
/// form Cargo writes and nothing else.
fn split_edge(text: &str) -> Option<(&str, Option<&str>, Option<&str>)> {
    let mut parts = text.splitn(3, ' ');
    let name = parts.next().filter(|n| !n.is_empty())?;
    let Some(version) = parts.next() else {
        return Some((name, None, None));
    };
    if version.is_empty() {
        return None;
    }
    let Some(rest) = parts.next() else {
        return Some((name, Some(version), None));
    };
    let source = rest.strip_prefix('(')?.strip_suffix(')')?;
    if source.is_empty() || source.contains([' ', '(', ')']) {
        return None;
    }
    Some((name, Some(version), Some(source)))
}

/// Cargo's lookup of an edge among `ids`: name, then version if given, then
/// source if given. Exactly one match or an error. Never picks one of several.
pub fn resolve_edge<'a>(
    ids: impl Iterator<Item = &'a Id>,
    text: &str,
) -> Result<&'a Id, EdgeError> {
    let (name, version, source) = split_edge(text).ok_or(EdgeError::Malformed)?;
    let version = version
        .map(|v| Version::parse(v).map_err(|_| EdgeError::Malformed))
        .transpose()?;
    let mut found: Option<&Id> = None;
    for id in ids {
        if id.name != name {
            continue;
        }
        if version.as_ref().is_some_and(|v| *v != id.version) {
            continue;
        }
        if source.is_some_and(|s| id.source.as_deref() != Some(s)) {
            continue;
        }
        if found.is_some() {
            return Err(EdgeError::Ambiguous);
        }
        found = Some(id);
    }
    found.ok_or(EdgeError::Unresolved)
}

/// An edge as Cargo writes it: name, optional version, optional source. The
/// derived order is Cargo's order for a package's `dependencies` (the text
/// fields, not the semver order: `rand 0.10.0` sorts before `rand 0.8.5`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EdgeForm {
    pub name: String,
    pub version: Option<String>,
    pub source: Option<String>,
}

impl EdgeForm {
    pub fn text(&self) -> String {
        let mut text = self.name.clone();
        if let Some(v) = &self.version {
            text.push(' ');
            text.push_str(v);
        }
        if let Some(s) = &self.source {
            text.push_str(&format!(" ({s})"));
        }
        text
    }
}

/// The form of an edge to `target`, as Cargo's `encodable_package_id` writes
/// it among `ids` (which contain `target`): the version is dropped when the
/// name has one version, the source when the name and version have one source.
///
/// `None` when the form is not known to match Cargo's: a name with one version
/// and several sources, whose edge Cargo would write with a source but no
/// version, a form this crate has not seen in a real lockfile.
pub fn edge_form(ids: &[&Id], target: &Id) -> Option<EdgeForm> {
    let same_name = ids.iter().filter(|i| i.name == target.name);
    let mut versions = BTreeSet::new();
    let mut same_version = 0usize;
    for i in same_name {
        versions.insert(&i.version);
        if i.version == target.version {
            same_version += 1;
        }
    }
    let mut form = EdgeForm {
        name: target.name.clone(),
        version: None,
        source: None,
    };
    if versions.len() > 1 {
        form.version = Some(target.version.to_string());
        if same_version > 1 {
            form.source = target.source.clone();
        }
    } else if same_version > 1 {
        return None;
    }
    Some(form)
}

/// V4's grouping: Cargo's semver compatibility class of a version.
pub fn compat_class(v: &Version) -> (u64, u64, u64) {
    if v.major != 0 {
        (v.major, 0, 0)
    } else if v.minor != 0 {
        (0, v.minor, 0)
    } else {
        (0, 0, v.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REG: &str = "registry+https://github.com/rust-lang/crates.io-index";

    fn id(name: &str, version: &str, source: Option<&str>) -> Id {
        Id {
            name: name.into(),
            version: version.parse().unwrap(),
            source: source.map(str::to_string),
        }
    }

    #[test]
    fn an_edge_resolves_by_name_version_and_source() {
        let ids = [
            id("rand", "0.6.5", Some(REG)),
            id("rand", "0.7.3", Some(REG)),
            id("serde", "1.0.100", Some(REG)),
        ];
        let at = |t: &str| resolve_edge(ids.iter(), t).cloned();
        assert_eq!(at("serde"), Ok(ids[2].clone()));
        assert_eq!(at("rand 0.7.3"), Ok(ids[1].clone()));
        assert_eq!(at(&format!("rand 0.6.5 ({REG})")), Ok(ids[0].clone()));
    }

    /// Accept and refuse neighbors: a bare name that matches several versions
    /// is never resolved to one of them.
    #[test]
    fn an_ambiguous_or_unknown_edge_is_an_error() {
        let ids = [
            id("rand", "0.6.5", Some(REG)),
            id("rand", "0.7.3", Some(REG)),
        ];
        let at = |t: &str| resolve_edge(ids.iter(), t).cloned();
        assert_eq!(at("rand"), Err(EdgeError::Ambiguous));
        assert_eq!(at("rand 0.8.0"), Err(EdgeError::Unresolved));
        assert_eq!(
            at("rand 0.6.5 (git+https://example.com/x)"),
            Err(EdgeError::Unresolved)
        );
        assert_eq!(at("nope"), Err(EdgeError::Unresolved));
    }

    #[test]
    fn a_version_less_edge_with_two_sources_of_one_version_is_ambiguous() {
        let ids = [
            id("foo", "1.0.0", Some(REG)),
            id("foo", "1.0.0", Some("git+https://example.com/foo.git")),
        ];
        assert_eq!(
            resolve_edge(ids.iter(), "foo 1.0.0").map(|_| ()),
            Err(EdgeError::Ambiguous)
        );
    }

    #[test]
    fn edges_in_forms_cargo_does_not_write_are_malformed() {
        let ids = [id("a", "1.0.0", Some(REG))];
        for text in [
            "",
            " a",
            "a ",
            "a  1.0.0",
            "a 1.0",
            "a 1.0.0 x",
            "a 1.0.0 ()",
            "a 1.0.0 (x y)",
            "a 1.0.0 (x) y",
        ] {
            assert_eq!(
                resolve_edge(ids.iter(), text).map(|_| ()),
                Err(EdgeError::Malformed),
                "{text:?}"
            );
        }
    }

    #[test]
    fn edges_are_as_short_as_cargo_writes_them() {
        let one = [&id("serde", "1.0.100", Some(REG))];
        assert_eq!(edge_form(&one, one[0]).unwrap().text(), "serde");

        let two = [
            &id("rand", "0.6.5", Some(REG)),
            &id("rand", "0.7.3", Some(REG)),
        ];
        assert_eq!(edge_form(&two, two[1]).unwrap().text(), "rand 0.7.3");

        let git = "git+https://example.com/foo.git";
        let sources = [
            &id("foo", "1.0.0", Some(REG)),
            &id("foo", "1.0.0", Some(git)),
            &id("foo", "2.0.0", Some(REG)),
        ];
        assert_eq!(
            edge_form(&sources, sources[1]).unwrap().text(),
            format!("foo 1.0.0 ({git})")
        );
        assert_eq!(edge_form(&sources, sources[2]).unwrap().text(), "foo 2.0.0");
    }

    /// One version, two sources: the form is not known to match Cargo's, so
    /// there is none.
    #[test]
    fn one_version_with_two_sources_has_no_known_form() {
        let git = "git+https://example.com/foo.git";
        let ids = [
            &id("foo", "1.0.0", Some(REG)),
            &id("foo", "1.0.0", Some(git)),
        ];
        assert_eq!(edge_form(&ids, ids[0]), None);
    }

    /// Cargo sorts a package's edges by their text fields, not by semver.
    #[test]
    fn edge_forms_sort_by_text_like_cargo() {
        let ids = [
            &id("rand", "0.10.0", Some(REG)),
            &id("rand", "0.8.5", Some(REG)),
        ];
        let mut forms: Vec<_> = ids.iter().map(|t| edge_form(&ids, t).unwrap()).collect();
        forms.sort();
        let text: Vec<_> = forms.iter().map(EdgeForm::text).collect();
        assert_eq!(text, ["rand 0.10.0", "rand 0.8.5"]);
        assert!(
            ids[1].version < ids[0].version,
            "semver order is the reverse"
        );
    }

    #[test]
    fn ids_sort_by_name_then_semver_then_source() {
        let mut ids = [
            id("b", "1.0.0", None),
            id("a", "1.10.0", None),
            id("a", "1.9.0", None),
            id("a", "1.9.0", Some(REG)),
        ];
        ids.sort();
        let text: Vec<_> = ids.iter().map(Id::to_string).collect();
        assert_eq!(
            text,
            [
                "a 1.9.0",
                &format!("a 1.9.0 ({REG})"),
                "a 1.10.0",
                "b 1.0.0"
            ]
        );
    }

    #[test]
    fn compatibility_classes_follow_cargo() {
        let class = |v: &str| compat_class(&v.parse().unwrap());
        assert_eq!(class("1.2.3"), class("1.9.0"));
        assert_ne!(class("1.2.3"), class("2.0.0"));
        assert_eq!(class("0.6.5"), class("0.6.9"));
        assert_ne!(class("0.6.5"), class("0.7.3"));
        assert_ne!(class("0.0.1"), class("0.0.2"));
        assert_eq!(class("0.0.2"), class("0.0.2"));
    }
}
