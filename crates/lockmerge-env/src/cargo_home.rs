//! Cargo's local registry index cache, read-only (`docs/formats/cargo-registry-index.md`).
//!
//! [`CargoHome`] lists `registry/index/` under `CARGO_HOME` and reads files. It
//! decides nothing, computes no directory name, never writes, never fetches and
//! never runs Cargo. Whatever it cannot read is left out, which makes the
//! format refuse (ADR 0008 decision 15).

use lockmerge_core::IndexCache;
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// A `CARGO_HOME` directory, or none.
#[derive(Clone, Debug)]
pub struct CargoHome {
    root: Option<PathBuf>,
}

impl CargoHome {
    /// A cache under `root`, which holds `registry/index/`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Some(root.into()),
        }
    }

    /// No cache at all: every lookup is empty.
    pub fn absent() -> Self {
        Self { root: None }
    }

    /// Cargo's rule: `CARGO_HOME`, else `.cargo` in the home directory. An
    /// empty value counts as unset, and with neither there is no cache.
    pub fn from_vars(cargo_home: Option<OsString>, home: Option<OsString>) -> Self {
        let non_empty = |v: Option<OsString>| v.filter(|v| !v.is_empty());
        match (non_empty(cargo_home), non_empty(home)) {
            (Some(c), _) => Self::new(c),
            (None, Some(h)) => Self::new(Path::new(&h).join(".cargo")),
            (None, None) => Self::absent(),
        }
    }

    /// From the process environment (`CARGO_HOME`, `HOME`, or `USERPROFILE` on
    /// Windows).
    pub fn from_env() -> Self {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        Self::from_vars(std::env::var_os("CARGO_HOME"), home)
    }
}

impl IndexCache for CargoHome {
    /// The file at `rel_path` in each directory of `registry/index/`, in the
    /// order of the directory names. A path that is not made of plain names, a
    /// missing or unreadable directory and a missing or unreadable file give
    /// nothing.
    fn files(&self, rel_path: &str) -> Vec<Vec<u8>> {
        let Some(root) = &self.root else {
            return Vec::new();
        };
        let plain = Path::new(rel_path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
        if rel_path.is_empty() || !plain {
            return Vec::new();
        }
        let Ok(entries) = fs::read_dir(root.join("registry").join("index")) else {
            return Vec::new();
        };
        let mut dirs: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        dirs.iter()
            .filter_map(|d| fs::read(d.join(rel_path)).ok())
            .collect()
    }
}
