//! Live push: a device with no push service of its own (the desktop app,
//! whose webview has no `PushManager`) hears what control would push it
//! over a stream it holds open while it runs.
//!
//! It subscribes as any browser does (`push::subscribe`), signed by its
//! device key, with keys it made itself; only the endpoint is control's own:
//! `{site}/api/push/live/{id}`. So everything that pushes to the account
//! reaches it the same way and under the same choices: control's notices
//! (`push::notify_with`), the platform's (`push::push_account`, with its
//! kinds held back as `prefs` say) and what daemons relay (encrypted by the
//! daemon for that subscription alone, and held back while the account's
//! machines are off). Where `push::post` would hand the ciphertext to a push
//! service, it hands it to the endpoint's open streams here, still
//! encrypted: control can't read a daemon's, as with any push service.
//!
//! Only the device that subscribed can listen. `subscribe` answers a live
//! endpoint with its listen token (a MAC of the endpoint, under a key that
//! never leaves control), which `GET /api/push/live/{id}` takes as a bearer.
//! The endpoint itself isn't enough: the daemons that serve the account see
//! it. A stream also needs its subscription still there, and its device
//! approved and not revoked, and it ends when either changes.
//!
//! Nothing waits for a device that isn't listening, beyond a few minutes
//! for one that drops and comes back (`Last-Event-ID`).
//!
//! The streams are in this process: the hub runs on one machine.

use std::{
    collections::{HashMap, VecDeque},
    convert::Infallible,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use arugula_e2e::push::PushSub;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use tokio::sync::broadcast;

use crate::{ApiError, App, err};

/// Where live endpoints are, on any of control's sites.
pub(crate) const PATH: &str = "/api/push/live/";

/// How long a notice waits for a stream that dropped to come back, and how
/// many do.
const KEEP: Duration = Duration::from_secs(120);
const KEEP_MAX: usize = 32;
/// Streams open at once on one endpoint: one app, and a reconnect's overlap.
const PER_ENDPOINT: usize = 4;
/// How often an open stream checks its subscription and device again.
const RECHECK: Duration = Duration::from_secs(60);

/// One notice, as a push service would have been handed it.
struct Notice {
    id: u64,
    at: Instant,
    body: Vec<u8>,
}

/// What comes next on a stream.
type Next = broadcast::Receiver<Arc<Notice>>;

#[derive(Default)]
struct Slot {
    tx: Option<broadcast::Sender<Arc<Notice>>>,
    recent: VecDeque<Arc<Notice>>,
}

impl Slot {
    fn tx(&mut self) -> &broadcast::Sender<Arc<Notice>> {
        self.tx.get_or_insert_with(|| broadcast::channel(KEEP_MAX).0)
    }

    fn listeners(&self) -> usize {
        self.tx.as_ref().map_or(0, |t| t.receiver_count())
    }

    fn prune(&mut self) {
        while self.recent.front().is_some_and(|n| n.at.elapsed() > KEEP) || self.recent.len() > KEEP_MAX {
            self.recent.pop_front();
        }
    }
}

/// The live endpoints' open streams, and what they missed lately.
#[derive(Default)]
pub struct Live {
    slots: Mutex<HashMap<String, Slot>>,
    next: AtomicU64,
    key: OnceLock<[u8; 32]>,
}

impl Live {
    /// Hand `body` to `endpoint`'s streams: 201 when one is open, 202 when
    /// it's only kept for one coming back.
    pub(crate) fn send(&self, endpoint: &str, body: Vec<u8>) -> u16 {
        let n = Arc::new(Notice { id: self.next.fetch_add(1, Ordering::SeqCst) + 1, at: Instant::now(), body });
        let mut slots = self.slots.lock().unwrap();
        // The idle ones go: nothing listening, nothing kept.
        slots.retain(|_, s| {
            s.prune();
            s.listeners() > 0 || !s.recent.is_empty()
        });
        let slot = slots.entry(endpoint.to_owned()).or_default();
        slot.recent.push_back(n.clone());
        slot.prune();
        let heard = slot.listeners() > 0;
        let _ = slot.tx().send(n);
        if heard { 201 } else { 202 }
    }

    /// A new stream on `endpoint`: what it missed after `last`, and what
    /// comes next. None when it has too many open.
    fn listen(&self, endpoint: &str, last: Option<u64>) -> Option<(Vec<Arc<Notice>>, Next)> {
        let mut slots = self.slots.lock().unwrap();
        let slot = slots.entry(endpoint.to_owned()).or_default();
        if slot.listeners() >= PER_ENDPOINT {
            return None;
        }
        slot.prune();
        // Only a stream that comes back gets what it missed: a new one
        // starts now.
        let missed = match last {
            Some(last) => slot.recent.iter().filter(|n| n.id > last).cloned().collect(),
            None => Vec::new(),
        };
        Some((missed, slot.tx().subscribe()))
    }

    /// The endpoint's subscription is gone: its streams end.
    pub(crate) fn close(&self, endpoint: &str) {
        self.slots.lock().unwrap().remove(endpoint);
    }

    /// The token that listens on `endpoint`.
    pub(crate) fn token(&self, app: &App, endpoint: &str) -> anyhow::Result<String> {
        let key = match self.key.get() {
            Some(k) => *k,
            None => {
                let k = match app.db.setting("push-live")? {
                    Some(h) => hex::decode(h)?.try_into().map_err(|_| anyhow::anyhow!("bad push-live key"))?,
                    None => {
                        let k = arugula_e2e::random::<32>();
                        app.db.set_setting("push-live", &hex::encode(k))?;
                        k
                    }
                };
                *self.key.get_or_init(|| k)
            }
        };
        let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("any key length");
        mac.update(b"arugula push live v1\n");
        mac.update(endpoint.as_bytes());
        Ok(B64.encode(mac.finalize().into_bytes()))
    }
}

/// `endpoint` is a live one: on one of control's sites, at `PATH` and an
/// id (32 hex digits), and nothing else.
pub(crate) fn is_live(app: &App, endpoint: &str) -> bool {
    let Ok(u) = url::Url::parse(endpoint) else { return false };
    let origin = u.origin().ascii_serialization();
    let id = u.path().strip_prefix(PATH);
    app.cfg.sites.iter().any(|s| s.origin == origin)
        && u.query().is_none()
        && u.fragment().is_none()
        && id.is_some_and(valid_id)
}

fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Whether the subscription at `endpoint` may still listen: there, signed
/// by an approved device of its account that isn't revoked.
fn still_ok(app: &App, endpoint: &str) -> anyhow::Result<Option<String>> {
    let Some(account) = app.db.push_sub_account(endpoint)? else { return Ok(None) };
    let sub = app
        .db
        .push_subs(&account)?
        .into_iter()
        .filter_map(|v| serde_json::from_value::<PushSub>(v).ok())
        .find(|s| s.endpoint == endpoint);
    let Some(sub) = sub else { return Ok(None) };
    let Some((cert, true)) = app.db.device(&account, &sub.device)? else { return Ok(None) };
    if !sub.signed_by(&cert) || app.db.revocations(&account)?.iter().any(|r| r.device == sub.device) {
        return Ok(None);
    }
    Ok(Some(account))
}

/// `GET /api/push/live/{id}`: Server-Sent Events, one notice each, as its
/// push service would have been handed it (aes128gcm, RFC 8291), in base64
/// (URL-safe, unpadded). `Authorization: Bearer <listen token>`;
/// `Last-Event-ID` resumes after a drop. A keep-alive comment every 25 s
/// holds it open through proxies.
pub async fn live(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    if !valid_id(&id) {
        return Err(err(StatusCode::NOT_FOUND, "no such endpoint"));
    }
    let endpoint = format!("{}{PATH}{id}", app.cfg.site(&headers).url);
    let given = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    let want = app.live.token(&app, &endpoint)?;
    // Constant time: the token is the only secret here.
    let same = given.len() == want.len() && given.bytes().zip(want.bytes()).fold(0u8, |d, (a, b)| d | (a ^ b)) == 0;
    if !same {
        return Err(err(StatusCode::UNAUTHORIZED, "the listen token this endpoint's subscribe answered"));
    }
    if still_ok(&app, &endpoint)?.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "that subscription is gone, or its device is"));
    }
    let last = headers.get("last-event-id").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
    let Some((missed, rx)) = app.live.listen(&endpoint, last) else {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "this endpoint has streams open already"));
    };
    let event = |n: &Notice| Event::default().id(n.id.to_string()).data(B64.encode(&n.body));
    struct Open {
        app: Arc<App>,
        endpoint: String,
        rx: broadcast::Receiver<Arc<Notice>>,
        pending: VecDeque<Event>,
    }
    let open = Open { app, endpoint, rx, pending: missed.iter().map(|n| event(n)).collect() };
    let stream = futures_util::stream::unfold(open, move |mut o| async move {
        loop {
            if let Some(ev) = o.pending.pop_front() {
                return Some((Ok(ev), o));
            }
            tokio::select! {
                got = o.rx.recv() => match got {
                    Ok(n) => {
                        // Revoked or unsubscribed since: no more.
                        if !matches!(still_ok(&o.app, &o.endpoint), Ok(Some(_))) {
                            return None;
                        }
                        return Some((Ok(event(&n)), o));
                    }
                    // Too far behind: what it lost is lost, as a push
                    // service's queue would drop it.
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return None,
                },
                _ = tokio::time::sleep(RECHECK) => {
                    if !matches!(still_ok(&o.app, &o.endpoint), Ok(Some(_))) {
                        return None;
                    }
                }
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(25))))
}
