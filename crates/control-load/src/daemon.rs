//! A machine on the relay, as `arugulad` is to control: one WebSocket
//! dialed out to `/api/relay/dial`, signed with its key, carrying the mux;
//! each stream control opens is a page's channel to it. What it sends on a
//! channel is the size and pace of a pane's output (#443: a busy pane is
//! about 4,000 small messages a second), as opaque bytes: control can't
//! read Noise messages, so it relays these just the same. A message from a
//! page that starts with `P` is echoed at once, in line with the output, as
//! a keystroke's echo would be: the page times it.
//!
//! Every minute (and when nudged) it asks what a daemon's refresh asks:
//! `/control.json`, its account's certificates and its peers.

use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use arugula_control_wire as wire;
use arugula_e2e::{DeviceKeys, mux::Mux};
use futures_util::{SinkExt, StreamExt};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream},
    sync::mpsc,
};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};

use crate::{Ctx, rand_f64};

/// Output messages start with this; echoes with `P`.
pub const OUTPUT: u8 = b'O';
pub const PING: u8 = b'P';

pub struct Daemon {
    pub keys: DeviceKeys,
    pub account: String,
}

/// Dial in, serve, and dial again as `arugulad` does (1 s, doubling to a
/// minute; back to 1 s after half a minute up), until the process ends.
pub async fn run(ctx: Arc<Ctx>, d: Arc<Daemon>) {
    let mut backoff = Duration::from_secs(1);
    // Spread the first dials and refreshes out.
    tokio::time::sleep(Duration::from_millis((rand_f64() * 2000.0) as u64)).await;
    {
        let (ctx, d) = (ctx.clone(), d.clone());
        tokio::spawn(async move { refresh_loop(ctx, d).await });
    }
    loop {
        let t = Instant::now();
        match dial(&ctx, &d).await {
            Ok(ws) => {
                ctx.stats.time("daemon.dial", t);
                ctx.stats.daemons.fetch_add(1, Ordering::Relaxed);
                let up = Instant::now();
                serve(&ctx, ws).await;
                ctx.stats.daemons.fetch_sub(1, Ordering::Relaxed);
                ctx.stats.inc("daemon.dropped");
                if up.elapsed() > Duration::from_secs(30) {
                    backoff = Duration::from_secs(1);
                }
            }
            Err(why) => ctx.stats.inc(&format!("daemon.dial:{why}")),
        }
        let jitter = Duration::from_millis((rand_f64() * 1000.0) as u64);
        tokio::time::sleep(backoff + jitter).await;
        backoff = (backoff * 2).min(Duration::from_secs(60));
    }
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The relay socket, or why not (a status, or `error`).
async fn dial(ctx: &Ctx, d: &Daemon) -> Result<Ws, String> {
    let url = ctx.ws_at(wire::RELAY_DIAL);
    let mut req = url.as_str().into_client_request().map_err(|_| "request".to_owned())?;
    let auth = arugula_e2e::cert::request_auth(&d.keys, "GET", wire::RELAY_DIAL, b"");
    let h = req.headers_mut();
    h.insert("x-arugula-auth", HeaderValue::from_str(&auth).map_err(|_| "header".to_owned())?);
    h.insert(wire::FEATURES_HEADER, HeaderValue::from_static(wire::APP_DEVICES));
    match tokio_tungstenite::connect_async_tls_with_config(req, None, true, ctx.tls.clone()).await {
        Ok((ws, _)) => Ok(ws),
        Err(tokio_tungstenite::tungstenite::Error::Http(r)) => Err(r.status().as_u16().to_string()),
        Err(_) => Err("error".into()),
    }
}

/// Serve the mux until the socket ends.
async fn serve(ctx: &Arc<Ctx>, ws: Ws) {
    let (accept_tx, mut accept) = mpsc::unbounded_channel::<DuplexStream>();
    let (mux, mut out) = Mux::new(Some(accept_tx));
    let (mut tx, mut rx) = ws.split();
    loop {
        tokio::select! {
            f = out.recv() => match f {
                Some(f) => if tx.send(Message::Binary(f.into())).await.is_err() { break },
                None => break,
            },
            m = rx.next() => match m {
                Some(Ok(Message::Binary(b))) => if mux.handle(&b).is_err() { break },
                Some(Ok(Message::Text(t))) => if t.as_str() == "trust" { ctx.stats.inc("daemon.nudged") },
                Some(Ok(_)) => {}
                Some(Err(_)) | None => break,
            },
            Some(s) = accept.recv() => {
                let ctx = ctx.clone();
                tokio::spawn(async move { channel(ctx, s).await });
            }
        }
    }
    mux.close();
}

/// One page's channel: output at a busy or an idle pane's pace, and echoes.
async fn channel(ctx: Arc<Ctx>, s: DuplexStream) {
    let busy = rand_f64() < ctx.cfg.busy;
    let rate = if busy { ctx.cfg.busy_rate } else { ctx.cfg.idle_rate };
    let (mut rd, mut wr) = tokio::io::split(s);
    // What to write: echoes and output in the order they came, as a pane's
    // bytes are. Bounded, so a page that can't keep up slows the output,
    // as the daemon's own queue does.
    let (q, mut queued) = mpsc::channel::<Vec<u8>>(1024);
    let writer = tokio::spawn(async move {
        let mut buf = Vec::with_capacity(64 * 1024);
        while let Some(m) = queued.recv().await {
            buf.clear();
            fn push(buf: &mut Vec<u8>, m: &[u8]) {
                buf.extend_from_slice(&(m.len() as u32).to_be_bytes());
                buf.extend_from_slice(m);
            }
            push(&mut buf, &m);
            while buf.len() < 16 * 1024 {
                match queued.try_recv() {
                    Ok(m) => push(&mut buf, &m),
                    Err(_) => break,
                }
            }
            if wr.write_all(&buf).await.is_err() {
                break;
            }
        }
    });
    let echo = {
        let q = q.clone();
        async move {
            let mut len = [0u8; 4];
            while rd.read_exact(&mut len).await.is_ok() {
                let l = u32::from_be_bytes(len) as usize;
                let mut b = vec![0u8; l];
                if rd.read_exact(&mut b).await.is_err() {
                    break;
                }
                if b.first() == Some(&PING) && q.send(b).await.is_err() {
                    break;
                }
            }
        }
    };
    let output = {
        let ctx = ctx.clone();
        async move {
            if rate <= 0.0 {
                return std::future::pending().await;
            }
            // Ticks of 10 ms, each with its share of the rate (and the
            // fraction carried), so 4,000 a second is 40 a tick.
            let mut tick = tokio::time::interval(Duration::from_millis(10));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let per_tick = rate / 100.0;
            let mut owed = rand_f64();
            let size = ctx.cfg.msg_bytes.max(2);
            let mut body = vec![0u8; size];
            body[0] = OUTPUT;
            for (i, b) in body.iter_mut().enumerate().skip(1) {
                *b = (i * 131 % 251) as u8;
            }
            loop {
                tick.tick().await;
                owed += per_tick;
                while owed >= 1.0 {
                    owed -= 1.0;
                    if q.send(body.clone()).await.is_err() {
                        return;
                    }
                    ctx.stats.tx_msgs.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    };
    tokio::select! {
        _ = echo => {},
        _ = output => {},
    }
    writer.abort();
}

/// A daemon's refresh, every minute: where control is, the account's
/// certificates, and its peers.
async fn refresh_loop(ctx: Arc<Ctx>, d: Arc<Daemon>) {
    tokio::time::sleep(Duration::from_millis((rand_f64() * 60_000.0) as u64)).await;
    let mut every = tokio::time::interval(Duration::from_secs(60));
    loop {
        every.tick().await;
        let _ = crate::account::call(&ctx, "daemon.control_json", ctx.http.get(ctx.at(wire::CONTROL_JSON))).await;
        let trust = format!(
            "{}&version=0.69.11&os=linux&arch=x86_64",
            wire::team::Features::path("presigned-invites,owner-moves,join-secret,owner-passkeys")
        );
        let _ = signed_get(&ctx, &d, "daemon.trust", &trust).await;
        let peers = wire::team::PeersQuery::path(std::slice::from_ref(&d.account));
        let _ = signed_get(&ctx, &d, "daemon.peers", &peers).await;
    }
}

async fn signed_get(ctx: &Ctx, d: &Daemon, name: &str, path_and_query: &str) -> anyhow::Result<()> {
    let auth = arugula_e2e::cert::request_auth(&d.keys, "GET", path_and_query, b"");
    let b = ctx
        .http
        .get(ctx.at(path_and_query))
        .header("x-arugula-auth", auth)
        .header(wire::FEATURES_HEADER, wire::APP_DEVICES);
    crate::account::call(ctx, name, b).await.map(|_| ())
}
