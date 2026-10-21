//! Block origins (M50, #150, from S27): a port or editor block on any
//! joined machine shows in control's page, each on an origin of its own,
//! `https://b-<site key>.<block domain>`. The site key is the daemon's
//! (`sites.rs`'s `site_key`, 32 hex digits); the block domain is
//! `--block-domain`, a registrable domain of its own so that every block is
//! cross-site to control's page and to every other block. Off when unset.
//!
//! On a block origin control serves only its own static files, and none of
//! its own page, cookies or API:
//!
//! - `/.arugula/boot.js`, `/.arugula/sw.js`, `/.arugula/shim.js`: the
//!   bootstrap script, the block's service worker and the WebSocket shim
//!   (`web/src/shell/block/`, built into `web/dist/block/`);
//! - a bootstrap page for any navigation the worker didn't answer (the first
//!   load, a hard reload, a cleared worker), under a strict CSP that lets
//!   only control's own origins frame it;
//! - control's worker again, wrapped, for a block's app that registers a
//!   worker of its own (`Service-Worker: script`): S27 found VS Code's
//!   webviews need it;
//! - `/.arugula/relay?d=<daemon id>`: the block's channel to that daemon,
//!   spliced onto a stream of the daemon's relay socket like
//!   `/api/relay/c`.
//!
//! Everything else a block's page loads comes through its worker, over a
//! Noise channel to the daemon, so anything else that reaches control here
//! is answered 503: the worker wasn't in the way.
//!
//! **The relay has no session.** A block's frame carries no cookie of
//! control's, and control can't read the grant in the channel's message 1
//! (`arugula_e2e::block`), so it can't tell a block from a stranger. The
//! daemon decides, in message 2. Control only relays ciphertext, and keeps
//! strangers' sockets cheap: a limit per client address, sockets at once
//! per daemon and per block origin, the browser's message 1 within
//! [`FIRST_WITHIN`] and nothing idle past [`IDLE`]. The socket counts
//! against the relay's ceiling and the daemon owner's traffic, never
//! against the owner's own sockets. An unknown daemon and one that isn't
//! connected get the same answer, so control says no more about a daemon
//! than the daemon's own answer to a handshake would (which anyone holding
//! its id and Noise key can already get).

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use arugula_e2e::{channel::MAX_WIRE, now_ms};
use axum::{
    body::Body,
    extract::{
        ConnectInfo, FromRequestParts, Request,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tracing::warn;

use crate::{App, relay::Ticket};

/// Where control's own files live on a block origin. A dev server's own
/// paths never start with it (the worker passes everything else through).
pub const PREFIX: &str = "/.arugula/";
/// Block sockets at once to one daemon, and from one block origin (a page's
/// worker and its shim's channel, a few tabs, VS Code's webviews).
pub const PER_DAEMON: usize = 64;
pub const PER_ORIGIN: usize = 16;
/// The browser's message 1 comes within this, or the socket closes.
pub const FIRST_WITHIN: Duration = Duration::from_secs(10);
/// A block socket with nothing crossing either way for this long closes.
/// The worker opens a new one on its next request; a hot-reload socket
/// through the shim reconnects (its dev server's client reloads the page).
pub const IDLE: Duration = Duration::from_secs(60 * 60);
/// What a socket to an unknown daemon, or one that isn't connected, hears:
/// the same for both.
pub const UNREACHABLE: &str = "can't reach that machine";
const CLIENT_PING: Duration = Duration::from_secs(30);
const CLIENT_READ_BUFFER: usize = 8 * 1024;

const JS: &str = "text/javascript; charset=utf-8";
const HTML: &str = "text/html; charset=utf-8";

/// What a request's Host makes it.
#[derive(Debug, PartialEq, Eq)]
pub enum Host {
    /// A block's origin: `b-<key>.<block domain>`, with its key.
    Block(String),
    /// `docs.<block domain>`: shared Docs' viewer (`doc_links.rs`).
    Docs,
    /// Under the block domain, but no block's (the bare domain, another
    /// label, a key that isn't one): nothing here.
    Nothing,
    /// Control's own, or anything else (as before blocks).
    Control,
}

/// A block domain as `--block-domain` takes it: lower case, no dots at the
/// ends, and never control's own host or one of its parents or children
/// (a block must be cross-site to control's page).
pub fn domain(raw: &str, sites: &[crate::Site]) -> anyhow::Result<Option<String>> {
    let d = raw.trim().trim_matches('.').to_ascii_lowercase();
    if d.is_empty() {
        return Ok(None);
    }
    if d.contains(['/', ':', ' ']) || !d.contains('.') {
        anyhow::bail!("--block-domain {raw:?}: a domain name, like arugula.run");
    }
    for s in sites {
        let h = s.host().to_ascii_lowercase();
        if h == d || h.ends_with(&format!(".{d}")) || d.ends_with(&format!(".{h}")) {
            anyhow::bail!(
                "--block-domain {d} shares a site with control's {h}: blocks need a registrable domain of their own"
            );
        }
    }
    Ok(Some(d))
}

/// Which kind of host a request is for, with `domain` the block domain.
pub fn host(domain: &str, headers: &HeaderMap) -> Host {
    let Some(h) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else { return Host::Control };
    // Without its port (an IPv6 literal keeps its brackets).
    let name = h.rsplit_once(':').filter(|(_, p)| p.parse::<u16>().is_ok()).map_or(h, |(n, _)| n);
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    let label = if name == domain {
        ""
    } else {
        match name.strip_suffix(domain).and_then(|l| l.strip_suffix('.')) {
            Some(l) => l,
            None => return Host::Control,
        }
    };
    // A fixed label: it can't be a `b-<hex>` one.
    if label == "docs" {
        return Host::Docs;
    }
    // `b-` and the daemon's site key (32 hex digits today; a label holds
    // at most 63 characters).
    match label.strip_prefix("b-") {
        Some(key) if (16..=61).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Host::Block(key.to_owned())
        }
        _ => Host::Nothing,
    }
}

/// The outermost layer: a request for a block's origin never reaches
/// control's own routes (its page, cookies, sessions, API).
pub async fn split(axum::extract::State(app): axum::extract::State<Arc<App>>, req: Request, next: Next) -> Response {
    let Some(domain) = app.cfg.block_domain.as_deref() else { return next.run(req).await };
    let mut res = match host(domain, req.headers()) {
        Host::Control => return next.run(req).await,
        Host::Nothing => return plain(StatusCode::NOT_FOUND, "no such block"),
        // Never past here: none of control's routes answer on this host.
        Host::Docs => {
            let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
            crate::doc_links::serve(&app, req.method(), req.headers(), peer, req.uri().path(), req.uri().query())
        }
        Host::Block(key) => serve(&app, key, req).await,
    };
    let h = res.headers_mut();
    h.remove(header::SET_COOKIE);
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::STRICT_TRANSPORT_SECURITY, HeaderValue::from_static("max-age=31536000"));
    res
}

async fn serve(app: &Arc<App>, key: String, req: Request) -> Response {
    let path = req.uri().path().to_owned();
    if let Some(name) = path.strip_prefix(PREFIX) {
        return match name {
            "relay" => relay(app.clone(), key, req).await,
            "boot.js" | "sw.js" | "shim.js" if matches!(*req.method(), Method::GET | Method::HEAD) => {
                let mut r = file(app, name);
                if name == "sw.js" {
                    // Its scope is the whole origin, from under /.arugula/.
                    r.headers_mut().insert("service-worker-allowed", HeaderValue::from_static("/"));
                }
                r
            }
            _ => plain(StatusCode::NOT_FOUND, "not found"),
        };
    }
    let h = |n: &str| req.headers().get(n).and_then(|v| v.to_str().ok()).unwrap_or("");
    if h("service-worker") == "script" {
        // The block's app registering a worker of its own: control doesn't
        // have its code, so it serves its own worker, which fetches the
        // app's through the channel and runs it inside (web/src/shell/block/sw.ts).
        let pq = req.uri().path_and_query().map_or("/", |p| p.as_str());
        let body = format!(
            "self.ARUGULA_APP_SW={};importScripts(\"{PREFIX}sw.js\");\n",
            serde_json::to_string(pq).unwrap_or_default()
        );
        return js(body.into_bytes());
    }
    if h("sec-fetch-mode") == "navigate" {
        return boot(app);
    }
    // Content control doesn't have: the worker wasn't in the way.
    plain(StatusCode::SERVICE_UNAVAILABLE, "this request didn't come through the block's worker")
}

fn plain(status: StatusCode, msg: &'static str) -> Response {
    let mut r = (status, msg).into_response();
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

fn js(body: Vec<u8>) -> Response {
    let mut r = Response::new(Body::from(body));
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(JS));
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    r
}

/// One of `web/dist/block/`'s files. No CSP on the worker: hosting an
/// app's own worker (above) runs its code with `new Function`.
fn file(app: &App, name: &str) -> Response {
    match crate::static_file(app, &format!("block/{name}")) {
        Some((data, _)) => js(data),
        None => plain(StatusCode::NOT_FOUND, "the block files aren't built: run `just web`"),
    }
}

/// The bootstrap page's only style, allowed by its hash.
const BOOT_STYLE: &str = "body{font:13px system-ui,sans-serif;color:#888;margin:16px}";
static BOOT_STYLE_HASH: LazyLock<String> = LazyLock::new(|| {
    format!("'sha256-{}'", base64::engine::general_purpose::STANDARD.encode(Sha256::digest(BOOT_STYLE)))
});

/// The bootstrap page's CSP: its own script and style only, and framed only
/// by control's own origins.
pub fn boot_csp(app: &App) -> String {
    let ancestors: Vec<&str> = app.cfg.sites.iter().map(|s| s.origin.as_str()).collect();
    format!(
        "default-src 'none'; script-src 'self'; style-src {}; connect-src 'self'; worker-src 'self'; \
         base-uri 'none'; form-action 'none'; frame-ancestors {}",
        *BOOT_STYLE_HASH,
        ancestors.join(" ")
    )
}

/// The bootstrap page: control's code only. `boot.js` registers the worker,
/// asks the parent (one of the origins in `arugula-control`) for the
/// block's credentials, hands them to the worker and loads the page again,
/// now through the worker (`web/src/shell/block/boot.ts`).
fn boot(app: &App) -> Response {
    let origins: Vec<&str> = app.cfg.sites.iter().map(|s| s.origin.as_str()).collect();
    let page = format!(
        "<!doctype html><html lang=en><meta charset=utf-8><meta name=viewport content=\"width=device-width\">\
         <meta name=arugula-control content=\"{}\"><title>Loading block</title><style>{BOOT_STYLE}</style>\
         <p id=arugula-block-state>Connecting\u{2026}</p><script src=\"{PREFIX}boot.js\"></script>",
        // Origins are URLs' serializations: no quotes or angle brackets.
        origins.join(" ")
    );
    let mut r = Response::new(Body::from(page));
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(HTML));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(v) = HeaderValue::from_str(&boot_csp(app)) {
        h.insert(header::CONTENT_SECURITY_POLICY, v);
    }
    r
}

// ---- the relay

/// Block sockets open, per daemon (`d:<id>`) and per block origin
/// (`o:<key>`), and when they close for want of traffic.
pub struct Open {
    seats: Mutex<HashMap<String, usize>>,
    pub deadlines: Deadlines,
}

impl Default for Open {
    fn default() -> Self {
        Self { seats: Mutex::default(), deadlines: Deadlines { first: FIRST_WITHIN, idle: IDLE } }
    }
}

impl Open {
    pub fn count(&self, key: &str) -> usize {
        self.seats.lock().unwrap().get(key).copied().unwrap_or(0)
    }
}

/// One block socket's place in [`Open`], given back when dropped.
pub struct Seat {
    app: Arc<App>,
    keys: [String; 2],
}

fn take_seat(app: &Arc<App>, daemon: &str, origin: &str) -> Option<Seat> {
    let keys = [format!("d:{daemon}"), format!("o:{origin}")];
    let mut m = app.blocks.seats.lock().unwrap();
    if m.get(&keys[0]).copied().unwrap_or(0) >= PER_DAEMON || m.get(&keys[1]).copied().unwrap_or(0) >= PER_ORIGIN {
        return None;
    }
    for k in &keys {
        *m.entry(k.clone()).or_default() += 1;
    }
    Some(Seat { app: app.clone(), keys })
}

impl Drop for Seat {
    fn drop(&mut self) {
        let mut m = self.app.blocks.seats.lock().unwrap();
        for k in &self.keys {
            if let Some(n) = m.get_mut(k) {
                *n -= 1;
                if *n == 0 {
                    m.remove(k);
                }
            }
        }
    }
}

/// Whether the socket's `Origin` is this block's own: a browser's socket
/// from another site (another block among them) is refused.
fn same_origin(headers: &HeaderMap) -> bool {
    let (Some(origin), Some(host)) = (
        headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()),
        headers.get(header::HOST).and_then(|v| v.to_str().ok()),
    ) else {
        return false;
    };
    let Ok(u) = url::Url::parse(origin) else { return false };
    let authority = match (u.host_str(), u.port()) {
        (Some(h), Some(p)) => format!("{h}:{p}"),
        (Some(h), None) => h.to_owned(),
        _ => return false,
    };
    authority.eq_ignore_ascii_case(host)
}

async fn relay(app: Arc<App>, origin_key: String, req: Request) -> Response {
    let (mut parts, _) = req.into_parts();
    let daemon = parts
        .uri
        .query()
        .and_then(|q| url::form_urlencoded::parse(q.as_bytes()).find(|(k, _)| k == "d").map(|(_, v)| v.into_owned()))
        .filter(|d| !d.is_empty() && d.len() <= 128);
    let Some(daemon) = daemon else { return plain(StatusCode::BAD_REQUEST, "which machine? (?d=<its id>)") };
    if !same_origin(&parts.headers) {
        return plain(StatusCode::FORBIDDEN, "only the block's own pages use its relay");
    }
    let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    if let Some(peer) = peer
        && let Err(e) = app.limits.check(crate::limit::BLOCK_RELAYS, app.limits.client_ip(peer, &parts.headers))
    {
        return e.into_response();
    }
    let up = match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(up) => up,
        Err(e) => return e.into_response(),
    };
    // Unknown and not connected look the same (a hosted sandbox, reached
    // through its provider rather than a relay socket, is neither here).
    let owner = app.db.daemon_account(&daemon).ok().flatten();
    let mux = owner.as_ref().and_then(|_| app.relay.daemon_mux(&daemon));
    let (Some(owner), Some(mux)) = (owner, mux) else {
        return plain(StatusCode::NOT_FOUND, UNREACHABLE);
    };
    let Some(seat) = take_seat(&app, &daemon, &origin_key) else {
        return plain(StatusCode::TOO_MANY_REQUESTS, "too many connections to this block or machine at once");
    };
    let ticket = match app.relay.admit_block(&app.db, &owner, app.cfg.billing) {
        Ok(t) => t,
        Err(crate::relay::Refused::Full) => {
            // A browser can't read a refused upgrade's status: it gets the
            // socket and at once a close that says why, as on /api/relay/c.
            return up.on_upgrade(|mut ws| async move {
                let close = CloseFrame { code: 1013, reason: crate::relay::FULL.into() };
                let _ = ws.send(Message::Close(Some(close))).await;
            });
        }
        Err(crate::relay::Refused::Account(why)) => {
            warn!(%why, "a block socket refused for its account");
            return plain(StatusCode::TOO_MANY_REQUESTS, "too many connections through the relay");
        }
    };
    let Ok(stream) = mux.open() else { return plain(StatusCode::NOT_FOUND, UNREACHABLE) };
    let slow = crate::plans::relay_standing(&app, &owner).await.is_ok_and(|(_, _, slow)| slow);
    let deadlines = app.blocks.deadlines;
    up.max_message_size(MAX_WIRE).read_buffer_size(CLIENT_READ_BUFFER).on_upgrade(move |ws| async move {
        let _seat = seat;
        let done = tokio::select! {
            n = splice(ws, stream, slow, &ticket, deadlines) => Some(n),
            _ = ticket.gone() => None,
        };
        let Some((up, down)) = done else { return };
        if let Err(e) = app.db.add_relay_bytes(&owner, &crate::day(now_ms()), up + down) {
            warn!(error = %e, "metering");
        }
    })
}

/// When a block socket closes for want of traffic.
#[derive(Clone, Copy)]
pub struct Deadlines {
    /// The browser's first message (Noise message 1) by then.
    pub first: Duration,
    /// Nothing either way for this long.
    pub idle: Duration,
}

/// `/api/relay/c`'s splice (WebSocket messages to `len ‖ bytes` frames on
/// the stream and back), with [`Deadlines`]: the socket closes with 1000
/// and why. Returns the bytes moved each way.
async fn splice(ws: WebSocket, stream: DuplexStream, slow: bool, ticket: &Ticket, d: Deadlines) -> (u64, u64) {
    let (mut wtx, mut wrx) = ws.split();
    let (mut rd, mut wr) = tokio::io::split(stream);
    let started = Instant::now();
    // Ms since `started` of the last message either way, and whether the
    // browser has sent one yet.
    let last = AtomicU64::new(0);
    let heard = std::sync::atomic::AtomicBool::new(false);
    let touch = || last.store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
    let (stop_tx, stop) = tokio::sync::watch::channel(false);
    let up = async {
        let mut n = 0u64;
        let mut stop = stop.clone();
        loop {
            let m = tokio::select! {
                m = wrx.next() => m,
                _ = stop.wait_for(|s| *s) => break,
            };
            let Some(Ok(m)) = m else { break };
            match m {
                Message::Binary(b) => {
                    touch();
                    heard.store(true, Ordering::Relaxed);
                    n += b.len() as u64;
                    if ticket.count(b.len() as u64) {
                        tokio::time::sleep(crate::relay::throttle(b.len())).await;
                    }
                    let mut f = Vec::with_capacity(4 + b.len());
                    f.extend_from_slice(&(b.len() as u32).to_be_bytes());
                    f.extend_from_slice(&b);
                    if wr.write_all(&f).await.is_err() {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        let _ = wr.shutdown().await;
        n
    };
    let (frames_tx, mut frames) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    let reader = tokio::spawn(async move {
        let mut len = [0u8; 4];
        while rd.read_exact(&mut len).await.is_ok() {
            let l = u32::from_be_bytes(len) as usize;
            if l > MAX_WIRE {
                break;
            }
            let mut b = vec![0u8; l];
            if rd.read_exact(&mut b).await.is_err() || frames_tx.send(b).await.is_err() {
                break;
            }
        }
    });
    let down = async {
        let mut n = 0u64;
        let mut ping = tokio::time::interval(CLIENT_PING);
        ping.tick().await;
        let mut check =
            tokio::time::interval(Duration::from_millis(250).min(d.first / 4).max(Duration::from_millis(10)));
        let mut why = "";
        loop {
            tokio::select! {
                f = frames.recv() => {
                    let Some(b) = f else { break };
                    touch();
                    n += b.len() as u64;
                    if ticket.count(b.len() as u64) || slow {
                        tokio::time::sleep(crate::relay::throttle(b.len())).await;
                    }
                    if wtx.send(Message::Binary(b.into())).await.is_err() {
                        break;
                    }
                }
                _ = ping.tick() => if wtx.send(Message::Ping(Default::default())).await.is_err() { break },
                _ = check.tick() => {
                    let now = started.elapsed();
                    if !heard.load(Ordering::Relaxed) && now >= d.first {
                        why = "no handshake";
                        break;
                    }
                    if now.saturating_sub(Duration::from_millis(last.load(Ordering::Relaxed))) >= d.idle {
                        why = "idle";
                        break;
                    }
                }
            }
        }
        let close = (!why.is_empty()).then(|| CloseFrame { code: 1000, reason: why.into() });
        let _ = wtx.send(Message::Close(close)).await;
        let _ = stop_tx.send(true);
        n
    };
    let r = tokio::join!(up, down);
    reader.abort();
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(host: &str) -> HeaderMap {
        let mut m = HeaderMap::new();
        m.insert(header::HOST, host.parse().unwrap());
        m
    }

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn hosts() {
        let d = "arugula.run";
        let b = format!("b-{KEY}.arugula.run");
        let block = || Host::Block(KEY.into());
        assert_eq!(host(d, &h(&b)), block());
        assert_eq!(host(d, &h(&format!("{b}:443"))), block());
        assert_eq!(host(d, &h(&format!("{}.", b.to_uppercase()))), block(), "names are case-blind");
        for nothing in [
            "arugula.run".to_owned(),
            "www.arugula.run".into(),
            format!("x-{KEY}.arugula.run"),
            "b-0123.arugula.run".into(),
            format!("b-{KEY}z.arugula.run"),
            format!("a.b-{KEY}.arugula.run"),
        ] {
            assert_eq!(host(d, &h(&nothing)), Host::Nothing, "{nothing}");
        }
        for control in ["control.arugula.io", "notarugula.run", &format!("b-{KEY}.arugula.run.evil.example")] {
            assert_eq!(host(d, &h(control)), Host::Control, "{control}");
        }
    }

    #[test]
    fn a_block_domain_is_a_site_of_its_own() {
        let sites = [crate::Site::new("https://control.arugula.io").unwrap()];
        assert_eq!(domain(" Arugula.Run. ", &sites).unwrap().as_deref(), Some("arugula.run"));
        assert_eq!(domain("", &sites).unwrap(), None);
        for bad in ["arugula.io", "control.arugula.io", "blocks.control.arugula.io", "localhost", "https://x.y"] {
            assert!(domain(bad, &sites).is_err(), "{bad}");
        }
    }
}
