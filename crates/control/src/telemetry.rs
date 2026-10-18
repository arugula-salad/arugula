//! Telemetry from Arugula on people's machines (M100): installs' reports,
//! passed to product analytics, and the version, OS and arch joined
//! daemons say they run.
//!
//! A report signed by a daemon joined here counts for its account; any
//! other goes on `install:<id>` with no person made for it, which is all
//! control knows of it. Either way it's checked first
//! ([`wire::telemetry::Report::check`]), and limited per client address
//! (unsigned), and per install or daemon.
//!
//! A joined daemon's version, OS and arch need no opt-in: they describe a
//! machine joined to this control, like the name and addresses it already
//! keeps. They ride on the trust request it makes anyway, and an older
//! daemon sends none.
//!
//! Control's own page (M104) isn't any one machine's, so its yes is the
//! account's (`accounts.telemetry`, NULL until asked). With it, the page
//! sends what it counted each minute, as `page day` under the account;
//! without it, control refuses them whatever a page sends.

use std::{net::SocketAddr, sync::Arc};

use arugula_control_wire as wire;
use arugula_e2e::now_ms;
use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use wire::telemetry::{Arch, Counts, Event, MAX_PAGE_COUNT, Os, PageConsent, PageReport, Report, SetPageConsent};

use crate::{
    ApiError, App,
    auth::{DaemonAuth, Session},
    err, limit, refusal,
};

/// The most a report's body may be.
pub const MAX_BODY: usize = 64 * 1024;

/// An install's report (`POST /api/telemetry`), signed or not.
pub async fn report(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    d: Option<DaemonAuth>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    match &d {
        Some(d) => app.limits.check_daemon(limit::INSTALL_REPORTS, &d.cert.device)?,
        None => app.limits.check(limit::TELEMETRY, app.limits.client_ip(peer, &headers))?,
    }
    let r: Report = serde_json::from_slice(&body).map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    r.check().map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    if d.is_none() {
        app.limits.check_install(limit::INSTALL_REPORTS, &r.install)?;
    }
    let mut common = Map::new();
    common.insert("install".into(), r.install.clone().into());
    common.insert("version".into(), r.version.clone().into());
    for (k, v) in
        [("os", name(&r.os)), ("arch", name(&r.arch)), ("channel", name(&r.channel)), ("control", name(&r.control))]
    {
        common.insert(k.into(), v.into());
    }
    if let Some(d) = &d {
        common.insert("machine".into(), d.cert.device.clone().into());
    }
    let send = |event: &str, props: Map<String, Value>| {
        let mut p = common.clone();
        p.extend(props);
        match &d {
            Some(d) => app.analytics.capture(&d.cert.account, event, Value::Object(p)),
            None => app.analytics.capture_anon(&format!("install:{}", r.install), event, Value::Object(p)),
        }
    };
    for e in &r.events {
        match e {
            Event::OptedIn { via } => send("install opted in", obj(json!({ "via": name(via) }))),
            Event::Started { service } => send("install started", obj(json!({ "service": service }))),
            Event::Daily(c) => send("install day", day(c)),
            Event::Other => {}
        }
    }
    for c in &r.crashes {
        // The version that crashed, which may not be the one reporting.
        send(
            "crash",
            obj(json!({
                "process": name(&c.process), "version": c.version, "install_version": r.version,
                "location": c.location, "frames": c.frames, "at": c.at,
            })),
        );
    }
    crate::reply(&wire::Ack {})
}

/// The most a page report's body may be.
pub const MAX_PAGE_BODY: usize = 4 * 1024;

/// The account's answer for control's page (M104).
pub async fn page_consent(State(app): State<Arc<App>>, s: Session) -> Result<Json<Value>, ApiError> {
    let (on, at) = app.db.page_telemetry(&s.account)?;
    crate::reply(&PageConsent { on, at, ask: app.analytics.on() })
}

/// The account answers (the page's card, or its switch under Account).
/// Saying yes is sent as `page opted in`; saying no sends nothing.
pub async fn set_page_consent(
    State(app): State<Arc<App>>,
    s: Session,
    Json(b): Json<SetPageConsent>,
) -> Result<Json<Value>, ApiError> {
    let now = now_ms();
    app.db.set_page_telemetry(&s.account, b.on, now)?;
    if b.on {
        app.analytics.capture(&s.account, "page opted in", json!({}));
    }
    crate::reply(&PageConsent { on: Some(b.on), at: Some(now), ask: app.analytics.on() })
}

/// Control's page's counts (`POST /api/telemetry/page`), sent as `page
/// day` under the account: only once it said yes, whatever a page sends.
pub async fn page(State(app): State<Arc<App>>, s: Session, body: Bytes) -> Result<Json<Value>, ApiError> {
    app.limits.check_account(limit::PAGE_REPORTS, &s.account)?;
    let r: PageReport = serde_json::from_slice(&body).map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    if app.db.page_telemetry(&s.account)?.0 != Some(true) {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            wire::telemetry::NOT_OPTED_IN,
            "this account hasn't said yes to usage reports from this page",
        ));
    }
    // A view this control doesn't know reads as `view_other`.
    let mut p = Map::new();
    for (v, n) in &r.views {
        p.insert(format!("view_{}", name(v).replace('-', "_")), (*n).min(MAX_PAGE_COUNT).into());
    }
    p.insert("web_errors".into(), r.web_errors.min(MAX_PAGE_COUNT).into());
    if r.views.values().any(|n| *n > 0) || r.web_errors > 0 {
        app.analytics.capture(&s.account, "page day", Value::Object(p));
    }
    crate::reply(&wire::Ack {})
}

/// A day's counts as properties: `panes_opened`, `agents_started_claude`,
/// `blocks_opened_editor`, `clients_web`, `view_swarm`, `flags_on` (a
/// list), `web_errors`.
fn day(c: &Counts) -> Map<String, Value> {
    let mut p = Map::new();
    p.insert("panes_opened".into(), c.panes_opened.into());
    p.insert("panes_peak".into(), c.panes_peak.into());
    p.insert("web_errors".into(), c.web_errors.into());
    let mut each = |prefix: &str, m: Vec<(String, u32)>| {
        for (k, n) in m {
            p.insert(format!("{prefix}_{}", k.replace('-', "_")), n.into());
        }
    };
    each("agents_started", c.agents_started.iter().map(|(k, n)| (name(k), *n)).collect());
    each("blocks_opened", c.blocks_opened.iter().map(|(k, n)| (name(k), *n)).collect());
    each("clients", c.clients.iter().map(|(k, n)| (name(k), *n)).collect());
    each("view", c.views.iter().map(|(k, n)| (name(k), *n)).collect());
    p.insert("flags_on".into(), c.flags_on.iter().map(name).collect::<Vec<_>>().into());
    p
}

/// A closed set's value as the wire spells it (`getting-started`).
fn name<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_else(|| "other".into())
}

fn obj(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

/// What a daemon says it runs, on `GET /api/daemon/trust` (M100). Read
/// leniently: an older daemon says nothing, and an unknown OS or arch is
/// `other`.
#[derive(Debug, Default, Deserialize)]
pub struct Machine {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    os: Option<String>,
    #[serde(default)]
    arch: Option<String>,
}

/// Keep what a daemon says it runs; `machine version` when it changed.
pub fn machine_seen(app: &App, d: &DaemonAuth, m: &Machine) -> Result<(), ApiError> {
    let Some(version) = m.version.as_deref().filter(|v| looks_like_version(v)) else { return Ok(()) };
    let os = name(&m.os.as_deref().map_or(Os::Other, |s| serde_json::from_value(s.into()).unwrap_or(Os::Other)));
    let arch =
        name(&m.arch.as_deref().map_or(Arch::Other, |s| serde_json::from_value(s.into()).unwrap_or(Arch::Other)));
    if app.db.set_daemon_machine(&d.cert.device, version, &os, &arch)? {
        app.analytics.capture(
            &d.cert.account,
            "machine version",
            json!({ "machine": d.cert.device, "version": version, "os": os, "arch": arch }),
        );
    }
    Ok(())
}

/// The same rule [`Report::check`] holds a version to.
fn looks_like_version(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
}

#[cfg(test)]
mod tests {
    use arugula_e2e::{Cert, DeviceKeys, Kind, now_ms};
    use tokio::sync::mpsc;

    use super::*;
    use crate::analytics::{Analytics, tests::fake_posthog};

    struct Control {
        app: Arc<App>,
        base: String,
        http: reqwest::Client,
        got: mpsc::UnboundedReceiver<Value>,
    }

    async fn control() -> Control {
        let (host, got) = fake_posthog().await;
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let mut app = App::for_tests(&base);
        app.analytics = Analytics::start("phc_test".into(), &host, reqwest::Client::new());
        let app = Arc::new(app);
        let svc = crate::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
        Control { app, base, http: reqwest::Client::new(), got }
    }

    impl Control {
        /// A daemon joined to account `acct1`.
        fn daemon(&self) -> DeviceKeys {
            self.app.db.account_for("github", "gh-1", "someone", "acct1", now_ms()).unwrap();
            let keys = DeviceKeys::generate();
            let cert = Cert::new(&keys, "acct1", Kind::Daemon, "geek");
            self.app.db.put_device(&cert, true, now_ms()).unwrap();
            self.app.db.put_daemon("acct1", &cert.device, "geek", &[]).unwrap();
            keys
        }

        async fn send(&self, method: &str, pq: &str, body: &[u8], signer: Option<&DeviceKeys>) -> u16 {
            let mut req = self.http.request(method.parse().unwrap(), format!("{}{pq}", self.base)).body(body.to_vec());
            if let Some(k) = signer {
                let ms = now_ms();
                let nonce = hex::encode(arugula_e2e::random::<16>());
                let msg = crate::auth::daemon_auth_message_v2(method, pq, ms, &nonce, body);
                let sig = hex::encode(k.signature(msg.as_bytes()));
                req = req.header("x-arugula-auth", format!("v2 {} {ms} {nonce} {sig}", k.id()));
            }
            req.send().await.unwrap().status().as_u16()
        }

        async fn report(&self, r: &Value, signer: Option<&DeviceKeys>) -> u16 {
            self.send("POST", wire::telemetry::TELEMETRY, &serde_json::to_vec(r).unwrap(), signer).await
        }

        /// What reached PostHog since the last call.
        async fn events(&mut self) -> Vec<Value> {
            self.app.analytics.flush().await;
            let mut all = vec![];
            while let Ok(b) = self.got.try_recv() {
                all.extend(b["batch"].as_array().unwrap().iter().cloned());
            }
            all
        }
    }

    fn report(install: &str) -> Value {
        json!({
            "install": install, "version": "0.43.0", "os": "linux", "arch": "x86_64",
            "channel": "script", "control": "hosted",
            "events": [
                { "event": "opted-in", "via": "cli" },
                { "event": "started", "service": true },
                { "event": "daily", "panes_opened": 7, "panes_peak": 3,
                  "agents_started": { "claude": 2 }, "blocks_opened": { "editor": 1 },
                  "clients": { "web": 4 }, "views": { "swarm": 5, "getting-started": 1 },
                  "flags_on": ["chat", "guest-ssh"] },
                { "event": "something-newer" },
            ],
            "crashes": [{ "process": "daemon", "version": "0.42.0", "location": "crates/mux/src/pane.rs:812",
                          "frames": ["arugula_mux::pane::Pane::resize"], "at": 5 }],
        })
    }

    /// A signed report goes on the daemon's account, every event mapped.
    #[tokio::test]
    async fn telemetry_signed_lands_on_the_account() {
        let mut c = control().await;
        let d = c.daemon();
        assert_eq!(c.report(&report("0123456789abcdef"), Some(&d)).await, 200);
        let ev = c.events().await;
        let names: Vec<&str> = ev.iter().map(|e| e["event"].as_str().unwrap()).collect();
        assert_eq!(names, ["install opted in", "install started", "install day", "crash"]);
        for e in &ev {
            assert_eq!(e["distinct_id"], "acct1");
            let p = &e["properties"];
            assert_eq!(p["install"], "0123456789abcdef");
            assert_eq!(p["machine"], d.id());
            assert_eq!(
                (&p["os"], &p["arch"], &p["channel"], &p["control"]),
                (&json!("linux"), &json!("x86_64"), &json!("script"), &json!("hosted"))
            );
            assert!(p.get("$process_person_profile").is_none());
        }
        assert_eq!(ev[0]["properties"]["via"], "cli");
        assert_eq!(ev[1]["properties"]["service"], true);
        let day = &ev[2]["properties"];
        assert_eq!(day["version"], "0.43.0");
        assert_eq!(day["panes_opened"], 7);
        assert_eq!(day["agents_started_claude"], 2);
        assert_eq!(day["blocks_opened_editor"], 1);
        assert_eq!(day["clients_web"], 4);
        assert_eq!(day["view_swarm"], 5);
        assert_eq!(day["view_getting_started"], 1);
        assert_eq!(day["flags_on"], json!(["chat", "guest-ssh"]));
        let crash = &ev[3]["properties"];
        assert_eq!((&crash["version"], &crash["install_version"]), (&json!("0.42.0"), &json!("0.43.0")));
        assert_eq!(crash["location"], "crates/mux/src/pane.rs:812");
        assert_eq!(crash["frames"], json!(["arugula_mux::pane::Pane::resize"]));
    }

    /// An unsigned report goes on its install, with no person made, and a
    /// badly signed one is refused.
    #[tokio::test]
    async fn telemetry_unsigned_lands_on_the_install() {
        let mut c = control().await;
        assert_eq!(c.report(&report("00000000000000aa"), None).await, 200);
        let ev = c.events().await;
        assert_eq!(ev.len(), 4);
        for e in &ev {
            assert_eq!(e["distinct_id"], "install:00000000000000aa");
            assert_eq!(e["properties"]["$process_person_profile"], false);
            assert!(e["properties"].get("machine").is_none());
        }
        let stranger = DeviceKeys::generate();
        assert_eq!(c.report(&report("00000000000000aa"), Some(&stranger)).await, 401);
        assert!(c.events().await.is_empty());
    }

    #[tokio::test]
    async fn telemetry_malformed_is_refused() {
        let mut c = control().await;
        let path = wire::telemetry::TELEMETRY;
        assert_eq!(c.send("POST", path, b"not json", None).await, 400);
        assert_eq!(c.report(&json!({ "install": "0123456789abcdef" }), None).await, 400);
        let mut bad = report("0123456789abcdef");
        bad["install"] = "NOT-HEX".into();
        assert_eq!(c.report(&bad, None).await, 400);
        let mut path_crash = report("0123456789abcdef");
        path_crash["crashes"][0]["location"] = "/home/someone/x.rs:1".into();
        assert_eq!(c.report(&path_crash, None).await, 400);
        assert_eq!(c.send("POST", path, &vec![b' '; MAX_BODY + 1], None).await, 413);
        assert!(c.events().await.is_empty());
    }

    /// Unsigned reports are limited per install and per client address.
    #[tokio::test]
    async fn telemetry_unsigned_past_the_limit() {
        let c = control().await;
        for _ in 0..limit::INSTALL_REPORTS.1 {
            assert_eq!(c.report(&report("00000000000000bb"), None).await, 200);
        }
        assert_eq!(c.report(&report("00000000000000bb"), None).await, 429);
        assert_eq!(c.report(&report("00000000000000cc"), None).await, 200, "another install");
        let here = "127.0.0.1".parse().unwrap();
        while c.app.limits.check(limit::TELEMETRY, here).is_ok() {}
        assert_eq!(c.report(&report("00000000000000dd"), None).await, 429, "this address sent its share");
        let d = c.daemon();
        assert_eq!(c.report(&report("00000000000000dd"), Some(&d)).await, 200, "a signed one counts per daemon");
    }

    /// A trust request with a new version sends `machine version` once.
    #[tokio::test]
    async fn telemetry_machine_version_when_it_changes() {
        let mut c = control().await;
        let d = c.daemon();
        let at = |v: &str| format!("{}&version={v}&os=linux&arch=aarch64", wire::Features::path(""));
        assert_eq!(c.send("GET", &wire::Features::path(""), b"", Some(&d)).await, 200);
        assert!(c.events().await.is_empty(), "an older daemon says nothing");
        assert_eq!(c.send("GET", &at("0.43.0"), b"", Some(&d)).await, 200);
        let ev = c.events().await;
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0]["event"], "machine version");
        assert_eq!(ev[0]["distinct_id"], "acct1");
        let p = &ev[0]["properties"];
        assert_eq!(
            (&p["machine"], &p["version"], &p["os"], &p["arch"]),
            (&json!(d.id()), &json!("0.43.0"), &json!("linux"), &json!("aarch64"))
        );
        assert_eq!(c.send("GET", &at("0.43.0"), b"", Some(&d)).await, 200);
        assert!(c.events().await.is_empty(), "the same again");
        assert_eq!(c.send("GET", &at("0.44.0"), b"", Some(&d)).await, 200);
        assert_eq!(c.events().await.len(), 1);
        let odd = format!("{}&version=0.44.0&os=plan9&arch=riscv64", wire::Features::path(""));
        assert_eq!(c.send("GET", &odd, b"", Some(&d)).await, 200);
        assert_eq!(c.events().await[0]["properties"]["os"], "other");
        let junk = format!("{}&version=%3Cscript%3E", wire::Features::path(""));
        assert_eq!(c.send("GET", &junk, b"", Some(&d)).await, 200, "a version that doesn't look like one is left out");
        assert!(c.events().await.is_empty());
    }

    /// A signed-in person's request (the session cookie `tok`, made here),
    /// from control's own page: its status and answer.
    async fn as_person(c: &Control, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = c
            .http
            .request(method.parse().unwrap(), format!("{}{path}", c.base))
            .header("cookie", format!("{}=tok", crate::auth::SESSION_COOKIE))
            .header("origin", &c.base);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let res = req.send().await.unwrap();
        (res.status().as_u16(), res.json().await.unwrap_or(Value::Null))
    }

    fn signed_in(c: &Control) {
        let now = now_ms();
        c.app.db.account_for("github", "gh-1", "someone", "acct1", now).unwrap();
        c.app.db.add_session(&crate::auth::hash("tok"), "acct1", now, now + 60_000).unwrap();
    }

    const ME: &str = wire::telemetry::ME_TELEMETRY;
    const PAGE: &str = wire::telemetry::PAGE;

    /// Not asked until it answers; yes is kept (and said once), and no too.
    #[tokio::test]
    async fn telemetry_page_consent() {
        let mut c = control().await;
        signed_in(&c);
        assert_eq!(c.send("GET", ME, b"", None).await, 401, "signed in only");
        let (st, got) = as_person(&c, "GET", ME, None).await;
        assert_eq!(st, 200);
        assert_eq!(got, json!({ "on": null, "at": null, "ask": true }));
        let (st, got) = as_person(&c, "POST", ME, Some(json!({ "on": true }))).await;
        assert_eq!((st, &got["on"]), (200, &json!(true)));
        assert!(got["at"].as_u64().unwrap() > 0);
        assert_eq!(as_person(&c, "GET", ME, None).await.1["on"], true);
        let ev = c.events().await;
        assert_eq!(ev.len(), 1, "no control request for it: {ev:?}");
        assert_eq!((&ev[0]["event"], &ev[0]["distinct_id"]), (&json!("page opted in"), &json!("acct1")));
        assert_eq!(as_person(&c, "POST", ME, Some(json!({ "on": false }))).await.0, 200);
        assert_eq!(as_person(&c, "GET", ME, None).await.1["on"], false);
        assert!(c.events().await.is_empty(), "saying no sends nothing");
        // Another site's page can't answer for it.
        let other = c
            .http
            .post(format!("{}{ME}", c.base))
            .header("cookie", format!("{}=tok", crate::auth::SESSION_COOKIE))
            .header("origin", "https://elsewhere.example")
            .json(&json!({ "on": true }))
            .send()
            .await
            .unwrap();
        assert_eq!(other.status().as_u16(), 403);
        assert_eq!(as_person(&c, "GET", ME, None).await.1["on"], false);
    }

    /// The page's counts are refused until the account says yes, go out as
    /// `page day` once it has, and are refused again after no.
    #[tokio::test]
    async fn telemetry_page_only_after_yes() {
        let mut c = control().await;
        signed_in(&c);
        let counts = json!({ "views": { "swarm": 3, "getting-started": 1, "some-newer-view": 2 }, "web_errors": 1 });
        assert_eq!(c.send("POST", PAGE, &serde_json::to_vec(&counts).unwrap(), None).await, 401);
        let (st, got) = as_person(&c, "POST", PAGE, Some(counts.clone())).await;
        assert_eq!((st, &got["reason"]), (403, &json!(wire::telemetry::NOT_OPTED_IN)), "not asked is no");
        assert!(c.events().await.is_empty());

        as_person(&c, "POST", ME, Some(json!({ "on": true }))).await;
        c.events().await;
        assert_eq!(as_person(&c, "POST", PAGE, Some(counts.clone())).await.0, 200);
        let ev = c.events().await;
        assert_eq!(ev.len(), 1, "{ev:?}");
        assert_eq!((&ev[0]["event"], &ev[0]["distinct_id"]), (&json!("page day"), &json!("acct1")));
        let p = &ev[0]["properties"];
        assert_eq!((&p["view_swarm"], &p["view_getting_started"], &p["view_other"]), (&json!(3), &json!(1), &json!(2)));
        assert_eq!(p["web_errors"], 1);
        assert!(p.get("install").is_none() && p.get("machine").is_none());
        assert_eq!(as_person(&c, "POST", PAGE, Some(json!({ "views": { "chat": 1_000_000 } }))).await.0, 200);
        assert_eq!(c.events().await[0]["properties"]["view_chat"], MAX_PAGE_COUNT, "a count is capped");
        assert_eq!(as_person(&c, "POST", PAGE, Some(json!({}))).await.0, 200);
        assert!(c.events().await.is_empty(), "nothing counted, nothing sent");
        assert_eq!(as_person(&c, "POST", PAGE, Some(json!({ "views": "swarm" }))).await.0, 400);

        as_person(&c, "POST", ME, Some(json!({ "on": false }))).await;
        let (st, got) = as_person(&c, "POST", PAGE, Some(counts)).await;
        assert_eq!((st, &got["reason"]), (403, &json!(wire::telemetry::NOT_OPTED_IN)), "no stops them");
        assert!(c.events().await.is_empty());
    }

    /// Limited per account.
    #[tokio::test]
    async fn telemetry_page_past_the_limit() {
        let c = control().await;
        signed_in(&c);
        as_person(&c, "POST", ME, Some(json!({ "on": true }))).await;
        for _ in 0..limit::PAGE_REPORTS.1 {
            assert_eq!(as_person(&c, "POST", PAGE, Some(json!({ "web_errors": 1 }))).await.0, 200);
        }
        assert_eq!(as_person(&c, "POST", PAGE, Some(json!({ "web_errors": 1 }))).await.0, 429);
    }
}
