//! arugula-mux: the daemon's core, as a library. For now the leaves, the
//! access model, and the panes: the state directory, permissions, paths, the
//! pane's process helpers (shim, holder, ConPTY), the ACL, the pane itself,
//! the files it can read, and the machines (providers) panes can run on.

pub mod acl;
pub mod classify;
#[cfg(windows)]
pub mod conpty;
pub mod fs;
pub mod heap;
#[cfg(unix)]
pub mod holder;
// The pane host on Windows (M58): the shim's part there.
#[cfg(windows)]
pub mod host;
pub mod inventory;
pub mod keys;
pub mod labs;
pub mod osc;
pub mod pane;
pub mod paths;
pub mod perm;
pub mod procinfo;
pub mod provider;
pub mod resume;
pub mod shellenv;
pub mod shellint;
pub mod shim;
pub mod store;
pub mod sys;
