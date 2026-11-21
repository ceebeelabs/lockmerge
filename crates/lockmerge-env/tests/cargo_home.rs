//! `CargoHome` over real directories: it lists `registry/index/`, reads the
//! file at one relative path in each directory, and nothing else.

use lockmerge_env::{CargoHome, IndexCache};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("lockmerge-home-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        Home(dir)
    }

    fn put(&self, index_dir: &str, rel: &str, bytes: &[u8]) {
        let p = self.0.join("registry/index").join(index_dir).join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn reads_the_file_in_every_index_directory_that_has_it_in_name_order() {
    let h = Home::new();
    h.put("index.crates.io-bbbb", ".cache/al/ph/alpha", b"second");
    h.put("index.crates.io-aaaa", ".cache/al/ph/alpha", b"first");
    h.put("index.crates.io-cccc", ".cache/be/ta/beta", b"other crate");
    assert_eq!(
        CargoHome::new(&h.0).files(".cache/al/ph/alpha"),
        vec![b"first".to_vec(), b"second".to_vec()]
    );
    assert_eq!(
        CargoHome::new(&h.0).files(".cache/be/ta/beta"),
        vec![b"other crate".to_vec()]
    );
}

#[test]
fn nothing_when_the_cache_or_the_file_is_missing() {
    let h = Home::new();
    assert!(CargoHome::new(&h.0).files(".cache/al/ph/alpha").is_empty());
    h.put("index.crates.io-aaaa", ".cache/be/ta/beta", b"x");
    assert!(CargoHome::new(&h.0).files(".cache/al/ph/alpha").is_empty());
    assert!(
        CargoHome::new(h.0.join("no-such-dir"))
            .files("x")
            .is_empty()
    );
    assert!(CargoHome::absent().files(".cache/be/ta/beta").is_empty());
}

#[test]
fn a_path_that_leaves_the_index_directory_reads_nothing() {
    let h = Home::new();
    fs::write(h.0.join("secret"), b"x").unwrap();
    h.put("index.crates.io-aaaa", "f", b"y");
    let c = CargoHome::new(&h.0);
    for bad in ["../../../secret", "/etc/hosts", "", "a/../f", "./f"] {
        assert!(c.files(bad).is_empty(), "{bad}");
    }
    assert_eq!(c.files("f"), vec![b"y".to_vec()]);
}

#[test]
fn a_directory_where_the_file_should_be_is_skipped() {
    let h = Home::new();
    h.put("index.crates.io-aaaa", "f/inner", b"x");
    h.put("index.crates.io-bbbb", "f", b"y");
    assert_eq!(CargoHome::new(&h.0).files("f"), vec![b"y".to_vec()]);
}

#[test]
fn cargo_home_wins_over_the_home_directory_and_empty_counts_as_unset() {
    let h = Home::new();
    h.put("index.crates.io-aaaa", "f", b"x");
    let os = |p: &Path| Some(OsString::from(p));
    let from_cargo_home = CargoHome::from_vars(os(&h.0), Some("/nonexistent".into()));
    assert_eq!(from_cargo_home.files("f"), vec![b"x".to_vec()]);

    // `<home>/.cargo` when CARGO_HOME is unset or empty.
    let parent = Home::new();
    fs::rename(&h.0, parent.0.join(".cargo")).unwrap();
    for unset in [None, Some(OsString::new())] {
        let c = CargoHome::from_vars(unset, os(&parent.0));
        assert_eq!(c.files("f"), vec![b"x".to_vec()]);
    }
    assert!(CargoHome::from_vars(None, None).files("f").is_empty());
    assert!(
        CargoHome::from_vars(Some(OsString::new()), Some(OsString::new()))
            .files("f")
            .is_empty()
    );
    // `h` was moved; its Drop removes nothing that exists.
}
