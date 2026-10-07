//! Operations (#451, a pilot): the daemon's half of each declaration in
//! `arugula_proto::op`. An operation is handled once ([`Handle`]); its HTTP
//! route ([`OpRoutes::op`]), its line in the access check ([`policy`]) and,
//! where it has one, its MCP tool or kind (`mcp/ops.rs`) come from that.
//!
//! The handler runs the same for every surface. Where the surfaces differ on
//! purpose, it asks [`Cx`]: who is calling and by which way in (an access
//! check an MCP agent's token needs and HTTP's middleware already made), and
//! its errors ([`OpError`]) are worded by each surface as before. The design
//! is in `docs/operations.md`.

mod close;
mod panes;
mod shell_env;

use std::sync::Arc;

use arugula_core::Role;
use arugula_proto::{
    PaneId,
    op::{Access, Method, Op, Request},
};
use axum::{
    Json, Router,
    extract::{FromRequest, FromRequestParts, Path, State},
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
    /// What an MCP agent's token doesn't reach. HTTP's middleware checks
    /// before a handler runs, so it never sees one.
    Unreachable(String),
}

impl OpError {
    pub fn http(self) -> ApiError {
        match self {
            OpError::NoPane(id) => ApiError(StatusCode::NOT_FOUND, format!("no pane %{id}")),
            OpError::Forbidden(why) | OpError::Unreachable(why) => ApiError(StatusCode::FORBIDDEN, why),
        }
    }
}

/// Which way a call came in.
pub enum Via<'a> {
    /// An HTTP request, past `authz::check`: whether it's the owner's, and
    /// whether the owner's CLI says an agent runs it.
    Http { owner: bool, agent: bool },
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

    /// An agent, or someone who isn't the owner: what the owner keeps for
    /// themselves (an invite block, #234) refuses them. Every MCP caller is
    /// an agent.
    pub fn agent_or_guest(&self) -> bool {
        match self.via {
            Via::Http { owner, agent } => !owner || agent,
            Via::Mcp(_) => true,
        }
    }
}

/// The daemon's half of an operation: what it does, once for every way in.
pub trait Handle: Op {
    fn handle(cx: &Cx<'_>, path: Self::Path, req: Self::Req)
    -> impl Future<Output = Result<Self::Res, OpError>> + Send;
}

/// A route's path segment, as axum extracts it: nothing, or a pane (with
/// axum's own answer to one that isn't a number, as before).
pub trait FromPath: Sized {
    fn from_parts(parts: &mut Parts) -> impl Future<Output = Result<Self, Response>> + Send;
}

impl FromPath for () {
    async fn from_parts(_: &mut Parts) -> Result<Self, Response> {
        Ok(())
    }
}

impl FromPath for PaneId {
    async fn from_parts(parts: &mut Parts) -> Result<Self, Response> {
        Path::<PaneId>::from_request_parts(parts, &()).await.map(|Path(id)| id).map_err(IntoResponse::into_response)
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
    let owner = parts.extensions.get::<Principal>().is_none_or(Principal::is_owner);
    let agent = crate::invite::agent(&parts.headers);
    let body = match <O::Req as Request>::without_body() {
        Some(none) => none,
        None => {
            let req = axum::extract::Request::from_parts(parts, body);
            match Json::<O::Req>::from_request(req, &()).await {
                Ok(Json(b)) => b,
                Err(r) => return r.into_response(),
            }
        }
    };
    if O::ACCESS == Access::Owner && !owner {
        return ApiError(StatusCode::FORBIDDEN, "only the owner can".into()).into_response();
    }
    let cx = Cx { app: &app, via: Via::Http { owner, agent } };
    match O::handle(&cx, path, body).await {
        Ok(res) => Json(res).into_response(),
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

/// Whether `path` is the route `template` (`{…}` matches any one segment),
/// and the segment it matched, if any.
fn matches<'p>(template: &str, path: &'p str) -> Option<Option<&'p str>> {
    let (t, p) = (template.trim_start_matches('/').split('/'), path.trim_start_matches('/').split('/'));
    let (t, p): (Vec<&str>, Vec<&str>) = (t.collect(), p.collect());
    if t.len() != p.len() {
        return None;
    }
    let mut hole = None;
    for (t, p) in t.iter().zip(&p) {
        if t.starts_with('{') && t.ends_with('}') {
            hole = Some(*p);
        } else if t != p {
            return None;
        }
    }
    Some(hole)
}

/// What an operation's route needs from someone who isn't the owner, for
/// `authz::check`: `None` for a route that isn't an operation.
pub fn policy(method: &axum::http::Method, path: &str) -> Option<crate::authz::Policy> {
    use crate::authz::Policy;
    let mut found = None;
    macro_rules! look {
        ($o:ty) => {
            if found.is_none()
                && method.as_str() == <$o as Op>::METHOD.as_str()
                && let Some(hole) = matches(<$o as Op>::PATH, path)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_match_their_templates() {
        assert_eq!(matches("/api/panes/{id}/close", "/api/panes/3/close"), Some(Some("3")));
        assert_eq!(matches("/api/panes", "/api/panes"), Some(None));
        assert_eq!(matches("/api/panes/{id}/close", "/api/panes/3/send"), None);
        assert_eq!(matches("/api/panes", "/api/panes/3"), None);
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
}
