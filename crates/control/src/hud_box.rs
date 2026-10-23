//! hud boxes as clients (M237, platform step 8, "hud is an in-app client:
//! platform assertions, a project roster"). A `hud` client an operator
//! bound to a platform project (`issuer add-client --kind hud --project`)
//! and gave a secret (`issuer client-secret`) calls these from the box's
//! server, never a browser, as itself: HTTP Basic with its client id (the
//! box's `HUD_FORGE_SIGNIN_AUDIENCE`) and that secret.
//!
//! - `GET /oauth/hud/members`: its roster, the project's members now
//!   (`arugula_platform::boxes::members`), each with their numeric GitHub id,
//!   which hud matches people on, and their login for display only. A
//!   member removed from the project is gone from the next read.
//! - `POST /oauth/hud/threads`: `{ target, author, text }`, a comment-mode
//!   review posted into the thread on an object in that project for its
//!   author (`arugula_platform::boxes::post`), kept as posted via the box.
//!   Only for someone the hub gave this box's assertion to in the last
//!   month (`hud_signins`): a box can't post for a member who never
//!   signed in to it.
//!
//! That's all a box's secret does. It reads no threads, names no other
//! project, and never links accounts (an `oidc` client's secret does that,
//! `links.rs`). An unbound box gets 409 `not_bound` and keeps its own
//! roster.

use std::{net::SocketAddr, sync::Arc};

use arugula_e2e::now_ms;
use arugula_platform::{
    PlatformPort as _,
    boxes::{BoxPost, BoxPosted},
};
use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use serde::Deserialize;

use crate::{
    App,
    db::issuer::{Client, ClientKind},
    issuer::{no_store, off},
};

/// The longest object id or account a box names.
const ID_MAX: usize = 100;
/// The longest review a box posts: a chat message's.
const TEXT_MAX: usize = 8 * 1024;

fn error(status: StatusCode, error: &str, why: &str) -> Response {
    no_store((status, Json(serde_json::json!({ "error": error, "error_description": why }))).into_response())
}

fn invalid_client() -> Response {
    let mut r =
        error(StatusCode::UNAUTHORIZED, "invalid_client", "this needs the box's client id and secret (HTTP Basic)");
    r.headers_mut().insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Basic realm=\"arugula\""));
    r
}

/// The `hud` client a request comes from, by its HTTP Basic id and secret.
fn client(app: &App, headers: &HeaderMap) -> Option<Client> {
    let basic = headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Basic ")?;
    let pair = base64::engine::general_purpose::STANDARD.decode(basic.trim()).ok()?;
    let (id, secret) = std::str::from_utf8(&pair).ok()?.split_once(':')?;
    if id.is_empty() || secret.is_empty() {
        return None;
    }
    app.db.issuer_client_by_secret(id, secret, ClientKind::Hud).ok()?
}

/// The box and its project, or the answer that refuses it.
fn bound(app: &App, peer: SocketAddr, headers: &HeaderMap) -> Result<(Client, String), Box<Response>> {
    if app.issuer.is_none() {
        return Err(Box::new(off()));
    }
    if let Err(e) = app.limits.check(crate::limit::ISSUER, app.limits.client_ip(peer, headers)) {
        return Err(Box::new(e.into_response()));
    }
    let Some(c) = client(app, headers) else { return Err(Box::new(invalid_client())) };
    let Some(project) = c.project.clone() else {
        return Err(Box::new(error(
            StatusCode::CONFLICT,
            "not_bound",
            "this box isn't bound to a project: it keeps its own roster",
        )));
    };
    Ok((c, project))
}

fn server_error(what: &str, e: &anyhow::Error) -> Response {
    tracing::warn!("a hud box's {what} failed: {e:#}");
    error(StatusCode::BAD_GATEWAY, "server_error", "try again")
}

/// `GET /oauth/hud/members`: the box's project's members now.
pub async fn members(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let (_, project) = match bound(&app, peer, &headers) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    let members = match app.platform.box_members(&project).await {
        Ok(Some(m)) => m,
        Ok(None) => return error(StatusCode::NOT_FOUND, "no_such_project", "this box's project isn't there"),
        Err(e) => return server_error("roster", &e),
    };
    let mut out = Vec::with_capacity(members.len());
    for m in members {
        // Matched by the number only; the login is for showing.
        let gh = match crate::issuer::github_id(&app, &m.account) {
            Ok(g) => g,
            Err(e) => return server_error("roster", &e),
        };
        out.push(serde_json::json!({
            "account": m.account,
            "github_id": gh.as_ref().map(|(id, _)| *id),
            "login": gh.map(|(_, login)| login),
            "name": m.name,
            "role": m.role,
        }));
    }
    no_store(Json(serde_json::json!({ "project": project, "members": out })).into_response())
}

#[derive(Deserialize)]
struct ThreadBody {
    target: String,
    author: String,
    text: String,
}

fn id_ok(s: &str) -> bool {
    (1..=ID_MAX).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `POST /oauth/hud/threads`: a review into an object's thread in the box's
/// project, for a member who signed in to the box through the hub.
pub async fn threads(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (c, project) = match bound(&app, peer, &headers) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    let Ok(b) = serde_json::from_slice::<ThreadBody>(&body) else {
        return error(StatusCode::BAD_REQUEST, "invalid_request", "a JSON body: target, author, text");
    };
    if !id_ok(&b.target) || !id_ok(&b.author) || b.text.trim().is_empty() || b.text.len() > TEXT_MAX {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "target is an object's id, author an account, and text 1 byte to 8 KB",
        );
    }
    match app.db.hud_signed_in_lately(&c.id, &b.author, now_ms()) {
        Ok(true) => {}
        Ok(false) => {
            return error(
                StatusCode::FORBIDDEN,
                "not_signed_in",
                "that person hasn't signed in to this box through Arugula lately",
            );
        }
        Err(e) => return server_error("thread post", &e),
    }
    let post = BoxPost { project, via: c.name.clone(), target: b.target, author: b.author, text: b.text };
    match app.platform.box_post(&post).await {
        Ok(BoxPosted::Posted { room, seq }) => {
            tracing::info!(client = c.id, "a hud box posted a review thread");
            no_store(Json(serde_json::json!({ "room": room, "seq": seq })).into_response())
        }
        Ok(BoxPosted::Refused { status, error: why }) => {
            error(StatusCode::from_u16(status).unwrap_or(StatusCode::FORBIDDEN), "refused", &why)
        }
        Err(e) => server_error("thread post", &e),
    }
}
