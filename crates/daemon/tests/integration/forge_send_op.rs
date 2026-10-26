//! M168: control asks a machine to send its person's draft on a linked repo
//! (the `forge.send` op over the relay socket), because no browser hop
//! could. Against a fake control and a fake GitHub: the daemon says it
//! knows the op when it connects, sends its owner's draft with their own
//! `gh` login (not the App's), answers with the comment's URL, and refuses
//! another person's draft without touching GitHub. The platform's side is
//! `arugula-platform`'s `forge::send_tests`.

use std::{
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
};

use arugula_e2e::{Cert, DeviceKeys, Kind};
use arugula_testkit::arugulad;
use axum::{
    Json, Router,
    extract::{
        Path, State,
        ws::{Message, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::Response,
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct Fake {
    url: Arc<Mutex<String>>,
    /// What the daemon said on its relay socket: the ops it knows, and its
    /// answers.
    said: Arc<Mutex<Vec<Value>>>,
    /// GitHub's comments: the token each came with, and the body.
    comments: Arc<Mutex<Vec<(String, Value)>>>,
    /// The platform's draft on acme/app#1, and the steps a machine said
    /// about sending it.
    draft: Arc<Mutex<Value>>,
    steps: Arc<Mutex<Vec<Value>>>,
}

const DRAFT: &str = "dft_0000000000000000000000000000000a";
const ITEM: &str = "fi_000000000000000000000000000000a1";

/// The mirror's row for `acme/app#1` (M172 pins the block to it).
fn item() -> Value {
    json!({ "id": ITEM, "repo": "acme/app", "number": 1, "kind": "pr", "state": "open",
            "title": "from the mirror", "labels": [], "author": "jake-gh", "assignees": [],
            "reviewers": [], "draft": false, "merge_state": "clean", "head_ref": "fix",
            "head_sha": "abc123", "base_ref": "main", "created_at": 1, "updated_at": 2 })
}

async fn ops(Json(b): Json<Value>) -> Json<Value> {
    Json(json!({ "op": b["op"], "v": 1, "result": { "item": item(), "events": [], "drafts": [] } }))
}

async fn items(uri: axum::http::Uri) -> Json<Value> {
    Json(match uri.path().rsplit('/').next() {
        Some("checks") => json!({ "item": ITEM, "head_sha": "abc123", "checks": [] }),
        Some("events") => json!({ "item": ITEM, "events": [] }),
        _ => item(),
    })
}

async fn repo(Path((owner, name)): Path<(String, String)>) -> Json<Value> {
    let linked = format!("{owner}/{name}") == "acme/app";
    Json(json!({ "repo": format!("{owner}/{name}"), "linked": linked,
                 "project": linked.then_some("prj_1"), "repo_id": linked.then_some(77),
                 "login": linked.then_some("jake-gh") }))
}

async fn drafts(State(f): State<Fake>) -> Json<Value> {
    let d = f.draft.lock().unwrap().clone();
    let waiting: Vec<Value> = [d].into_iter().filter(|d| d["state"] == "waiting").collect();
    Json(json!({ "drafts": waiting }))
}

async fn machine(State(f): State<Fake>, Json(b): Json<Value>) -> Json<Value> {
    f.steps.lock().unwrap().push(b.clone());
    let mut d = f.draft.lock().unwrap();
    d["state"] = json!(match b["step"].as_str() {
        Some("start") => "sending",
        Some("sent") => "sent",
        _ => "failed",
    });
    Json(d.clone())
}

async fn about(State(f): State<Fake>) -> Json<Value> {
    let url = f.url.lock().unwrap().clone();
    Json(json!({ "control": true, "daemon_auth": 2, "url": url }))
}

fn ask(id: &str, account: &str) -> String {
    json!({ "t": "op", "id": id, "op": { "op": "forge.send", "draft": format!("dft_{id}"), "account": account,
        "repo": "acme/app", "number": 1, "kind": "comment", "body": "ship it" } })
    .to_string()
}

/// The relay socket: once the daemon says it knows `forge.send`, ask it for
/// its owner's draft and for someone else's.
async fn dial(State(f): State<Fake>, up: WebSocketUpgrade) -> Response {
    up.on_upgrade(|ws| async move {
        let (mut tx, mut rx) = ws.split();
        while let Some(Ok(m)) = rx.next().await {
            let Message::Text(t) = m else { continue };
            let Ok(v) = serde_json::from_str::<Value>(t.as_str()) else { continue };
            let knows = v["t"] == "ops" && v["ops"].as_array().is_some_and(|o| o.iter().any(|x| x == "forge.send"));
            f.said.lock().unwrap().push(v);
            if knows {
                let _ = tx.send(Message::Text(ask("mine", "a1").into())).await;
                let _ = tx.send(Message::Text(ask("theirs", "someone-else").into())).await;
            }
        }
    })
}

async fn comment(
    State(f): State<Fake>,
    Path((owner, name, n)): Path<(String, String, u64)>,
    headers: HeaderMap,
    Json(b): Json<Value>,
) -> Json<Value> {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    f.comments.lock().unwrap().push((auth, b));
    Json(json!({ "id": 5, "html_url": format!("https://github.com/{owner}/{name}/pull/{n}#issuecomment-5") }))
}

fn serve(rt: &tokio::runtime::Runtime, f: Fake) -> String {
    rt.block_on(async {
        let app = Router::new()
            .route("/control.json", get(about))
            .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
            .route("/api/daemon/peers", get(|| async { Json(json!({ "a1": { "name": "jake" } })) }))
            .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
            .route("/api/daemon/access", post(|| async { Json(json!({})) }))
            .route("/api/relay/dial", get(dial))
            .route("/github/repos/{owner}/{name}/issues/{n}/comments", post(comment))
            .route("/api/forge/repos/{owner}/{name}", get(repo))
            .route("/api/forge/ops", post(ops))
            .route("/api/forge/items/{*rest}", get(items))
            .route("/api/projects/{project}/drafts", get(drafts))
            .route(&format!("/api/forge/drafts/{DRAFT}/machine"), post(machine))
            .with_state(f.clone());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        *f.url.lock().unwrap() = at.clone();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    })
}

fn enroll(state: &std::path::Path, control: &str) {
    let keys = DeviceKeys::load_or_create(&state.join("daemon.key")).unwrap();
    let mut cert = Cert::new(&keys, "a1", Kind::Daemon, "test");
    cert.sign_with(&keys);
    let saved =
        json!({ "url": control, "trust": { "account": "a1", "root": keys.id() }, "cert": cert, "login": "jake" });
    std::fs::write(state.join("control.json"), saved.to_string()).unwrap();
}

/// A `gh` logged in as the person.
fn gh(dir: &std::path::Path) -> String {
    std::fs::create_dir_all(dir).unwrap();
    let gh = dir.join("gh");
    std::fs::write(&gh, "#!/bin/sh\necho gho_the_person\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default())
}

fn answer(f: &Fake, id: &str) -> Option<Value> {
    f.said.lock().unwrap().iter().find(|v| v["t"] == "op.done" && v["id"] == id).map(|v| v["answer"].clone())
}

#[test]
fn a_machine_sends_its_persons_draft_when_control_asks() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let mut d = arugulad!("forgesendop")
        .no_wisp()
        .no_tailscale()
        .env("ARUGULA_GITHUB_API", format!("{url}/github"))
        .args(["--direct-url", "http://127.0.0.1:0"])
        .wait_secs(30)
        .start();
    d.stop();
    let path = gh(&d.state.join("bin"));
    d.set_env("PATH", path);
    enroll(&d.state, &url);
    d.start();

    d.wait_for("both answers", || answer(&f, "mine").is_some() && answer(&f, "theirs").is_some());
    let ops = f.said.lock().unwrap().iter().find(|v| v["t"] == "ops").cloned().unwrap();
    assert_eq!(ops["ops"], json!(["forge.send", "runs.start", "runs.revoke", "hosted.archive"]));
    assert_eq!(
        answer(&f, "mine").unwrap(),
        json!({ "answer": "done", "url": "https://github.com/acme/app/pull/1#issuecomment-5" })
    );
    // Sent once, with the person's own login.
    let comments = f.comments.lock().unwrap().clone();
    assert_eq!(comments.len(), 1, "{comments:?}");
    assert_eq!(comments[0].0, "Bearer gho_the_person");
    assert_eq!(comments[0].1, json!({ "body": "ship it" }));
    // Another person's draft: this machine can't, and GitHub heard nothing.
    let theirs = answer(&f, "theirs").unwrap();
    assert_eq!(theirs["answer"], "cant", "{theirs}");
}

fn info(d: &arugula_testkit::Daemon, pane: u64) -> Value {
    d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == pane).cloned().unwrap_or_default()
}

/// A block's card on a linked repo (pinned to the platform's item, M172)
/// sends the platform's draft with the owner's login, and tells the
/// platform: it holds it first, then says it was sent and where.
#[test]
fn a_blocks_card_sends_a_platform_draft() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    *f.draft.lock().unwrap() = json!({
        "id": DRAFT, "project": "prj_1", "repo": "acme/app", "repo_id": 77, "number": 1, "kind": "comment",
        "event": null, "title": null, "body": "drafted by an agent", "state": "waiting", "author": "a1",
        "agent": "mcp:claude-code", "machine": "m1", "created_at": 1, "updated_at": 1,
    });
    let url = serve(&rt, f.clone());
    let mut d = arugulad!("forgecard")
        .no_wisp()
        .no_tailscale()
        .env("ARUGULA_FORGE_POLL_MS", "200,200")
        .env("ARUGULA_GITHUB_API", format!("{url}/github"))
        .args(["--direct-url", "http://127.0.0.1:0"])
        .wait_secs(30)
        .start();
    d.stop();
    let path = gh(&d.state.join("bin"));
    d.set_env("PATH", path);
    enroll(&d.state, &url);
    d.start();
    let block = d.post(
        "/api/blocks",
        json!({ "type": "forge", "config": { "pr": "https://github.com/acme/app/pull/1" }, "local": true }),
    )["block"]
        .as_u64()
        .unwrap();
    d.wait_for("the block pinned", || d.get(&format!("/api/blocks/{block}"))["state"]["pin"]["global_id"] == ITEM);
    d.wait_for("the platform's draft on the card", || info(&d, block)["ask"]["id"] == DRAFT);
    d.post(&format!("/api/blocks/{block}/call/answer"), json!({ "id": DRAFT, "content": { "body": "edited by me" } }));
    d.wait_for("the platform heard", || f.steps.lock().unwrap().len() == 2);
    let steps = f.steps.lock().unwrap().clone();
    assert_eq!(steps[0]["step"], "start");
    assert_eq!(steps[1]["step"], "sent");
    assert_eq!(steps[1]["url"], "https://github.com/acme/app/pull/1#issuecomment-5");
    // (The relay socket's asks went too: only the card's is this one.)
    let comments: Vec<_> = f.comments.lock().unwrap().iter().filter(|c| c.1["body"] != "ship it").cloned().collect();
    assert_eq!(comments, [("Bearer gho_the_person".to_owned(), json!({ "body": "edited by me" }))]);
}
