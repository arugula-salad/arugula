//! The mux's Labs surface (see the daemon's `labs/mod.rs` for the pattern):
//! what Labs puts inside the core, with a twin for builds without the
//! feature: a VM pane's terminal.

use std::sync::Arc;

use crate::provider::{Begin, Exec, ExecEvent, Provider};

// VMs: the machines panes run on.
#[cfg(feature = "labs")]
pub mod machine;

/// What a request that needs a Labs feature this build lacks answers.
pub fn not_built(what: &str) -> String {
    format!("{what} isn't in this build (built without labs)")
}

/// What a request for a Labs feature whose flag is off on this machine is
/// told (#347): which feature, and how its owner turns it on. The machine
/// is the daemon that answered, which `arugula --host` may have reached
/// from another one.
pub fn flag_off(flag: &str) -> String {
    let title = arugula_proto::flags::FLAGS.iter().find(|f| f.name == flag).map_or(flag, |f| f.title);
    flag_off_as(&format!("{title} is"), flag)
}

/// [`flag_off`] with its own subject and verb: "Forgejo blocks are".
pub fn flag_off_as(subject: &str, flag: &str) -> String {
    format!(
        "{subject} a Labs feature that's off on this machine: its owner turns it on with `arugulad flags {flag} on` there, or in Developer settings"
    )
}

/// `Ok` where the machine has turned on `flag` (#347), else the refusal
/// ([`flag_off`]). Each gate on a Labs feature asks this, so the feature
/// follows its flag, not just its listing. A build without Labs answers
/// `Ok`: what it lacks says so in its own words ([`not_built`]).
#[cfg(feature = "labs")]
pub fn flag(state_dir: &std::path::Path, flag: &str) -> Result<(), String> {
    if arugula_proto::flags::get(state_dir, flag) { Ok(()) } else { Err(flag_off(flag)) }
}

#[cfg(not(feature = "labs"))]
pub fn flag(_state_dir: &std::path::Path, _flag: &str) -> Result<(), String> {
    Ok(())
}

/// Why a request that needs VM panes can't have them: `why` when there is
/// no provider here, and that this build has no VMs when it was built without
/// Labs.
#[cfg(feature = "labs")]
pub fn vms_unavailable(why: &str) -> String {
    why.to_owned()
}

#[cfg(not(feature = "labs"))]
pub fn vms_unavailable(_why: &str) -> String {
    "VMs aren't in this build (built without labs)".to_owned()
}

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
