//! The WebSocket transport under control's relay: a socket to the other
//! end (`open_ws`, ws or wss with the system's roots) carrying a stream
//! mux (`arugula_e2e::mux`) whose streams the other end opens and this
//! daemon serves (`serve_mux`), with pings both ways and text messages
//! beside the streams.
//!
//! Every machine's connection to control rides on it (`control.rs`, M18),
//! a hosted box's redial after a resume too, and so does the standing
//! channel to another machine's agents (`peer.rs`, #856). The dial-out
//! tunnel to a home daemon (`dial.rs`, M4c) was its first use and is the
//! same protocol over the same functions.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use axum::http::HeaderValue;
use futures_util::{SinkExt, StreamExt};
use tokio::{
    io::DuplexStream,
    sync::{Notify, mpsc},
};
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};

use arugula_e2e::mux::Mux;

/// How often each end pings, and how long silence lasts before the socket
/// is given up as dead.
pub(crate) const PING_EVERY: Duration = Duration::from_secs(15);
pub(crate) const DEAD_AFTER: Duration = Duration::from_secs(45);

pub type Ws = tokio_tungstenite::WebSocketStream<Box<dyn Io>>;
pub trait Io: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Io for T {}

/// A WebSocket to `url` (ws or wss) with extra request headers: the tunnel
/// to a home daemon, or (M18) to control's relay.
pub async fn open_ws(url: &reqwest::Url, headers: &[(&str, &str)]) -> anyhow::Result<Ws> {
    let mut req = url.as_str().into_client_request()?;
    for (k, v) in headers {
        req.headers_mut().insert(axum::http::HeaderName::from_bytes(k.as_bytes())?, HeaderValue::from_str(v)?);
    }
    let host = url.host_str().ok_or_else(|| anyhow::anyhow!("{url} has no host"))?.to_owned();
    let port = url.port_or_known_default().unwrap_or(443);
    let tcp = tokio::time::timeout(Duration::from_secs(15), tokio::net::TcpStream::connect((host.as_str(), port)))
        .await
        .map_err(|_| anyhow::anyhow!("connecting timed out"))??;
    tcp.set_nodelay(true)?;
    let io: Box<dyn Io> = if url.scheme() == "wss" {
        let name = tokio_rustls::rustls::pki_types::ServerName::try_from(host)?;
        Box::new(tls_connector()?.connect(name, tcp).await?)
    } else {
        Box::new(tcp)
    };
    let mut config = tungstenite::protocol::WebSocketConfig::default();
    config.max_message_size = Some(1 << 20);
    let (ws, _) = tokio_tungstenite::client_async_with_config(req, io, Some(config)).await.map_err(|e| match e {
        tungstenite::Error::Http(r) => {
            let said = format!(
                "{} said {}: {}",
                url.host_str().unwrap_or("it"),
                r.status(),
                r.body().as_deref().map(String::from_utf8_lossy).unwrap_or_default()
            );
            let wait = r.headers().get("retry-after").and_then(|v| v.to_str().ok()?.trim().parse().ok());
            match wait {
                Some(secs) if r.status() == 503 => {
                    anyhow::Error::new(Busy { wait: Duration::from_secs(secs).min(MOST_WAIT), said })
                }
                _ => anyhow::anyhow!(said),
            }
        }
        e => e.into(),
    })?;
    Ok(ws)
}

/// The other end is full for now and said how long to wait (`503` with
/// `Retry-After`: control's relay at its ceiling, #344), up to
/// [`MOST_WAIT`].
#[derive(Debug)]
pub struct Busy {
    pub wait: Duration,
    said: String,
}

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.said)
    }
}

impl std::error::Error for Busy {}

const MOST_WAIT: Duration = Duration::from_secs(600);

fn tls_connector() -> anyhow::Result<tokio_rustls::TlsConnector> {
    use tokio_rustls::rustls;
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = crate::roots::verifier(provider.clone())?;
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(tokio_rustls::TlsConnector::from(Arc::new(config)))
}

/// Text messages on control's relay socket besides `trust` (M40, M65):
/// what to do with those that come, and messages to send: the latest of
/// each `out` (sent on connect and whenever it changes).
pub struct Texts<'a> {
    pub on_text: &'a (dyn Fn(&str) + Send + Sync),
    pub out: Vec<tokio::sync::watch::Receiver<Option<String>>>,
    /// Messages to send each in turn, none dropped (M168: ops' answers).
    pub queue: Option<mpsc::UnboundedReceiver<String>>,
}

/// The host end of a mux over `ws`: streams the other end opens go to
/// `accept` (raw streams to `raw`, with their kind, when given), until the
/// socket closes or goes quiet.
/// A text message `trust` (control's relay) wakes `nudge`; others go to
/// `texts`.
pub async fn serve_mux(
    ws: Ws,
    accept: &mpsc::UnboundedSender<DuplexStream>,
    raw: Option<&mpsc::UnboundedSender<(Vec<u8>, DuplexStream)>>,
    nudge: Option<&Notify>,
    texts: Option<Texts<'_>>,
) -> anyhow::Result<()> {
    let (mux, mut out) = Mux::with_raw(Some(accept.clone()), raw.cloned());
    let (mut tx, mut rx) = ws.split();
    // Each `out`'s latest message, now and as it changes, in one queue.
    let (say_tx, mut say) = mpsc::unbounded_channel::<String>();
    let mut sayers = Vec::new();
    let on_text = match texts {
        Some(t) => {
            for mut w in t.out {
                let say_tx = say_tx.clone();
                sayers.push(crate::task::spawn(async move {
                    loop {
                        if let Some(m) = w.borrow_and_update().clone()
                            && say_tx.send(m).is_err()
                        {
                            return;
                        }
                        if w.changed().await.is_err() {
                            return;
                        }
                    }
                }));
            }
            if let Some(mut q) = t.queue {
                let say_tx = say_tx.clone();
                sayers.push(crate::task::spawn(async move {
                    while let Some(m) = q.recv().await {
                        if say_tx.send(m).is_err() {
                            return;
                        }
                    }
                }));
            }
            Some(t.on_text)
        }
        None => None,
    };
    drop(say_tx);
    struct Abort(Vec<tokio::task::JoinHandle<()>>);
    impl Drop for Abort {
        fn drop(&mut self) {
            self.0.iter().for_each(|h| h.abort());
        }
    }
    let _sayers = Abort(sayers);
    let mut ping = tokio::time::interval(PING_EVERY);
    let mut heard = Instant::now();
    let result = loop {
        tokio::select! {
            m = rx.next() => match m {
                Some(Ok(tungstenite::Message::Binary(b))) => {
                    heard = Instant::now();
                    if let Err(e) = mux.handle(&b) {
                        break Err(anyhow::anyhow!("the other end broke the protocol: {e}"));
                    }
                }
                Some(Ok(tungstenite::Message::Close(_))) | None => break Ok(()),
                Some(Err(e)) => break Err(e.into()),
                Some(Ok(tungstenite::Message::Text(t))) => {
                    heard = Instant::now();
                    if t.as_str() == "trust" {
                        if let Some(n) = nudge {
                            n.notify_one();
                        }
                    } else if let Some(f) = on_text {
                        f(t.as_str());
                    }
                }
                Some(Ok(_)) => heard = Instant::now(),
            },
            Some(f) = out.recv() => {
                if let Err(e) = tx.send(tungstenite::Message::Binary(f.into())).await { break Err(e.into()) }
            }
            Some(m) = say.recv() => {
                if let Err(e) = tx.send(tungstenite::Message::Text(m.into())).await { break Err(e.into()) }
            }
            _ = ping.tick() => {
                if heard.elapsed() > DEAD_AFTER {
                    break Err(anyhow::anyhow!("the other end went quiet"));
                }
                if let Err(e) = tx.send(tungstenite::Message::Ping(Default::default())).await { break Err(e.into()) }
            }
        }
    };
    mux.close();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        http::{StatusCode, header},
        response::IntoResponse,
        routing::get,
    };

    #[tokio::test]
    async fn a_full_relay_says_how_long_to_wait() {
        // Control's relay at its ceiling (#344); and a plain refusal.
        let full = || async {
            let mut r =
                (StatusCode::SERVICE_UNAVAILABLE, r#"{"error":"control is full: try again shortly"}"#).into_response();
            r.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from_static("30"));
            r
        };
        let app = Router::new()
            .route("/full", get(full))
            .route("/no", get(|| async { (StatusCode::TOO_MANY_REQUESTS, "slow down") }));
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = l.local_addr().unwrap();
        crate::task::spawn(async move { axum::serve(l, app).await.unwrap() });
        let url = |p: &str| reqwest::Url::parse(&format!("ws://{at}{p}")).unwrap();
        let e = open_ws(&url("/full"), &[]).await.err().unwrap();
        assert_eq!(e.downcast_ref::<Busy>().unwrap().wait, Duration::from_secs(30));
        assert!(e.to_string().contains("503 Service Unavailable: {\"error\":\"control is full"), "{e}");
        let e = open_ws(&url("/no"), &[]).await.err().unwrap();
        assert!(e.downcast_ref::<Busy>().is_none(), "{e}");
    }
}
