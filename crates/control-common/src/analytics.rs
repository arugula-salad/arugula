//! Product analytics, sent to PostHog: off unless there's a key, so a
//! control someone else runs never reports to anyone. A person is their
//! account id (no name, login or address), a team is a `team` group, and
//! nothing carries an IP (`$geoip_disable`).
//!
//! Capturing never waits: events go on a bounded queue that a task sends
//! in batches (every [`FLUSH_EVERY`] or [`BATCH`] events), and a full queue
//! drops them. What's queued is sent before control stops.
//!
//! The hub and the platform each capture their own events with one of
//! these. In process they share one (it's a handle on the queue); once the
//! platform is its own process it starts its own, with the same key: events
//! are keyed by account, so PostHog puts them together either way, and
//! routing them through the hub would add a hop to something that drops
//! events rather than wait.

use std::time::Duration;

use serde_json::{Map, Value, json};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

pub const BATCH: usize = 100;
pub const FLUSH_EVERY: Duration = Duration::from_secs(10);
const QUEUE: usize = 10_000;

enum Msg {
    Event(Value),
    Flush(oneshot::Sender<()>),
}

#[derive(Default, Clone)]
pub struct Analytics {
    tx: Option<mpsc::Sender<Msg>>,
}

impl Analytics {
    /// Sending to a PostHog project (`host`: `https://us.i.posthog.com`).
    pub fn start(key: String, host: &str, http: reqwest::Client) -> Self {
        let (tx, rx) = mpsc::channel(QUEUE);
        let url = format!("{}/batch/", host.trim_end_matches('/'));
        info!(%url, "product analytics on");
        tokio::spawn(send(rx, url, key, http));
        Analytics { tx: Some(tx) }
    }

    pub fn on(&self) -> bool {
        self.tx.is_some()
    }

    /// `event` by `who` (an account id, or `daemon:<id>` for a machine
    /// acting alone), with `props` (an object).
    pub fn capture(&self, who: &str, event: &str, props: Value) {
        let Some(tx) = &self.tx else { return };
        let mut p = match props {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        p.insert("$lib".into(), "arugula-control".into());
        p.insert("$lib_version".into(), env!("CARGO_PKG_VERSION").into());
        p.insert("$geoip_disable".into(), true.into());
        let e = json!({
            "event": event,
            "distinct_id": who,
            "properties": p,
            "timestamp": iso(arugula_e2e::now_ms()),
        });
        if tx.try_send(Msg::Event(e)).is_err() {
            warn!(event, "analytics queue full: dropped an event");
        }
    }

    /// `event` before there's an account to put it on (a machine's join
    /// request): by `who`, with no person made for it.
    pub fn capture_anon(&self, who: &str, event: &str, props: Value) {
        let mut props = match props {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        props.insert("$process_person_profile".into(), false.into());
        self.capture(who, event, Value::Object(props))
    }

    /// `event` about a team: the account's, grouped under the team.
    pub fn capture_team(&self, who: &str, team: &str, event: &str, props: Value) {
        let mut props = match props {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        props.insert("$groups".into(), json!({ "team": team }));
        props.insert("team".into(), team.into());
        self.capture(who, event, Value::Object(props))
    }

    /// Person properties for an account.
    pub fn identify(&self, who: &str, set: Value, set_once: Value) {
        self.capture(who, "$identify", json!({ "$set": set, "$set_once": set_once }));
    }

    /// Group properties for a team.
    pub fn group(&self, who: &str, team: &str, set: Value) {
        self.capture(who, "$groupidentify", json!({ "$group_type": "team", "$group_key": team, "$group_set": set }));
    }

    /// A sign-in by `method` (`github`, `passkey`, `app`): `signed up`
    /// first when it made the account.
    pub fn signed_in(&self, who: &str, method: &str, new: bool) {
        if new {
            self.capture(who, "signed up", json!({ "method": method }));
        }
        self.capture(who, "signed in", json!({ "method": method, "new_account": new }));
        self.identify(
            who,
            json!({ "last_sign_in_method": method }),
            json!({ "first_sign_in_method": method, "signed_up_at": iso(arugula_e2e::now_ms()) }),
        );
    }

    /// Send what's queued now (control stopping, tests).
    pub async fn flush(&self) {
        let Some(tx) = &self.tx else { return };
        let (done, wait) = oneshot::channel();
        if tx.send(Msg::Flush(done)).await.is_ok() {
            let _ = tokio::time::timeout(Duration::from_secs(5), wait).await;
        }
    }
}

async fn send(mut rx: mpsc::Receiver<Msg>, url: String, key: String, http: reqwest::Client) {
    let mut batch = Vec::with_capacity(BATCH);
    let mut tick = tokio::time::interval(FLUSH_EVERY);
    loop {
        let done = tokio::select! {
            m = rx.recv() => match m {
                Some(Msg::Event(e)) => {
                    batch.push(e);
                    if batch.len() < BATCH {
                        continue;
                    }
                    None
                }
                Some(Msg::Flush(done)) => Some(done),
                None => {
                    post(&http, &url, &key, &mut batch).await;
                    return;
                }
            },
            _ = tick.tick() => None,
        };
        post(&http, &url, &key, &mut batch).await;
        if let Some(done) = done {
            let _ = done.send(());
        }
    }
}

async fn post(http: &reqwest::Client, url: &str, key: &str, batch: &mut Vec<Value>) {
    if batch.is_empty() {
        return;
    }
    let n = batch.len();
    let body = json!({ "api_key": key, "batch": std::mem::take(batch) });
    match http.post(url).json(&body).send().await.and_then(|r| r.error_for_status()) {
        Ok(_) => {}
        Err(e) => warn!(error = %e, events = n, "analytics batch didn't go"),
    }
}

/// RFC 3339 (UTC, ms) for a time in ms.
fn iso(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}T{:02}:{:02}:{:02}.{:03}Z", crate::day(ms), (s / 3600) % 24, (s / 60) % 60, s % 60, ms % 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_790_000_000_123), "2026-09-21T14:13:20.123Z");
    }

    #[test]
    fn off_drops_everything() {
        let a = Analytics::default();
        assert!(!a.on());
        a.capture("acct", "x", json!({}));
    }

    /// Events reach PostHog's batch endpoint, with the key, in one batch,
    /// from either of two handles on one queue.
    #[tokio::test]
    async fn batches_reach_posthog() {
        use axum::{Json, Router, routing::post};
        let (got_tx, mut got) = mpsc::unbounded_channel::<Value>();
        let fake = Router::new().route(
            "/batch/",
            post(move |Json(b): Json<Value>| {
                let got_tx = got_tx.clone();
                async move {
                    let _ = got_tx.send(b);
                    "{}"
                }
            }),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, fake).await });
        let a = Analytics::start("phc_test".into(), &format!("http://{at}/"), reqwest::Client::new());
        let b = a.clone();
        a.capture("acct1", "signed in", json!({ "method": "passkey" }));
        b.capture_team("acct1", "team1", "team created", json!({}));
        a.flush().await;
        let b = got.recv().await.unwrap();
        assert_eq!(b["api_key"], "phc_test");
        let events = b["batch"].as_array().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["event"], "signed in");
        assert_eq!(events[0]["distinct_id"], "acct1");
        assert_eq!(events[0]["properties"]["method"], "passkey");
        assert_eq!(events[0]["properties"]["$geoip_disable"], true);
        assert_eq!(events[0]["properties"]["$lib"], "arugula-control");
        assert_eq!(events[1]["properties"]["$groups"]["team"], "team1");
    }
}
