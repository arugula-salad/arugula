//! Block channels (M50, #150): a block's page in control's page, with
//! nothing set up on this machine for it (no block listener, DNS name or
//! certificate).
//!
//! **The contract.** Control serves each block an origin of its own,
//! `https://b-<site key>.<block domain>` (the domain is control's to pick;
//! this daemon never names it). The site key is [`crate::sites::Site::site_key`],
//! in the block's state as `site_key`. A service worker on that origin
//! opens a Noise channel to this daemon, at `/e2e` directly or through
//! control's relay, exactly as a device's channel comes, but with a one-off
//! key and a grant (`arugula_e2e::block`) in message 1's payload:
//!
//! - **message 1**: `{"grant": …}`, a grant for this daemon, this block
//!   (its id, as a string), the handshake's static key, signed by one of
//!   the owner's devices, with its expiry by this daemon's clock, at most
//!   ten minutes ahead. Device channels learn that clock in their own
//!   message 2 (`arugula_e2e::channel::Welcome`).
//! - **message 2**: `{"ok":true}`, or `{"ok":false,"refused":"<code>",…}`
//!   and the channel closes.
//! - **then** the frames of `arugula_e2e::block::wire`: HTTP requests,
//!   answered by the block listener's own proxy (`sites::forward` with
//!   `Via::Channel`), and WebSockets to the block's port.
//!
//! A channel lasts until its socket closes, the block closes, or the
//! device that granted it stops being trusted. Its grant's expiry gates
//! new channels only.
//!
//! What the frame has to do for the proxy: send each request's path and
//! query and its headers (cookies from its own jar: a worker's requests
//! carry none), and say `sec-fetch-mode`/`sec-fetch-dest` from the
//! request's `mode` and `destination`, so navigations are reported and get
//! the head script. Answers come with `Content-Security-Policy:
//! frame-ancestors 'self' <control's origins>` added and redirects to the
//! port made paths; bodies come uncompressed.
//!
//! Every port and editor block has a site: on the daemon's own listener
//! (`--block-listen`, the dev or tailnet scheme), or, without one, a site
//! reached only from here (`sites::Scheme::Channel`). A block with no site
//! (a web page, a block that's gone) is refused as `no-block`.

use std::{collections::HashMap, sync::Arc};

use arugula_e2e::{
    block::{
        Answer, Expect, Grant, Hello, Refused,
        wire::{self, ReqHead, ResHead, WsClose, WsOpen},
    },
    channel::{Msg, Responder},
    now_ms,
};
use arugula_proto::PaneId;
use futures_util::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::{
    Request,
    body::Bytes,
    header::{self, HeaderName, HeaderValue},
};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};
use tracing::{debug, info};

use crate::{
    e2e::Out,
    server::App,
    sites::{self, Site, Sites, Upstream, Via},
};

/// Whether `grant` lets `remote` in, as this daemon (`id`) sees it now.
fn check(app: &App, id: &str, remote: &[u8; 32], payload: &[u8]) -> Result<(Grant, Arc<Site>), Refused> {
    let grant = Hello::decode(payload).map_err(|_| Refused::Malformed)?.grant;
    let site = || grant.block.parse::<PaneId>().ok().and_then(|b| sites::get()?.site(b));
    grant.check(
        &Expect { daemon: id, remote, now: now_ms() },
        |by| app.control.block_signer(by),
        |_| site().is_some(),
    )?;
    let site = site().ok_or(Refused::NoBlock)?;
    Ok((grant, site))
}

/// A channel whose message 1 carried a payload: a block's. Answers the
/// handshake, then serves the block until something ends it.
pub(crate) async fn serve(
    app: Arc<App>,
    id: &str,
    responder: Responder,
    remote: [u8; 32],
    payload: &[u8],
    mut inbound: mpsc::Receiver<Vec<u8>>,
    out: mpsc::Sender<Vec<u8>>,
) -> anyhow::Result<()> {
    let verdict = check(&app, id, &remote, payload);
    let answer = match &verdict {
        Ok(_) => Answer::ok(),
        Err(why) => Answer::refused(*why),
    };
    let (m2, ch) = responder.finish(&answer.encode())?;
    // Queued before the socket closes, so the refusal reaches the frame.
    out.send(m2).await?;
    let (grant, site) = match verdict {
        Ok(v) => v,
        Err(why) => {
            info!(code = why.code(), why = %why, "refused a block channel");
            return Ok(());
        }
    };
    let sites = sites::get().ok_or_else(|| anyhow::anyhow!("no block sites"))?;
    let out = Arc::new(Out::new(ch, out));
    let ch = out.ch.clone();
    info!(block = site.id, device = grant.by, "block channel open");
    let up = Arc::new(Upstream::default());
    let mut streams: HashMap<u32, Stream> = HashMap::new();
    let mut changed = app.control.changed.subscribe();
    let mut closings = sites.closings();
    let r = loop {
        let w = tokio::select! {
            w = inbound.recv() => w,
            Ok(()) = changed.changed() => {
                if app.control.block_signer(&grant.by).is_none() {
                    info!(block = site.id, device = grant.by, "device no longer trusted: closing its block channel");
                    break Ok(());
                }
                continue;
            }
            Ok(()) = closings.changed() => {
                if !sites.site(site.id).is_some_and(|s| Arc::ptr_eq(&s, &site)) {
                    info!(block = site.id, "block closed: closing its channel");
                    break Ok(());
                }
                continue;
            }
        };
        let Some(w) = w else { break Ok(()) };
        let f = match ch.open(&w) {
            Ok(None) => continue,
            Ok(Some(Msg::Binary(f))) => f,
            Ok(Some(_)) => break Err(anyhow::anyhow!("a block channel carries frames only")),
            Err(e) => break Err(e),
        };
        drop_finished(&mut streams);
        if let Err(e) = frame(&sites, &site, &up, &out, &mut streams, &f, Via::Channel) {
            break Err(e);
        }
    };
    abort(streams);
    info!(block = site.id, "block channel closed");
    r
}

/// Forget streams that have finished.
pub(crate) fn drop_finished(streams: &mut HashMap<u32, Stream>) {
    streams.retain(|_, s| !matches!(s, Stream::Running(h, _) if h.is_finished()));
}

/// Stop every stream still running: the channel closed.
pub(crate) fn abort(streams: HashMap<u32, Stream>) {
    for s in streams.into_values() {
        if let Stream::Running(h, _) = s {
            h.abort();
        }
    }
}

/// A stream the client opened.
pub(crate) enum Stream {
    /// A request whose body is still coming.
    Http(ReqHead, Vec<u8>),
    /// A request on its way, or a WebSocket (with what to send it).
    Running(tokio::task::JoinHandle<()>, Option<mpsc::UnboundedSender<WsCmd>>),
}

pub(crate) enum WsCmd {
    Msg(tungstenite::Message),
    Close(WsClose),
}

/// The largest request body a block channel takes (a fetch event's body
/// comes whole).
const MAX_BODY: usize = 64 << 20;

/// One frame from the client, for `site`'s port, its requests come `via`
/// a block's channel or a public preview's (M187, `public_preview.rs`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn frame(
    sites: &Arc<Sites>,
    site: &Arc<Site>,
    up: &Arc<Upstream>,
    out: &Arc<Out>,
    streams: &mut HashMap<u32, Stream>,
    f: &[u8],
    via: Via,
) -> anyhow::Result<()> {
    let (kind, sid, payload) = wire::parse(f)?;
    let failed = move |out: Arc<Out>, e: anyhow::Error| async move {
        let _ =
            out.put(&Msg::Binary(wire::json_frame(wire::FAILED, sid, &json!({ "message": format!("{e:#}") })))).await;
    };
    match kind {
        wire::HEAD => {
            streams.insert(sid, Stream::Http(serde_json::from_slice(payload)?, Vec::new()));
        }
        wire::DATA => {
            if let Some(Stream::Http(_, body)) = streams.get_mut(&sid) {
                anyhow::ensure!(body.len() + payload.len() <= MAX_BODY, "request body too large");
                body.extend_from_slice(payload);
            }
        }
        wire::END => {
            if let Some(Stream::Http(head, body)) = streams.remove(&sid) {
                let (sites, site, up, out) = (sites.clone(), site.clone(), up.clone(), out.clone());
                let h = tokio::spawn(async move {
                    if let Err(e) = request(&sites, &site, &up, &out, sid, head, body, via).await {
                        failed(out, e).await;
                    }
                });
                streams.insert(sid, Stream::Running(h, None));
            }
        }
        wire::OPEN => {
            let open: WsOpen = serde_json::from_slice(payload)?;
            let (tx, rx) = mpsc::unbounded_channel();
            let (site, out) = (site.clone(), out.clone());
            let h = tokio::spawn(async move {
                if let Err(e) = websocket(&site, &out, sid, open, rx).await {
                    failed(out, e).await;
                }
            });
            streams.insert(sid, Stream::Running(h, Some(tx)));
        }
        wire::MESSAGE => {
            if let Some(Stream::Running(_, Some(tx))) = streams.get(&sid)
                && let Some((&k, data)) = payload.split_first()
            {
                let m = if k == 0 {
                    tungstenite::Message::Text(String::from_utf8_lossy(data).into_owned().into())
                } else {
                    tungstenite::Message::Binary(data.to_vec().into())
                };
                let _ = tx.send(WsCmd::Msg(m));
            }
        }
        wire::CLOSE => {
            if let Some(Stream::Running(_, Some(tx))) = streams.get(&sid) {
                let _ = tx.send(WsCmd::Close(serde_json::from_slice(payload).unwrap_or_default()));
            }
        }
        wire::CANCEL => {
            if let Some(Stream::Running(h, _)) = streams.remove(&sid) {
                h.abort();
            }
        }
        k => anyhow::bail!("unknown frame kind {k}"),
    }
    Ok(())
}

/// One HTTP request, through the block listener's proxy; the answer streams
/// back as `R`, `D`s and `E`.
#[allow(clippy::too_many_arguments)]
async fn request(
    sites: &Sites,
    site: &Site,
    up: &Upstream,
    out: &Out,
    sid: u32,
    head: ReqHead,
    body: Vec<u8>,
    via: Via,
) -> anyhow::Result<()> {
    anyhow::ensure!(head.path.starts_with('/'), "a request's path starts with /");
    let mut req = Request::builder().method(head.method.as_str()).uri(head.path.as_str());
    let h = req.headers_mut().expect("builder");
    for (k, v) in &head.headers {
        if let (Ok(k), Ok(v)) = (HeaderName::try_from(k.as_str()), HeaderValue::from_str(v)) {
            h.append(k, v);
        }
    }
    let req = req.body(Full::new(Bytes::from(body)).map_err(|e| match e {}).boxed())?;
    let res = sites::forward(sites, site, up, req, via, None).await;
    let rh = ResHead {
        status: res.status().as_u16(),
        headers: res
            .headers()
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
            .collect(),
    };
    out.put(&Msg::Binary(wire::json_frame(wire::RESPONSE, sid, &rh))).await?;
    let mut body = res.into_body();
    while let Some(f) = body.frame().await {
        if let Ok(data) = f?.into_data()
            && !data.is_empty()
        {
            out.put(&Msg::Binary(wire::frame(wire::DATA, sid, &data))).await?;
        }
    }
    out.put(&Msg::Binary(wire::frame(wire::END, sid, &[]))).await?;
    Ok(())
}

/// One WebSocket to the block's port (hot reload, code-server), on a
/// connection of its own, as the listener's are.
async fn websocket(
    site: &Site,
    out: &Out,
    sid: u32,
    open: WsOpen,
    mut cmds: mpsc::UnboundedReceiver<WsCmd>,
) -> anyhow::Result<()> {
    anyhow::ensure!(open.path.starts_with('/'), "a WebSocket's path starts with /");
    let target = site.target().ok_or_else(|| anyhow::anyhow!("this block isn't showing a port"))?;
    if let Some(s) = sites::get() {
        s.allowed(&target).map_err(anyhow::Error::msg)?;
    }
    let local = target.authority();
    let mut req = format!("ws://{local}{}", open.path).into_client_request()?;
    let h = req.headers_mut();
    h.insert(header::ORIGIN, HeaderValue::from_str(&format!("http://{local}"))?);
    if !open.protocols.is_empty() {
        h.insert(header::SEC_WEBSOCKET_PROTOCOL, HeaderValue::from_str(&open.protocols.join(", "))?);
    }
    let io = target.dial().await.map_err(|e| anyhow::anyhow!("nothing is answering on {}: {e}", target.what()))?;
    let (ws, res) = tokio_tungstenite::client_async_with_config(req, io, None).await?;
    let protocol = res.headers().get(header::SEC_WEBSOCKET_PROTOCOL).and_then(|v| v.to_str().ok()).unwrap_or("");
    out.put(&Msg::Binary(wire::json_frame(wire::ACCEPTED, sid, &json!({ "protocol": protocol })))).await?;
    let (mut tx, mut rx) = ws.split();
    let send = |kind: u8, payload: Vec<u8>| Msg::Binary(wire::frame(kind, sid, &payload));
    loop {
        tokio::select! {
            m = rx.next() => {
                let close = |c: Option<tungstenite::protocol::CloseFrame>| WsClose {
                    code: c.as_ref().map(|c| u16::from(c.code)).unwrap_or(1005),
                    reason: c.map(|c| c.reason.to_string()).unwrap_or_default(),
                };
                match m {
                    Some(Ok(tungstenite::Message::Text(t))) => out.put(&send(wire::MESSAGE, [&[0u8][..], t.as_bytes()].concat())).await?,
                    Some(Ok(tungstenite::Message::Binary(b))) => out.put(&send(wire::MESSAGE, [&[1u8][..], &b[..]].concat())).await?,
                    Some(Ok(tungstenite::Message::Close(c))) => {
                        out.put(&Msg::Binary(wire::json_frame(wire::CLOSE, sid, &close(c)))).await?;
                        return Ok(());
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        debug!(block = site.id, error = %e, "block WebSocket");
                        out.put(&Msg::Binary(wire::json_frame(wire::CLOSE, sid, &close(None)))).await?;
                        return Ok(());
                    }
                    None => {
                        out.put(&Msg::Binary(wire::json_frame(wire::CLOSE, sid, &close(None)))).await?;
                        return Ok(());
                    }
                }
            }
            c = cmds.recv() => match c {
                Some(WsCmd::Msg(m)) => tx.send(m).await?,
                Some(WsCmd::Close(c)) => {
                    // Only codes a client may send; anything else is a plain close.
                    let code = if c.code == 1000 || (3000..5000).contains(&c.code) { c.code } else { 1000 };
                    let _ = tx.send(tungstenite::Message::Close(Some(tungstenite::protocol::CloseFrame {
                        code: code.into(),
                        reason: c.reason.into(),
                    }))).await;
                    return Ok(());
                }
                None => return Ok(()),
            }
        }
    }
}
