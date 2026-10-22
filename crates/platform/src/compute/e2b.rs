//! The E2B adapter: E2B's REST API as its SDKs (2.58) call it.
//!
//! | Call | Route |
//! |---|---|
//! | create | `POST /v2/sandboxes` (`autoPause`, memory kept, auto-resume off) |
//! | resume | `POST /v2/sandboxes/{id}/connect` (201 woke, 200 was running) |
//! | pause | `POST /sandboxes/{id}/pause` (409: paused already) |
//! | delete | `DELETE /sandboxes/{id}` (404: gone already) |
//! | status | `GET /sandboxes/{id}` |
//! | list | `GET /v2/sandboxes?metadata=…&state=running,paused`, paged by `X-Next-Token` |
//! | hold | `POST /sandboxes/{id}/timeout` |
//!
//! Every call but create is retried a few times on no answer, 429, 502,
//! 503 and 504: each is idempotent here (a second pause finds it paused, a
//! second resume finds it running). A create is never resent: a lost
//! answer may have made a box, which the caller finds by its metadata.
//!
//! What E2B answers is read into the fields below and nothing else: a
//! create's or connect's `envdAccessToken` and `trafficAccessToken` are
//! credentials for the box's envd and ports, which the platform doesn't
//! use, so they're never kept.

use std::{collections::BTreeMap, time::Duration};

use futures_util::future::BoxFuture;
use reqwest::{Client, Method, Response, StatusCode, Url};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use super::{Create, Created, Error, Paused, Result, Resumed, Sandbox, State, Substrate};

/// Tries of a retried call, and the longest `Retry-After` waited out.
const TRIES: u32 = 3;
const MAX_RETRY_AFTER: u64 = 5;

pub struct E2b {
    client: Client,
    base: Url,
    key: String,
}

impl std::fmt::Debug for E2b {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("E2b").field("base", &self.base.as_str()).finish_non_exhaustive()
    }
}

impl E2b {
    /// The adapter for the API at `base` (`https://api.e2b.dev`, or loam's
    /// `https://api.e2b.<loam domain>`), with `key` as its `X-API-Key`.
    pub fn new(base: &str, key: String) -> anyhow::Result<Self> {
        let base = Url::parse(base)?;
        if !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            anyhow::bail!("invalid compute provider URL");
        }
        let local = matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if base.scheme() != "https" && !(base.scheme() == "http" && local) {
            anyhow::bail!("the compute provider requires HTTPS");
        }
        if key.trim().is_empty() {
            anyhow::bail!("the compute provider's key is empty");
        }
        Ok(Self {
            // A create waits for the box's envd; a pause writes its memory.
            client: Client::builder()
                .timeout(Duration::from_secs(90))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(concat!("arugula/", env!("CARGO_PKG_VERSION")))
                .build()?,
            base,
            key,
        })
    }

    fn url(&self, segments: &[&str]) -> Result<Url> {
        let mut url = self.base.clone();
        url.path_segments_mut().map_err(|_| Error::Invalid)?.pop_if_empty().extend(segments);
        Ok(url)
    }

    /// One request, its answer whatever its status, or `Unreachable`.
    async fn send(&self, method: Method, url: Url, body: Option<&Value>) -> Result<Response> {
        let mut request = self.client.request(method, url).header("X-API-Key", &self.key);
        if let Some(body) = body {
            request = request.json(body);
        }
        request.send().await.map_err(|_| Error::Unreachable)
    }

    /// A request retried while there's no answer or the provider says
    /// later (see the top): only for idempotent calls.
    async fn idempotent(&self, method: Method, url: Url, body: Option<&Value>) -> Result<Response> {
        let mut tried = 0;
        loop {
            tried += 1;
            let wait = match self.send(method.clone(), url.clone(), body).await {
                Ok(r) if !matches!(r.status().as_u16(), 429 | 502 | 503 | 504) => return Ok(r),
                Ok(r) if tried < TRIES => retry_after(&r).filter(|s| *s <= MAX_RETRY_AFTER),
                Err(Error::Unreachable) if tried < TRIES => None,
                other => return other,
            };
            tokio::time::sleep(wait.map_or(Duration::from_millis(250 * u64::from(tried)), Duration::from_secs)).await;
        }
    }
}

fn retry_after(r: &Response) -> Option<u64> {
    r.headers().get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim().parse().ok()
}

/// What a refusal keeps: its status, E2B's `error_code` when it's a plain
/// word, and `Retry-After`. Its `message` and the rest are never read out.
async fn failure(r: Response) -> Error {
    let status = r.status().as_u16();
    let retry_after = retry_after(&r);
    let body: Value = r.json().await.unwrap_or(Value::Null);
    let code = body["error_code"]
        .as_str()
        .filter(|c| !c.is_empty() && c.len() <= 64 && c.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        .map(str::to_owned);
    match status {
        404 => Error::NotFound,
        408 | 429 | 500..=599 => Error::Unavailable { status, code, retry_after },
        400..=499 => Error::Refused { status, code },
        _ => Error::Invalid,
    }
}

async fn read<T: DeserializeOwned>(r: Response) -> Result<T> {
    r.json().await.map_err(|_| Error::Invalid)
}

/// A successful answer with no body, else its failure.
async fn done(r: Response) -> Result<()> {
    if r.status().is_success() { Ok(()) } else { Err(failure(r).await) }
}

fn seconds(d: Duration) -> u64 {
    d.as_secs().max(1)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewBox {
    #[serde(rename = "sandboxID")]
    sandbox_id: String,
    #[serde(rename = "templateID")]
    template_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Detail {
    #[serde(rename = "sandboxID")]
    sandbox_id: String,
    #[serde(rename = "templateID")]
    template_id: String,
    state: String,
    #[serde(default)]
    metadata: Option<BTreeMap<String, String>>,
    started_at: String,
    end_at: String,
    #[serde(default)]
    lifecycle: Option<Lifecycle>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lifecycle {
    on_timeout: String,
}

impl TryFrom<Detail> for Sandbox {
    type Error = Error;
    fn try_from(d: Detail) -> Result<Sandbox> {
        let state = match d.state.as_str() {
            "running" => State::Running,
            "paused" => State::Paused,
            _ => return Err(Error::Invalid),
        };
        Ok(Sandbox {
            id: d.sandbox_id,
            template: d.template_id,
            state,
            metadata: d.metadata.unwrap_or_default(),
            started_at: d.started_at,
            end_at: d.end_at,
            pauses_on_timeout: d.lifecycle.map(|l| l.on_timeout == "pause"),
        })
    }
}

/// E2B's `metadata` filter, as its SDKs write it: each key and value
/// URL-encoded, then joined as a query string (which the request's own
/// query encodes again).
fn metadata_query(metadata: &BTreeMap<String, String>) -> String {
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in metadata {
        q.append_pair(&enc(k), &enc(v));
    }
    q.finish()
}

impl Substrate for E2b {
    fn create<'a>(&'a self, b: &'a Create) -> BoxFuture<'a, Result<Created>> {
        Box::pin(async move {
            let body = json!({
                "templateID": b.template,
                "timeout": seconds(b.timeout),
                "autoPause": true,
                "autoPauseMemory": true,
                "autoResume": {"enabled": false},
                "metadata": b.metadata,
                "envVars": b.env,
            });
            let r = self.send(Method::POST, self.url(&["v2", "sandboxes"])?, Some(&body)).await?;
            if !r.status().is_success() {
                // An unknown template is a 404, which isn't "no such box".
                return Err(match failure(r).await {
                    Error::NotFound => Error::Refused { status: 404, code: None },
                    e => e,
                });
            }
            let made: NewBox = read(r).await?;
            Ok(Created { id: made.sandbox_id, template: made.template_id })
        })
    }

    fn resume<'a>(&'a self, id: &'a str, timeout: Duration) -> BoxFuture<'a, Result<Resumed>> {
        Box::pin(async move {
            let body = json!({"timeout": seconds(timeout)});
            let r = self.idempotent(Method::POST, self.url(&["v2", "sandboxes", id, "connect"])?, Some(&body)).await?;
            match r.status() {
                StatusCode::CREATED => Ok(Resumed::Woke),
                StatusCode::OK => Ok(Resumed::Running),
                _ => Err(failure(r).await),
            }
        })
    }

    fn pause<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Paused>> {
        Box::pin(async move {
            let body = json!({});
            let r = self.idempotent(Method::POST, self.url(&["sandboxes", id, "pause"])?, Some(&body)).await?;
            match r.status() {
                s if s.is_success() => Ok(Paused::Paused),
                StatusCode::CONFLICT => Ok(Paused::Already),
                _ => Err(failure(r).await),
            }
        })
    }

    fn delete<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let r = self.idempotent(Method::DELETE, self.url(&["sandboxes", id])?, None).await?;
            match done(r).await {
                Err(Error::NotFound) => Ok(()),
                other => other,
            }
        })
    }

    fn status<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<Sandbox>>> {
        Box::pin(async move {
            let r = self.idempotent(Method::GET, self.url(&["sandboxes", id])?, None).await?;
            if r.status() == StatusCode::NOT_FOUND {
                return Ok(None);
            }
            if !r.status().is_success() {
                return Err(failure(r).await);
            }
            read::<Detail>(r).await?.try_into().map(Some)
        })
    }

    fn list<'a>(&'a self, metadata: &'a BTreeMap<String, String>) -> BoxFuture<'a, Result<Vec<Sandbox>>> {
        Box::pin(async move {
            let filter = metadata_query(metadata);
            let mut next: Option<String> = None;
            let mut all = Vec::new();
            // A page is at most 100; this many pages is a provider looping.
            for _ in 0..1000 {
                let mut url = self.url(&["v2", "sandboxes"])?;
                {
                    let mut q = url.query_pairs_mut();
                    if !filter.is_empty() {
                        q.append_pair("metadata", &filter);
                    }
                    q.append_pair("state", "running,paused").append_pair("limit", "100");
                    if let Some(token) = &next {
                        q.append_pair("nextToken", token);
                    }
                }
                let r = self.idempotent(Method::GET, url, None).await?;
                if !r.status().is_success() {
                    return Err(failure(r).await);
                }
                next = r
                    .headers()
                    .get("x-next-token")
                    .and_then(|v| v.to_str().ok())
                    .filter(|t| !t.is_empty())
                    .map(str::to_owned);
                let page: Vec<Detail> = read(r).await?;
                for d in page {
                    all.push(Sandbox::try_from(d)?);
                }
                if next.is_none() {
                    // The filter is the provider's to apply; checked here
                    // too, so a provider that ignored it lists nothing extra.
                    all.retain(|s: &Sandbox| metadata.iter().all(|(k, v)| s.metadata.get(k) == Some(v)));
                    return Ok(all);
                }
            }
            Err(Error::Invalid)
        })
    }

    fn hold<'a>(&'a self, id: &'a str, timeout: Duration) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let body = json!({"timeout": seconds(timeout)});
            let r = self.idempotent(Method::POST, self.url(&["sandboxes", id, "timeout"])?, Some(&body)).await?;
            done(r).await
        })
    }
}
