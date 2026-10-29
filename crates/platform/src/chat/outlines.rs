//! Outline presence (S43, spike): who's around, as live camera outlines.
//! A page that turned its outline on posts a frame (a few KB of traced
//! polylines, opaque here) a few times a second, and control passes it on
//! to the open People blocks of those who share a team with its person
//! ([`super::people`]'s `visible`), the person included.
//!
//! **Nothing is kept but the newest frame, in memory**: never a row, never
//! a file. A frame older than [`STALE_MS`] is gone, and a stream that had
//! shown it says `off`. In memory means one machine: a page posting to one
//! platform machine isn't heard by a stream on another (the spike runs on
//! one; a shipped relay crosses machines, as chat's events do).
//!
//! Behind chat's gate, as presence is.

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_control_common::{ApiError, err};
use arugula_e2e::now_ms;
use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
};
use base64::Engine;
use serde_json::json;
use tokio::sync::broadcast;

use super::{gate, people::visible};
use crate::{Platform, auth::Person};

/// The biggest frame taken.
pub const MAX_FRAME: usize = 32 << 10;
/// A frame this soon after the person's last isn't kept (a page sends at
/// most a few a second).
const GAP_MS: u64 = 70;
/// A frame older than this is a camera that stopped.
const STALE_MS: u64 = 10_000;
/// How often a stream re-reads whom its person sees.
const RESEE: Duration = Duration::from_secs(30);
/// How often a stream looks for frames gone stale.
const SWEEP: Duration = Duration::from_secs(2);
/// Open streams, all told and per person.
const STREAMS: usize = 2_000;
const STREAMS_PER_ACCOUNT: usize = 8;
/// Frames in flight to a slow stream before it skips ahead.
const RING: usize = 256;

#[derive(Clone)]
struct Kept {
    at: u64,
    /// The frame, as the stream sends it: base64, made once.
    data: Arc<str>,
}

enum Change {
    Frame { account: String, kept: Kept },
    Off { account: String },
}

/// The newest frame of each person whose outline is on, and the streams
/// that hear them.
pub struct Outlines {
    kept: Mutex<HashMap<String, Kept>>,
    tx: broadcast::Sender<Arc<Change>>,
    open: Mutex<HashMap<String, usize>>,
}

impl Default for Outlines {
    fn default() -> Self {
        Self { kept: Mutex::new(HashMap::new()), tx: broadcast::channel(RING).0, open: Mutex::new(HashMap::new()) }
    }
}

impl Outlines {
    /// Keep `account`'s frame and pass it on; false for one inside the gap.
    fn put(&self, account: &str, bytes: &[u8], now: u64) -> bool {
        let kept = {
            let mut k = self.kept.lock().unwrap();
            if k.get(account).is_some_and(|p| now.saturating_sub(p.at) < GAP_MS) {
                return false;
            }
            if k.len() > 10_000 {
                k.retain(|_, p| now.saturating_sub(p.at) < STALE_MS);
            }
            let kept = Kept { at: now, data: base64::engine::general_purpose::STANDARD.encode(bytes).into() };
            k.insert(account.to_owned(), kept.clone());
            kept
        };
        let _ = self.tx.send(Arc::new(Change::Frame { account: account.to_owned(), kept }));
        true
    }

    fn off(&self, account: &str) {
        if self.kept.lock().unwrap().remove(account).is_some() {
            let _ = self.tx.send(Arc::new(Change::Off { account: account.to_owned() }));
        }
    }

    /// The fresh frames of `who`.
    fn fresh(&self, who: &HashSet<String>, now: u64) -> Vec<(String, Kept)> {
        let k = self.kept.lock().unwrap();
        let mut out: Vec<_> = k
            .iter()
            .filter(|(a, p)| who.contains(*a) && now.saturating_sub(p.at) < STALE_MS)
            .map(|(a, p)| (a.clone(), p.clone()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

/// `POST /api/people/outline`: the person's newest frame (raw bytes, at
/// most [`MAX_FRAME`]). 204 whether it was kept or fell inside the gap.
pub async fn put(State(app): State<Arc<Platform>>, s: Person, body: Bytes) -> Result<StatusCode, ApiError> {
    gate(&app, &s.account).await?;
    if body.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "an empty frame"));
    }
    if body.len() > MAX_FRAME {
        return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "a frame is at most 32 KiB"));
    }
    app.chat.outlines.put(&s.account, &body, now_ms());
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/people/outline/off`: the person's outline is off now.
pub async fn off(State(app): State<Arc<Platform>>, s: Person) -> Result<StatusCode, ApiError> {
    gate(&app, &s.account).await?;
    app.chat.outlines.off(&s.account);
    Ok(StatusCode::NO_CONTENT)
}

/// One open stream, counted while it lives.
struct Open {
    app: Arc<Platform>,
    account: String,
}

impl Drop for Open {
    fn drop(&mut self) {
        let mut open = self.app.chat.outlines.open.lock().unwrap();
        if let Some(n) = open.get_mut(&self.account) {
            *n -= 1;
            if *n == 0 {
                open.remove(&self.account);
            }
        }
    }
}

struct Live {
    open: Open,
    rx: broadcast::Receiver<Arc<Change>>,
    sees: HashSet<String>,
    /// Who this stream has a frame of on screen, and that frame's time.
    shown: HashMap<String, u64>,
    pending: std::collections::VecDeque<Event>,
    resee: tokio::time::Interval,
    sweep: tokio::time::Interval,
}

fn frame(account: &str, k: &Kept) -> Event {
    Event::default().event("frame").data(json!({ "account": account, "at": k.at, "data": &*k.data }).to_string())
}

fn gone(account: &str) -> Event {
    Event::default().event("off").data(json!({ "account": account }).to_string())
}

impl Live {
    fn app(&self) -> &Platform {
        &self.open.app
    }

    /// Re-read whom the person sees: those no longer seen go off, and the
    /// newly seen's fresh frames come.
    async fn resee(&mut self) {
        let sees = match visible(self.app(), &self.open.account).await {
            Ok((people, _)) => people,
            Err(e) => {
                tracing::warn!(error = %e, "outlines: reading whom a stream's person sees");
                return;
            }
        };
        let dropped: Vec<String> = self.shown.keys().filter(|a| !sees.contains(*a)).cloned().collect();
        for a in dropped {
            self.shown.remove(&a);
            self.pending.push_back(gone(&a));
        }
        let fresh = self.app().chat.outlines.fresh(&sees, now_ms());
        for (a, k) in fresh {
            if self.shown.get(&a) != Some(&k.at) {
                self.shown.insert(a.clone(), k.at);
                self.pending.push_back(frame(&a, &k));
            }
        }
        self.sees = sees;
    }

    fn sweep(&mut self) {
        let now = now_ms();
        let stale: Vec<String> =
            self.shown.iter().filter(|(_, at)| now.saturating_sub(**at) >= STALE_MS).map(|(a, _)| a.clone()).collect();
        for a in stale {
            self.shown.remove(&a);
            self.pending.push_back(gone(&a));
        }
    }

    fn deliver(&mut self, c: &Change) {
        match c {
            Change::Frame { account, kept } if self.sees.contains(account) => {
                self.shown.insert(account.clone(), kept.at);
                self.pending.push_back(frame(account, kept));
            }
            Change::Off { account } if self.shown.remove(account).is_some() => {
                self.pending.push_back(gone(account));
            }
            _ => {}
        }
    }
}

/// `GET /api/people/outlines`: Server-Sent Events: `frame`
/// `{account, at, data}` (data the frame in base64) for each fresh frame of
/// those the person sees (themself included), first the ones kept and then
/// live, and `off` `{account}` when one's outline stops (turned off, gone
/// stale, or no longer someone they see).
pub async fn stream(
    State(app): State<Arc<Platform>>,
    s: Person,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    gate(&app, &s.account).await?;
    {
        let mut open = app.chat.outlines.open.lock().unwrap();
        if open.values().sum::<usize>() >= STREAMS {
            return Err(err(StatusCode::SERVICE_UNAVAILABLE, "too many open pages here right now; try again soon"));
        }
        let n = open.entry(s.account.clone()).or_default();
        if *n >= STREAMS_PER_ACCOUNT {
            return Err(err(StatusCode::TOO_MANY_REQUESTS, "too many open People blocks; close some"));
        }
        *n += 1;
    }
    let rx = app.chat.outlines.tx.subscribe();
    let start = tokio::time::Instant::now();
    let mut live = Live {
        open: Open { app: app.clone(), account: s.account },
        rx,
        sees: HashSet::new(),
        shown: HashMap::new(),
        pending: Default::default(),
        resee: tokio::time::interval_at(start + RESEE, RESEE),
        sweep: tokio::time::interval_at(start + SWEEP, SWEEP),
    };
    live.resee().await;
    // A comment first, so the page knows it's open before anyone's frame.
    live.pending.push_front(Event::default().comment("open"));
    let stream = futures_util::stream::unfold(live, |mut live| async move {
        loop {
            if let Some(ev) = live.pending.pop_front() {
                return Some((Ok(ev), live));
            }
            tokio::select! {
                got = live.rx.recv() => match got {
                    Ok(c) => live.deliver(&c),
                    // Behind: what matters is the newest of each, which a
                    // re-read sends.
                    Err(broadcast::error::RecvError::Lagged(_)) => live.resee().await,
                    Err(broadcast::error::RecvError::Closed) => return None,
                },
                _ = live.resee.tick() => live.resee().await,
                _ = live.sweep.tick() => live.sweep(),
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(25))))
}
