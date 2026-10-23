//! The hub as an issuer (M228, platform step 8): other products (studio,
//! hud boxes, the platform page) ask the hub who someone is, and the hub
//! answers with tokens they check against its published keys. Identity
//! stays in the hub ("The hub stays the identity provider"); this only
//! says who someone is to products that aren't control's own page.
//!
//! OIDC-shaped, kept small:
//! - `GET /.well-known/openid-configuration` and `/.well-known/jwks.json`;
//! - `GET /oauth/authorize`: the authorization code flow with PKCE (S256
//!   only), for clients registered as rows (`db/issuer.rs`) with exact
//!   redirect addresses. Someone signed in to control's page gets a code at
//!   once; anyone else signs in there first (GitHub or a passkey, as
//!   always, `#issuer-signin=`) and comes back;
//! - `POST /oauth/token`: the code and its verifier for an ID token (a JWT
//!   signed EdDSA, minutes long) and an access token (opaque, minutes long,
//!   ending with the hub session it came from);
//! - `GET /oauth/userinfo`: who an access token is for;
//! - `GET /oauth/hud`: hud's forge assertion (`hudfs1`, hud's own format)
//!   for a box registered as a client, in the redirect's fragment.
//!
//! Tokens say who (the account, its numeric GitHub id) and never a project
//! or a role: a product reads those from the platform with the access
//! token. The token formats, and how anyone verifies them, are
//! `arugula_control_common::issuer`.
//!
//! Keys come from config, never the database: `ARUGULA_ISSUER_KEY` signs,
//! `ARUGULA_ISSUER_KEY_NEXT` is published before it signs, and
//! `ARUGULA_ISSUER_KEY_PREVIOUS` stays published after it stopped, until
//! its tokens have run out (docs/control-ops.md, "The issuer").

use std::{
    collections::HashMap,
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
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Redirect, Response},
};
use base64::Engine as _;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    App,
    auth::{SESSION_COOKIE, cookie, hash, token},
    db::issuer::Client,
};

/// How long a code lasts: a redirect and a call.
pub const CODE_MS: u64 = 60_000;

/// The issuer's keys: the one that signs, the one published before it
/// signs, and the one still published after it stopped.
#[derive(Debug, Clone)]
pub struct Keys {
    pub current: IssuerKey,
    pub next: Option<IssuerKey>,
    pub previous: Option<IssuerKey>,
}

/// A key as config gives it: 32 bytes of hex, or `file:<path>` holding
/// them. Empty (or an empty file) is none.
fn key_value(v: &str, name: &str) -> anyhow::Result<Option<IssuerKey>> {
    let v = v.trim();
    let text = match v.strip_prefix("file:") {
        Some(path) => std::fs::read_to_string(path).with_context(|| format!("{name}: reading {path}"))?,
        None => v.to_owned(),
    };
    if text.trim().is_empty() {
        return Ok(None);
    }
    IssuerKey::from_hex(&text).map(Some).with_context(|| name.to_owned())
}

impl Keys {
    /// From `ARUGULA_ISSUER_KEY`, `_NEXT` and `_PREVIOUS`: none when the
    /// first is unset or empty, and an error for anything that would sign
    /// with something other than what was meant.
    pub fn from_config(
        current: Option<&str>,
        next: Option<&str>,
        previous: Option<&str>,
    ) -> anyhow::Result<Option<Keys>> {
        let next = next.map(|v| key_value(v, "ARUGULA_ISSUER_KEY_NEXT")).transpose()?.flatten();
        let previous = previous.map(|v| key_value(v, "ARUGULA_ISSUER_KEY_PREVIOUS")).transpose()?.flatten();
        let current = match current.map(str::trim) {
            None | Some("") => None,
            Some(v) => Some(
                key_value(v, "ARUGULA_ISSUER_KEY")?
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
        let kids: Vec<&str> =
            [Some(&current), next.as_ref(), previous.as_ref()].into_iter().flatten().map(|k| k.kid()).collect();
        let mut unique = kids.clone();
        unique.sort_unstable();
        unique.dedup();
        anyhow::ensure!(unique.len() == kids.len(), "the issuer's keys must all differ: one key is set twice");
        Ok(Some(Keys { current, next, previous }))
    }

    /// Every key published: the one that signs first.
    pub fn jwks(&self) -> Jwks {
        Jwks {
            keys: [Some(&self.current), self.next.as_ref(), self.previous.as_ref()]
                .into_iter()
                .flatten()
                .map(IssuerKey::jwk)
                .collect(),
        }
    }

    /// For the log at start: which key is which, by id.
    pub fn describe(&self) -> String {
        let id = |k: &Option<IssuerKey>| k.as_ref().map_or("none".to_owned(), |k| k.kid().to_owned());
        format!("signs with {}, next {}, previous {}", self.current.kid(), id(&self.next), id(&self.previous))
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
}

impl Issuer {
    pub fn new(keys: Keys) -> Self {
        Issuer { keys, codes: Mutex::default() }
    }

    fn put(&self, code_hash: String, p: Pending, now: u64) {
        let mut m = self.codes.lock().unwrap();
        m.retain(|_, p| p.until > now);
        m.insert(code_hash, p);
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

    /// Checks an ID token this hub issued to `client` against its
    /// published keys, now: what a caller in this process (M229's link
    /// API) takes a token with. Elsewhere, `issuer::verify_id_token` with
    /// the JWKS.
    pub fn verify(&self, iss: &str, token: &str, client: &str) -> Result<IdClaims, Refused> {
        verify_id_token(token, &self.keys.jwks(), iss, client, now_ms() / 1000)
    }
}

fn b64url(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// A PKCE value as RFC 7636 has it: 43 to 128 of `A-Z a-z 0-9 - . _ ~`.
fn pkce_shaped(s: &str) -> bool {
    (43..=128).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
}

fn no_store(mut r: Response) -> Response {
    let h = r.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    r
}

fn off() -> Response {
    (StatusCode::NOT_FOUND, "this control isn't an issuer").into_response()
}

/// Shown, never redirected: a client or address that isn't registered
/// gets nothing sent anywhere.
fn not_registered() -> Response {
    no_store(
        (StatusCode::BAD_REQUEST, "this sign-in's client or its redirect address isn't registered with this control")
            .into_response(),
    )
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

/// The registered client and the address it named, exactly.
fn registered(app: &App, client_id: Option<&str>, redirect_uri: Option<&str>) -> Option<(Client, String)> {
    let (id, uri) = client_id.zip(redirect_uri)?;
    let c = app.db.issuer_client(id).ok().flatten()?;
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
    let mut r = Json(i.keys.jwks()).into_response();
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

/// `GET /oauth/authorize`: a code for a registered client, or control's
/// page to sign in first.
pub async fn authorize(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    uri: Uri,
    Query(q): Query<Authorize>,
) -> Response {
    let Some(issuer) = &app.issuer else { return off() };
    let Some((client, redirect_uri)) = registered(&app, q.client_id.as_deref(), q.redirect_uri.as_deref()) else {
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
    let code = token();
    issuer.put(
        hash(&code),
        Pending {
            account,
            hub_session,
            client: client.id,
            redirect_uri: redirect_uri.clone(),
            challenge: q.code_challenge.unwrap_or_default(),
            nonce: q.nonce,
            until: now + CODE_MS,
        },
        now,
    );
    let mut p = vec![("code", code.as_str()), ("iss", iss)];
    if !state.is_empty() {
        p.push(("state", state));
    }
    back(&redirect_uri, &p)
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

/// The account's numeric GitHub id, when it has one.
fn github_id(app: &App, account: &str) -> anyhow::Result<Option<(u64, String)>> {
    Ok(app.db.github_identity(account)?.and_then(|(id, login)| id.parse().ok().map(|id| (id, login))))
}

/// `POST /oauth/token`: a code, its client, address and verifier, for an
/// ID token and an access token.
pub async fn token_endpoint(State(app): State<Arc<App>>, Form(b): Form<TokenRequest>) -> Response {
    let Some(issuer) = &app.issuer else { return off() };
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
    // The client still registered, and the hub session it came from still
    // live and still theirs.
    if !app.db.issuer_client(&p.client).ok().flatten().is_some_and(|c| c.redirect_uris.contains(&p.redirect_uri)) {
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

/// `GET /oauth/userinfo`: who an access token is for.
pub async fn userinfo(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if app.issuer.is_none() {
        return off();
    }
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    let found = bearer.and_then(|t| app.db.issuer_token(&hash(t), now_ms()).ok().flatten());
    let Some((account, _client)) = found else {
        let mut r = no_store(StatusCode::UNAUTHORIZED.into_response());
        r.headers_mut().insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer error=\"invalid_token\""));
        return r;
    };
    let gh = github_id(&app, &account).ok().flatten();
    let name = app.db.account(&account).ok().flatten().map(|a| a.name).filter(|n| !n.is_empty());
    let mut body = serde_json::json!({ "sub": account });
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
}

/// `GET /oauth/hud`: hud's forge assertion for a box registered as a
/// client (its id is the box's `HUD_FORGE_SIGNIN_AUDIENCE`), in the
/// fragment of the box's own page: `#hud-forge=hudfs1.…`. hud signs a
/// session in as `github:<login>`, so the account needs a GitHub sign-in.
pub async fn hud(State(app): State<Arc<App>>, headers: HeaderMap, uri: Uri, Query(q): Query<HudStart>) -> Response {
    let Some(issuer) = &app.issuer else { return off() };
    let Some((client, redirect_uri)) = registered(&app, q.client_id.as_deref(), q.redirect_uri.as_deref()) else {
        return not_registered();
    };
    let now = now_ms();
    let Some((_, account)) = hub_session(&app, &headers, now) else { return sign_in_first(&uri) };
    let Ok(Some((github_id, login))) = github_id(&app, &account) else {
        return no_store(
            (
                StatusCode::FORBIDDEN,
                "hud signs you in as your GitHub login: sign in to Arugula with GitHub once, then try again",
            )
                .into_response(),
        );
    };
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
    };
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
        #[arg(long, env = "ARUGULA_ISSUER_KEY", hide_env_values = true)]
        key: Option<String>,
        #[arg(long, env = "ARUGULA_ISSUER_KEY_NEXT", hide_env_values = true)]
        next: Option<String>,
        #[arg(long, env = "ARUGULA_ISSUER_KEY_PREVIOUS", hide_env_values = true)]
        previous: Option<String>,
    },
    /// Register a client, or change one's name and redirect addresses.
    AddClient {
        #[arg(long, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
        db: std::path::PathBuf,
        /// Its id: what it sends as client_id, and a hud box's
        /// HUD_FORGE_SIGNIN_AUDIENCE.
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        /// An address a sign-in may go back to, compared whole; repeat it
        /// for more than one.
        #[arg(long = "redirect", required = true)]
        redirect_uris: Vec<String>,
    },
    /// The registered clients.
    Clients {
        #[arg(long, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
        db: std::path::PathBuf,
    },
    /// Take a client out, and the access tokens it holds.
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
        What::Keys { key, next, previous } => {
            let Some(keys) = Keys::from_config(key.as_deref(), next.as_deref(), previous.as_deref())? else {
                println!("no ARUGULA_ISSUER_KEY: the issuer is off");
                return Ok(());
            };
            for (slot, k) in
                [("current", Some(&keys.current)), ("next", keys.next.as_ref()), ("previous", keys.previous.as_ref())]
            {
                if let Some(k) = k {
                    println!("{slot:<8} kid {}  hud {}", k.kid(), k.spki_base64());
                }
            }
        }
        What::AddClient { db, id, name, redirect_uris } => {
            let c = open(&db)?;
            let created =
                crate::db::issuer::clients(&c)?.into_iter().find(|c| c.id == id).map_or(now_ms(), |c| c.created);
            crate::db::issuer::put_client(&c, &Client { id: id.clone(), name, redirect_uris, created })?;
            println!("client {id} registered");
        }
        What::Clients { db } => {
            for c in crate::db::issuer::clients(&open(&db)?)? {
                println!("{}\t{}\t{}", c.id, c.name, c.redirect_uris.join(" "));
            }
        }
        What::RemoveClient { db, id } => {
            let gone = crate::db::issuer::remove_client(&open(&db)?, &id)?;
            println!("{}", if gone { format!("client {id} removed") } else { format!("no client {id}") });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_from_config() {
        let (a, b) = (IssuerKey::generate(), IssuerKey::generate());
        assert!(Keys::from_config(None, None, None).unwrap().is_none());
        assert!(Keys::from_config(Some(""), None, Some("")).unwrap().is_none());
        // Next or previous with nothing that signs is a mistake, not off.
        assert!(Keys::from_config(None, Some(&a.seed_hex()), None).is_err());
        let k = Keys::from_config(Some(&a.seed_hex()), Some(&b.seed_hex()), Some("")).unwrap().unwrap();
        assert_eq!(k.current.kid(), a.kid());
        assert_eq!(k.next.as_ref().unwrap().kid(), b.kid());
        assert!(k.previous.is_none());
        assert_eq!(k.jwks().keys.iter().map(|j| j.kid.clone()).collect::<Vec<_>>(), [a.kid(), b.kid()]);
        // The same key twice, or one that isn't a key.
        assert!(Keys::from_config(Some(&a.seed_hex()), None, Some(&a.seed_hex())).is_err());
        assert!(Keys::from_config(Some("not hex"), None, None).is_err());

        // From files: an empty one is none, except for the key that signs.
        let dir = std::env::temp_dir().join(format!("issuer-keys-{}", hex::encode(arugula_e2e::random::<6>())));
        std::fs::create_dir_all(&dir).unwrap();
        let (cur, next) = (dir.join("current"), dir.join("next"));
        std::fs::write(&cur, format!("{}\n", a.seed_hex())).unwrap();
        std::fs::write(&next, "").unwrap();
        let f = |p: &std::path::Path| format!("file:{}", p.display());
        let k = Keys::from_config(Some(&f(&cur)), Some(&f(&next)), None).unwrap().unwrap();
        assert_eq!((k.current.kid(), k.next.is_none()), (a.kid(), true));
        assert!(Keys::from_config(Some(&f(&next)), None, None).is_err());
        assert!(Keys::from_config(Some(&f(&dir.join("missing"))), None, None).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
