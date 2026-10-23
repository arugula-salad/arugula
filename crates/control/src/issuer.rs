//! The hub as an issuer (M228, platform step 8): other products (studio,
//! hud boxes, the platform page) ask the hub who someone is, and the hub
//! answers with tokens they check against its published keys. Identity
//! stays in the hub ("The hub stays the identity provider"); this only
//! says who someone is to products that aren't control's own page.
//!
//! OIDC-shaped, kept small:
//! - `GET /.well-known/openid-configuration` and `/.well-known/jwks.json`;
//! - `GET /oauth/authorize`: the authorization code flow with PKCE (S256
//!   only), for `oidc` clients registered as rows (`db/issuer.rs`) with
//!   exact redirect addresses. Someone signed in to control's page gets a
//!   code at once; anyone else signs in there first (GitHub or a passkey,
//!   as always, `#issuer-signin=`) and comes back;
//! - `POST /oauth/token`: the code and its verifier for an ID token (a JWT
//!   signed EdDSA, minutes long) and an access token (opaque, minutes long,
//!   bound to its client, ending with the hub session it came from);
//! - `GET /oauth/userinfo`: who an access token is for, and its client;
//! - `GET /oauth/hud`: hud's forge assertion (`hudfs1`, hud's own format)
//!   for a `hud` client (a box), in the redirect's fragment.
//!
//! Tokens say who (the account, its numeric GitHub id) and never a project
//! or a role: a product reads those from the platform with the access
//! token. The token formats, and how anyone verifies them, are
//! `arugula_control_common::issuer`.
//!
//! Keys come from config, never the database: `ARUGULA_ISSUER_KEY` signs,
//! `ARUGULA_ISSUER_KEY_NEXT` is published before it signs, and
//! `ARUGULA_ISSUER_KEY_PREVIOUS` stays published after it stopped, until
//! `ARUGULA_ISSUER_KEY_PREVIOUS_UNTIL` (docs/control-ops.md, "The issuer").

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use anyhow::Context as _;
use arugula_control_common::issuer::{
    ACCESS_TOKEN_SECS, HUD_SECS, HudClaims, ID_TOKEN_SECS, IdClaims, IssuerKey, Jwks, Refused, sign_hud, sign_id_token,
    verify_id_token,
};
use arugula_e2e::now_ms;
use axum::{
    Form, Json,
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Redirect, Response},
};
use base64::Engine as _;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    App,
    auth::{SESSION_COOKIE, cookie, hash, token},
    db::issuer::{Client, ClientKind},
};

/// How long a consent screen waits for its answer (M229).
pub const CONSENT_MS: u64 = 10 * 60_000;

/// How long a code lasts: a redirect and a call.
pub const CODE_MS: u64 = 60_000;
/// Codes one hub session may have waiting at once.
pub const CODES_PER_SESSION: usize = 20;
/// The longest a previous key may stay published after a restart: its
/// tokens last minutes, so a day is generous, and a key forgotten there
/// stops the hub from starting rather than living on.
pub const MAX_PREVIOUS_SECS: u64 = 86_400;

/// The issuer's keys: the one that signs, the one published before it
/// signs, and the one still published after it stopped, until a time.
#[derive(Debug, Clone)]
pub struct Keys {
    pub current: IssuerKey,
    pub next: Option<IssuerKey>,
    /// With when it stops being published (seconds since the epoch).
    pub previous: Option<(IssuerKey, u64)>,
}

/// A value as config gives it: itself, or `file:<path>` holding it.
/// Empty (or an empty file) is none.
fn config_value(v: &str, name: &str) -> anyhow::Result<Option<String>> {
    let v = v.trim();
    let text = match v.strip_prefix("file:") {
        Some(path) => std::fs::read_to_string(path).with_context(|| format!("{name}: reading {path}"))?,
        None => v.to_owned(),
    };
    let text = text.trim();
    Ok((!text.is_empty()).then(|| text.to_owned()))
}

fn key_value(v: Option<&str>, name: &str) -> anyhow::Result<Option<IssuerKey>> {
    let Some(v) = v.map(|v| config_value(v, name)).transpose()?.flatten() else { return Ok(None) };
    IssuerKey::from_hex(&v).map(Some).with_context(|| name.to_owned())
}

/// The issuer's settings, as the hub and the CLI read them.
#[derive(clap::Args, Debug, Default, Clone)]
pub struct KeyArgs {
    /// The issuer's signing key (M228): 32 bytes of hex, or `file:<path>`
    /// holding them. Other products (studio, hud boxes) take the tokens it
    /// signs. The issuer is off when unset (docs/control-ops.md, "The
    /// issuer").
    #[arg(long = "issuer-key", env = "ARUGULA_ISSUER_KEY", hide_env_values = true)]
    pub key: Option<String>,
    /// The key that signs next, published before it does.
    #[arg(long = "issuer-key-next", env = "ARUGULA_ISSUER_KEY_NEXT", hide_env_values = true)]
    pub next: Option<String>,
    /// The key that signed before, published until
    /// ARUGULA_ISSUER_KEY_PREVIOUS_UNTIL.
    #[arg(long = "issuer-key-previous", env = "ARUGULA_ISSUER_KEY_PREVIOUS", hide_env_values = true)]
    pub previous: Option<String>,
    /// When the previous key stops being published, in seconds since the
    /// epoch (or `file:<path>`): once its last token has run out. Required
    /// with a previous key, and at most a day ahead when the hub starts.
    #[arg(long = "issuer-key-previous-until", env = "ARUGULA_ISSUER_KEY_PREVIOUS_UNTIL")]
    pub previous_until: Option<String>,
}

impl Keys {
    /// From the issuer's settings at `now` (seconds): none when the key is
    /// unset or empty, and an error for anything that would sign or trust
    /// something other than what was meant.
    pub fn from_config(a: &KeyArgs, now: u64) -> anyhow::Result<Option<Keys>> {
        let next = key_value(a.next.as_deref(), "ARUGULA_ISSUER_KEY_NEXT")?;
        let previous = key_value(a.previous.as_deref(), "ARUGULA_ISSUER_KEY_PREVIOUS")?;
        let until = a
            .previous_until
            .as_deref()
            .map(|v| config_value(v, "ARUGULA_ISSUER_KEY_PREVIOUS_UNTIL"))
            .transpose()?
            .flatten()
            .map(|v| v.parse::<u64>().context("ARUGULA_ISSUER_KEY_PREVIOUS_UNTIL is seconds since the epoch"))
            .transpose()?;
        let current = match a.key.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(v) => Some(
                key_value(Some(v), "ARUGULA_ISSUER_KEY")?
                    .ok_or_else(|| anyhow::anyhow!("ARUGULA_ISSUER_KEY names an empty file"))?,
            ),
        };
        let Some(current) = current else {
            anyhow::ensure!(
                next.is_none() && previous.is_none(),
                "ARUGULA_ISSUER_KEY_NEXT or _PREVIOUS is set without ARUGULA_ISSUER_KEY: set the key that signs"
            );
            return Ok(None);
        };
        let previous = match (previous, until) {
            (None, _) => None,
            (Some(_), None) => anyhow::bail!(
                "ARUGULA_ISSUER_KEY_PREVIOUS needs ARUGULA_ISSUER_KEY_PREVIOUS_UNTIL: when its last token runs out \
                 (seconds since the epoch)"
            ),
            (Some(_), Some(u)) if u > now + MAX_PREVIOUS_SECS => anyhow::bail!(
                "ARUGULA_ISSUER_KEY_PREVIOUS_UNTIL is more than a day away: a previous key is published only until \
                 its tokens run out (minutes)"
            ),
            (Some(k), Some(u)) => {
                if u <= now {
                    tracing::warn!(
                        kid = k.kid(),
                        "ARUGULA_ISSUER_KEY_PREVIOUS is past ARUGULA_ISSUER_KEY_PREVIOUS_UNTIL and no longer \
                         published: unset both"
                    );
                }
                Some((k, u))
            }
        };
        let kids: Vec<&str> = [Some(&current), next.as_ref(), previous.as_ref().map(|(k, _)| k)]
            .into_iter()
            .flatten()
            .map(|k| k.kid())
            .collect();
        let mut unique = kids.clone();
        unique.sort_unstable();
        unique.dedup();
        anyhow::ensure!(unique.len() == kids.len(), "the issuer's keys must all differ: one key is set twice");
        Ok(Some(Keys { current, next, previous }))
    }

    /// The previous key, while it's still published.
    fn previous_live(&self, now: u64) -> Option<&IssuerKey> {
        self.previous.as_ref().filter(|(_, until)| *until > now).map(|(k, _)| k)
    }

    /// Every key published at `now`: the one that signs first.
    pub fn jwks(&self, now: u64) -> Jwks {
        Jwks {
            keys: [Some(&self.current), self.next.as_ref(), self.previous_live(now)]
                .into_iter()
                .flatten()
                .map(IssuerKey::jwk)
                .collect(),
        }
    }

    /// The keys a token this hub issued may be signed by at `now`: the one
    /// that signs and a live previous one. Never next: it signs nothing
    /// until it's made the key.
    pub fn trusted(&self, now: u64) -> Jwks {
        Jwks {
            keys: [Some(&self.current), self.previous_live(now)].into_iter().flatten().map(IssuerKey::jwk).collect(),
        }
    }

    /// For the log at start: which key is which, by id.
    pub fn describe(&self) -> String {
        let next = self.next.as_ref().map_or("none".to_owned(), |k| k.kid().to_owned());
        let previous = self.previous.as_ref().map_or("none".to_owned(), |(k, u)| format!("{} until {u}", k.kid()));
        format!("signs with {}, next {next}, previous {previous}", self.current.kid())
    }
}

/// A code waiting to be swapped at the token endpoint.
struct Pending {
    account: String,
    /// The hub session it came from (its token's hash).
    hub_session: String,
    client: String,
    redirect_uri: String,
    challenge: String,
    nonce: Option<String>,
    until: u64,
}

/// The issuer: its keys, and the codes not yet swapped, by their hash. In
/// memory, as the platform page's are: a code lasts a minute, and a hub
/// that restarts in between sends the client round again.
pub struct Issuer {
    pub keys: Keys,
    codes: Mutex<HashMap<String, Pending>>,
    /// Consent screens shown and not yet answered (M229), by their
    /// ticket's hash: the request each answers, and its `state`.
    asking: Mutex<HashMap<String, (Pending, String)>>,
}

impl Issuer {
    pub fn new(keys: Keys) -> Self {
        Issuer { keys, codes: Mutex::default(), asking: Mutex::default() }
    }

    /// Keep a request waiting on the person's consent, unless its hub
    /// session already has [`CODES_PER_SESSION`] waiting.
    fn ask(&self, ticket_hash: String, p: Pending, state: String, now: u64) -> bool {
        let mut m = self.asking.lock().unwrap();
        m.retain(|_, (p, _)| p.until > now);
        if m.values().filter(|(o, _)| o.hub_session == p.hub_session).count() >= CODES_PER_SESSION {
            return false;
        }
        m.insert(ticket_hash, (p, state));
        true
    }

    /// Single use: gone whatever the answer.
    fn answer(&self, ticket_hash: &str) -> Option<(Pending, String)> {
        self.asking.lock().unwrap().remove(ticket_hash)
    }

    /// Keep a code, unless its hub session already has
    /// [`CODES_PER_SESSION`] waiting.
    fn put(&self, code_hash: String, p: Pending, now: u64) -> bool {
        let mut m = self.codes.lock().unwrap();
        m.retain(|_, p| p.until > now);
        if m.values().filter(|o| o.hub_session == p.hub_session).count() >= CODES_PER_SESSION {
            return false;
        }
        m.insert(code_hash, p);
        true
    }

    /// Single use: gone whatever the swap makes of it.
    fn take(&self, code_hash: &str) -> Option<Pending> {
        self.codes.lock().unwrap().remove(code_hash)
    }

    /// Every code waiting, `ms` older (tests).
    #[cfg(test)]
    pub fn age_codes(&self, ms: u64) {
        for p in self.codes.lock().unwrap().values_mut() {
            p.until = p.until.saturating_sub(ms);
        }
    }

    /// Checks an ID token this hub issued to `client`, now, against the
    /// keys it trusts (never next): what a caller in this process (M229's
    /// link API) takes a token with. Elsewhere, `issuer::verify_id_token`
    /// with the JWKS.
    pub fn verify(&self, iss: &str, token: &str, client: &str) -> Result<IdClaims, Refused> {
        let now = now_ms() / 1000;
        verify_id_token(token, &self.keys.trusted(now), iss, client, now)
    }
}

/// The account a live access token is for, when it was issued to
/// `client`: what userinfo, and anything later that takes these tokens
/// (the gateway, M229), checks. A token issued to one client is never
/// another's.
pub fn access_token_for(app: &App, token: &str, client: &str) -> Option<String> {
    let (account, issued_to) = app.db.issuer_token(&hash(token), now_ms()).ok().flatten()?;
    (issued_to == client).then_some(account)
}

fn b64url(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// A PKCE value as RFC 7636 has it: 43 to 128 of `A-Z a-z 0-9 - . _ ~`.
fn pkce_shaped(s: &str) -> bool {
    (43..=128).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
}

/// A nonce for hud's assertion: 8 to 200 URL-safe characters.
fn nonce_shaped(s: &str) -> bool {
    (8..=200).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
}

pub(crate) fn no_store(mut r: Response) -> Response {
    let h = r.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    r
}

pub(crate) fn off() -> Response {
    (StatusCode::NOT_FOUND, "this control isn't an issuer").into_response()
}

/// Shown, never redirected: a client or address that isn't registered
/// (for this flow) gets nothing sent anywhere.
fn not_registered() -> Response {
    no_store(
        (StatusCode::BAD_REQUEST, "this sign-in's client or its redirect address isn't registered with this control")
            .into_response(),
    )
}

/// `/oauth/*` from one address, per hour (`limit.rs`).
pub(crate) fn limited(app: &App, peer: SocketAddr, headers: &HeaderMap) -> Option<Response> {
    app.limits.check(crate::limit::ISSUER, app.limits.client_ip(peer, headers)).err().map(IntoResponse::into_response)
}

/// The hub session on this request, by its cookie: its hash and account.
fn hub_session(app: &App, headers: &HeaderMap, now: u64) -> Option<(String, String)> {
    cookie(headers, SESSION_COOKIE)
        .map(hash)
        .and_then(|h| app.db.session_until(&h, now).ok().flatten().map(|(account, _)| (h, account)))
}

/// Signed out here: control's page signs them in, then comes back to this
/// same request (`#issuer-signin=`).
fn sign_in_first(uri: &Uri) -> Response {
    let back = uri.path_and_query().map_or_else(|| uri.path(), |p| p.as_str());
    no_store(Redirect::to(&format!("/#issuer-signin={}", b64url(back.as_bytes()))).into_response())
}

/// The registered client of `kind` and the address it named, exactly as
/// it was registered.
fn registered(
    app: &App,
    kind: ClientKind,
    client_id: Option<&str>,
    redirect_uri: Option<&str>,
) -> Option<(Client, String)> {
    let (id, uri) = client_id.zip(redirect_uri)?;
    let c = app.db.issuer_client(id).ok().flatten().filter(|c| c.kind == kind)?;
    c.redirect_uris.iter().any(|u| u == uri).then(|| (c, uri.to_owned()))
}

/// Back to the client's address with `params` in its query.
fn back(redirect_uri: &str, params: &[(&str, &str)]) -> Response {
    let mut u = url::Url::parse(redirect_uri).expect("a registered address parses");
    {
        let mut q = u.query_pairs_mut();
        for (k, v) in params {
            q.append_pair(k, v);
        }
    }
    no_store(Redirect::to(u.as_str()).into_response())
}

/// `GET /.well-known/openid-configuration`.
pub async fn discovery(State(app): State<Arc<App>>) -> Response {
    if app.issuer.is_none() {
        return off();
    }
    let iss = &app.cfg.public_url;
    let mut r = Json(serde_json::json!({
        "issuer": iss,
        "authorization_endpoint": format!("{iss}/oauth/authorize"),
        "token_endpoint": format!("{iss}/oauth/token"),
        "userinfo_endpoint": format!("{iss}/oauth/userinfo"),
        "jwks_uri": format!("{iss}/.well-known/jwks.json"),
        "response_types_supported": ["code"],
        "response_modes_supported": ["query"],
        "grant_types_supported": ["authorization_code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["EdDSA"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        "scopes_supported": ["openid"],
        "claims_supported": ["iss", "sub", "aud", "iat", "exp", "nonce", "github_id"],
        "authorization_response_iss_parameter_supported": true,
        // M229: a product's own user to the platform account, with the
        // client's link secret (HTTP Basic), from its server.
        "arugula_links_endpoint": format!("{iss}/oauth/links"),
    }))
    .into_response();
    public_json(&mut r, 300);
    r
}

/// Anyone may read it, from anywhere, for `max_age` seconds.
fn public_json(r: &mut Response, max_age: u32) {
    let h = r.headers_mut();
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_str(&format!("public, max-age={max_age}")).unwrap());
}

/// How long a client may keep the JWKS: a key is published as next at
/// least this long before it signs (docs/control-ops.md).
pub const JWKS_MAX_AGE_SECS: u32 = 300;

/// `GET /.well-known/jwks.json`: every key published, the one that signs
/// first.
pub async fn jwks(State(app): State<Arc<App>>) -> Response {
    let Some(i) = &app.issuer else { return off() };
    let mut r = Json(i.keys.jwks(now_ms() / 1000)).into_response();
    public_json(&mut r, JWKS_MAX_AGE_SECS);
    r
}

#[derive(Deserialize)]
pub struct Authorize {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    state: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    nonce: Option<String>,
    prompt: Option<String>,
}

/// `GET /oauth/authorize`: a code for a registered `oidc` client, or
/// control's page to sign in first.
pub async fn authorize(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Query(q): Query<Authorize>,
) -> Response {
    let Some(issuer) = &app.issuer else { return off() };
    if let Some(r) = limited(&app, peer, &headers) {
        return r;
    }
    let Some((client, redirect_uri)) =
        registered(&app, ClientKind::Oidc, q.client_id.as_deref(), q.redirect_uri.as_deref())
    else {
        return not_registered();
    };
    let iss = app.cfg.public_url.as_str();
    let state = q.state.as_deref().unwrap_or("");
    let fail = |error: &str, why: &str| {
        let mut p = vec![("error", error), ("error_description", why), ("iss", iss)];
        if !state.is_empty() {
            p.push(("state", state));
        }
        back(&redirect_uri, &p)
    };
    if q.response_type.as_deref() != Some("code") {
        return fail("unsupported_response_type", "only the authorization code flow (response_type=code)");
    }
    if q.code_challenge_method.as_deref() != Some("S256") || !q.code_challenge.as_deref().is_some_and(pkce_shaped) {
        return fail("invalid_request", "PKCE is required: code_challenge with code_challenge_method=S256");
    }
    if state.len() > 512 || q.nonce.as_deref().is_some_and(|n| n.len() > 256) {
        return fail("invalid_request", "state or nonce too long");
    }
    let now = now_ms();
    let Some((hub_session, account)) = hub_session(&app, &headers, now) else {
        if q.prompt.as_deref() == Some("none") {
            return fail("login_required", "not signed in to Arugula");
        }
        return sign_in_first(&uri);
    };
    let p = Pending {
        account,
        hub_session,
        client: client.id.clone(),
        redirect_uri: redirect_uri.clone(),
        challenge: q.code_challenge.unwrap_or_default(),
        nonce: q.nonce,
        until: now + CODE_MS,
    };
    // M229: a product signs someone in only once they've said it may, and
    // again after they unlinked it.
    match app.db.issuer_grant(&p.account, &p.client) {
        Ok(true) => give_code(&app, issuer, p, state, now),
        Ok(false) if q.prompt.as_deref() == Some("none") => {
            fail("consent_required", "the person hasn't let this product sign them in yet")
        }
        Ok(false) => {
            let ticket = token();
            let who = app.db.account(&p.account).ok().flatten().map(|a| a.name).unwrap_or_default();
            let p = Pending { until: now + CONSENT_MS, ..p };
            if !issuer.ask(hash(&ticket), p, state.to_owned(), now) {
                return fail("temporarily_unavailable", "too many sign-ins under way: try again in a minute");
            }
            consent_page(&client.name, &who, &ticket)
        }
        Err(e) => {
            tracing::warn!("the issuer couldn't read a consent: {e:#}");
            fail("server_error", "try again")
        }
    }
}

/// A code for `p`, back to its client's address.
fn give_code(app: &App, issuer: &Issuer, p: Pending, state: &str, now: u64) -> Response {
    let iss = app.cfg.public_url.as_str();
    let redirect_uri = p.redirect_uri.clone();
    let code = token();
    if !issuer.put(hash(&code), Pending { until: now + CODE_MS, ..p }, now) {
        let mut q = vec![
            ("error", "temporarily_unavailable"),
            ("error_description", "too many sign-ins under way: try again in a minute"),
            ("iss", iss),
        ];
        if !state.is_empty() {
            q.push(("state", state));
        }
        return back(&redirect_uri, &q);
    }
    let mut q = vec![("code", code.as_str()), ("iss", iss)];
    if !state.is_empty() {
        q.push(("state", state));
    }
    back(&redirect_uri, &q)
}

fn escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            '\'' => "&#39;".into(),
            c => c.to_string(),
        })
        .collect()
}

/// The consent screen (M229): "Link <product> to your Arugula account?",
/// a form back to `/oauth/consent` with the ticket that names the request.
/// The ticket is single use and bound to this hub session, so a page
/// elsewhere can't answer it; nothing here runs script, and nothing may
/// frame it.
fn consent_page(product: &str, who: &str, ticket: &str) -> Response {
    let (product, who) = (escape(product), escape(who));
    let as_who = if who.is_empty() { String::new() } else { format!(" (signed in as <b>{who}</b>)") };
    let html = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Link {product} to Arugula?</title>
<style>body{{font:16px/1.5 system-ui,sans-serif;max-width:32rem;margin:10vh auto;padding:0 16px;background:#0d1117;color:#e6edf3}}
button{{font:inherit;padding:.5em 1.2em;margin-right:.6em;border-radius:6px;border:1px solid #30363d;background:#21262d;color:inherit;cursor:pointer}}
button[value=allow]{{background:#238636;border-color:#238636}}p.dim{{color:#8b949e}}</style></head>
<body><h1>Link {product} to your Arugula account?</h1>
<p>{product} will learn who you are on Arugula{as_who}: your account and your GitHub account's number. Nothing else: not your projects, machines or teams.</p>
<p class="dim">You can unlink it any time in the platform's Settings, under Linked products.</p>
<form method="post" action="/oauth/consent"><input type="hidden" name="ticket" value="{ticket}">
<button type="submit" name="decision" value="allow" data-consent-allow>Link {product}</button><button type="submit" name="decision" value="deny" data-consent-deny>Cancel</button></form>
</body></html>"#
    );
    let mut r = no_store((StatusCode::OK, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response());
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'",
        ),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    r
}

#[derive(Deserialize)]
pub struct Consent {
    ticket: Option<String>,
    decision: Option<String>,
}

/// `POST /oauth/consent`: the person's answer to the consent screen. Only
/// from the same origin it was shown on (a cross-site post is refused), with the
/// ticket that screen was shown with, in the hub session it was shown in.
/// Allowed: the consent is kept and the client gets its code. Cancelled:
/// the client hears `access_denied`.
pub async fn consent(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(b): Form<Consent>,
) -> Response {
    let Some(issuer) = &app.issuer else { return off() };
    if let Some(r) = limited(&app, peer, &headers) {
        return r;
    }
    let refused = |why: &str| no_store((StatusCode::FORBIDDEN, why.to_owned()).into_response());
    // Same origin as this request: the Origin's host and port are the
    // Host this came to (control answers on more than one site, so not
    // only its public URL's).
    let header_is = |h: &str| headers.get(h).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let host = header_is("host").unwrap_or_default().to_ascii_lowercase();
    let same = |o: &str| {
        url::Url::parse(o).ok().is_some_and(|u| {
            let authority = match (u.host_str(), u.port()) {
                (Some(h), Some(p)) => format!("{h}:{p}"),
                (Some(h), None) => h.to_owned(),
                _ => return false,
            };
            authority.to_ascii_lowercase() == host
        })
    };
    // A browser that was told no-referrer says `Origin: null` on a form's
    // post; its Sec-Fetch-Site still says where the post came from.
    let fetch_site = header_is("sec-fetch-site");
    let origin_ok = match header_is("origin").as_deref() {
        None => true,
        Some("null") => fetch_site.as_deref() == Some("same-origin"),
        Some(o) => same(o),
    };
    if !origin_ok || fetch_site.is_some_and(|s| s != "same-origin" && s != "none") {
        tracing::info!(
            origin = header_is("origin").unwrap_or_default(),
            fetch_site = header_is("sec-fetch-site").unwrap_or_default(),
            "a consent answer from another page was refused"
        );
        return refused("this answer didn't come from Arugula's own page");
    }
    let Some((p, state)) = b.ticket.as_deref().and_then(|t| issuer.answer(&hash(t))) else {
        return refused("this question has been answered or has run out: start again from the product");
    };
    let now = now_ms();
    if p.until <= now || hub_session(&app, &headers, now).is_none_or(|(h, a)| h != p.hub_session || a != p.account) {
        return refused("this question has been answered or has run out: start again from the product");
    }
    let iss = app.cfg.public_url.as_str();
    match b.decision.as_deref() {
        Some("allow") => {
            if let Err(e) = app.db.put_issuer_grant(&p.account, &p.client, now) {
                tracing::warn!("a consent wasn't kept: {e:#}");
                return refused("that didn't work: try again");
            }
            tracing::info!(client = p.client, "a person let a product sign them in");
            give_code(&app, issuer, p, &state, now)
        }
        _ => {
            let mut q = vec![("error", "access_denied"), ("error_description", "the person said no"), ("iss", iss)];
            if !state.is_empty() {
                q.push(("state", state.as_str()));
            }
            back(&p.redirect_uri, &q)
        }
    }
}

#[derive(Deserialize)]
pub struct TokenRequest {
    grant_type: Option<String>,
    code: Option<String>,
    redirect_uri: Option<String>,
    client_id: Option<String>,
    code_verifier: Option<String>,
}

fn token_error(status: StatusCode, error: &str, why: &str) -> Response {
    no_store((status, Json(serde_json::json!({ "error": error, "error_description": why }))).into_response())
}

/// The account's numeric GitHub id, when it has one, and its login then.
pub(crate) fn github_id(app: &App, account: &str) -> anyhow::Result<Option<(u64, String)>> {
    Ok(app.db.github_identity(account)?.and_then(|(id, login)| id.parse().ok().map(|id| (id, login))))
}

/// `POST /oauth/token`: a code, its client, address and verifier, for an
/// ID token and an access token.
pub async fn token_endpoint(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(b): Form<TokenRequest>,
) -> Response {
    let Some(issuer) = &app.issuer else { return off() };
    if let Some(r) = limited(&app, peer, &headers) {
        return r;
    }
    if b.grant_type.as_deref() != Some("authorization_code") {
        return token_error(StatusCode::BAD_REQUEST, "unsupported_grant_type", "only authorization_code");
    }
    let stale = || token_error(StatusCode::BAD_REQUEST, "invalid_grant", "that code is used, stale or not yours");
    let Some(p) = b.code.as_deref().and_then(|c| issuer.take(&hash(c))) else { return stale() };
    let now = now_ms();
    let verifier = b.code_verifier.as_deref().unwrap_or("");
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    if p.until <= now
        || b.client_id.as_deref() != Some(p.client.as_str())
        || b.redirect_uri.as_deref() != Some(p.redirect_uri.as_str())
        || !pkce_shaped(verifier)
        || challenge != p.challenge
    {
        return stale();
    }
    // The client still registered as it was, and the hub session it came
    // from still live and still theirs.
    let still = app.db.issuer_client(&p.client).ok().flatten();
    if !still.is_some_and(|c| c.kind == ClientKind::Oidc && c.redirect_uris.contains(&p.redirect_uri)) {
        return token_error(StatusCode::UNAUTHORIZED, "invalid_client", "that client isn't registered any more");
    }
    let Ok(Some((account, session_until))) = app.db.session_until(&p.hub_session, now) else { return stale() };
    if account != p.account {
        return stale();
    }
    let gh = match github_id(&app, &account) {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!("the issuer couldn't read an account's GitHub id: {e:#}");
            return token_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "try again");
        }
    };
    let iat = now / 1000;
    let claims = IdClaims {
        iss: app.cfg.public_url.clone(),
        sub: account.clone(),
        aud: p.client.clone(),
        iat,
        exp: iat + ID_TOKEN_SECS,
        github_id: gh.map(|(id, _)| id),
        nonce: p.nonce,
    };
    let id_token = sign_id_token(&issuer.keys.current, &claims);
    let access = format!("aat_{}", token());
    let expires = (now + ACCESS_TOKEN_SECS * 1000).min(session_until);
    if let Err(e) = app.db.add_issuer_token(&hash(&access), &account, &p.client, &p.hub_session, now, expires) {
        tracing::warn!("an access token wasn't kept: {e:#}");
        return token_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "try again");
    }
    tracing::info!(client = p.client, kid = issuer.keys.current.kid(), "issued an ID token");
    no_store(
        Json(serde_json::json!({
            "access_token": access,
            "token_type": "Bearer",
            "expires_in": (expires - now) / 1000,
            "id_token": id_token,
            "scope": "openid",
        }))
        .into_response(),
    )
}

#[derive(Deserialize)]
pub struct UserinfoQuery {
    client_id: Option<String>,
}

/// `GET /oauth/userinfo`: who an access token is for, and the client it
/// was issued to (`client_id`), which the caller checks is itself. Given
/// `?client_id=`, a token issued to another client is refused here.
pub async fn userinfo(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<UserinfoQuery>,
) -> Response {
    if app.issuer.is_none() {
        return off();
    }
    if let Some(r) = limited(&app, peer, &headers) {
        return r;
    }
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    let found = bearer.and_then(|t| match &q.client_id {
        Some(c) => access_token_for(&app, t, c).map(|a| (a, c.clone())),
        None => app.db.issuer_token(&hash(t), now_ms()).ok().flatten(),
    });
    let Some((account, client)) = found else {
        let mut r = no_store(StatusCode::UNAUTHORIZED.into_response());
        r.headers_mut().insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer error=\"invalid_token\""));
        return r;
    };
    let gh = github_id(&app, &account).ok().flatten();
    // Only a name the person set: never their login, which isn't theirs to
    // keep (logins change hands).
    let name = app.db.issuer_set_name(&account).ok().flatten();
    let mut body = serde_json::json!({ "sub": account, "client_id": client });
    if let Some((id, _)) = gh {
        body["github_id"] = id.into();
    }
    if let Some(n) = name {
        body["name"] = n.into();
    }
    no_store(Json(body).into_response())
}

#[derive(Deserialize)]
pub struct HudStart {
    client_id: Option<String>,
    redirect_uri: Option<String>,
    nonce: Option<String>,
}

fn refuse_hud(status: StatusCode, why: &'static str) -> Response {
    no_store((status, why).into_response())
}

/// `GET /oauth/hud`: hud's forge assertion for a `hud` client (a box; its
/// id is the box's `HUD_FORGE_SIGNIN_AUDIENCE`), in the fragment of the
/// box's own page: `#hud-forge=hudfs1.…`. hud signs a session in as
/// `github:<login>`, so the account needs a GitHub sign-in, and its login
/// must be no one else's here. A `nonce` the box sends is echoed in the
/// assertion: hud from M237 on binds it to the browser that asked, and
/// requires it for a sign-in that admits someone by project membership.
pub async fn hud(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Query(q): Query<HudStart>,
) -> Response {
    let Some(issuer) = &app.issuer else { return off() };
    if let Some(r) = limited(&app, peer, &headers) {
        return r;
    }
    let Some((client, redirect_uri)) =
        registered(&app, ClientKind::Hud, q.client_id.as_deref(), q.redirect_uri.as_deref())
    else {
        return not_registered();
    };
    if q.nonce.as_deref().is_some_and(|n| !nonce_shaped(n)) {
        return refuse_hud(StatusCode::BAD_REQUEST, "a nonce is 8 to 200 of A-Z a-z 0-9 - . _ ~");
    }
    let now = now_ms();
    let Some((_, account)) = hub_session(&app, &headers, now) else { return sign_in_first(&uri) };
    let Ok(Some((github_id, login))) = github_id(&app, &account) else {
        return refuse_hud(
            StatusCode::FORBIDDEN,
            "hud signs you in as your GitHub login: sign in to Arugula with GitHub once, then try again",
        );
    };
    // A login is only as good as its last sign-in: when another account
    // here has signed in with the same one since (a rename, and the old
    // name taken), neither is vouched for until both sign in again.
    match app.db.github_login_shared(&login, &github_id.to_string()) {
        Ok(false) => {}
        Ok(true) => {
            return refuse_hud(
                StatusCode::CONFLICT,
                "your GitHub login is another account's here too: sign out and in again with GitHub, then try again",
            );
        }
        Err(e) => {
            tracing::warn!("the issuer couldn't check a login: {e:#}");
            return refuse_hud(StatusCode::INTERNAL_SERVER_ERROR, "try again");
        }
    }
    let iat = now / 1000;
    let claims = HudClaims {
        sub: format!("github:{login}"),
        aud: client.id,
        iat,
        exp: iat + HUD_SECS,
        jti: hex::encode(arugula_e2e::random::<16>()),
        account,
        github_id,
        kid: issuer.keys.current.kid().to_owned(),
        nonce: q.nonce,
    };
    // A bound box (M237) may post review threads for whoever signs in to
    // it through here, for a while (`hud_box.rs`).
    if client.project.is_some()
        && let Err(e) = app.db.hud_signed_in(&claims.aud, &claims.account, now)
    {
        tracing::warn!("the issuer couldn't note a box's sign-in: {e:#}");
        return refuse_hud(StatusCode::INTERNAL_SERVER_ERROR, "try again");
    }
    let assertion = sign_hud(&issuer.keys.current, &claims);
    no_store(Redirect::to(&format!("{redirect_uri}#hud-forge={assertion}")).into_response())
}

// ---------------------------------------------------------------- the CLI

#[derive(clap::Parser, Debug)]
#[command(name = "arugula-control issuer", about = "The hub as an issuer (M228): its keys and its clients")]
struct Args {
    #[command(subcommand)]
    what: What,
}

#[derive(clap::Subcommand, Debug)]
enum What {
    /// A new signing key: its seed (the secret) on stdout, its id and
    /// public halves on stderr. Set it as ARUGULA_ISSUER_KEY_NEXT first
    /// (docs/control-ops.md, "The issuer").
    NewKey,
    /// The public halves of the keys this environment configures (never
    /// the secrets): each one's id, and its SPKI as hud's
    /// HUD_FORGE_SIGNIN_KEY takes it.
    Keys {
        #[command(flatten)]
        keys: KeyArgs,
    },
    /// Register a client, or change one's kind, name and redirect addresses.
    AddClient {
        #[arg(long, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
        db: std::path::PathBuf,
        /// Its id: what it sends as client_id, and a hud box's
        /// HUD_FORGE_SIGNIN_AUDIENCE.
        #[arg(long)]
        id: String,
        /// `oidc` (a product signing people in with /oauth/authorize) or
        /// `hud` (a box taking /oauth/hud's assertions). Each flow refuses
        /// the other kind.
        #[arg(long)]
        kind: ClientKind,
        #[arg(long)]
        name: String,
        /// An address a sign-in may go back to, compared whole as it's
        /// printed back; repeat it for more than one. A hud box's page must
        /// be on an origin that serves only hud's own UI.
        #[arg(long = "redirect", required = true)]
        redirect_uris: Vec<String>,
        /// A hud box's platform project (M237, `prj_…`): its members are the
        /// box's roster, and its reviews post to threads on that project's
        /// objects (with a client secret, `client-secret`). Left out, the
        /// box keeps its own roster; changing it ends who the box may post
        /// for until they sign in to it again.
        #[arg(long)]
        project: Option<String>,
    },
    /// The registered clients.
    Clients {
        #[arg(long, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
        db: std::path::PathBuf,
    },
    /// Give a client a secret, shown once on stdout: what it calls the
    /// hub with, as HTTP Basic with its id. An oidc client links accounts
    /// with it (M229); a hud box bound to a project reads its members and
    /// posts review threads (M237). A new one replaces the old at once;
    /// --revoke takes it away.
    #[command(visible_alias = "client-secret")]
    LinkSecret {
        #[arg(long, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
        db: std::path::PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        revoke: bool,
    },
    /// The ops counter of bulk link calls (M229): per client, how many
    /// unlinks or lookups went over the alert line, and when last.
    LinkAlerts {
        #[arg(long, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
        db: std::path::PathBuf,
    },
    /// Take a client out, and the access tokens, link secret, consents and
    /// account links it holds (each link ended is audited).
    RemoveClient {
        #[arg(long, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
        db: std::path::PathBuf,
        #[arg(long)]
        id: String,
    },
}

/// The hub's file, never a new one.
fn open(db: &std::path::Path) -> anyhow::Result<rusqlite::Connection> {
    let c = rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {}", db.display()))?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(c)
}

/// `arugula-control issuer …`.
pub fn run(args: impl Iterator<Item = String>) -> anyhow::Result<()> {
    use clap::Parser as _;
    let a = Args::parse_from(std::iter::once("arugula-control issuer".to_owned()).chain(args));
    match a.what {
        What::NewKey => {
            let k = IssuerKey::generate();
            eprintln!("kid {}\nhud's HUD_FORGE_SIGNIN_KEY {}", k.kid(), k.spki_base64());
            println!("{}", k.seed_hex());
        }
        What::Keys { keys } => {
            let now = now_ms() / 1000;
            let Some(keys) = Keys::from_config(&keys, now)? else {
                println!("no ARUGULA_ISSUER_KEY: the issuer is off");
                return Ok(());
            };
            println!("current  kid {}  hud {}", keys.current.kid(), keys.current.spki_base64());
            if let Some(k) = &keys.next {
                println!("next     kid {}  hud {}", k.kid(), k.spki_base64());
            }
            if let Some((k, until)) = &keys.previous {
                let state = if *until > now { "published" } else { "no longer published" };
                println!("previous kid {}  hud {}  until {until} ({state})", k.kid(), k.spki_base64());
            }
        }
        What::AddClient { db, id, kind, name, redirect_uris, project } => {
            let c = open(&db)?;
            let created =
                crate::db::issuer::clients(&c)?.into_iter().find(|c| c.id == id).map_or(now_ms(), |c| c.created);
            let stored = crate::db::issuer::put_client(
                &c,
                &Client { id: id.clone(), kind, name, redirect_uris, created, project },
            )?;
            let bound = stored.project.as_deref().map(|p| format!(" (project {p})")).unwrap_or_default();
            println!("client {id} ({}){bound} registered: {}", kind.as_str(), stored.redirect_uris.join(" "));
        }
        What::Clients { db } => {
            for c in crate::db::issuer::clients(&open(&db)?)? {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    c.id,
                    c.kind.as_str(),
                    c.name,
                    c.redirect_uris.join(" "),
                    c.project.as_deref().unwrap_or("-")
                );
            }
        }
        What::LinkSecret { db, id, revoke } => {
            let c = open(&db)?;
            if revoke {
                let had = crate::db::issuer::drop_client_secret(&c, &id)?;
                println!(
                    "{}",
                    if had { format!("client {id}'s secret revoked") } else { format!("client {id} had no secret") }
                );
            } else {
                let secret = format!("acs_{}", token());
                crate::db::issuer::set_client_secret(&c, &id, &hash(&secret), now_ms())?;
                eprintln!("client {id}'s secret (shown once; the old one, if any, stopped working):");
                println!("{secret}");
            }
        }
        What::LinkAlerts { db } => {
            for (client, what, count, last) in crate::db::issuer::link_alerts(&open(&db)?)? {
                println!("{client}\t{what}\t{count}\tlast {last}");
            }
        }
        What::RemoveClient { db, id } => {
            let gone = crate::db::issuer::remove_client(&mut open(&db)?, &id)?;
            println!("{}", if gone { format!("client {id} removed") } else { format!("no client {id}") });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(key: Option<String>, next: Option<String>, previous: Option<String>, until: Option<u64>) -> KeyArgs {
        KeyArgs { key, next, previous, previous_until: until.map(|u| u.to_string()) }
    }

    #[test]
    fn keys_from_config() {
        let (a, b) = (IssuerKey::generate(), IssuerKey::generate());
        let now = 1_800_000_000;
        let hexa = || Some(a.seed_hex());
        let hexb = || Some(b.seed_hex());
        assert!(Keys::from_config(&args(None, None, None, None), now).unwrap().is_none());
        assert!(Keys::from_config(&args(Some(String::new()), None, Some(String::new()), None), now).unwrap().is_none());
        // Next or previous with nothing that signs is a mistake, not off.
        assert!(Keys::from_config(&args(None, hexa(), None, None), now).is_err());
        let k = Keys::from_config(&args(hexa(), hexb(), Some(String::new()), None), now).unwrap().unwrap();
        assert_eq!(k.current.kid(), a.kid());
        assert_eq!(k.next.as_ref().unwrap().kid(), b.kid());
        assert!(k.previous.is_none());
        assert_eq!(k.jwks(now).keys.iter().map(|j| j.kid.clone()).collect::<Vec<_>>(), [a.kid(), b.kid()]);
        // Next is published, never trusted.
        assert_eq!(k.trusted(now).keys.iter().map(|j| j.kid.clone()).collect::<Vec<_>>(), [a.kid()]);
        // The same key twice, or one that isn't a key.
        assert!(Keys::from_config(&args(hexa(), None, hexa(), Some(now + 600)), now).is_err());
        assert!(Keys::from_config(&args(Some("not hex".into()), None, None, None), now).is_err());

        // A previous key needs an end, at most a day off, and is
        // published and trusted until then only.
        assert!(Keys::from_config(&args(hexb(), None, hexa(), None), now).is_err());
        assert!(Keys::from_config(&args(hexb(), None, hexa(), Some(now + MAX_PREVIOUS_SECS + 1)), now).is_err());
        let k = Keys::from_config(&args(hexb(), None, hexa(), Some(now + 600)), now).unwrap().unwrap();
        assert_eq!(k.jwks(now).keys.len(), 2);
        assert_eq!(k.trusted(now + 599).keys.len(), 2);
        assert_eq!(k.jwks(now + 600).keys.iter().map(|j| j.kid.clone()).collect::<Vec<_>>(), [b.kid()]);
        assert_eq!(k.trusted(now + 600).keys.len(), 1);
        // Already past at start: kept out, with a warning.
        let k = Keys::from_config(&args(hexb(), None, hexa(), Some(now - 1)), now).unwrap().unwrap();
        assert_eq!(k.jwks(now).keys.len(), 1);

        // From files: an empty one is none, except for the key that signs.
        let dir = std::env::temp_dir().join(format!("issuer-keys-{}", hex::encode(arugula_e2e::random::<6>())));
        std::fs::create_dir_all(&dir).unwrap();
        let (cur, next, until) = (dir.join("current"), dir.join("next"), dir.join("until"));
        std::fs::write(&cur, format!("{}\n", a.seed_hex())).unwrap();
        std::fs::write(&next, "").unwrap();
        std::fs::write(&until, format!("{}\n", now + 60)).unwrap();
        let f = |p: &std::path::Path| Some(format!("file:{}", p.display()));
        let k = Keys::from_config(&args(f(&cur), f(&next), None, None), now).unwrap().unwrap();
        assert_eq!((k.current.kid(), k.next.is_none()), (a.kid(), true));
        let k =
            Keys::from_config(&KeyArgs { key: hexb(), next: None, previous: f(&cur), previous_until: f(&until) }, now)
                .unwrap()
                .unwrap();
        assert_eq!(k.previous.as_ref().unwrap().1, now + 60);
        assert!(Keys::from_config(&args(f(&next), None, None, None), now).is_err());
        assert!(Keys::from_config(&args(f(&dir.join("missing")), None, None, None), now).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
