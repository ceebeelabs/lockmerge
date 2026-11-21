//! The formats the `lockmerge` binary knows, in the order they are tried.
//! A file name is handed to the first format whose `matches` accepts it.

use lockmerge_core::Format;

pub fn all() -> Vec<&'static dyn Format> {
    vec![lockmerge_cargo::format()]
}
