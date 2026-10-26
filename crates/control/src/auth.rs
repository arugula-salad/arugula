//! Who is asking.
//!
//! - **People** sign in with GitHub (OAuth, as a GitHub App) and get a
//!   session cookie for control's own API: the directory, approvals, joins.
//!   A session never reaches a daemon. Daemons trust devices, by key.
//! - **The CLI** (M49) is a `cli` device: it joins with a code, as a
//!   daemon does, and then signs each request as a daemon does (below).
//!   A signed request from an approved CLI device is a session for its
//!   account, with no cookie to ride on, so it needs no origin; but only
//!   on routes whose routes.toml row lists `cli` (`clients::gate`). Its key
//!   is a file an agent on its machine can read, so what only the page does
//!   takes the cookie alone ([`PageSession`]).
//! - **Daemons** sign each request with their enrolled Ed25519 key:
//!   `x-arugula-auth: v2 <device id> <ms> <nonce> <sig>` over
//!   `illogical daemon auth v2\n<METHOD>\n<path and query>\n<ms>\n<nonce>\n<sha256 of the body, hex>\n`,
//!   within five minutes of now. Each signature is good once: control
//!   remembers the ones it took until they'd be too old anyway.
//!   Daemons from before 0.17 signed `<device id> <ms> <sig>` (no query,
//!   no body, no nonce); those are told to update (426).
//!
//! Cookie-authenticated requests that change anything, and every WebSocket
//! upgrade, must come from control's own origin (no cross-site requests
//! riding on the cookie).

use std::sync::Arc;

use arugula_e2e::{Cert, Kind, now_ms};
use axum::{
    extract::{FromRequestParts, Query, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header, request::Parts},
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::{ApiError, App, db::hosted::HostedRow, err, refusal};

pub const SESSION_COOKIE: &str = "ilg_session";
const STATE_COOKIE: &str = "ilg_oauth";
const SESSION_DAYS: u64 = 30;
/// The header a device or daemon signs its requests in.
pub const AUTH_HEADER: &str = "x-arugula-auth";

/// The signature header.
pub fn auth_header(headers: &HeaderMap) -> Option<&HeaderValue> {
    headers.get(AUTH_HEADER)
}
const SKEW_MS: u64 = 5 * 60 * 1000;

pub fn token() -> String {
    hex::encode(arugula_e2e::random::<32>())
}

pub fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

pub(crate) fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v)
}

fn set_cookie(app: &App, name: &str, value: &str, max_age_secs: u64) -> HeaderValue {
    let secure = if app.cfg.public_url.starts_with("https://") { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure}"))
        .unwrap()
}

/// A signed-in person.
pub struct Session {
    pub account: String,
}

impl FromRequestParts<Arc<App>> for Session {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<App>) -> Result<Self, ApiError> {
        // A CLI device's signature (M49): no cookie, so no origin to check.
        match parts.extensions.get::<Signed>() {
            Some(Signed(Ok(cert))) if cert.kind == Kind::Cli => {
                if let Some(a) = parts.extensions.get::<crate::analytics::Asker>() {
                    a.set(&cert.account);
                }
                return Ok(Session { account: cert.account.clone() });
            }
            Some(Signed(Ok(_))) => return Err(err(StatusCode::UNAUTHORIZED, "sign in first")),
            Some(Signed(Err((status, msg)))) => return Err(err(*status, msg)),
            None => {}
        }
        // M152: on the platform page's host, its own session: from the
        // platform page alone, by the cookie the hub set there.
        if crate::platform_host::on_platform_host(app, &parts.headers) {
            let origin = parts.headers.get(header::ORIGIN).and_then(|o| o.to_str().ok());
            if (parts.method != Method::GET || origin.is_some())
                && origin != crate::app_signin::platform_origin(app).as_deref()
            {
                return Err(err(StatusCode::FORBIDDEN, "cross-origin request refused"));
            }
            let account = crate::app_signin::account(app, &parts.headers)?
                .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "sign in first"))?;
            if let Some(a) = parts.extensions.get::<crate::analytics::Asker>() {
                a.set(&account);
            }
            return Ok(Session { account });
        }
        let upgrade = parts.headers.contains_key(header::UPGRADE);
        if parts.method != Method::GET || upgrade {
            let origin = parts.headers.get(header::ORIGIN).and_then(|o| o.to_str().ok());
            match origin {
                Some(o) if o == app.cfg.site(&parts.headers).origin => {}
                None if !upgrade => {}
                _ => return Err(err(StatusCode::FORBIDDEN, "cross-origin request refused")),
            }
        }
        let token =
            cookie(&parts.headers, SESSION_COOKIE).ok_or_else(|| err(StatusCode::UNAUTHORIZED, "sign in first"))?;
        let account =
            app.db.session(&hash(token), now_ms())?.ok_or_else(|| err(StatusCode::UNAUTHORIZED, "sign in again"))?;
        if let Some(a) = parts.extensions.get::<crate::analytics::Asker>() {
            a.set(&account);
        }
        Ok(Session { account })
    }
}

/// A signed-in person on control's page, by their session cookie alone:
/// what only the page does takes this, not [`Session`]. No signature
/// stands in for it, a CLI's included: allowing the desktop app's sign-in,
/// adding a passkey, enrolling and approving devices, recovery codes.
pub struct PageSession {
    pub account: String,
    /// The hash of the cookie's token: this sign-in's row in `sessions`.
    pub token_hash: String,
}

impl FromRequestParts<Arc<App>> for PageSession {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<App>) -> Result<Self, ApiError> {
        if parts.extensions.get::<Signed>().is_some() || parts.extensions.get::<HostedSigned>().is_some() {
            return Err(refusal(StatusCode::UNAUTHORIZED, crate::clients::PAGE_ONLY, crate::clients::PAGE_ONLY_SAY));
        }
        let s = Session::from_request_parts(parts, app).await?;
        let token_hash = cookie(&parts.headers, SESSION_COOKIE).map(hash).unwrap_or_default();
        Ok(PageSession { account: s.account, token_hash })
    }
}

impl PageSession {
    /// Not the desktop app's window (`via = app`): what it can't do, a
    /// browser's or a phone's page can (deleting the account, removing a
    /// passkey, signing out everywhere). Refused with what to do.
    pub fn not_app(&self, app: &App) -> Result<(), ApiError> {
        let via = app.db.session_info(&self.token_hash, now_ms())?.and_then(|i| i.via);
        if via.as_deref() == Some(Via::App.as_str()) {
            return Err(refusal(
                StatusCode::FORBIDDEN,
                "app_window",
                "do this from a browser or a phone that's signed in: the desktop app's window can't",
            ));
        }
        Ok(())
    }
}

/// Someone reaching a machine through the relay: a signed-in person, or
/// (#399) one of their daemons, which reaches another machine's agents for
/// its own agents. The daemon on the other end decides what a daemon may
/// do there (`/api/a2a/` only); control only routes, as for a person.
pub struct Reacher {
    pub account: String,
    /// A daemon's signature, not a person's.
    pub daemon: bool,
    /// That daemon's id (M184: the relay checks its grant before routing it
    /// to a hosted machine).
    pub device: Option<String>,
}

impl FromRequestParts<Arc<App>> for Reacher {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<App>) -> Result<Self, ApiError> {
        if let Some(Signed(Ok(cert))) = parts.extensions.get::<Signed>()
            && cert.kind == Kind::Daemon
        {
            return Ok(Reacher { account: cert.account.clone(), daemon: true, device: Some(cert.device.clone()) });
        }
        let s = Session::from_request_parts(parts, app).await?;
        Ok(Reacher { account: s.account, daemon: false, device: None })
    }
}

/// An enrolled daemon, by its signature (checked by [`verify_daemon`],
/// which sees the body).
pub struct DaemonAuth {
    pub cert: Cert,
}

/// What a daemon signs: the request's method, path and query, time, a
/// nonce, and its body's hash.
pub fn daemon_auth_message_v2(method: &str, path_and_query: &str, ms: u64, nonce: &str, body: &[u8]) -> String {
    let body = hex::encode(Sha256::digest(body));
    format!("illogical daemon auth v2\n{method}\n{path_and_query}\n{ms}\n{nonce}\n{body}\n")
}

/// The most a signed request's body may be.
pub const MAX_SIGNED_BODY: usize = 1 << 20;

/// Signatures (or other one-time proofs) already taken, until they'd be
/// too old to take anyway.
#[derive(Default)]
pub struct Replays(std::sync::Mutex<std::collections::HashMap<String, u64>>);

impl Replays {
    /// Whether this is the first time `key` is used; remembered until
    /// `until` (ms).
    pub fn first(&self, key: &str, until: u64) -> bool {
        let now = now_ms();
        let mut m = self.0.lock().unwrap();
        if m.len() > 50_000 {
            m.retain(|_, u| *u > now);
        }
        match m.get(key) {
            Some(u) if *u > now => false,
            _ => {
                m.insert(key.to_owned(), until);
                true
            }
        }
    }
}

/// The outcome of checking a request's daemon signature, for
/// [`DaemonAuth`] to hand over.
#[derive(Clone)]
pub(crate) struct Signed(pub(crate) Result<Cert, (StatusCode, String)>);

/// A hosted machine's signature, checked (M178).
#[derive(Clone)]
pub(crate) struct HostedSigned(Result<HostedRow, (StatusCode, String)>);

/// A request signed by a live hosted machine (M178): only its own routes
/// and the relay take one. Every route a daemon's signature opens refuses
/// it (`DaemonAuth` and `Session` see a refusal), so it can't act as
/// anyone's machine.
pub struct HostedAuth(pub HostedRow);

impl FromRequestParts<Arc<App>> for HostedAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _app: &Arc<App>) -> Result<Self, ApiError> {
        match parts.extensions.get::<HostedSigned>() {
            Some(HostedSigned(Ok(h))) => Ok(HostedAuth(h.clone())),
            Some(HostedSigned(Err((status, msg)))) => Err(err(*status, msg)),
            None => Err(err(StatusCode::UNAUTHORIZED, "not a hosted machine")),
        }
    }
}

/// Whoever dials the relay: an enrolled daemon, or a hosted machine
/// (M178). `account` is who its sockets are metered against: the
/// daemon's account, or the hosted machine's project.
pub struct Dialer {
    pub id: String,
    pub account: String,
    pub hosted: bool,
}

impl FromRequestParts<Arc<App>> for Dialer {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<App>) -> Result<Self, ApiError> {
        if let Some(HostedSigned(Ok(h))) = parts.extensions.get::<HostedSigned>() {
            return Ok(Dialer { id: h.cert.device.clone(), account: h.cert.project.clone(), hosted: true });
        }
        let d = <DaemonAuth as FromRequestParts<Arc<App>>>::from_request_parts(parts, app).await?;
        Ok(Dialer { id: d.cert.device, account: d.cert.account, hosted: false })
    }
}

/// Checks `x-arugula-auth` on any request that has one, with its body.
pub async fn verify_daemon(
    State(app): State<Arc<App>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if auth_header(req.headers()).is_none() {
        return next.run(req).await;
    }
    let (mut parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_SIGNED_BODY).await else {
        return err(StatusCode::PAYLOAD_TOO_LARGE, "a signed request's body is at most 1 MB").into_response();
    };
    let checked = match check_hosted(&app, &parts, &bytes) {
        // A hosted machine (M178) signs its own routes and the relay, and
        // nothing a daemon's signature opens.
        Ok(Some(h)) => {
            parts.extensions.insert(HostedSigned(Ok(h)));
            Err((StatusCode::FORBIDDEN, "a hosted machine signs only its own routes".to_owned()))
        }
        Err(e) => {
            parts.extensions.insert(HostedSigned(Err((e.0, e.1.clone()))));
            Err((e.0, e.1))
        }
        Ok(None) => check_daemon(&app, &parts, &bytes).map_err(|e| (e.0, e.1)),
    };
    if let Ok(cert) = &checked {
        note_old_host(&app, &parts.headers, cert);
    }
    parts.extensions.insert(Signed(checked));
    next.run(axum::extract::Request::from_parts(parts, axum::body::Body::from(bytes))).await
}

/// Daemons and CLIs still signing requests at a URL control is moving away
/// from (#535): logged once an hour each, so `fly logs | grep "old URL"`
/// says who's left before the old URL goes.
#[derive(Default)]
pub struct OldHosts(pub(crate) std::sync::Mutex<std::collections::HashMap<String, u64>>);

fn note_old_host(app: &App, headers: &HeaderMap, cert: &Cert) {
    let site = app.cfg.site(headers);
    if site == &app.cfg.sites[0] {
        return;
    }
    let now = now_ms();
    let mut seen = app.old_hosts.0.lock().unwrap();
    if seen.get(&cert.device).is_some_and(|at| now - at < 3_600_000) {
        return;
    }
    seen.insert(cert.device.clone(), now);
    info!(
        device = cert.device,
        account = cert.account,
        kind = cert.kind.as_str(),
        name = cert.name,
        at = site.url,
        "a signed request at an old URL"
    );
}

/// The signature's device id, time and signature, and the message it
/// signs.
fn signed_parts<'a>(parts: &Parts, h: &'a str, body: &[u8]) -> Result<(&'a str, u64, &'a str, String), ApiError> {
    let bad = || err(StatusCode::UNAUTHORIZED, "bad daemon signature");
    let f: Vec<&str> = h.split_whitespace().collect();
    let (id, ms, sig, msg) = match f.as_slice() {
        ["v2", id, ms, nonce, sig] => {
            if !(16..=64).contains(&nonce.len()) || !nonce.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(bad());
            }
            let ms: u64 = ms.parse().map_err(|_| bad())?;
            let pq = parts.uri.path_and_query().map_or_else(|| parts.uri.path(), |p| p.as_str());
            (*id, ms, *sig, daemon_auth_message_v2(parts.method.as_str(), pq, ms, nonce, body))
        }
        // A daemon from before 0.17 signs `<id> <ms> <sig>`.
        [_, _, _] => {
            return Err(err(
                StatusCode::UPGRADE_REQUIRED,
                "this machine's Arugula is too old for this control: update Arugula (0.17 or newer) and restart it",
            ));
        }
        _ => return Err(bad()),
    };
    if now_ms().abs_diff(ms) > SKEW_MS {
        return Err(err(StatusCode::UNAUTHORIZED, "clock skew: check this machine's time"));
    }
    Ok((id, ms, sig, msg))
}

/// A hosted machine's signature (M178): `None` when the signer isn't one,
/// so it's checked as a daemon's or the CLI's.
fn check_hosted(app: &App, parts: &Parts, body: &[u8]) -> Result<Option<HostedRow>, ApiError> {
    let bad = || err(StatusCode::UNAUTHORIZED, "bad daemon signature");
    let h = auth_header(&parts.headers).and_then(|v| v.to_str().ok()).ok_or_else(bad)?;
    let Some(id) = h.split_whitespace().nth(1) else { return Ok(None) };
    let row = match app.db.hosted(id)? {
        Some(row) => row,
        None if app.db.hosted_removed(id)? => {
            return Err(err(StatusCode::GONE, "this hosted machine was removed from its project"));
        }
        None => return Ok(None),
    };
    let (_, ms, sig, msg) = signed_parts(parts, h, body)?;
    if !arugula_e2e::cert::verify_hex(&row.cert.sign, msg.as_bytes(), sig) {
        return Err(bad());
    }
    if !app.daemon_sigs.first(&format!("{id} {sig}"), ms + SKEW_MS + 60_000) {
        return Err(err(StatusCode::UNAUTHORIZED, "that signature was used already"));
    }
    Ok(Some(row))
}

fn check_daemon(app: &App, parts: &Parts, body: &[u8]) -> Result<Cert, ApiError> {
    let bad = || err(StatusCode::UNAUTHORIZED, "bad daemon signature");
    let h = auth_header(&parts.headers).and_then(|v| v.to_str().ok()).ok_or_else(bad)?;
    let (id, ms, sig, msg) = signed_parts(parts, h, body)?;
    let cert = match app.db.daemon_cert(id)? {
        Some(c) => c,
        None => match cli_cert(app, id)? {
            Some(c) => c,
            None if app.db.daemon_account_deleted(id)? => {
                return Err(err(StatusCode::UNAUTHORIZED, "this machine's account was deleted"));
            }
            // Removed from a browser (#330): Gone, for the daemon to make
            // a new key and join again.
            None => match crate::api::removed(app, id, Kind::Daemon, false)? {
                Some((msg, _)) => return Err(err(StatusCode::GONE, &msg)),
                None => return Err(err(StatusCode::UNAUTHORIZED, "not an enrolled daemon (left, or revoked?)")),
            },
        },
    };
    if !arugula_e2e::cert::verify_hex(&cert.sign, msg.as_bytes(), sig) {
        return Err(bad());
    }
    // Once each: a copy of a request (from a log, a proxy) does nothing.
    if !app.daemon_sigs.first(&format!("{id} {sig}"), ms + SKEW_MS + 60_000) {
        return Err(err(StatusCode::UNAUTHORIZED, "that signature was used already"));
    }
    Ok(cert)
}

/// An approved CLI device (M49) that its account still trusts: one that
/// was revoked, or whose approver was, signs nothing here.
fn cli_cert(app: &App, id: &str) -> Result<Option<Cert>, ApiError> {
    let Some(cert) = app.db.approved_cert(id, Kind::Cli)? else { return Ok(None) };
    let Some(root) = app.db.account(&cert.account)?.and_then(|a| a.root) else { return Ok(None) };
    let (certs, _) = app.db.devices(&cert.account)?;
    let revs = app.db.revocations(&cert.account)?;
    let trust = arugula_e2e::Trust { account: cert.account.clone(), root };
    Ok(trust.evaluate(&certs, &revs).get(id).filter(|c| **c == cert).cloned())
}

impl FromRequestParts<Arc<App>> for DaemonAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _app: &Arc<App>) -> Result<Self, ApiError> {
        match parts.extensions.get::<Signed>() {
            Some(Signed(Ok(cert))) if cert.kind == Kind::Daemon => Ok(DaemonAuth { cert: cert.clone() }),
            Some(Signed(Ok(_))) => Err(err(StatusCode::UNAUTHORIZED, "not an enrolled daemon")),
            Some(Signed(Err((status, msg)))) => Err(err(*status, msg)),
            None => Err(err(StatusCode::UNAUTHORIZED, "bad daemon signature")),
        }
    }
}

/// A daemon's signature where a request may come without one (a
/// telemetry report, M100): none when it isn't signed, and refused, as
/// [`DaemonAuth`] is, when it's signed badly.
impl axum::extract::OptionalFromRequestParts<Arc<App>> for DaemonAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<App>) -> Result<Option<Self>, ApiError> {
        if parts.extensions.get::<Signed>().is_none() {
            return Ok(None);
        }
        <Self as FromRequestParts<Arc<App>>>::from_request_parts(parts, app).await.map(Some)
    }
}

// ---------------------------------------------------------------- GitHub

#[derive(Deserialize)]
pub struct Start {
    next: Option<String>,
    /// Signing in again to add a first passkey: GitHub asks which account,
    /// so it takes the person's click, not a redirect GitHub answers alone.
    #[serde(default)]
    reauth: bool,
    /// `link=1`: link GitHub to the account signed in (#208), not sign in.
    link: Option<String>,
}

/// Only paths on this site, never `//elsewhere`.
fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(n) if n.starts_with('/') && !n.starts_with("//") && !n.contains('\\') => n.to_owned(),
        _ => "/".into(),
    }
}

/// Back to the site the sign-in started on (#507): its cookies are there.
pub(crate) fn callback_url(app: &App, headers: &HeaderMap) -> String {
    format!("{}/auth/github/callback", app.cfg.site(headers).url)
}

pub async fn github_start(State(app): State<Arc<App>>, headers: HeaderMap, Query(q): Query<Start>) -> Response {
    let Some(gh) = &app.cfg.github else {
        return (StatusCode::NOT_FOUND, "GitHub sign-in isn't configured here").into_response();
    };
    let next = safe_next(q.next.as_deref());
    let callback = callback_url(&app, &headers);
    if matches!(q.link.as_deref(), Some("1" | "true")) {
        return crate::github_link::start(&app, &headers, gh, next, callback);
    }
    let state = token();
    let mut params = vec![("client_id", gh.client_id.as_str()), ("redirect_uri", &callback), ("state", &state)];
    if q.reauth {
        params.push(("prompt", "select_account"));
    }
    let url = url::Url::parse_with_params(&format!("{}/login/oauth/authorize", gh.url), &params).unwrap();
    let mut res = Redirect::to(url.as_str()).into_response();
    let value = format!("{state}.{}", base64_url(next.as_bytes()));
    res.headers_mut().append(header::SET_COOKIE, set_cookie(&app, STATE_COOKIE, &value, 600));
    res
}

fn base64_url(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

fn unbase64_url(s: &str) -> Option<String> {
    use base64::Engine;
    String::from_utf8(base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s).ok()?).ok()
}

#[derive(Deserialize)]
pub struct Callback {
    pub(crate) code: Option<String>,
    pub(crate) state: Option<String>,
    pub(crate) error: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct GithubUser {
    pub(crate) id: u64,
    pub(crate) login: String,
}

pub async fn github_callback(
    State(app): State<Arc<App>>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<Callback>,
) -> Response {
    if let Err(e) = app.limits.check(crate::limit::SIGN_INS, app.limits.client_ip(peer, &headers)) {
        return e.into_response();
    }
    // A person's send (M168), not a sign-in.
    if q.state.as_deref().is_some_and(|s| s.starts_with(crate::forge_send::STATE_PREFIX)) {
        return crate::forge_send::finish(&app, &headers, q).await;
    }
    // Linking GitHub to the account signed in (#208), not a sign-in.
    if q.state.as_deref().is_some_and(|s| s.starts_with(crate::github_link::STATE_PREFIX)) {
        return crate::github_link::finish(&app, &headers, q).await;
    }
    match github_finish(&app, &headers, q).await {
        Ok((account, next)) => {
            let cookie = match start_session(&app, &account, kept_via(&app, &headers, Via::Github)) {
                Ok(c) => c,
                Err(e) => return e.into_response(),
            };
            info!(%account, "signed in with GitHub");
            let mut res = Redirect::to(&next).into_response();
            let h = res.headers_mut();
            h.append(header::SET_COOKIE, cookie);
            h.append(header::SET_COOKIE, set_cookie(&app, STATE_COOKIE, "", 0));
            res
        }
        Err(why) => {
            warn!(%why, "GitHub sign-in failed");
            app.analytics.capture("anonymous", "sign-in failed", serde_json::json!({ "method": "github" }));
            (StatusCode::BAD_REQUEST, format!("Sign-in failed: {why}")).into_response()
        }
    }
}

async fn github_finish(app: &App, headers: &HeaderMap, q: Callback) -> Result<(String, String), String> {
    let gh = app.cfg.github.as_ref().ok_or("not configured")?;
    if let Some(e) = q.error {
        return Err(e);
    }
    let (code, state) = q.code.zip(q.state).ok_or("missing code")?;
    let kept = cookie(headers, STATE_COOKIE).and_then(|c| c.split_once('.'));
    let Some((_, next)) = kept.filter(|(want, _)| *want == state) else {
        // A code this browser didn't start a sign-in for (someone sent
        // the link): used up all the same, as GitHub's codes work once, so
        // it signs nobody in anywhere else; its token goes at once.
        if let Ok(t) = exchange(app, gh, &code, &callback_url(app, headers)).await {
            crate::forge_send::revoke(app, gh, &t).await;
        }
        return Err(
            if kept.is_none() { "the sign-in took too long; try again" } else { "state mismatch; try again" }.into()
        );
    };
    let next = safe_next(unbase64_url(next).as_deref());
    let access = exchange(app, gh, &code, &callback_url(app, headers)).await?;
    let user = github_user(app, gh, &access).await?;
    let new_id = new_account_id();
    let account = app
        .db
        .account_for("github", &user.id.to_string(), &user.login, &new_id, now_ms())
        .map_err(|e| e.to_string())?;
    app.analytics.signed_in(&account, "github", account == new_id);
    Ok((account, next))
}

/// Who a user token is on GitHub.
pub(crate) async fn github_user(app: &App, gh: &crate::Github, access: &str) -> Result<GithubUser, String> {
    app.http
        .get(format!("{}/user", gh.api))
        .bearer_auth(access)
        .header(header::USER_AGENT, "arugula-control")
        .header(header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

/// GitHub's code for a user token, with `gh`'s client: the token, or why
/// not. Neither the token nor the code is logged.
pub(crate) async fn exchange(app: &App, gh: &crate::Github, code: &str, redirect_uri: &str) -> Result<String, String> {
    let form: String = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", &gh.client_id)
        .append_pair("client_secret", &gh.client_secret)
        .append_pair("code", code)
        .append_pair("redirect_uri", redirect_uri)
        .finish();
    let tok: TokenResponse = app
        .http
        .post(format!("{}/login/oauth/access_token", gh.url))
        .header(header::ACCEPT, "application/json")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?
        .json()
        .await
        .map_err(|e| e.without_url().to_string())?;
    tok.access_token.ok_or_else(|| tok.error_description.unwrap_or_else(|| "no token".into()))
}

/// How a session was signed in, kept on its row: adding a passkey to an
/// account with none asks for a GitHub sign-in a few minutes old.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Github,
    Passkey,
    /// The desktop app's window, from a browser's hand-over
    /// (`app_login`).
    App,
}

impl Via {
    pub fn as_str(self) -> &'static str {
        match self {
            Via::Github => "github",
            Via::Passkey => "passkey",
            Via::App => "app",
        }
    }
}

/// How a new sign-in from this request counts: `via`, unless the request
/// comes from the desktop app's window (its session says `app`), which
/// stays the app's, so the window can't make itself a browser's session by
/// signing in again (with a passkey or GitHub).
pub fn kept_via(app: &App, headers: &HeaderMap, via: Via) -> Via {
    let now = cookie(headers, SESSION_COOKIE).and_then(|t| app.db.session_info(&hash(t), now_ms()).ok().flatten());
    if now.is_some_and(|i| i.via.as_deref() == Some(Via::App.as_str())) { Via::App } else { via }
}

/// A new session for `account`, signed in `via`: the Set-Cookie for it.
pub fn start_session(app: &App, account: &str, via: Via) -> Result<HeaderValue, ApiError> {
    let t = token();
    let now = now_ms();
    app.db.add_session(&hash(&t), account, now, now + SESSION_DAYS * 86_400_000)?;
    app.db.set_session_via(&hash(&t), via.as_str())?;
    Ok(set_cookie(app, SESSION_COOKIE, &t, SESSION_DAYS * 86_400))
}

pub fn new_account_id() -> String {
    hex::encode(arugula_e2e::random::<8>())
}

pub async fn logout(State(app): State<Arc<App>>, s: Session, headers: HeaderMap) -> Response {
    app.analytics.capture(&s.account, "signed out", serde_json::json!({}));
    if let Some(t) = cookie(&headers, SESSION_COOKIE) {
        let _ = app.db.drop_session(&hash(t));
    }
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().append(header::SET_COOKIE, set_cookie(&app, SESSION_COOKIE, "", 0));
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_stays_here() {
        assert_eq!(safe_next(Some("/x?y=1")), "/x?y=1");
        assert_eq!(safe_next(Some("//evil.example")), "/");
        assert_eq!(safe_next(Some("https://evil.example")), "/");
        assert_eq!(safe_next(Some("/\\evil.example")), "/");
        assert_eq!(safe_next(None), "/");
    }

    /// A GitHub sign-in's code is used up even when it reaches a browser
    /// that didn't start that sign-in (someone sent the link): it signs
    /// nobody in, then or later, and its token goes at once. Signing in
    /// again to add a passkey asks GitHub for the account picker.
    #[tokio::test]
    async fn a_code_without_its_sign_in_is_used_up() {
        use std::sync::{Arc as A, Mutex};

        use axum::{
            Json, Router,
            routing::{delete, get, post},
        };
        use serde_json::json;

        #[derive(Default)]
        struct Seen {
            codes: Vec<String>,
            revoked: usize,
        }
        let seen: A<Mutex<Seen>> = A::default();
        let s2 = seen.clone();
        let s3 = seen.clone();
        let gh = Router::new()
            .route(
                "/login/oauth/access_token",
                post(move |body: String| async move {
                    s2.lock().unwrap().codes.push(body);
                    Json(json!({ "access_token": "ghu_test", "token_type": "bearer" }))
                }),
            )
            .route("/user", get(|| async { Json(json!({ "id": 7, "login": "jake-gh" })) }))
            .route(
                "/applications/cid/token",
                delete(move || async move {
                    s3.lock().unwrap().revoked += 1;
                    StatusCode::NO_CONTENT
                }),
            );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, gh).await.unwrap() });
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let mut app = App::for_tests(&base);
        app.cfg.github =
            Some(crate::Github { client_id: "cid".into(), client_secret: "s".into(), url: api.clone(), api });
        let app = app.start();
        let svc = crate::router(app.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>();
        tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
        let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();

        // The re-auth asks GitHub for the account picker; a sign-in doesn't.
        let start = |q: &str| http.get(format!("{base}/auth/github{q}")).send();
        let r = start("?reauth=true&next=/%23add-passkey").await.unwrap();
        let to = r.headers()[header::LOCATION].to_str().unwrap().to_owned();
        assert!(to.contains("prompt=select_account"), "{to}");
        let state_cookie = r.headers()[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_owned();
        let state = state_cookie.split_once('=').unwrap().1.split_once('.').unwrap().0.to_owned();
        assert!(!start("").await.unwrap().headers()[header::LOCATION].to_str().unwrap().contains("prompt="));

        // A code with no sign-in here, or another one's: refused, but used.
        let back = |code: &str, st: &str, cookie: Option<&str>| {
            let mut r = http.get(format!("{base}/auth/github/callback?code={code}&state={st}"));
            if let Some(c) = cookie {
                r = r.header(header::COOKIE, c);
            }
            r.send()
        };
        assert_eq!(back("sent-1", "x", None).await.unwrap().status(), 400);
        assert_eq!(back("sent-2", "x", Some(&state_cookie)).await.unwrap().status(), 400);
        {
            let s = seen.lock().unwrap();
            assert!(
                s.codes.iter().any(|b| b.contains("code=sent-1")) && s.codes.iter().any(|b| b.contains("code=sent-2"))
            );
            assert_eq!(s.revoked, 2, "their tokens went");
        }
        // Its own: signed in, a GitHub sign-in, back where it asked.
        let r = back("mine", &state, Some(&state_cookie)).await.unwrap();
        assert_eq!(r.status(), 303);
        assert_eq!(r.headers()[header::LOCATION], "/#add-passkey");
        let token = r
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(|v| {
                v.strip_prefix(&format!("{SESSION_COOKIE}=")).map(|v| v.split(';').next().unwrap().to_owned())
            })
            .unwrap();
        assert_eq!(app.db.session_info(&hash(&token), now_ms()).unwrap().unwrap().via.as_deref(), Some("github"));
    }

    #[test]
    fn cookies() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_static("a=1; ilg_session=tok; b=2"));
        assert_eq!(cookie(&h, SESSION_COOKIE), Some("tok"));
        assert_eq!(cookie(&h, "c"), None);
    }
}
