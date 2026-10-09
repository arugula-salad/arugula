//! The access routes: the ACL over HTTP, as operations (M12, M19).

use std::sync::Arc;

use arugula_proto::{
    api::{AclSetRequest, LinkRequest},
    op::ops::{AclGet, AclSet, LinkMint},
};
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};

use crate::{api::ApiError, mux::Cmd, ops::OpRoutes, server::App};

pub fn routes() -> Router<Arc<App>> {
    Router::new().op::<AclGet>().op::<AclSet>().op::<LinkMint>()
}

fn refuse(status: StatusCode, why: impl Into<String>) -> ApiError {
    ApiError(status, why.into())
}

/// A read-only link (M19): one session, live, until it expires.
pub(crate) async fn link(app: &App, b: LinkRequest) -> Result<Value, ApiError> {
    if b.key.len() != 64 || !b.key.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(refuse(StatusCode::BAD_REQUEST, "key: 32 bytes of hex"));
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
    match app.acl.add_link(b.session, &b.key.to_ascii_lowercase(), expires, from) {
        Ok(id) => {
            app.mux.send(Cmd::AclChanged);
            // Control lets its viewers through the relay once it knows.
            app.control.poke();
            Ok(json!({ "link": id, "expires": expires }))
        }
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
    if (principal.starts_with("account:") || principal.starts_with("team:"))
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
    if let Err(e) = app.acl.set_full(b.session, &principal, &name, b.role, "owner", from, b.root, None) {
        return Err(refuse(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }
    // Takes effect at once: new state for everyone, and a hang-up for
    // whoever has nothing left; someone new from control needs their
    // certificates fetched.
    app.mux.send(Cmd::AclChanged);
    app.control.poke();
    Ok(json!({ "grants": app.acl.list() }))
}
