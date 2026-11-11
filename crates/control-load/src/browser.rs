//! A signed-in browser page, as control's page is to control: one socket
//! to `/api/relay/m` carrying a channel to each of the account's machines
//! (M25), the page's refresh every 10 s (devices and the directory), and
//! the platform page's calls every 30 s. It times:
//!
//! - `page.channel_open`: asking for a machine's channel until control says
//!   it's open (what opening a tab waits for, before any pane shows);
//! - `page.echo`: a message to the machine and its echo back, through
//!   whatever pane output is queued ahead of it (a keystroke's echo);
//! - each HTTP call by name.

use std::{
    collections::HashMap,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::{
    Message, client::IntoClientRequest, http::HeaderValue, protocol::frame::coding::CloseCode,
};

use crate::{Ctx, account::Account, daemon, rand_f64};

const M_OPEN: u8 = 1;
const M_DATA: u8 = 2;
const M_CLOSE: u8 = 3;
const M_OPENED: u8 = 4;

fn mframe(kind: u8, chan: u32, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(5 + payload.len());
    f.push(kind);
    f.extend_from_slice(&chan.to_be_bytes());
    f.extend_from_slice(payload);
    f
}

/// Keep a page open for `a`, on `machines`, until the process ends.
pub async fn run(ctx: Arc<Ctx>, a: Arc<Account>, machines: Vec<String>) {
    tokio::time::sleep(Duration::from_millis((rand_f64() * 5000.0) as u64)).await;
    {
        let (ctx, cookie) = (ctx.clone(), a.cookie());
        tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_secs(10));
            let mut n = 0u64;
            loop {
                every.tick().await;
                crate::account::page_refresh(&ctx, &cookie).await;
                if n.is_multiple_of(3) {
                    crate::account::platform_refresh(&ctx, &cookie).await;
                }
                n += 1;
            }
        });
    }
    let mut backoff = Duration::from_secs(1);
    loop {
        let t = Instant::now();
        match connect(&ctx, &a).await {
            Ok(ws) => {
                ctx.stats.time("page.connect", t);
                ctx.stats.browsers.fetch_add(1, Ordering::Relaxed);
                let why = page(&ctx, ws, &machines).await;
                ctx.stats.browsers.fetch_sub(1, Ordering::Relaxed);
                ctx.stats.inc(&format!("page.socket_ended:{why}"));
                backoff = Duration::from_secs(1);
            }
            Err(why) => ctx.stats.inc(&format!("page.connect:{why}")),
        }
        tokio::time::sleep(backoff + Duration::from_millis((rand_f64() * 1000.0) as u64)).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(ctx: &Ctx, a: &Account) -> Result<Ws, String> {
    let mut req = ctx.ws_at("/api/relay/m").as_str().into_client_request().map_err(|_| "request".to_owned())?;
    let h = req.headers_mut();
    h.insert("cookie", HeaderValue::from_str(&a.cookie()).map_err(|_| "header".to_owned())?);
    h.insert("origin", HeaderValue::from_str(&ctx.origin).map_err(|_| "header".to_owned())?);
    match tokio_tungstenite::connect_async_tls_with_config(req, None, true, ctx.tls.clone()).await {
        Ok((ws, _)) => Ok(ws),
        Err(tokio_tungstenite::tungstenite::Error::Http(r)) => Err(r.status().as_u16().to_string()),
        Err(_) => Err("error".into()),
    }
}

struct Chan {
    asked: Instant,
    open: bool,
    first: bool,
}

/// The page's socket until it ends; why it did.
async fn page(ctx: &Ctx, ws: Ws, machines: &[String]) -> String {
    let (mut tx, mut rx) = ws.split();
    let mut chans: HashMap<u32, Chan> = HashMap::new();
    let start = Instant::now();
    for (i, m) in machines.iter().enumerate() {
        let chan = i as u32 + 1;
        if tx.send(Message::Binary(mframe(M_OPEN, chan, m.as_bytes()).into())).await.is_err() {
            return "send".into();
        }
        chans.insert(chan, Chan { asked: Instant::now(), open: false, first: true });
    }
    let mut ping = tokio::time::interval(Duration::from_millis(ctx.cfg.echo_every_ms));
    let mut reopen = tokio::time::interval(Duration::from_secs(5));
    let why = 'page: loop {
        tokio::select! {
            m = rx.next() => match m {
                Some(Ok(Message::Binary(b))) if b.len() >= 5 => {
                    let chan = u32::from_be_bytes(b[1..5].try_into().unwrap());
                    let payload = &b[5..];
                    let Some(c) = chans.get_mut(&chan) else { continue };
                    match b[0] {
                        M_OPENED => {
                            ctx.stats.time("page.channel_open", c.asked);
                            c.open = true;
                            ctx.stats.channels.fetch_add(1, Ordering::Relaxed);
                        }
                        M_DATA => {
                            ctx.stats.rx_msgs.fetch_add(1, Ordering::Relaxed);
                            ctx.stats.rx_bytes.fetch_add(payload.len() as u64, Ordering::Relaxed);
                            if c.first {
                                c.first = false;
                                ctx.stats.time("page.first_message", c.asked);
                            }
                            if payload.first() == Some(&daemon::PING) && payload.len() >= 9 {
                                let sent = u64::from_be_bytes(payload[1..9].try_into().unwrap());
                                let now = start.elapsed().as_micros() as u64;
                                ctx.stats.sample("page.echo", now.saturating_sub(sent).min(u32::MAX as u64) as u32);
                            }
                        }
                        M_CLOSE => {
                            if c.open {
                                ctx.stats.channels.fetch_sub(1, Ordering::Relaxed);
                            }
                            let why = String::from_utf8_lossy(payload);
                            ctx.stats.inc(&format!("page.channel_closed:{}", if why.is_empty() { "ended" } else { &why }));
                            chans.remove(&chan);
                        }
                        _ => {}
                    }
                }
                Some(Ok(Message::Close(f))) => {
                    break match f {
                        Some(f) if f.code == CloseCode::Again => "full (1013)".to_owned(),
                        Some(f) => format!("closed {}", u16::from(f.code)),
                        None => "closed".to_owned(),
                    };
                }
                Some(Ok(_)) => {}
                Some(Err(_)) => break "error".into(),
                None => break "ended".into(),
            },
            _ = ping.tick() => {
                let now = start.elapsed().as_micros() as u64;
                let mut p = vec![daemon::PING];
                p.extend_from_slice(&now.to_be_bytes());
                for (chan, _) in chans.iter().filter(|(_, c)| c.open) {
                    if tx.send(Message::Binary(mframe(M_DATA, *chan, &p).into())).await.is_err() {
                        break 'page "send".into();
                    }
                }
            }
            _ = reopen.tick() => {
                // Channels that closed: asked for again, as a page does.
                for (i, m) in machines.iter().enumerate() {
                    let chan = i as u32 + 1;
                    if let std::collections::hash_map::Entry::Vacant(e) = chans.entry(chan) {
                        if tx.send(Message::Binary(mframe(M_OPEN, chan, m.as_bytes()).into())).await.is_err() {
                            break 'page "send".into();
                        }
                        e.insert(Chan { asked: Instant::now(), open: false, first: true });
                    }
                }
            }
        }
    };
    let open = chans.values().filter(|c| c.open).count() as i64;
    ctx.stats.channels.fetch_sub(open, Ordering::Relaxed);
    why
}
