//! The mux's Labs surface (see the daemon's `labs/mod.rs` for the pattern):
//! what Labs puts inside the core, with a twin for builds without the
//! feature. Today that is a VM pane's terminal.

use std::sync::Arc;

use crate::provider::{Begin, Exec, ExecEvent, Provider};

// VMs: the machines panes run on.
#[cfg(feature = "labs")]
pub mod machine;

/// Starts (or resumes) a session on `sprite`, a VM pane's terminal. There is
/// no pane on a machine without a provider, which a build without Labs
/// never has; if it is asked anyway, the session is lost at once.
#[cfg(feature = "labs")]
pub fn machine_start(
    rt: &tokio::runtime::Handle,
    provider: Arc<dyn Provider>,
    sprite: String,
    begin: Begin,
    size: (u16, u16),
    sink: impl Fn(ExecEvent) -> bool + Send + 'static,
) -> Exec {
    machine::start(rt, provider, sprite, begin, size, sink)
}

#[cfg(not(feature = "labs"))]
pub fn machine_start(
    _rt: &tokio::runtime::Handle,
    _provider: Arc<dyn Provider>,
    _sprite: String,
    _begin: Begin,
    _size: (u16, u16),
    sink: impl Fn(ExecEvent) -> bool + Send + 'static,
) -> Exec {
    sink(ExecEvent::Lost { machine_gone: true });
    Exec { tx: tokio::sync::mpsc::unbounded_channel().0 }
}
