//! Signing in to the platform page (M152, "The hub signs people in to the
//! platform page"). The platform page is on a host of its own
//! (`--platform-host`, `app.arugula.io`), where the hub's session cookie
//! never goes. A visitor without a session there is sent here:
//!
//! 1. `GET /signin/platform?return=…&challenge=…&state=…` on control's
//!    host: with the hub's session (signed in on control's page, passkeys
//!    and all), a one-time code goes back to `return` in its fragment.
//!    Without one, control's page signs them in first
//!    (`#platform-signin=`, which the page sends back here once it has).
//! 2. The code is short-lived ([`CODE_MS`]), single use, bound to the
//!    return address and to a PKCE verifier (S256) only the platform page
//!    holds.
//! 3. `POST /api/app/session` on the platform page's host swaps it for a
//!    session of that host's alone (host-only, `__Host-` over HTTPS).
//!
//! The hub keeps these sessions, not the platform: the platform never sees
//! a cookie and trusts only the principal the hub's gateway signs ("The
//! hub stays the identity provider"). Each is made from one hub session and
//! ends with it: signing out on control's page, "End all sessions",
//! ending one in Sign-in and account, and deleting the account end it at
//! once (`Db::drop_session` and the rest), and so does its expiry, which is
//! the hub session's.
//!
//! Passkeys stay bound to control's host: the platform page never asks for
//! one.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use base64::Engine as _;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    App,
    auth::{SESSION_COOKIE, cookie, hash, token},
};

/// How long a code lasts: a redirect and a fetch.
pub const CODE_MS: u64 = 60_000;

/// The platform page's session cookie: `__Host-` over HTTPS (Secure, no
/// Domain, Path=/), plain over a local control's HTTP.
pub fn app_cookie(app: &App) -> &'static str {
    if app.cfg.public_url.starts_with("https://") { "__Host-arugula_app" } else { "arugula_app" }
}

/// A code waiting to be swapped.
struct Pending {
    account: String,
    /// The hub session it came from (its token's hash).
    hub_session: String,
    return_to: String,
    challenge: String,
    until: u64,
}

/// The codes not yet swapped, by their hash. In memory: a code lasts a
/// minute, and a hub that restarts in between sends the visitor round
/// again.
#[derive(Default)]
pub struct Codes(Mutex<HashMap<String, Pending>>);

impl Codes {
    fn put(&self, code_hash: String, p: Pending, now: u64) {
        let mut m = self.0.lock().unwrap();
        m.retain(|_, p| p.until > now);
        m.insert(code_hash, p);
    }

    /// Single use: gone whatever the swap makes of it.
    fn take(&self, code_hash: &str) -> Option<Pending> {
        self.0.lock().unwrap().remove(code_hash)
    }
}

/// The platform page's origin, when this control has one.
pub fn platform_origin(app: &App) -> Option<String> {
    app.cfg.platform_host.as_ref().map(|p| p.origin.clone())
}

use crate::platform_host::on_platform_host;

/// The hub's routes that answer on the platform page's host
/// (`platform_host::split`): the swap, and signing out there.
pub fn route(method: &axum::http::Method, path: &str) -> bool {
    *method == axum::http::Method::POST && matches!(path, APP_SESSION | APP_SIGNOUT)
}

pub const APP_SESSION: &str = "/api/app/session";
pub const APP_SIGNOUT: &str = "/api/app/signout";

fn b64url(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// A PKCE value as RFC 7636 has it: 43 to 128 of `A-Z a-z 0-9 - . _ ~`.
fn pkce_shaped(s: &str) -> bool {
    (43..=128).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
}

fn refuse(status: StatusCode, msg: &'static str) -> Response {
    let mut r = (status, msg).into_response();
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

/// Where the code goes back to: an address on the platform page's origin,
/// with no fragment of its own.
fn return_ok(origin: &str, to: &str) -> bool {
    url::Url::parse(to).is_ok_and(|u| u.origin().ascii_serialization() == origin && u.fragment().is_none())
}

#[derive(Deserialize)]
pub struct Start {
    #[serde(rename = "return")]
    return_to: String,
    challenge: String,
    state: String,
}

/// `GET /signin/platform`: a code for the platform page, or control's page
/// to sign in first.
pub async fn start(State(app): State<Arc<App>>, headers: HeaderMap, Query(q): Query<Start>) -> Response {
    let Some(origin) = platform_origin(&app) else {
        return refuse(StatusCode::NOT_FOUND, "this control has no platform page");
    };
    let state_ok =
        (16..=64).contains(&q.state.len()) && q.state.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b));
    if !return_ok(&origin, &q.return_to) || !pkce_shaped(&q.challenge) || !state_ok {
        return refuse(StatusCode::BAD_REQUEST, "not a sign-in this control can answer");
    }
    let now = now_ms();
    let session = cookie(&headers, SESSION_COOKIE)
        .map(hash)
        .and_then(|h| app.db.session_until(&h, now).ok().flatten().map(|(account, _)| (h, account)));
    let Some((hub_session, account)) = session else {
        // Signed out here: control's page signs them in, then comes back.
        let back = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("return", &q.return_to)
            .append_pair("challenge", &q.challenge)
            .append_pair("state", &q.state)
            .finish();
        return no_store(Redirect::to(&format!("/#platform-signin={}", b64url(back.as_bytes()))).into_response());
    };
    let code = token();
    app.app_codes.put(
        hash(&code),
        Pending { account, hub_session, return_to: q.return_to.clone(), challenge: q.challenge, until: now + CODE_MS },
        now,
    );
    no_store(Redirect::to(&format!("{}#code={code}&state={}", q.return_to, q.state)).into_response())
}

fn no_store(mut r: Response) -> Response {
    let h = r.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    r
}

#[derive(Deserialize)]
pub struct Swap {
    code: String,
    verifier: String,
    #[serde(rename = "return")]
    return_to: String,
}

/// A same-origin call from the platform page: what its writes need.
fn from_platform_page(app: &App, headers: &HeaderMap) -> bool {
    let origin = headers.get(header::ORIGIN).and_then(|o| o.to_str().ok());
    on_platform_host(app, headers) && origin.is_some() && origin == platform_origin(app).as_deref()
}

/// `POST /api/app/session` on the platform page's host: the code, its
/// verifier and the address it came back to, for a session there.
pub async fn swap(State(app): State<Arc<App>>, headers: HeaderMap, Json(b): Json<Swap>) -> Response {
    if !from_platform_page(&app, &headers) {
        return refuse(StatusCode::FORBIDDEN, "cross-origin request refused");
    }
    let stale = || refuse(StatusCode::BAD_REQUEST, "that sign-in is used or stale: sign in again");
    let Some(p) = app.app_codes.take(&hash(&b.code)) else { return stale() };
    let now = now_ms();
    let challenge = b64url(&Sha256::digest(b.verifier.as_bytes()));
    if p.until <= now || p.return_to != b.return_to || !pkce_shaped(&b.verifier) || challenge != p.challenge {
        return stale();
    }
    // The hub session it came from, still live and still theirs.
    let Ok(Some((account, until))) = app.db.session_until(&p.hub_session, now) else { return stale() };
    if account != p.account {
        return stale();
    }
    let t = token();
    if let Err(e) = app.db.add_app_session(&hash(&t), &account, &p.hub_session, now, until) {
        tracing::warn!("a platform page session wasn't kept: {e:#}");
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, "couldn't sign in here: try again");
    }
    let mut r = no_store(StatusCode::NO_CONTENT.into_response());
    r.headers_mut().append(header::SET_COOKIE, set_app_cookie(&app, &t, (until - now) / 1000));
    r
}

/// `POST /api/app/signout` on the platform page's host: this page only;
/// the hub's session stays.
pub async fn signout(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !from_platform_page(&app, &headers) {
        return refuse(StatusCode::FORBIDDEN, "cross-origin request refused");
    }
    if let Some(t) = cookie(&headers, app_cookie(&app)) {
        let _ = app.db.drop_app_session(&hash(t));
    }
    let mut r = no_store(StatusCode::NO_CONTENT.into_response());
    r.headers_mut().append(header::SET_COOKIE, set_app_cookie(&app, "", 0));
    r
}

fn set_app_cookie(app: &App, value: &str, max_age_secs: u64) -> HeaderValue {
    let secure = if app.cfg.public_url.starts_with("https://") { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure}",
        app_cookie(app)
    ))
    .unwrap()
}

/// The account a request on the platform page's host is signed in as, by
/// its session there: what `auth::Session` answers on that host.
pub fn account(app: &App, headers: &HeaderMap) -> anyhow::Result<Option<String>> {
    let Some(t) = cookie(headers, app_cookie(app)) else { return Ok(None) };
    app.db.app_session(&hash(t), now_ms())
}
