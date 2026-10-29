//! Public previews (M187, aru-097): a port of a hosted workspace that a
//! project's owner or editor made public, open to anyone at
//! `https://<id>.<preview domain>` (`ARUGULA_PREVIEW_DOMAIN`,
//! `arugula.site` for the hosted one). The id is the platform's
//! (`arugula_platform::hosted::previews`): 32 random hex characters.
//!
//! **Why a domain of its own.** A stranger's page must never share a
//! registrable domain with control's cookies (`arugula.io`) or with block
//! origins (`arugula.run`), so the preview domain is neither, nor a parent
//! or child of either ([`domain`]). On these hosts control serves nothing
//! of its own: no page, cookie, session or API.
//!
//! **Control terminates them.** A signed-out stranger has no device key to
//! sign a block grant with, so control opens the channel itself: for each
//! request it asks the platform which machine and port the id is (cached
//! for [`FOR`]), and opens a block channel to that machine over its relay
//! socket with a grant signed by control's hosted key that names the port
//! and the id (`arugula_e2e::preview`). The machine serves it only while
//! the platform, asked through control, still says that port is public
//! under that id. So control sees a public page's traffic in plain text,
//! which is fine because it's public; members' previews stay end to end
//! on block origins (`blocks.rs`).
//!
//! - **Turning it off** takes it down within a minute: control's cache
//!   ([`FOR`]) and the machine's (its `PUBLIC_FOR`) both pass in well
//!   under one, and an open WebSocket is closed by the machine.
//! - **A paused box isn't woken** for a stranger: the page says it's
//!   asleep. A member opening the workspace wakes it (M182).
//! - **What crosses.** The request's own headers, less hop-by-hop ones and
//!   what says who or where the visitor is (`Fly-*`, `X-Forwarded-*`,
//!   `Forwarded`, `Via`); its cookies are the preview's own (nothing of
//!   control's is ever sent to this domain). An answer's `Set-Cookie`
//!   loses any `Domain`, so one preview can't set cookies for another.
//! - **Limits.** Per client address an hour ([`crate::limit::PUBLIC_PREVIEWS`]),
//!   [`PER_PREVIEW`] requests at once per preview, and traffic metered
//!   against the project, as block sockets are.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use arugula_e2e::{
    DeviceKeys,
    block::{
        Answer, MAX_LIFE_MS,
        wire::{self, ReqHead, ResHead, WsClose, WsOpen},
    },
    channel::{Channel, Initiator, MAX_WIRE, Msg, prologue},
    now_ms,
    preview::{PublicGrant, PublicHello},
};
use arugula_platform_api::{PlatformPort as _, PublicPreview};
use axum::{
    body::{Body, Bytes},
    extract::{
        ConnectInfo, FromRequestParts, Request,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf};
use tracing::{info, warn};

use crate::App;

/// How long control trusts the platform's answer for an id.
pub const FOR: Duration = Duration::from_secs(15);
/// Requests (and WebSockets) at once to one preview.
pub const PER_PREVIEW: usize = 64;
/// The largest request body a public preview takes.
const MAX_BODY: usize = 16 << 20;
/// How long the machine has to answer the channel's message 1.
const ANSWER_WITHIN: Duration = Duration::from_secs(10);
/// A grant's life: short, and well inside [`MAX_LIFE_MS`] either side of
/// the machine's clock.
const GRANT_MS: u64 = MAX_LIFE_MS / 2;

/// A preview domain: `ARUGULA_PREVIEW_DOMAIN` as the platform reads it,
/// and never control's own site, the block domain or the platform page's
/// host, nor a parent or child of one.
pub fn domain(
    raw: &str,
    sites: &[crate::Site],
    block_domain: Option<&str>,
    platform_host: Option<&str>,
) -> anyhow::Result<Option<String>> {
    let Some(d) = arugula_platform_api::preview_domain(raw)? else { return Ok(None) };
    let related = |h: &str| h == d || h.ends_with(&format!(".{d}")) || d.ends_with(&format!(".{h}"));
    for h in sites.iter().map(|s| s.host().to_ascii_lowercase()).chain(block_domain.map(str::to_owned)) {
        if related(&h) {
            anyhow::bail!(
                "ARUGULA_PREVIEW_DOMAIN {d} shares a site with {h}: public previews need a registrable domain of their own"
            );
        }
    }
    if let Some(h) = platform_host
        && related(h)
    {
        anyhow::bail!("ARUGULA_PREVIEW_DOMAIN {d} shares a site with the platform page's {h}");
    }
    Ok(Some(d))
}

/// What a request's Host makes it, with `domain` the preview domain.
#[derive(Debug, PartialEq, Eq)]
pub enum Host {
    /// `<id>.<domain>`.
    Preview(String),
    /// Under the domain, but no preview's.
    Nothing,
    /// Anything else.
    Other,
}

pub fn host(domain: &str, headers: &HeaderMap) -> Host {
    let Some(h) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else { return Host::Other };
    let name = h.rsplit_once(':').filter(|(_, p)| p.parse::<u16>().is_ok()).map_or(h, |(n, _)| n);
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    if name == domain {
        return Host::Nothing;
    }
    match name.strip_suffix(domain).and_then(|l| l.strip_suffix('.')) {
        None => Host::Other,
        Some(l) if l.len() == 32 && l.bytes().all(|b| b.is_ascii_hexdigit()) => Host::Preview(l.to_owned()),
        Some(_) => Host::Nothing,
    }
}

/// What control holds for public previews: the platform's recent answers
/// and the requests open to each.
#[derive(Default)]
pub struct Previews {
    known: Mutex<HashMap<String, (Instant, Option<PublicPreview>)>>,
    open: Mutex<HashMap<String, usize>>,
}

impl Previews {
    /// Forget an id at once (tests; the operator's takedown is the
    /// platform's and reaches here within [`FOR`]).
    pub fn forget(&self, id: &str) {
        self.known.lock().unwrap().remove(id);
    }
}

/// One request's place under [`PER_PREVIEW`], given back when dropped.
struct Seat {
    app: Arc<App>,
    id: String,
}

impl Seat {
    fn take(app: &Arc<App>, id: &str) -> Option<Self> {
        let mut m = app.previews.open.lock().unwrap();
        let n = m.entry(id.to_owned()).or_default();
        if *n >= PER_PREVIEW {
            return None;
        }
        *n += 1;
        Some(Self { app: app.clone(), id: id.to_owned() })
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        let mut m = self.app.previews.open.lock().unwrap();
        if let Some(n) = m.get_mut(&self.id) {
            *n -= 1;
            if *n == 0 {
                m.remove(&self.id);
            }
        }
    }
}

async fn lookup(app: &App, id: &str) -> anyhow::Result<Option<PublicPreview>> {
    if let Some((at, p)) = app.previews.known.lock().unwrap().get(id)
        && at.elapsed() < FOR
    {
        return Ok(p.clone());
    }
    let p = app.platform.public_preview(id).await?;
    let mut known = app.previews.known.lock().unwrap();
    if known.len() > 10_000 {
        known.retain(|_, (at, _)| at.elapsed() < FOR);
    }
    known.insert(id.to_owned(), (Instant::now(), p.clone()));
    Ok(p)
}

/// The outermost layer: a request for the preview domain never reaches
/// anything of control's.
pub async fn split(axum::extract::State(app): axum::extract::State<Arc<App>>, req: Request, next: Next) -> Response {
    let Some(domain) = app.cfg.preview_domain.as_deref() else { return next.run(req).await };
    match host(domain, req.headers()) {
        Host::Other => next.run(req).await,
        Host::Nothing => page(StatusCode::NOT_FOUND, NOT_PUBLIC),
        Host::Preview(id) => serve(&app, id, req).await,
    }
}

const NOT_PUBLIC: (&str, &str) =
    ("No preview here", "Nothing is public at this address. It may have been turned off, or expired.");
const ASLEEP: (&str, &str) = (
    "This preview is asleep",
    "Its workspace is paused. It wakes when someone on its project opens it; try again later.",
);
const UNAVAILABLE: (&str, &str) = ("This preview isn't answering", "Try again in a moment.");
const BUSY: (&str, &str) = ("This preview is busy", "Too many requests at once. Try again in a moment.");

/// Control's own answer on a preview host: a short page of its own, under
/// a CSP that runs nothing.
fn page(status: StatusCode, (title, text): (&str, &str)) -> Response {
    let body = format!(
        "<!doctype html><html lang=en><meta charset=utf-8><meta name=viewport content=\"width=device-width\">\
         <title>{title}</title><body style=\"font:15px system-ui,sans-serif;margin:40px auto;max-width:36em;\
         padding:0 16px;color:#333\"><h1 style=\"font-size:20px\">{title}</h1><p>{text}</p>"
    );
    let mut r = (status, body).into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'"),
    );
    h.insert("x-robots-tag", HeaderValue::from_static("noindex"));
    if status == StatusCode::SERVICE_UNAVAILABLE {
        h.insert(header::RETRY_AFTER, HeaderValue::from_static("30"));
    }
    r
}

async fn serve(app: &Arc<App>, id: String, req: Request) -> Response {
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    if let Some(peer) = peer
        && let Err(e) = app.limits.check(crate::limit::PUBLIC_PREVIEWS, app.limits.client_ip(peer, req.headers()))
    {
        return e.into_response();
    }
    let p = match lookup(app, &id).await {
        Ok(Some(p)) => p,
        Ok(None) => return page(StatusCode::NOT_FOUND, NOT_PUBLIC),
        Err(e) => {
            warn!(error = %e, "asking the platform for a public preview");
            return page(StatusCode::SERVICE_UNAVAILABLE, UNAVAILABLE);
        }
    };
    // The machine its workspace enrolled as, as control knows it.
    let row = p.machine.as_deref().and_then(|m| app.db.hosted(m).ok().flatten());
    let Some(row) = row.filter(|r| r.cert.project == p.project && r.cert.workspace == p.workspace) else {
        return page(StatusCode::SERVICE_UNAVAILABLE, ASLEEP);
    };
    let machine = row.cert.device.clone();
    // Never woken for a stranger: a paused box's socket looks open, so ask.
    if p.state == "paused"
        || app.relay.answers(&machine, Duration::from_secs(5), Duration::from_millis(300)).await != Some(true)
    {
        return page(StatusCode::SERVICE_UNAVAILABLE, ASLEEP);
    }
    let Some(seat) = Seat::take(app, &id) else { return page(StatusCode::TOO_MANY_REQUESTS, BUSY) };
    let to = To { machine, noise: row.cert.noise_key(), port: p.port, id, project: p.project };
    let (mut parts, body) = req.into_parts();
    let upgrade = parts.headers.contains_key(header::UPGRADE);
    if upgrade {
        let protocols = protocols(&parts.headers);
        let path = parts.uri.path_and_query().map_or("/", |p| p.as_str()).to_owned();
        let Ok(up) = WebSocketUpgrade::from_request_parts(&mut parts, &()).await else {
            return page(StatusCode::BAD_REQUEST, UNAVAILABLE);
        };
        return websocket(app.clone(), to, seat, up, WsOpen { path, protocols }).await;
    }
    let head = ReqHead {
        method: parts.method.as_str().to_owned(),
        path: parts.uri.path_and_query().map_or("/", |p| p.as_str()).to_owned(),
        headers: request_headers(&parts.headers),
    };
    let Ok(body) = axum::body::to_bytes(body, MAX_BODY).await else {
        return (StatusCode::PAYLOAD_TOO_LARGE, "request body too large").into_response();
    };
    http(app.clone(), to, seat, head, body).await
}

/// Where a request goes: the machine, its Noise key, the port and the
/// preview, and the project its traffic is metered to.
struct To {
    machine: String,
    noise: [u8; 32],
    port: u16,
    id: String,
    project: String,
}

/// Why a channel didn't open: each is a page of control's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fail {
    NotPublic,
    Asleep,
    Busy,
    Unavailable,
}

impl Fail {
    fn page(self) -> Response {
        match self {
            Fail::NotPublic => page(StatusCode::NOT_FOUND, NOT_PUBLIC),
            Fail::Asleep => page(StatusCode::SERVICE_UNAVAILABLE, ASLEEP),
            Fail::Busy => page(StatusCode::TOO_MANY_REQUESTS, BUSY),
            Fail::Unavailable => page(StatusCode::BAD_GATEWAY, UNAVAILABLE),
        }
    }
}

/// A stream of the machine's relay socket: length-prefixed messages,
/// counted against the project.
struct Pipe {
    rd: ReadHalf<DuplexStream>,
    wr: WriteHalf<DuplexStream>,
    ticket: crate::relay::Ticket,
    bytes: u64,
}

impl Pipe {
    async fn write(&mut self, m: &[u8]) -> std::io::Result<()> {
        self.bytes += m.len() as u64;
        if self.ticket.count(m.len() as u64) {
            tokio::time::sleep(crate::relay::throttle(m.len())).await;
        }
        let mut f = Vec::with_capacity(4 + m.len());
        f.extend_from_slice(&(m.len() as u32).to_be_bytes());
        f.extend_from_slice(m);
        self.wr.write_all(&f).await
    }

    async fn read(&mut self) -> std::io::Result<Vec<u8>> {
        let mut len = [0u8; 4];
        self.rd.read_exact(&mut len).await?;
        let n = u32::from_be_bytes(len) as usize;
        if n > MAX_WIRE {
            return Err(std::io::Error::other("message too large"));
        }
        let mut b = vec![0u8; n];
        self.rd.read_exact(&mut b).await?;
        self.bytes += n as u64;
        self.ticket.count(n as u64);
        Ok(b)
    }
}

/// A channel to the machine, opened with a public grant.
struct Open {
    pipe: Pipe,
    ch: Channel,
}

async fn open(app: &App, to: &To) -> Result<Open, Fail> {
    let mux = app.relay.daemon_mux(&to.machine).ok_or(Fail::Asleep)?;
    let ticket = app.relay.admit_block(&app.db, &to.project, app.cfg.billing).map_err(|_| Fail::Busy)?;
    let (rd, wr) = tokio::io::split(mux.open().map_err(|_| Fail::Asleep)?);
    let mut pipe = Pipe { rd, wr, ticket, bytes: 0 };
    let key = DeviceKeys::generate();
    let grant = PublicGrant::new(&app.hosted_key, &to.machine, to.port, &to.id, &key.noise_public, now_ms() + GRANT_MS);
    let hello = PublicHello { public: grant }.encode();
    let (init, m1) =
        Initiator::start_with(&key, &to.noise, &prologue(&to.machine), &hello).map_err(|_| Fail::Unavailable)?;
    pipe.write(&m1).await.map_err(|_| Fail::Unavailable)?;
    let m2 = tokio::time::timeout(ANSWER_WITHIN, pipe.read())
        .await
        .map_err(|_| Fail::Unavailable)?
        .map_err(|_| Fail::Unavailable)?;
    let (answer, ch) = init.finish(&m2).map_err(|_| Fail::Unavailable)?;
    if let Err(why) = Answer::decode(&answer) {
        info!(preview = to.id, code = why.code(), "the machine refused a public preview");
        return Err(Fail::NotPublic);
    }
    Ok(Open { pipe, ch })
}

impl Open {
    async fn send(&mut self, frame: Vec<u8>) -> anyhow::Result<()> {
        for w in self.ch.seal(&Msg::Binary(frame))? {
            self.pipe.write(&w).await?;
        }
        Ok(())
    }

    /// The next frame from the machine, or `None` once the channel closes.
    async fn frame(&mut self) -> Option<Vec<u8>> {
        loop {
            let w = self.pipe.read().await.ok()?;
            match self.ch.open(&w) {
                Ok(None) => continue,
                Ok(Some(Msg::Binary(f))) => return Some(f),
                _ => return None,
            }
        }
    }

    /// Meter what crossed against the project.
    fn done(self, app: &App, project: &str) {
        if let Err(e) = app.db.add_relay_bytes(project, &crate::day(now_ms()), self.pipe.bytes) {
            warn!(error = %e, "metering");
        }
    }
}

/// Hop-by-hop headers, and what says who or where the visitor is: never
/// sent on.
fn dropped(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-connection"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "forwarded"
            | "via"
            | "x-real-ip"
            | "true-client-ip"
    ) || name.starts_with("x-forwarded-")
        || name.starts_with("fly-")
        || name.starts_with("cf-")
        || name.starts_with("sec-websocket-")
}

fn request_headers(h: &HeaderMap) -> Vec<(String, String)> {
    h.iter()
        .filter(|(k, _)| !dropped(k.as_str()))
        .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
        .collect()
}

fn protocols(h: &HeaderMap) -> Vec<String> {
    h.get_all(header::SEC_WEBSOCKET_PROTOCOL)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
        .collect()
}

/// A `Set-Cookie` without its `Domain`: host-only, so a preview can't set a
/// cookie that every other preview gets.
fn host_only(cookie: &str) -> String {
    cookie
        .split(';')
        .filter(|a| !a.trim_start().to_ascii_lowercase().starts_with("domain="))
        .collect::<Vec<_>>()
        .join(";")
}

fn response_headers(rh: &ResHead, into: &mut HeaderMap) {
    for (k, v) in &rh.headers {
        let k = k.to_ascii_lowercase();
        if dropped(&k) {
            continue;
        }
        let v = if k == "set-cookie" { host_only(v) } else { v.clone() };
        if let (Ok(k), Ok(v)) = (HeaderName::try_from(k.as_str()), HeaderValue::from_str(&v)) {
            into.append(k, v);
        }
    }
    into.insert("x-robots-tag", HeaderValue::from_static("noindex"));
}

/// One HTTP request through a channel of its own; the answer streams back.
async fn http(app: Arc<App>, to: To, seat: Seat, head: ReqHead, body: Bytes) -> Response {
    let mut o = match open(&app, &to).await {
        Ok(o) => o,
        Err(why) => return why.page(),
    };
    let mut sent = o.send(wire::json_frame(wire::HEAD, 1, &head)).await;
    for chunk in body.chunks(256 << 10) {
        sent = sent.and(o.send(wire::frame(wire::DATA, 1, chunk)).await);
    }
    if sent.and(o.send(wire::frame(wire::END, 1, &[])).await).is_err() {
        return page(StatusCode::BAD_GATEWAY, UNAVAILABLE);
    }
    // The head first, then the body as it comes.
    let rh = loop {
        let Some(f) = o.frame().await else { return page(StatusCode::BAD_GATEWAY, UNAVAILABLE) };
        match wire::parse(&f) {
            Ok((wire::RESPONSE, 1, p)) => match serde_json::from_slice::<ResHead>(p) {
                Ok(rh) => break rh,
                Err(_) => return page(StatusCode::BAD_GATEWAY, UNAVAILABLE),
            },
            Ok((wire::FAILED, 1, _)) => return page(StatusCode::BAD_GATEWAY, UNAVAILABLE),
            _ => continue,
        }
    };
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(16);
    tokio::spawn(async move {
        let _seat = seat;
        while let Some(f) = o.frame().await {
            match wire::parse(&f) {
                Ok((wire::DATA, 1, p)) => {
                    if tx.send(Ok(Bytes::copy_from_slice(p))).await.is_err() {
                        break;
                    }
                }
                Ok((wire::END, 1, _)) => break,
                Ok((wire::FAILED, 1, _)) => {
                    let _ = tx.send(Err(std::io::Error::other("the preview failed"))).await;
                    break;
                }
                _ => {}
            }
        }
        o.done(&app, &to.project);
    });
    let stream = futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|v| (v, rx)) });
    let mut r = Response::new(Body::from_stream(stream));
    *r.status_mut() = StatusCode::from_u16(rh.status).unwrap_or(StatusCode::BAD_GATEWAY);
    response_headers(&rh, r.headers_mut());
    r
}

/// A WebSocket (hot reload, an app's own): opened on the machine first, so
/// a refusal is a status the browser sees, then spliced.
async fn websocket(app: Arc<App>, to: To, seat: Seat, up: WebSocketUpgrade, ws: WsOpen) -> Response {
    let mut o = match open(&app, &to).await {
        Ok(o) => o,
        Err(why) => return why.page(),
    };
    if o.send(wire::json_frame(wire::OPEN, 1, &ws)).await.is_err() {
        return page(StatusCode::BAD_GATEWAY, UNAVAILABLE);
    }
    let protocol = loop {
        let Some(f) = o.frame().await else { return page(StatusCode::BAD_GATEWAY, UNAVAILABLE) };
        match wire::parse(&f) {
            Ok((wire::ACCEPTED, 1, p)) => {
                let v: serde_json::Value = serde_json::from_slice(p).unwrap_or_default();
                break v["protocol"].as_str().unwrap_or_default().to_owned();
            }
            Ok((wire::FAILED | wire::CLOSE, 1, _)) => return page(StatusCode::BAD_GATEWAY, UNAVAILABLE),
            _ => continue,
        }
    };
    let up = if protocol.is_empty() { up } else { up.protocols([protocol]) };
    up.max_message_size(MAX_WIRE).on_upgrade(move |socket| async move {
        let _seat = seat;
        splice(socket, &mut o).await;
        o.done(&app, &to.project);
    })
}

async fn splice(socket: WebSocket, o: &mut Open) {
    let (mut tx, mut rx) = socket.split();
    loop {
        tokio::select! {
            m = rx.next() => {
                let frame = match m {
                    Some(Ok(Message::Text(t))) => wire::frame(wire::MESSAGE, 1, &[&[0u8][..], t.as_bytes()].concat()),
                    Some(Ok(Message::Binary(b))) => wire::frame(wire::MESSAGE, 1, &[&[1u8][..], &b[..]].concat()),
                    Some(Ok(Message::Close(c))) => {
                        let c = c.map(|c| WsClose { code: c.code, reason: c.reason.to_string() }).unwrap_or_default();
                        let _ = o.send(wire::json_frame(wire::CLOSE, 1, &c)).await;
                        return;
                    }
                    Some(Ok(_)) => continue,
                    _ => {
                        let _ = o.send(wire::json_frame(wire::CLOSE, 1, &WsClose { code: 1001, reason: String::new() })).await;
                        return;
                    }
                };
                if o.send(frame).await.is_err() {
                    return;
                }
            }
            f = o.frame() => {
                let Some(f) = f else {
                    let _ = tx.send(Message::Close(Some(CloseFrame { code: 1011, reason: "".into() }))).await;
                    return;
                };
                match wire::parse(&f) {
                    Ok((wire::MESSAGE, 1, p)) => {
                        let m = match p.split_first() {
                            Some((0, t)) => Message::Text(String::from_utf8_lossy(t).into_owned().into()),
                            Some((_, b)) => Message::Binary(b.to_vec().into()),
                            None => continue,
                        };
                        if tx.send(m).await.is_err() {
                            return;
                        }
                    }
                    Ok((wire::CLOSE | wire::FAILED, 1, p)) => {
                        let c: WsClose = serde_json::from_slice(p).unwrap_or_default();
                        let code = if c.code == 1000 || (3000..5000).contains(&c.code) { c.code } else { 1000 };
                        let _ = tx.send(Message::Close(Some(CloseFrame { code, reason: c.reason.into() }))).await;
                        return;
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(host: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, HeaderValue::from_str(host).unwrap());
        h
    }

    #[test]
    fn hosts() {
        let id = "0123456789abcdef0123456789abcdef";
        assert_eq!(host("arugula.site", &headers(&format!("{id}.arugula.site"))), Host::Preview(id.into()));
        assert_eq!(host("arugula.site", &headers(&format!("{id}.Arugula.Site:8443"))), Host::Preview(id.into()));
        assert_eq!(host("arugula.site", &headers("arugula.site")), Host::Nothing);
        assert_eq!(host("arugula.site", &headers("www.arugula.site")), Host::Nothing);
        assert_eq!(host("arugula.site", &headers(&format!("a.{id}.arugula.site"))), Host::Nothing);
        assert_eq!(host("arugula.site", &headers("control.arugula.io")), Host::Other);
        assert_eq!(host("arugula.site", &headers("notarugula.site")), Host::Other);
    }

    #[test]
    fn a_preview_domain_is_a_site_of_its_own() {
        let sites = [crate::Site::new("https://control.arugula.io").unwrap()];
        let d = |raw: &str, blocks: Option<&str>, platform: Option<&str>| domain(raw, &sites, blocks, platform);
        assert_eq!(
            d("arugula.site", Some("arugula.run"), Some("app.arugula.io")).unwrap().as_deref(),
            Some("arugula.site")
        );
        assert_eq!(d("", None, None).unwrap(), None);
        assert!(d("arugula.io", None, None).is_err(), "control's parent");
        assert!(d("p.control.arugula.io", None, None).is_err(), "under control");
        assert!(d("arugula.run", Some("arugula.run"), None).is_err(), "the block domain");
        assert!(d("p.arugula.run", Some("arugula.run"), None).is_err(), "under the block domain");
        assert!(d("p.app.example", None, Some("app.example")).is_err(), "under the platform page");
    }

    #[test]
    fn what_crosses() {
        let mut h = HeaderMap::new();
        for (k, v) in [
            ("cookie", "a=1"),
            ("accept", "text/html"),
            ("fly-client-ip", "1.2.3.4"),
            ("x-forwarded-for", "1.2.3.4"),
            ("via", "1.1 fly.io"),
            ("connection", "keep-alive"),
            ("host", "x.arugula.site"),
            ("sec-fetch-site", "same-origin"),
        ] {
            h.append(k, HeaderValue::from_static(v));
        }
        let got: Vec<String> = request_headers(&h).into_iter().map(|(k, _)| k).collect();
        assert_eq!(got, ["cookie", "accept", "sec-fetch-site"]);

        assert_eq!(host_only("s=1; Path=/; Domain=arugula.site; HttpOnly"), "s=1; Path=/; HttpOnly");
        assert_eq!(host_only("s=1;domain=.arugula.site"), "s=1");
        let mut out = HeaderMap::new();
        let rh = ResHead {
            status: 200,
            headers: vec![
                ("Set-Cookie".into(), "a=1; Domain=arugula.site".into()),
                ("Set-Cookie".into(), "b=2".into()),
                ("transfer-encoding".into(), "chunked".into()),
            ],
        };
        response_headers(&rh, &mut out);
        let cookies: Vec<_> = out.get_all(header::SET_COOKIE).iter().map(|v| v.to_str().unwrap()).collect();
        assert_eq!(cookies, ["a=1", "b=2"]);
        assert!(!out.contains_key(header::TRANSFER_ENCODING));
    }
}
