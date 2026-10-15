//! Fountain track 2, F1 (#942): this daemon's own Fountain sign-in and the
//! account stream, against a fake Fountain served here.
//!
//! What's checked: `POST /api/fountain/login` with no URL asks for one; with
//! one it gives the device code, the daemon polls until it's approved, keeps
//! the key in the state dir (0600) and never shows it; the account stream
//! (`--fountain-account-stream`) connects with that key and `?blocks=true`,
//! hands events on and keeps its cursor; a daemon killed (SIGKILL) and
//! started again resumes from the cursor (`Last-Event-ID`) with no gap and
//! nothing twice, across Fountain's idle closes; `DELETE` signs out and
//! revokes the key, and the stream stops.

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
/// (Fountain's replay may overlap), at most three a connection, then closed
/// as an idle Fountain closes it.
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

struct Fountain {
    _rt: tokio::runtime::Runtime,
    f: Fake,
    origin: String,
}

impl Fountain {
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
                .with_state(f.clone());
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let at = format!("http://{}", l.local_addr().unwrap());
            tokio::spawn(async move { axum::serve(l, api).await.unwrap() });
            at
        });
        Self { _rt: rt, f, origin }
    }
}

fn login(d: &Daemon) -> Value {
    d.get("/api/fountain/login")
}

#[test]
fn device_sign_in_and_the_account_stream_resume_across_a_restart() {
    let fake = Fountain::start();
    let dir = Scratch::new("fountain-account");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let home = home.display().to_string();
    let mut d = Daemon::child_labs_env(
        &["--fountain-account-stream", "--wisp-token-file", "/nonexistent", "--tailscale-socket", "/nonexistent/sock"],
        &[
            ("HOME", home.as_str()),
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
    assert!(body.contains("which Fountain?"), "{body}");

    // Sign in: the code, then the key once it's approved.
    let s = d.post("/api/fountain/login", json!({ "base_url": fake.origin }));
    assert_eq!(s["pending"]["user_code"], "QWER-TYUI", "{s}");
    assert_eq!(s["pending"]["base_url"], fake.origin.as_str());
    d.wait_for("the sign-in", || login(&d)["source"] == "arugula");
    let s = login(&d);
    assert_eq!((s["email"].as_str(), s["key_prefix"].as_str()), (Some("owner@example.com"), Some("ftn_de")), "{s}");
    assert!(s["pending"].is_null() && s["error"].is_null(), "{s}");
    let file = d.state.join("fountain/login.json");
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
    d.start();
    d.wait_for("events 6 to 11", || login(&d)["stream"]["cursor"] == 11);
    let s = login(&d);
    assert_eq!(s["source"], "arugula", "the sign-in outlives a restart: {s}");
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
    assert_eq!(s["note"], "the key was revoked on Fountain", "{s}");
    assert_eq!(s["signed_in"], false);
    assert!(!file.exists());
    assert_eq!(fake.f.with(|i| i.revoked.clone()), ["key-1"]);
    d.wait_for("the stream to stop", || login(&d)["stream"]["connected"] == false);
}
