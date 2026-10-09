//! arugula-mux: the daemon's core, as a library. For now the leaves and the
//! access model: the state directory, permissions, paths, the pane's
//! process helpers (shim, holder, ConPTY) and the ACL.

pub mod acl;
pub mod classify;
#[cfg(windows)]
pub mod conpty;
pub mod heap;
#[cfg(unix)]
pub mod holder;
pub mod keys;
pub mod osc;
pub mod paths;
pub mod perm;
pub mod procinfo;
pub mod resume;
pub mod shim;
pub mod store;
pub mod sys;
