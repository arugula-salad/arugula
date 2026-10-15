//! Fountain's REST API, typed: what the account view, the conversation link
//! and recipes on Fountain need (#942, F1). [`super::api`] is the older,
//! read-only reader the catalog uses.
//!
//! **Shapes** are Fountain's OpenAPI (`<base_url>/api/openapi.json`): a
//! record is `{data: …}`, a list `{data: [...]}`. Each type keeps the fields
//! Arugula reads and passes the rest through (`extra`), so a newer Fountain's
//! fields aren't lost on the way to a client.
//!
//! **Errors** ([`Error`]) are Fountain's: its body's `error` is the code to
//! branch on, except on the key and scope refusals, where `error` is a
//! sentence and `reason` the code. Each status becomes one variant, keeping
//! the code (`conversation_busy`, `sandbox_not_ready`, `fleet_full`, …).
//!
//! **Rate limits:** a 429 is refused before the operation runs, so any
//! request, a mutation too, is sent again after the `Retry-After` it names
//! (at most [`MAX_WAIT`], [`RETRIES_429`] times); then it is
//! [`Error::RateLimited`]. A 503 (`fleet_full`, `sandbox_unavailable`) keeps
//! its `Retry-After` for the caller, who decides whether a launch is worth
//! waiting for.
//!
//! The key is a bearer token, held in memory only: never logged (the
//! `Debug` impls leave it out) and never put in an error.

// The account view, the conversation link and recipes (F2 to F4) call
// most of this; F1 uses the device flow and the errors.
#![allow(dead_code)]

use std::{collections::BTreeMap, time::Duration};

use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};

use super::api::{Agent, Listing, rows};

/// How many times a 429 is waited out before it's an error.
pub const RETRIES_429: u32 = 3;
/// The longest `Retry-After` waited out in place.
pub const MAX_WAIT: Duration = Duration::from_secs(60);

/// What went wrong talking to Fountain. `code` is Fountain's
/// machine-readable code (empty when it sent none), `message` a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No answer: DNS, TLS, a refused connection, a timeout.
    Unreachable(String),
    /// 401: the key is wrong, expired or revoked.
    Unauthorized {
        code: String,
        message: String,
    },
    /// 403: the key may not (`insufficient_scope`: it isn't a full-scope key).
    Forbidden {
        code: String,
        message: String,
    },
    NotFound {
        code: String,
        message: String,
    },
    /// 409: `conversation_busy`, `sandbox_not_ready` (parked: reads don't
    /// wake it), `no_turn_running`, `chatgpt_reconnect_required`, …
    Conflict {
        code: String,
        message: String,
    },
    /// 410: the conversation or its sandbox is gone for good.
    Gone {
        code: String,
        message: String,
    },
    /// 400 or 422: the request was wrong (`invalid_prompt`,
    /// `validation_failed` with `fields`, `invalid_label_filter`, …).
    Invalid {
        status: u16,
        code: String,
        message: String,
        fields: Option<Value>,
    },
    /// 429, still, after [`RETRIES_429`] waits.
    RateLimited {
        retry_after: Duration,
    },
    /// 503: `fleet_full`, `sandbox_unavailable`; retry after `retry_after`.
    Unavailable {
        code: String,
        message: String,
        retry_after: Option<Duration>,
    },
    /// Any other status.
    Status {
        status: u16,
        code: String,
        message: String,
    },
    /// A success whose body isn't what the OpenAPI says.
    Unexpected(String),
}

impl Error {
    /// Fountain's code, when it sent one.
    pub fn code(&self) -> &str {
        match self {
            Error::Unauthorized { code, .. }
            | Error::Forbidden { code, .. }
            | Error::NotFound { code, .. }
            | Error::Conflict { code, .. }
            | Error::Gone { code, .. }
            | Error::Invalid { code, .. }
            | Error::Unavailable { code, .. }
            | Error::Status { code, .. } => code,
            Error::RateLimited { .. } => "rate_limited",
            Error::Unreachable(_) | Error::Unexpected(_) => "",
        }
    }

    /// The HTTP status, when there was one.
    pub fn status(&self) -> Option<u16> {
        Some(match self {
            Error::Unauthorized { .. } => 401,
            Error::Forbidden { .. } => 403,
            Error::NotFound { .. } => 404,
            Error::Conflict { .. } => 409,
            Error::Gone { .. } => 410,
            Error::Invalid { status, .. } | Error::Status { status, .. } => *status,
            Error::RateLimited { .. } => 429,
            Error::Unavailable { .. } => 503,
            Error::Unreachable(_) | Error::Unexpected(_) => return None,
        })
    }

    /// Whether asking again later can work: no answer, a rate limit, a 503
    /// or a 5xx. A 4xx is the same answer however often it's asked.
    pub fn transient(&self) -> bool {
        match self {
            Error::Unreachable(_) | Error::RateLimited { .. } | Error::Unavailable { .. } => true,
            Error::Status { status, .. } => *status >= 500,
            _ => false,
        }
    }

    /// The key itself was refused (401): signing in again is the fix.
    pub fn key_refused(&self) -> bool {
        matches!(self, Error::Unauthorized { .. })
    }

    /// How long Fountain asked to be left alone, if it did.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Error::RateLimited { retry_after } => Some(*retry_after),
            Error::Unavailable { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let said = |f: &mut std::fmt::Formatter<'_>, what: &str, code: &str, message: &str| match (code, message) {
            ("", "") => write!(f, "{what}"),
            (c, "") | ("", c) => write!(f, "{what}: {c}"),
            (c, m) if m == c => write!(f, "{what}: {c}"),
            (c, m) => write!(f, "{what}: {m} ({c})"),
        };
        match self {
            Error::Unreachable(e) => write!(f, "can't reach Fountain: {e}"),
            Error::Unauthorized { code, message } => said(f, "Fountain refused the key", code, message),
            Error::Forbidden { code, message } => said(f, "Fountain says this key may not", code, message),
            Error::NotFound { code, message } => said(f, "not found on Fountain", code, message),
            Error::Conflict { code, message } => said(f, "Fountain refused", code, message),
            Error::Gone { code, message } => said(f, "gone on Fountain", code, message),
            Error::Invalid { code, message, .. } => said(f, "Fountain says the request is wrong", code, message),
            Error::RateLimited { retry_after } => {
                write!(f, "Fountain's rate limit: try again in {}s", retry_after.as_secs().max(1))
            }
            Error::Unavailable { code, message, retry_after } => {
                said(f, "Fountain can't now", code, message)?;
                if let Some(r) = retry_after {
                    write!(f, " (try again in {}s)", r.as_secs().max(1))?;
                }
                Ok(())
            }
            Error::Status { status, code, message } => said(f, &format!("Fountain answered {status}"), code, message),
            Error::Unexpected(e) => write!(f, "Fountain said something else: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// The error a non-success status and body make (and `Retry-After`).
pub fn error_for(status: u16, body: &[u8], retry_after: Option<Duration>) -> Error {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let s = |k: &str| v[k].as_str().unwrap_or("").to_owned();
    // `{error: {message}}` from older ones, and the key refusals' sentence.
    let nested = v["error"]["message"].as_str().map(str::to_owned);
    let (code, message) = match (s("reason"), s("error"), s("message")) {
        (r, e, m) if !r.is_empty() => (r, if m.is_empty() { e } else { m }),
        (_, e, m) => (e, nested.or(Some(m)).unwrap_or_default()),
    };
    match status {
        401 => Error::Unauthorized { code, message },
        403 => Error::Forbidden { code, message },
        404 => Error::NotFound { code, message },
        409 => Error::Conflict { code, message },
        410 => Error::Gone { code, message },
        400 | 422 => {
            Error::Invalid { status, code, message, fields: v.get("errors").filter(|e| !e.is_null()).cloned() }
        }
        429 => Error::RateLimited { retry_after: retry_after.unwrap_or(Duration::from_secs(1)) },
        503 => Error::Unavailable { code, message, retry_after },
        _ => Error::Status { status, code, message },
    }
}

/// `Retry-After`, in seconds (Fountain sends seconds; an HTTP date is read
/// as a few seconds).
pub fn retry_after(h: &reqwest::header::HeaderMap) -> Option<Duration> {
    let v = h.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim();
    Some(v.parse::<u64>().map(Duration::from_secs).unwrap_or(Duration::from_secs(5)))
}

/// A path segment, percent-encoded (ids are UUIDs, but a request id or a
/// name is whatever it is).
pub fn seg(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

// ------------------------------------------------------------------ types

/// A conversation (`Conversation`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    /// pending, running, idle, failed or terminated.
    #[serde(default, deserialize_with = "null_default")]
    pub status: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub parent_conversation_id: Option<String>,
    #[serde(default)]
    pub sandbox_id: Option<String>,
    #[serde(default)]
    pub environment_id: Option<String>,
    #[serde(default)]
    pub vault_id: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub runtime: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub unread: bool,
    #[serde(default, deserialize_with = "null_default")]
    pub turn_count: u64,
    #[serde(default)]
    pub last_active_at: Option<String>,
    #[serde(default)]
    pub inserted_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `GET /api/conversations`' filters. `labels` are `key=value` pairs,
/// combined with AND.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConversationQuery {
    pub roots_only: bool,
    pub agent_id: Option<String>,
    pub sandbox_id: Option<String>,
    pub channel_id: Option<String>,
    pub status: Option<String>,
    pub limit: Option<u32>,
    pub labels: Vec<(String, String)>,
}

impl ConversationQuery {
    fn pairs(&self) -> Vec<(&'static str, String)> {
        let mut q = vec![];
        if self.roots_only {
            q.push(("roots_only", "true".to_owned()));
        }
        for (k, v) in [
            ("agent_id", &self.agent_id),
            ("sandbox_id", &self.sandbox_id),
            ("channel_id", &self.channel_id),
            ("status", &self.status),
        ] {
            if let Some(v) = v {
                q.push((k, v.clone()));
            }
        }
        if let Some(l) = self.limit {
            q.push(("limit", l.to_string()));
        }
        // Fountain splits each on its first colon.
        q.extend(self.labels.iter().map(|(k, v)| ("label", format!("{k}:{v}"))));
        q
    }
}

/// `POST /api/conversations`: an agent, and optionally a first prompt.
/// Fields left `None` aren't sent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NewConversation {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub labels: BTreeMap<String, String>,
    /// Narrowing only: Fountain refuses one that widens the agent's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_policy: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_config: Option<Value>,
}

/// What a create answers: the conversation, and whether a `channel_id`
/// resumed one instead of starting it (then the prompt wasn't delivered).
#[derive(Debug, Clone, PartialEq)]
pub struct Created {
    pub conversation: Conversation,
    pub resumed: bool,
}

/// `POST /prompts`' answer: `queued` (it must wake first), `accepted`, …
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Prompted {
    #[serde(default, deserialize_with = "null_default")]
    pub status: String,
    #[serde(default)]
    pub client_request_id: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One turn (`Turn`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub id: String,
    #[serde(default, deserialize_with = "null_default")]
    pub turn_number: u64,
    /// pending, running, completed, failed or interrupted.
    #[serde(default, deserialize_with = "null_default")]
    pub status: String,
    #[serde(default, deserialize_with = "null_default")]
    pub prompt: String,
    #[serde(default)]
    pub client_request_id: Option<String>,
    /// user or autonomous.
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One node of a conversation's tree (its fan-out).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TreeNode {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `POST /wake`'s answer: `awake` or `waking`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Woken {
    #[serde(default, deserialize_with = "null_default")]
    pub status: String,
}

/// A directory in a sandbox (`SandboxListing`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Listed {
    #[serde(default, deserialize_with = "null_default")]
    pub path: String,
    #[serde(default, deserialize_with = "null_default")]
    pub entries: Vec<Value>,
    #[serde(default, deserialize_with = "null_default")]
    pub truncated: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A file in a sandbox (`SandboxFile`): `encoding` utf-8 or base64.
/// Check `truncated` before showing it as the whole file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct File {
    #[serde(default, deserialize_with = "null_default")]
    pub path: String,
    #[serde(default, deserialize_with = "null_default")]
    pub content: String,
    #[serde(default, deserialize_with = "null_default")]
    pub encoding: String,
    #[serde(default, deserialize_with = "null_default")]
    pub size: u64,
    #[serde(default, deserialize_with = "null_default")]
    pub truncated: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A sandbox's diff (`SandboxDiff`): tracked content only.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Diff {
    #[serde(default, deserialize_with = "null_default")]
    pub diff: String,
    #[serde(default, deserialize_with = "null_default")]
    pub path: String,
    #[serde(default, deserialize_with = "null_default")]
    pub repo_root: String,
    #[serde(default, deserialize_with = "null_default")]
    pub staged: bool,
    #[serde(default)]
    pub r#ref: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub truncated: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `git status` in a sandbox (`SandboxStatus`): entry paths are relative to
/// the repository's root.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GitStatus {
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub entries: Vec<Value>,
    #[serde(default, deserialize_with = "null_default")]
    pub path: String,
    #[serde(default, deserialize_with = "null_default")]
    pub repo_root: String,
    #[serde(default, deserialize_with = "null_default")]
    pub truncated: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Options for a diff.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffQuery {
    pub path: Option<String>,
    pub staged: bool,
    pub r#ref: Option<String>,
    pub max_bytes: Option<u64>,
}

/// One document of a bulk apply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    /// Environment, Vault or Agent.
    pub kind: String,
    pub name: String,
    /// Absent: leave every field alone.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub spec: Option<Value>,
}

/// One row of an apply's answer: `created`, `updated`, `unchanged` or
/// `error` (one failed row doesn't undo the others).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Applied {
    #[serde(default, deserialize_with = "null_default")]
    pub kind: String,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub action: String,
    #[serde(default)]
    pub errors: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// An environment or a vault, by what Arugula reads of it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Named {
    pub id: String,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub secret_count: u64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A secret, by its key: Fountain never returns a value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SecretKey {
    pub id: String,
    pub key: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// Which provider's inference credential the account has set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InferenceCredential {
    pub provider: String,
    #[serde(default)]
    pub set: bool,
}

/// The account a key signs in as.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Me {
    pub id: String,
    #[serde(default)]
    pub email: Option<String>,
}

/// Where a secret lives: an environment's or a vault's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    Environment,
    Vault,
}

impl Holder {
    fn path(self) -> &'static str {
        match self {
            Holder::Environment => "/api/environments",
            Holder::Vault => "/api/vaults",
        }
    }
}

fn null_default<'de, D: serde::Deserializer<'de>, T: Default + Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}

// ------------------------------------------------------------- the client

/// The User-Agent every request carries: some deployments refuse default
/// ones (S24 saw Python's get a 403).
pub const USER_AGENT: &str = concat!("arugula/", env!("CARGO_PKG_VERSION"));

/// An HTTP client for Fountain: Arugula's roots, its User-Agent, no
/// redirects (a redirect would carry the key elsewhere), and `timeout` for
/// the whole request (none for a stream).
pub fn http(timeout: Option<Duration>) -> reqwest::Client {
    let mut b = crate::roots::http()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none());
    if let Some(t) = timeout {
        b = b.timeout(t);
    }
    b.build().expect("an HTTP client")
}

/// A client for one account: its base URL and key.
#[derive(Clone)]
pub struct Client {
    base: String,
    key: String,
    http: reqwest::Client,
    /// The longest `Retry-After` waited out in place.
    max_wait: Duration,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("base", &self.base).finish_non_exhaustive()
    }
}

impl Client {
    pub fn new(base: &str, key: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_owned(),
            key: key.to_owned(),
            http: http(Some(Duration::from_secs(30))),
            max_wait: MAX_WAIT,
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// Wait out at most `d` of a 429's `Retry-After` (tests).
    #[cfg(test)]
    pub fn max_wait(mut self, d: Duration) -> Self {
        self.max_wait = d;
        self
    }

    fn url(&self, path: &str, query: &[(&str, String)]) -> Result<url::Url, Error> {
        let mut u = url::Url::parse(&format!("{}{path}", self.base))
            .map_err(|e| Error::Unreachable(format!("{} isn't a URL: {e}", self.base)))?;
        if !query.is_empty() {
            let mut q = u.query_pairs_mut();
            for (k, v) in query {
                q.append_pair(k, v);
            }
        }
        Ok(u)
    }

    /// Send one request, waiting out 429s; the success's status and body.
    async fn send(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<(StatusCode, Vec<u8>), Error> {
        let url = self.url(path, query)?;
        let mut waited = 0;
        loop {
            let mut req = self
                .http
                .request(method.clone(), url.clone())
                .bearer_auth(&self.key)
                .header(reqwest::header::ACCEPT, "application/json");
            if let Some(b) = body {
                req = req.json(b);
            }
            let res = req.send().await.map_err(|e| Error::Unreachable(format!("{}: {}", self.base, plain(&e))))?;
            let status = res.status();
            let after = retry_after(res.headers());
            let bytes = res.bytes().await.map_err(|e| Error::Unreachable(format!("{path}: {}", plain(&e))))?;
            if status.is_success() {
                return Ok((status, bytes.to_vec()));
            }
            let err = error_for(status.as_u16(), &bytes, after);
            if let Error::RateLimited { retry_after } = err
                && waited < RETRIES_429
            {
                waited += 1;
                let wait = retry_after.min(self.max_wait);
                tracing::info!(path, wait_ms = wait.as_millis() as u64, "Fountain's rate limit: waiting");
                tokio::time::sleep(wait).await;
                continue;
            }
            return Err(err);
        }
    }

    async fn data<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<T, Error> {
        let (_, bytes) = self.send(method, path, query, body).await?;
        serde_json::from_slice::<Data<T>>(&bytes).map(|d| d.data).map_err(|e| Error::Unexpected(format!("{path}: {e}")))
    }

    async fn list<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<Listing<T>, Error> {
        Ok(rows(self.data::<Vec<Value>>(Method::GET, path, query, None).await?))
    }

    async fn unit(&self, method: Method, path: &str, body: Option<&Value>) -> Result<(), Error> {
        self.send(method, path, &[], body).await.map(|_| ())
    }

    // ------------------------------------------------------ conversations

    /// The account's conversations, newest activity first.
    pub async fn conversations(&self, q: &ConversationQuery) -> Result<Listing<Conversation>, Error> {
        self.list("/api/conversations", &q.pairs()).await
    }

    pub async fn conversation(&self, id: &str) -> Result<Conversation, Error> {
        self.data(Method::GET, &format!("/api/conversations/{}", seg(id)), &[], None).await
    }

    /// Start a conversation (or, with a `channel_id`, resume that channel's).
    pub async fn create(&self, new: &NewConversation) -> Result<Created, Error> {
        let body = serde_json::to_value(new).map_err(|e| Error::Unexpected(e.to_string()))?;
        let (_, bytes) = self.send(Method::POST, "/api/conversations", &[], Some(&body)).await?;
        #[derive(Deserialize)]
        struct Answer {
            data: Conversation,
            #[serde(default)]
            meta: Option<Value>,
        }
        let a: Answer = serde_json::from_slice(&bytes).map_err(|e| Error::Unexpected(format!("create: {e}")))?;
        let resumed = a.meta.as_ref().and_then(|m| m["resumed"].as_bool()).unwrap_or(false);
        Ok(Created { conversation: a.data, resumed })
    }

    /// Send a prompt. `client_request_id` is how the turn it opens is found
    /// (it isn't an idempotency key: don't send again on a lost answer,
    /// read the turns).
    pub async fn prompt(&self, id: &str, prompt: &str, client_request_id: Option<&str>) -> Result<Prompted, Error> {
        let mut body = json!({ "prompt": prompt });
        if let Some(c) = client_request_id {
            body["client_request_id"] = json!(c);
        }
        let path = format!("/api/conversations/{}/prompts", seg(id));
        let (_, bytes) = self.send(Method::POST, &path, &[], Some(&body)).await?;
        // `{status, client_request_id}`, at the top or under `data`.
        let v: Value = serde_json::from_slice(&bytes).map_err(|e| Error::Unexpected(format!("prompts: {e}")))?;
        let v = if v["data"].is_object() { v["data"].clone() } else { v };
        serde_json::from_value(v).map_err(|e| Error::Unexpected(format!("prompts: {e}")))
    }

    /// Stop the running turn (409 `no_turn_running` when there's none).
    pub async fn interrupt(&self, id: &str) -> Result<(), Error> {
        self.unit(Method::POST, &format!("/api/conversations/{}/interrupt", seg(id)), None).await
    }

    /// Wake a parked conversation's machine.
    pub async fn wake(&self, id: &str) -> Result<Woken, Error> {
        let path = format!("/api/conversations/{}/wake", seg(id));
        let (_, bytes) = self.send(Method::POST, &path, &[], None).await?;
        let v: Value = serde_json::from_slice(&bytes).map_err(|e| Error::Unexpected(format!("wake: {e}")))?;
        let v = if v["data"].is_object() { v["data"].clone() } else { v };
        serde_json::from_value(v).map_err(|e| Error::Unexpected(format!("wake: {e}")))
    }

    /// End it for good: its sandbox, and the disk with it.
    pub async fn terminate(&self, id: &str) -> Result<(), Error> {
        self.unit(Method::POST, &format!("/api/conversations/{}/terminate", seg(id)), None).await
    }

    pub async fn turns(&self, id: &str) -> Result<Listing<Turn>, Error> {
        self.list(&format!("/api/conversations/{}/turns", seg(id)), &[]).await
    }

    /// The conversation and every conversation it started, and theirs.
    pub async fn tree(&self, id: &str) -> Result<Listing<TreeNode>, Error> {
        self.list(&format!("/api/conversations/{}/tree", seg(id)), &[]).await
    }

    /// Answer a permission request with one of its options. The first
    /// answer wins; a later one is 409 or 404.
    pub async fn answer(&self, id: &str, request_id: &str, option_id: &str) -> Result<(), Error> {
        let path = format!("/api/conversations/{}/requests/{}", seg(id), seg(request_id));
        self.unit(Method::POST, &path, Some(&json!({ "option_id": option_id }))).await
    }

    /// Mark it read: what clears its unread dot, on every client.
    pub async fn mark_read(&self, id: &str) -> Result<(), Error> {
        self.unit(Method::POST, &format!("/api/conversations/{}/read", seg(id)), None).await
    }

    /// Merge labels into the conversation's; a `None` removes that key.
    pub async fn set_labels(&self, id: &str, labels: &BTreeMap<String, Option<String>>) -> Result<Conversation, Error> {
        let path = format!("/api/conversations/{}/labels", seg(id));
        self.data(Method::PATCH, &path, &[], Some(&json!({ "labels": labels }))).await
    }

    // ---------------------------------------------------------- sandboxes

    /// A directory (the sandbox's home when `path` is `None`). A parked
    /// sandbox is 409 `sandbox_not_ready`: reads never wake it.
    pub async fn files(&self, sandbox: &str, path: Option<&str>) -> Result<Listed, Error> {
        let q: Vec<_> = path.map(|p| ("path", p.to_owned())).into_iter().collect();
        self.data(Method::GET, &format!("/api/sandboxes/{}/files", seg(sandbox)), &q, None).await
    }

    pub async fn file(&self, sandbox: &str, path: &str, max_bytes: Option<u64>) -> Result<File, Error> {
        let mut q = vec![("path", path.to_owned())];
        if let Some(m) = max_bytes {
            q.push(("max_bytes", m.to_string()));
        }
        self.data(Method::GET, &format!("/api/sandboxes/{}/file", seg(sandbox)), &q, None).await
    }

    pub async fn diff(&self, sandbox: &str, d: &DiffQuery) -> Result<Diff, Error> {
        let mut q = vec![];
        if let Some(p) = &d.path {
            q.push(("path", p.clone()));
        }
        if d.staged {
            q.push(("staged", "true".to_owned()));
        }
        if let Some(r) = &d.r#ref {
            q.push(("ref", r.clone()));
        }
        if let Some(m) = d.max_bytes {
            q.push(("max_bytes", m.to_string()));
        }
        self.data(Method::GET, &format!("/api/sandboxes/{}/diff", seg(sandbox)), &q, None).await
    }

    /// `untracked`: normal, all or no.
    pub async fn git_status(
        &self,
        sandbox: &str,
        path: Option<&str>,
        untracked: Option<&str>,
    ) -> Result<GitStatus, Error> {
        let mut q = vec![];
        if let Some(p) = path {
            q.push(("path", p.to_owned()));
        }
        if let Some(u) = untracked {
            q.push(("untracked", u.to_owned()));
        }
        self.data(Method::GET, &format!("/api/sandboxes/{}/git-status", seg(sandbox)), &q, None).await
    }

    // ------------------------------------------------------------- agents

    pub async fn agents(&self) -> Result<Listing<Agent>, Error> {
        self.list("/api/agents", &[]).await
    }

    pub async fn agent(&self, id: &str) -> Result<Agent, Error> {
        self.data(Method::GET, &format!("/api/agents/{}", seg(id)), &[], None).await
    }

    /// Bulk apply (a full-scope key): each row's outcome.
    pub async fn apply(&self, resources: &[Resource]) -> Result<Vec<Applied>, Error> {
        #[derive(Deserialize)]
        struct Results {
            results: Vec<Applied>,
        }
        let r: Results = self.data(Method::POST, "/api/apply", &[], Some(&json!({ "resources": resources }))).await?;
        Ok(r.results)
    }

    // ---------------------------------------------- environments and vaults

    pub async fn environments(&self) -> Result<Listing<Named>, Error> {
        self.list("/api/environments", &[]).await
    }

    pub async fn vaults(&self) -> Result<Listing<Named>, Error> {
        self.list("/api/vaults", &[]).await
    }

    /// The keys of an environment's or a vault's secrets (never values).
    pub async fn secret_keys(&self, holder: Holder, id: &str) -> Result<Listing<SecretKey>, Error> {
        self.list(&format!("{}/{}/secrets", holder.path(), seg(id)), &[]).await
    }

    /// Set one secret (Fountain upserts by key). The value goes in the body
    /// and nowhere else.
    pub async fn set_secret(&self, holder: Holder, id: &str, key: &str, value: &str) -> Result<SecretKey, Error> {
        let path = format!("{}/{}/secrets", holder.path(), seg(id));
        self.data(Method::POST, &path, &[], Some(&json!({ "key": key, "value": value }))).await
    }

    // ------------------------------------------------------------ account

    /// Who the key signs in as (`GET /api/auth/me`, a flat object).
    pub async fn me(&self) -> Result<Me, Error> {
        let (_, bytes) = self.send(Method::GET, "/api/auth/me", &[], None).await?;
        let v: Value = serde_json::from_slice(&bytes).map_err(|e| Error::Unexpected(format!("me: {e}")))?;
        let v = if v["data"].is_object() { v["data"].clone() } else { v };
        serde_json::from_value(v).map_err(|e| Error::Unexpected(format!("me: {e}")))
    }

    /// Revoke an API key by its id (the key this client holds, at sign-out).
    pub async fn revoke_key(&self, key_id: &str) -> Result<(), Error> {
        self.unit(Method::DELETE, &format!("/api/auth/api-keys/{}", seg(key_id)), None).await
    }

    /// Which inference credentials the account has set (never values).
    pub async fn inference_credentials(&self) -> Result<Vec<InferenceCredential>, Error> {
        Ok(self.list("/api/account/inference-credentials", &[]).await?.items)
    }
}

/// A reqwest error without the URL (which is fine) or anything else long.
fn plain(e: &reqwest::Error) -> String {
    let mut s = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(c) = src {
        s = format!("{s}: {c}");
        src = c.source();
    }
    s
}

// ------------------------------------------------------- the device flow

/// `POST /api/auth/device`'s answer: show `user_code` and send the person to
/// `verification_uri` (or `verification_uri_complete`), then poll with
/// `device_code` every `interval` seconds, for `expires_in` seconds.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceGrant {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    #[serde(default = "five")]
    pub expires_in: u64,
    #[serde(default = "five")]
    pub interval: u64,
}

fn five() -> u64 {
    5
}

impl std::fmt::Debug for DeviceGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The device code is as good as the key, until it's used.
        f.debug_struct("DeviceGrant")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .finish_non_exhaustive()
    }
}

/// A key the device flow minted: full scope. `prefix` and `key_id` are what
/// Fountain's key list shows, safe to show here.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MintedKey {
    pub api_key: String,
    #[serde(default)]
    pub key_id: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
}

impl std::fmt::Debug for MintedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MintedKey").field("key_id", &self.key_id).field("prefix", &self.prefix).finish_non_exhaustive()
    }
}

/// One poll's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    Pending,
    /// Polled too fast: add five seconds to the interval (RFC 8628).
    SlowDown,
    Approved(MintedKey),
    Denied,
    Expired,
    /// Unknown or already used.
    Invalid,
}

/// Start a device grant on `base`. `client_name` names the key Fountain
/// mints ("Arugula on geek"), once Fountain reads it; until then it's
/// ignored.
pub async fn device_start(http: &reqwest::Client, base: &str, client_name: &str) -> Result<DeviceGrant, Error> {
    let url = format!("{}/api/auth/device", base.trim_end_matches('/'));
    let res = http
        .post(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&json!({ "client_name": client_name }))
        .send()
        .await
        .map_err(|e| Error::Unreachable(format!("{base}: {}", plain(&e))))?;
    let status = res.status();
    let after = retry_after(res.headers());
    let bytes = res.bytes().await.map_err(|e| Error::Unreachable(plain(&e)))?;
    if !status.is_success() {
        return Err(error_for(status.as_u16(), &bytes, after));
    }
    serde_json::from_slice(&bytes).map_err(|e| {
        Error::Unexpected(format!("{base} isn't a Fountain (its /api/auth/device answered something else: {e})"))
    })
}

/// Poll a device grant once.
pub async fn device_poll(http: &reqwest::Client, base: &str, device_code: &str) -> Result<Poll, Error> {
    let url = format!("{}/api/auth/device/token", base.trim_end_matches('/'));
    let res = http
        .post(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&json!({ "device_code": device_code }))
        .send()
        .await
        .map_err(|e| Error::Unreachable(format!("{base}: {}", plain(&e))))?;
    let status = res.status();
    let after = retry_after(res.headers());
    let bytes = res.bytes().await.map_err(|e| Error::Unreachable(plain(&e)))?;
    if status.is_success() {
        return serde_json::from_slice(&bytes)
            .map(Poll::Approved)
            .map_err(|e| Error::Unexpected(format!("device token: {e}")));
    }
    if status.as_u16() == 400 {
        let code = serde_json::from_slice::<Value>(&bytes).ok().and_then(|v| v["error"].as_str().map(str::to_owned));
        return Ok(match code.as_deref() {
            Some("authorization_pending") => Poll::Pending,
            Some("slow_down") => Poll::SlowDown,
            Some("access_denied") => Poll::Denied,
            Some("expired_token") => Poll::Expired,
            _ => Poll::Invalid,
        });
    }
    Err(error_for(status.as_u16(), &bytes, after))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use axum::{
        Json, Router,
        extract::State,
        http::{HeaderMap, StatusCode as S},
        response::{IntoResponse, Response},
        routing::post,
    };

    use super::*;

    type Seen = (String, String, String, Value);

    /// A stub Fountain: answers what `answers` says, in order, and records
    /// each request (method, path and query, auth, body).
    #[derive(Clone, Default)]
    struct Stub {
        /// Method, path and query, Authorization, body.
        seen: Arc<Mutex<Vec<Seen>>>,
        answers: Arc<Mutex<VecDeque<Response>>>,
    }

    async fn serve(stub: Stub) -> String {
        async fn any(
            State(s): State<Stub>,
            method: axum::http::Method,
            uri: axum::http::Uri,
            h: HeaderMap,
            body: axum::body::Bytes,
        ) -> Response {
            let auth = h.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
            let ua = h.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("");
            assert!(ua.starts_with("arugula/"), "{ua}");
            let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
            s.seen.lock().unwrap().push((method.to_string(), uri.to_string(), auth, body));
            s.answers.lock().unwrap().pop_front().unwrap_or_else(|| S::IM_A_TEAPOT.into_response())
        }
        let app = Router::new().fallback(any).with_state(stub);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    }

    fn answer(s: u16, v: Value) -> Response {
        (S::from_u16(s).unwrap(), Json(v)).into_response()
    }

    #[tokio::test]
    async fn requests_carry_the_key_and_read_fountains_shapes() {
        let stub = Stub::default();
        let base = serve(stub.clone()).await;
        let c = Client::new(&format!("{base}/"), "ftn_key");
        stub.answers.lock().unwrap().extend([
            answer(
                200,
                json!({ "data": [
                    { "id": "c1", "status": "idle", "labels": { "env": "prod" }, "runtime": "claude", "unread": true, "new_field": 7 },
                    { "garbage": true },
                ]}),
            ),
            answer(201, json!({ "data": { "id": "c2", "status": "pending", "runtime": "claude" }, "meta": { "resumed": false } })),
            answer(200, json!({ "status": "queued", "client_request_id": "r-1" })),
            S::NO_CONTENT.into_response(),
            answer(200, json!({ "data": { "status": "waking" } })),
            answer(200, json!({ "ok": true })),
            answer(200, json!({ "data": { "results": [{ "kind": "Agent", "name": "recipe-x", "action": "unchanged" }] } })),
            answer(201, json!({ "data": { "id": "s1", "key": "TOKEN", "vault_id": "v1" } })),
            answer(200, json!({ "data": [{ "provider": "anthropic_api_key", "set": true }] })),
            answer(200, json!({ "data": { "diff": "@@", "path": ".", "repo_root": "/home/sprite/r", "staged": false, "truncated": false } })),
        ]);
        let q = ConversationQuery {
            roots_only: true,
            labels: vec![("env".into(), "prod".into()), ("path".into(), "a:b".into())],
            ..Default::default()
        };
        let l = c.conversations(&q).await.unwrap();
        assert_eq!((l.items.len(), l.unreadable), (1, 1));
        assert_eq!(l.items[0].labels["env"], "prod");
        assert!(l.items[0].unread);
        assert_eq!(l.items[0].extra["new_field"], 7, "passed through");
        let made = c
            .create(&NewConversation {
                agent_id: "a1".into(),
                prompt: Some("hi".into()),
                labels: [("arugula.recipe".to_owned(), "x".to_owned())].into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!((made.conversation.id.as_str(), made.resumed), ("c2", false));
        assert_eq!(c.prompt("c2", "more", Some("r-1")).await.unwrap().status, "queued");
        c.interrupt("c2").await.unwrap();
        assert_eq!(c.wake("c2").await.unwrap().status, "waking");
        c.answer("c2", "req/1", "allow").await.unwrap();
        let applied = c
            .apply(&[Resource {
                kind: "Agent".into(),
                name: "recipe-x".into(),
                spec: Some(json!({ "runtime": "claude" })),
            }])
            .await
            .unwrap();
        assert_eq!(applied[0].action, "unchanged");
        assert_eq!(c.set_secret(Holder::Vault, "v1", "TOKEN", "s3cret").await.unwrap().key, "TOKEN");
        assert!(c.inference_credentials().await.unwrap()[0].set);
        let d = DiffQuery { staged: false, path: Some("src".into()), ..Default::default() };
        assert_eq!(c.diff("sb 1", &d).await.unwrap().diff, "@@");

        let seen = stub.seen.lock().unwrap().clone();
        assert!(seen.iter().all(|(_, _, auth, _)| auth == "Bearer ftn_key"));
        let lines: Vec<String> = seen.iter().map(|(m, u, _, _)| format!("{m} {u}")).collect();
        assert_eq!(
            lines,
            [
                "GET /api/conversations?roots_only=true&label=env%3Aprod&label=path%3Aa%3Ab",
                "POST /api/conversations",
                "POST /api/conversations/c2/prompts",
                "POST /api/conversations/c2/interrupt",
                "POST /api/conversations/c2/wake",
                "POST /api/conversations/c2/requests/req%2F1",
                "POST /api/apply",
                "POST /api/vaults/v1/secrets",
                "GET /api/account/inference-credentials",
                "GET /api/sandboxes/sb%201/diff?path=src",
            ]
        );
        assert_eq!(seen[1].3, json!({ "agent_id": "a1", "prompt": "hi", "labels": { "arugula.recipe": "x" } }));
        assert_eq!(seen[2].3, json!({ "prompt": "more", "client_request_id": "r-1" }));
        assert_eq!(seen[5].3, json!({ "option_id": "allow" }));
        assert_eq!(seen[6].3["resources"][0]["kind"], "Agent");
        assert_eq!(seen[7].3, json!({ "key": "TOKEN", "value": "s3cret" }));
        // The key is in no Debug.
        assert!(!format!("{c:?}").contains("ftn_key"));
    }

    #[tokio::test]
    async fn errors_are_fountains_codes() {
        let stub = Stub::default();
        let base = serve(stub.clone()).await;
        let c = Client::new(&base, "k");
        stub.answers.lock().unwrap().extend([
            answer(401, json!({ "error": "Invalid or expired API key", "reason": "invalid_api_key" })),
            answer(
                403,
                json!({ "error": "This key may not", "reason": "insufficient_scope", "required_scope": "full" }),
            ),
            answer(409, json!({ "error": "sandbox_not_ready", "message": "The sandbox is parked" })),
            answer(409, json!({ "error": "conversation_busy" })),
            answer(422, json!({ "error": "validation_failed", "errors": { "labels": ["too many"] } })),
            (S::SERVICE_UNAVAILABLE, [("retry-after", "60")], Json(json!({ "error": "fleet_full" }))).into_response(),
            answer(404, json!({ "error": "not_found" })),
            answer(500, json!({ "error": "server_error" })),
            answer(200, json!({ "unexpected": true })),
        ]);
        let e = c.conversation("x").await.unwrap_err();
        assert!(e.key_refused() && e.code() == "invalid_api_key", "{e:?}");
        assert!(e.to_string().contains("Invalid or expired API key"), "{e}");
        let e = c.inference_credentials().await.unwrap_err();
        assert_eq!((e.status(), e.code()), (Some(403), "insufficient_scope"));
        let e = c.files("s", None).await.unwrap_err();
        assert_eq!(e, Error::Conflict { code: "sandbox_not_ready".into(), message: "The sandbox is parked".into() });
        let e = c.prompt("c", "p", None).await.unwrap_err();
        assert_eq!(e.code(), "conversation_busy");
        assert!(!e.transient());
        let e = c.set_labels("c", &BTreeMap::new()).await.unwrap_err();
        let Error::Invalid { status: 422, fields: Some(f), .. } = &e else { panic!("{e:?}") };
        assert_eq!(f["labels"][0], "too many");
        let e = c.create(&NewConversation { agent_id: "a".into(), ..Default::default() }).await.unwrap_err();
        assert_eq!(e.retry_after(), Some(Duration::from_secs(60)));
        assert!(e.transient() && e.code() == "fleet_full");
        assert_eq!(c.agent("a").await.unwrap_err().status(), Some(404));
        assert!(c.turns("c").await.unwrap_err().transient());
        assert!(matches!(c.conversation("c").await.unwrap_err(), Error::Unexpected(_)));
        // Nothing answers there.
        let gone = Client::new("http://127.0.0.1:9", "k");
        assert!(matches!(gone.agents().await.unwrap_err(), Error::Unreachable(_)));
    }

    #[tokio::test]
    async fn a_429_is_waited_out_then_named() {
        let stub = Stub::default();
        let base = serve(stub.clone()).await;
        let c = Client::new(&base, "k").max_wait(Duration::from_millis(50));
        let limited = || (S::TOO_MANY_REQUESTS, [("retry-after", "1")], Json(json!({ "error": "rate_limited" })));
        stub.answers
            .lock()
            .unwrap()
            .extend([limited().into_response(), answer(200, json!({ "data": [{ "id": "t1", "turn_number": 1 }] }))]);
        let started = std::time::Instant::now();
        assert_eq!(c.turns("c").await.unwrap().items[0].id, "t1");
        assert!(started.elapsed() >= Duration::from_millis(50));
        assert_eq!(stub.seen.lock().unwrap().len(), 2);
        // A mutation too: a 429 is refused before anything runs.
        stub.answers.lock().unwrap().extend((0..=RETRIES_429).map(|_| limited().into_response()));
        let e = c.terminate("c").await.unwrap_err();
        assert_eq!(e, Error::RateLimited { retry_after: Duration::from_secs(1) });
        assert_eq!(stub.seen.lock().unwrap().len(), 2 + 1 + RETRIES_429 as usize);
    }

    #[tokio::test]
    async fn the_device_flow() {
        #[derive(Clone, Default)]
        struct D(Arc<Mutex<(u32, Vec<Value>)>>);
        async fn start(State(d): State<D>, Json(b): Json<Value>) -> Response {
            d.0.lock().unwrap().1.push(b);
            answer(
                201,
                json!({ "device_code": "dc-1", "user_code": "ABCD-EFGH", "verification_uri": "http://f/device",
                        "verification_uri_complete": "http://f/device?code=ABCD-EFGH", "expires_in": 600, "interval": 5 }),
            )
        }
        async fn token(State(d): State<D>, Json(b): Json<Value>) -> Response {
            let mut g = d.0.lock().unwrap();
            if b["device_code"] != "dc-1" {
                return answer(400, json!({ "error": "invalid_grant" }));
            }
            g.0 += 1;
            match g.0 {
                1 => answer(400, json!({ "error": "authorization_pending" })),
                2 => answer(400, json!({ "error": "slow_down" })),
                3 => answer(201, json!({ "api_key": "ftn_minted", "key_id": "k1", "prefix": "ftn_mi" })),
                _ => answer(400, json!({ "error": "invalid_grant" })),
            }
        }
        let d = D::default();
        let app = Router::new()
            .route("/api/auth/device", post(start))
            .route("/api/auth/device/token", post(token))
            .with_state(d.clone());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        let http = http(Some(Duration::from_secs(5)));
        let g = device_start(&http, &base, "Arugula on test").await.unwrap();
        assert_eq!((g.user_code.as_str(), g.interval, g.expires_in), ("ABCD-EFGH", 5, 600));
        assert!(!format!("{g:?}").contains("dc-1"));
        assert_eq!(d.0.lock().unwrap().1[0]["client_name"], "Arugula on test");
        assert_eq!(device_poll(&http, &base, "dc-1").await.unwrap(), Poll::Pending);
        assert_eq!(device_poll(&http, &base, "dc-1").await.unwrap(), Poll::SlowDown);
        let Poll::Approved(k) = device_poll(&http, &base, "dc-1").await.unwrap() else { panic!() };
        assert_eq!((k.api_key.as_str(), k.prefix.as_deref()), ("ftn_minted", Some("ftn_mi")));
        assert!(!format!("{k:?}").contains("ftn_minted"));
        assert_eq!(device_poll(&http, &base, "dc-1").await.unwrap(), Poll::Invalid);
        // Not a Fountain.
        let e = device_start(&http, &format!("{base}/x"), "n").await.unwrap_err();
        assert!(e.status().is_some() || matches!(e, Error::Unexpected(_)), "{e:?}");
    }
}
