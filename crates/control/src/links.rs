//! Account links (M229, platform step 8): a product (studio, planter, a
//! registered issuer client) tells the hub which of its own users a person
//! is, and gets back their platform account, the hub's account. Identity
//! stays in the hub ("The hub stays the identity provider"), so links live
//! beside the issuer's clients (`db/issuer.rs`).
//!
//! A product calls these from its server, never a browser, as itself: HTTP
//! Basic with its client id and the link secret an operator gave it
//! (`arugula-control issuer link-secret`, for `oidc` clients only: a hud box
//! has no users of its own to link). A public client's tokens prove
//! only who signed in, so without the secret anyone could name any of the
//! product's users.
//!
//! - `POST /oauth/links`: `{ user, github_id, assertion, name? }`. The
//!   assertion is an ID token the hub issued to this very client (its
//!   `aud` is the caller's id: a token minted for studio never links a
//!   planter user), checked with `arugula_control_common::issuer`. The product's `github_id` for its
//!   user must be the account's numeric GitHub id, in the token and in the
//!   hub now: matching is by that number only, never a login, because
//!   logins change hands. Idempotent; a product user already linked to
//!   another account, or an account already linked to another of the
//!   product's users, is refused until that link is ended.
//! - `GET /oauth/links/{user}`: the link, or 404 `not_linked`.
//! - `DELETE /oauth/links/{user}`: the product ends it.
//!
//! The person ends a link on the platform page (Settings, "Linked
//! products", through `HubPort::unlink_product`). The hub doesn't call the
//! product: the product sees it on its next call (`GET` answers 404), and a
//! link comes back only with a fresh assertion, the person signing in
//! through that product again. Links made and ended are audited
//! (`account_link_events`).

use std::sync::Arc;

use arugula_e2e::now_ms;
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use serde::Deserialize;

use crate::{
    App,
    db::issuer::{Link, LinkBy, LinkRefused},
    issuer::{github_id, no_store, off},
};

/// The longest product user id the hub keeps.
pub const USER_MAX: usize = 200;
/// The longest name a product gives its user.
pub const NAME_MAX: usize = 100;

fn error(status: StatusCode, error: &str, why: &str) -> Response {
    no_store((status, Json(serde_json::json!({ "error": error, "error_description": why }))).into_response())
}

fn invalid_client() -> Response {
    let mut r =
        error(StatusCode::UNAUTHORIZED, "invalid_client", "this needs the client's id and link secret (HTTP Basic)");
    r.headers_mut().insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Basic realm=\"arugula\""));
    r
}

/// The client a request comes from, by its HTTP Basic id and link secret.
fn client(app: &App, headers: &HeaderMap) -> Option<String> {
    let basic = headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Basic ")?;
    let pair = base64::engine::general_purpose::STANDARD.decode(basic.trim()).ok()?;
    let (id, secret) = std::str::from_utf8(&pair).ok()?.split_once(':')?;
    if id.is_empty() || secret.is_empty() {
        return None;
    }
    app.db.issuer_client_secret_ok(id, secret).ok()?.then(|| id.to_owned())
}

/// A product user id: 1 to [`USER_MAX`] bytes, no control characters.
fn user_ok(u: &str) -> bool {
    (1..=USER_MAX).contains(&u.len()) && !u.chars().any(char::is_control)
}

fn shown(l: &Link) -> serde_json::Value {
    serde_json::json!({
        "platform_account": l.account,
        "client": l.client,
        "user": l.user,
        "linked_at": l.linked,
    })
}

#[derive(Deserialize)]
struct LinkBody {
    user: String,
    github_id: u64,
    assertion: String,
    #[serde(default)]
    name: Option<String>,
}

/// Who an ID token issued to `client` (and to no other client: its `aud`
/// is checked against the caller's own id) says signed in: the account and
/// its numeric GitHub id, as the hub's keys vouch now.
fn signed_in(app: &App, id_token: &str, client: &str) -> Result<(String, Option<u64>), String> {
    let issuer = app.issuer.as_ref().ok_or("this control isn't an issuer")?;
    let c = issuer.verify(&app.cfg.public_url, id_token, client).map_err(|e| e.to_string())?;
    Ok((c.sub, c.github_id))
}

/// `POST /oauth/links`: link the product's user to the account an
/// assertion is for, when their numeric GitHub ids are the same.
pub async fn link(State(app): State<Arc<App>>, headers: HeaderMap, body: Bytes) -> Response {
    if app.issuer.is_none() {
        return off();
    }
    let Some(client) = client(&app, &headers) else { return invalid_client() };
    let Ok(b) = serde_json::from_slice::<LinkBody>(&body) else {
        return error(StatusCode::BAD_REQUEST, "invalid_request", "a JSON body: user, github_id (a number), assertion");
    };
    let name = b.name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
    if !user_ok(&b.user)
        || b.github_id == 0
        || name.as_deref().is_some_and(|n| n.len() > NAME_MAX || n.chars().any(char::is_control))
    {
        return error(StatusCode::BAD_REQUEST, "invalid_request", "user is 1 to 200 characters, name at most 100");
    }
    let (account, token_github) = match signed_in(&app, b.assertion.trim(), &client) {
        Ok(v) => v,
        Err(why) => return error(StatusCode::UNAUTHORIZED, "invalid_token", &why),
    };
    // By the number only, in the token and in the hub now: never a login.
    let now_github = match github_id(&app, &account) {
        Ok(g) => g.map(|(id, _)| id),
        Err(e) => {
            tracing::warn!("a link couldn't read an account's GitHub id: {e:#}");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "try again");
        }
    };
    if token_github != Some(b.github_id) || now_github != Some(b.github_id) {
        tracing::info!(client, "a link was refused: the numeric GitHub ids differ");
        return error(
            StatusCode::FORBIDDEN,
            "github_id_mismatch",
            "that user's numeric GitHub id isn't this account's (or the account has never signed in with GitHub)",
        );
    }
    let link = Link { client: client.clone(), user: b.user, account, github_id: b.github_id, name, linked: now_ms() };
    match app.db.link_account(&link) {
        Ok(Ok(created)) => {
            if created {
                tracing::info!(client, "an account was linked");
            }
            let l = app.db.account_link(&link.client, &link.user).ok().flatten().unwrap_or(link);
            let mut v = shown(&l);
            v["created"] = created.into();
            no_store(Json(v).into_response())
        }
        Ok(Err(LinkRefused::UserLinkedElsewhere)) => error(
            StatusCode::CONFLICT,
            "user_linked_elsewhere",
            "this user is linked to another Arugula account: unlink it first",
        ),
        Ok(Err(LinkRefused::AccountLinkedElsewhere)) => error(
            StatusCode::CONFLICT,
            "account_linked_elsewhere",
            "this Arugula account is linked to another of this product's users: unlink it first",
        ),
        Err(e) => {
            tracing::warn!("a link wasn't kept: {e:#}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "try again")
        }
    }
}

/// `GET /oauth/links/{user}`: the product's user's link, or 404.
pub async fn show(State(app): State<Arc<App>>, headers: HeaderMap, Path(user): Path<String>) -> Response {
    if app.issuer.is_none() {
        return off();
    }
    let Some(client) = client(&app, &headers) else { return invalid_client() };
    match app.db.account_link(&client, &user) {
        Ok(Some(l)) => no_store(Json(shown(&l)).into_response()),
        Ok(None) => error(StatusCode::NOT_FOUND, "not_linked", "this user isn't linked to an Arugula account"),
        Err(e) => {
            tracing::warn!("a link wasn't read: {e:#}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "try again")
        }
    }
}

/// `DELETE /oauth/links/{user}`: the product ends its user's link.
pub async fn unlink(State(app): State<Arc<App>>, headers: HeaderMap, Path(user): Path<String>) -> Response {
    if app.issuer.is_none() {
        return off();
    }
    let Some(client) = client(&app, &headers) else { return invalid_client() };
    match app.db.unlink_account(&client, &user, None, LinkBy::Product, now_ms()) {
        Ok(gone) => {
            if gone {
                tracing::info!(client, "a product ended an account link");
            }
            no_store(Json(serde_json::json!({ "unlinked": gone })).into_response())
        }
        Err(e) => {
            tracing::warn!("a link wasn't ended: {e:#}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "try again")
        }
    }
}
