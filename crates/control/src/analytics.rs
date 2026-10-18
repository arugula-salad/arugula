//! Product analytics: what people and their machines do with control,
//! sent to PostHog. Off unless `POSTHOG_API_KEY` is set, so a control
//! someone else runs never reports to anyone.
//!
//! Events are about control's own metadata: sign-ins, devices, joins,
//! teams, invites, billing, hosted sandboxes, cloud runs, relay sockets
//! and the requests people make. A person is their account id (no name,
//! login or address), a team is a `team` group, and nothing carries an IP
//! (`$geoip_disable`). Control never sees terminal bytes, so no event can
//! hold any.
//!
//! Capturing never waits: events go on a bounded queue that a task sends
//! in batches (every [`FLUSH_EVERY`] or [`BATCH`] events), and a full queue
//! drops them. What's queued is sent before control stops.

use std::{sync::Arc, time::Duration};

use axum::{
    extract::{MatchedPath, Request, State},
    middleware::Next,
    response::Response,
};
use serde_json::{Map, Value, json};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use crate::App;

const BATCH: usize = 100;
const FLUSH_EVERY: Duration = Duration::from_secs(10);
const QUEUE: usize = 10_000;

enum Msg {
    Event(Value),
    Flush(oneshot::Sender<()>),
}

#[derive(Default)]
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

/// Who asked, as the request's handler found out (a session's account):
/// put in the request's extensions for [`requests`] to read afterwards.
#[derive(Clone, Default)]
pub struct Asker(Arc<std::sync::OnceLock<String>>);

impl Asker {
    pub fn set(&self, account: &str) {
        let _ = self.0.set(account.to_owned());
    }
}

/// Every request a person makes that changes something, and every one
/// that fails: `control request`, by its route (not its path, which can
/// hold codes), method, status and time taken. Reads that succeed are
/// left out (pages poll), and so are daemons' own requests, which have
/// events of their own where they matter.
pub async fn requests(State(app): State<Arc<App>>, mut req: Request, next: Next) -> Response {
    if !app.analytics.on() {
        return next.run(req).await;
    }
    let asker = Asker::default();
    req.extensions_mut().insert(asker.clone());
    let route = req.extensions().get::<MatchedPath>().map(|p| p.as_str().to_owned());
    let method = req.method().clone();
    let started = std::time::Instant::now();
    let res = next.run(req).await;
    let status = res.status().as_u16();
    let (Some(route), Some(account)) = (route, asker.0.get()) else { return res };
    if method == axum::http::Method::GET && status < 400 {
        return res;
    }
    app.analytics.capture(
        account,
        "control request",
        json!({
            "route": route, "method": method.as_str(), "status": status,
            "ok": status < 400, "ms": started.elapsed().as_millis() as u64,
        }),
    );
    res
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

    /// A fake PostHog: where it listens, and the batches it gets.
    async fn fake_posthog() -> (String, mpsc::UnboundedReceiver<Value>) {
        use axum::{Json, Router, routing::post};
        let (got_tx, got) = mpsc::unbounded_channel::<Value>();
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
        (format!("http://{at}/"), got)
    }

    /// Events reach PostHog's batch endpoint, with the key, in one batch.
    #[tokio::test]
    async fn batches_reach_posthog() {
        let (host, mut got) = fake_posthog().await;
        let a = Analytics::start("phc_test".into(), &host, reqwest::Client::new());
        a.capture("acct1", "signed in", json!({ "method": "passkey" }));
        a.capture_team("acct1", "team1", "team created", json!({}));
        a.flush().await;
        let b = got.recv().await.unwrap();
        assert_eq!(b["api_key"], "phc_test");
        let events = b["batch"].as_array().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["event"], "signed in");
        assert_eq!(events[0]["distinct_id"], "acct1");
        assert_eq!(events[0]["properties"]["method"], "passkey");
        assert_eq!(events[0]["properties"]["$geoip_disable"], true);
        assert_eq!(events[1]["properties"]["$groups"]["team"], "team1");
    }

    /// A person's change goes as `control request` by its route, with the
    /// handler's own event; their reads that succeed don't.
    #[tokio::test]
    async fn requests_by_route() {
        let (host, mut got) = fake_posthog().await;
        let mut app = App::for_tests("http://control.test");
        app.analytics = Analytics::start("phc_test".into(), &host, reqwest::Client::new());
        let now = arugula_e2e::now_ms();
        let account = app.db.account_for("github", "1", "someone", "acct1", now).unwrap();
        app.db.add_session(&crate::auth::hash("tok"), &account, now, now + 60_000).unwrap();
        let app = Arc::new(app);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = l.local_addr().unwrap();
        let r = crate::router(app.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>();
        tokio::spawn(async move { axum::serve(l, r).await });

        let http = reqwest::Client::new();
        let cookie = format!("{}=tok", crate::auth::SESSION_COOKIE);
        let me = http.get(format!("http://{at}/api/me")).header("cookie", &cookie).send().await.unwrap();
        assert!(me.status().is_success());
        let named = http
            .post(format!("http://{at}/api/me/name"))
            .header("cookie", &cookie)
            .header("origin", "http://control.test")
            .json(&json!({ "name": "Someone" }))
            .send()
            .await
            .unwrap();
        assert!(named.status().is_success());
        app.analytics.flush().await;
        let b = got.recv().await.unwrap();
        let events: Vec<&str> = b["batch"].as_array().unwrap().iter().map(|e| e["event"].as_str().unwrap()).collect();
        assert_eq!(events, ["name changed", "control request"]);
        let req = &b["batch"][1];
        assert_eq!(req["distinct_id"], "acct1");
        assert_eq!(req["properties"]["route"], "/api/me/name");
        assert_eq!(req["properties"]["method"], "POST");
        assert_eq!(req["properties"]["status"], 200);
    }
}
