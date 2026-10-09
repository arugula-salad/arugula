//! The mux's Labs surface (see the daemon's `labs/mod.rs` for the pattern):
//! what Labs puts inside the core, with a twin for builds without the
//! feature: a VM pane's terminal, chat's threads and huddles' calls.

use std::sync::Arc;

use crate::provider::{Begin, Exec, ExecEvent, Provider};

// Huddles (M63): who is in each voice call on a session.
#[cfg(feature = "labs")]
pub mod calls;
// VMs: the machines panes run on.
#[cfg(feature = "labs")]
pub mod machine;
// Chat (M61): the threads on panes and sessions.
#[cfg(feature = "labs")]
pub mod threads;

/// What a request that needs a Labs feature this build lacks answers.
pub fn not_built(what: &str) -> String {
    format!("{what} isn't in this build (built without labs)")
}

/// Threads on panes and sessions, and huddles on sessions. Always made, so
/// each has a twin that holds nothing and touches nothing on disk: a build
/// without Labs never opens, writes or deletes the state dir's `threads/`,
/// so a later build with Labs finds it as it was.
#[cfg(feature = "labs")]
pub use calls::Calls;
#[cfg(feature = "labs")]
pub use threads::Threads;

#[cfg(not(feature = "labs"))]
pub use absent::{Calls, Threads};

/// Whether `token` (an `@name` in a message) names this person: see
/// `threads::names`. Nobody is named where there is no chat.
#[cfg(feature = "labs")]
pub use threads::names as thread_names;

#[cfg(not(feature = "labs"))]
pub fn thread_names(_token: &str, _id: &str, _name: &str) -> bool {
    false
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

/// The twins of Threads and Calls, for a build without Labs.
#[cfg(not(feature = "labs"))]
mod absent {
    use std::path::Path;

    use arugula_proto::{CallMember, ClientId, SessionId, ThreadMsg, ThreadTarget};

    /// Threads of a build without Labs: none, and nothing read from or
    /// written to the state dir.
    pub struct Threads;

    impl Threads {
        pub fn open(_root: &Path) -> Self {
            Threads
        }

        pub fn get(&self, _target: ThreadTarget) -> &[ThreadMsg] {
            &[]
        }

        pub fn targets(&self) -> impl Iterator<Item = ThreadTarget> + '_ {
            std::iter::empty()
        }

        pub fn mark_read(&mut self, _who: &str, _target: ThreadTarget, _upto: u64) -> bool {
            false
        }

        pub fn save(&mut self) {}
    }

    /// Huddles of a build without Labs: none.
    #[derive(Default)]
    pub struct Calls;

    impl Calls {
        pub fn leave_all(&mut self, _client: ClientId) -> bool {
            false
        }

        pub fn retain(&mut self, _keep: impl FnMut(SessionId, &CallMember) -> bool) -> bool {
            false
        }
    }
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
