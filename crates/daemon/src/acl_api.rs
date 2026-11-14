//! The access routes: the ACL over HTTP, as operations (M12, M19).

use std::sync::Arc;

use arugula_core::SessionId;
use arugula_proto::op::ops::{LinkRevoke, LinksList};
use arugula_proto::{
    api::{AclSetRequest, LinkRequest},
    op::ops::{AccessGet, AclGet, AclSet, LinkMint},
};
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};

use crate::{api::ApiError, mux::Cmd, ops::OpRoutes, server::App};

pub fn routes() -> Router<Arc<App>> {
    Router::new().op::<AclGet>().op::<AclSet>().op::<LinkMint>().op::<AccessGet>()
}

/// Listing a session's read-only links and ending one.
pub fn link_routes() -> Router<Arc<App>> {
    Router::new().op::<LinksList>().op::<LinkRevoke>()
}

fn refuse(status: StatusCode, why: impl Into<String>) -> ApiError {
    ApiError(status, why.into())
}

/// A read-only link (M19): one session, live, until it expires.
pub(crate) async fn link(app: &App, b: LinkRequest) -> Result<Value, ApiError> {
    if b.key.len() != 64 || !b.key.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(refuse(StatusCode::BAD_REQUEST, "key: 32 bytes of hex"));
    }
    if !app.control.admits_links_and_guests() {
        return Err(refuse(StatusCode::CONFLICT, "a deploy's box admits no links"));
    }
    let from = if b.history {
        None
    } else {
        match app.mux.api(|r| crate::mux::Api::SessionEnds(b.session, r)).await.flatten() {
            Some(ends) => Some(ends),
            None => return Err(refuse(StatusCode::NOT_FOUND, "no such session")),
        }
    };
    let expires = crate::store::now_ms() + b.ttl_secs.clamp(10, 7 * 86_400) * 1000;
    let made = app.acl.add_link(b.session, &b.key.to_ascii_lowercase(), expires, from);
    // Control lets its viewers through the relay once it knows. An error too:
    // the grants may have changed all the same (`Acl::commit`).
    app.mux.send(Cmd::AclChanged);
    app.control.poke();
    match made {
        Ok(id) => Ok(json!({ "link": id, "expires": expires })),
        Err(e) => Err(refuse(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

/// The read-only links that still work: `session`'s, or every session's.
pub(crate) fn links(app: &App, session: Option<SessionId>) -> Vec<arugula_proto::api::SessionLink> {
    let links = app.acl.links().into_iter().filter(|g| session.is_none_or(|s| g.session == s));
    links
        .map(|g| arugula_proto::api::SessionLink {
            id: g.principal.strip_prefix("link:").unwrap_or(&g.principal).to_owned(),
            session: g.session,
            made_ms: g.at,
            expires_ms: g.expires.unwrap_or_default(),
        })
        .collect()
}

/// End a read-only link now. Everyone who has nothing else here hangs up as
/// when a grant goes (`AclChanged`), and control hears what's left, so with
/// no other link live its relay stops letting strangers through to here.
pub(crate) fn revoke_link(app: &App, id: &str) -> Result<(), ApiError> {
    let principal = format!("link:{}", id.strip_prefix("link:").unwrap_or(id));
    let revoked = app.acl.revoke_link(&principal, "owner");
    // An error too: the link may be gone all the same (`Acl::commit`).
    if !matches!(revoked, Ok(false)) {
        app.mux.send(Cmd::AclChanged);
        app.control.poke();
    }
    match revoked {
        Ok(true) => Ok(()),
        Ok(false) => Err(refuse(StatusCode::NOT_FOUND, format!("no read-only link {id}"))),
        Err(e) => Err(refuse(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub(crate) fn list(app: &App) -> Value {
    let audit = app.acl.audit();
    let recent = &audit[audit.len().saturating_sub(200)..];
    let grants = app.acl.list();
    // #663: granted logins seen from more than one device share one
    // role; tagged devices that called have no login to grant to.
    let mut logins: Vec<String> =
        grants.iter().filter_map(|g| g.principal.strip_prefix("tailnet:").map(str::to_owned)).collect();
    logins.sort();
    logins.dedup();
    let callers = app.identify.callers().report(&logins);
    json!({ "grants": grants, "audit": recent, "callers": callers })
}

pub(crate) async fn set(app: &App, b: AclSetRequest) -> Result<Value, ApiError> {
    if b.principal.starts_with("link:") {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "a read-only link is always a viewer; `arugula links revoke` ends one",
        ));
    }
    let ok_id = |rest: &str| !rest.is_empty() && rest.len() <= 200 && !rest.chars().any(char::is_control);
    let valid = b.principal.strip_prefix("tailnet:").is_some_and(ok_id)
        || b.principal.strip_prefix("account:").is_some_and(ok_id)
        || b.principal.strip_prefix("team:").is_some_and(ok_id);
    if !valid {
        return Err(refuse(StatusCode::BAD_REQUEST, "principal is tailnet:<login>, account:<id> or team:<id>"));
    }
    // Account ids are as control made them; logins aren't case-sensitive.
    let principal =
        if b.principal.starts_with("tailnet:") { b.principal.to_ascii_lowercase() } else { b.principal.clone() };
    // Joined on control's word (aru-141), an account's people get in with
    // control's tokens, not by certificates chained to a root: none needed.
    let by_control = app.control.enrolled().is_some_and(|e| e.saved.by_control().is_some());
    if (principal.starts_with("team:") || (principal.starts_with("account:") && !by_control))
        && b.role.is_some()
        && b.root.is_none()
        && app.acl.list().iter().all(|g| g.principal != principal)
    {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "sharing with an account needs its root device (a team: its founder's)",
        ));
    }
    let name = b.name.unwrap_or_else(|| principal.split_once(':').map(|(_, n)| n.to_owned()).unwrap_or_default());
    let from = if b.history || b.role.is_none() {
        None
    } else {
        match app.mux.api(|r| crate::mux::Api::SessionEnds(b.session, r)).await.flatten() {
            Some(ends) => Some(ends),
            None => return Err(refuse(StatusCode::NOT_FOUND, "no such session")),
        }
    };
    let set = app.acl.set_full(b.session, &principal, &name, b.role, "owner", from, b.root);
    // Takes effect at once: new state for everyone, and a hang-up for
    // whoever has nothing left; someone new from control needs their
    // certificates fetched. An error too: it may have happened all the same
    // (`Acl::commit`).
    app.mux.send(Cmd::AclChanged);
    app.control.poke();
    if let Err(e) = set {
        return Err(refuse(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }
    Ok(json!({ "grants": app.acl.list() }))
}
