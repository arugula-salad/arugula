//! M40 end to end inside control: its real router on a port, a fake
//! GitHub API that checks the App's JWT, and daemons on the relay socket
//! (signed as daemons sign) that subscribe and hear pokes. Since M161 the
//! App is the platform's: in process first, then with the platform as its
//! own server behind the hub's gateway, where a webhook lands on the
//! platform, the hub relays its pokes, and a restarted platform still knows
//! a delivery it took.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_e2e::{Cert, DeviceKeys, Kind, now_ms};
use aws_lc_rs::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

use arugula_platform::forge::{Caller, GithubApp, Purpose, Repo, Writes, parse_pem};

use crate::{App, forge};

const SECRET: &str = "whsec-m40-test";

#[derive(Default)]
struct Hub {
    public: Vec<u8>,
    minted: Vec<Value>,
    asked: Vec<String>,
    /// Tokens revoked (`DELETE /installation/token`).
    revoked: Vec<String>,
}

type Shared = Arc<Mutex<Hub>>;

/// A JWT this App signed, unexpired.
fn app_jwt(h: &Shared, headers: &HeaderMap) -> bool {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
    let Some(t) = auth.strip_prefix("Bearer ") else { return false };
    let p: Vec<&str> = t.split('.').collect();
    if p.len() != 3 {
        return false;
    }
    let Ok(sig) = B64.decode(p[2]) else { return false };
    let public = h.lock().unwrap().public.clone();
    let ok = UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, &public)
        .verify(format!("{}.{}", p[0], p[1]).as_bytes(), &sig)
        .is_ok();
    let claims: Value = serde_json::from_slice(&B64.decode(p[1]).unwrap_or_default()).unwrap_or_default();
    let now = now_ms() / 1000;
    ok && claims["iss"] == 42 && claims["exp"].as_u64().is_some_and(|e| e > now && e <= now + 600)
}

async fn installation(State(h): State<Shared>, headers: HeaderMap, Path((o, r)): Path<(String, String)>) -> Response {
    if !app_jwt(&h, &headers) {
        return (StatusCode::UNAUTHORIZED, "bad JWT").into_response();
    }
    h.lock().unwrap().asked.push(format!("installation {o}/{r}"));
    match o.as_str() {
        "jhgaylor" => {
            Json(json!({ "id": 11, "account": { "login": "jhgaylor", "id": 1, "type": "User" } })).into_response()
        }
        "acme" => {
            Json(json!({ "id": 12, "account": { "login": "acme", "id": 900, "type": "Organization" } })).into_response()
        }
        _ => (StatusCode::NOT_FOUND, Json(json!({ "message": "Not Found" }))).into_response(),
    }
}

/// The fake's repositories, by GitHub id.
const REPOS: [(u64, &str); 5] = [
    (101, "jhgaylor/hud"),
    (102, "acme/tool"),
    (103, "jhgaylor/wide"),
    (104, "jhgaylor/wrong"),
    (105, "jhgaylor/bare"),
];

/// A new token each time (`ghs_test_INSTALLATION_N`), covering the one
/// repository asked for, as GitHub's answer lists it. A misbehaving
/// GitHub's answers: `wide` covers two, `wrong` another repository, and
/// `bare` lists none.
async fn access_tokens(
    State(h): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(body): Json<Value>,
) -> Response {
    if !app_jwt(&h, &headers) {
        return (StatusCode::UNAUTHORIZED, "bad JWT").into_response();
    }
    let n = {
        let mut h = h.lock().unwrap();
        h.minted.push(json!({ "installation": id, "body": body }));
        h.minted.len()
    };
    let by_id = body["repository_ids"].as_array().into_iter().flatten().filter_map(Value::as_u64);
    let by_name = body["repositories"].as_array().into_iter().flatten().filter_map(Value::as_str);
    let mut covers: Vec<Value> = REPOS
        .iter()
        .filter(|(i, full)| by_id.clone().any(|x| x == *i) || by_name.clone().any(|x| full.ends_with(&format!("/{x}"))))
        .map(|(i, full)| json!({ "id": i, "full_name": full }))
        .collect();
    let hud = json!({ "id": 101, "full_name": "jhgaylor/hud" });
    match covers.first().and_then(|r| r["full_name"].as_str()) {
        Some("jhgaylor/wide") => covers.push(hud),
        Some("jhgaylor/wrong") => covers = vec![hud],
        _ => {}
    }
    let token = format!("ghs_test_{id}_{n}");
    let mut answer = json!({ "token": token, "expires_at": "2099-01-01T00:00:00Z", "repositories": covers,
        "permissions": body["permissions"] });
    if covers.iter().any(|r| r["full_name"] == "jhgaylor/bare") {
        answer.as_object_mut().unwrap().remove("repositories");
    }
    (StatusCode::CREATED, Json(answer)).into_response()
}

async fn revoke(State(h): State<Shared>, headers: HeaderMap) -> StatusCode {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
    match auth.strip_prefix("Bearer ghs_test_") {
        Some(t) => {
            h.lock().unwrap().revoked.push(format!("ghs_test_{t}"));
            StatusCode::NO_CONTENT
        }
        None => StatusCode::UNAUTHORIZED,
    }
}

/// The platform's token log, as rows of JSON: every column of every mint.
fn mints(db: &arugula_platform::db::Db) -> Vec<Value> {
    db.token_mints(0).unwrap().iter().map(|m| serde_json::to_value(m).unwrap()).collect()
}

/// Every mint GitHub saw is a row in the log (besides the rows for read
/// tokens handed out again from the cache), and no row has a token in it.
fn every_mint_logged(h: &Shared, db: &arugula_platform::db::Db) -> Vec<Value> {
    let rows = mints(db);
    let minted = rows.iter().filter(|r| r["purpose"] != "read (cached)").count();
    assert_eq!(minted, h.lock().unwrap().minted.len(), "{rows:?}");
    assert!(!rows.is_empty());
    for r in &rows {
        assert!(!r.to_string().contains("ghs_"), "a token in the log: {r}");
        assert_eq!(r["kind"], "installation");
    }
    rows
}

async fn permission(headers: HeaderMap, Path((o, r, login)): Path<(String, String, String)>) -> Response {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
    if !auth.starts_with("Bearer ghs_test_12_") || (o, r) != ("acme".into(), "tool".into()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    // Who has each login now: jhgaylor is GitHub user 1.
    let (p, id) = match login.as_str() {
        "jhgaylor" => ("write", 1),
        "stranger" => ("none", 2),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    Json(json!({ "permission": p, "user": { "login": login, "id": id } })).into_response()
}

async fn fake_github(h: Shared) -> String {
    let r = Router::new()
        .route("/repos/{o}/{r}/installation", get(installation))
        .route("/app/installations/{id}/access_tokens", post(access_tokens))
        .route("/installation/token", axum::routing::delete(revoke))
        .route("/repos/{o}/{r}/collaborators/{login}/permission", get(permission))
        .with_state(h);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
    at
}

/// Signed as a 0.17 daemon signs: with a nonce, so two requests alike in
/// the same millisecond aren't one signature twice (control refuses a
/// replay).
fn sign_header(keys: &DeviceKeys, method: &str, path: &str, body: &[u8]) -> String {
    let ms = now_ms();
    let nonce = hex::encode(arugula_e2e::random::<16>());
    let msg = crate::auth::daemon_auth_message_v2(method, path, ms, &nonce, body);
    format!("v2 {} {ms} {nonce} {}", keys.id(), hex::encode(keys.signature(msg.as_bytes())))
}

pub(crate) fn daemon(app: &App, account: &str, name: &str) -> DeviceKeys {
    let keys = DeviceKeys::generate();
    let cert = Cert::new(&keys, account, Kind::Daemon, name);
    app.db.put_device(&cert, true, now_ms()).unwrap();
    app.db.put_daemon(account, &cert.device, name, &[]).unwrap();
    keys
}

pub(crate) type Sock = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub(crate) async fn dial(base: &str, keys: &DeviceKeys) -> Sock {
    let mut req = format!("{}/api/relay/dial", base.replace("http://", "ws://")).into_client_request().unwrap();
    req.headers_mut().insert("x-arugula-auth", sign_header(keys, "GET", "/api/relay/dial", b"").parse().unwrap());
    tokio_tungstenite::connect_async(req).await.unwrap().0
}

/// The next text message, or none within `ms`.
async fn text(s: &mut Sock, ms: u64) -> Option<Value> {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
    loop {
        let m = tokio::time::timeout_at(deadline, s.next()).await.ok()??.ok()?;
        if let Message::Text(t) = m {
            return serde_json::from_str(t.as_str()).ok();
        }
    }
}

fn sig(body: &str) -> String {
    use hmac::{KeyInit, Mac};
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(SECRET.as_bytes()).unwrap();
    mac.update(body.as_bytes());
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

async fn hook(base: &str, event: &str, delivery: &str, body: &Value, signature: Option<String>) -> (u16, Value) {
    let body = body.to_string();
    let mut req = reqwest::Client::new()
        .post(format!("{base}/github/webhook"))
        .header("x-github-event", event)
        .header("x-github-delivery", delivery)
        .header("content-type", "application/json");
    if let Some(s) = signature {
        req = req.header("x-hub-signature-256", s);
    }
    let r = req.body(body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

#[tokio::test]
async fn webhooks_poke_only_subscribed_daemons_of_allowed_accounts() {
    let (pem, public) = forge::tests::throwaway_key();
    let h: Shared = Arc::new(Mutex::new(Hub { public, ..Hub::default() }));
    let api = fake_github(h.clone()).await;
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let mut app = App::for_tests(&base);
    app.github_app = Some("arugula-test".into());
    app.platform.inner_mut().github = Some(the_app(&api, &pem));
    let app = app.start();
    // Jake signed in with GitHub; a stranger did too; someone else only
    // with a passkey.
    let now = now_ms();
    app.db.account_for("github", "1", "jhgaylor", "a1", now).unwrap();
    app.db.account_for("github", "2", "stranger", "a2", now).unwrap();
    app.db.account_for("passkey", "p3", "pk", "a3", now).unwrap();
    // Someone who was jhgaylor once (GitHub user 77) and hasn't signed in
    // since they renamed: control still has the old login for them.
    app.db.account_for("github", "77", "jhgaylor", "a4", now).unwrap();
    let k4 = daemon(&app, "a4", "was-jhgaylor");
    let (k1, k2, k3) = (daemon(&app, "a1", "geek"), daemon(&app, "a2", "theirs"), daemon(&app, "a3", "pk"));
    let svc = crate::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
    tokio::spawn(forge::heartbeat(app.clone(), Duration::from_millis(700)));

    let watch = json!({ "t": "forge.watch", "repos": ["jhgaylor/hud", "acme/tool", "nobody/x", "bad/../x"] });
    let mut socks = Vec::new();
    for k in [&k1, &k2, &k3] {
        let mut s = dial(&base, k).await;
        s.send(Message::Text(watch.to_string().into())).await.unwrap();
        socks.push(s);
    }
    let live = |v: &Value| -> Vec<(String, bool)> {
        v["repos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["repo"].as_str().unwrap().to_owned(), r["live"] == true))
            .collect()
    };
    let w1 = text(&mut socks[0], 3000).await.expect("d1 hears where it stands");
    assert_eq!(w1["t"], "forge.watching");
    assert_eq!(
        live(&w1),
        vec![("acme/tool".into(), true), ("jhgaylor/hud".into(), true), ("nobody/x".into(), false)],
        "{w1}"
    );
    assert!(w1["repos"][2]["why"].as_str().unwrap().contains("isn't installed on nobody"), "{w1}");
    let w2 = text(&mut socks[1], 3000).await.unwrap();
    assert!(live(&w2).iter().all(|(_, l)| !l), "a stranger hears nothing: {w2}");
    assert!(w2["repos"][0]["why"].as_str().unwrap().contains("collaborator"), "{w2}");
    let w3 = text(&mut socks[2], 3000).await.unwrap();
    assert!(w3["repos"][0]["why"].as_str().unwrap().contains("sign in to control with GitHub"), "{w3}");
    // The access check minted a token for acme/tool, read-only, that one
    // repository.
    let minted = h.lock().unwrap().minted.clone();
    assert!(
        minted.iter().any(|m| m["installation"] == 12
            && m["body"]["repositories"] == json!(["tool"])
            && m["body"]["permissions"]["pull_requests"] == "read"),
        "{minted:?}"
    );
    assert!(minted.iter().all(|m| m["body"]["permissions"].as_object().unwrap().values().all(|v| v == "read")));

    // A signed event: only Jake's daemon hears, and only the poke.
    let pr = json!({ "action": "review_requested", "repository": { "full_name": "jhgaylor/hud" },
        "pull_request": { "number": 7, "title": "a private title", "body": "private words" } });
    let (st, v) = hook(&base, "pull_request", "del-1", &pr, Some(sig(&pr.to_string()))).await;
    assert_eq!((st, v["relayed"].as_u64()), (202, Some(1)), "{v}");
    let p = loop {
        let m = text(&mut socks[0], 3000).await.unwrap();
        if m["t"] == "forge.poke" {
            break m;
        }
    };
    assert_eq!(
        p["poke"],
        json!({ "provider": "github", "host": "github.com", "repo": "jhgaylor/hud", "number": 7,
            "event": "pull_request", "delivery": "del-1" })
    );
    assert!(!p.to_string().contains("private"));
    // GitHub redelivers: once is enough.
    let (st, v) = hook(&base, "pull_request", "del-1", &pr, Some(sig(&pr.to_string()))).await;
    assert_eq!((st, v["duplicate"].as_bool()), (200, Some(true)));
    // Bad, wrong-secret and missing signatures are refused.
    let (st, _) = hook(&base, "pull_request", "del-2", &pr, Some("sha256=00".into())).await;
    assert_eq!(st, 401);
    let (st, _) = hook(&base, "pull_request", "del-3", &pr, None).await;
    assert_eq!(st, 401);
    let tampered = json!({ "repository": { "full_name": "jhgaylor/hud" }, "pull_request": { "number": 8 } });
    let (st, _) = hook(&base, "pull_request", "del-4", &tampered, Some(sig(&pr.to_string()))).await;
    assert_eq!(st, 401);
    // A ping.
    let ping = json!({ "zen": "Design for failure.", "hook_id": 1 });
    let (st, v) = hook(&base, "ping", "del-5", &ping, Some(sig(&ping.to_string()))).await;
    assert_eq!((st, v["pong"].as_bool()), (200, Some(true)));
    // A check run on an org's repository Jake collaborates on: his daemon,
    // for both PRs it names.
    let run = json!({ "repository": { "full_name": "acme/tool" },
        "check_run": { "head_sha": "abc", "pull_requests": [{ "number": 3 }, { "number": 5 }] } });
    let (st, v) = hook(&base, "check_run", "del-6", &run, Some(sig(&run.to_string()))).await;
    assert_eq!((st, v["relayed"].as_u64()), (202, Some(2)));
    let mut got = vec![];
    while got.len() < 2 {
        let m = text(&mut socks[0], 3000).await.unwrap();
        if m["t"] == "forge.poke" {
            got.push(m["poke"]["number"].as_u64().unwrap());
        }
    }
    assert_eq!(got, vec![3, 5]);
    // Nothing reached the others (beyond heartbeats).
    for s in &mut socks[1..] {
        while let Some(m) = text(s, 300).await {
            assert_eq!(m["t"], "forge.watching", "{m}");
        }
    }
    // The heartbeat comes again.
    let mut beat = false;
    for _ in 0..5 {
        if let Some(m) = text(&mut socks[0], 1500).await
            && m["t"] == "forge.watching"
        {
            beat = true;
            break;
        }
    }
    assert!(beat, "no heartbeat");

    // A hosted box reads through the App: a token for one repository.
    let ask = |k: &DeviceKeys, repo: &str| {
        let body = json!({ "repo": repo }).to_string();
        reqwest::Client::new()
            .post(format!("{base}/api/daemon/github/token"))
            .header("x-arugula-auth", sign_header(k, "POST", "/api/daemon/github/token", body.as_bytes()))
            .header("content-type", "application/json")
            .body(body)
            .send()
    };
    let r = ask(&k1, "jhgaylor/hud").await.unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    assert!(v["token"].as_str().unwrap().starts_with("ghs_test_11_"), "{v}");
    assert_eq!(v["login"], "jhgaylor");
    assert_eq!(v["expires_at"], "2099-01-01T00:00:00Z");
    // Asked again: the same token, from the cache.
    let again: Value = ask(&k1, "jhgaylor/hud").await.unwrap().json().await.unwrap();
    assert_eq!(again["token"], v["token"]);
    assert!(
        h.lock().unwrap().minted.iter().any(|m| m["installation"] == 11 && m["body"]["repositories"] == json!(["hud"]))
    );
    assert_eq!(ask(&k2, "jhgaylor/hud").await.unwrap().status(), 403);
    assert_eq!(ask(&k3, "jhgaylor/hud").await.unwrap().status(), 403);
    assert_eq!(ask(&k1, "nobody/x").await.unwrap().status(), 403);
    // A login that changed hands gets nothing: access is by GitHub's id.
    let r = ask(&k4, "jhgaylor/hud").await.unwrap();
    assert_eq!(r.status(), 403);
    let r = ask(&k4, "acme/tool").await.unwrap();
    assert_eq!(r.status(), 403);
    assert!(r.text().await.unwrap().contains("isn't yours any more"));
    let unsigned = reqwest::Client::new()
        .post(format!("{base}/api/daemon/github/token"))
        .json(&json!({ "repo": "jhgaylor/hud" }))
        .send()
        .await
        .unwrap();
    assert_eq!(unsigned.status(), 401);
    // M167: each mint (the access check's for acme/tool, the box's for
    // jhgaylor/hud) is in the token log, read-only, for one repository,
    // naming who it was for. A token used again inside the platform (an
    // access check's) adds no row; one handed to a box again does, `read
    // (cached)`, naming the box's account even when the token was first
    // minted for someone else's access check (the stranger's, on hud).
    let rows = every_mint_logged(&h, &app.platform.inner().db);
    let boxes: Vec<&Value> = rows.iter().filter(|r| r["caller"] == "a1" && r["repo_id"] == 101).collect();
    assert_eq!(boxes.len(), 2, "a row for each of the box's two asks: {rows:?}");
    assert!(boxes.iter().any(|r| r["purpose"] == "read (cached)"), "{rows:?}");
    let row = |repo: u64| rows.iter().find(|r| r["repo_id"] == repo).unwrap_or_else(|| panic!("{rows:?}"));
    assert_eq!(row(102)["installation_id"], 12);
    assert!(["a1", "a2"].contains(&row(102)["caller"].as_str().unwrap()), "{rows:?}");
    assert_eq!((row(101)["installation_id"].as_u64(), row(101)["purpose"].as_str()), (Some(11), Some("read")));
    assert!(rows.iter().all(|r| !r["permissions"].as_str().unwrap().contains("write")), "{rows:?}");

    // Jake's daemon hangs up: its subscription goes with the socket.
    let s = socks.remove(0);
    drop(s);
    for _ in 0..50 {
        if app.forge.route("jhgaylor/hud").is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(app.forge.route("jhgaylor/hud").is_empty());
    let pr2 = json!({ "repository": { "full_name": "jhgaylor/hud" }, "pull_request": { "number": 7 } });
    let (_, v) = hook(&base, "pull_request", "del-7", &pr2, Some(sig(&pr2.to_string()))).await;
    assert_eq!(v["relayed"], 0);
}

fn the_app(api: &str, pem: &str) -> GithubApp {
    GithubApp::new("42".into(), "arugula-test".into(), SECRET.into(), api, parse_pem(pem).unwrap())
}

/// Wait for the next `forge.poke`, past heartbeats.
async fn poke(s: &mut Sock) -> Value {
    loop {
        let m = text(s, 3000).await.expect("a poke");
        if m["t"] == "forge.poke" {
            return m;
        }
    }
}

/// M161: the platform as its own server, on a database file of its own,
/// behind the hub's gateway. A signed webhook to the hub lands on the
/// platform, which records the delivery and hands the pokes to the hub over
/// `HubPort`; the daemon watching hears the same bytes as ever. The access
/// check and the hosted box's token are the platform's, asked over
/// `PlatformPort`. A platform started again on the same database, with
/// nothing in memory, knows the delivery when GitHub sends it again; one
/// with no App refuses it with an error.
#[tokio::test]
async fn webhooks_land_on_the_platform_out_of_process() {
    let (pem, public) = forge::tests::throwaway_key();
    let h: Shared = Arc::new(Mutex::new(Hub { public, ..Hub::default() }));
    let api = fake_github(h.clone()).await;
    let path = std::env::temp_dir().join(format!("arugula-m161-{}.db", hex::encode(arugula_e2e::random::<16>())));
    let open = || arugula_platform::db::Db::open(&path).unwrap();
    let s = crate::gateway_wire::split_github(open(), |hub| hub, Some(the_app(&api, &pem))).await;
    let now = now_ms();
    s.hub.db.account_for("github", "1", "jhgaylor", "a1", now).unwrap();
    s.hub.db.account_for("github", "2", "stranger", "a2", now).unwrap();
    let (k1, k2) = (daemon(&s.hub, "a1", "geek"), daemon(&s.hub, "a2", "theirs"));

    // Where each repository stands is the platform's answer.
    let mut sock = dial(&s.base, &k1).await;
    let watch = json!({ "t": "forge.watch", "repos": ["jhgaylor/hud", "nobody/x"] });
    sock.send(Message::Text(watch.to_string().into())).await.unwrap();
    let w = text(&mut sock, 3000).await.expect("where it stands");
    assert_eq!(w["t"], "forge.watching");
    assert_eq!((w["repos"][0]["live"].as_bool(), w["repos"][1]["live"].as_bool()), (Some(true), Some(false)), "{w}");
    assert!(w["repos"][1]["why"].as_str().unwrap().contains("isn't installed on nobody"), "{w}");

    // A signed event through the hub's gateway: the platform takes it, and
    // the daemon hears the poke.
    let pr = json!({ "action": "opened", "repository": { "full_name": "jhgaylor/hud" },
        "pull_request": { "number": 7, "title": "a private title" } });
    let (st, v) = hook(&s.base, "pull_request", "del-1", &pr, Some(sig(&pr.to_string()))).await;
    assert_eq!((st, v["relayed"].as_u64()), (202, Some(1)), "{v}");
    let p = poke(&mut sock).await;
    assert_eq!(
        p,
        json!({ "t": "forge.poke", "poke": { "provider": "github", "host": "github.com", "repo": "jhgaylor/hud",
            "number": 7, "event": "pull_request", "delivery": "del-1" } })
    );
    let recorded = |db: &arugula_platform::db::Db, d: &str| db.delivery_event(d).unwrap();
    assert_eq!(recorded(&s.platform.db, "del-1").as_deref(), Some("pull_request"), "the platform recorded it");
    // A bad signature is refused, and not recorded.
    let (st, _) = hook(&s.base, "pull_request", "del-2", &pr, Some("sha256=00".into())).await;
    assert_eq!(st, 401);
    assert_eq!(recorded(&s.platform.db, "del-2"), None);

    // A hosted box's token, minted by the platform.
    let ask = |k: &DeviceKeys, repo: &str| {
        let body = json!({ "repo": repo }).to_string();
        reqwest::Client::new()
            .post(format!("{}/api/daemon/github/token", s.base))
            .header("x-arugula-auth", sign_header(k, "POST", "/api/daemon/github/token", body.as_bytes()))
            .header("content-type", "application/json")
            .body(body)
            .send()
    };
    let r = ask(&k1, "jhgaylor/hud").await.unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    assert!(v["token"].as_str().unwrap().starts_with("ghs_test_11_"), "{v}");
    assert_eq!((v["login"].as_str(), v["app"].as_str()), (Some("jhgaylor"), Some("arugula-test")));
    let r = ask(&k2, "jhgaylor/hud").await.unwrap();
    assert_eq!(r.status(), 403, "a stranger: the platform's refusal is the answer");
    assert!(r.text().await.unwrap().contains("collaborator"));
    assert_eq!(ask(&k1, "bad/../x").await.unwrap().status(), 400);
    // The platform logged the mints, in its own database.
    let rows = every_mint_logged(&h, &s.platform.db);
    assert!(rows.iter().all(|r| r["purpose"] == "read" && r["caller"] == "a1"), "{rows:?}");

    // The platform starts again on the same database: nothing in memory,
    // and GitHub's redelivery is known.
    drop(sock);
    let again = crate::gateway_wire::split_github(open(), |hub| hub, Some(the_app(&api, &pem))).await;
    let (st, v) = hook(&again.base, "pull_request", "del-1", &pr, Some(sig(&pr.to_string()))).await;
    assert_eq!((st, v["duplicate"].as_bool()), (200, Some(true)), "{v}");
    let (st, v) = hook(&again.base, "pull_request", "del-3", &pr, Some(sig(&pr.to_string()))).await;
    assert_eq!((st, v["relayed"].as_u64()), (202, Some(0)), "a new one is taken: {v}");
    assert!(recorded(&again.platform.db, "del-3").is_some());
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }

    // A platform started without the App's secrets refuses the webhook out
    // loud (GitHub keeps it, to redeliver), never drops it.
    let bare = crate::gateway_wire::split_github(arugula_platform::db::Db::memory(), |hub| hub, None).await;
    let (st, v) = hook(&bare.base, "pull_request", "del-4", &pr, Some(sig(&pr.to_string()))).await;
    assert_eq!(st, 503, "{v}");
    assert!(v["error"].as_str().unwrap().contains("no GitHub App key"), "{v}");
    assert_eq!(bare.platform.db.delivery_event("del-4").unwrap(), None);
}

/// M167: a read's token is read-only and kept; a send's token writes the
/// one thing it names, is minted for each send, and is never the cached
/// read token. Every mint is a row in the log, with no token in it.
#[tokio::test]
async fn reads_and_sends_get_their_own_tokens() {
    let (pem, public) = forge::tests::throwaway_key();
    let h: Shared = Arc::new(Mutex::new(Hub { public, ..Hub::default() }));
    let api = fake_github(h.clone()).await;
    let gh = the_app(&api, &pem);
    let (http, db) = (reqwest::Client::new(), arugula_platform::db::Db::memory());
    let c = Caller { http: &http, db: &db, who: "a1" };
    let body = |n: usize| h.lock().unwrap().minted[n]["body"].clone();

    let hud = Repo::Name("jhgaylor/hud");
    let read = gh.read_token(&c, 11, hud, Purpose::Read).await.unwrap();
    assert_eq!(body(0)["repositories"], json!(["hud"]));
    assert!(body(0)["permissions"].as_object().unwrap().values().all(|v| v == "read"));
    let again = gh.read_token(&c, 11, hud, Purpose::Ingest).await.unwrap();
    assert_eq!(again.secret(), read.secret(), "kept");
    assert_eq!(h.lock().unwrap().minted.len(), 1, "a cache hit isn't a mint");
    // The same token handed out to a box, for someone else: no mint, but
    // a row naming them.
    let b = Caller { who: "a2", ..c };
    assert_eq!(gh.hand_out(&b, 11, hud).await.unwrap().secret(), read.secret());
    assert_eq!(h.lock().unwrap().minted.len(), 1);

    // A send, on the same repository, while that read token is kept: its
    // one call has the token, and then it's revoked.
    let send = gh.write_token(&c, 11, hud, Writes::Issues).await.unwrap();
    assert_eq!(body(1)["permissions"], json!({ "metadata": "read", "issues": "write" }));
    assert_eq!(body(1)["repositories"], json!(["hud"]));
    let s1 = send.use_once(async |t: &str| t.to_owned()).await;
    assert_ne!(s1, read.secret(), "never the cached read token");
    assert_eq!(h.lock().unwrap().revoked, vec![s1.clone()], "revoked after its call");
    let send2 = gh.write_token(&c, 11, hud, Writes::Issues).await.unwrap();
    let s2 = send2.use_once(async |t: &str| t.to_owned()).await;
    assert_ne!(s2, s1, "a send's token isn't kept");
    // Nor does a send's token reach the read cache.
    let read2 = gh.read_token(&c, 11, hud, Purpose::Read).await.unwrap();
    assert_eq!(read2.secret(), read.secret());
    assert_eq!(h.lock().unwrap().minted.len(), 3);
    // By id, a review: pull requests only. Dropped unused, it's revoked
    // all the same.
    let review = gh.write_token(&c, 11, Repo::Id(101), Writes::PullRequests).await.unwrap();
    assert_eq!(review.send(), Writes::PullRequests);
    assert_eq!(
        body(3),
        json!({ "repository_ids": [101], "permissions": { "metadata": "read", "pull_requests": "write" } })
    );
    drop(review);
    for _ in 0..50 {
        if h.lock().unwrap().revoked.len() == 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(h.lock().unwrap().revoked.len(), 3, "an unused write token is revoked when dropped");
    assert!(h.lock().unwrap().revoked.iter().all(|t| *t != read.secret()), "the read token wasn't revoked");

    // GitHub's answer covering two repositories, another one, or not
    // saying: refused, the token revoked, and the mint still logged.
    for name in ["jhgaylor/wide", "jhgaylor/wrong", "jhgaylor/bare"] {
        let before = h.lock().unwrap().revoked.len();
        assert!(gh.write_token(&c, 11, Repo::Name(name), Writes::Issues).await.is_err(), "{name}");
        assert!(gh.read_token(&c, 11, Repo::Name(name), Purpose::Read).await.is_err(), "{name}");
        let n = h.lock().unwrap().minted.len();
        let revoked = h.lock().unwrap().revoked[before..].to_vec();
        assert_eq!(revoked, vec![format!("ghs_test_11_{}", n - 1), format!("ghs_test_11_{n}")], "{name}");
    }
    // By id: the id GitHub confirms is taken, and an answer for another
    // id is refused.
    gh.write_token(&c, 11, Repo::Id(102), Writes::Issues).await.unwrap().use_once(async |_: &str| ()).await;
    let before = h.lock().unwrap().revoked.len();
    assert!(gh.read_token(&c, 11, Repo::Id(999), Purpose::Read).await.is_err(), "covers nothing");
    assert_eq!(h.lock().unwrap().revoked.len(), before + 1);

    let rows = every_mint_logged(&h, &db);
    // Rows in the same millisecond come in any order.
    let mut purposes: Vec<&str> = rows.iter().map(|r| r["purpose"].as_str().unwrap()).collect();
    purposes.sort_unstable();
    assert_eq!(
        purposes,
        [
            "read",
            "read",
            "read",
            "read",
            "read",
            "read (cached)",
            "send:issues",
            "send:issues",
            "send:issues",
            "send:issues",
            "send:issues",
            "send:issues",
            "send:pull_requests"
        ]
    );
    let cached = rows.iter().find(|r| r["purpose"] == "read (cached)").unwrap();
    assert_eq!((cached["caller"].as_str(), cached["repo_id"].as_u64()), (Some("a2"), Some(101)));
    assert!(rows.iter().all(|r| r["installation_id"] == 11), "{rows:?}");
    assert_eq!(rows.iter().filter(|r| r["caller"] == "a1").count(), rows.len() - 1);
    let on = |id: Value| rows.iter().filter(|r| r["repo_id"] == id).count();
    assert_eq!((on(json!(101)), on(Value::Null)), (5, 6), "a name GitHub didn't confirm names none: {rows:?}");
    let issues = rows.iter().find(|r| r["purpose"] == "send:issues" && r["repo_id"] == 101).unwrap();
    assert_eq!(issues["permissions"], r#"{"issues":"write","metadata":"read"}"#);
}

/// By hand, once, read-only: the real App (`ARUGULA_REAL_GITHUB_APP_ENV`
/// names its env file, as the manifest flow left it): a JWT, the App's
/// installations, an installation token for one repository, and a PR read
/// with it. Prints no secret.
/// `ARUGULA_REAL_GITHUB_APP_ENV=… ARUGULA_REAL_REPO=o/r ARUGULA_REAL_PR=N cargo test -p arugula-control real_app -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn real_app_reads() {
    let env = std::fs::read_to_string(std::env::var("ARUGULA_REAL_GITHUB_APP_ENV").unwrap()).unwrap();
    let get = |k: &str| {
        env.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).map(|v| v.trim().trim_matches('"').to_owned())
    };
    let pem = std::fs::read_to_string(get("GITHUB_APP_PRIVATE_KEY_FILE").unwrap()).unwrap();
    let app = GithubApp::new(
        get("GITHUB_APP_ID").unwrap(),
        get("GITHUB_APP_SLUG").unwrap(),
        String::new(),
        "https://api.github.com",
        parse_pem(&pem).unwrap(),
    );
    let (http, db) = (reqwest::Client::new(), arugula_platform::db::Db::memory());
    let c = Caller { http: &http, db: &db, who: "platform" };
    let jwt = app.jwt(now_ms() / 1000).unwrap();
    let r = http
        .get("https://api.github.com/app/installations")
        .bearer_auth(&jwt)
        .header("User-Agent", "arugula-control")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .unwrap();
    println!("GET /app/installations: {}", r.status());
    let v: Value = r.json().await.unwrap();
    for i in v.as_array().into_iter().flatten() {
        println!("  installation {} on {} ({})", i["id"], i["account"]["login"], i["repository_selection"]);
    }
    let repo = std::env::var("ARUGULA_REAL_REPO").unwrap();
    let pr = std::env::var("ARUGULA_REAL_PR").unwrap();
    let install = app.installation(&http, &repo).await.unwrap().expect("installed there");
    println!("installation for {repo}: {} on {}", install.id, install.account);
    let owner = repo.split('/').next().unwrap();
    println!(
        "may {owner} hear {repo}: {:?}",
        app.may(&c, &arugula_control_wire::internal::GithubUser { id: install.account_id, login: owner.into() }, &repo)
            .await
            .map(|i| i.id)
    );
    let t = app.read_token(&c, install.id, Repo::Name(&repo), Purpose::Read).await.unwrap();
    println!("installation token minted, {} chars, expires {}", t.secret().len(), t.expires_at);
    println!("logged: {:?}", db.token_mints(0).unwrap());
    let token = t.secret();
    let r = http
        .get(format!("https://api.github.com/repos/{repo}/pulls/{pr}"))
        .bearer_auth(token)
        .header("User-Agent", "arugula-control")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .unwrap();
    let status = r.status();
    let v: Value = r.json().await.unwrap();
    println!("GET repos/{repo}/pulls/{pr}: {status}: {:?} ({}), head {}", v["title"], v["state"], v["head"]["sha"]);
}

/// M168: the hub asks a machine a typed op over its relay socket and waits
/// for its answer. One that never said it knows the op is `Unknown`,
/// unasked; one not connected is `Offline`; another daemon can't answer
/// someone else's ask.
#[tokio::test]
async fn a_machine_answers_an_op_it_said_it_knows() {
    use arugula_control_wire::forge::{DaemonOp, ForgeSend, OpAnswer};
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let app = App::for_tests(&base).start();
    let svc = crate::router(app.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>();
    tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
    let op = DaemonOp::ForgeSend(ForgeSend { draft: "dft_1".into(), account: "a1".into(), ..Default::default() });

    let (new, old, other) = (daemon(&app, "a1", "new"), daemon(&app, "a1", "old"), daemon(&app, "a2", "theirs"));
    assert_eq!(app.relay.ask(&new.id(), &op).await, OpAnswer::Offline);
    let mut s = dial(&base, &new).await;
    let mut o = dial(&base, &old).await;
    let mut t = dial(&base, &other).await;
    s.send(Message::Text(json!({ "t": "ops", "ops": ["forge.send"] }).to_string().into())).await.unwrap();
    for _ in 0..100 {
        if !app.relay.ops_of(&new.id()).is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(app.relay.ops_of(&new.id()), ["forge.send"]);
    assert_eq!(app.relay.ask(&old.id(), &op).await, OpAnswer::Unknown, "it never said");
    let ask = {
        let (app, id, op) = (app.clone(), new.id(), op.clone());
        tokio::spawn(async move { app.relay.ask(&id, &op).await })
    };
    let m = loop {
        let m = text(&mut s, 3000).await.expect("the ask");
        if m["t"] == "op" && m["op"]["draft"] == "dft_1" {
            break m;
        }
    };
    assert_eq!(m["op"]["op"], "forge.send");
    let id = m["id"].as_str().unwrap().to_owned();
    // Another machine answering it changes nothing.
    t.send(Message::Text(json!({ "t": "op.done", "id": id, "answer": { "answer": "done" } }).to_string().into()))
        .await
        .unwrap();
    s.send(Message::Text(
        json!({ "t": "op.done", "id": id, "answer": { "answer": "done", "url": "u" } }).to_string().into(),
    ))
    .await
    .unwrap();
    assert_eq!(ask.await.unwrap(), OpAnswer::Done { url: Some("u".into()) });
    drop((o.close(None).await, t.close(None).await));
}
