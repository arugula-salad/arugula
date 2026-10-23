//! Account links (M229, platform step 8): a product (studio, planter, a
//! registered `oidc` issuer client) tells the hub which of its own users a
//! person is, and gets back their platform account, the hub's account.
//! Identity stays in the hub ("The hub stays the identity provider"), so
//! links live beside the issuer's clients (`db/issuer.rs`).
//!
//! **A link is never authorization.** It says which product user a person
//! is, nothing more: nothing on the hub or the platform may grant a power, a
//! project, a role or a machine because of an `account_links` row.
//!
//! A product calls these from its server, never a browser, as itself: HTTP
//! Basic with its client id and the link secret an operator gave it
//! (`arugula-control issuer link-secret`). Only `oidc` clients link: a hud
//! box has no users of its own, and its secret is for its own routes
//! (`hud_box.rs`, M237). A public client's tokens prove only
//! who signed in, so without the secret anyone could name any of the
//! product's users. Each call counts against the caller's address
//! (`issuer::limited`) and against the product (`limit::ACCOUNT_LINKS`);
//! bulk unlinks or lookups by one product are logged at warn and counted
//! (`link_alerts`).
//!
//! - `POST /oauth/links`: `{ user, github_id, assertion, name? }`. The
//!   assertion is an ID token the hub issued to this very client (its
//!   `aud` is the caller's id: a token minted for studio never links a
//!   planter user), checked with `arugula_control_common::issuer`. The
//!   product's `github_id` for its user must be the account's numeric
//!   GitHub id, in the token and in the hub now: matching is by that number
//!   only, never a login, because logins change hands. Idempotent; a
//!   product user already linked to another account, or an account already
//!   linked to another of the product's users, is refused until that link
//!   ends; and a token issued no later than the account's last unlink from
//!   this product is refused (the tombstone), so an unlink sticks.
//! - `GET /oauth/links/{user}`: the link, or 404 `not_linked`.
//! - `DELETE /oauth/links/{user}`: the product ends it.
//!
//! The person unlinks on the platform page (Settings, "Linked products",
//! through `HubPort::unlink_product`). Every unlink also ends the person's
//! consent (`issuer_grants`), so the next sign-in through that product asks
//! again. The hub doesn't call the product: the product sees it on its next
//! call (`GET` answers 404). Links made, renamed and ended are audited
//! (`account_link_events`).

use std::{net::SocketAddr, sync::Arc};

use arugula_e2e::now_ms;
use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use serde::Deserialize;

use crate::{
    App,
    db::issuer::{ClientKind, Link, LinkRefused},
    issuer::{github_id, limited, no_store, off},
    limit,
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
    app.db.issuer_client_by_secret(id, secret, ClientKind::Oidc).ok()?.map(|c| c.id)
}

/// Every link call: the issuer's limit on the address, then the product's
/// credentials, then its own limit. The product's id, or the refusal.
fn admit(app: &App, peer: SocketAddr, headers: &HeaderMap) -> Result<String, Box<Response>> {
    if app.issuer.is_none() {
        return Err(Box::new(off()));
    }
    if let Some(r) = limited(app, peer, headers) {
        return Err(Box::new(r));
    }
    let client = client(app, headers).ok_or_else(|| Box::new(invalid_client()))?;
    app.limits.check_client(limit::ACCOUNT_LINKS, &client).map_err(|e| Box::new(e.into_response()))?;
    Ok(client)
}

/// Counts an unlink or a lookup by `client` against its alert line; past
/// it, a warn (once an hour per product and kind) and the ops counter.
/// Never refuses.
fn watch(app: &App, client: &str, what: &'static str) {
    let line = if what == "unlinks" { limit::LINK_UNLINKS_ALERT } else { limit::LINK_LOOKUPS_ALERT };
    if app.limits.check_client(line, client).is_ok() {
        return;
    }
    if let Err(e) = app.db.bump_link_alert(client, what, now_ms()) {
        tracing::warn!("the link alert counter wasn't kept: {e:#}");
    }
    if app.limits.check_client(limit::LINK_ALERT_LOG, &format!("{client}/{what}")).is_ok() {
        tracing::warn!(
            ops = "account-links-bulk",
            client,
            what,
            line = line.1,
            "a product is making bulk {what} of account links: over {} an hour (arugula-control issuer link-alerts)",
            line.1
        );
    }
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

/// What an ID token issued to `client` (and to no other client: its `aud`
/// is checked against the caller's own id) says: the account, its numeric
/// GitHub id, and when it was issued (`iat`, seconds), as the hub's keys
/// vouch now.
fn signed_in(app: &App, id_token: &str, client: &str) -> Result<(String, Option<u64>, u64), String> {
    let issuer = app.issuer.as_ref().ok_or("this control isn't an issuer")?;
    let c = issuer.verify(&app.cfg.public_url, id_token, client).map_err(|e| e.to_string())?;
    Ok((c.sub, c.github_id, c.iat))
}

/// `POST /oauth/links`: link the product's user to the account an ID token
/// it was issued is for, when their numeric GitHub ids are the same.
pub async fn link(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let client = match admit(&app, peer, &headers) {
        Ok(c) => c,
        Err(r) => return *r,
    };
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
    let (account, token_github, iat) = match signed_in(&app, b.assertion.trim(), &client) {
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
    match app.db.link_account(&link, iat) {
        Ok(Ok(created)) => {
            if created {
                tracing::info!(client, "an account was linked");
            }
            let l = app.db.account_link(&link.client, &link.user).ok().flatten().unwrap_or(link);
            let mut v = shown(&l);
            v["created"] = created.into();
            no_store(Json(v).into_response())
        }
        Ok(Err(LinkRefused::Unlinked)) => error(
            StatusCode::UNAUTHORIZED,
            "invalid_token",
            "this sign-in is from before the person unlinked this product: sign in through Arugula again",
        ),
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
pub async fn show(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(user): Path<String>,
) -> Response {
    let client = match admit(&app, peer, &headers) {
        Ok(c) => c,
        Err(r) => return *r,
    };
    watch(&app, &client, "lookups");
    match app.db.account_link(&client, &user) {
        Ok(Some(l)) => no_store(Json(shown(&l)).into_response()),
        Ok(None) => error(StatusCode::NOT_FOUND, "not_linked", "this user isn't linked to an Arugula account"),
        Err(e) => {
            tracing::warn!("a link wasn't read: {e:#}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "try again")
        }
    }
}

/// `DELETE /oauth/links/{user}`: the product ends its user's link, and
/// that account's consent to it.
pub async fn unlink(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(user): Path<String>,
) -> Response {
    let client = match admit(&app, peer, &headers) {
        Ok(c) => c,
        Err(r) => return *r,
    };
    watch(&app, &client, "unlinks");
    match app.db.unlink_user(&client, &user, now_ms()) {
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
