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
//! endpoint with its listen token, random and made for that subscription
//! alone (control keeps only its hash, with the subscription), which
//! `GET /api/push/live/{id}` takes as a bearer. The endpoint itself isn't
//! enough: the daemons that serve the account see it. Nor can anyone else
//! take it over: an endpoint that's one account's is refused to another,
//! and a stream needs the subscription it opened on still there, as it was
//! (account, device, token), its device approved and not revoked. It ends
//! when any of that changes, and when the subscription is replaced.
//!
//! Each device listens at one live endpoint (subscribing again replaces
//! it), and one nobody has listened at for 30 days is forgotten.
//!
//! Nothing waits for a device that isn't listening, beyond a few minutes
//! for one that drops and comes back (`Last-Event-ID`).
//!
//! The streams are in this process: the hub runs on one machine.

use std::{
    collections::{HashMap, VecDeque},
    convert::Infallible,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use arugula_e2e::{now_ms, push::PushSub};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use tokio::sync::broadcast;

use crate::{
    ApiError, App,
    auth::hash,
    db::{LiveRefused, LiveSub},
    err,
};

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
/// A live subscription nobody listened at for this long is forgotten.
const IDLE_MS: u64 = 30 * 24 * 3600 * 1000;

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

/// Keep `sub` (signed and checked by `push::subscribe`) at its live
/// endpoint, in place of its device's other live one: the listen token,
/// shown here alone. `most` subscriptions an account may have.
pub(crate) fn subscribe(app: &App, sub: &PushSub, most: usize) -> Result<String, ApiError> {
    forget_idle(app, &sub.account)?;
    let token = B64.encode(arugula_e2e::random::<32>());
    let body = serde_json::to_string(sub).map_err(anyhow::Error::from)?;
    let listen = hash(&token);
    let live =
        LiveSub { endpoint: &sub.endpoint, account: &sub.account, device: &sub.device, body: &body, listen: &listen };
    match app.db.put_live_sub(&live, most, now_ms())? {
        Ok(gone) => {
            // Their streams end: the one this replaced too.
            for e in gone {
                app.live.close(&e);
            }
            Ok(token)
        }
        Err(LiveRefused::Taken) => Err(err(StatusCode::CONFLICT, "that endpoint is someone else's")),
        Err(LiveRefused::Full) => Err(err(StatusCode::TOO_MANY_REQUESTS, "too many push subscriptions; turn some off")),
    }
}

/// Forget `account`'s live subscriptions nobody listened at lately.
pub(crate) fn forget_idle(app: &App, account: &str) -> anyhow::Result<()> {
    for e in app.db.drop_idle_live_subs(account, now_ms().saturating_sub(IDLE_MS))? {
        app.live.close(&e);
    }
    Ok(())
}

/// Who opened a stream: what its subscription was then.
#[derive(Clone)]
struct Holder {
    account: String,
    device: String,
    listen: String,
}

/// Whether the subscription at `endpoint` is still the one `holder` opened
/// a stream on: there, by the same account, device and token, signed by an
/// approved device of the account that isn't revoked.
fn still_ok(app: &App, endpoint: &str, holder: &Holder) -> anyhow::Result<bool> {
    let Some((account, body, listen)) = app.db.live_sub(endpoint)? else { return Ok(false) };
    let Ok(sub) = serde_json::from_str::<PushSub>(&body) else { return Ok(false) };
    if account != holder.account || sub.account != account || sub.device != holder.device || listen != holder.listen {
        return Ok(false);
    }
    let Some((cert, true)) = app.db.device(&account, &sub.device)? else { return Ok(false) };
    Ok(sub.signed_by(&cert) && !app.db.revocations(&account)?.iter().any(|r| r.device == sub.device))
}

/// Constant time: the token is the only secret here.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
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
    let Some((account, body, listen)) = app.db.live_sub(&endpoint)? else {
        return Err(err(StatusCode::NOT_FOUND, "that subscription is gone"));
    };
    if !same(&hash(given), &listen) {
        return Err(err(StatusCode::UNAUTHORIZED, "the listen token this endpoint's subscribe answered"));
    }
    let device = serde_json::from_str::<PushSub>(&body).map(|s| s.device).unwrap_or_default();
    let holder = Holder { account, device, listen };
    if !still_ok(&app, &endpoint, &holder)? {
        return Err(err(StatusCode::NOT_FOUND, "that subscription's device is gone"));
    }
    app.db.touch_live_sub(&endpoint, now_ms())?;
    let last = headers.get("last-event-id").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
    let Some((missed, rx)) = app.live.listen(&endpoint, last) else {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "this endpoint has streams open already"));
    };
    let event = |n: &Notice| Event::default().id(n.id.to_string()).data(B64.encode(&n.body));
    struct Open {
        app: Arc<App>,
        endpoint: String,
        holder: Holder,
        rx: Next,
        pending: VecDeque<Event>,
    }
    let open = Open { app, endpoint, holder, rx, pending: missed.iter().map(|n| event(n)).collect() };
    let stream = futures_util::stream::unfold(open, move |mut o| async move {
        loop {
            if let Some(ev) = o.pending.pop_front() {
                return Some((Ok(ev), o));
            }
            tokio::select! {
                got = o.rx.recv() => match got {
                    Ok(n) => {
                        // Revoked, unsubscribed or replaced since: no more.
                        if !matches!(still_ok(&o.app, &o.endpoint, &o.holder), Ok(true)) {
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
                    if !matches!(still_ok(&o.app, &o.endpoint, &o.holder), Ok(true)) {
                        return None;
                    }
                    // Listened at: not idle.
                    let _ = o.app.db.touch_live_sub(&o.endpoint, now_ms());
                }
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(25))))
}
