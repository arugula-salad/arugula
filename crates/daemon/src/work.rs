//! A hosted machine's runs and agent sessions (M212, M255), as the rest of
//! the daemon sees them. The engine that pulls, runs and reports them is
//! `work_engine` (#232), which registers itself as [`Work`]; the daemon
//! reaches it only through that. What stays here is what others read
//! without it:
//!
//! - [`printable`], which the deploy drivers use to show what they run;
//! - the ledger's directory ([`DIR`]) and the ports a run is kept off
//!   ([`fenced_port`]);
//! - the session users and the sessions this box holds ([`sessions`]),
//!   which the agent blocks, the hosted box and a session's MCP read.

use std::sync::{Arc, OnceLock};

use crate::server::App;

pub mod sessions;

/// The ledger's directory, in the state directory.
pub const DIR: &str = "work";

/// The box's provider's agent on loopback (E2B's envd and its sibling),
/// which a run must never reach.
pub const PROVIDER_PORTS: &str = "49982:49983";

/// `s` with nothing that doesn't print as itself: control characters
/// (escapes included) and the invisible ones that reorder or hide text
/// (bidi controls, zero-width characters), each written as `\u{…}`. What
/// a run shows of itself is what it runs.
pub fn printable(s: &str) -> String {
    let hidden = |c: char| {
        c.is_control()
            || matches!(c as u32, 0x061C | 0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB)
    };
    s.chars().map(|c| if hidden(c) { format!("\\u{{{:x}}}", c as u32) } else { c.to_string() }).collect()
}

/// Whether the firewall keeps runs and sessions off loopback port `port`
/// whatever the daemon's own are: the privileged ones and the provider's.
pub fn fenced_port(port: u16) -> bool {
    let (a, b) = PROVIDER_PORTS.split_once(':').unwrap_or((PROVIDER_PORTS, PROVIDER_PORTS));
    port <= 1023 || (a.parse::<u16>().is_ok_and(|a| a <= port) && b.parse::<u16>().is_ok_and(|b| port <= b))
}

/// The runs engine, as the daemon calls it.
pub trait Work: Send + Sync + 'static {
    /// A hosted machine's work, once it's in control: follow up what a
    /// restart left, then pull, take, stop and send.
    fn start(&self, app: Arc<App>);
    /// Ask for work now, not at the next pull.
    fn nudge(&self);
    /// Stop the runs going under a standing grant a revoke takes back.
    fn stop_revoked(&self, app: &App);
    /// Whether a run, or a session's turn, is going on this machine.
    fn running(&self) -> bool;
}

static WORK: OnceLock<Box<dyn Work>> = OnceLock::new();

/// Puts the runs engine in this daemon (once, before `main`).
pub fn install(w: Box<dyn Work>) {
    if WORK.set(w).is_err() {
        tracing::warn!("the runs engine was registered twice");
    }
}

/// [`Work::start`]; nothing without the engine.
pub fn start(app: Arc<App>) {
    match WORK.get() {
        Some(w) => w.start(app),
        None => tracing::warn!("hosted: this build has no runs engine: it takes no runs or sessions"),
    }
}

/// [`Work::nudge`].
pub fn nudge() {
    if let Some(w) = WORK.get() {
        w.nudge();
    }
}

/// [`Work::stop_revoked`].
pub fn stop_revoked(app: &App) {
    if let Some(w) = WORK.get() {
        w.stop_revoked(app);
    }
}

/// [`Work::running`]; never without the engine.
pub fn running() -> bool {
    WORK.get().is_some_and(|w| w.running())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_run_shows_prints_as_itself() {
        assert_eq!(printable("make test"), "make test");
        assert_eq!(printable("a\nb\rc\x1b[31m"), r"a\u{a}b\u{d}c\u{1b}[31m");
        assert_eq!(printable("rm\u{202e}fdp.exe\u{200b}"), r"rm\u{202e}fdp.exe\u{200b}");
    }
}
