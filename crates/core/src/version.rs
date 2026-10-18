//! The release this process is: its binary says, at start (`set`).
//!
//! Library crates keep version 0.0.0 rather than the workspace's, so a
//! release bump rebuilds only the binaries, not every crate under them. What
//! a library reports as its version (the mux's Hello) comes from here.

use std::sync::OnceLock;

static VERSION: OnceLock<&'static str> = OnceLock::new();

/// Called once, first thing in a binary's `main`, with its
/// `env!("CARGO_PKG_VERSION")`. A second call changes nothing.
pub fn set(version: &'static str) {
    let _ = VERSION.set(version);
}

/// The binary's version, or 0.0.0 when nothing set it (a library's own
/// tests).
pub fn get() -> &'static str {
    VERSION.get().copied().unwrap_or("0.0.0")
}
