//! Agents (M6b): what the daemon knows of agent blocks without the block
//! itself, which is `agent_block/` (#232) and reaches the daemon through
//! [`Agents`], `features` and `mcp::registry`.
//!
//! Here: the agents there are and how each is run ([`defs`]), their ACP
//! adapters ([`adapters`]), the platform's hosted agents ([`hosted`]),
//! pictures ([`images`]), the transcript ([`transcript`]), the recipe a
//! block wears ([`Worn`], which Labs makes) and the secrets it keeps out of
//! logs ([`scrub`]), and what work sessions and deploys share with the
//! block ([`WOKEN`], `pipe_cloexec`).

pub mod adapters;
pub mod defs;
pub mod hosted;
pub mod images;
pub mod transcript;

use std::sync::{Arc, OnceLock};

use serde_json::Value;

use crate::{hosted::archive::Closed, mux::AgentSessions};

/// A recipe an agent block wears (M76): what it adds to the block, and the
/// secrets to keep out of every log. Labs wears one (`lab_hooks`).
pub trait Worn: Send + Sync {
    /// Put it on `session/new`'s `_meta`.
    fn dress(&self, meta: &mut Value);
    /// The permission mode it asks for (a recipe's `permissionMode`).
    fn permission_mode(&self) -> Option<&str>;
    /// The servers it adds to the agent's own, each credential a reference.
    fn servers(&self) -> &[Value];
    /// What the references stand for: the adapter's environment.
    fn env(&self) -> &[(String, String)];
    /// Every value that may be secret, to keep out of logs.
    fn secrets(&self) -> &[String];
    /// What the block shows of it (nothing secret), as JSON.
    fn info(&self) -> Value;
    /// What it is, for `capture --text`.
    fn text(&self) -> String;
}

/// What stands in for a secret scrubbed out of what's shown.
pub const REDACTED: &str = "<redacted>";

/// Replace every secret in a JSON line with `<redacted>` (as it's escaped
/// in JSON, and as it is).
pub fn scrub(line: &str, secrets: &[String]) -> Option<String> {
    let mut out: Option<String> = None;
    for s in secrets {
        let escaped = serde_json::to_string(s).unwrap_or_default();
        let escaped = escaped.trim_matches('"');
        for needle in [escaped, s.as_str()] {
            if needle.len() < 6 {
                continue;
            }
            let cur = out.as_deref().unwrap_or(line);
            if cur.contains(needle) {
                out = Some(cur.replace(needle, REDACTED));
            }
        }
    }
    out
}

/// A pipe whose ends aren't inherited (`pipe2` doesn't exist on macOS).
#[cfg(unix)]
pub fn pipe_cloexec() -> nix::Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    use nix::fcntl::{FcntlArg, FdFlag, fcntl};
    let (r, w) = nix::unistd::pipe()?;
    for fd in [&r, &w] {
        fcntl(fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;
    }
    Ok((r, w))
}

/// A turn a background task woke, which no one typed: its prompt as a
/// block's turn shows it, and as a work session reads it back.
pub const WOKEN: &str = "(woken by a background task)";

/// What the daemon asks of the agent block, once it's registered.
pub trait Agents: Send + Sync + 'static {
    /// What each open agent block would leave if it closed now (Arugula
    /// #209): a hosted box archives these before a rebuild takes it.
    fn open_transcripts(&self) -> Vec<Closed>;
    /// The Claude Code conversations on this machine, as the mux asks
    /// (#146).
    fn sessions(&self) -> Arc<dyn AgentSessions>;
    /// The titles of Claude Code conversations, by id (#429): the name a
    /// session goes by. Reads the index, so call it off the runtime.
    fn conversation_titles(&self, _ids: &[String]) -> std::collections::HashMap<String, String> {
        std::collections::HashMap::new()
    }
}

static AGENTS: OnceLock<Box<dyn Agents>> = OnceLock::new();

/// Puts the agent block in this daemon (once, before `main`).
pub fn install(a: Box<dyn Agents>) {
    if AGENTS.set(a).is_err() {
        tracing::warn!("the agent block was registered twice");
    }
}

/// [`Agents::open_transcripts`]; none without the agent block.
pub fn open_transcripts() -> Vec<Closed> {
    AGENTS.get().map(|a| a.open_transcripts()).unwrap_or_default()
}

/// [`Agents::sessions`]; none without the agent block.
pub(crate) fn sessions() -> Arc<dyn AgentSessions> {
    match AGENTS.get() {
        Some(a) => a.sessions(),
        None => Arc::new(NoSessions),
    }
}

/// [`Agents::conversation_titles`]; none without the agent block.
pub(crate) fn conversation_titles(ids: &[String]) -> std::collections::HashMap<String, String> {
    AGENTS.get().map(|a| a.conversation_titles(ids)).unwrap_or_default()
}

/// No conversations: a build without the agent block.
#[derive(Debug)]
struct NoSessions;

impl AgentSessions for NoSessions {
    fn transcript(&self, _: &str) -> Option<std::path::PathBuf> {
        None
    }

    fn live(
        &self,
        _: std::collections::HashMap<u32, arugula_proto::PaneId>,
    ) -> std::collections::HashMap<arugula_proto::PaneId, (String, Option<String>)> {
        std::collections::HashMap::new()
    }
}
