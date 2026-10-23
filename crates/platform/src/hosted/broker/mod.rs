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
//! box, not a browser, not a client. Every refusal is a 403 with a
//! sentence saying who can fix it (Claude Code retries a 401 for minutes,
//! but shows a 403 at once), and a reason a client can read
//! (`arugula.reason`).
//!
//! **Who's asking** ([`Asker`]): a member of the project (a session or a
//! CLI device, as the hub found out), or a registered client
//! ([`clients`]) by its credential, naming the account the call is for
//! (`arugula-account`, a member of the project) and, if its policy lets
//! it, who else pays (`arugula-sponsor`, `arugula-visitor`).
//!
//! **Whose key pays** ([`payer`]), the lobby's order (`lobby/llm-proxy.mjs`
//! `requestPayer`, `payerCredential`), with the platform's keys in place
//! of the lobby's saved tokens:
//! 1. a visitor's own key, the client said (the lobby's grants);
//! 2. a sponsor's own key, the client said (the lobby's adoption);
//! 3. the project's own key (`project_inference_keys`, step 5's);
//! 4. the account's own key: its personal project's;
//! 5. the platform's key ([`Config::key`], held here, unlike step 5's,
//!    which only E2B's proxy holds), under a monthly cap per account: the
//!    client's ([`clients::Policy::house_usd`]: studio's lobby's
//!    `STUDIO_SPEND_CAP_USD`, any of its accounts), else, for an account
//!    on the beta allowlist (`ARUGULA_CLOUD_ACCOUNTS`),
//!    `ARUGULA_INFERENCE_MONTHLY_USD`. The cap is checked before a call
//!    goes out, against what its calls this UTC month cost, so a call
//!    under way finishes;
//! 6. else nothing pays: `no_key`, with how to add one.
//!
//! **Metering**: each `POST /v1/messages` that Anthropic answered is read
//! as it streams by ([`prices::Reader`], uncompressed), priced at list
//! prices ([`prices`]), and kept in `inference_calls` when it ends, cut
//! short or not ([`Db::record_call`](crate::db::Db::record_call)): per
//! project and per account, whoever's key paid. Only the platform's key
//! has a cap; the others are shown. A project's spend, its hosted agent
//! turns beside it, is [`project_spend`]; an account's month on the
//! platform's key, as the lobby showed it, is [`my_spend`].
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
use arugula_e2e::now_ms;
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{info, warn};

use crate::{Platform, auth::Asked, db::inference::Call, forge::links::membership, keys};
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

/// Request headers that describe the hop or the caller, not the request:
/// never forwarded. The caller's credential is among them, and anything
/// the hub or a client added for the platform.
const DROP: [&str; 15] = [
    "host",
    "x-api-key",
    "authorization",
    "cookie",
    "connection",
    "keep-alive",
    "proxy-authorization",
    "te",
    "upgrade",
    "forwarded",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
    "x-real-ip",
    "content-length",
];

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

    fn oauth(&self) -> bool {
        self.0.starts_with("sk-ant-oat")
    }

    /// The headers that make it work upstream (`credentialHeaders` in the
    /// lobby): `x-api-key` for an API key; for an OAuth token, a bearer and
    /// the OAuth beta added to the caller's betas.
    fn headers(&self, betas: Option<&str>) -> Vec<(HeaderName, String)> {
        if !self.oauth() {
            return vec![(HeaderName::from_static("x-api-key"), self.0.clone())];
        }
        let mut all: Vec<String> =
            betas.unwrap_or("").split(',').map(str::trim).filter(|b| !b.is_empty()).map(str::to_owned).collect();
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
    pub clients: Clients,
    /// Its own client: no overall timeout (an answer streams for minutes),
    /// a read waits ten minutes at most.
    pub http: reqwest::Client,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            key: None,
            upstream: ANTHROPIC.into(),
            monthly_usd: MONTHLY_USD,
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
            .field("clients", &self.clients)
            .finish_non_exhaustive()
    }
}

/// The broker's HTTP client.
pub fn http() -> reqwest::Client {
    arugula_control_common::roots::build(|| {
        reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .read_timeout(std::time::Duration::from_secs(600))
    })
    .unwrap_or_default()
}

impl Config {
    /// From the flags: the platform's key (empty is none), Anthropic's
    /// URL, the allowlist's monthly cap and the registered clients.
    pub fn new(key: Option<Key>, upstream: &str, monthly_usd: f64, clients: &[String]) -> anyhow::Result<Config> {
        let upstream =
            Some(upstream.trim()).filter(|u| !u.is_empty()).unwrap_or(ANTHROPIC).trim_end_matches('/').to_owned();
        anyhow::ensure!(
            upstream.starts_with("https://") || upstream.starts_with("http://"),
            "ARUGULA_INFERENCE_UPSTREAM is a URL, not {upstream:?}"
        );
        anyhow::ensure!(monthly_usd.is_finite() && monthly_usd >= 0.0, "ARUGULA_INFERENCE_MONTHLY_USD is dollars");
        Ok(Config {
            key: key.filter(|k| !k.is_empty()),
            upstream,
            monthly_usd,
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
    pub extra: serde_json::Value,
}

impl Refused {
    fn new(status: StatusCode, reason: &'static str, message: impl Into<String>) -> Refused {
        let kind = match status {
            StatusCode::UNAUTHORIZED => "authentication_error",
            StatusCode::NOT_FOUND => "not_found_error",
            StatusCode::PAYLOAD_TOO_LARGE => "request_too_large",
            StatusCode::BAD_GATEWAY => "api_error",
            StatusCode::BAD_REQUEST => "invalid_request_error",
            _ => "permission_error",
        };
        Refused { status, kind, reason, message: message.into(), extra: json!({}) }
    }

    fn with(mut self, extra: serde_json::Value) -> Refused {
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

fn header_of<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty())
}

/// The bearer a request carries, if any.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    let v = header_of(headers, "authorization")?;
    let (scheme, token) = v.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then_some(token.trim()).filter(|t| !t.is_empty())
}

/// Whether `account` is an account at all: it has a personal project.
async fn personal(app: &Platform, account: &str) -> Result<Option<String>, Refused> {
    crate::projects::personal_project(app, account).await.map_err(|e| Refused::from(anyhow::anyhow!("{e:#}")))
}

/// Who's asking for `project`: a registered client by its bearer, naming
/// a member of the project; else a member, as the hub vouched for them.
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
        let (project, _) = membership(app, &account, project, false).await?;
        let named = |h: &str| header_of(headers, h).map(str::to_owned);
        let (sponsor, visitor) = (named(SPONSOR), named(VISITOR));
        if (sponsor.is_some() || visitor.is_some()) && !policy.names_payers {
            return Err(Refused::new(StatusCode::FORBIDDEN, "payers", format!("{client} may not name who pays.")));
        }
        return Ok(Asker { project, account, client: Some((client, policy)), sponsor, visitor });
    }
    let account = match asked {
        Some(Asked::Principal(Principal::Person { account } | Principal::Cli { account, .. })) => account.clone(),
        Some(Asked::Refused { status, error, .. }) => return Err(Refused::new(*status, "sign_in", error.clone())),
        _ => return Err(Refused::new(StatusCode::UNAUTHORIZED, "sign_in", "Sign in first.")),
    };
    let (project, _) = membership(app, &account, project, false).await?;
    Ok(Asker { project, account, client: None, sponsor: None, visitor: None })
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
/// error, never a reason to pay with the platform's.
pub async fn payer(app: &Platform, a: &Asker, now: u64) -> Result<(Pays, Key), Refused> {
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
    if let Some(k) = project_key(app, &a.project)? {
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
    let (Some(cap_usd), Some(key)) = (cap.filter(|c| *c > 0.0), broker.key.clone()) else {
        return Err(no_key(a));
    };
    let client = a.client.as_ref().map(|(c, _)| c.as_str());
    let spent = app.db.platform_spend(&a.account, client, prices::month_start(now))?;
    if spent as f64 >= cap_usd * 1e6 {
        let resets = prices::day_of(prices::next_month(now));
        info!(account = %a.account, client = client.unwrap_or("-"), spent, "an account is at its monthly cap on the platform's key");
        return Err(Refused::new(
            StatusCode::FORBIDDEN,
            "cap",
            format!(
                "{account} has used this month's ${cap_usd} of Claude on Arugula's key. It starts again on {resets}, \
                 or they can add their own Anthropic key (`arugula cloud key set anthropic --project personal`), which has no cap.",
                account = a.account,
            ),
        )
        .with(json!({ "cap_usd": cap_usd, "spent_usd": usd(spent), "resets": resets, "month": prices::month_of(now) })));
    }
    Ok((Pays::Platform { cap_usd }, key))
}

fn no_key(a: &Asker) -> Refused {
    Refused::new(
        StatusCode::FORBIDDEN,
        "no_key",
        format!(
            "No Anthropic key pays for this call: project {project} has none, {account} has none of their own, \
             and Arugula's key doesn't cover them. An owner of the project adds one with \
             `arugula cloud key set anthropic --project {project}`, or {account} adds their own with \
             `arugula cloud key set anthropic --project personal`. It's never handed out.",
            project = a.project,
            account = a.account,
        ),
    )
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

async fn broker(app: Arc<Platform>, project: &str, rest: &str, req: Request) -> Result<Response, Refused> {
    let method = req.method().clone();
    // Claude Code's hello, with no credential.
    if rest == "api/hello" && (method == Method::HEAD || method == Method::GET) {
        return Ok((StatusCode::OK, Json(json!({}))).into_response());
    }
    let model = rest.strip_prefix("v1/models/").is_some_and(|id| {
        !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_')
    });
    if !(ROUTES.iter().any(|(m, p)| *m == method && *p == rest) || (method == Method::GET && model)) {
        return Err(Refused::new(
            StatusCode::NOT_FOUND,
            "route",
            format!("The broker doesn't forward {method} /{rest}."),
        ));
    }
    let (parts, body) = req.into_parts();
    let a = asker(&app, &parts.headers, parts.extensions.get::<Asked>(), project).await?;
    let now = now_ms();
    let (pays, key) = payer(&app, &a, now).await?;
    let body = axum::body::to_bytes(body, MAX_BODY)
        .await
        .map_err(|_| Refused::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", "Request is larger than 32 MB."))?;

    let metered = method == Method::POST && rest == "v1/messages";
    let mut headers = reqwest::header::HeaderMap::new();
    for (k, v) in &parts.headers {
        let name = k.as_str();
        if DROP.contains(&name) || name.starts_with("arugula-") || name.starts_with("x-arugula") {
            continue;
        }
        if metered && *k == header::ACCEPT_ENCODING {
            continue;
        }
        headers.append(k.clone(), v.clone());
    }
    let betas = header_of(&parts.headers, "anthropic-beta").map(str::to_owned);
    for (k, v) in key.headers(betas.as_deref()) {
        let mut v = HeaderValue::from_str(&v).map_err(|_| anyhow::anyhow!("a key that isn't a header value"))?;
        v.set_sensitive(true);
        headers.insert(k, v);
    }
    // Read for its usage as it streams, so it comes back uncompressed.
    if metered {
        headers.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    }
    let broker = &app.hosted.broker;
    let query = parts.uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    let url = format!("{}/{rest}{query}", broker.upstream);
    let answer = match broker.http.request(method.clone(), &url).headers(headers).body(body).send().await {
        Ok(a) => a,
        Err(e) => {
            warn!(project = %a.project, error = %e.without_url(), "the inference broker couldn't reach Anthropic");
            return Err(Refused::new(StatusCode::BAD_GATEWAY, "upstream", "The platform couldn't reach Anthropic."));
        }
    };
    let status = answer.status();
    if status == StatusCode::UNAUTHORIZED {
        return Err(key_refused(&a, &pays));
    }

    let mut out = Response::builder().status(status.as_u16());
    if let Some(h) = out.headers_mut() {
        for (k, v) in answer.headers() {
            // Never one hop's, and never anything that says the key back.
            if hop(k) || v.as_bytes().windows(key.0.len().max(1)).any(|w| w == key.0.as_bytes()) {
                continue;
            }
            h.append(k.clone(), v.clone());
        }
        h.insert(
            HeaderName::from_static(PAYS),
            HeaderValue::from_str(&pays.header()).unwrap_or(HeaderValue::from_static("")),
        );
    }
    // The answer as it comes, chunk by chunk: dropping it (the caller went
    // away) drops the call to Anthropic too.
    let stream = futures_util::stream::unfold(Some(answer), |a| async move {
        let mut a = a?;
        match a.chunk().await {
            Ok(Some(b)) => Some((Ok::<_, reqwest::Error>(b), Some(a))),
            Ok(None) => None,
            Err(e) => Some((Err(e), None)),
        }
    });
    let body = if metered && status.is_success() {
        let mut meter = Meter::new(app.clone(), &a, &pays, status.as_u16(), now);
        Body::from_stream(stream.map(move |chunk| {
            if let Ok(b) = &chunk {
                meter.reader.feed(b);
            }
            chunk
        }))
    } else {
        Body::from_stream(stream)
    };
    Ok(out.body(body).map_err(|e| anyhow::anyhow!("{e}"))?)
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
    Refused::new(StatusCode::FORBIDDEN, "key_refused", message)
        .with(json!({ "pays": pays.kind(), "payer": match pays { Pays::Account(w) => Some(w.as_str()), _ => None } }))
}

fn hop(k: &HeaderName) -> bool {
    matches!(
        k.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

/// A metered answer: read as it streams by, priced and kept when it's
/// dropped (it ended, or the caller went away mid-stream).
struct Meter {
    app: Arc<Platform>,
    call: Call,
    reader: Reader,
}

impl Meter {
    fn new(app: Arc<Platform>, a: &Asker, pays: &Pays, status: u16, now: u64) -> Meter {
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
            model: None,
            status,
            input: 0,
            output: 0,
            cache_write: 0,
            cache_write_1h: 0,
            cache_read: 0,
            cost_micros: 0,
            priced_as: None,
            started_at: now,
            ended_at: now,
        };
        Meter { app, call, reader: Reader::default() }
    }
}

impl Drop for Meter {
    fn drop(&mut self) {
        let mut call = self.call.clone();
        call.ended_at = now_ms();
        if let Some((model, u)) = std::mem::take(&mut self.reader).result() {
            let (cost, row) = prices::cost_of(&model, &u);
            call.model = Some(model).filter(|m| !m.is_empty());
            (call.input, call.output, call.cache_write, call.cache_write_1h, call.cache_read) =
                (u.input, u.output, u.cache_write, u.cache_write_1h, u.cache_read);
            call.cost_micros = prices::micros(cost);
            call.priced_as = Some(row.to_owned());
        }
        if let Err(e) = self.app.db.record_call(&call) {
            warn!(project = %call.project, error = %format!("{e:#}"), "couldn't keep a brokered call's usage");
        }
    }
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
/// whose key, with its hosted agent turns beside them: for its members.
pub async fn project_spend(
    State(app): State<Arc<Platform>>,
    crate::cloud::CloudDriver(crate::auth::Person { account }): crate::cloud::CloudDriver,
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
/// a member's own, or, for a client, the account it names, through it.
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
    let (parts, _) = req.into_parts();
    let now = now_ms();
    let (account, client, cap) = if let Some(credential) = bearer(&parts.headers) {
        let found = match app.hosted.broker.clients.who(credential).await {
            Ok(f) => f,
            Err(e) => return Refused::from(e).into_response(),
        };
        let Some((client, policy)) = found else {
            return Refused::new(StatusCode::UNAUTHORIZED, "client", "That isn't a registered client's credential.")
                .into_response();
        };
        let Some(account) = header_of(&parts.headers, ACCOUNT) else {
            return Refused::new(StatusCode::BAD_REQUEST, "account", format!("Name the account in {ACCOUNT}."))
                .into_response();
        };
        let cap = if policy.house_usd > 0.0 {
            policy.house_usd
        } else if app.hosted.inference.allowlisted(account) {
            app.hosted.broker.monthly_usd
        } else {
            0.0
        };
        (account.to_owned(), Some(client), cap)
    } else {
        match parts.extensions.get::<Asked>() {
            Some(Asked::Principal(Principal::Person { account } | Principal::Cli { account, .. })) => {
                let cap = if app.hosted.inference.allowlisted(account) { app.hosted.broker.monthly_usd } else { 0.0 };
                (account.clone(), None, cap)
            }
            _ => return Refused::new(StatusCode::UNAUTHORIZED, "sign_in", "Sign in first.").into_response(),
        }
    };
    let cap = if app.hosted.broker.key.is_some() { cap } else { 0.0 };
    match app.db.platform_spend(&account, client.as_deref(), prices::month_start(now)) {
        Ok(spent) => {
            Json(MySpend { account, client, month: prices::month_of(now), spent_usd: usd(spent), cap_usd: cap })
                .into_response()
        }
        Err(e) => Refused::from(e).into_response(),
    }
}
