//! Shared command-line infrastructure for Kival binaries.

#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod args;
pub mod dirs;
pub mod runner;
pub mod sigsegv;

/// The default filename for Kival configuration files.
pub const DEFAULT_CONFIG_FILENAME: &str = "kival.toml";
