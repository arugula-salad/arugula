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
//!    header (`arugula_control_common::principal`), the hub's newest change
//!    (`arugula_platform::cache::SEQ_HEADER`, so a call after a write reads
//!    it), the method, path,
//!    query and body untouched (a daemon's signature covers the body), the
//!    Host header too (billing's checkout goes back to the site it came
//!    from);
//! 5. passes the answer back as it comes, status, headers (`Retry-After`
//!    among them) and body, streamed: chat's live stream (SSE) goes through
//!    unbuffered, `Last-Event-ID` and all;
//! 6. adds where the time went to the platform's `Server-Timing` (`up`,
//!    `gw`; [`server_timing`]), and logs it at `debug`.
//!
//! A platform that can't be reached at all (a deploy restarting it) is
//! tried again for a few seconds, then answered as a maintenance 503: the
//! call never reached it, so any client may send it again. Through Flycast
//! (M144: `http://arugula-platform.flycast:7700`, two machines behind Fly's
//! proxy) the connection is the proxy's, which accepts it even with no
//! machine to give it to and then closes it unanswered: a `GET` or `HEAD`
//! closed that way is treated the same, since sending it again is safe.
//! Anything else closed unanswered is a 502, since it may have arrived.

use std::{sync::Arc, time::Duration};

use arugula_control_common::{
    ApiError, internal,
    principal::{self, NotVouched, PrincipalKey, Vouch},
};
use arugula_control_wire::forge::Standing;
use arugula_platform::{
    Deleted, Platform, PlatformPort,
    auth::Asked,
    forge::{GithubUser, TokenAnswer},
    remote::HttpPlatform,
};
use axum::{
    Router,
    body::Body,
    extract::{MatchedPath, Request, State},
    http::{HeaderMap, HeaderName, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{MethodFilter, MethodRouter},
};
use futures_util::{TryStreamExt as _, future::BoxFuture};
use serde::Deserialize;
use tracing::{debug, warn};

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

    fn forge_standing<'a>(
        &'a self,
        account: &'a str,
        github: Option<&'a GithubUser>,
        repos: &'a [String],
        fresh: bool,
    ) -> BoxFuture<'a, anyhow::Result<Vec<Standing>>> {
        self.port().forge_standing(account, github, repos, fresh)
    }

    fn github_token<'a>(
        &'a self,
        account: &'a str,
        github: Option<&'a GithubUser>,
        repo: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<TokenAnswer>> {
        self.port().github_token(account, github, repo)
    }

    fn send_draft<'a>(
        &'a self,
        account: &'a str,
        draft: &'a str,
        token: Option<arugula_platform::forge::send::UserToken>,
    ) -> BoxFuture<'a, anyhow::Result<arugula_platform::forge::send::Outcome>> {
        self.port().send_draft(account, draft, token)
    }

    fn hosted_wake<'a>(
        &'a self,
        project: &'a str,
        workspace: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<arugula_control_wire::hosted::WakeAnswer>> {
        self.port().hosted_wake(project, workspace)
    }

    fn hosted_turn<'a>(
        &'a self,
        workspace: &'a str,
        machine: &'a str,
        agent: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<arugula_platform::TurnAnswer>> {
        self.port().hosted_turn(workspace, machine, agent)
    }

    fn hosted_turn_end<'a>(
        &'a self,
        workspace: &'a str,
        machine: &'a str,
        turn: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<bool>> {
        self.port().hosted_turn_end(workspace, machine, turn)
    }

    fn hosted_work<'a>(
        &'a self,
        machine: &'a arugula_platform::work::HostedMachine,
        takes: &'a [String],
    ) -> BoxFuture<'a, anyhow::Result<arugula_control_wire::work::Work>> {
        self.port().hosted_work(machine, takes)
    }

    fn hosted_report<'a>(
        &'a self,
        machine: &'a arugula_platform::work::HostedMachine,
        report: &'a arugula_control_wire::work::Report,
    ) -> BoxFuture<'a, anyhow::Result<arugula_control_wire::work::Reported>> {
        self.port().hosted_report(machine, report)
    }

    fn hosted_session_report<'a>(
        &'a self,
        machine: &'a arugula_platform::work::HostedMachine,
        report: &'a arugula_control_wire::work::SessionReport,
    ) -> BoxFuture<'a, anyhow::Result<arugula_control_wire::work::Reported>> {
        self.port().hosted_session_report(machine, report)
    }

    fn hosted_secrets<'a>(
        &'a self,
        machine: &'a arugula_platform::work::HostedMachine,
        ask: &'a arugula_control_wire::work::SecretsAsk,
    ) -> BoxFuture<'a, anyhow::Result<arugula_control_wire::work::SecretsGiven>> {
        self.port().hosted_secrets(machine, ask)
    }

    fn hosted_artifact<'a>(
        &'a self,
        machine: &'a arugula_platform::work::HostedMachine,
        ask: &'a arugula_control_wire::work::SecretsAsk,
    ) -> BoxFuture<'a, anyhow::Result<arugula_control_wire::work::ArtifactGiven>> {
        self.port().hosted_artifact(machine, ask)
    }

    fn hosted_plan<'a>(
        &'a self,
        project: &'a str,
        workspace: &'a str,
        machine: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Option<arugula_control_wire::hosted::SetupPlan>>> {
        self.port().hosted_plan(project, workspace, machine)
    }

    fn hosted_setup<'a>(
        &'a self,
        project: &'a str,
        workspace: &'a str,
        machine: &'a str,
        report: &'a arugula_control_wire::hosted::SetupReport,
    ) -> BoxFuture<'a, anyhow::Result<bool>> {
        self.port().hosted_setup(project, workspace, machine, report)
    }

    fn machine_drives<'a>(&'a self, daemon: &'a str, account: &'a str) -> BoxFuture<'a, anyhow::Result<bool>> {
        self.port().machine_drives(daemon, account)
    }

    fn hosted_transcript<'a>(
        &'a self,
        workspace: &'a str,
        machine: &'a str,
        transcript: &'a arugula_control_wire::transcripts::TranscriptPost,
    ) -> BoxFuture<'a, anyhow::Result<arugula_platform::Archived>> {
        self.port().hosted_transcript(workspace, machine, transcript)
    }

    fn public_preview<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Option<arugula_platform::PublicPreview>>> {
        self.port().public_preview(id)
    }

    fn public_ports<'a>(&'a self, workspace: &'a str) -> BoxFuture<'a, anyhow::Result<Vec<(u16, String)>>> {
        self.port().public_ports(workspace)
    }

    fn hosted_upload<'a>(
        &'a self,
        machine: &'a arugula_platform::work::HostedMachine,
        upload: &'a arugula_control_wire::run_logs::Upload,
    ) -> BoxFuture<'a, anyhow::Result<arugula_control_wire::run_logs::UploadGiven>> {
        self.port().hosted_upload(machine, upload)
    }

    fn hosted_uploaded<'a>(
        &'a self,
        machine: &'a arugula_platform::work::HostedMachine,
        upload: &'a arugula_control_wire::run_logs::Upload,
    ) -> BoxFuture<'a, anyhow::Result<arugula_control_wire::run_logs::Uploaded>> {
        self.port().hosted_uploaded(machine, upload)
    }

    fn box_members<'a>(
        &'a self,
        project: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Option<Vec<arugula_platform::boxes::BoxMember>>>> {
        self.port().box_members(project)
    }

    fn box_post<'a>(
        &'a self,
        post: &'a arugula_platform::boxes::BoxPost,
    ) -> BoxFuture<'a, anyhow::Result<arugula_platform::boxes::BoxPosted>> {
        self.port().box_post(post)
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

/// Every platform route in `routes.toml`, one per method and path, as
/// `(method, path)`: `ANY` for every method.
pub fn listed_platform_routes() -> Vec<(String, String)> {
    let list: List = toml::from_str(include_str!("../routes.toml")).expect("routes.toml parses (routes::)");
    list.route.into_iter().filter(|r| r.process == "platform").map(|r| (r.method, r.path)).collect()
}

/// Every platform route in `routes.toml`, by path, with the methods
/// each takes (`None`: every method).
pub fn platform_routes() -> Vec<(String, Option<MethodFilter>)> {
    let mut out: Vec<(String, Option<MethodFilter>)> = Vec::new();
    for (method, path) in listed_platform_routes() {
        let m = match method.as_str() {
            "GET" => Some(MethodFilter::GET),
            "POST" => Some(MethodFilter::POST),
            "PUT" => Some(MethodFilter::PUT),
            "PATCH" => Some(MethodFilter::PATCH),
            "DELETE" => Some(MethodFilter::DELETE),
            _ => None,
        };
        match out.iter_mut().find(|(p, _)| *p == path) {
            Some((_, Some(have))) => *have = m.map_or(*have, |m| have.or(m)),
            Some((_, None)) => {}
            None => out.push((path, m)),
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

/// The platform routes a client's own `Authorization` is for, and so the
/// only ones it reaches (in process too, `hub_port::gate`): the MCP
/// endpoint's agent grant token (M230) and its uploads (M235), and a
/// registered client's credential on the inference broker (M231,
/// `/api/projects/{project}/inference/anthropic/{*rest}`) and on its
/// account's month there ([`INFERENCE_SPEND`]), and a registered client's
/// own routes (M236, [`client_route`]). On every other platform
/// route it's dropped, so no route can come to trust one by mistake: the
/// broker's other routes (`/api/inference/spend/paid`, the consent switch)
/// are a signed-in person's.
pub fn bearer_route(path: &str) -> bool {
    let s: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    path == arugula_control_wire::agent_mcp::MCP
        || path == arugula_control_wire::agent_mcp::UPLOADS
        || path == INFERENCE_SPEND
        || matches!(s.as_slice(), ["api", "projects", p, "inference", "anthropic", _, ..] if !p.is_empty())
        || client_route(&s)
}

/// A registered client's own routes (M236, `arugula_platform::clients`):
/// these four shapes, each segment filled, and nothing else under
/// `/api/client/`.
fn client_route(s: &[&str]) -> bool {
    match s {
        ["api", "client", "projects"] | ["api", "client", "apps"] => true,
        ["api", "client", "apps", app] => !app.is_empty(),
        ["api", "client", "apps", app, "secrets", name] => !app.is_empty() && !name.is_empty(),
        _ => false,
    }
}

/// An account's month on the platform's key through the inference broker
/// (M231), which a registered client reads with its credential.
pub const INFERENCE_SPEND: &str = "/api/inference/spend";

/// What the platform is sent of the client's headers: not one hop's, not
/// the cookie or the signature ("The hub stays the identity provider": the
/// platform never sees either), never a principal or internal header
/// a client made up, and `Authorization` only where [`bearer_route`] says.
fn forwarded_headers(from: &HeaderMap, path: &str) -> HeaderMap {
    let mut out = HeaderMap::with_capacity(from.len());
    for (k, v) in from {
        if hop(k)
            || (*k == header::AUTHORIZATION && !bearer_route(path))
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

/// Whether a call that failed may be sent again as if it never reached the
/// platform: it couldn't connect, or (a `GET` or `HEAD`, which is safe to
/// repeat) the connection closed with no answer, as Fly's proxy closes one
/// it has no healthy machine for.
fn never_reached(e: &reqwest::Error, method: &reqwest::Method) -> bool {
    e.is_connect()
        || (matches!(*method, reqwest::Method::GET | reqwest::Method::HEAD) && e.is_request() && !e.is_timeout())
}

fn parts_method(m: &axum::http::Method) -> reqwest::Method {
    reqwest::Method::from_bytes(m.as_str().as_bytes()).unwrap_or(reqwest::Method::GET)
}

/// Where a forwarded call's time went (`docs/control-ops.md`, "Where a
/// request's time goes"): the platform's own `Server-Timing` entries
/// (`arugula_platform::timing`), then the gateway's.
const SERVER_TIMING: &str = arugula_platform::timing::HEADER;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// The answer's `Server-Timing`: what the platform said, as it said it,
/// then `up` (from sending the call until the platform's answer began:
/// the private network, Flycast's proxy and the platform's whole time)
/// and `gw` (the gateway's whole time, `up` included, until the answer
/// begins). Durations only.
fn server_timing(from: &HeaderMap, up: Duration, gw: Duration) -> String {
    let mut parts: Vec<String> = from
        .get_all(SERVER_TIMING)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect();
    parts.push(format!("up;dur={:.2}, gw;dur={:.2}", ms(up), ms(gw)));
    parts.join(", ")
}

/// One platform route, forwarded; and out of process, a page GET on the
/// platform's host (`platform_host`), which the platform answers with its
/// page.
pub(crate) async fn forward(State(app): State<Arc<App>>, req: Request) -> Response {
    let PlatformAt::Remote(r) = &app.platform else {
        return ApiError(StatusCode::INTERNAL_SERVER_ERROR, "the platform is in process".into(), None).into_response();
    };
    let began = std::time::Instant::now();
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
    let method = parts_method(&parts.method);
    let mut headers = forwarded_headers(&parts.headers, parts.uri.path());
    crate::hub_port::seq_header(&app, &mut headers);
    let url = format!("{}{pq}", r.base);
    let small = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok()?.parse::<u64>().ok())
        .is_some_and(|n| n <= BUFFER_UP_TO);
    let started = std::time::Instant::now();
    let mut up;
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
            let sent = std::time::Instant::now();
            let res = r.http.request(method.clone(), &url).headers(headers.clone()).body(bytes.clone()).send().await;
            up = sent.elapsed();
            match res {
                Err(e) if never_reached(&e, &method) && started.elapsed() + wait < RECONNECT_FOR => {
                    tokio::time::sleep(wait).await;
                    wait = (wait * 2).min(Duration::from_secs(1));
                }
                other => break other,
            }
        }
    } else {
        sign(&mut headers);
        let stream = body.into_data_stream();
        let sent = std::time::Instant::now();
        let res = r.http.request(method, &url).headers(headers).body(reqwest::Body::wrap_stream(stream)).send().await;
        up = sent.elapsed();
        res
    };
    let res = match sent {
        Ok(res) => res,
        Err(e) if never_reached(&e, &parts_method(&parts.method)) => {
            warn!(%request, path = parts.uri.path(), error = %e, "the platform can't be reached");
            return unreachable();
        }
        Err(e) => {
            warn!(%request, path = parts.uri.path(), error = %e, "the platform didn't answer");
            return ApiError(StatusCode::BAD_GATEWAY, "the platform didn't answer".into(), None).into_response();
        }
    };
    let mut out = Response::builder().status(res.status().as_u16());
    let timing = server_timing(res.headers(), up, began.elapsed());
    debug!(
        %request,
        route = parts.extensions.get::<MatchedPath>().map_or("", MatchedPath::as_str),
        status = res.status().as_u16(),
        gw_ms = ms(began.elapsed()),
        up_ms = ms(up),
        platform = %timing,
        "forwarded"
    );
    // A platform that answers with its own error (a webhook it can't take,
    // say: M161) is in the hub's log too, by the same request id.
    if res.status().is_server_error() {
        warn!(%request, path = parts.uri.path(), status = res.status().as_u16(), "the platform answered with an error");
    }
    if let Some(h) = out.headers_mut() {
        for (k, v) in res.headers() {
            if !hop(k) && k.as_str() != SERVER_TIMING {
                h.append(k.clone(), v.clone());
            }
        }
        if let Ok(v) = timing.parse() {
            h.insert(SERVER_TIMING, v);
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
        h.append(header::AUTHORIZATION, "Bearer aru_agent_x".parse().unwrap());
        let f = forwarded_headers(&h, "/api/chat");
        let mut kept: Vec<&str> = f.keys().map(HeaderName::as_str).collect();
        kept.sort();
        assert_eq!(kept, ["host", "last-event-id", "origin", "stripe-signature"]);
        // A bearer crosses to the routes it's for (M230 and M235's uploads;
        // M231's broker and a client's view of its account's month; M236's
        // client routes), only.
        for path in [
            "/api/mcp",
            "/api/mcp/uploads",
            "/api/projects/prj_1/inference/anthropic/v1/messages",
            "/api/inference/spend",
            "/api/client/projects",
            "/api/client/apps",
            "/api/client/apps/app-1",
            "/api/client/apps/app-1/secrets/STRIPE",
        ] {
            assert!(forwarded_headers(&h, path).contains_key(header::AUTHORIZATION), "{path}");
        }
        for path in [
            "/api/mcp/x",
            "/api/mcp/uploads/x",
            "/api/deploys/dep_1/approve",
            "/api/projects/prj_1/inference/anthropic",
            "/api/projects//inference/anthropic/v1",
            "/api/agent-tokens",
            // A person's, never a client's.
            "/api/inference/spend/paid",
            "/api/inference/clients",
            "/api/inference/clients/studio/consent",
            "/api/inference/spend/",
            // M236's are the client's four route shapes, and nothing else under them.
            "/api/client",
            "/api/client/",
            "/api/client/other",
            "/api/client/apps/",
            "/api/client/apps/app-1/secrets",
            "/api/client/apps/app-1/secrets/",
            "/api/client/apps/app-1/other/STRIPE",
            "/api/client/apps/app-1/secrets/STRIPE/x",
            "/api/projects/prj_1/clients",
            "/api/projects/prj_1/secrets/STRIPE",
            "/api/linked-products/studio/projects",
        ] {
            assert!(!forwarded_headers(&h, path).contains_key(header::AUTHORIZATION), "{path}");
        }
    }
}
