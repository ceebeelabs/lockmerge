//! The formats the benchmark attributes results to. Each format crate adds
//! itself here when it lands.

use lockmerge_core::Format;

pub fn formats() -> Vec<&'static dyn Format> {
    vec![lockmerge_cargo::format()]
}
