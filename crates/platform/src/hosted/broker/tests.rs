//! The inference broker (M231) through the platform's routes, against a
//! fake Anthropic: whose key pays (a project's, an account's own, a
//! sponsor's or visitor's a client names, the platform's under a cap),
//! the lobby-shaped client, metering and the spend it shows, and that no
//! key ever comes back. On SQLite and, with `ARUGULA_TEST_POSTGRES_URL`,
//! Postgres.

use std::sync::{Arc, Mutex};

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
}

/// What the fake streams for a request of `model` that asked for
/// `max_tokens`: 100 input tokens in, `max_tokens` out.
fn events(model: &str, out: u64) -> String {
    format!(
        "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_1\",\"model\":\"{model}\",\"usage\":{{\"input_tokens\":100,\"output_tokens\":1}}}}}}\n\n\
         event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"delta\":{{\"type\":\"text_delta\",\"text\":\"hi\"}}}}\n\n\
         event: message_delta\ndata: {{\"type\":\"message_delta\",\"usage\":{{\"output_tokens\":{out}}}}}\n\n\
         event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
    )
}

async fn messages(State(f): State<Arc<Fake>>, headers: HeaderMap, req: Request) -> Response {
    let path = req.uri().path().to_owned();
    let body: Value =
        serde_json::from_slice(&axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap()).unwrap_or_default();
    let seen = Seen { path: path.clone(), headers, body: body.clone() };
    f.seen.lock().unwrap().push(seen.clone());
    if seen.key().contains("revoked") {
        let e = json!({ "type": "error", "error": { "type": "authentication_error", "message": "invalid x-api-key" } });
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    if path.ends_with("count_tokens") {
        return Json(json!({ "input_tokens": 7 })).into_response();
    }
    let model = body["model"].as_str().unwrap_or("claude-opus-5").to_owned();
    let out = body["max_tokens"].as_u64().unwrap_or(10);
    // A provider that says the key back in a header: the broker never
    // passes it on.
    let echo = [("x-echo", seen.key())];
    if body["slow"] == true {
        let first = Bytes::from(events(&model, out).split("event: content_block_delta").next().unwrap().to_owned());
        let s = futures_util::stream::once(async move { Ok::<_, std::io::Error>(first) })
            .chain(futures_util::stream::pending());
        return ([("content-type", "text/event-stream")], Body::from_stream(s)).into_response();
    }
    if body["stream"] == true {
        return ([("content-type", "text/event-stream")], echo, events(&model, out)).into_response();
    }
    let m = json!({ "type": "message", "id": "msg_2", "model": model, "content": [{ "type": "text", "text": "hi" }],
                    "usage": { "input_tokens": 100, "output_tokens": out, "cache_read_input_tokens": 1000 } });
    (echo, Json(m)).into_response()
}

async fn models(State(f): State<Arc<Fake>>, headers: HeaderMap, req: Request) -> Json<Value> {
    f.seen.lock().unwrap().push(Seen { path: req.uri().path().to_owned(), headers, body: Value::Null });
    Json(json!({ "data": [{ "id": "claude-opus-5" }] }))
}

/// A fake Anthropic: its URL.
async fn anthropic(f: Arc<Fake>) -> String {
    let r = Router::new()
        .route("/v1/messages", post(messages))
        .route("/v1/messages/count_tokens", post(messages))
        .route("/v1/models", get(models))
        .route("/v1/models/{id}", get(models))
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

/// The broker with the platform's key, the allowlist (bob), and two
/// clients: studio (the platform's key pays for its accounts up to $20 a
/// month, and it names sponsors and visitors) and hud (neither).
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

    async fn raw(&self, who: As<'_>, method: &str, url: &str, body: Option<Vec<u8>>) -> (u16, HeaderMap, String) {
        let mut req = reqwest::Client::new()
            .request(method.parse().unwrap(), url)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "fine-grained-tool-streaming-2025-05-14")
            // What a box's agent holds: a stand-in the broker drops.
            .header("x-api-key", "arugula-brokered");
        match who {
            As::Member(a) => req = req.header("cookie", format!("ilg_session=session-{a}")),
            As::Client { credential, account, sponsor, visitor } => {
                req = req.bearer_auth(credential);
                for (h, v) in [(super::ACCOUNT, account), (super::SPONSOR, sponsor), (super::VISITOR, visitor)] {
                    if let Some(v) = v {
                        req = req.header(h, v);
                    }
                }
            }
        }
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
        let body = json!({ "model": model, "max_tokens": out, "stream": true, "messages": [{ "role": "user", "content": "hi" }] });
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

    fn calls(&self) -> Vec<crate::db::inference::Call> {
        let c = self.app.db.c();
        use crate::db::sql::Sql as _;
        c.query_map(
            "SELECT id, project_id, account, client, provider, pays, payer, model, status, input_tokens, output_tokens,
                 cache_write_tokens, cache_write_1h_tokens, cache_read_tokens, cost_micros, priced_as, started_at, ended_at
             FROM inference_calls ORDER BY started_at, id",
            crate::db::sql::params![],
            |r| {
                Ok(crate::db::inference::Call {
                    id: r.get(0)?,
                    project: r.get(1)?,
                    account: r.get(2)?,
                    client: r.get(3)?,
                    provider: r.get(4)?,
                    pays: r.get(5)?,
                    payer: r.get(6)?,
                    model: r.get(7)?,
                    status: r.get::<u32>(8)? as u16,
                    input: r.get(9)?,
                    output: r.get(10)?,
                    cache_write: r.get(11)?,
                    cache_write_1h: r.get(12)?,
                    cache_read: r.get(13)?,
                    cost_micros: r.get(14)?,
                    priced_as: r.get(15)?,
                    started_at: r.get(16)?,
                    ended_at: r.get(17)?,
                })
            },
        )
        .unwrap()
    }

    /// The broker kept a call: wait for it (it's written when the answer's
    /// body is dropped).
    async fn kept(&self, n: usize) -> Vec<crate::db::inference::Call> {
        for _ in 0..200 {
            let c = self.calls();
            if c.len() >= n {
                return c;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("{n} calls never kept: {:?}", self.calls());
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

/// $25: a million output tokens of Opus 5, past a $20 cap in one call.
const BIG: u64 = 1_000_000;

/// A member's call with no key that pays is refused, saying how to add
/// one; with the project's own key it streams through as Anthropic sent
/// it, on that key and nothing of the caller's, and is metered against
/// the project and the member. Count-tokens and models go through
/// unmetered; anything else isn't forwarded; Claude Code's hello needs
/// nothing.
#[tokio::test]
async fn a_member_calls_on_the_projects_own_key() {
    let r = rig().await;
    let p = r.project.clone();
    let (status, _, body) = r.ask(As::Member("alice"), "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "no_key"), "{body}");
    assert!(body.contains(&format!("arugula cloud key set anthropic --project {p}")), "{body}");
    assert_eq!(r.fake.count(), 0, "never sent");
    assert_eq!(r.send(As::Member("dana"), &p, "POST", "/v1/messages", Some(json!({}))).await.0, 404, "not a member");
    let (status, _, body) = r.send(As::Member("nobody"), &p, "POST", "/v1/messages", Some(json!({}))).await;
    assert_eq!((status, reason(&body).as_str()), (404, "no_project"));

    r.key("alice", &p, PROJECT_KEY).await;
    let (status, h, body) = r.ask(As::Member("vic"), "claude-opus-5", 10).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, events("claude-opus-5", 10), "streamed through as sent");
    assert_eq!(pays(&h), "project");
    assert!(h.get("x-echo").is_none(), "a header saying the key back never comes back");
    let seen = r.fake.last();
    assert_eq!(seen.key(), PROJECT_KEY);
    assert_eq!(seen.header("accept-encoding"), Some("identity"), "read as it streams");
    assert_eq!(seen.header("anthropic-beta"), Some("fine-grained-tool-streaming-2025-05-14"));
    assert!(seen.header("cookie").is_none() && seen.header("authorization").is_none());
    assert_eq!(seen.body["model"], "claude-opus-5");

    let calls = r.kept(1).await;
    let c = &calls[0];
    assert_eq!(
        (c.project.as_str(), c.account.as_deref(), c.client.as_deref(), c.pays.as_str(), c.model.as_deref()),
        (p.as_str(), Some("vic"), None, "project", Some("claude-opus-5"))
    );
    assert_eq!((c.input, c.output, c.status), (100, 10, 200));
    assert_eq!(
        c.cost_micros,
        prices::micros(
            prices::cost_of("claude-opus-5", &prices::Usage { input: 100, output: 10, ..Default::default() }).0
        )
    );

    // Not streamed: one JSON message, metered the same way.
    let json = json!({ "model": "claude-haiku-4-5", "max_tokens": 5, "messages": [] });
    let (status, h, body) = r.send(As::Member("alice"), &p, "POST", "/v1/messages", Some(json)).await;
    assert_eq!((status, pays(&h)), (200, "project"), "{body}");
    assert!(h.get("x-echo").is_none());
    let c = &r.kept(2).await[1];
    assert_eq!((c.input, c.output, c.cache_read, c.priced_as.as_deref()), (100, 5, 1000, Some("claude-haiku-4-5")));

    let (status, _, body) = r.send(As::Member("alice"), &p, "POST", "/v1/messages/count_tokens", Some(json!({}))).await;
    assert_eq!((status, body.as_str()), (200, r#"{"input_tokens":7}"#));
    assert_eq!(r.send(As::Member("alice"), &p, "GET", "/v1/models?limit=1", None).await.0, 200);
    assert_eq!(r.fake.last().path, "/v1/models");
    assert_eq!(r.send(As::Member("alice"), &p, "GET", "/v1/models/claude-opus-5", None).await.0, 200);
    assert_eq!(r.fake.last().path, "/v1/models/claude-opus-5");
    for (m, path) in
        [("POST", "/v1/complete"), ("GET", "/v1/messages"), ("DELETE", "/v1/models"), ("GET", "/v1/models/../x")]
    {
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
/// again; its own key then pays, with no cap. An account not on the
/// allowlist never gets the platform's key from the broker.
#[tokio::test]
async fn the_platforms_key_pays_for_the_allowlist_under_a_monthly_cap() {
    let r = rig().await;
    let (status, h, body) = r.ask(As::Member("bob"), "claude-opus-5", BIG).await;
    assert_eq!((status, pays(&h)), (200, "platform"), "{body}");
    assert_eq!(r.fake.last().key(), PLATFORM_KEY);
    let c = &r.kept(1).await[0];
    assert_eq!((c.pays.as_str(), c.account.as_deref(), c.cost_micros), ("platform", Some("bob"), 25_000_500));

    let (_, v) = call(&r.base, "bob", "GET", "/api/inference/spend", None).await;
    assert_eq!(
        (v["spent_usd"].as_f64(), v["cap_usd"].as_f64(), v["client"].clone()),
        (Some(25.0005), Some(20.0), Value::Null),
        "{v}"
    );

    let sent = r.fake.count();
    let (status, _, body) = r.ask(As::Member("bob"), "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "cap"), "{body}");
    let said: Value = serde_json::from_str(&body).unwrap();
    let now = arugula_e2e::now_ms();
    assert_eq!(said["arugula"]["resets"], prices::day_of(prices::next_month(now)));
    assert_eq!(said["arugula"]["cap_usd"], 20.0);
    assert!(said["error"]["message"].as_str().unwrap().contains("bob has used this month's $20"), "{body}");
    assert_eq!(r.fake.count(), sent, "refused before it went out");

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

/// studio's lobby, as a registered client: it names the account (a member
/// of the project); the platform's key pays for any of its accounts with
/// no key of their own, up to its $20 a month each, counted apart from the
/// allowlist's; it names a sponsor whose own key pays over the account's,
/// and a visitor whose own key pays for their own turn. A client that
/// may not name payers can't, and one with no house cap gets no key for
/// an account off the allowlist. Its credential never reaches Anthropic.
#[tokio::test]
async fn a_registered_client_calls_for_its_accounts() {
    let r = rig().await;
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
    assert!(seen.header(super::ACCOUNT).is_none(), "nor the platform's own headers");
    let c = &r.kept(1).await[0];
    assert_eq!(
        (c.client.as_deref(), c.account.as_deref(), c.pays.as_str()),
        (Some("studio"), Some("alice"), "platform")
    );
    let (status, _, body) = r.ask(studio("alice"), "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "cap"), "{body}");
    // bob's allowlist month is his own: studio's spend for alice isn't in it.
    assert_eq!(r.ask(As::Member("bob"), "claude-opus-5", 10).await.0, 200);
    let spend = |who: As<'static>| {
        let url = format!("{}/api/inference/spend", r.base);
        let r = &r;
        async move { serde_json::from_str::<Value>(&r.raw(who, "GET", &url, None).await.2).unwrap() }
    };
    let v = spend(studio("alice")).await;
    assert_eq!(
        (v["client"].as_str(), v["spent_usd"].as_f64(), v["cap_usd"].as_f64()),
        (Some("studio"), Some(25.0005), Some(20.0)),
        "{v}"
    );

    // A sponsor's own key pays over the account's (the lobby's adoption).
    r.key("dana", "personal", OAUTH).await;
    r.key("alice", "personal", OWN_KEY).await;
    let sponsored = As::Client { credential: STUDIO, account: Some("alice"), sponsor: Some("dana"), visitor: None };
    let (status, h, body) = r.ask(sponsored, "claude-opus-5", 10).await;
    assert_eq!((status, pays(&h)), (200, "account dana"), "{body}");
    let seen = r.fake.last();
    assert_eq!(seen.header("authorization"), Some(format!("Bearer {OAUTH}").as_str()), "an OAuth token as a bearer");
    assert_eq!(seen.header("anthropic-beta"), Some("fine-grained-tool-streaming-2025-05-14,oauth-2025-04-20"));
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
    let c = r.kept(5).await.pop().unwrap();
    assert_eq!((c.account.as_deref(), c.payer.as_deref(), c.pays.as_str()), (Some("alice"), Some("erin"), "account"));

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
    assert_eq!(line("alice", json!("studio"), "platform").unwrap()["usd"], 25.0005, "{v}");
    assert_eq!(line("alice", json!("studio"), "account").unwrap()["calls"], 3, "{v}");
    assert!(line("bob", Value::Null, "platform").is_some(), "{v}");
    assert!(line("bob", json!("hud"), "platform").is_some(), "{v}");
    r.nothing_leaked();
}

/// No key ever comes back: Anthropic refusing one says whose, never it;
/// a header that repeats it is dropped; nothing kept here holds one in the
/// clear; and the settings never print one.
#[tokio::test]
async fn no_key_ever_comes_back() {
    let r = rig().await;
    r.key("alice", &r.project, REVOKED).await;
    let (status, _, body) = r.ask(As::Member("alice"), "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "key_refused"), "{body}");
    assert!(body.contains(&format!("project {}'s own key", r.project)), "{body}");
    assert_eq!(r.fake.last().key(), REVOKED, "it was the project's key that went");
    assert!(r.calls().is_empty(), "a refused call costs nothing");
    r.key("alice", &r.project, PROJECT_KEY).await;
    let (status, h, _) = r.ask(As::Member("alice"), "claude-opus-5", 10).await;
    assert_eq!(status, 200);
    assert!(h.get("x-echo").is_none());
    // A sponsor's own key that Anthropic refuses: whose it was.
    r.key("dana", "personal", REVOKED).await;
    let sponsored = As::Client { credential: STUDIO, account: Some("vic"), sponsor: Some("dana"), visitor: None };
    let (status, _, body) = r.ask(sponsored, "claude-opus-5", 10).await;
    assert_eq!((status, reason(&body).as_str()), (403, "key_refused"), "{body}");
    assert!(body.contains("Anthropic refused dana's own key"), "{body}");
    // The settings, shown, show none of it.
    let debug = format!("{:?} {:?}", r.app.hosted.broker, Key::new(PLATFORM_KEY));
    assert!(!debug.contains(PLATFORM_KEY) && debug.contains("key: Some(\"set\")"), "{debug}");
    r.nothing_leaked();
}

/// A caller that goes away mid-answer stops the call to Anthropic, and
/// what the answer had used so far is kept; a request past Anthropic's
/// ceiling is refused here.
#[tokio::test]
async fn a_call_cut_short_is_metered_and_a_huge_one_refused() {
    let r = rig().await;
    r.key("alice", &r.project, PROJECT_KEY).await;
    let url = format!("{}/api/projects/{}/inference/anthropic/v1/messages", r.base, r.project);
    let res = reqwest::Client::new()
        .post(&url)
        .header("cookie", "ilg_session=session-alice")
        .json(&json!({ "model": "claude-sonnet-4-6", "stream": true, "slow": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let mut res = res;
    let first = res.chunk().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&first).contains("message_start"));
    drop(res);
    let c = &r.kept(1).await[0];
    assert_eq!((c.model.as_deref(), c.input, c.output), (Some("claude-sonnet-4-6"), 100, 1), "what it had used");

    let huge = vec![b'x'; super::MAX_BODY + 1];
    let (status, _, body) = r.raw(As::Member("alice"), "POST", &url, Some(huge)).await;
    assert_eq!((status, reason(&body).as_str()), (413, "too_large"), "{body}");
    r.nothing_leaked();
}

/// The broker's settings from its flags.
#[test]
fn settings() {
    let c = Config::new(Some(Key::new("  ")), "", 5.0, &[]).unwrap();
    assert!(c.key.is_none() && !c.on(), "an empty key is none");
    assert_eq!(c.upstream, super::ANTHROPIC);
    let c = Config::new(Some(Key::new(PLATFORM_KEY)), "http://fakes:9004/", 5.0, &[]).unwrap();
    assert!(c.on());
    assert_eq!(c.upstream, "http://fakes:9004");
    assert!(Config::new(None, "fakes", 5.0, &[]).is_err());
    assert!(Config::new(None, "", -1.0, &[]).is_err());
    assert!(Config::new(None, "", 1.0, &["bad".into()]).is_err());
}
