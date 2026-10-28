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
    let server = McpServer { app: app.clone(), fallback: None };
    let service: StreamableHttpService<McpServer, LocalSessionManager> =
        StreamableHttpService::new(move || Ok(server.clone()), Default::default(), config);
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
        let server = McpServer { app, fallback: Some(caller) };
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
}

impl McpServer {
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

const INSTRUCTIONS_BASE: &str = "Arugula runs commands in durable terminal panes that the user can watch \
(on the web and the phone) and take over. capabilities says what this machine offers you (its blocks, \
the forges the user is signed in to, whether your hooks are live, control and teams): call it first. Use run to start a build or a dev server in a pane (wait: true \
to wait for it), wait and read_output to follow it (they return \"still running\" with an offset: call \
again; read_output with screen: true is what a full-screen program shows), list to see what's there (kind \
conversations: Claude Code conversations from a terminal or the desktop app), attach to put a file (a \
screenshot) into a terminal or an agent block, show to put a block in front of the user beside a pane, \
read_forge to read a PR or issue block and draft for a comment, review, merge or new issue the user sends, \
invite_person to ask the user to bring someone into the session (read_invite says what became of it), \
start_agent and agent_respond to supervise another agent, \
cloud and read_cloud for the user's Cloud workspaces in Arugula control (agents on machines in the cloud; \
list kind cloud_workspaces lists them, and you act as the user there), \
history for what happened before (kind output: what panes printed). Output is paged: pass next_offset \
back as offset. show's kinds: port (a dev server in a browser block beside its terminal), changes (a \
diff), file (at a line), pr, issue, conversation (a Claude Code conversation, to continue or fork); show \
one instead of describing it, and once you open a pull request, or one is ready for the person's review \
or merge, show it (kind pr) rather than linking it, and use draft for the merge; wait (until idle or \
needs_input) instead of polling output. \
Claude Code hooks put your questions (arugula ask), permission prompts (arugula hook, which anyone allowed can \
answer), follow-ups (arugula inbox) and attention on cards; without them your questions stay in the terminal. \
`arugula hooks install` adds them: ask your person first (capabilities says whether yours are live). \
Run each `arugula ...` command on its own, not in a pipeline or an `&&` chain, so the allow rules match it.";

/// The server's instructions, with the Doc sentence (`flags::DOCS_STEERING`,
/// which `arugula hook` also gives on `SessionStart`, #1486; everyone's since
/// Docs left Labs, native #204) and the issues sentence
/// (`flags::ISSUES_STEERING`). Chat isn't an agent's (#1095): nothing of it
/// is here.
pub fn instructions() -> String {
    let at = INSTRUCTIONS_BASE.find("Claude Code hooks put").expect("the instructions name the hooks");
    let mut out = INSTRUCTIONS_BASE[..at].to_owned();
    out.push_str(flags::DOCS_STEERING);
    out.push(' ');
    out.push_str(flags::ISSUES_STEERING);
    out.push(' ');
    out.push_str(&INSTRUCTIONS_BASE[at..]);
    out
}

impl ServerHandler for McpServer {
    /// An MCP client's session began (counted for telemetry, M101).
    async fn on_initialized(&self, _: rmcp::service::NotificationContext<RoleServer>) {
        crate::telemetry::client(arugula_control_wire::telemetry::Surface::Mcp);
    }

    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().build())
            .with_server_info(Implementation::new("arugula", arugula_core::version::get()))
            .with_instructions(instructions())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let scope = self.caller(&ctx).scope;
        Ok(fresh(ListToolsResult::with_all_items(tools::list(scope, self.flags()))))
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
        let call = Call::new(&self.app, caller, client, &ctx);
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
