//! HTTP: the embedded web client, and the WebSocket protocol at `/ws`.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use arugula_control_wire::telemetry::Surface;
use arugula_proto::op::ops::SigninLinkGet;
use arugula_proto::{ClientId, ClientMsg, Frame, FrameKind, PaneId, ServerMsg, block_patch};
use axum::{
    Extension, Router,
    body::Body,
    extract::{
        ConnectInfo, Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use rust_embed::Embed;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::{
    access::Access,
    acl::Principal,
    hosts::Hosts,
    mux::{Cmd, MuxHandle},
    ops::OpRoutes,
    pane::{Subscriber, ToClient, client_queue},
    tailscale::Identify,
};

#[derive(Embed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Assets;

pub struct App {
    pub access: Access,
    /// Who is on the other end of a TCP connection (tailscaled's WhoIs).
    pub identify: Identify,
    pub mux: MuxHandle,
    pub push: Option<crate::push::Push>,
    /// Claude Code's IDE (M28), if on.
    pub ide: Option<Arc<dyn crate::editors::Ide>>,
    pub hosts: Arc<Hosts>,
    /// Dial-out hosts connected to us (M4c).
    pub dial_outs: Arc<crate::dial::DialOuts>,
    /// Read-only share links.
    pub shares: Arc<crate::share::Shares>,
    /// History other hosts synced to us.
    pub synced: Arc<crate::sync::Synced>,
    /// Enrollment in Arugula control: trusted devices, the relay.
    pub control: Arc<crate::control::Control>,
    /// Who else may reach which sessions (M12).
    pub acl: Arc<crate::acl::Acl>,
    /// MCP's tokens (M16).
    pub mcp: Arc<crate::mcp::Tokens>,
    /// Devices lending their tools to agents (S33).
    pub hands: Arc<crate::hand::Hands>,
    /// Standing permission rules (#166).
    pub rules: Arc<crate::rules::Rules>,
    next_client: AtomicU64,
    /// The owner has reached us over the tailnet (#110: the phone step).
    pub tailnet_seen: std::sync::atomic::AtomicBool,
    /// When a Claude Code hook (`arugula hook`) last reached us from each
    /// pane (ms since the epoch): whether a pane's hooks are live, for MCP's
    /// `capabilities` (#229). Memory only.
    pub hooks_heard: std::sync::Mutex<std::collections::HashMap<PaneId, u64>>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        access: Access,
        identify: Identify,
        mux: MuxHandle,
        push: Option<crate::push::Push>,
        ide: Option<Arc<dyn crate::editors::Ide>>,
        hosts: Arc<Hosts>,
        shares: Arc<crate::share::Shares>,
        synced: Arc<crate::sync::Synced>,
        control: Arc<crate::control::Control>,
        acl: Arc<crate::acl::Acl>,
        mcp: Arc<crate::mcp::Tokens>,
        hands: Arc<crate::hand::Hands>,
        rules: Arc<crate::rules::Rules>,
    ) -> Arc<Self> {
        Arc::new(Self {
            access,
            identify,
            mux,
            push,
            ide,
            hosts,
            dial_outs: Default::default(),
            shares,
            synced,
            control,
            acl,
            mcp,
            hands,
            rules,
            next_client: AtomicU64::new(1),
            tailnet_seen: Default::default(),
            hooks_heard: Default::default(),
        })
    }

    pub fn new_client_id(&self) -> ClientId {
        self.next_client.fetch_add(1, Ordering::Relaxed)
    }
}

/// What a daemon serves to its owner: its own API, and MCP (M16).
fn own_routes(app: &Arc<App>) -> Router<Arc<App>> {
    crate::api::routes()
        .merge(crate::mcp::routes(app))
        .merge(crate::fs::routes())
        .merge(crate::hosts::routes())
        .merge(crate::share::api_routes())
        .merge(crate::acl::api::routes())
        .merge(crate::setup::routes())
        .merge(crate::update::routes())
        .merge(crate::selfupdate::routes())
}

/// Plus what makes it a home daemon: hosts dialing in and pushing history,
/// the way through to dial-out hosts (`/h/NAME`), its provider's sandboxes,
/// and the provider tunnel to resident daemons in them (`/tunnel/NAME`).
fn api_routes(app: &Arc<App>) -> Router<Arc<App>> {
    crate::lab_hooks::labs().home_routes(own_routes(app).merge(crate::dial::routes()).merge(crate::sync::routes()))
}

/// Over TCP (loopback, behind `tailscale serve`, or a tailnet address):
/// every request passes the access checks, and API calls from a browser
/// must come from one of our origins. Serve it with
/// `into_make_service_with_connect_info::<SocketAddr>()`.
pub fn router(app: Arc<App>) -> Router {
    let r = Router::new()
        .route("/ws", get(ws))
        .route(crate::e2e::PATH, get(crate::e2e::ws))
        .route(crate::localauth::AUTH_PATH, get(signin))
        .merge(
            api_routes(&app)
                .layer(middleware::from_fn_with_state(app.clone(), crate::authz::check))
                .layer(middleware::from_fn_with_state(app.clone(), api_origin)),
        )
        .merge(crate::share::viewer_routes());
    // M40: Forgejo's and GitLab's webhooks, by their signatures (the handler
    // checks); they are Labs'.
    crate::lab_hooks::labs()
        .hook_routes(r)
        .fallback(asset)
        .layer(middleware::from_fn_with_state(app.clone(), cors))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app)
}

/// Over the Unix socket (the CLI, programs in panes): the socket lives in
/// the user's private state directory, so reaching it is the check.
pub fn local_router(app: Arc<App>) -> Router {
    // Editors on this machine join the swarm here (M28).
    crate::editors::local_routes(Router::new().route("/ws", get(local_ws)))
        // `arugula web`: only over the socket, which is the owner's.
        .op::<SigninLinkGet>()
        // Stop: save every pane and exit, as on Ctrl-C (an upgrade on
        // Windows, where there's no service manager to ask; M59).
        .route("/api/daemon/stop", axum::routing::post(stop))
        .merge(api_routes(&app))
        .with_state(app)
}

#[cfg(unix)]
/// Editors only (M28): `<state>/editors/sock`, the one socket a dev
/// container gets (its directory mounted): joining the swarm as an editor
/// is all it can do there, not drive the daemon.
pub fn editors_router(app: Arc<App>) -> Router {
    crate::editors::local_routes(Router::new()).with_state(app)
}

/// Over the tunnel to the home daemon (a dial-out host): the home daemon
/// checked who is asking. Our own WebSocket and API only: nothing that
/// would make this host a way to anywhere else (no `/h/`, no dialing in).
pub fn tunnel_router(app: Arc<App>) -> Router {
    Router::new().route("/ws", get(local_ws)).merge(own_routes(&app)).with_state(app)
}

/// Inside an end-to-end channel (`e2e.rs`): the device was checked by the
/// handshake. The daemon's own API only, as over the tunnel.
pub fn channel_router(app: Arc<App>) -> Router {
    own_routes(&app).layer(middleware::from_fn_with_state(app.clone(), crate::authz::check)).with_state(app)
}

/// An embedded web client file.
pub fn asset_file(path: &str) -> Option<Vec<u8>> {
    Assets::get(path).map(|f| f.data.into_owned())
}

/// Cross-site requests can't read our answers, but a POST still lands: so a
/// browser's API call must come from one of our own origins. Programs (no
/// Origin header) are fine. Nor does a link on another site open an API
/// address in the browser (`check_api_navigation`).
async fn api_origin(State(app): State<Arc<App>>, req: Request, next: Next) -> Response {
    match crate::access::check_api_navigation(req.headers()).and_then(|()| app.access.check_origin(req.headers())) {
        Ok(()) => next.run(req).await,
        Err((status, why)) => {
            warn!(%why, uri = %req.uri(), "rejected API request");
            (status, why).into_response()
        }
    }
}

/// The home daemon's page talks to other daemons' APIs (another origin):
/// browsers allow that only if we say so, and we say so only to origins the
/// access checks accept, exactly. On the tailnet identity is the tailnet's;
/// on this machine it's the page key header (`access::PAGE_KEY_HEADER`), so
/// that header and credentials are allowed (for those exact origins only).
async fn cors(State(app): State<Arc<App>>, req: Request, next: Next) -> Response {
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|o| o.to_str().ok())
        .filter(|o| app.access.origin_allowed(o))
        .map(str::to_owned);
    let Some(origin) = origin else { return next.run(req).await };
    let preflight = req.method() == axum::http::Method::OPTIONS
        && req.headers().contains_key(header::ACCESS_CONTROL_REQUEST_METHOD);
    let mut res = if preflight { StatusCode::NO_CONTENT.into_response() } else { next.run(req).await };
    let h = res.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&origin) {
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
    }
    h.append(header::VARY, HeaderValue::from_static("origin"));
    h.insert(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, HeaderValue::from_static("true"));
    if preflight {
        h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, DELETE"));
        h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("content-type, x-arugula-key"));
        h.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
    }
    res
}

async fn guard(
    State(app): State<Arc<App>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let peer = app.identify.peer(addr).await;
    let mut req = req;
    page_key_from_query(&mut req);
    // A request serve passed on: the tailnet address it came from (#663).
    let mut forwarded: Option<String> = None;
    // The app's own files, for a browser on this machine: the same for
    // everyone, and a page load can't carry the page key. Through serve
    // (or from the tailnet), as everything else.
    let class = match class(&req, &app.access) {
        Class::Shell
            if peer == crate::access::Peer::Local
                && !app.access.tunnelled()
                && !req.headers().contains_key("tailscale-user-login")
                && !app.access.is_public_host(req.headers()) =>
        {
            Class::Shell
        }
        Class::Shell => Class::Owner,
        c => c,
    };
    let signin = matches!(class, Class::SignIn);
    let checked = app.access.check_host(req.headers()).and_then(|()| match class {
        Class::Shell => Ok(()),
        Class::Owner => {
            // serve's identity header, on loopback: only from tailscaled.
            if peer == crate::access::Peer::Local
                && !app.access.tunnelled()
                && req.headers().contains_key("tailscale-user-login")
                && !crate::localauth::serve_peer_ok(addr, app.access.port())
            {
                return Err((
                    StatusCode::FORBIDDEN,
                    "a Tailscale-User-Login header from a local account other than tailscaled's".into(),
                ));
            }
            // From serve: a person's request (the header), or a tagged
            // node's (none, for a tailnet name), which is refused below.
            if peer == crate::access::Peer::Local
                && !app.access.tunnelled()
                && (req.headers().contains_key("tailscale-user-login") || app.access.is_public_host(req.headers()))
                && crate::localauth::serve_peer_ok(addr, app.access.port())
            {
                forwarded = req.headers().get("x-forwarded-for").and_then(|v| v.to_str().ok()).map(str::to_owned);
            }
            let pic = req.headers().get("tailscale-user-profile-pic").and_then(|v| v.to_str().ok()).map(str::to_owned);
            let who = app.access.check_identity(req.headers(), &peer)?.with_pic(pic);
            // Another user gets in only once something is shared with them.
            if !app.acl.knows(&who) {
                let why = match &who {
                    crate::acl::Principal::User { id, name, .. } if id.starts_with("tailnet:") => {
                        app.access.not_yours(name)
                    }
                    _ => "nothing on this machine is shared with you".into(),
                };
                return Err((StatusCode::FORBIDDEN, why));
            }
            // The phone step is done once the owner comes in over the tailnet (#110).
            if matches!(who, crate::acl::Principal::Owner) && app.access.via_tailnet(req.headers(), &peer) {
                app.tailnet_seen.store(true, Ordering::Relaxed);
            }
            req.extensions_mut().insert(who);
            Ok(())
        }
        Class::Viewer => app.access.check_viewer(req.headers(), &peer),
        Class::Token => Ok(()),
        // The link carries the credential; a browser on this machine only.
        Class::SignIn if peer == crate::access::Peer::Local && !app.access.tunnelled() => Ok(()),
        Class::SignIn => Err((StatusCode::FORBIDDEN, "sign-in links are for this machine's browsers".into())),
        // A browser asking first, for one of our origins (`cors` answers
        // it; it carries no credentials, and its answer gives nothing away).
        Class::Preflight => app.access.check_origin(req.headers()),
        // From this machine or the tailnet; the MCP layer checks the token.
        Class::McpToken if matches!(peer, crate::access::Peer::Other) => {
            Err((StatusCode::FORBIDDEN, "not from this machine or the tailnet".into()))
        }
        Class::McpToken => Ok(()),
    });
    if let Some(f) = forwarded {
        app.identify.saw_forwarded(&f).await;
    }
    // Checked: nothing past here (a handler, a dial-out host's proxy) sees
    // it, but the sign-in link's page, which hands it back.
    if !signin {
        req.headers_mut().remove(crate::access::PAGE_KEY_HEADER);
    }
    let mut res = match checked {
        Ok(()) => next.run(req).await,
        Err((status, why)) => {
            warn!(%why, ?peer, %addr, uri = %req.uri(), "rejected request");
            // A browser opening the page gets one it can copy the fix from (#109).
            let page = req
                .headers()
                .get(header::ACCEPT)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|a| a.contains("text/html"));
            let mut res = if page {
                (status, axum::response::Html(crate::access::refusal_page(status, &why))).into_response()
            } else {
                (status, why).into_response()
            };
            // The page's cue to go to the sign-in link's page (local-key.ts),
            // rather than any 401 an API call answers for its own reasons.
            if status == StatusCode::UNAUTHORIZED
                && peer == crate::access::Peer::Local
                && !app.access.tunnelled()
                && app.access.needs_local_token()
            {
                res.headers_mut().insert("x-arugula-sign-in", HeaderValue::from_static("/auth"));
            }
            res
        }
    };
    // serve authenticates by source, so any page the owner visits could frame
    // the logged-in app (clickjacking). Nothing frames us legitimately.
    // (A stricter policy already set, on a dial-out host's answer, stays.)
    let h = res.headers_mut();
    h.entry(header::CONTENT_SECURITY_POLICY).or_insert(HeaderValue::from_static("frame-ancestors 'none'"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    res
}

/// A browser's WebSocket can't carry the page key as a header, so the page
/// puts it in the URL (`?arugula_key=`): moved to the header here, and out
/// of the URL, so it isn't logged or passed on.
fn page_key_from_query(req: &mut Request) {
    const PARAM: &str = "arugula_key";
    let upgrade = req.headers().get(header::UPGRADE).and_then(|v| v.to_str().ok());
    if !upgrade.is_some_and(|u| u.eq_ignore_ascii_case("websocket")) {
        return;
    }
    let Some(query) = req.uri().query() else { return };
    let mut key = None;
    let rest: Vec<&str> = query
        .split('&')
        .filter(|kv| match kv.split_once('=') {
            Some((PARAM, v)) => {
                key = Some(v.to_owned());
                false
            }
            _ => *kv != PARAM,
        })
        .collect();
    let Some(key) = key else { return };
    let path = req.uri().path();
    let pq = if rest.is_empty() { path.to_owned() } else { format!("{path}?{}", rest.join("&")) };
    let mut parts = req.uri().clone().into_parts();
    let Ok(pq) = pq.parse() else { return };
    parts.path_and_query = Some(pq);
    if let Ok(uri) = Uri::from_parts(parts) {
        *req.uri_mut() = uri;
    }
    // The page's own key wins over a header something else added.
    if let Ok(v) = HeaderValue::from_str(&key) {
        req.headers_mut().insert(crate::access::PAGE_KEY_HEADER, v);
    }
}

/// The app's own files: the page and what Vite built for it.
fn shell_file(path: &str) -> bool {
    let file = path.trim_start_matches('/');
    file.is_empty() || (!file.starts_with("api/") && !file.contains("..") && Assets::get(file).is_some())
}

/// Whose request this can be.
enum Class {
    /// The owner's, like everything by default.
    Owner,
    /// A share link's viewer: the viewer page, its socket and static files.
    Viewer,
    /// A host's, without a user identity: the token it carries (an invite,
    /// or its per-host token) is the credential, checked by the handler.
    /// Or an end-to-end channel, whose handshake is the credential.
    /// Joining is how a tagged sandbox node adds itself; dialing in and
    /// pushing history are how a dial-out host reaches us. A forge's
    /// webhook (M40) is signed with a secret only this daemon and the forge
    /// hold.
    Token,
    /// MCP with a bearer token (M16): an MCP client without a tailnet
    /// identity of its own, or an agent block's.
    McpToken,
    /// The sign-in link (`localauth.rs`), whose token the handler checks.
    SignIn,
    /// A CORS preflight.
    Preflight,
    /// One of the app's own files (`shell_file`): open to a browser on
    /// this machine, the owner's otherwise.
    Shell,
}

/// Exactly `/share/<token>`, `/share/<token>/ws`, `/assets/<file>` or the
/// icon: nothing with dots that a router or proxy might resolve elsewhere.
fn viewer_path(path: &str) -> bool {
    let plain = |s: &str| !s.is_empty() && !s.starts_with('.') && !s.contains('%');
    match path.trim_start_matches('/').split('/').collect::<Vec<_>>().as_slice() {
        ["share", token] | ["share", token, "ws"] => plain(token),
        ["assets", file] => plain(file),
        ["icon.svg"] | ["favicon.ico"] => true,
        _ => false,
    }
}

fn class(req: &Request, access: &Access) -> Class {
    use axum::http::Method;
    let (m, path) = (req.method(), req.uri().path());
    if m == Method::GET && path == crate::localauth::AUTH_PATH {
        return Class::SignIn;
    }
    if m == Method::OPTIONS
        && req.headers().contains_key(header::ACCESS_CONTROL_REQUEST_METHOD)
        && req.headers().contains_key(header::ORIGIN)
    {
        return Class::Preflight;
    }
    if (m == Method::POST && path == crate::hosts::JOIN_PATH)
        || (m == Method::GET && path == crate::dial::DIAL_PATH)
        || (m == Method::GET && path == crate::e2e::PATH)
        || path.starts_with(crate::sync::PUSH_PREFIX)
        || (m == Method::POST && crate::lab_hooks::labs().is_hook(path))
    {
        Class::Token
    } else if path == crate::mcp::PATH
        && req.headers().get(header::AUTHORIZATION).is_some()
        && !access.local_bearer(req.headers())
    {
        Class::McpToken
    } else if m == Method::GET && viewer_path(path) {
        Class::Viewer
    } else if (m == Method::GET || m == Method::HEAD) && shell_file(path) {
        Class::Shell
    } else {
        Class::Owner
    }
}

#[derive(serde::Deserialize)]
struct SignInQuery {
    token: Option<String>,
    next: Option<String>,
}

/// A sign-in link (`/auth?token=…[&next=/path]`): a page that gives this
/// browser the page key (kept in this origin's storage, which is this
/// port's alone) and goes on to the app. Without a token, a browser that
/// still has an older link's cookie (the token) is signed in from it; the
/// cookie is removed either way.
async fn signin(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<SignInQuery>,
) -> Response {
    // Without a token: an older link's cookie (for a while after an
    // upgrade), or the page key itself (a browser that sends it on every
    // request, as the specs' do), which gets nothing it hasn't got.
    let ok = match &q.token {
        Some(t) => app.access.is_local_token(t),
        None => app.access.legacy_cookie(&headers) || app.access.is_page_key(&headers),
    };
    let key = app.access.page_key().filter(|_| ok);
    let Some(key) = key else {
        let why = if q.token.is_some() {
            "That sign-in link isn't this daemon's (its token changed, or it's another daemon's).\n\n\
             Run this in a terminal here for a new one:\n\narugula web"
        } else {
            "This machine's Arugula needs you to sign in from it.\n\n\
             Run this in a terminal here, which opens the page signed in:\n\narugula web"
        };
        return (
            StatusCode::UNAUTHORIZED,
            axum::response::Html(crate::access::refusal_page(StatusCode::UNAUTHORIZED, why)),
        )
            .into_response();
    };
    // Only a path of ours: never another site.
    let next = q.next.filter(|n| n.starts_with('/') && !n.starts_with("//") && !n.contains('\\')).unwrap_or("/".into());
    let clear = format!("{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0", app.access.legacy_cookie_name());
    let mut res = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::REFERRER_POLICY, "no-referrer");
    if let Ok(v) = HeaderValue::from_str(&clear) {
        res = res.header(header::SET_COOKIE, v);
    }
    res.body(Body::from(signin_page(key, &next))).unwrap()
}

/// The sign-in link's page: stores `key` where the app's page looks for it
/// (`web/src/shell/local-key.ts`), then goes to `next`.
fn signin_page(key: &str, next: &str) -> String {
    // JSON in a script: `<` escaped, so nothing in it can close the tag.
    let js = |v: &str| serde_json::to_string(v).unwrap_or_default().replace('<', "\\u003c");
    format!(
        "<!doctype html><meta charset=utf-8><meta name=referrer content=no-referrer><title>arugula</title>\
         <script>try{{localStorage.setItem(\"arugula.key\",{})}}catch(e){{}}location.replace({})</script>\
         <noscript>Arugula's page needs JavaScript.</noscript>",
        js(key),
        js(next)
    )
}

async fn stop() -> &'static str {
    crate::STOP.notify_one();
    "stopping"
}

async fn asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path) {
        Some(file) => {
            // Vite fingerprints everything under assets/; the rest must revalidate.
            let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
            Response::builder()
                .header(header::CONTENT_TYPE, file.metadata.mimetype())
                .header(header::CACHE_CONTROL, cache)
                .body(Body::from(file.data))
                .unwrap()
        }
        None if path == "index.html" => (StatusCode::NOT_FOUND, "web client not built: run `just web`").into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn ws(
    State(app): State<Arc<App>>,
    who: Option<Extension<Principal>>,
    headers: HeaderMap,
    uri: Uri,
    upgrade: WebSocketUpgrade,
) -> Response {
    if let Err((status, why)) = app.access.check_origin(&headers) {
        warn!(%why, "rejected websocket");
        return (status, why).into_response();
    }
    let who = who.map(|Extension(w)| w).unwrap_or(Principal::Owner);
    let surface = crate::telemetry::surface_of(&headers, uri.query());
    upgrade.on_upgrade(move |socket| connection(app, socket, who, surface))
}

async fn local_ws(State(app): State<Arc<App>>, upgrade: WebSocketUpgrade) -> Response {
    // The socket's WebSocket is `arugula attach` (and an older CLI's TUI).
    upgrade.on_upgrade(move |socket| connection(app, socket, Principal::Owner, Surface::Cli))
}

async fn connection(app: Arc<App>, mut socket: WebSocket, who: Principal, surface: Surface) {
    crate::telemetry::client(surface);
    let client: ClientId = app.new_client_id();
    info!(client, who = who.id(), "client connected");
    let (data_tx, mut data_rx) = client_queue();
    let (ctrl_tx, mut ctrl_rx) = mpsc::unbounded_channel();
    app.hands.connect(client, ctrl_tx.clone(), who.is_owner(), None);
    app.mux.send(Cmd::Connect { sub: Subscriber { client, data: data_tx, ctrl: ctrl_tx, principal: who, name: None } });

    loop {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(msg)) => {
                    if let Err(e) = handle(&app, client, msg) {
                        debug!(client, error = %e, "bad client message");
                    }
                }
                Some(Err(e)) => {
                    debug!(client, error = %e, "websocket error");
                    break;
                }
                None => break,
            },
            Some(out) = ctrl_rx.recv() => {
                let mut gone = false;
                for out in ctrl_batch(out, &mut ctrl_rx) {
                    if matches!(out, ToClient::Close) || send(&mut socket, out).await.is_err() {
                        gone = true;
                        break;
                    }
                }
                if gone { break }
            }
            Some(out) = data_rx.recv() => if send(&mut socket, out).await.is_err() { break },
        }
    }
    app.mux.send(Cmd::Disconnect { client });
    app.hands.disconnect(client);
    info!(client, "client disconnected");
}

pub(crate) fn handle(app: &App, client: ClientId, msg: Message) -> anyhow::Result<()> {
    match msg {
        Message::Text(text) => match serde_json::from_str::<ClientMsg>(&text)? {
            ClientMsg::Hand { tools, name } => app.hands.offer(client, tools, name),
            ClientMsg::HandReply { id, result, error } => app.hands.reply(client, id, result, error),
            msg => app.mux.send(Cmd::Msg { client, msg }),
        },
        Message::Binary(bytes) => {
            let frame = Frame::decode(&bytes)?;
            match frame.kind {
                FrameKind::Input => {
                    app.mux.send(Cmd::Input { client: Some(client), pane: frame.pane, data: frame.data })
                }
                k => anyhow::bail!("unexpected frame kind {k:?} from client"),
            }
        }
        _ => {}
    }
    Ok(())
}

/// Most a client's control queue is drained by at once.
const CTRL_BATCH: usize = 1024;

/// `first` and what else is in a client's control queue now, with each
/// block's states merged into its newest (#713). A block's state is sent
/// whole, or as a patch on the one before ([`block_patch`]), so one still
/// queued behind a newer one for the same block folds into it: a client that
/// fell behind (a phone, a slow link, a busy page) skips to the latest
/// instead of working through every state it missed. Everything else keeps
/// its order, and nothing after a `Close` is taken.
pub(crate) fn ctrl_batch(first: ToClient, rx: &mut mpsc::UnboundedReceiver<ToClient>) -> Vec<ToClient> {
    let mut batch = vec![first];
    while batch.len() < CTRL_BATCH && !matches!(batch.last(), Some(ToClient::Close)) {
        match rx.try_recv() {
            Ok(o) => batch.push(o),
            Err(_) => break,
        }
    }
    let mut out: Vec<Option<ToClient>> = Vec::with_capacity(batch.len());
    let mut last: HashMap<PaneId, usize> = HashMap::new();
    for o in batch {
        let ToClient::Msg(ServerMsg::Block { block, state }) = o else {
            out.push(Some(o));
            continue;
        };
        let merged = last.get(&block).and_then(|&i| match &out[i] {
            Some(ToClient::Msg(ServerMsg::Block { state: older, .. })) => {
                Some((i, block_patch::merge(older, state.clone())?))
            }
            _ => None,
        });
        let state = match merged {
            Some((i, m)) => {
                out[i] = None;
                m
            }
            None => state,
        };
        last.insert(block, out.len());
        out.push(Some(ToClient::Msg(ServerMsg::Block { block, state })));
    }
    out.into_iter().flatten().collect()
}

async fn send(socket: &mut WebSocket, out: ToClient) -> Result<(), axum::Error> {
    let msg = match out {
        ToClient::Frame(bytes) => Message::Binary(bytes.into()),
        ToClient::Msg(m) => Message::Text(serde_json::to_string(&m).expect("serialize").into()),
        ToClient::Json(t) => Message::Text(t.into()),
        ToClient::Close => return Ok(()),
    };
    socket.send(msg).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req(uri: &str, upgrade: bool) -> Request {
        let mut b = axum::http::Request::builder().uri(uri);
        if upgrade {
            b = b.header(header::UPGRADE, "websocket");
        }
        b.body(Body::empty()).unwrap()
    }

    #[test]
    fn a_websockets_page_key_moves_from_its_url_to_the_header() {
        let mut r = req("/ws?arugula_key=ilp_abc&surface=web", true);
        page_key_from_query(&mut r);
        assert_eq!(r.uri().to_string(), "/ws?surface=web");
        assert_eq!(r.headers()["x-arugula-key"], "ilp_abc");
        let mut r = req("/h/box/ws?arugula_key=ilp_abc", true);
        r.headers_mut().insert("x-arugula-key", HeaderValue::from_static("ilp_other"));
        page_key_from_query(&mut r);
        assert_eq!(r.uri().to_string(), "/h/box/ws");
        assert_eq!(r.headers()["x-arugula-key"], "ilp_abc", "the page's own key wins");
        // Only for a WebSocket: elsewhere it stays put and counts for nothing.
        let mut r = req("/api/panes?arugula_key=ilp_abc", false);
        page_key_from_query(&mut r);
        assert_eq!(r.uri().to_string(), "/api/panes?arugula_key=ilp_abc");
        assert!(!r.headers().contains_key("x-arugula-key"));
        let mut r = req("/ws?surface=web", true);
        page_key_from_query(&mut r);
        assert_eq!(r.uri().to_string(), "/ws?surface=web");
    }

    #[test]
    fn the_signin_page_stores_the_key_and_goes_on() {
        let p = signin_page("ilp_abc", "/#pane=3");
        assert!(p.contains(r#"localStorage.setItem("arugula.key","ilp_abc")"#), "{p}");
        assert!(p.contains(r#"location.replace("/#pane=3")"#), "{p}");
        // Nothing in `next` closes the script.
        let p = signin_page("ilp_abc", "/</script><script>alert(1)//");
        assert!(!p.contains("</script><script>alert"), "{p}");
        assert!(!shell_file("/api/panes") && !shell_file("/ws") && !shell_file("/auth"));
        assert!(shell_file("/"));
    }

    fn state(block: u32, n: u64) -> ToClient {
        ToClient::Msg(ServerMsg::Block { block, state: json!({ "n": n }) })
    }

    /// What a batch says, as `block:n`, `pong:id` or `close`.
    fn said(batch: &[ToClient]) -> Vec<String> {
        batch
            .iter()
            .map(|o| match o {
                ToClient::Msg(ServerMsg::Block { block, state }) => format!("{block}:{}", state["n"]),
                ToClient::Msg(ServerMsg::Pong { id }) => format!("pong:{id}"),
                ToClient::Close => "close".into(),
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn a_client_behind_gets_each_blocks_newest_state_and_everything_else_in_order() {
        // #713: an agent streaming while its client can't keep up queues one
        // whole state after another; only the newest of each block is sent.
        let (tx, mut rx) = mpsc::unbounded_channel();
        for o in [state(7, 2), ToClient::Msg(ServerMsg::Pong { id: 1 }), state(9, 1), state(7, 3), state(7, 4)] {
            tx.send(o).unwrap();
        }
        assert_eq!(said(&ctrl_batch(state(7, 1), &mut rx)), ["pong:1", "9:1", "7:4"]);
        // The queue was drained: the next one starts afresh.
        tx.send(state(7, 5)).unwrap();
        let next = rx.try_recv().unwrap();
        assert_eq!(said(&ctrl_batch(next, &mut rx)), ["7:5"]);
    }

    #[test]
    fn patches_queued_behind_a_state_fold_into_it() {
        // A client that takes patches and fell behind gets the state they
        // add up to, once.
        let entries = |from: u64, es: &[&str]| json!({ "entries_from": from, "entries": es });
        let block = |state| ToClient::Msg(ServerMsg::Block { block: 7, state });
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(block(block_patch::to_patch(&entries(0, &["a", "b", "c"]), 2))).unwrap();
        tx.send(block(block_patch::to_patch(&entries(1, &["b", "c", "d"]), 3))).unwrap();
        let batch = ctrl_batch(block(entries(0, &["a", "b"])), &mut rx);
        let [ToClient::Msg(ServerMsg::Block { state, .. })] = &batch[..] else { panic!("{batch:?}") };
        assert_eq!(*state, entries(1, &["b", "c", "d"]));
    }

    #[test]
    fn nothing_after_a_close_is_taken() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        for o in [ToClient::Close, state(7, 2)] {
            tx.send(o).unwrap();
        }
        assert_eq!(said(&ctrl_batch(state(7, 1), &mut rx)), ["7:1", "close"]);
        assert!(rx.try_recv().is_ok(), "what came after the close stays queued");
    }
}
