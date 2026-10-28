//! MCP (M16): Arugula as tools for any agent. A curated layer over the
//! same machinery the API uses, served by rmcp at `/mcp` as Streamable HTTP
//! (both the stateless 2026-07-28 protocol Claude Code speaks and the older
//! sessions Codex speaks).
//!
//! **Who.** `/mcp` is part of the owner's API, with the same checks: the
//! owner over the Unix socket (`arugula mcp` bridges a stdio client to
//! it), on loopback, or over the tailnet (serve's headers, or `WhoIs` on a
//! direct listener). Anyone else needs a bearer token (`tokens.rs`): a
//! client token from `arugula mcp token`, for clients without a tailnet
//! identity, or an agent block's own, which only reaches the block's tab
//! (see [`Scope`]). Browsers must come from one of our origins, as for the
//! API. rmcp's own Host check is off: the daemon's (`Access::check_host`)
//! already ran on every TCP request, with the tailnet names in it.
//!
//! **Where** (#232). The server (rmcp's handler, the table, the tools'
//! calls, `capabilities`) is `mcp_server`, which the base reaches through
//! [`Mcp`]. Here is what it shares with the base's operations (`ops.rs`)
//! and the feature crates' tools (`registry.rs`): who's calling
//! ([`Caller`]), a call and a tool's row (`tools.rs`), the tokens, a VM
//! agent's relay, an agent block's [`Link`], and driving a cloud
//! workspace's machine (`hosted.rs`).
//!
//! **Tools** (`mcp_server`'s `tools.rs`) return their structured result with a `summary`
//! sentence in it, and the same JSON as text: Claude Code shows the model
//! only the structured part, Codex both (S14). Output is paged (about 16 KB
//! a page) so a chatty pane can't flood the agent's context, and waits
//! send progress every 15s (over HTTP, Claude Code drops a call that's
//! silent for 60s) and return "still running" by 100s, before interactive
//! Claude Code moves a call into the background.
//!
//! **Resources** are read-only templates (`arugula://pane/N/output`,
//! `…/screen`, `arugula://block/N`, `arugula://history`). No client
//! subscribes to resources (S14), so they aren't subscribable.
//!
//! Every call is logged with the client's name and token, and what it
//! starts says so: the pane shows "started by mcp:<client>", and what the
//! client typed is in history as theirs.

pub mod hosted;
pub mod issues;
pub mod ops;
pub mod registry;
pub mod relay;
pub mod results;
pub mod session;
pub mod tokens;
pub mod tools;

use std::sync::{Arc, OnceLock};

use arugula_proto::PaneId;
use axum::Router;

pub use self::tokens::Tokens;
use crate::server::App;

pub const PATH: &str = "/mcp";
/// What agent blocks call the server.
pub const SERVER_NAME: &str = "arugula";

/// Whether a permission rule's tool is `tool`. A rule made before the
/// rename names our tools `mcp__illogical__X`; they're now
/// `mcp__arugula__X`, and the rule still means them.
pub fn same_tool(rule: &str, tool: &str) -> bool {
    rule == tool
        || rule.strip_prefix("mcp__illogical__").is_some_and(|x| tool.strip_prefix("mcp__arugula__") == Some(x))
}

/// How an agent block reaches MCP: the daemon's loopback `/mcp`, or the
/// CLI's bridge on the Unix socket, with the block's token; in a VM, a
/// relay the daemon opens into it (`relay.rs`).
#[derive(Clone)]
pub struct Link {
    pub url: String,
    pub cli: std::path::PathBuf,
    pub socket: std::path::PathBuf,
    pub tokens: Arc<Tokens>,
    /// Serves a VM agent's connections; set once the server is up.
    pub serve: Arc<std::sync::OnceLock<relay::Serve>>,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link").field("url", &self.url).finish_non_exhaustive()
    }
}

/// What a caller may reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Everything, like the CLI: the owner, or a full client token. The MCP
    /// client's own tool permissions are the guard, which is why the tools'
    /// annotations are honest.
    Full,
    /// The read-only tools (a `read` client token).
    Read,
    /// An agent block's token: new panes and blocks in the block's own tab
    /// (on the tab's machine, in a VM tab), driving and closing what it
    /// started, and reading the rest of its tab. Nothing in other tabs.
    Block(PaneId),
}

impl Scope {
    /// The agent block whose token this is.
    pub fn block(self) -> Option<PaneId> {
        match self {
            Scope::Block(b) => Some(b),
            Scope::Full | Scope::Read => None,
        }
    }
}

/// Who is calling, as the request's checks found.
#[derive(Debug, Clone)]
pub struct Caller {
    pub scope: Scope,
    /// The token it came with, for the log (`None`: the owner's own).
    pub token: Option<String>,
    /// The caller's own pane, which a tool uses where it's left out: an
    /// agent block's own id, or for the others the pane `arugula mcp` says
    /// it runs in (`x-arugula-pane`). It only fills defaults, and is no credential:
    /// what a caller may reach is its scope's alone.
    pub pane: Option<PaneId>,
}

impl Caller {
    /// A caller with `scope`. An agent block is always its own pane, whatever
    /// `from_header` says; the others take the header's, if there was one.
    pub fn new(scope: Scope, token: Option<String>, from_header: Option<PaneId>) -> Self {
        let pane = scope.block().or(from_header);
        Caller { scope, token, pane }
    }
}

/// MCP's server (#232, `mcp_server`): `/mcp` and what goes with it, a VM
/// agent's relayed sessions, a hosted session's own listener, and the tool
/// table. What the rest of the daemon and the feature crates share with it
/// (a call, the tools' rows, the registry, the operations' half) stays here.
pub trait Mcp: Send + Sync + 'static {
    /// `/mcp`, the tokens' API and a cloud workspace's `POST /api/mcp/call`,
    /// for one of the daemon's routers.
    fn routes(&self, app: &Arc<App>) -> Router<Arc<App>>;
    /// What serves a VM agent's relayed connections: an MCP session each.
    fn pipe_server(&self, app: &Arc<App>) -> relay::Serve;
    /// A hosted session's MCP, on a listener of its own (`session.rs`).
    fn start_sessions(&self, app: Arc<App>);
    /// Whether `tool` only reads: a read-only token may call it.
    fn read_only_tool(&self, tool: &str) -> bool;
}

static MCP: OnceLock<Box<dyn Mcp>> = OnceLock::new();

/// Puts MCP's server in this daemon (once, before `main`).
pub fn install(m: Box<dyn Mcp>) {
    if MCP.set(m).is_err() {
        tracing::warn!("MCP's server was registered twice");
    }
}

/// [`Mcp::routes`]; none without the server.
pub fn routes(app: &Arc<App>) -> Router<Arc<App>> {
    MCP.get().map(|m| m.routes(app)).unwrap_or_default()
}

/// [`Mcp::pipe_server`], if there's a server.
pub fn pipe_server(app: &Arc<App>) -> Option<relay::Serve> {
    MCP.get().map(|m| m.pipe_server(app))
}

/// [`Mcp::start_sessions`]; nothing without the server.
pub fn start_sessions(app: Arc<App>) {
    match MCP.get() {
        Some(m) => m.start_sessions(app),
        None => tracing::warn!("this build has no MCP server: a session's agent gets no issue tools"),
    }
}

/// [`Mcp::read_only_tool`]; none without the server.
pub fn read_only_tool(tool: &str) -> bool {
    MCP.get().is_some_and(|m| m.read_only_tool(tool))
}
