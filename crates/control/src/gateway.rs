//! The hub as the platform's gateway (M140, "Platform paths stay on
//! control.arugula.io forever, behind the hub's gateway"). The platform
//! runs in this process ([`PlatformAt::InProcess`], the default, and the
//! rollback) or as its own (`arugula-platform`, [`PlatformAt::Remote`]),
//! and the platform's routes stay on control's own site either way.
//!
//! Out of process, for every route `routes.toml` gives
//! `process = "platform"` (read from the file, so the list is never
//! written twice), the hub:
//! 1. answers 503 while the maintenance switch is on, before anything else
//!    ([`maintenance`](crate::maintenance));
//! 2. checks who's asking exactly as in process (`hub_port::who`): the
//!    session with its same-origin check, or the signature already checked
//!    by `auth::verify_daemon`, replay cache and all;
//! 3. drops what the platform mustn't see or mustn't trust from a client:
//!    the cookie, the signature, any principal or internal header;
//! 4. forwards the call with what it found signed into the principal
//!    header (`arugula_control_common::principal`), the method, path,
//!    query and body untouched (a daemon's signature covers the body), the
//!    Host header too (billing's checkout goes back to the site it came
//!    from);
//! 5. passes the answer back as it comes, status, headers (`Retry-After`
//!    among them) and body, streamed: chat's live stream (SSE) goes through
//!    unbuffered, `Last-Event-ID` and all.
//!
//! A platform that can't be reached at all (a deploy restarting it) is
//! tried again for a few seconds, then answered as a maintenance 503: the
//! call never reached it, so any client may send it again.

use std::{sync::Arc, time::Duration};

use arugula_control_common::{
    ApiError, internal,
    principal::{self, NotVouched, PrincipalKey, Vouch},
};
use arugula_platform::{Deleted, Platform, PlatformPort, auth::Asked, remote::HttpPlatform};
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderName, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{MethodFilter, MethodRouter},
};
use futures_util::{TryStreamExt as _, future::BoxFuture};
use serde::Deserialize;
use tracing::warn;

use crate::App;

/// Where the platform runs.
pub enum PlatformAt {
    /// In this process: its router is mounted here, behind `hub_port::gate`.
    InProcess(Arc<Platform>),
    /// Its own process on the private network, which this hub forwards to.
    Remote(Remote),
}

impl PlatformAt {
    /// The platform, when it's in this process.
    pub fn local(&self) -> Option<&Arc<Platform>> {
        match self {
            PlatformAt::InProcess(p) => Some(p),
            PlatformAt::Remote(_) => None,
        }
    }

    fn port(&self) -> &dyn PlatformPort {
        match self {
            PlatformAt::InProcess(p) => &**p,
            PlatformAt::Remote(r) => &r.port,
        }
    }
}

#[cfg(test)]
impl PlatformAt {
    /// The platform in this process, for a test that reaches into it.
    pub fn inner(&self) -> &Arc<Platform> {
        self.local().expect("the platform is in this process")
    }

    /// The same, to change before the app starts.
    pub fn inner_mut(&mut self) -> &mut Platform {
        match self {
            PlatformAt::InProcess(p) => Arc::get_mut(p).expect("not started yet"),
            PlatformAt::Remote(_) => panic!("the platform is in this process"),
        }
    }
}

/// The hub's calls into the platform go where it runs.
impl PlatformPort for PlatformAt {
    fn delete_blockers<'a>(&'a self, account: &'a str, start: bool) -> BoxFuture<'a, anyhow::Result<Vec<String>>> {
        self.port().delete_blockers(account, start)
    }

    fn account_deleted<'a>(&'a self, deleted: &'a Deleted) -> BoxFuture<'a, anyhow::Result<()>> {
        self.port().account_deleted(deleted)
    }

    fn active_plans<'a>(&'a self, owners: &'a [String]) -> BoxFuture<'a, anyhow::Result<Vec<String>>> {
        self.port().active_plans(owners)
    }

    fn chat_on<'a>(&'a self, account: &'a str) -> BoxFuture<'a, anyhow::Result<bool>> {
        self.port().chat_on(account)
    }

    fn roster_changed<'a>(&'a self, team: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        self.port().roster_changed(team)
    }
}

/// An out-of-process platform: `PlatformPort` over HTTP, and the way this
/// hub forwards its routes.
pub struct Remote {
    pub port: HttpPlatform,
    /// Its URL, without a trailing slash.
    base: String,
    key: PrincipalKey,
    /// No overall timeout: chat's live stream stays open as long as the
    /// page does. A read waits at most [`IDLE`].
    http: reqwest::Client,
}

impl Remote {
    pub fn new(base: &str, key: PrincipalKey) -> anyhow::Result<Self> {
        let base = base.trim_end_matches('/').to_owned();
        anyhow::ensure!(
            base.starts_with("http://") || base.starts_with("https://"),
            "--platform is in-process or the platform's URL, not {base:?}"
        );
        let http = arugula_control_common::roots::build(|| {
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(2))
                .read_timeout(IDLE)
                .tcp_nodelay(true)
                .no_proxy()
        })?;
        Ok(Remote { port: HttpPlatform::new(&base, key.clone())?, base, key, http })
    }
}

/// The platform's routes, as `routes.toml` lists them.
#[derive(Deserialize)]
struct List {
    route: Vec<Listed>,
}

#[derive(Deserialize)]
struct Listed {
    method: String,
    path: String,
    process: String,
}

/// Every platform route in `routes.toml`, by path, with the methods
/// each takes (`None`: every method).
pub fn platform_routes() -> Vec<(String, Option<MethodFilter>)> {
    let list: List = toml::from_str(include_str!("../routes.toml")).expect("routes.toml parses (routes::)");
    let mut out: Vec<(String, Option<MethodFilter>)> = Vec::new();
    for r in list.route.into_iter().filter(|r| r.process == "platform") {
        let m = match r.method.as_str() {
            "GET" => Some(MethodFilter::GET),
            "POST" => Some(MethodFilter::POST),
            "PUT" => Some(MethodFilter::PUT),
            "PATCH" => Some(MethodFilter::PATCH),
            "DELETE" => Some(MethodFilter::DELETE),
            _ => None,
        };
        match out.iter_mut().find(|(p, _)| *p == r.path) {
            Some((_, Some(have))) => *have = m.map_or(*have, |m| have.or(m)),
            Some((_, None)) => {}
            None => out.push((r.path, m)),
        }
    }
    out
}

/// The platform's routes as this hub serves them: the in-process router
/// behind the gate, or every listed platform route forwarded. Both behind
/// the maintenance switch.
pub fn router(app: &Arc<App>) -> Router<Arc<App>> {
    let routes = match &app.platform {
        PlatformAt::InProcess(p) => arugula_platform::router(p.clone())
            .layer(axum::middleware::from_fn_with_state(app.clone(), crate::hub_port::gate)),
        PlatformAt::Remote(_) => {
            let mut r = Router::new();
            for (path, methods) in platform_routes() {
                let m: MethodRouter<Arc<App>> = match methods {
                    Some(f) => axum::routing::on(f, forward),
                    None => axum::routing::any(forward),
                };
                r = r.route(&path, m);
            }
            r
        }
    };
    routes.layer(axum::middleware::from_fn_with_state(app.clone(), crate::maintenance::refuse))
}

/// A body at most this big (by its `Content-Length`) is read before it's
/// forwarded, so a call that couldn't reach the platform can be tried
/// again; a bigger one, or one of unknown length, streams through.
const BUFFER_UP_TO: u64 = 1024 * 1024;

/// How long the platform may go quiet mid-answer before the hub gives up
/// on it: chat's live stream sends a keep-alive every 25 s.
const IDLE: Duration = Duration::from_secs(60);

/// How long a call that can't reach the platform is tried again for.
const RECONNECT_FOR: Duration = Duration::from_secs(5);

/// What a call the platform never got is told to wait, in seconds.
const UNREACHABLE_RETRY: u64 = 5;

/// Headers that belong to one hop, not the call.
fn hop(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

/// What the platform is sent of the client's headers: not one hop's, not
/// the cookie or the signature ("The hub stays the identity provider": the
/// platform never sees either), and never a principal or internal header
/// a client made up.
fn forwarded_headers(from: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::with_capacity(from.len());
    for (k, v) in from {
        if hop(k)
            || *k == header::COOKIE
            || k.as_str() == crate::auth::AUTH_HEADER
            || k.as_str() == principal::HEADER
            || k.as_str() == internal::HEADER
        {
            continue;
        }
        out.append(k.clone(), v.clone());
    }
    out
}

/// What a client is told when the platform can't be reached: the
/// maintenance answer, since the call never got there.
fn unreachable() -> Response {
    crate::maintenance::answer(UNREACHABLE_RETRY)
}

/// One platform route, forwarded.
async fn forward(State(app): State<Arc<App>>, req: Request) -> Response {
    let PlatformAt::Remote(r) = &app.platform else {
        return ApiError(StatusCode::INTERNAL_SERVER_ERROR, "the platform is in process".into(), None).into_response();
    };
    let (mut parts, body) = req.into_parts();
    let vouch = match crate::hub_port::who(&app, &mut parts).await {
        Some(Asked::Principal(p)) => Vouch::Principal(p),
        Some(Asked::Refused { status, error, by }) => {
            Vouch::Not(NotVouched::Refused { status: status.as_u16(), error, by })
        }
        None => Vouch::Not(NotVouched::Nobody),
    };
    let request = hex::encode(arugula_e2e::random::<8>());
    let pq = parts.uri.path_and_query().map_or_else(|| parts.uri.path().to_owned(), |p| p.as_str().to_owned());
    let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes()).unwrap_or(reqwest::Method::GET);
    let mut headers = forwarded_headers(&parts.headers);
    let url = format!("{}{pq}", r.base);
    let small = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok()?.parse::<u64>().ok())
        .is_some_and(|n| n <= BUFFER_UP_TO);
    let started = std::time::Instant::now();
    let sign = |headers: &mut HeaderMap| {
        let h = r.key.sign_vouch(&vouch, method.as_str(), &pq, &request, arugula_e2e::now_ms());
        headers.insert(principal::HEADER, h.parse().expect("a header value"));
    };
    let sent = if small || parts.method == axum::http::Method::GET || parts.method == axum::http::Method::HEAD {
        let Ok(bytes) = axum::body::to_bytes(body, BUFFER_UP_TO as usize).await else {
            return ApiError(StatusCode::BAD_REQUEST, "the request's body didn't arrive".into(), None).into_response();
        };
        // Tried again while it can't connect: the call hasn't reached the
        // platform, so sending it again is safe.
        let mut wait = Duration::from_millis(100);
        loop {
            sign(&mut headers);
            let res = r.http.request(method.clone(), &url).headers(headers.clone()).body(bytes.clone()).send().await;
            match res {
                Err(e) if e.is_connect() && started.elapsed() + wait < RECONNECT_FOR => {
                    tokio::time::sleep(wait).await;
                    wait = (wait * 2).min(Duration::from_secs(1));
                }
                other => break other,
            }
        }
    } else {
        sign(&mut headers);
        let stream = body.into_data_stream();
        r.http.request(method, &url).headers(headers).body(reqwest::Body::wrap_stream(stream)).send().await
    };
    let res = match sent {
        Ok(res) => res,
        Err(e) if e.is_connect() => {
            warn!(%request, path = parts.uri.path(), error = %e, "the platform can't be reached");
            return unreachable();
        }
        Err(e) => {
            warn!(%request, path = parts.uri.path(), error = %e, "the platform didn't answer");
            return ApiError(StatusCode::BAD_GATEWAY, "the platform didn't answer".into(), None).into_response();
        }
    };
    let mut out = Response::builder().status(res.status().as_u16());
    if let Some(h) = out.headers_mut() {
        for (k, v) in res.headers() {
            if !hop(k) {
                h.append(k.clone(), v.clone());
            }
        }
    }
    let stream = res.bytes_stream().map_err(std::io::Error::other);
    out.body(Body::from_stream(stream)).unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every listed platform route is one path here, with each method it
    /// was listed with.
    #[test]
    fn the_list_is_routes_toml() {
        let routes = platform_routes();
        let get = |p: &str| routes.iter().find(|(path, _)| path == p).map(|(_, m)| *m);
        assert!(get("/api/chat/live").is_some());
        assert!(get("/api/stripe/webhook").is_some());
        assert!(get("/api/devices").is_none(), "a hub route");
        let mut paths: Vec<&str> = routes.iter().map(|(p, _)| p.as_str()).collect();
        let n = paths.len();
        paths.dedup();
        assert_eq!(paths.len(), n, "a path once");
        let mut platform: Vec<&str> = arugula_platform::paths();
        platform.sort();
        paths.sort();
        assert_eq!(paths, platform, "routes.toml's platform paths are the platform's router's");
    }

    #[test]
    fn what_the_platform_is_not_sent() {
        let mut h = HeaderMap::new();
        for (k, v) in [
            ("cookie", "ilg_session=x"),
            ("x-arugula-auth", "v2 a b c d"),
            ("x-arugula-principal", "v1 forged"),
            ("x-arugula-internal", "v1 forged"),
            ("connection", "keep-alive"),
            ("transfer-encoding", "chunked"),
            ("host", "control.arugula.io"),
            ("last-event-id", "7"),
            ("origin", "https://control.arugula.io"),
            ("stripe-signature", "t=1,v1=x"),
        ] {
            h.append(HeaderName::from_static(k), v.parse().unwrap());
        }
        let f = forwarded_headers(&h);
        let mut kept: Vec<&str> = f.keys().map(HeaderName::as_str).collect();
        kept.sort();
        assert_eq!(kept, ["host", "last-event-id", "origin", "stripe-signature"]);
    }
}
