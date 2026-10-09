//! Operations (#451, #572): the daemon's half of each declaration in
//! `arugula_proto::op`. An operation is handled once ([`Handle`]); its HTTP
//! route ([`OpRoutes::op`]), its line in the access check ([`policy`]) and,
//! where it has one, its MCP tool or kind (`mcp/ops.rs`) come from that.
//!
//! The handler runs the same for every surface. Where the surfaces differ on
//! purpose, it asks [`Cx`]: who is calling and by which way in (an access
//! check an MCP agent's token needs and HTTP's middleware already made), and
//! its errors ([`OpError`]) are worded by each surface as before. The design
//! is in `docs/operations.md`.

mod acl;
mod close;
mod conversations;
mod flags;
mod fountain;
mod fs;
mod hosts;
mod input;
mod inspect;
mod invite;
mod notify;
mod panes;
pub(crate) mod prompt;
mod rules;
mod sandboxes;
mod settings;
mod setup;
mod shares;
mod shell_env;
mod signin;
mod studio;
mod synced;
mod threads;
mod tokens;
mod wait;
mod waits;

use std::sync::Arc;

use arugula_core::Role;
use arugula_proto::{
    PaneId,
    op::{Access, Method, Op, Request},
};
use axum::{
    Json, Router,
    extract::{FromRequest, FromRequestParts, Path, Query, State},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
    routing::{MethodRouter, delete, get, post, put},
};

use crate::{acl::Principal, api::ApiError, mcp::tools::Call, server::App};

/// Every operation, in one place: the access check and the MCP tool list
/// look each one up here.
macro_rules! every_op {
    ($m:ident) => {
        $m!(arugula_proto::op::ops::ListPanes);
        $m!(arugula_proto::op::ops::ClosePane);
        $m!(arugula_proto::op::ops::ShellEnvGet);
        $m!(arugula_proto::op::ops::ShellEnvRefresh);
        $m!(arugula_proto::op::ops::FlagsList);
        $m!(arugula_proto::op::ops::FlagSet);
        $m!(arugula_proto::op::ops::RulesList);
        $m!(arugula_proto::op::ops::RulesForgetAll);
        $m!(arugula_proto::op::ops::RuleForget);
        $m!(arugula_proto::op::ops::PaneWait);
        $m!(arugula_proto::op::ops::PaneSend);
        $m!(arugula_proto::op::ops::PaneKeys);
        $m!(arugula_proto::op::ops::PaneMouse);
        $m!(arugula_proto::op::ops::PaneAttention);
        $m!(arugula_proto::op::ops::PaneFollowUp);
        $m!(arugula_proto::op::ops::PanePrompt);
        $m!(arugula_proto::op::ops::PaneAsk);
        $m!(arugula_proto::op::ops::PaneAskWithdraw);
        $m!(arugula_proto::op::ops::PanePermit);
        $m!(arugula_proto::op::ops::PaneInbox);
        $m!(arugula_proto::op::ops::PaneProcess);
        $m!(arugula_proto::op::ops::PaneDetection);
        $m!(arugula_proto::op::ops::PaneDiffOf);
        $m!(arugula_proto::op::ops::PaneDrivers);
        $m!(arugula_proto::op::ops::IdeGet);
        $m!(arugula_proto::op::ops::IdeSet);
        $m!(arugula_proto::op::ops::IdeMention);
        $m!(arugula_proto::op::ops::AgentsGet);
        $m!(arugula_proto::op::ops::AgentsRefresh);
        $m!(arugula_proto::op::ops::AdaptersList);
        $m!(arugula_proto::op::ops::AdapterInstall);
        $m!(arugula_proto::op::ops::ConversationsList);
        $m!(arugula_proto::op::ops::ConversationOpen);
        $m!(arugula_proto::op::ops::MachinesList);
        $m!(arugula_proto::op::ops::MachineReset);
        $m!(arugula_proto::op::ops::PushKeyGet);
        $m!(arugula_proto::op::ops::PushSubscribe);
        $m!(arugula_proto::op::ops::PushTest);
        $m!(arugula_proto::op::ops::NotifyGet);
        $m!(arugula_proto::op::ops::NotifySet);
        $m!(arugula_proto::op::ops::ThreadGet);
        $m!(arugula_proto::op::ops::ThreadPost);
        $m!(arugula_proto::op::ops::ThreadRead);
        $m!(arugula_proto::op::ops::FountainAgentsGet);
        // #578 part a: fs, hosts, setup.
        $m!(arugula_proto::op::ops::FsListGet);
        $m!(arugula_proto::op::ops::FsStatGet);
        $m!(arugula_proto::op::ops::FsRecentGet);
        $m!(arugula_proto::op::ops::PaneCd);
        $m!(arugula_proto::op::ops::HostGet);
        $m!(arugula_proto::op::ops::HostsList);
        $m!(arugula_proto::op::ops::HostAdd);
        $m!(arugula_proto::op::ops::HostRemove);
        $m!(arugula_proto::op::ops::HostTokenMint);
        $m!(arugula_proto::op::ops::HostTokenRevoke);
        $m!(arugula_proto::op::ops::SetupGet);
        $m!(arugula_proto::op::ops::SetupTailscale);
        $m!(arugula_proto::op::ops::SetupControl);
        $m!(arugula_proto::op::ops::SetupControlConfirm);
        $m!(arugula_proto::op::ops::SetupClaude);
        $m!(arugula_proto::op::ops::SetupAgent);
        // #578 part b: access and credentials.
        $m!(arugula_proto::op::ops::SharesList);
        $m!(arugula_proto::op::ops::ShareMint);
        $m!(arugula_proto::op::ops::ShareRevoke);
        $m!(arugula_proto::op::ops::GuestsList);
        $m!(arugula_proto::op::ops::GuestMint);
        $m!(arugula_proto::op::ops::GuestRevoke);
        $m!(arugula_proto::op::ops::AclGet);
        $m!(arugula_proto::op::ops::AclSet);
        $m!(arugula_proto::op::ops::LinkMint);
        $m!(arugula_proto::op::ops::InviteSend);
        $m!(arugula_proto::op::ops::TeamPinsGet);
        $m!(arugula_proto::op::ops::TeamPinsSet);
        $m!(arugula_proto::op::ops::McpTokensList);
        $m!(arugula_proto::op::ops::McpTokenMint);
        $m!(arugula_proto::op::ops::McpTokenRevoke);
        $m!(arugula_proto::op::ops::SigninLinkGet);
        // #578 part c: synced history, sandboxes, studio.
        $m!(arugula_proto::op::ops::SyncedList);
        $m!(arugula_proto::op::ops::SyncedRotate);
        $m!(arugula_proto::op::ops::SyncedForget);
        $m!(arugula_proto::op::ops::SandboxesList);
        $m!(arugula_proto::op::ops::SandboxPromote);
        $m!(arugula_proto::op::ops::SandboxDemote);
        $m!(arugula_proto::op::ops::StudioGet);
        $m!(arugula_proto::op::ops::StudioLogin);
        $m!(arugula_proto::op::ops::StudioLogout);
        $m!(arugula_proto::op::ops::StudioAppsList);
        $m!(arugula_proto::op::ops::StudioFollow);
        $m!(arugula_proto::op::ops::StudioUnfollow);
    };
}

/// Why an operation didn't happen. Each surface words it: HTTP as a status
/// and the sentence it always sent, MCP as a sentence an agent can act on.
#[derive(Debug)]
pub enum OpError {
    /// No such pane: HTTP's 404 "no pane %N"; MCP says what became of it.
    NoPane(PaneId),
    /// Not this caller's to do (403).
    Forbidden(String),
    /// No such flag (404), with the sentence that names the ones there are.
    NoFlag(String),
    /// The state dir couldn't be written (500).
    Failed(String),
    /// What an MCP agent's token doesn't reach. HTTP's middleware checks
    /// before a handler runs, so it never sees one.
    Unreachable(String),
    /// Any other refusal, with the status and sentence its handler had before
    /// it was an operation (a pane that closed while waited on, 410).
    Status(StatusCode, String),
    /// A refusal whose body is plain text, not `{"error": …}`, as its
    /// handler sent it before it was an operation (HTTP only: the other
    /// surfaces word it as `Status`).
    Text(StatusCode, String),
}

impl From<ApiError> for OpError {
    fn from(ApiError(code, why): ApiError) -> Self {
        OpError::Status(code, why)
    }
}

impl OpError {
    pub fn http(self) -> ApiError {
        match self {
            OpError::NoPane(id) => ApiError(StatusCode::NOT_FOUND, format!("no pane %{id}")),
            OpError::Forbidden(why) | OpError::Unreachable(why) => ApiError(StatusCode::FORBIDDEN, why),
            OpError::NoFlag(why) => ApiError(StatusCode::NOT_FOUND, why),
            OpError::Failed(why) => ApiError(StatusCode::INTERNAL_SERVER_ERROR, why),
            OpError::Status(code, why) | OpError::Text(code, why) => ApiError(code, why),
        }
    }
}

/// Which way a call came in.
pub enum Via<'a> {
    /// An HTTP request, past `authz::check`: who asks (and whether that's
    /// the owner), and whether the owner's CLI says an agent runs it.
    Http { who: Principal, owner: bool, agent: bool },
    /// An MCP tool call, with its caller's scope.
    Mcp(&'a Call<'a>),
}

/// What a handler knows about the call.
pub struct Cx<'a> {
    pub app: &'a Arc<App>,
    pub via: Via<'a>,
}

impl Cx<'_> {
    /// What `access` means for this caller on `pane`, where the way in
    /// hasn't checked it: an MCP agent block's token reads its own tab and
    /// drives what it started. Over HTTP, `authz::check` already did.
    pub async fn may(&self, access: Access, pane: PaneId) -> Result<(), OpError> {
        let Via::Mcp(call) = &self.via else { return Ok(()) };
        let r = match access {
            Access::Pane(Role::Viewer) => call.readable(pane).await.map(|_| ()),
            Access::Pane(_) => call.drivable(pane).await.map(|_| ()),
            Access::Owner | Access::Handler | Access::Anyone => Ok(()),
        };
        r.map_err(OpError::Unreachable)
    }

    /// Who asks, for what is recorded as theirs or shown by what they may
    /// see. Only HTTP knows: an MCP caller is an agent's token, not a person.
    pub fn principal(&self) -> Result<Principal, OpError> {
        match &self.via {
            Via::Http { who, .. } => Ok(who.clone()),
            Via::Mcp(_) => Err(OpError::Unreachable("that is for a person, not an agent's token".into())),
        }
    }

    /// An agent, or someone who isn't the owner: what the owner keeps for
    /// themselves (an invite block, #234) refuses them. Every MCP caller is
    /// an agent.
    pub fn agent_or_guest(&self) -> bool {
        match self.via {
            Via::Http { owner, agent, .. } => !owner || agent,
            Via::Mcp(_) => true,
        }
    }
}

/// The daemon's half of an operation: what it does, once for every way in.
pub trait Handle: Op {
    fn handle(cx: &Cx<'_>, path: Self::Path, req: Self::Req)
    -> impl Future<Output = Result<Self::Res, OpError>> + Send;
}

/// A route's path segment, as axum extracts it: nothing, a pane (with
/// axum's own answer to one that isn't a number, as before) or a name.
pub trait FromPath: Sized {
    fn from_parts(parts: &mut Parts) -> impl Future<Output = Result<Self, Response>> + Send;
}

impl FromPath for () {
    async fn from_parts(_: &mut Parts) -> Result<Self, Response> {
        Ok(())
    }
}

/// A pane, or a session (the same `u32`).
impl FromPath for PaneId {
    async fn from_parts(parts: &mut Parts) -> Result<Self, Response> {
        Path::<PaneId>::from_request_parts(parts, &()).await.map(|Path(id)| id).map_err(IntoResponse::into_response)
    }
}

impl FromPath for usize {
    async fn from_parts(parts: &mut Parts) -> Result<Self, Response> {
        Path::<usize>::from_request_parts(parts, &()).await.map(|Path(i)| i).map_err(IntoResponse::into_response)
    }
}

/// `pane-7` or `session-2`; anything else is the 404 the thread routes gave.
impl FromPath for arugula_proto::ThreadTarget {
    async fn from_parts(parts: &mut Parts) -> Result<Self, Response> {
        let Path(key) = Path::<String>::from_request_parts(parts, &()).await.map_err(IntoResponse::into_response)?;
        Self::parse(&key)
            .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "a thread is pane-N or session-N".into()).into_response())
    }
}

impl FromPath for String {
    async fn from_parts(parts: &mut Parts) -> Result<Self, Response> {
        Path::<String>::from_request_parts(parts, &()).await.map(|Path(s)| s).map_err(IntoResponse::into_response)
    }
}

/// An operation's HTTP handler: the path, the body (or none), who asks, the
/// owner's check for an owner's operation (`authz::check` makes it first;
/// this is the handler's own, as `owner_only` was), then the handler.
async fn serve<O>(State(app): State<Arc<App>>, req: axum::extract::Request) -> Response
where
    O: Handle,
    O::Path: FromPath,
{
    let (mut parts, body) = req.into_parts();
    let path = match O::Path::from_parts(&mut parts).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let who = parts.extensions.get::<Principal>().cloned().unwrap_or(Principal::Owner);
    let owner = who.is_owner();
    let agent = crate::invite::agent(&parts.headers);
    let body = match <O::Req as Request>::without_body() {
        Some(none) => none,
        // A GET's request is its query string.
        None if O::METHOD == Method::Get => match Query::<O::Req>::try_from_uri(&parts.uri) {
            Ok(Query(q)) => q,
            Err(r) => return r.into_response(),
        },
        None => {
            let req = axum::extract::Request::from_parts(parts, body);
            match <O::Req as Request>::absent() {
                // A body that may be left off (`Content-Type` and all).
                Some(none) => match Option::<Json<O::Req>>::from_request(req, &()).await {
                    Ok(b) => b.map_or(none, |Json(b)| b),
                    Err(r) => return r.into_response(),
                },
                None => match Json::<O::Req>::from_request(req, &()).await {
                    Ok(Json(b)) => b,
                    Err(r) => return r.into_response(),
                },
            }
        }
    };
    if O::ACCESS == Access::Owner && !owner {
        return ApiError(StatusCode::FORBIDDEN, "only the owner can".into()).into_response();
    }
    let cx = Cx { app: &app, via: Via::Http { who, owner, agent } };
    match O::handle(&cx, path, body).await {
        Ok(res) => Json(res).into_response(),
        Err(OpError::Text(code, why)) => (code, why).into_response(),
        Err(e) => e.http().into_response(),
    }
}

/// Routes for operations, beside the hand-written ones.
pub trait OpRoutes {
    /// `O`'s route, at its path and method.
    fn op<O>(self) -> Self
    where
        O: Handle,
        O::Path: FromPath;
}

impl OpRoutes for Router<Arc<App>> {
    fn op<O>(self) -> Self
    where
        O: Handle,
        O::Path: FromPath,
    {
        let m: MethodRouter<Arc<App>> = match O::METHOD {
            Method::Get => get(serve::<O>),
            Method::Post => post(serve::<O>),
            Method::Put => put(serve::<O>),
            Method::Delete => delete(serve::<O>),
        };
        self.route(O::PATH, m)
    }
}

/// Whether `parts` (a path's decoded segments) are the route `template`
/// (`{…}` matches any one segment), and the segment it matched, if any.
fn matches<'p>(template: &str, parts: &[&'p str]) -> Option<Option<&'p str>> {
    let t: Vec<&str> = template.trim_start_matches('/').split('/').collect();
    if t.len() != parts.len() {
        return None;
    }
    let mut hole = None;
    for (t, p) in t.iter().zip(parts) {
        if t.starts_with('{') && t.ends_with('}') {
            hole = Some(*p);
        } else if t != p {
            return None;
        }
    }
    Some(hole)
}

/// What an operation's route needs from someone who isn't the owner, for
/// `authz::check`: `None` for a route that isn't an operation. It matches
/// the path's segments decoded, as `authz::policy` does and the router does
/// for the ones it hands a handler, so `/api/panes/%31/close` is pane 1's.
/// A segment that isn't UTF-8 once decoded is `None` too, and `authz` gives
/// that the owner.
pub fn policy(method: &axum::http::Method, path: &str) -> Option<crate::authz::Policy> {
    use crate::authz::Policy;
    let parts = crate::authz::segments(path)?;
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    let mut found = None;
    macro_rules! look {
        ($o:ty) => {
            if found.is_none()
                && method.as_str() == <$o as Op>::METHOD.as_str()
                && let Some(hole) = matches(<$o as Op>::PATH, &parts)
            {
                let pane = || hole.and_then(|s| s.parse::<PaneId>().ok());
                found = Some(match <$o as Op>::ACCESS {
                    Access::Owner => Policy::Owner,
                    Access::Anyone => Policy::Anyone,
                    Access::Handler => Policy::Handler,
                    Access::Pane(role) => pane().map_or(Policy::Owner, |p| Policy::On(p, role)),
                });
            }
        };
    }
    every_op!(look);
    found
}

/// The routes that drive a pane and are not operations (`Op::DRIVES` is how
/// an operation says it does), as `POST` templates: `authz::check` asks the
/// mux whether an editor may drive the pane for these too (M14).
const DRIVES_HAND_WRITTEN: &[&str] = &[
    // M70: writes a file where the pane runs.
    "/api/panes/{id}/upload",
    // Typing a path into the pane (with upload).
    "/api/panes/{id}/paste",
    // A block call is any method of any block: on a pane that isn't a block,
    // `send` and `keys` type into it (`api::call`), and `mouse`, `followup`,
    // `upload` and `paste` were driving by their name before DRIVES (#574).
    "/api/blocks/{id}/call/send",
    "/api/blocks/{id}/call/keys",
    "/api/blocks/{id}/call/mouse",
    "/api/blocks/{id}/call/followup",
    "/api/blocks/{id}/call/upload",
    "/api/blocks/{id}/call/paste",
];

/// Whether `method path` types into a pane, so needs the owner's trust on
/// their machine for an editor to call it (M14): an operation with `DRIVES`,
/// or a hand-written route on `DRIVES_HAND_WRITTEN`. Matched on the decoded
/// segments, as `policy` is. A route that merely ends in `send` isn't one.
pub fn drives(method: &axum::http::Method, path: &str) -> bool {
    let Some(parts) = crate::authz::segments(path) else { return false };
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    let mut found = false;
    macro_rules! look {
        ($o:ty) => {
            found |= <$o as Op>::DRIVES
                && method.as_str() == <$o as Op>::METHOD.as_str()
                && matches(<$o as Op>::PATH, &parts).is_some();
        };
    }
    every_op!(look);
    found || (method == axum::http::Method::POST && DRIVES_HAND_WRITTEN.iter().any(|t| matches(t, &parts).is_some()))
}

/// Whether a read-only MCP token may call `O`: only a GET reads, unless its
/// answer grants access (`Op::CREDENTIAL`).
pub const fn read_only<O: Op>() -> bool {
    matches!(O::METHOD, Method::Get) && !O::CREDENTIAL
}

/// The routers whose routes are checked: each file, and the start of the
/// function that builds its router. An operation is added on the router its
/// route was on, so it keeps that router's place in `server.rs` (which
/// ways in reach it) and its layers (#578).
#[cfg(test)]
const ROUTERS: &[(&str, &str)] = &[
    ("src/api.rs", "pub fn routes()"),
    // #578 part a: fs, hosts, setup.
    ("src/fs.rs", "pub fn routes()"),
    ("src/hosts.rs", "pub fn routes()"),
    ("src/setup.rs", "pub fn routes()"),
    // #578 part b: access and credentials.
    ("src/share.rs", "pub fn api_routes()"),
    // Nested in `mod api`: its function ends at its own indent.
    ("src/acl.rs", "    pub fn routes()"),
    ("src/invite/mod.rs", "pub fn routes()"),
    ("src/mcp/tokens.rs", "pub fn api_routes()"),
    ("src/labs/guest_ssh.rs", "pub fn routes()"),
    // The Unix socket's router: the only one with the sign-in link.
    ("src/server.rs", "pub fn local_router("),
    // #578 part c.
    ("src/sync.rs", "pub fn routes()"),
    ("src/labs/resident.rs", "pub fn routes()"),
    ("src/labs/apps/routes.rs", "pub fn routes("),
];

/// The routes in [`ROUTERS`] that are not operations, each with why. A new
/// route is an operation unless it is here (`every_api_route_is_an_operation_or_hand_written`).
/// Entries leave as their operations arrive (#575–#578).
#[cfg(test)]
const HAND_WRITTEN: &[(&str, &str)] = &[
    // Streams, text and bytes: the answer isn't one JSON value.
    ("/api/events", "NDJSON stream"),
    ("/api/panes/{id}/tail", "follows as a stream"),
    ("/api/panes/{id}/capture", "plain text"),
    ("/api/panes/{id}/export.cast", "asciicast"),
    ("/api/editors/vsix", "binary download"),
    ("/api/panes/{id}/upload", "raw bytes, its own body limit (drives: DRIVES_HAND_WRITTEN)"),
    ("/api/panes/{id}/paste", "types a path into the pane (drives: DRIVES_HAND_WRITTEN)"),
    // Block calls: the arguments depend on the block's type and method (#459).
    ("/api/blocks/{id}/call/{method}", "block calls, decided per method"),
    ("/api/panes/{id}/hook", "hook input, not yet converted"),
    ("/api/panes/{id}/share-machine", "not yet converted, #575"),
    // Other path types and GET queries (#573).
    ("/api/history", "GET with a query"),
    ("/api/search", "GET with a query"),
    ("/api/sessions/{id}/secrets", "other path types, #573"),
    // Not asked for in #575's list; left.
    ("/api/editors", "settings, #575"),
    // Not yet converted for other reasons.
    ("/api/run", "composes several calls"),
    ("/api/turn", "not yet converted"),
    ("/api/attention", "not yet converted"),
    ("/api/attention/act", "not yet converted"),
    ("/api/blocks", "not yet converted"),
    ("/api/blocks/{id}", "not yet converted"),
    // #578 part a: fs, hosts, setup.
    ("/api/fs/read", "bytes, with the file's size in headers"),
    ("/api/fs/watch", "NDJSON stream"),
    ("/api/hosts/invite", "a POST whose ttl is in the query, not a body"),
    ("/api/hosts/join", "the token in the body is the credential: no user identity (server::guard)"),
    // #578 part b: `local_router`'s own routes that stay as they are.
    ("/ws", "websocket"),
    ("/api/editors/connect", "websocket, editors join the swarm (M28)"),
    ("/api/daemon/stop", "stops the daemon, over the socket only"),
    // #578 part c: daemon-to-daemon pushes, a host token's, not a person's.
    ("/api/sync/state", "a pushing host's token is the credential (server::guard); not a person's call"),
    ("/api/sync/{pane}/log", "a host's push of raw bytes, 1 MB at most"),
    ("/api/sync/{pane}/index", "a host's push of raw bytes, 1 MB at most"),
    ("/api/sync/{pane}/closed", "a host's push, authorized by its host token"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(path: &str) -> Vec<&str> {
        path.trim_start_matches('/').split('/').collect()
    }

    #[test]
    fn routes_match_their_templates() {
        assert_eq!(matches("/api/panes/{id}/close", &parts("/api/panes/3/close")), Some(Some("3")));
        assert_eq!(matches("/api/panes", &parts("/api/panes")), Some(None));
        assert_eq!(matches("/api/panes/{id}/close", &parts("/api/panes/3/send")), None);
        assert_eq!(matches("/api/panes", &parts("/api/panes/3")), None);
    }

    #[test]
    fn each_operation_is_its_own_policy() {
        use crate::authz::Policy;
        let (g, p) = (axum::http::Method::GET, axum::http::Method::POST);
        assert_eq!(policy(&p, "/api/panes/3/close"), Some(Policy::On(3, Role::Editor)));
        assert_eq!(policy(&p, "/api/panes/x/close"), Some(Policy::Owner));
        assert_eq!(policy(&g, "/api/panes/3/close"), None);
        assert_eq!(policy(&g, "/api/panes"), Some(Policy::Owner));
        assert_eq!(policy(&g, "/api/hosts/self/shell-env"), Some(Policy::Owner));
        assert_eq!(policy(&p, "/api/hosts/self/shell-env/refresh"), Some(Policy::Owner));
        assert_eq!(policy(&g, "/api/panes/3/capture"), None);
    }

    /// The policy is decided on what the router hands the handler (4a974df),
    /// not on the raw path: an encoded path is the plain one.
    #[test]
    fn an_encoded_path_has_its_plain_paths_policy() {
        use crate::authz::Policy;
        let p = axum::http::Method::POST;
        assert_eq!(policy(&p, "/api/panes/%33/close"), policy(&p, "/api/panes/3/close"));
        assert_eq!(policy(&p, "/api/panes/%33/close"), Some(Policy::On(3, Role::Editor)));
        // Owner operations, encoded in the literal segments.
        assert_eq!(policy(&p, "/api/hosts/self/shell-env/%72efresh"), Some(Policy::Owner));
        assert_eq!(policy(&axum::http::Method::GET, "/api/%70anes"), Some(Policy::Owner));
        // A segment that isn't UTF-8 once decoded isn't matched: authz gives the owner.
        assert_eq!(policy(&p, "/api/panes/%ff/close"), None);
    }

    /// One row per operation, from its declaration.
    fn rows() -> Vec<[String; 7]> {
        let mut rows = Vec::new();
        macro_rules! row {
            ($o:ty) => {
                rows.push([
                    <$o as Op>::NAME.to_owned(),
                    <$o as Op>::METHOD.as_str().to_owned(),
                    <$o as Op>::PATH.to_owned(),
                    format!("{:?}", <$o as Op>::ACCESS),
                    <$o as Op>::DRIVES.to_string(),
                    <$o as Op>::CREDENTIAL.to_string(),
                    read_only::<$o>().to_string(),
                ]);
            };
        }
        every_op!(row);
        rows.sort();
        rows
    }

    /// `ARUGULA_BLESS=1` rewrites `file` (under `tests/fixtures/`) after a
    /// change meant to alter it.
    fn against_fixture(file: &str, got: String) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(file);
        if std::env::var_os("ARUGULA_BLESS").is_some() {
            std::fs::write(&path, &got).unwrap();
        }
        let want = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("tests/fixtures/{file} (ARUGULA_BLESS=1 writes it)"));
        assert!(got == want, "{file} changed; ARUGULA_BLESS=1 rewrites {}", path.display());
    }

    /// Who may call what, in one reviewable place: every operation's
    /// `ACCESS`, `DRIVES` and `CREDENTIAL`.
    #[test]
    fn every_operations_access_is_this_table() {
        let mut got = String::from("# name method path access drives credential\n");
        for r in rows() {
            got += &format!("{} {} {} {} drives={} credential={}\n", r[0], r[1], r[2], r[3], r[4], r[5]);
        }
        against_fixture("ops-access.txt", got);
    }

    /// A read-only MCP token reaches a GET operation that isn't a
    /// `CREDENTIAL`, and nothing else. A new one shows up in review here.
    #[test]
    fn a_read_only_token_reaches_only_these_operations() {
        let mut got = String::from("# name method path\n");
        for r in rows().iter().filter(|r| r[6] == "true") {
            assert_eq!(r[1], "GET", "{} reads without being a GET", r[0]);
            assert_eq!(r[5], "false", "{} is a credential", r[0]);
            got += &format!("{} {} {}\n", r[0], r[1], r[2]);
        }
        against_fixture("ops-read-only.txt", got);
    }

    /// The paths [`ROUTERS`] add, and the operations they add with
    /// `.op::<…>()`, read from the source (the router can't be walked once
    /// built). Routers not listed there are outside it.
    fn api_routes() -> (Vec<String>, Vec<String>) {
        let (mut paths, mut ops) = (Vec::new(), Vec::new());
        for (file, start) in ROUTERS {
            let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file)).unwrap();
            let start = src.find(start).unwrap_or_else(|| panic!("{file} has no {start}"));
            // The function ends at a `}` at the indent it starts at.
            let line = &src[src[..start].rfind('\n').map_or(0, |i| i + 1)..];
            let end = format!("\n{}}}\n", &line[..line.len() - line.trim_start_matches(' ').len()]);
            let body = &src[start..start + src[start..].find(&end).unwrap()];
            for (i, _) in body.match_indices(".route(") {
                let rest = body[i + ".route(".len()..].trim_start();
                // A path is a string literal, or a `const NAME: &str = "…";` in the same file.
                let literal = match rest.strip_prefix('"') {
                    Some(lit) => lit,
                    None => {
                        let name = &rest[..rest.find(',').unwrap()];
                        let at = src.find(&format!("const {name}: &str = \"")).expect("a route's path is a string");
                        &src[at + format!("const {name}: &str = \"").len()..]
                    }
                };
                paths.push(literal[..literal.find('"').unwrap()].to_owned());
            }
            for (i, _) in body.match_indices(".op::<") {
                let rest = &body[i + ".op::<".len()..];
                ops.push(rest[..rest.find(">()").unwrap()].to_owned());
            }
        }
        (paths, ops)
    }

    #[test]
    fn every_api_route_is_an_operation_or_hand_written() {
        let (paths, declared) = api_routes();
        // Read something: routes still hand-written, and operations.
        assert!(!paths.is_empty() && declared.len() > 20, "read {} routes, {} operations", paths.len(), declared.len());
        let mut every = Vec::new();
        macro_rules! name {
            ($o:ty) => {
                every.push(stringify!($o).replace(' ', "").rsplit("::").next().unwrap().to_owned());
            };
        }
        every_op!(name);
        for o in &declared {
            assert!(every.contains(o), "a router routes {o}, which is not in every_op!");
        }
        for p in &paths {
            let hand = HAND_WRITTEN.iter().any(|(h, _)| h == p);
            assert!(
                hand,
                "{p} is a hand-written route not on HAND_WRITTEN: declare it as an operation, or list it with why"
            );
        }
        for (h, why) in HAND_WRITTEN {
            assert!(paths.iter().any(|p| p == h), "{h} ({why}) is on HAND_WRITTEN but no router in ROUTERS routes it");
        }
        for o in &every {
            assert!(declared.contains(o), "{o} is an operation no router in ROUTERS routes");
        }
    }
}
