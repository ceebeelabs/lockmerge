//! The replay harness and gate of lockmerge (docs/02-oracle.md, ADRs 0005 to
//! 0007). The binary is `lockmerge-bench`; see [`cli`].

pub mod attribution;
pub mod case;
pub mod check;
pub mod classify;
pub mod cli;
pub mod driver;
pub mod extract;
pub mod gate;
pub mod metrics;
pub mod oracle;
pub mod registry;
pub mod replay;
pub mod report;
pub mod scan;
pub mod selftest;
pub mod util;
