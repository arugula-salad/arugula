//! Managoat track 2, F1 (#942): this daemon's own Managoat sign-in and the
//! account stream, against a fake Managoat served here.
//!
//! What's checked: `POST /api/fountain/login` with no URL asks for one; with
//! one it gives the device code, the daemon polls until it's approved, keeps
//! the key in the state dir (0600) and never shows it; the account stream
//! (`--managoat-account-stream`) connects with that key and `?blocks=true`,
//! hands events on and keeps its cursor; a daemon killed (SIGKILL) and
//! started again resumes from the cursor (`Last-Event-ID`) with no gap and
//! nothing twice, across Managoat's idle closes; `DELETE` signs out and
//! revokes the key, and the stream stops. And (F5: Managoat is out of
//! Labs, with no flag) a new install with no login asks Managoat nothing,
//! even with the account stream on and Managoat's URL known.

// Over the daemon's Unix socket.
#![cfg(unix)]

use crate::agentd;

use std::{
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
};

use agentd::*;
use axum::{
    Json, Router,
    body::Body,
    extract::{Path as UrlPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde_json::{Value, json};

const KEY: &str = "ftn_device_minted_key";

#[derive(Default)]
struct Inner {
    /// Events 1..=total exist.
    total: u64,
    /// Polls of the device token until it's approved.
    polls: u32,
    /// The `Last-Event-ID` each stream connection came with.
    cursors: Vec<Option<u64>>,
    revoked: Vec<String>,
    /// Every request, whatever it was for.
    requests: Vec<String>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<Inner>>);

impl Fake {
    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        f(&mut self.0.lock().unwrap())
    }
}

async fn device(Json(b): Json<Value>) -> Response {
    assert!(b["client_name"].as_str().unwrap().starts_with("Arugula on "), "{b}");
    (
        StatusCode::CREATED,
        Json(json!({ "device_code": "dc", "user_code": "QWER-TYUI", "verification_uri": "http://f/device",
                     "verification_uri_complete": "http://f/device?code=QWER-TYUI", "expires_in": 600, "interval": 1 })),
    )
        .into_response()
}

async fn token(State(f): State<Fake>) -> Response {
    if f.with(|i| {
        i.polls += 1;
        i.polls < 2
    }) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "authorization_pending" }))).into_response();
    }
    (StatusCode::CREATED, Json(json!({ "api_key": KEY, "key_id": "key-1", "prefix": "ftn_de" }))).into_response()
}

fn authed(h: &HeaderMap) -> bool {
    h.get("authorization").and_then(|a| a.to_str().ok()) == Some(&format!("Bearer {KEY}"))
}

async fn me(h: HeaderMap) -> Response {
    if !authed(&h) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(json!({ "id": "user-1", "email": "owner@example.com", "role": "user" })).into_response()
}

async fn revoke(State(f): State<Fake>, h: HeaderMap, UrlPath(id): UrlPath<String>) -> StatusCode {
    assert!(authed(&h));
    f.with(|i| i.revoked.push(id));
    StatusCode::NO_CONTENT
}

/// The account stream: what's after `Last-Event-ID`, the event at it again
/// (Managoat's replay may overlap), at most three a connection, then closed
/// as an idle Managoat closes it.
async fn stream(State(f): State<Fake>, h: HeaderMap, q: axum::extract::RawQuery) -> Response {
    if !authed(&h) {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "bad key", "reason": "invalid_api_key" })))
            .into_response();
    }
    assert_eq!(q.0.as_deref(), Some("blocks=true"));
    let after = h.get("last-event-id").map(|v| v.to_str().unwrap().parse::<u64>().unwrap());
    let total = f.with(|i| {
        i.cursors.push(after);
        i.total
    });
    let mut body = String::from(": connected\n\n");
    for id in (after.map_or(1, |a| a.max(1))..=total).take(3) {
        let ev = json!({ "conversation_id": "conv-1", "kind": "stage", "stage": "turn", "state": "done", "ts": "now" });
        body.push_str(&format!("id: {id}\nevent: stage\ndata: {ev}\n\n: heartbeat\n\n"));
    }
    Response::builder().header("content-type", "text/event-stream").body(Body::from(body)).unwrap()
}

struct Managoat {
    _rt: tokio::runtime::Runtime,
    f: Fake,
    origin: String,
}

impl Managoat {
    fn start() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let f = Fake::default();
        let origin = rt.block_on(async {
            let api = Router::new()
                .route("/api/auth/device", post(device))
                .route("/api/auth/device/token", post(token))
                .route("/api/auth/me", get(me))
                .route("/api/auth/api-keys/{id}", delete(revoke))
                .route("/api/events/stream", get(stream))
                .fallback(|| async { StatusCode::NOT_FOUND })
                .layer(axum::middleware::from_fn_with_state(f.clone(), counted))
                .with_state(f.clone());
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let at = format!("http://{}", l.local_addr().unwrap());
            tokio::spawn(async move { axum::serve(l, api).await.unwrap() });
            at
        });
        Self { _rt: rt, f, origin }
    }
}

/// Notes each request before it's answered.
async fn counted(State(f): State<Fake>, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    f.with(|i| i.requests.push(format!("{} {}", req.method(), req.uri())));
    next.run(req).await
}

fn login(d: &Daemon) -> Value {
    d.get("/api/fountain/login")
}

#[test]
fn device_sign_in_and_the_account_stream_resume_across_a_restart() {
    let fake = Managoat::start();
    let dir = Scratch::new("fountain-account");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let home = home.display().to_string();
    let mut d = Daemon::child_env(
        &["--managoat-account-stream", "--wisp-token-file", "/nonexistent", "--tailscale-socket", "/nonexistent/sock"],
        &[
            ("HOME", home.as_str()),
            ("MANAGOAT_API_KEY", ""),
            ("MANAGOAT_BASE_URL", ""),
            ("FOUNTAIN_API_KEY", ""),
            ("FOUNTAIN_BASE_URL", ""),
            ("ARUGULA_FOUNTAIN_STREAM_RETRY_MS", "50"),
        ],
    );

    // Nothing here yet: no CLI login, and no instance to default to.
    let s = login(&d);
    assert_eq!(s["signed_in"], false, "{s}");
    assert_eq!(s["stream"]["enabled"], true, "{s}");
    let (code, body) = d.raw("POST", "/api/fountain/login", Some(json!({})));
    assert_eq!(code, 400, "{body}");
    assert!(body.contains("which Managoat?"), "{body}");

    // Sign in: the code, then the key once it's approved.
    let s = d.post("/api/fountain/login", json!({ "base_url": fake.origin }));
    assert_eq!(s["pending"]["user_code"], "QWER-TYUI", "{s}");
    assert_eq!(s["pending"]["base_url"], fake.origin.as_str());
    d.wait_for("the sign-in", || login(&d)["source"] == "arugula");
    let s = login(&d);
    assert_eq!((s["email"].as_str(), s["key_prefix"].as_str()), (Some("owner@example.com"), Some("ftn_de")), "{s}");
    assert!(s["pending"].is_null() && s["error"].is_null(), "{s}");
    let file = d.state.join("managoat/login.json");
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(!s.to_string().contains(KEY));
    assert!(!d.raw("GET", "/api/host", None).1.contains(KEY));

    // The stream: events 1..=5 over two connections.
    fake.f.with(|i| i.total = 5);
    d.wait_for("events 1 to 5", || login(&d)["stream"]["cursor"] == 5);
    let s = login(&d)["stream"].clone();
    assert_eq!(s["delivered"], 5, "nothing twice: {s}");

    // Killed: no chance to save anything more. More happens meanwhile.
    d.kill();
    let before = fake.f.with(|i| i.cursors.len());
    fake.f.with(|i| i.total = 11);
    // And started as a daemon from before the rename had left it: the
    // sign-in and the cursor under `fountain/`. They move to `managoat/`.
    std::fs::rename(d.state.join("managoat"), d.state.join("fountain")).unwrap();
    d.start();
    d.wait_for("events 6 to 11", || login(&d)["stream"]["cursor"] == 11);
    let s = login(&d);
    assert_eq!(s["source"], "arugula", "the sign-in outlives a restart: {s}");
    assert!(file.is_file() && !d.state.join("fountain").exists(), "fountain/ moved to managoat/");
    assert_eq!(s["stream"]["delivered"], 6, "6 to 11, each once: {s}");
    let cursors = fake.f.with(|i| i.cursors.clone());
    assert_eq!(cursors[before], Some(5), "resumed from the kept cursor: {cursors:?}");
    // How often it connected before the first event depends on timing (a slow
    // runner reconnects more); what matters is that the cursor only moves
    // forward and that 3 then 5 were each resumed from.
    let kept: Vec<u64> = cursors.iter().flatten().copied().collect();
    assert!(kept.windows(2).all(|w| w[0] <= w[1]), "never backwards: {cursors:?}");
    assert_eq!(&kept[..2], [3, 5], "{cursors:?}");

    // Sign out: the key is revoked and forgotten; the stream has no login.
    let (code, body) = d.raw("DELETE", "/api/fountain/login", None);
    assert_eq!(code, 200, "{body}");
    let s: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(s["note"], "the key was revoked on Managoat", "{s}");
    assert_eq!(s["signed_in"], false);
    assert!(!file.exists());
    assert_eq!(fake.f.with(|i| i.revoked.clone()), ["key-1"]);
    d.wait_for("the stream to stop", || login(&d)["stream"]["connected"] == false);
}

/// A new install with no Managoat login makes no request to Managoat (#942,
/// F5): no flag keeps it quiet any more, only the missing login. Here the
/// account stream is on and Managoat's URL is known (`FOUNTAIN_BASE_URL`),
/// and the host, the login's status, a Managoat block and an `agent:
/// managoat` block are all asked for; Managoat hears nothing. It's said
/// with the names from before the rename (`--fountain-account-stream`,
/// `FOUNTAIN_BASE_URL`, block type `fountain`, `agent: fountain`), which
/// still work.
#[test]
fn no_login_no_requests_to_fountain() {
    let fake = Managoat::start();
    let dir = Scratch::new("fountain-quiet");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let home = home.display().to_string();
    let d = Daemon::child_env(
        &["--fountain-account-stream", "--wisp-token-file", "/nonexistent", "--tailscale-socket", "/nonexistent/sock"],
        &[
            ("HOME", home.as_str()),
            ("MANAGOAT_API_KEY", ""),
            ("MANAGOAT_BASE_URL", ""),
            ("FOUNTAIN_API_KEY", ""),
            ("FOUNTAIN_BASE_URL", fake.origin.as_str()),
            ("ARUGULA_FOUNTAIN_STREAM_RETRY_MS", "50"),
        ],
    );
    assert!(!d.state.join("flags.json").exists(), "a new install has no flags file");
    assert_eq!(d.get("/api/host")["features"]["fountain"], false);
    // The stream has looked for a login, and found none.
    d.wait_for("the stream to find no login", || login(&d)["stream"]["error"].is_string());
    let s = login(&d);
    assert_eq!((s["signed_in"].clone(), s["stream"]["connected"].clone()), (false.into(), false.into()), "{s}");
    // The account view, opened anyway: it says there's no login.
    let view = d.post("/api/blocks", json!({ "type": "fountain", "config": {}, "local": true }));
    let id = view["block"].as_u64().unwrap();
    d.wait_for("the view to say why it's empty", || d.state(id)["error"].is_string());
    // A conversation: refused before it starts, for want of an agent.
    let (status, text) =
        d.raw("POST", "/api/blocks", Some(json!({ "type": "agent", "config": { "agent": "fountain" } })));
    assert_eq!(status, 400, "{text}");
    assert_eq!(fake.f.with(|i| i.requests.clone()), Vec::<String>::new());
}
