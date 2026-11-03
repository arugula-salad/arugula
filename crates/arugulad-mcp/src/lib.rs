//! MCP's server (M16, #232): `/mcp` (rmcp's Streamable HTTP, behind the
//! owner's checks or a token), a VM agent's relayed sessions, a hosted
//! session's own listener ([`session`]) and a cloud workspace's `POST
//! /api/mcp/call` ([`hosted`]), with the tool table and the tools' calls
//! ([`tools`]) and `capabilities`. The base's `mcp` has the rest: who's
//! calling, a call ([`Call`](crate::mcp::tools::Call)), a tool's row, the
//! registry and the operations' half, which the base's operations and the
//! feature crates share. The base reaches this through `mcp::Mcp`, and
//! the card *What your agent can do here* is `AgentCanGet`, an operation
//! of ours ([`Server`]); the binary calls [`register`].
//!
//! This is the crate `arugulad-mcp` (#232), beside the other feature crates
//! on the base: the base's `mcp` is what `arugulad-control-tools` and
//! `arugulad-agent` use, so neither is built on this.

mod capabilities;
mod hosted;
mod listing;
mod results;
mod session;
pub mod tools;

// What the code here names as `crate::…`: the mux's modules and the daemon's,
// as it named them when it was the daemon's.
use arugula_mux::{acl, block, history, keys, mux, osc, pane, store};
#[cfg(unix)]
use arugulad_base::upload;
use arugulad_base::{
    agent, api, features, forges, fs, lab_flags, lab_hooks, mcp, ops, review, server, task, telemetry, work, worktree,
};

use std::sync::Arc;

use arugula_proto::{
    BlockType, PaneId,
    api::{AgentCan, Empty},
    flags::{self, On},
    op::ops::AgentCanGet,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    model::{
        CacheScope, CallToolRequestParams, CallToolResponse, Implementation, ListResourceTemplatesResult,
        ListResourcesResult, ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse,
        ReadResourceResult, Resource, ResourceContents, ResourceTemplate, ServerCapabilities, ServerConfig,
    },
    service::RequestContext,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use tracing::info;

use self::tools::ToolCall;
use crate::{
    mcp::{
        Caller, PATH, Scope, relay,
        tokens::{self, Bearer, TokenScope},
        tools::Call,
    },
    mux::Api,
    ops::{Cx, Handle, OpError, OpRoutes},
    server::App,
};

/// MCP's server, as the daemon reaches it (`mcp::Mcp`), and as a feature
/// (`AgentCanGet` is its operation).
pub struct Server;

/// Puts MCP's server in this daemon: the binary calls it before `main`.
pub fn register() {
    crate::mcp::install(Box::new(Server));
    crate::features::register(Box::new(Server));
}

impl crate::mcp::Mcp for Server {
    fn routes(&self, app: &Arc<App>) -> Router<Arc<App>> {
        routes(app)
    }

    fn pipe_server(&self, app: &Arc<App>) -> relay::Serve {
        pipe_server(app)
    }

    fn start_sessions(&self, app: Arc<App>) {
        session::start(app);
    }

    fn read_only_tool(&self, tool: &str) -> bool {
        tools::read_only_tool(tool)
    }
}

impl crate::features::Feature for Server {
    fn routes(&self, r: Router<Arc<App>>) -> Router<Arc<App>> {
        r.op_of::<AgentCanGet, Server>()
    }
}

/// The card *What your agent can do here* (M98).
impl Handle<Server> for AgentCanGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Vec<AgentCan>, OpError> {
        Ok(capabilities::can_do(cx.app).await)
    }
}

/// `/mcp`, for one of the daemon's routers.
pub fn routes(app: &Arc<App>) -> Router<Arc<App>> {
    let config = StreamableHttpServerConfig::default().disable_allowed_hosts();
    let served = app.clone();
    // A server of its own for each MCP session: it keeps what that session
    // was last told it has.
    let service: StreamableHttpService<McpServer, LocalSessionManager> =
        StreamableHttpService::new(move || Ok(McpServer::new(served.clone(), None)), Default::default(), config);
    Router::new()
        .nest_service(PATH, service)
        .layer(middleware::from_fn_with_state(app.clone(), authenticate))
        .merge(tokens::api_routes())
        .merge(hosted::routes())
}

/// What serves a VM agent's relayed connections: an MCP session each, on
/// stdio framing, scoped as the block's token would be over HTTP.
pub fn pipe_server(app: &Arc<App>) -> relay::Serve {
    let app = Arc::downgrade(app);
    Arc::new(move |id, io| {
        let Some(app) = app.upgrade() else { return };
        let caller = Caller::new(Scope::Block(id), Some(format!("%{id}")), None);
        let server = McpServer::new(app, Some(caller));
        crate::task::spawn(async move {
            match rmcp::ServiceExt::serve(server, tokio::io::split(io)).await {
                Ok(running) => {
                    let _ = running.waiting().await;
                }
                Err(e) => tracing::debug!(block = id, error = %e, "relayed MCP session didn't start"),
            }
        });
    })
}

fn refuse(status: StatusCode, why: &str) -> Response {
    (status, Json(serde_json::json!({ "error": why }))).into_response()
}

/// Whose request this is: a bearer token's holder, or the owner (who got
/// past the server's checks, or is on the socket).
async fn authenticate(State(app): State<Arc<App>>, mut req: Request, next: Next) -> Response {
    // Any Authorization at all skipped the identity check (`server.rs`):
    // it must be one of our tokens, never the owner by default.
    let bearer = match req.headers().get(header::AUTHORIZATION) {
        None => None,
        Some(v) => {
            match v.to_str().ok().and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer "))) {
                Some(t) => Some(t.trim().to_owned()),
                None => return refuse(StatusCode::UNAUTHORIZED, "MCP takes a bearer token (Authorization: Bearer …)"),
            }
        }
    };
    let pane = req
        .headers()
        .get("x-arugula-pane")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().trim_start_matches('%').parse::<PaneId>().ok());
    let caller = match bearer {
        None => Caller::new(Scope::Full, None, pane),
        // The daemon's local token: the owner, as the server already found.
        Some(t) if app.access.is_local_token(&t) => Caller::new(Scope::Full, None, pane),
        Some(t) => match app.mcp.check(&t) {
            Some(Bearer::Client { name, scope }) => {
                let scope = match scope {
                    TokenScope::Full => Scope::Full,
                    TokenScope::Read => Scope::Read,
                };
                Caller::new(scope, Some(name), pane)
            }
            Some(Bearer::Block(id)) => match app.mux.api(|r| Api::Block(id, r)).await.flatten() {
                Some(b) if b.kind() == BlockType::Agent => Caller::new(Scope::Block(id), Some(format!("%{id}")), pane),
                _ => return refuse(StatusCode::UNAUTHORIZED, &format!("agent block %{id} is gone; its token with it")),
            },
            None => return refuse(StatusCode::UNAUTHORIZED, "unknown or revoked MCP token"),
        },
    };
    req.extensions_mut().insert(caller);
    next.run(req).await
}

/// The server one MCP session (or one stateless request) talks to.
#[derive(Clone)]
pub struct McpServer {
    app: Arc<App>,
    /// Who it serves when there's no HTTP request to say (a pipe).
    fallback: Option<Caller>,
    /// What this session was last told it has (#370).
    session: Arc<listing::Session>,
}

impl McpServer {
    fn new(app: Arc<App>, fallback: Option<Caller>) -> Self {
        let session = listing::Session::open(app.control.state_dir().to_owned());
        Self { app, fallback, session }
    }

    /// This machine's Labs flags: read on each call, so a change needs no
    /// restart.
    fn flags(&self) -> On {
        crate::lab_flags::flags(self.app.control.state_dir())
    }

    fn caller(&self, ctx: &RequestContext<RoleServer>) -> Caller {
        ctx.extensions
            .get::<axum::http::request::Parts>()
            .and_then(|p| p.extensions.get::<Caller>().cloned())
            .or_else(|| self.fallback.clone())
            // Never reached through `/mcp`; the least, to be safe.
            .unwrap_or(Caller::new(Scope::Read, Some("unknown".into()), None))
    }
}

/// What list and resource results carry: Claude Code 2.1.287 rejects them
/// without (S14 saw it for resources; `tools/list` too, at 2026-07-28).
fn fresh<T: CacheHints>(r: T) -> T {
    r.hints()
}

trait CacheHints {
    fn hints(self) -> Self;
}

impl CacheHints for ListToolsResult {
    fn hints(self) -> Self {
        self.with_ttl_ms(0).with_cache_scope(CacheScope::Private)
    }
}

impl CacheHints for ListResourcesResult {
    fn hints(self) -> Self {
        self.with_ttl_ms(0).with_cache_scope(CacheScope::Private)
    }
}

impl CacheHints for ListResourceTemplatesResult {
    fn hints(self) -> Self {
        self.with_ttl_ms(0).with_cache_scope(CacheScope::Private)
    }
}

impl CacheHints for ReadResourceResult {
    fn hints(self) -> Self {
        self.with_ttl_ms(0).with_cache_scope(CacheScope::Private)
    }
}

/// The first sentence: what Arugula is, and where to start.
const INSTRUCTIONS_HEAD: &str = "Arugula runs commands in durable terminal panes that the user can watch \
(on the web and the phone) and take over. capabilities says what this machine offers you: call it first.";

/// The tool tour, short: each tool's own description says what it does.
const INSTRUCTIONS_TOUR: &str = "wait (until idle or needs_input), \
don't poll; read_output pages (pass next_offset as offset). show a block, don't describe it; \
once you open a pull request, or one is ready for review or merge, show it (kind pr), not a link; \
use draft for the merge.";

/// The hooks paragraph, last.
const INSTRUCTIONS_HOOKS: &str = "Claude Code hooks put your questions (arugula ask), permission prompts (arugula hook), \
follow-ups (arugula inbox) and attention on cards; without them questions stay in the terminal. \
`arugula hooks install` adds them: ask first. \
Run each `arugula ...` command alone, not piped or `&&`-chained, to match allow rules.";

/// The server's instructions. Claude Code shows an agent only their first
/// 2048 characters (#606), so the steering comes first, then a short tour
/// and the hooks: the Doc sentence (`flags::DOCS_STEERING`, which
/// `arugula hook` also gives on `SessionStart`, #1486; everyone's since
/// Docs left Labs, native #204), the issues sentence
/// (`flags::ISSUES_STEERING`) and the boards sentence
/// (`flags::BOARDS_STEERING`, which says nothing to do where boards are
/// off, and does no harm). Chat isn't in the instructions (#1095): how to
/// reach a teammate's agent is in the thread tools' descriptions (#574).
pub fn instructions() -> String {
    [
        INSTRUCTIONS_HEAD,
        flags::DOCS_STEERING,
        flags::ISSUES_STEERING,
        flags::BOARDS_STEERING,
        INSTRUCTIONS_TOUR,
        INSTRUCTIONS_HOOKS,
    ]
    .join(" ")
}

impl ServerHandler for McpServer {
    /// An MCP client's session began (counted for telemetry, M101).
    async fn on_initialized(&self, _: rmcp::service::NotificationContext<RoleServer>) {
        crate::telemetry::client(arugula_control_wire::telemetry::Surface::Mcp);
    }

    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder().enable_tools().enable_tool_list_changed().enable_resources().build(),
        )
        .with_server_info(Implementation::new("arugula", arugula_core::version::get()))
        .with_instructions(instructions())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let scope = self.caller(&ctx).scope;
        let on = self.flags();
        self.session.listing(ctx.peer.clone(), scope, tools::listed_names(scope, on));
        Ok(fresh(ListToolsResult::with_all_items(tools::list(scope, on))))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let caller = self.caller(&ctx);
        let client = ctx
            .client_info()
            .map(|i| i.name)
            .filter(|n| !n.trim().is_empty())
            .or_else(|| caller.token.clone())
            .unwrap_or_else(|| "client".into());
        info!(
            tool = %request.name,
            client,
            token = caller.token.as_deref().unwrap_or("owner"),
            scope = ?caller.scope,
            "mcp call"
        );
        let args = request.arguments.map(serde_json::Value::Object).unwrap_or_else(|| serde_json::json!({}));
        let call = Call::new(&self.app, caller, client, &ctx).in_session(self.session.listed.clone());
        Ok(ToolCall(&call).dispatch(&request.name, args).await.into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let mut history = Resource::new("arugula://history", "history");
        history.description = Some("The last commands across panes, with exit codes and who ran them".into());
        history.mime_type = Some("application/json".into());
        Ok(fresh(ListResourcesResult::with_all_items(vec![history])))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        let t = |uri: &str, name: &str, what: &str| {
            let mut t = ResourceTemplate::new(uri, name);
            t.description = Some(what.into());
            t
        };
        Ok(fresh(ListResourceTemplatesResult::with_all_items(vec![
            t("arugula://pane/{id}/output", "pane output", "A pane's latest output, escape sequences stripped"),
            t("arugula://pane/{id}/screen", "pane screen", "What a pane shows now, as text"),
            t("arugula://block/{id}", "block", "A pane's or block's state, as JSON"),
        ])))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let caller = self.caller(&ctx);
        let call = Call::new(&self.app, caller, "resource".into(), &ctx);
        let text = ToolCall(&call).resource(&request.uri).await.map_err(|e| McpError::resource_not_found(e, None))?;
        Ok(fresh(ReadResourceResult::new(vec![ResourceContents::text(text, request.uri)])).into())
    }
}
