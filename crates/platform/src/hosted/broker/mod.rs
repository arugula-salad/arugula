//! The inference broker (M231, Arugula #29): Claude through the platform
//! for a project's members and for registered clients (studio's lobby),
//! with the platform setting the key on the way out ("The Claude
//! credential broker and its metering move to the platform", amending
//! "Hosted inference: strangers bring their own key", aru-096).
//!
//! `ANTHROPIC_BASE_URL=<control>/api/projects/<project>/inference/anthropic`:
//! the broker forwards what Claude Code and Anthropic's SDKs send there
//! (`POST /v1/messages`, `/v1/messages/count_tokens`, `GET /v1/models`)
//! to Anthropic, streamed both ways, with a key in place of whatever
//! credential the caller sent. **No key ever reaches the caller**: not a
//! box, not a browser, not a client. Headers are allowlisted both ways
//! ([`OUT`], [`BACK`]), and redirects aren't followed. Every refusal is a
//! 403 with a sentence saying who can fix it (Claude Code retries a 401 for
//! minutes, but shows a 403 at once), and a reason a client can read
//! (`arugula.reason`); too many calls at once is a 429, which it retries.
//!
//! **Who's asking** ([`asker`]): a member of the project (a session or a
//! CLI device, as the hub found out), or a registered client
//! ([`clients`]) by its credential, naming the account the call is for
//! (`arugula-account`, a member of the project) and, if its policy lets
//! it, who else pays (`arugula-sponsor`, `arugula-visitor`). **A client
//! acts for an account only with its consent**: each account it names has
//! let that client act for it ([`set_consent`], on the platform page; off
//! until then). A sponsor or visitor who isn't an account, or hasn't let
//! the client, is refused, never passed over.
//!
//! **Whose key pays** ([`payer`]), the lobby's order (`lobby/llm-proxy.mjs`
//! `requestPayer`, `payerCredential`), with the platform's keys in place
//! of the lobby's saved tokens:
//! 1. a visitor's own key, the client said (the lobby's grants);
//! 2. a sponsor's own key, the client said (the lobby's adoption);
//! 3. the project's own key (`project_inference_keys`, step 5's), for a
//!    member who edits it or owns it (never a viewer);
//! 4. the account's own key: its personal project's;
//! 5. the platform's key ([`Config::key`], held here, unlike step 5's,
//!    which only E2B's proxy holds), under a monthly cap per account: the
//!    client's ([`clients::Policy::house_usd`]: studio's lobby's
//!    `STUDIO_SPEND_CAP_USD`, any of its accounts), else, for an account
//!    on the beta allowlist (`ARUGULA_CLOUD_ACCOUNTS`),
//!    `ARUGULA_INFERENCE_MONTHLY_USD`; and at most
//!    `ARUGULA_INFERENCE_CONCURRENT` of its calls at once. On it, only the
//!    betas `ARUGULA_INFERENCE_PLATFORM_BETAS` names are sent, and the
//!    long-context ones are refused;
//! 6. else nothing pays: `no_key`, with how to add one.
//!
//! **Metering** ([`run`]): each `POST /v1/messages` is reserved before it
//! goes out ([`prices::reservation`]: its input as the dearest cache
//! write, and all of `max_tokens`), in the same transaction as the cap
//! check, under the account's lock ([`Db::reserve_call`]). The call then
//! runs in a task of its own, which outlives the request: it reads the
//! answer as it streams by ([`prices::Reader`], uncompressed) and settles
//! the call at its list price once the provider's final usage arrives (a
//! stream's `message_delta`, or a whole JSON message). Without it (the
//! caller went away, the gateway dropped an idle answer, the body was
//! cut short, the call was dropped while it went out) the reservation
//! stands. An answer that's an error costs nothing. Every call is kept,
//! whoever's key paid; only the platform's key has a cap. A project's
//! spend, its hosted agent turns beside it, is [`project_spend`]; an
//! account's month on the platform's key, as the lobby showed it, is
//! [`my_spend`]; what was spent on an account's own key, by whom, is
//! [`paid_by_me`].
//!
//! **What stays in the lobby**: which app a box's token is, its owner's
//! and sponsor's switches, the box's declared capabilities and the
//! visitor grants it mints and checks. The lobby refuses those before it
//! calls here, then names the project, the account, and the sponsor or
//! visitor that apply.

pub mod clients;
pub mod prices;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use arugula_control_common::{ApiError, Principal, err};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use crate::{
    Platform,
    auth::{Asked, Person},
    db::inference::{Call, Ended, MonthCap, Reserved},
    forge::links::membership,
    keys,
};
use clients::{Clients, Policy};
use prices::Reader;

/// Anthropic's API.
pub const ANTHROPIC: &str = "https://api.anthropic.com";
/// The largest request the broker forwards: Anthropic's own ceiling.
pub const MAX_BODY: usize = 32 * 1024 * 1024;
/// What the platform's key may spend each UTC month for an account on the
/// beta allowlist calling the broker itself, when nothing says otherwise:
/// the lobby's cap.
pub const MONTHLY_USD: f64 = 20.0;
/// How many of an account's calls may run on the platform's key at once.
pub const CONCURRENT: u64 = 4;
/// The header a client names the account a call is for in.
pub const ACCOUNT: &str = "arugula-account";
/// The header a client names a sponsor in: the account that pays for
/// `ACCOUNT`'s calls with its own key, when it has one.
pub const SPONSOR: &str = "arugula-sponsor";
/// The header a client names a visitor in: the account whose own key pays
/// for its own turn in another's project.
pub const VISITOR: &str = "arugula-visitor";
/// The answer's header saying whose key paid: `project`, `account
/// <account>` or `platform`.
pub const PAYS: &str = "arugula-pays";

/// The routes Anthropic's clients send that the broker forwards.
const ROUTES: [(Method, &str); 3] =
    [(Method::POST, "v1/messages"), (Method::POST, "v1/messages/count_tokens"), (Method::GET, "v1/models")];

/// The request headers forwarded to Anthropic; `anthropic-beta` too, as
/// [`betas`] filters it. Nothing else: never the caller's credential,
/// cookies, the hub's or the platform's own headers, `origin` or `referer`.
pub const OUT: [&str; 4] = ["content-type", "accept", "anthropic-version", "user-agent"];
/// The answer's headers passed back: what the SDKs read. Never
/// `set-cookie`, the organization's id, the rate-limit headers, `via` or
/// a proxy's.
pub const BACK: [&str; 5] = ["content-type", "request-id", "retry-after", "retry-after-ms", "x-should-retry"];
/// Betas that bill the long-context premium: never on the platform's key.
const LONG_CONTEXT: &str = "context-1m";

/// A Claude credential: an API key, or a `claude setup-token` OAuth token,
/// which needs a bearer and the OAuth beta. Never shown.
#[derive(Clone, PartialEq, Eq)]
pub struct Key(String);

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(..)")
    }
}

impl std::str::FromStr for Key {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Key(s.trim().to_owned()))
    }
}

impl Key {
    pub fn new(s: &str) -> Key {
        Key(s.trim().to_owned())
    }

    pub(crate) fn oauth(&self) -> bool {
        self.0.starts_with("sk-ant-oat")
    }

    /// The headers that make it work upstream (`credentialHeaders` in the
    /// lobby): `x-api-key` for an API key; for an OAuth token, a bearer and
    /// the OAuth beta added to the betas sent.
    fn headers(&self, betas: &[String]) -> Vec<(HeaderName, String)> {
        let mut all = betas.to_vec();
        if !self.oauth() {
            let mut out = vec![(HeaderName::from_static("x-api-key"), self.0.clone())];
            if !all.is_empty() {
                out.push((HeaderName::from_static("anthropic-beta"), all.join(",")));
            }
            return out;
        }
        if !all.iter().any(|b| b == "oauth-2025-04-20") {
            all.push("oauth-2025-04-20".into());
        }
        vec![
            (header::AUTHORIZATION, format!("Bearer {}", self.0)),
            (HeaderName::from_static("anthropic-beta"), all.join(",")),
        ]
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The broker's settings (`hosted::Args`).
#[derive(Clone)]
pub struct Config {
    /// The platform's own Anthropic key, held here for the calls the
    /// platform pays for. Unset: the platform's key pays for nothing
    /// through the broker.
    pub key: Option<Key>,
    /// Anthropic's API (a fake, in tests and on the testnet).
    pub upstream: String,
    /// What the platform's key may spend each UTC month for an account on
    /// the beta allowlist calling the broker itself.
    pub monthly_usd: f64,
    /// How many of an account's calls may run on the platform's key at
    /// once.
    pub concurrent: u64,
    /// The betas sent on the platform's key; the rest are dropped (the
    /// long-context ones refused).
    pub platform_betas: Vec<String>,
    pub clients: Clients,
    /// Its own client: no overall timeout (an answer streams for minutes),
    /// a read waits ten minutes at most, no redirect followed.
    pub http: reqwest::Client,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            key: None,
            upstream: ANTHROPIC.into(),
            monthly_usd: MONTHLY_USD,
            concurrent: CONCURRENT,
            platform_betas: vec![],
            clients: Clients::default(),
            http: http(),
        }
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("key", &self.key.as_ref().map(|_| "set"))
            .field("upstream", &self.upstream)
            .field("monthly_usd", &self.monthly_usd)
            .field("concurrent", &self.concurrent)
            .field("platform_betas", &self.platform_betas)
            .field("clients", &self.clients)
            .finish_non_exhaustive()
    }
}

/// The broker's HTTP client: no redirect is followed (a key goes only to
/// the URL it was set for).
pub fn http() -> reqwest::Client {
    arugula_control_common::roots::build(|| {
        reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .read_timeout(std::time::Duration::from_secs(600))
            .redirect(reqwest::redirect::Policy::none())
    })
    .unwrap_or_default()
}

impl Config {
    /// From the flags: the platform's key (empty is none), Anthropic's
    /// URL, the allowlist's monthly cap, the calls at once, the betas sent
    /// on the platform's key, and the registered clients.
    pub fn new(
        key: Option<Key>,
        upstream: &str,
        monthly_usd: f64,
        concurrent: u64,
        platform_betas: &[String],
        clients: &[String],
    ) -> anyhow::Result<Config> {
        let upstream =
            Some(upstream.trim()).filter(|u| !u.is_empty()).unwrap_or(ANTHROPIC).trim_end_matches('/').to_owned();
        anyhow::ensure!(
            upstream.starts_with("https://") || upstream.starts_with("http://"),
            "ARUGULA_INFERENCE_UPSTREAM is a URL, not {upstream:?}"
        );
        anyhow::ensure!(monthly_usd.is_finite() && monthly_usd >= 0.0, "ARUGULA_INFERENCE_MONTHLY_USD is dollars");
        let platform_betas: Vec<String> =
            platform_betas.iter().map(|b| b.trim().to_owned()).filter(|b| !b.is_empty()).collect();
        anyhow::ensure!(
            !platform_betas.iter().any(|b| b.starts_with(LONG_CONTEXT)),
            "ARUGULA_INFERENCE_PLATFORM_BETAS can't name a long-context beta"
        );
        Ok(Config {
            key: key.filter(|k| !k.is_empty()),
            upstream,
            monthly_usd,
            concurrent: concurrent.max(1),
            platform_betas,
            clients: Clients::new(clients::parse(clients)?),
            http: http(),
        })
    }

    /// Whether the broker has anything to do here: a key of the
    /// platform's, or a client to serve. A project's own keys can be kept
    /// for it then even with no hosted workspaces.
    pub fn on(&self) -> bool {
        self.key.is_some() || !self.clients.policies.is_empty()
    }
}

/// Who a call is for, as [`asker`] found out.
#[derive(Debug, Clone)]
pub struct Asker {
    pub project: String,
    /// The member, or the account the client named.
    pub account: String,
    /// Its role in the project: a viewer never spends the project's key.
    pub role: TeamRole,
    pub client: Option<(String, Policy)>,
    pub sponsor: Option<String>,
    pub visitor: Option<String>,
}

/// Whose key pays for a call.
#[derive(Debug, Clone, PartialEq)]
pub enum Pays {
    Project,
    /// This account's own key (its personal project's).
    Account(String),
    /// The platform's, under a monthly cap of this many dollars.
    Platform {
        cap_usd: f64,
    },
}

impl Pays {
    fn kind(&self) -> &'static str {
        match self {
            Pays::Project => "project",
            Pays::Account(_) => "account",
            Pays::Platform { .. } => "platform",
        }
    }

    fn header(&self) -> String {
        match self {
            Pays::Account(a) => format!("account {a}"),
            other => other.kind().into(),
        }
    }
}

/// A refusal, as the broker answers it: Anthropic's error shape, so
/// Claude Code shows the sentence, and `arugula` for a client to read.
#[derive(Debug)]
pub struct Refused {
    pub status: StatusCode,
    pub kind: &'static str,
    pub reason: &'static str,
    pub message: String,
    pub extra: Value,
}

impl Refused {
    fn new(status: StatusCode, reason: &'static str, message: impl Into<String>) -> Refused {
        let kind = match status {
            StatusCode::UNAUTHORIZED => "authentication_error",
            StatusCode::NOT_FOUND => "not_found_error",
            StatusCode::PAYLOAD_TOO_LARGE => "request_too_large",
            StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
            StatusCode::BAD_GATEWAY => "api_error",
            StatusCode::BAD_REQUEST => "invalid_request_error",
            _ => "permission_error",
        };
        Refused { status, kind, reason, message: message.into(), extra: json!({}) }
    }

    fn with(mut self, extra: Value) -> Refused {
        self.extra = extra;
        self
    }
}

impl IntoResponse for Refused {
    fn into_response(self) -> Response {
        let mut arugula = json!({ "reason": self.reason });
        if let (Some(a), Some(e)) = (arugula.as_object_mut(), self.extra.as_object()) {
            a.extend(e.clone());
        }
        let body =
            json!({ "type": "error", "error": { "type": self.kind, "message": self.message }, "arugula": arugula });
        (self.status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
    }
}

impl From<ApiError> for Refused {
    fn from(e: ApiError) -> Refused {
        let reason = match e.0 {
            StatusCode::UNAUTHORIZED => "sign_in",
            StatusCode::NOT_FOUND => "no_project",
            _ => "refused",
        };
        Refused::new(e.0, reason, e.1)
    }
}

impl From<anyhow::Error> for Refused {
    fn from(e: anyhow::Error) -> Refused {
        warn!(error = %format!("{e:#}"), "the inference broker failed");
        Refused::new(StatusCode::INTERNAL_SERVER_ERROR, "error", "The platform couldn't broker this call.")
    }
}

pub(crate) fn header_of<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty())
}

/// The bearer a request carries, if any.
pub(crate) fn bearer(headers: &HeaderMap) -> Option<&str> {
    let v = header_of(headers, "authorization")?;
    let (scheme, token) = v.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then_some(token.trim()).filter(|t| !t.is_empty())
}

/// Whether `account` is an account at all: it has a personal project.
async fn personal(app: &Platform, account: &str) -> Result<Option<String>, Refused> {
    crate::projects::personal_project(app, account).await.map_err(|e| Refused::from(anyhow::anyhow!("{e:#}")))
}

/// `account` has let `client` act for it, or the refusal that says how to.
fn consent(app: &Platform, account: &str, client: &str, as_what: &str) -> Result<(), Refused> {
    if app.db.consented(account, client)? {
        return Ok(());
    }
    Err(Refused::new(
        StatusCode::FORBIDDEN,
        "consent",
        format!(
            "{account} ({as_what}) hasn't let {client} use Claude through Arugula for them. They turn it on in Arugula's \
             settings, under Claude."
        ),
    )
    .with(json!({ "account": account, "client": client })))
}

/// Who's asking for `project`: a registered client by its bearer, naming
/// a member of the project that let it; else a member, as the hub vouched
/// for them. Membership is read fresh.
pub async fn asker(
    app: &Platform,
    headers: &HeaderMap,
    asked: Option<&Asked>,
    project: &str,
) -> Result<Asker, Refused> {
    if let Some(credential) = bearer(headers) {
        let Some((client, policy)) = app.hosted.broker.clients.who(credential).await? else {
            return Err(Refused::new(
                StatusCode::UNAUTHORIZED,
                "client",
                "That isn't a registered client's credential.",
            ));
        };
        let Some(account) = header_of(headers, ACCOUNT).map(str::to_owned) else {
            return Err(Refused::new(
                StatusCode::BAD_REQUEST,
                "account",
                format!("Name the account the call is for in {ACCOUNT}."),
            ));
        };
        consent(app, &account, &client, "the account")?;
        let (project, role) = membership(app, &account, project, true).await?;
        let named = |h: &str| header_of(headers, h).map(str::to_owned);
        let (sponsor, visitor) = (named(SPONSOR), named(VISITOR));
        if (sponsor.is_some() || visitor.is_some()) && !policy.names_payers {
            return Err(Refused::new(StatusCode::FORBIDDEN, "payers", format!("{client} may not name who pays.")));
        }
        for (who, as_what) in [(&sponsor, "the sponsor"), (&visitor, "the visitor")] {
            let Some(who) = who else { continue };
            if personal(app, who).await?.is_none() {
                return Err(Refused::new(
                    StatusCode::FORBIDDEN,
                    "payer_unknown",
                    format!("{who} ({as_what}) isn't an account here."),
                )
                .with(json!({ "account": who })));
            }
            consent(app, who, &client, as_what)?;
        }
        return Ok(Asker { project, account, role, client: Some((client, policy)), sponsor, visitor });
    }
    let account = match asked {
        Some(Asked::Principal(Principal::Person { account } | Principal::Cli { account, .. })) => account.clone(),
        Some(Asked::Refused { status, error, .. }) => return Err(Refused::new(*status, "sign_in", error.clone())),
        _ => return Err(Refused::new(StatusCode::UNAUTHORIZED, "sign_in", "Sign in first.")),
    };
    let (project, role) = membership(app, &account, project, true).await?;
    Ok(Asker { project, account, role, client: None, sponsor: None, visitor: None })
}

/// A project's own Anthropic key, opened.
fn project_key(app: &Platform, project: &str) -> anyhow::Result<Option<Key>> {
    let Some((_, sealed)) = app
        .db
        .sealed_inference_keys(project)?
        .into_iter()
        .find(|(p, _)| p == super::inference::Provider::Anthropic.as_str())
    else {
        return Ok(None);
    };
    let cipher = app.hosted.cipher.as_deref().ok_or_else(|| {
        anyhow::anyhow!("project {project} has its own Anthropic key, and this control has no key to open it")
    })?;
    let key = keys::open_inference(&app.db.c(), cipher, project, "anthropic", &sealed)
        .map_err(|e| anyhow::anyhow!("project {project}'s own Anthropic key doesn't open: {e:#}"))?;
    Ok(Some(Key::new(&key)))
}

/// An account's own key: its personal project's.
async fn own_key(app: &Platform, account: &str) -> Result<Option<Key>, Refused> {
    let Some(p) = personal(app, account).await? else { return Ok(None) };
    Ok(project_key(app, &p)?)
}

/// Whose key pays for a call for `a`, with the key, or why none does (see
/// the module's docs for the order). A key that can't be opened is an
/// error, never a reason to pay with the platform's. The platform's key
/// comes with its cap; the cap itself is checked when the call is
/// reserved.
pub async fn payer(app: &Platform, a: &Asker) -> Result<(Pays, Key), Refused> {
    if let Some(v) = &a.visitor
        && let Some(k) = own_key(app, v).await?
    {
        return Ok((Pays::Account(v.clone()), k));
    }
    if let Some(s) = &a.sponsor
        && let Some(k) = own_key(app, s).await?
    {
        return Ok((Pays::Account(s.clone()), k));
    }
    if a.role != TeamRole::Viewer
        && let Some(k) = project_key(app, &a.project)?
    {
        return Ok((Pays::Project, k));
    }
    if let Some(k) = own_key(app, &a.account).await? {
        return Ok((Pays::Account(a.account.clone()), k));
    }
    let broker = &app.hosted.broker;
    let cap = match &a.client {
        Some((_, p)) if p.house_usd > 0.0 => Some(p.house_usd),
        _ if app.hosted.inference.allowlisted(&a.account) => Some(broker.monthly_usd),
        _ => None,
    };
    match (cap.filter(|c| *c > 0.0), broker.key.clone()) {
        (Some(cap_usd), Some(key)) => Ok((Pays::Platform { cap_usd }, key)),
        _ => Err(no_key(a)),
    }
}

fn no_key(a: &Asker) -> Refused {
    let project = if a.role == TeamRole::Viewer {
        format!("a viewer of project {} can't spend its key", a.project)
    } else {
        format!("project {} has none", a.project)
    };
    Refused::new(
        StatusCode::FORBIDDEN,
        "no_key",
        format!(
            "No Anthropic key pays for this call: {project}, {account} has none of their own, and Arugula's key doesn't \
             cover them. An owner of the project adds one with `arugula cloud key set anthropic --project {id}`, or \
             {account} adds their own with `arugula cloud key set anthropic --project personal`. It's never handed out.",
            account = a.account,
            id = a.project,
        ),
    )
}

/// The betas a call sends: as asked on a key of the project's or an
/// account's; on the platform's, only those `allowed`, and none that bills
/// the long-context premium.
fn betas(asked: Option<&str>, pays: &Pays, allowed: &[String]) -> Result<Vec<String>, Refused> {
    let asked: Vec<String> =
        asked.unwrap_or("").split(',').map(str::trim).filter(|b| !b.is_empty()).map(str::to_owned).collect();
    if !matches!(pays, Pays::Platform { .. }) {
        return Ok(asked);
    }
    if let Some(b) = asked.iter().find(|b| b.starts_with(LONG_CONTEXT)) {
        return Err(Refused::new(
            StatusCode::FORBIDDEN,
            "beta",
            format!("{b} isn't available on Arugula's key: add your own key to use it."),
        ));
    }
    Ok(asked.into_iter().filter(|b| allowed.contains(b)).collect())
}

/// Dollars from millionths, to a hundredth of a cent.
fn usd(micros: u64) -> f64 {
    (micros as f64 / 100.0).round() / 1e4
}

/// `ANY /api/projects/{project}/inference/anthropic/{*rest}`: the broker
/// (the module's docs).
pub async fn forward(
    State(app): State<Arc<Platform>>,
    Path((project, rest)): Path<(String, String)>,
    req: Request,
) -> Response {
    match broker(app, &project, &rest, req).await {
        Ok(r) => r,
        Err(refused) => refused.into_response(),
    }
}

/// A model id in a path: a letter or digit first, then those, `-`, `.`
/// and `_`.
fn model_id(id: &str) -> bool {
    id.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_')
}

async fn broker(app: Arc<Platform>, project: &str, rest: &str, req: Request) -> Result<Response, Refused> {
    let method = req.method().clone();
    // Claude Code's hello, with no credential.
    if rest == "api/hello" && (method == Method::HEAD || method == Method::GET) {
        return Ok((StatusCode::OK, Json(json!({}))).into_response());
    }
    let model = rest.strip_prefix("v1/models/").is_some_and(model_id);
    if !(ROUTES.iter().any(|(m, p)| *m == method && *p == rest) || (method == Method::GET && model)) {
        return Err(Refused::new(
            StatusCode::NOT_FOUND,
            "route",
            format!("The broker doesn't forward {method} /{rest}."),
        ));
    }
    let (parts, body) = req.into_parts();
    let a = asker(&app, &parts.headers, parts.extensions.get::<Asked>(), project).await?;
    let (pays, key) = payer(&app, &a).await?;
    let broker = &app.hosted.broker;
    // Every anthropic-beta header the caller sent, not only the first.
    let asked_betas: Vec<&str> =
        parts.headers.get_all("anthropic-beta").iter().filter_map(|v| v.to_str().ok()).collect();
    let sent_betas = betas(Some(&asked_betas.join(",")), &pays, &broker.platform_betas)?;
    let body = axum::body::to_bytes(body, MAX_BODY)
        .await
        .map_err(|_| Refused::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", "Request is larger than 32 MB."))?;

    // A message is reserved before it goes out, and its cap checked with it.
    let metered = method == Method::POST && rest == "v1/messages";
    let call = if metered { Some(reserve(&app, &a, &pays, &body)?) } else { None };

    let mut headers = reqwest::header::HeaderMap::new();
    for name in OUT {
        for v in parts.headers.get_all(name) {
            headers.append(HeaderName::from_static(name), v.clone());
        }
    }
    for (k, v) in key.headers(&sent_betas) {
        let mut v = HeaderValue::from_str(&v).map_err(|_| anyhow::anyhow!("a key that isn't a header value"))?;
        v.set_sensitive(true);
        headers.insert(k, v);
    }
    headers.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    let query = parts.uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    let url = format!("{}/{rest}{query}", broker.upstream);
    let request = broker.http.request(method, &url).headers(headers).body(body);

    let (head_tx, head_rx) = oneshot::channel();
    let (body_tx, body_rx) = mpsc::channel(16);
    tokio::spawn(run(app.clone(), request, call, a.clone(), pays.clone(), head_tx, body_tx));
    let Ok(head) = head_rx.await else {
        return Err(Refused::new(StatusCode::BAD_GATEWAY, "upstream", "The platform couldn't reach Anthropic."));
    };
    let (status, back) = head?;
    let mut out = Response::builder().status(status.as_u16());
    if let Some(h) = out.headers_mut() {
        for name in BACK {
            for v in back.get_all(name) {
                // Never anything that says the key back.
                if !v.as_bytes().windows(key.0.len().max(1)).any(|w| w == key.0.as_bytes()) {
                    h.append(HeaderName::from_static(name), v.clone());
                }
            }
        }
        h.insert(
            HeaderName::from_static(PAYS),
            HeaderValue::from_str(&pays.header()).unwrap_or(HeaderValue::from_static("")),
        );
    }
    let stream = futures_util::stream::unfold(body_rx, |mut rx| async move { rx.recv().await.map(|c| (c, rx)) });
    Ok(out.body(Body::from_stream(stream)).map_err(|e| anyhow::anyhow!("{e}"))?)
}

/// Reserve a message for `a` before it goes out (see the module's docs):
/// its id. On the platform's key, refused at the account's monthly cap
/// (403 `cap`, with when it starts again) or with too many of its calls
/// running (429 `busy`).
fn reserve(app: &Platform, a: &Asker, pays: &Pays, body: &Bytes) -> Result<String, Refused> {
    let asked: Value = serde_json::from_slice(body).unwrap_or_default();
    let model = asked.get("model").and_then(Value::as_str).unwrap_or("").to_owned();
    let fast = asked.get("speed").and_then(Value::as_str) == Some("fast");
    let reserved = prices::reservation(&model, body.len(), asked.get("max_tokens").and_then(Value::as_u64), fast);
    let now = now_ms();
    let call = Call {
        id: format!("ic_{}", hex::encode(arugula_e2e::random::<16>())),
        project: a.project.clone(),
        account: Some(a.account.clone()),
        client: a.client.as_ref().map(|(c, _)| c.clone()),
        provider: "anthropic".into(),
        pays: pays.kind().into(),
        payer: match pays {
            Pays::Account(w) => Some(w.clone()),
            _ => None,
        },
        state: "open".into(),
        model: Some(model).filter(|m| !m.is_empty()),
        status: 0,
        input: 0,
        output: 0,
        cache_write: 0,
        cache_write_1h: 0,
        cache_read: 0,
        web_searches: 0,
        reserved_micros: reserved,
        cost_micros: reserved,
        priced_as: None,
        started_at: now,
        ended_at: None,
    };
    let broker = &app.hosted.broker;
    let cap = match pays {
        Pays::Platform { cap_usd } => Some(MonthCap {
            micros: prices::micros(*cap_usd),
            since: prices::month_start(now),
            concurrent: broker.concurrent,
        }),
        _ => None,
    };
    let client = call.client.as_deref().unwrap_or("-").to_owned();
    match app.db.reserve_call(&call, cap.as_ref())? {
        Reserved::Yes => Ok(call.id),
        Reserved::Capped(spent) => {
            let cap_usd = match pays {
                Pays::Platform { cap_usd } => *cap_usd,
                _ => 0.0,
            };
            let resets = prices::day_of(prices::next_month(now));
            info!(account = %a.account, %client, spent, "an account is at its monthly cap on the platform's key");
            Err(Refused::new(
                StatusCode::FORBIDDEN,
                "cap",
                format!(
                    "{account} has used this month's ${cap_usd} of Claude on Arugula's key. It starts again on {resets}, \
                     or they can add their own Anthropic key (`arugula cloud key set anthropic --project personal`), \
                     which has no cap.",
                    account = a.account,
                ),
            )
            .with(json!({ "cap_usd": cap_usd, "spent_usd": usd(spent), "resets": resets, "month": prices::month_of(now) })))
        }
        Reserved::Busy(running) => {
            info!(account = %a.account, %client, running, "an account has as many calls running on the platform's key as it may");
            Err(Refused::new(
                StatusCode::TOO_MANY_REQUESTS,
                "busy",
                format!("{} has {running} calls running on Arugula's key, as many as it may at once. Try again in a moment.", a.account),
            )
            .with(json!({ "concurrent": broker.concurrent })))
        }
    }
}

/// What [`run`] tells the request: the answer's status and headers, or why
/// there's none.
type Head = Result<(StatusCode, reqwest::header::HeaderMap), Refused>;

/// A call, in a task of its own that outlives the request: send it, hand
/// the answer's head and then its body back while the caller's there, and
/// end the call (`call`: a message reserved) when the answer ends or the
/// caller goes. Settled at its list price only once the provider's final
/// usage arrived; an error answer costs nothing; otherwise its reservation
/// stands.
async fn run(
    app: Arc<Platform>,
    request: reqwest::RequestBuilder,
    call: Option<String>,
    a: Asker,
    pays: Pays,
    head: oneshot::Sender<Head>,
    body: mpsc::Sender<Result<Bytes, std::io::Error>>,
) {
    let end = |status: u16, e: Ended| {
        if let Some(id) = &call
            && let Err(error) = app.db.end_call(id, &Ended { status, ..e }, now_ms())
        {
            warn!(project = %a.project, error = %format!("{error:#}"), "couldn't end a brokered call");
        }
    };
    let mut answer = match request.send().await {
        Ok(answer) => answer,
        Err(e) => {
            warn!(project = %a.project, error = %e.without_url(), "the inference broker couldn't reach Anthropic");
            // It may have reached Anthropic: the reservation stands.
            end(0, Ended::default());
            let _ = head.send(Err(Refused::new(
                StatusCode::BAD_GATEWAY,
                "upstream",
                "The platform couldn't reach Anthropic.",
            )));
            return;
        }
    };
    let status = answer.status();
    // An error answer bills nothing.
    let free = || Ended { settled: Some((0, String::new())), ..Ended::default() };
    if status == StatusCode::UNAUTHORIZED {
        end(status.as_u16(), free());
        let _ = head.send(Err(key_refused(&a, &pays)));
        return;
    }
    if head.send(Ok((status, answer.headers().clone()))).is_err() {
        // The caller went away while it went out.
        end(status.as_u16(), if status.is_success() { Ended::default() } else { free() });
        return;
    }
    let mut reader = Reader::default();
    loop {
        tokio::select! {
            chunk = answer.chunk() => match chunk {
                Ok(Some(b)) => {
                    reader.feed(&b);
                    if body.send(Ok(b)).await.is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = body.send(Err(std::io::Error::other(e.without_url()))).await;
                    break;
                }
            },
            () = body.closed() => break,
        }
    }
    drop(answer);
    if !status.is_success() {
        return end(status.as_u16(), free());
    }
    let Some((model, u, last)) = reader.result() else { return end(status.as_u16(), Ended::default()) };
    let settled = last.then(|| {
        let (cost, row) = prices::cost_of(&model, &u);
        (prices::micros(cost), row.to_owned())
    });
    end(
        status.as_u16(),
        Ended {
            status: 0,
            model: Some(model).filter(|m| !m.is_empty()),
            input: u.input,
            output: u.output,
            cache_write: u.cache_write,
            cache_write_1h: u.cache_write_1h,
            cache_read: u.cache_read,
            web_searches: u.web_searches,
            settled,
        },
    );
}

/// Anthropic refused the key that paid (401): who can fix it, never the
/// key.
fn key_refused(a: &Asker, pays: &Pays) -> Refused {
    let message = match pays {
        Pays::Project => format!(
            "Anthropic refused project {p}'s own key. An owner of the project sets a new one with \
             `arugula cloud key set anthropic --project {p}`.",
            p = a.project
        ),
        Pays::Account(who) => format!(
            "Anthropic refused {who}'s own key. They set a new one with `arugula cloud key set anthropic --project personal`."
        ),
        Pays::Platform { .. } => {
            warn!(project = %a.project, "Anthropic refused the platform's own key (401)");
            "Anthropic refused Arugula's own key. The operator has to replace it; meanwhile the project can add its own \
             (`arugula cloud key set anthropic --project <project>`)."
                .into()
        }
    };
    Refused::new(StatusCode::FORBIDDEN, "key_refused", message).with(json!({
        "pays": pays.kind(),
        "payer": match pays {
            Pays::Account(w) => Some(w.as_str()),
            _ => None,
        }
    }))
}

/// `?month=2026-10`: which UTC month (this one when left out).
#[derive(Deserialize)]
pub struct Month {
    pub month: Option<String>,
}

fn month(q: &Month, now: u64) -> Result<u64, ApiError> {
    match &q.month {
        None => Ok(prices::month_start(now)),
        Some(m) => prices::parse_month(m).ok_or_else(|| err(StatusCode::BAD_REQUEST, "month: YYYY-MM")),
    }
}

/// A line of [`ProjectSpend`]: an account's calls through a client (or
/// none) on one kind of key.
#[derive(Debug, Serialize)]
pub struct SpendLine {
    pub account: Option<String>,
    pub client: Option<String>,
    pub pays: String,
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub usd: f64,
}

/// `GET /api/projects/{project}/inference/spend`.
#[derive(Debug, Serialize)]
pub struct ProjectSpend {
    pub project: String,
    pub month: String,
    pub usd: f64,
    /// Of `usd`, what the platform's key paid.
    pub platform_usd: f64,
    pub lines: Vec<SpendLine>,
    /// Agent turns on the project's hosted boxes (step 5): their calls go
    /// through E2B's proxy, which doesn't see their usage, so they're
    /// counted, not priced.
    pub hosted: crate::db::inference::Turns,
}

/// What a project's Claude calls cost in a month, by account, client and
/// whose key (a reservation counting until its call is settled), with its
/// hosted agent turns beside them: for its members.
pub async fn project_spend(
    State(app): State<Arc<Platform>>,
    crate::cloud::CloudDriver(Person { account }): crate::cloud::CloudDriver,
    Path(project): Path<String>,
    Query(q): Query<Month>,
) -> Result<Json<ProjectSpend>, ApiError> {
    let (project, _) = membership(&app, &account, &project, false).await?;
    let now = now_ms();
    let since = month(&q, now)?;
    let until = prices::next_month(since);
    let rows = app.db.project_spend(&project, since, until)?;
    let total: u64 = rows.iter().map(|r| r.cost_micros).sum();
    let platform: u64 = rows.iter().filter(|r| r.pays == "platform").map(|r| r.cost_micros).sum();
    let lines = rows
        .into_iter()
        .map(|r| SpendLine {
            account: r.account,
            client: r.client,
            pays: r.pays,
            calls: r.calls,
            input_tokens: r.input_tokens,
            output_tokens: r.output_tokens,
            usd: usd(r.cost_micros),
        })
        .collect();
    let hosted = app.db.project_turns(&project, since, until, now)?;
    Ok(Json(ProjectSpend {
        project,
        month: prices::month_of(since),
        usd: usd(total),
        platform_usd: usd(platform),
        lines,
        hosted,
    }))
}

/// `GET /api/inference/spend`: an account's month on the platform's key,
/// as the lobby's meter showed it (`summary` in `lobby/metering.mjs`):
/// a member's own, or, for a client, the account it names (which let it),
/// through it.
#[derive(Debug, Serialize)]
pub struct MySpend {
    pub account: String,
    pub client: Option<String>,
    pub month: String,
    pub spent_usd: f64,
    /// Zero: the platform's key doesn't pay for it.
    pub cap_usd: f64,
}

pub async fn my_spend(State(app): State<Arc<Platform>>, req: Request) -> Response {
    match spend_of(&app, req).await {
        Ok(r) => r.into_response(),
        Err(r) => r.into_response(),
    }
}

async fn spend_of(app: &Platform, req: Request) -> Result<Json<MySpend>, Refused> {
    let (parts, _) = req.into_parts();
    let now = now_ms();
    let broker = &app.hosted.broker;
    let (account, client, cap) = if let Some(credential) = bearer(&parts.headers) {
        let Some((client, policy)) = broker.clients.who(credential).await? else {
            return Err(Refused::new(
                StatusCode::UNAUTHORIZED,
                "client",
                "That isn't a registered client's credential.",
            ));
        };
        let Some(account) = header_of(&parts.headers, ACCOUNT) else {
            return Err(Refused::new(StatusCode::BAD_REQUEST, "account", format!("Name the account in {ACCOUNT}.")));
        };
        consent(app, account, &client, "the account")?;
        let cap = if policy.house_usd > 0.0 {
            policy.house_usd
        } else if app.hosted.inference.allowlisted(account) {
            broker.monthly_usd
        } else {
            0.0
        };
        (account.to_owned(), Some(client), cap)
    } else {
        match parts.extensions.get::<Asked>() {
            Some(Asked::Principal(Principal::Person { account } | Principal::Cli { account, .. })) => {
                let cap = if app.hosted.inference.allowlisted(account) { broker.monthly_usd } else { 0.0 };
                (account.clone(), None, cap)
            }
            _ => return Err(Refused::new(StatusCode::UNAUTHORIZED, "sign_in", "Sign in first.")),
        }
    };
    let cap = if broker.key.is_some() { cap } else { 0.0 };
    let spent = app.db.platform_spend(&account, client.as_deref(), prices::month_start(now))?;
    Ok(Json(MySpend { account, client, month: prices::month_of(now), spent_usd: usd(spent), cap_usd: cap }))
}

/// `GET /api/inference/spend/paid`: what calls on the signed-in account's
/// own key cost in a month, by project, account and client: its own, and
/// those others made (a client naming it as a sponsor or a visitor).
#[derive(Debug, Serialize)]
pub struct PaidByMe {
    pub month: String,
    pub usd: f64,
    pub lines: Vec<PaidLine>,
}

#[derive(Debug, Serialize)]
pub struct PaidLine {
    pub project: String,
    pub account: Option<String>,
    pub client: Option<String>,
    pub calls: u64,
    pub usd: f64,
}

pub async fn paid_by_me(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Query(q): Query<Month>,
) -> Result<Json<PaidByMe>, ApiError> {
    let since = month(&q, now_ms())?;
    let rows = app.db.paid_by(&account, since, prices::next_month(since))?;
    let total = rows.iter().map(|r| r.cost_micros).sum();
    let lines = rows
        .into_iter()
        .map(|r| PaidLine {
            project: r.project,
            account: r.account,
            client: r.client,
            calls: r.calls,
            usd: usd(r.cost_micros),
        })
        .collect();
    Ok(Json(PaidByMe { month: prices::month_of(since), usd: usd(total), lines }))
}

/// `GET /api/inference/clients`: the registered clients, and which the
/// signed-in account lets act for it.
#[derive(Debug, Serialize)]
pub struct ClientConsent {
    pub client: String,
    pub allowed: bool,
    /// Whether it may name who pays (so spend this account's own key as a
    /// sponsor or a visitor).
    pub names_payers: bool,
}

pub async fn list_clients(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
) -> Result<Json<Vec<ClientConsent>>, ApiError> {
    let mine = app.db.consents(&account)?;
    Ok(Json(
        app.hosted
            .broker
            .clients
            .policies
            .iter()
            .map(|(c, p)| ClientConsent { client: c.clone(), allowed: mine.contains(c), names_payers: p.names_payers })
            .collect(),
    ))
}

/// `PUT` (let it) or `DELETE` (stop it) `/api/inference/clients/{client}/consent`:
/// whether a registered client may act for the signed-in account (name it
/// in a call, and spend its own key when it names it as the payer). Only
/// the page does this: it's trust, never a CLI's or an agent's.
pub async fn set_consent(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(client): Path<String>,
    method: Method,
) -> Result<Json<Vec<ClientConsent>>, ApiError> {
    if !app.hosted.broker.clients.policies.contains_key(&client) {
        return Err(err(StatusCode::NOT_FOUND, "no such client"));
    }
    let on = method == Method::PUT;
    app.db.set_consent(&account, &client, on, now_ms())?;
    info!(%account, %client, on, "an account changed what a client may do for it");
    app.analytics.capture(&account, "inference client consent", json!({ "client": client, "on": on }));
    list_clients(State(app), Person { account }).await
}
