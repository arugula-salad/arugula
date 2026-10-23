//! The inference broker (M231) through the platform's routes, against a
//! fake Anthropic: whose key pays (a project's, an account's own, a
//! sponsor's or visitor's a client names, the platform's under a cap), a
//! client's consent, the lobby-shaped client, reservations and how a call
//! ends, the spend it shows, and that no key ever comes back. On SQLite
//! and, with `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_e2e::team::TeamRole;
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::StreamExt as _;
use serde_json::{Value, json};

use super::{
    Config, Key,
    clients::{self, Clients},
    prices,
};
use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::inference::Call,
    hosted::inference::{Caps, Settings},
    secrets::SecretCipher,
};

/// Test values, not keys.
const PLATFORM_KEY: &str = "sk-ant-api03-platform-test-0123456789abcdef"; // gitleaks:allow: a test value
const PROJECT_KEY: &str = "sk-ant-api03-project-test-0123456789abcdef"; // gitleaks:allow: a test value
const OWN_KEY: &str = "sk-ant-api03-own-test-0123456789abcdef"; // gitleaks:allow: a test value
const OAUTH: &str = "sk-ant-oat01-sponsor-test-0123456789abcdef"; // gitleaks:allow: a test value
const VISITOR_KEY: &str = "sk-ant-api03-visitor-test-0123456789abcdef"; // gitleaks:allow: a test value
const REVOKED: &str = "sk-ant-api03-revoked-test-0123456789abcdef"; // gitleaks:allow: a test value
const STUDIO: &str = "studio-client-credential-test-0123456789"; // gitleaks:allow: a test value
const HUD: &str = "hud-client-credential-test-0123456789"; // gitleaks:allow: a test value

/// Every secret here: none may come back to a caller or be kept in the
/// clear.
const SECRETS: [&str; 8] = [PLATFORM_KEY, PROJECT_KEY, OWN_KEY, OAUTH, VISITOR_KEY, REVOKED, STUDIO, HUD];

/// A request the fake Anthropic took.
#[derive(Debug, Clone)]
struct Seen {
    path: String,
    headers: HeaderMap,
    body: Value,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// The credential it carried: `x-api-key`, or the bearer.
    fn key(&self) -> String {
        self.header("x-api-key")
            .map(str::to_owned)
            .or_else(|| self.header("authorization").map(|b| b.trim_start_matches("Bearer ").to_owned()))
            .unwrap_or_default()
    }
}

#[derive(Default)]
struct Fake {
    seen: Mutex<Vec<Seen>>,
}

impl Fake {
    fn last(&self) -> Seen {
        self.seen.lock().unwrap().last().cloned().expect("a request reached Anthropic")
    }

    fn count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    fn paths(&self) -> Vec<String> {
        self.seen.lock().unwrap().iter().map(|s| s.path.clone()).collect()
    }
}

/// What the fake streams for a request of `model` that asked for
/// `max_tokens`: 100 input tokens in, `max_tokens` out.
fn events(model: &str, out: u64) -> String {
    format!("{}{}{}", start(model), delta(out), "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n")
}

fn start(model: &str) -> String {
    format!(
        "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_1\",\"model\":\"{model}\",\"usage\":{{\"input_tokens\":100,\"output_tokens\":1}}}}}}\n\n\
         event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"delta\":{{\"type\":\"text_delta\",\"text\":\"hi\"}}}}\n\n"
    )
}

fn delta(out: u64) -> String {
    format!("event: message_delta\ndata: {{\"type\":\"message_delta\",\"usage\":{{\"output_tokens\":{out}}}}}\n\n")
}

/// Headers a real answer carries that must never come back.
const NEVER_BACK: [(&str, &str); 5] = [
    ("set-cookie", "cf=1"),
    ("anthropic-organization-id", "org-secret"),
    ("anthropic-ratelimit-requests-limit", "50"),
    ("via", "1.1 upstream"),
    ("fly-request-id", "x"),
];

/// The fake's ways to answer a message, by a field of its body:
/// `slow` (the start, then nothing), `no_delta` (a stream that ends with no
/// final usage), `truncated` (half a JSON body), `hold_ms` (wait before
/// answering), `bad` (a 400), `searches` (web searches in the usage).
async fn messages(State(f): State<Arc<Fake>>, headers: HeaderMap, req: Request) -> Response {
    let path = req.uri().path().to_owned();
    let body: Value =
        serde_json::from_slice(&axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap()).unwrap_or_default();
    let seen = Seen { path: path.clone(), headers, body: body.clone() };
    f.seen.lock().unwrap().push(seen.clone());
    if let Some(ms) = body["hold_ms"].as_u64() {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
    if seen.key().contains("revoked") {
        let e = json!({ "type": "error", "error": { "type": "authentication_error", "message": "invalid x-api-key" } });
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    if path.ends_with("count_tokens") {
        return Json(json!({ "input_tokens": 7 })).into_response();
    }
    if body["bad"] == true {
        let e = json!({ "type": "error", "error": { "type": "invalid_request_error", "message": "no" } });
        return (StatusCode::BAD_REQUEST, Json(e)).into_response();
    }
    let model = body["model"].as_str().unwrap_or("claude-opus-5").to_owned();
    let out = body["max_tokens"].as_u64().unwrap_or(10);
    // A provider that says the key back in a header, and the headers a
    // real answer carries: none of them comes back.
    let mut back: Vec<(String, String)> = NEVER_BACK.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect();
    back.push(("x-echo".into(), seen.key()));
    back.push(("request-id".into(), "req_1".into()));
    let res = |status: StatusCode, ctype: &str, body: Body| {
        let mut r = Response::builder().status(status).header("content-type", ctype);
        for (k, v) in &back {
            r = r.header(k, v);
        }
        r.body(body).unwrap()
    };
    if body["slow"] == true {
        let first = Bytes::from(start(&model));
        let s = futures_util::stream::once(async move { Ok::<_, std::io::Error>(first) })
            .chain(futures_util::stream::pending());
        return res(StatusCode::OK, "text/event-stream", Body::from_stream(s));
    }
    if body["no_delta"] == true {
        return res(StatusCode::OK, "text/event-stream", Body::from(start(&model)));
    }
    if body["truncated"] == true {
        let half = Bytes::from_static(br#"{"type":"message","model":"claude-opus-5","usage":{"input_tokens":100,"out"#);
        let s = futures_util::stream::iter([Ok(half), Err(std::io::Error::other("cut"))]);
        return res(StatusCode::OK, "application/json", Body::from_stream(s));
    }
    if body["stream"] == true {
        return res(StatusCode::OK, "text/event-stream", Body::from(events(&model, out)));
    }
    let searches = body["searches"].as_u64().unwrap_or(0);
    let m = json!({ "type": "message", "id": "msg_2", "model": model, "content": [{ "type": "text", "text": "hi" }],
                    "usage": { "input_tokens": 100, "output_tokens": out, "cache_read_input_tokens": 1000,
                               "server_tool_use": { "web_search_requests": searches } } });
    res(StatusCode::OK, "application/json", Body::from(m.to_string()))
}

async fn models(State(f): State<Arc<Fake>>, headers: HeaderMap, req: Request) -> Response {
    let redirect = req.uri().query().is_some_and(|q| q.contains("redirect"));
    f.seen.lock().unwrap().push(Seen { path: req.uri().path().to_owned(), headers, body: Value::Null });
    if redirect {
        return Response::builder().status(307).header("location", "/stolen").body(Body::empty()).unwrap();
    }
    Json(json!({ "data": [{ "id": "claude-opus-5" }] })).into_response()
}

async fn stolen(State(f): State<Arc<Fake>>, headers: HeaderMap) -> &'static str {
    f.seen.lock().unwrap().push(Seen { path: "/stolen".into(), headers, body: Value::Null });
    "stolen"
}

/// A fake Anthropic: its URL.
async fn anthropic(f: Arc<Fake>) -> String {
    let r = Router::new()
        .route("/v1/messages", post(messages))
        .route("/v1/messages/count_tokens", post(messages))
        .route("/v1/models", get(models))
        .route("/v1/models/{id}", get(models))
        .route("/stolen", get(stolen))
        .with_state(f);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
    url
}

struct Rig {
    app: Arc<Platform>,
    fake: Arc<Fake>,
    base: String,
    /// Team t1's project: alice owns it, bob edits it (and is on the
    /// allowlist), vic views it. dana and erin are accounts of team t2.
    project: String,
    /// Every answer a caller got, headers and body, to look for keys in.
    said: Mutex<Vec<String>>,
}

/// The broker with the platform's key, the allowlist (bob), two calls at
/// once on it, one beta allowed there, and two clients: studio (the
/// platform's key pays for its accounts up to $20 a month, and it names
/// sponsors and visitors) and hud (neither).
async fn rig() -> Rig {
    let hub = Arc::new(TestHub::default());
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("vic", TeamRole::Viewer)]);
    hub.team("t2", &[("dana", TeamRole::Owner), ("erin", TeamRole::Editor)]);
    let fake = Arc::new(Fake::default());
    let upstream = anthropic(fake.clone()).await;
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, crate::db::Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    let registered = clients::parse(&[
        format!("studio:{}:20:payers", hex::encode(clients::digest(STUDIO))),
        format!("hud:{}", hex::encode(clients::digest(HUD))),
    ])
    .unwrap();
    p.hosted = crate::hosted::Config {
        cipher: Some(Arc::new(SecretCipher::new(&"01".repeat(32)).unwrap())),
        inference: Settings::new(&["bob".into()], &[], Caps { concurrent: 2, daily: 100, minutes: 120 }),
        broker: Config {
            key: Some(Key::new(PLATFORM_KEY)),
            upstream,
            monthly_usd: 20.0,
            concurrent: 2,
            platform_betas: vec!["fine-grained-tool-streaming-2025-05-14".into()],
            clients: Clients::new(registered),
            http: super::http(),
        },
        ..Default::default()
    };
    let app = Arc::new(p);
    let project = app.db.team_project("t1", "t1", 0).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    Rig { app, fake, base, project, said: Mutex::new(vec![]) }
}

/// Who calls: a member by their session, or a client by its credential
/// for an account, naming a sponsor and a visitor when it does.
#[derive(Clone, Copy)]
enum As<'a> {
    Member(&'a str),
    Client { credential: &'a str, account: Option<&'a str>, sponsor: Option<&'a str>, visitor: Option<&'a str> },
}

fn studio(account: &str) -> As<'_> {
    As::Client { credential: STUDIO, account: Some(account), sponsor: None, visitor: None }
}

impl Rig {
    /// `method path` under the broker's base for `project`, as `who`: the
    /// status, the headers and the body.
    async fn send(
        &self,
        who: As<'_>,
        project: &str,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, HeaderMap, String) {
        let url = format!("{}/api/projects/{project}/inference/anthropic{path}", self.base);
        self.raw(who, method, &url, body.map(|b| b.to_string().into_bytes())).await
    }

    fn request(&self, who: As<'_>, method: &str, url: &str) -> reqwest::RequestBuilder {
        let mut req = reqwest::Client::new()
            .request(method.parse().unwrap(), url)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14")
            .header("user-agent", "claude-cli/2.1")
            .header("x-custom", "dropped")
            .header("referer", "https://evil.example/page")
            // What a box's agent holds: a stand-in the broker drops.
            .header("x-api-key", "arugula-brokered");
        match who {
            // A member's page is control's own (the hub refuses a session
            // from another origin); a client's request may say anything.
            As::Member(a) => {
                req =
                    req.header("cookie", format!("ilg_session=session-{a}")).header("origin", "https://control.example")
            }
            As::Client { credential, account, sponsor, visitor } => {
                req = req.bearer_auth(credential).header("origin", "https://evil.example");
                for (h, v) in [(super::ACCOUNT, account), (super::SPONSOR, sponsor), (super::VISITOR, visitor)] {
                    if let Some(v) = v {
                        req = req.header(h, v);
                    }
                }
            }
        }
        req
    }

    async fn raw(&self, who: As<'_>, method: &str, url: &str, body: Option<Vec<u8>>) -> (u16, HeaderMap, String) {
        let mut req = self.request(who, method, url);
        if let Some(b) = body {
            req = req.header("content-type", "application/json").body(b);
        }
        let r = req.send().await.unwrap();
        let status = r.status().as_u16();
        let headers = r.headers().clone();
        let text = r.text().await.unwrap();
        self.said.lock().unwrap().push(format!("{headers:?} {text}"));
        (status, headers, text)
    }

    /// One streamed message of `model` with `out` output tokens, as `who`.
    async fn ask(&self, who: As<'_>, model: &str, out: u64) -> (u16, HeaderMap, String) {
        self.ask_with(who, json!({ "model": model, "max_tokens": out, "stream": true })).await
    }

    async fn ask_with(&self, who: As<'_>, mut body: Value) -> (u16, HeaderMap, String) {
        body["messages"] = json!([{ "role": "user", "content": "hi" }]);
        self.send(who, &self.project, "POST", "/v1/messages", Some(body)).await
    }

    /// `who` sets `project`'s own Anthropic key.
    async fn key(&self, who: &str, project: &str, key: &str) {
        let (status, v) = call(
            &self.base,
            who,
            "PUT",
            &format!("/api/projects/{project}/inference-keys/anthropic"),
            Some(json!({ "value": key })),
        )
        .await;
        assert_eq!(status, 200, "{v}");
        self.said.lock().unwrap().push(v.to_string());
    }

    /// `who` lets `client` act for them (or stops it).
    async fn consent(&self, who: &str, client: &str, on: bool) -> (u16, Value) {
        call(
            &self.base,
            who,
            if on { "PUT" } else { "DELETE" },
            &format!("/api/inference/clients/{client}/consent"),
            None,
        )
        .await
    }

    fn calls(&self) -> Vec<Call> {
        let ids = self.app.db.call_ids().unwrap();
        ids.iter().map(|id| self.app.db.call(id).unwrap().unwrap()).collect()
    }

    /// The broker ended its `n`th call: wait for it (its task ends it).
    async fn ended(&self, n: usize) -> Call {
        for _ in 0..300 {
            if let Some(c) = self.calls().get(n - 1)
                && c.state != "open"
            {
                return c.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("call {n} never ended: {:?}", self.calls());
    }

    /// Nothing a caller was told, and nothing kept here, holds a secret.
    fn nothing_leaked(&self) {
        for said in self.said.lock().unwrap().iter() {
            for s in SECRETS {
                assert!(!said.contains(s), "a caller was told a secret: {said}");
            }
        }
        for s in SECRETS {
            assert!(self.app.db.mentions(s).is_empty(), "kept in the clear: {:?}", self.app.db.mentions(s));
        }
        let debug = format!("{:?}", self.app.hosted);
        for s in SECRETS {
            assert!(!debug.contains(s), "{debug}");
        }
    }
}

fn reason(body: &str) -> String {
    serde_json::from_str::<Value>(body).unwrap()["arugula"]["reason"].as_str().unwrap_or_default().to_owned()
}

fn pays(h: &HeaderMap) -> &str {
    h.get(super::PAYS).and_then(|v| v.to_str().ok()).unwrap_or_default()
}

/// What a streamed message of `model` with `out` output tokens costs: 100
/// tokens in, `out` out.
fn cost(model: &str, out: u64) -> u64 {
    prices::micros(prices::cost_of(model, &prices::Usage { input: 100, output: out, ..Default::default() }).0)
}

/// $25: a million output tokens of Opus 5, past a $20 cap in one call.
const BIG: u64 = 1_000_000;

/// A member's call with no key that pays is refused, saying how to add
/// one; with the project's own key it streams through as Anthropic sent
/// it, with only the allowlisted headers each way, and is reserved, then
/// settled at its final usage against the project and the member. A
/// viewer never spends the project's key. Count-tokens and models go
/// through unmetered; anything else isn't forwarded; Claude Code's hello
/// needs nothing.
#[tokio::test]
async fn a_member_calls_on_the_projects_own_key() {
    let r = rig().await;
    let p = r.project.clone();
    let (status, _, body) = r.ask(As::Member("alice"), "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "no_key"), "{body}");
    assert!(body.contains(&format!("arugula cloud key set anthropic --project {p}")), "{body}");
    assert_eq!(r.fake.count(), 0, "never sent");
    assert!(r.calls().is_empty(), "nor reserved");
    assert_eq!(r.send(As::Member("dana"), &p, "POST", "/v1/messages", Some(json!({}))).await.0, 404, "not a member");
    let (status, _, body) = r.send(As::Member("nobody"), &p, "POST", "/v1/messages", Some(json!({}))).await;
    assert_eq!((status, reason(&body).as_str()), (404, "no_project"));

    r.key("alice", &p, PROJECT_KEY).await;
    let (status, _, body) = r.ask(As::Member("vic"), "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "no_key"), "a viewer: {body}");
    assert!(body.contains("a viewer of project"), "{body}");

    let (status, h, body) = r.ask(As::Member("alice"), "claude-opus-5", 10).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, events("claude-opus-5", 10), "streamed through as sent");
    assert_eq!(pays(&h), "project");
    assert_eq!(h.get("request-id").and_then(|v| v.to_str().ok()), Some("req_1"), "what the SDKs read");
    for (name, _) in NEVER_BACK.iter().chain(&[("x-echo", "")]) {
        assert!(h.get(*name).is_none(), "{name} came back");
    }
    let seen = r.fake.last();
    assert_eq!(seen.key(), PROJECT_KEY);
    assert_eq!(seen.header("accept-encoding"), Some("identity"), "read as it streams");
    assert_eq!(
        seen.header("anthropic-beta"),
        Some("fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14"),
        "a project's key takes the betas asked"
    );
    assert_eq!(seen.header("user-agent"), Some("claude-cli/2.1"));
    for name in ["cookie", "authorization", "x-custom", "origin", "referer", super::ACCOUNT] {
        assert!(seen.header(name).is_none(), "{name} went upstream");
    }
    assert_eq!(seen.body["model"], "claude-opus-5");

    let c = r.ended(1).await;
    assert_eq!(
        (c.project.as_str(), c.account.as_deref(), c.client.as_deref(), c.pays.as_str(), c.state.as_str()),
        (p.as_str(), Some("alice"), None, "project", "settled")
    );
    assert_eq!((c.input, c.output, c.status, c.model.as_deref()), (100, 10, 200, Some("claude-opus-5")));
    assert!(c.reserved_micros > 0, "reserved before it went out: {c:?}");
    assert_eq!(c.cost_micros, cost("claude-opus-5", 10));

    // Not streamed: one JSON message, settled the same way; its web
    // searches priced.
    let (status, h, body) =
        r.ask_with(As::Member("bob"), json!({ "model": "claude-haiku-4-5", "max_tokens": 5, "searches": 2 })).await;
    assert_eq!((status, pays(&h)), (200, "project"), "an editor spends it: {body}");
    let c = r.ended(2).await;
    assert_eq!((c.input, c.output, c.cache_read, c.web_searches, c.state.as_str()), (100, 5, 1000, 2, "settled"));
    assert_eq!(c.priced_as.as_deref(), Some("claude-haiku-4-5"));
    let usage = prices::Usage { input: 100, output: 5, cache_read: 1000, web_searches: 2, ..Default::default() };
    assert_eq!(c.cost_micros, prices::micros(prices::cost_of("claude-haiku-4-5", &usage).0));

    let (status, _, body) = r.send(As::Member("alice"), &p, "POST", "/v1/messages/count_tokens", Some(json!({}))).await;
    assert_eq!((status, body.as_str()), (200, r#"{"input_tokens":7}"#));
    assert_eq!(r.send(As::Member("alice"), &p, "GET", "/v1/models?limit=1", None).await.0, 200);
    assert_eq!(r.fake.last().path, "/v1/models");
    assert_eq!(r.send(As::Member("alice"), &p, "GET", "/v1/models/claude-opus-5", None).await.0, 200);
    assert_eq!(r.fake.last().path, "/v1/models/claude-opus-5");
    for (m, path) in [
        ("POST", "/v1/complete"),
        ("GET", "/v1/messages"),
        ("DELETE", "/v1/models"),
        ("GET", "/v1/models/-x"),
        ("GET", "/v1/models/.x"),
        ("GET", "/v1/models/_x"),
    ] {
        let (status, _, body) = r.send(As::Member("alice"), &p, m, path, None).await;
        assert!(status == 404 || status == 405, "{m} {path}: {status} {body}");
    }
    let (status, _, _) = r.send(As::Member("nobody"), &p, "HEAD", "/api/hello", None).await;
    assert_eq!(status, 200, "Claude Code's hello needs no credential");
    assert_eq!(r.calls().len(), 2, "only messages are metered");

    // What it cost, for the project's members.
    let (status, v) = call(&r.base, "vic", "GET", &format!("/api/projects/{p}/inference/spend"), None).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["lines"].as_array().unwrap().len(), 2, "{v}");
    assert_eq!(v["platform_usd"], 0.0);
    assert!(v["usd"].as_f64().unwrap() > 0.0, "{v}");
    assert_eq!(call(&r.base, "dana", "GET", &format!("/api/projects/{p}/inference/spend"), None).await.0, 404);
    assert_eq!(call(&r.base, "vic", "GET", &format!("/api/projects/{p}/inference/spend?month=13"), None).await.0, 400);
    let (_, v) = call(&r.base, "vic", "GET", &format!("/api/projects/{p}/inference/spend?month=2001-01"), None).await;
    assert_eq!(v["lines"], json!([]));
    r.nothing_leaked();
}

/// An account on the allowlist with no key runs on the platform's key
/// until its month's calls cost the cap, then is told when it starts
/// again; a call running counts at its reservation, so a second can't
/// overshoot while the first is under way; and only so many run at once.
/// On it, only the allowed betas go, and the long-context ones are
/// refused. Its own key then pays, with no cap. An account not on the
/// allowlist never gets the platform's key from the broker.
#[tokio::test]
async fn the_platforms_key_pays_for_the_allowlist_under_a_monthly_cap() {
    let r = rig().await;
    let (status, h, body) = r.ask(As::Member("bob"), "claude-opus-5", 10).await;
    assert_eq!((status, pays(&h)), (200, "platform"), "{body}");
    let seen = r.fake.last();
    assert_eq!(seen.key(), PLATFORM_KEY);
    assert_eq!(seen.header("anthropic-beta"), Some("fine-grained-tool-streaming-2025-05-14"), "only the allowed betas");
    let (status, _, body) = r.ask_with(As::Member("bob"), json!({ "model": "claude-opus-5", "max_tokens": 10 })).await;
    assert_eq!(status, 200, "{body}");
    let url = format!("{}/api/projects/{}/inference/anthropic/v1/messages", r.base, r.project);
    let res = r
        .request(As::Member("bob"), "POST", &url)
        .header("anthropic-beta", "context-1m-2025-08-07")
        .json(&json!({ "model": "claude-opus-5", "max_tokens": 10 }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
    assert_eq!(reason(&res.text().await.unwrap()), "beta", "the long-context premium isn't on the platform's key");

    // Two held calls: the third is busy (two at once), and is retried.
    let held = |out: u64| {
        let r = &r;
        async move {
            r.ask_with(As::Member("bob"), json!({ "model": "claude-opus-5", "max_tokens": out, "hold_ms": 1500 })).await
        }
    };
    let (a, b, c) = tokio::join!(held(10), held(10), async {
        tokio::time::sleep(Duration::from_millis(400)).await;
        r.ask(As::Member("bob"), "claude-opus-5", 10).await
    });
    assert_eq!((a.0, b.0), (200, 200));
    assert_eq!((c.0, reason(&c.2).as_str()), (429, "busy"), "{}", c.2);

    // A call reserved at $25 is allowed under the cap; while it runs, the
    // next counts its reservation and is refused: no overshoot.
    let sent = r.fake.count();
    let (big, next) = tokio::join!(held(BIG), async {
        tokio::time::sleep(Duration::from_millis(400)).await;
        r.ask(As::Member("bob"), "claude-opus-5", 10).await
    });
    assert_eq!(big.0, 200);
    assert_eq!((next.0, reason(&next.2).as_str()), (403, "cap"), "{}", next.2);
    assert_eq!(r.fake.count(), sent + 1, "refused before it went out");
    let said: Value = serde_json::from_str(&next.2).unwrap();
    let now = arugula_e2e::now_ms();
    assert_eq!(said["arugula"]["resets"], prices::day_of(prices::next_month(now)));
    assert_eq!(said["arugula"]["cap_usd"], 20.0);
    assert!(said["error"]["message"].as_str().unwrap().contains("bob has used this month's $20"), "{}", next.2);
    // Settled, it's still past the cap.
    let n = r.calls().len();
    let settled = r.ended(n).await;
    let usage = prices::Usage { input: 100, output: BIG, cache_read: 1000, ..Default::default() };
    let big_cost = prices::micros(prices::cost_of("claude-opus-5", &usage).0);
    assert_eq!((settled.state.as_str(), settled.cost_micros), ("settled", big_cost), "its JSON's usage");
    assert_eq!(r.ask(As::Member("bob"), "claude-opus-5", 10).await.0, 403);

    let (_, v) = call(&r.base, "bob", "GET", "/api/inference/spend", None).await;
    assert_eq!((v["cap_usd"].as_f64(), v["client"].clone()), (Some(20.0), Value::Null), "{v}");
    assert!(v["spent_usd"].as_f64().unwrap() >= 25.0, "{v}");

    // A new month starts the count again.
    use crate::db::sql::Sql as _;
    r.app.db.c().execute("UPDATE inference_calls SET started_at = 1000", crate::db::sql::params![]).unwrap();
    assert_eq!(r.ask(As::Member("bob"), "claude-opus-5", 10).await.0, 200);

    r.key("bob", "personal", OWN_KEY).await;
    for _ in 0..2 {
        let (status, h, _) = r.ask(As::Member("bob"), "claude-opus-5", BIG).await;
        assert_eq!((status, pays(&h)), (200, "account bob"));
        assert_eq!(r.fake.last().key(), OWN_KEY);
    }

    let (status, _, body) = r.ask(As::Member("alice"), "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "no_key"), "not on the allowlist: {body}");
    let (_, v) = call(&r.base, "alice", "GET", "/api/inference/spend", None).await;
    assert_eq!(v["cap_usd"], 0.0);
    r.nothing_leaked();
}

/// A client acts for an account only once that account let it, on the
/// platform page; letting it can be taken back. A sponsor or visitor it
/// names must be an account that let it too, or the call is refused, not
/// passed over.
#[tokio::test]
async fn a_client_acts_only_for_accounts_that_let_it() {
    let r = rig().await;
    let (status, _, body) = r.ask(studio("alice"), "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "consent"), "{body}");
    assert!(body.contains("hasn't let studio"), "{body}");
    assert_eq!(r.fake.count(), 0);

    let (status, v) = call(&r.base, "alice", "GET", "/api/inference/clients", None).await;
    assert_eq!(status, 200);
    assert_eq!(
        v,
        json!([{ "client": "hud", "allowed": false, "names_payers": false },
                         { "client": "studio", "allowed": false, "names_payers": true }])
    );
    assert_eq!(r.consent("alice", "nope", true).await.0, 404);
    let (status, v) = r.consent("alice", "studio", true).await;
    assert_eq!((status, v[1]["allowed"].clone()), (200, json!(true)), "{v}");
    assert_eq!(r.ask(studio("alice"), "claude-opus-5", 10).await.0, 200);

    // Sponsors and visitors: unknown, or not consenting, is refused.
    let named = |sponsor: Option<&'static str>, visitor: Option<&'static str>| As::Client {
        credential: STUDIO,
        account: Some("alice"),
        sponsor,
        visitor,
    };
    let (status, _, body) = r.ask(named(Some("ghost"), None), "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "payer_unknown"), "{body}");
    r.key("dana", "personal", OAUTH).await;
    let (status, _, body) = r.ask(named(Some("dana"), None), "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "consent"), "dana's key needs dana's say: {body}");
    let (status, _, body) = r.ask(named(None, Some("erin")), "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "consent"), "{body}");
    let spend = format!("{}/api/inference/spend", r.base);
    let (status, _, body) = r.raw(studio("bob"), "GET", &spend, None).await;
    assert_eq!((status, reason(&body).as_str()), (403, "consent"), "nor its month: {body}");

    // Taken back: refused again at once.
    assert_eq!(r.consent("alice", "studio", false).await.0, 200);
    assert_eq!(reason(&r.ask(studio("alice"), "claude-opus-5", 10).await.2), "consent");
    r.nothing_leaked();
}

/// studio's lobby, as a registered client its accounts let act for them:
/// it names the account (a member of the project); the platform's key pays
/// for any of its accounts with no key of their own, up to its $20 a month
/// each, counted apart from the allowlist's; it names a sponsor whose own
/// key pays over the account's, and a visitor whose own key pays for their
/// own turn. The payer sees what others spent on their key. A client that
/// may not name payers can't, and one with no house cap gets no key for
/// an account off the allowlist. Its credential never reaches Anthropic.
#[tokio::test]
async fn a_registered_client_calls_for_its_accounts() {
    let r = rig().await;
    for who in ["alice", "bob", "vic", "dana", "erin"] {
        for client in ["studio", "hud"] {
            assert_eq!(r.consent(who, client, true).await.0, 200);
        }
    }
    let wrong = As::Client { credential: "not-a-client", account: Some("alice"), sponsor: None, visitor: None };
    let (status, _, body) = r.ask(wrong, "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (401, "client"), "{body}");
    let nobody = As::Client { credential: STUDIO, account: None, sponsor: None, visitor: None };
    assert_eq!(r.ask(nobody, "claude-opus-5", 10).await.0, 400);
    assert_eq!(r.ask(studio("dana"), "claude-opus-5", 10).await.0, 404, "dana isn't a member");

    // The lobby's house key: alice isn't on the allowlist, and pays nothing.
    let (status, h, body) = r.ask(studio("alice"), "claude-opus-5", BIG).await;
    assert_eq!((status, pays(&h)), (200, "platform"), "{body}");
    let seen = r.fake.last();
    assert_eq!(seen.key(), PLATFORM_KEY);
    assert!(!format!("{:?}", seen.headers).contains(STUDIO), "the client's credential never goes upstream");
    let c = r.ended(1).await;
    assert_eq!(
        (c.client.as_deref(), c.account.as_deref(), c.pays.as_str()),
        (Some("studio"), Some("alice"), "platform")
    );
    let (status, _, body) = r.ask(studio("alice"), "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "cap"), "{body}");
    // bob's allowlist month is his own: studio's spend for alice isn't in it.
    assert_eq!(r.ask(As::Member("bob"), "claude-opus-5", 10).await.0, 200);
    let url = format!("{}/api/inference/spend", r.base);
    let v: Value = serde_json::from_str(&r.raw(studio("alice"), "GET", &url, None).await.2).unwrap();
    assert_eq!((v["client"].as_str(), v["cap_usd"].as_f64()), (Some("studio"), Some(20.0)), "{v}");
    assert_eq!(v["spent_usd"].as_f64(), Some(25.0005), "{v}");

    // A sponsor's own key pays over the account's (the lobby's adoption).
    r.key("dana", "personal", OAUTH).await;
    r.key("alice", "personal", OWN_KEY).await;
    let sponsored = As::Client { credential: STUDIO, account: Some("alice"), sponsor: Some("dana"), visitor: None };
    let (status, h, body) = r.ask(sponsored, "claude-opus-5", 10).await;
    assert_eq!((status, pays(&h)), (200, "account dana"), "{body}");
    let seen = r.fake.last();
    assert_eq!(seen.header("authorization"), Some(format!("Bearer {OAUTH}").as_str()), "an OAuth token as a bearer");
    assert_eq!(
        seen.header("anthropic-beta"),
        Some("fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14,oauth-2025-04-20")
    );
    assert!(seen.header("x-api-key").is_none());
    // A sponsor with no key: the account's own pays.
    let sponsored = As::Client { credential: STUDIO, account: Some("alice"), sponsor: Some("erin"), visitor: None };
    assert_eq!(pays(&r.ask(sponsored, "claude-opus-5", 10).await.1), "account alice");
    // A visitor's own key pays for their turn first (the lobby's grants).
    r.key("erin", "personal", VISITOR_KEY).await;
    let visiting =
        As::Client { credential: STUDIO, account: Some("alice"), sponsor: Some("dana"), visitor: Some("erin") };
    let (status, h, _) = r.ask(visiting, "claude-opus-5", 10).await;
    assert_eq!((status, pays(&h)), (200, "account erin"));
    assert_eq!(r.fake.last().key(), VISITOR_KEY);
    let c = r.ended(5).await;
    assert_eq!((c.account.as_deref(), c.payer.as_deref(), c.pays.as_str()), (Some("alice"), Some("erin"), "account"));

    // dana sees what studio spent on her key, and for whom.
    let (status, v) = call(&r.base, "dana", "GET", "/api/inference/spend/paid", None).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["lines"][0]["account"], "alice", "{v}");
    assert_eq!(v["lines"][0]["client"], "studio");
    assert_eq!(v["lines"][0]["calls"], 1);

    // hud may not name payers, and has no house cap.
    let hud = As::Client { credential: HUD, account: Some("alice"), sponsor: Some("dana"), visitor: None };
    let (status, _, body) = r.ask(hud, "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "payers"), "{body}");
    let hud = As::Client { credential: HUD, account: Some("vic"), sponsor: None, visitor: None };
    let (status, _, body) = r.ask(hud, "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "no_key"), "{body}");
    let hud_bob = As::Client { credential: HUD, account: Some("bob"), sponsor: None, visitor: None };
    assert_eq!(pays(&r.ask(hud_bob, "claude-opus-5", 10).await.1), "platform", "bob's on the allowlist");

    // The project's spend shows each, by account, client and key.
    let (_, v) = call(&r.base, "alice", "GET", &format!("/api/projects/{}/inference/spend", r.project), None).await;
    let lines = v["lines"].as_array().unwrap();
    let line = |acct: &str, client: Value, pays: &str| {
        lines.iter().find(|l| l["account"] == acct && l["client"] == client && l["pays"] == pays).cloned()
    };
    assert_eq!(line("alice", json!("studio"), "account").unwrap()["calls"], 3, "{v}");
    assert!(line("alice", json!("studio"), "platform").is_some(), "{v}");
    assert!(line("bob", Value::Null, "platform").is_some(), "{v}");
    assert!(line("bob", json!("hud"), "platform").is_some(), "{v}");
    r.nothing_leaked();
}

/// No key ever comes back: Anthropic refusing one says whose, never it;
/// a header that repeats it is dropped; a redirect isn't followed; nothing
/// kept here holds one in the clear; and the settings never print one.
#[tokio::test]
async fn no_key_ever_comes_back() {
    let r = rig().await;
    r.key("alice", &r.project, REVOKED).await;
    let (status, _, body) = r.ask(As::Member("alice"), "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "key_refused"), "{body}");
    assert!(body.contains(&format!("project {}'s own key", r.project)), "{body}");
    assert_eq!(r.fake.last().key(), REVOKED, "it was the project's key that went");
    let c = r.ended(1).await;
    assert_eq!((c.state.as_str(), c.cost_micros, c.status), ("settled", 0, 401), "a refused call costs nothing");
    r.key("alice", &r.project, PROJECT_KEY).await;
    let (status, h, _) = r.ask(As::Member("alice"), "claude-opus-5", 10).await;
    assert_eq!(status, 200);
    assert!(h.get("x-echo").is_none());
    // A sponsor's own key that Anthropic refuses: whose it was.
    for who in ["vic", "dana"] {
        r.consent(who, "studio", true).await;
    }
    r.key("dana", "personal", REVOKED).await;
    let sponsored = As::Client { credential: STUDIO, account: Some("vic"), sponsor: Some("dana"), visitor: None };
    let (status, _, body) = r.ask(sponsored, "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "key_refused"), "{body}");
    assert!(body.contains("Anthropic refused dana's own key"), "{body}");
    // A redirect from upstream isn't followed with the key.
    let (status, h, _) = r.send(As::Member("alice"), &r.project, "GET", "/v1/models?redirect=1", None).await;
    assert_eq!(status, 307);
    assert!(h.get("location").is_none(), "only the allowlisted headers come back");
    assert!(!r.fake.paths().contains(&"/stolen".to_owned()), "never followed");
    // The settings, shown, show none of it.
    let debug = format!("{:?} {:?}", r.app.hosted.broker, Key::new(PLATFORM_KEY));
    assert!(!debug.contains(PLATFORM_KEY) && debug.contains("key: Some(\"set\")"), "{debug}");
    r.nothing_leaked();
}

/// A call settles only at the provider's final usage. A caller that goes
/// away mid-answer stops the call to Anthropic and leaves its reservation;
/// so does a stream that ends with no final usage, a JSON body cut short,
/// and a call dropped while it went out; an error answer costs nothing. A
/// request past Anthropic's ceiling is refused here.
#[tokio::test]
async fn a_call_without_its_final_usage_keeps_its_reservation() {
    let r = rig().await;
    r.key("alice", &r.project, PROJECT_KEY).await;
    let url = format!("{}/api/projects/{}/inference/anthropic/v1/messages", r.base, r.project);
    let reserved = |c: &Call| (c.state.clone(), c.cost_micros == c.reserved_micros && c.reserved_micros > 0);

    // The caller goes away after the first chunk.
    let mut res = r
        .request(As::Member("alice"), "POST", &url)
        .json(&json!({ "model": "claude-sonnet-4-6", "max_tokens": 4000, "stream": true, "slow": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let first = res.chunk().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&first).contains("message_start"));
    drop(res);
    let c = r.ended(1).await;
    assert_eq!(reserved(&c), ("kept".into(), true), "{c:?}");
    assert_eq!((c.input, c.output, c.model.as_deref()), (100, 1, Some("claude-sonnet-4-6")), "what it had seen");
    assert!(c.cost_micros >= prices::micros(4000.0 * 15.0 / 1e6), "the reservation: all of max_tokens");

    // A stream that ends without its message_delta.
    let (status, _, _) = r
        .ask_with(
            As::Member("alice"),
            json!({ "model": "claude-opus-5", "max_tokens": 50, "stream": true, "no_delta": true }),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(reserved(&r.ended(2).await), ("kept".into(), true));

    // A JSON body cut short.
    let res = r
        .request(As::Member("alice"), "POST", &url)
        .json(&json!({ "max_tokens": 50, "truncated": true }))
        .send()
        .await
        .unwrap();
    let _ = res.text().await;
    assert_eq!(reserved(&r.ended(3).await), ("kept".into(), true));

    // Dropped while it went out: the request gives up before Anthropic
    // answers; the call's task still ends it.
    let gave_up = r
        .request(As::Member("alice"), "POST", &url)
        .timeout(Duration::from_millis(100))
        .json(&json!({ "model": "claude-opus-5", "max_tokens": 50, "hold_ms": 400 }))
        .send()
        .await;
    assert!(gave_up.is_err());
    let c = r.ended(4).await;
    assert_eq!(reserved(&c), ("kept".into(), true), "{c:?}");
    assert_eq!(c.status, 200, "Anthropic answered after the caller left");

    // An error answer bills nothing.
    let (status, _, _) =
        r.ask_with(As::Member("alice"), json!({ "model": "claude-opus-5", "max_tokens": 50, "bad": true })).await;
    assert_eq!(status, 400);
    let c = r.ended(5).await;
    assert_eq!((c.state.as_str(), c.cost_micros, c.status), ("settled", 0, 400));

    // A stream with its final usage settles, as before.
    assert_eq!(r.ask(As::Member("alice"), "claude-opus-5", 10).await.0, 200);
    let c = r.ended(6).await;
    assert_eq!((c.state.as_str(), c.cost_micros), ("settled", cost("claude-opus-5", 10)));

    // A call whose machine died mid-call keeps its reservation too.
    use crate::db::sql::Sql as _;
    r.app
        .db
        .c()
        .execute("UPDATE inference_calls SET state = 'open' WHERE id = ?1", crate::db::sql::params![c.id])
        .unwrap();
    r.app.db.tidy_inference_calls(c.started_at + crate::db::inference::OPEN_MS + 1).unwrap();
    assert_eq!(r.app.db.call(&c.id).unwrap().unwrap().state, "kept");

    let huge = vec![b'x'; super::MAX_BODY + 1];
    let (status, _, body) = r.raw(As::Member("alice"), "POST", &url, Some(huge)).await;
    assert_eq!((status, reason(&body).as_str()), (413, "too_large"), "{body}");
    r.nothing_leaked();
}

/// The broker's settings from its flags.
#[test]
fn settings() {
    let c = Config::new(Some(Key::new("  ")), "", 5.0, 4, &[], &[]).unwrap();
    assert!(c.key.is_none() && !c.on(), "an empty key is none");
    assert_eq!(c.upstream, super::ANTHROPIC);
    let c = Config::new(Some(Key::new(PLATFORM_KEY)), "http://fakes:9004/", 5.0, 0, &[" ".into()], &[]).unwrap();
    assert!(c.on());
    assert_eq!((c.upstream.as_str(), c.concurrent, c.platform_betas.len()), ("http://fakes:9004", 1, 0));
    assert!(Config::new(None, "fakes", 5.0, 4, &[], &[]).is_err());
    assert!(Config::new(None, "", -1.0, 4, &[], &[]).is_err());
    assert!(Config::new(None, "", 1.0, 4, &[], &["bad".into()]).is_err());
    assert!(Config::new(None, "", 1.0, 4, &["context-1m-2025-08-07".into()], &[]).is_err(), "never the premium");
}
