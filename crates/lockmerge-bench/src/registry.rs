//! The formats the benchmark attributes results to. Each format crate adds
//! itself here when it lands (`lockmerge-cargo`, #36). Until then the list is
//! empty: classification still works, because it reads git and the oracle,
//! but the per-rule and per-code tables only cover the core registry.

use lockmerge_core::Format;

pub fn formats() -> Vec<&'static dyn Format> {
    Vec::new()
}
