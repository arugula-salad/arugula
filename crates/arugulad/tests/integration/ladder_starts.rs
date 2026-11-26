//! Ladder starts (aru-143), on two daemons of one account and a fake
//! control that relays between them, signs machine tokens and start
//! tickets, and holds a board's grant (its machines and a cap).
//!
//! `start_agent {board, issue, to}` on one machine starts an issue
//! agent on the other: in a worktree of that machine's own clone, attached
//! to the row with its ticket, kept on the block and in the remote sessions
//! log. Without an issue it's the board's lead. An offline target and a
//! grant at its cap are refused in control's words. The target checks the
//! ticket itself: from the machine it names, of its own account, for the
//! start it names, once, and never with remote input off.
//!
//! The lead staffs a board's rows inside the grant (Arugula #1438): with a
//! cap of two it starts two issue agents, one here and one on the other
//! machine; a third is refused naming the cap; `close {pane, to}`
//! stops the other machine's, which tells control at once, so the next
//! start fits without waiting; and a revoked grant refuses the next start.
//! On its own machine (Arugula #1471), the lead's own `close {pane}`
//! closes its board's issue agents in their tabs, and frees their slots
//! the same way; another agent, or another tab's pane, is still refused.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use arugula_control_wire::ladder::START_USE;
use arugula_control_wire::machine_token::USE;
use arugula_e2e::{Cert, DeviceKeys, Kind, mux::Mux};
use arugula_testkit::{Daemon, arugulad};
use axum::{
    Json, Router,
    extract::{
        Path as UrlPath, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, KeyInit, Mac};
use rmcp::{
    ClientHandler, RoleClient, ServiceExt,
    model::{CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, Implementation},
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, TokioChildProcess, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const ACCOUNT: &str = "a1";
const BOARD: &str = "brd_1";
const PROJECT: &str = "prj_1";

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

fn issuer() -> SigningKey {
    SigningKey::from_bytes(&[42; 32])
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// A JWT as the hub signs one.
fn jwt(claims: &Value) -> String {
    let h = b64(&serde_json::to_vec(&json!({ "alg": "EdDSA", "typ": "JWT", "kid": "k1" })).unwrap());
    let p = b64(&serde_json::to_vec(claims).unwrap());
    let s = b64(&issuer().sign(format!("{h}.{p}").as_bytes()).to_bytes());
    format!("{h}.{p}.{s}")
}

fn jti() -> String {
    hex::encode(arugula_e2e::random::<16>())
}

/// The machine a daemon's signed request came from (`v2 ID ...`).
fn signer(h: &HeaderMap) -> String {
    let a = h.get("x-arugula-auth").and_then(|v| v.to_str().ok()).unwrap_or_default();
    a.split(' ').nth(1).unwrap_or_default().to_owned()
}

/// Control, as far as these two machines need it.
#[derive(Clone, Default)]
struct Fake {
    url: Arc<Mutex<String>>,
    certs: Arc<Mutex<Vec<Cert>>>,
    /// (id, name) of each machine.
    machines: Arc<Mutex<Vec<(String, String)>>>,
    /// The relay's socket to each machine that dialed.
    muxes: Arc<Mutex<HashMap<String, Mux>>>,
    /// Machines control says are offline.
    offline: Arc<Mutex<Vec<String>>>,
    /// The board's grant: how many issue agents at once, and whether it was
    /// revoked.
    cap: Arc<AtomicUsize>,
    revoked: Arc<AtomicBool>,
    issued: Arc<Mutex<Vec<Value>>>,
    attached: Arc<Mutex<Vec<Value>>>,
    leads: Arc<Mutex<Vec<Value>>>,
    /// Each block's last state, by (machine, pane), as its machine told it.
    states: Arc<Mutex<HashMap<(String, String), String>>>,
    dialed: Arc<AtomicBool>,
}

impl Fake {
    fn name_of(&self, id: &str) -> String {
        self.machines.lock().unwrap().iter().find(|(i, _)| i == id).map(|(_, n)| n.clone()).unwrap_or_default()
    }

    /// The issue agents the grant has running, as control counts them: a
    /// ticket not attached yet, or an attached block its machine hasn't
    /// said is closed.
    fn running(&self) -> usize {
        let attached = self.attached.lock().unwrap().clone();
        let states = self.states.lock().unwrap().clone();
        let issued = self.issued.lock().unwrap().clone();
        issued
            .iter()
            .filter(|t| t["rung"] == "issue")
            .filter(|t| match attached.iter().find(|a| a["ticket"] == t["jti"]) {
                None => true,
                Some(a) => {
                    let at = (a["signer"].as_str().unwrap().to_owned(), a["pane"].as_str().unwrap().to_owned());
                    states.get(&at).is_none_or(|s| s != "closed")
                }
            })
            .count()
    }
}

async fn about() -> Json<Value> {
    Json(json!({ "control": true, "daemon_auth": 2 }))
}

async fn jwks() -> Json<Value> {
    Json(json!({ "keys": [{
        "kty": "OKP", "crv": "Ed25519", "kid": "k1", "alg": "EdDSA", "use": "sig",
        "x": b64(issuer().verifying_key().as_bytes()),
    }]}))
}

async fn trust(State(f): State<Fake>) -> Json<Value> {
    Json(json!({ "certs": *f.certs.lock().unwrap(), "revocations": [] }))
}

async fn directory(State(f): State<Fake>) -> Json<Value> {
    let off = f.offline.lock().unwrap().clone();
    let daemons: Vec<Value> = f
        .machines
        .lock()
        .unwrap()
        .iter()
        .map(|(id, name)| json!({ "id": id, "name": name, "account": ACCOUNT, "online": !off.contains(id) }))
        .collect();
    Json(json!({ "daemons": daemons }))
}

/// An `agents` token for the signing machine to reach `id`.
async fn machine_token(State(f): State<Fake>, h: HeaderMap, UrlPath(id): UrlPath<String>) -> Json<Value> {
    let iat = now();
    let claims = json!({
        "iss": *f.url.lock().unwrap(), "sub": ACCOUNT, "aud": id, "use": USE, "role": "agents",
        "name": "me", "iat": iat, "exp": iat + 300, "jti": jti(), "from": signer(&h),
    });
    Json(json!({ "token": jwt(&claims), "expires": iat + 300, "role": "agents" }))
}

/// A board's start ticket, within its grant: both machines, `cap` at once.
async fn start_ticket(
    State(f): State<Fake>,
    h: HeaderMap,
    UrlPath(board): UrlPath<String>,
    Json(a): Json<Value>,
) -> Response {
    let to = a["to"].as_str().unwrap_or_default().to_owned();
    let refuse = |s: StatusCode, why: String, reason: &str| (s, Json(json!({ "error": why, "reason": reason })));
    if f.offline.lock().unwrap().contains(&to) {
        let others: Vec<String> =
            f.machines.lock().unwrap().iter().filter(|(id, _)| *id != to).map(|(_, n)| n.clone()).collect();
        let why =
            format!("{} is offline; the board's grant also allows {} (online)", f.name_of(&to), others.join(", "));
        return refuse(StatusCode::CONFLICT, why, "offline").into_response();
    }
    if f.revoked.load(Ordering::SeqCst) {
        let why = "this board has no grant to start agents: its person gives one with \"Run this board\"".to_owned();
        return refuse(StatusCode::FORBIDDEN, why, "no-grant").into_response();
    }
    let (cap, running) = (f.cap.load(Ordering::SeqCst), f.running());
    if a["rung"] == "issue" && running >= cap {
        let why = format!(
            "the board's grant allows {cap} issue agents at once, and {running} are running: stop one, or ask for a new card"
        );
        return refuse(StatusCode::FORBIDDEN, why, "cap").into_response();
    }
    let iat = now();
    let claims = json!({
        "iss": *f.url.lock().unwrap(), "sub": ACCOUNT, "aud": to, "use": START_USE, "from": signer(&h),
        "board": board, "project": PROJECT, "grant": "grt_1", "rung": a["rung"], "issue": a["issue"],
        "recipe": a["recipe"], "iat": iat, "exp": iat + 60, "jti": jti(),
    });
    f.issued.lock().unwrap().push(claims.clone());
    Json(json!({ "ticket": jwt(&claims), "expires": iat + 60 })).into_response()
}

async fn claim_check() -> Json<Value> {
    Json(json!({ "held": null }))
}

async fn attach_block(State(f): State<Fake>, h: HeaderMap, Json(mut a): Json<Value>) -> Json<Value> {
    a["signer"] = json!(signer(&h));
    f.attached.lock().unwrap().push(a);
    Json(json!({ "on": [{ "board": BOARD, "title": "The ladder", "row": "row_1" }] }))
}

/// A machine's word on one of its blocks.
async fn block_state(State(f): State<Fake>, h: HeaderMap, Json(a): Json<Value>) -> Json<Value> {
    let (pane, state) = (a["pane"].as_str().unwrap_or_default(), a["block"]["state"].as_str().unwrap_or_default());
    f.states.lock().unwrap().insert((signer(&h), pane.to_owned()), state.to_owned());
    Json(json!({}))
}

/// The board as an agent reads it: here, only who leads it (the last lead
/// taken), as `BoardLead`.
async fn board(State(f): State<Fake>, UrlPath(board): UrlPath<String>) -> Response {
    let Some(l) = f.leads.lock().unwrap().last().cloned() else {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "no such board" }))).into_response();
    };
    let lead = json!({ "machine": l["signer"], "session": l["session"], "pane": l["pane"], "at": 1 });
    Json(json!({ "board": { "id": board, "lead": lead } })).into_response()
}

async fn lead(State(f): State<Fake>, h: HeaderMap, Json(mut a): Json<Value>) -> Json<Value> {
    a["signer"] = json!(signer(&h));
    f.leads.lock().unwrap().push(a);
    Json(json!({}))
}

/// A machine's relay socket: control's end of its stream mux.
async fn dial(State(f): State<Fake>, h: HeaderMap, up: WebSocketUpgrade) -> Response {
    let id = signer(&h);
    up.on_upgrade(move |ws| async move {
        let (mux, mut out) = Mux::new(None);
        f.muxes.lock().unwrap().insert(id.clone(), mux.clone());
        f.dialed.store(true, Ordering::SeqCst);
        let (mut tx, mut rx) = ws.split();
        loop {
            tokio::select! {
                o = out.recv() => match o {
                    Some(b) => if tx.send(Message::Binary(b.into())).await.is_err() { break },
                    None => break,
                },
                m = rx.next() => match m {
                    Some(Ok(Message::Binary(b))) => if mux.handle(&b).is_err() { break },
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                },
            }
        }
    })
}

/// A machine's channel to machine `id`, spliced onto a stream of its mux:
/// each WebSocket message a length-prefixed frame, as control's relay does.
async fn client(State(f): State<Fake>, UrlPath(id): UrlPath<String>, up: WebSocketUpgrade) -> Response {
    let Some(mux) = f.muxes.lock().unwrap().get(&id).cloned() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "that daemon isn't connected to the relay").into_response();
    };
    let Ok(stream) = mux.open() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "that daemon just went away").into_response();
    };
    up.on_upgrade(move |ws: WebSocket| async move {
        let (mut wtx, mut wrx) = ws.split();
        let (mut rd, mut wr) = tokio::io::split(stream);
        let up = async move {
            while let Some(Ok(m)) = wrx.next().await {
                match m {
                    Message::Binary(b) => {
                        let mut fr = (b.len() as u32).to_be_bytes().to_vec();
                        fr.extend_from_slice(&b);
                        if wr.write_all(&fr).await.is_err() {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        };
        let down = async move {
            let mut len = [0u8; 4];
            while rd.read_exact(&mut len).await.is_ok() {
                let mut b = vec![0u8; u32::from_be_bytes(len) as usize];
                if rd.read_exact(&mut b).await.is_err() || wtx.send(Message::Binary(b.into())).await.is_err() {
                    break;
                }
            }
        };
        tokio::select! { _ = up => {}, _ = down => {} }
    })
}

async fn serve(f: Fake) -> String {
    let ok = || async { Json(json!({})) };
    let app = Router::new()
        .route("/control.json", get(about))
        .route("/.well-known/jwks.json", get(jwks))
        .route("/api/daemon/trust", get(trust))
        .route("/api/daemon/peers", get(ok))
        .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
        .route("/api/daemon/access", post(ok))
        .route("/api/directory", get(directory))
        .route("/api/machines/{id}/machine-token", post(machine_token))
        .route("/api/relay/dial", get(dial))
        .route("/api/relay/c/{id}", get(client))
        .route("/api/daemon/boards/{board}", get(board))
        .route("/api/daemon/boards/{board}/start-ticket", post(start_ticket))
        .route("/api/daemon/boards/{board}/lead", post(lead))
        .route("/api/daemon/projects/{p}/claim-check", post(claim_check))
        .route("/api/daemon/projects/{p}/attach-block", post(attach_block))
        .route(arugula_control_wire::boards::AGENT_BLOCK_STATE, axum::routing::put(block_state))
        .with_state(f.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    *f.url.lock().unwrap() = at.clone();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    at
}

/// A daemon started once to make its key, its certificate under the
/// account's root, and the daemon (stopped).
fn machine(name: &str, root: &DeviceKeys) -> (Daemon, DeviceKeys, Cert) {
    let mut d =
        arugulad!(name).no_provider().no_tailscale().args(["--direct-url", "http://127.0.0.1:0"]).wait_secs(30).start();
    let keys = DeviceKeys::load_or_create(&d.state.join("daemon.key")).unwrap();
    let mut cert = Cert::new(&keys, ACCOUNT, Kind::Daemon, name);
    cert.sign_with(root);
    d.stop();
    (d, keys, cert)
}

/// Joins `d` to control at `url`, and starts it.
fn join(d: &mut Daemon, url: &str, root: &DeviceKeys, cert: &Cert, certs: &[Cert]) {
    let saved = json!({
        "url": url,
        "trust": { "account": ACCOUNT, "root": root.id() },
        "cert": cert,
        "certs": certs,
        "login": "me",
    });
    std::fs::write(d.state.join("control.json"), saved.to_string()).unwrap();
    d.start();
    d.wait_for("joined", || d.get("/api/host")["control_state"]["state"] == "joined");
}

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

/// A clone of `acme/widgets`, from a bare repository at `.../acme/widgets.git`.
fn clone_in(root: &Path, name: &str) -> PathBuf {
    let bare = root.join("acme").join("widgets.git");
    if !bare.exists() {
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        git(&seed, &["init", "-q", "-b", "main"]);
        std::fs::write(seed.join("README"), "widgets\n").unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "first"]);
        std::fs::create_dir_all(bare.parent().unwrap()).unwrap();
        git(root, &["clone", "-q", "--bare", &seed.display().to_string(), &bare.display().to_string()]);
    }
    let dir = root.join(name);
    git(root, &["clone", "-q", &bare.display().to_string(), &dir.display().to_string()]);
    dir
}

fn cli_bin() -> PathBuf {
    let bin = Path::new(env!("CARGO_BIN_EXE_arugulad")).with_file_name("arugula");
    let status = Command::new(env!("CARGO")).args(["build", "-q", "-p", "arugula"]).status().unwrap();
    assert!(status.success(), "building the CLI");
    bin
}

#[derive(Clone)]
struct Client;

impl ClientHandler for Client {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(ClientCapabilities::default(), Implementation::new("claude-code", "1"))
    }
}

type Session = RunningService<RoleClient, Client>;

async fn bridge(d: &Daemon) -> Session {
    let mut cmd = tokio::process::Command::new(cli_bin());
    arugula_testkit::scrub_env(cmd.as_std_mut());
    cmd.arg("--socket").arg(d.sock()).arg("mcp").env_remove("ARUGULA_PANE");
    Client.serve(TokioChildProcess::new(cmd).unwrap()).await.expect("connecting through arugula mcp")
}

/// Agent block `id`'s own MCP session, with the token the daemon gives it
/// (`mcp::Tokens`: an HMAC of "arugula block {id}" under `mcp/key`).
async fn as_block(d: &Daemon, id: u64) -> Session {
    let key = std::fs::read(d.state.join("mcp/key")).unwrap();
    let mut m = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(&key).unwrap();
    m.update(format!("arugula block {id}").as_bytes());
    let token = format!("ilb_{id}_{}", hex::encode(m.finalize().into_bytes()));
    let cfg =
        StreamableHttpClientTransportConfig::with_uri(format!("http://127.0.0.1:{}/mcp", d.port)).auth_header(token);
    Client.serve(StreamableHttpClientTransport::from_config(cfg)).await.expect("connecting as the block")
}

async fn call(s: &Session, args: Value) -> CallToolResult {
    call_tool(s, "start_agent", args).await
}

async fn call_tool(s: &Session, tool: &'static str, args: Value) -> CallToolResult {
    let params = CallToolRequestParams::new(tool).with_arguments(args.as_object().cloned().unwrap());
    s.call_tool(params).await.unwrap()
}

fn text(r: &CallToolResult) -> String {
    r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect()
}

/// A ticket for machine `aud`, as control signs one, with `edit` applied.
fn ticket(url: &str, aud: &str, from: &str, edit: impl FnOnce(&mut Value)) -> String {
    let iat = now();
    let mut c = json!({
        "iss": url, "sub": ACCOUNT, "aud": aud, "use": START_USE, "from": from, "board": BOARD,
        "project": PROJECT, "rung": "lead", "iat": iat, "exp": iat + 60, "jti": jti(),
    });
    edit(&mut c);
    jwt(&c)
}

/// Two machines of one account, alpha and bravo, joined to a fake control
/// whose board grant allows `cap` issue agents at once, bravo on the relay.
struct Pair {
    a: Daemon,
    b: Daemon,
    a_id: String,
    b_id: String,
    f: Fake,
    url: String,
}

async fn pair(cap: usize) -> Pair {
    let root = DeviceKeys::generate();
    let mut root_cert = Cert::new(&root, ACCOUNT, Kind::Browser, "laptop");
    root_cert.sign_with(&root);
    let (mut a, a_keys, a_cert) = machine("alpha", &root);
    let (mut b, b_keys, b_cert) = machine("bravo", &root);
    let (a_id, b_id) = (a_keys.id(), b_keys.id());
    let certs = vec![root_cert, a_cert.clone(), b_cert.clone()];

    let f = Fake::default();
    f.cap.store(cap, Ordering::SeqCst);
    *f.certs.lock().unwrap() = certs.clone();
    *f.machines.lock().unwrap() = vec![(a_id.clone(), "alpha".into()), (b_id.clone(), "bravo".into())];
    let url = serve(f.clone()).await;
    join(&mut a, &url, &root, &a_cert, &certs);
    join(&mut b, &url, &root, &b_cert, &certs);
    arugula_testkit::wait_for("bravo on the relay", Duration::from_secs(30), || {
        f.muxes.lock().unwrap().contains_key(&b_id)
    });
    Pair { a, b, a_id, b_id, f, url }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lead_starts_agents_on_another_machine_within_the_grant() {
    let Pair { a, b, a_id, b_id, f, url } = pair(1).await;

    // Each machine has its own clone of the board's repo, where a pane works.
    let work = a.state.join("ladder-work");
    std::fs::create_dir_all(&work).unwrap();
    let clone_a = clone_in(&work, "alpha-clone");
    let clone_b = clone_in(&work, "bravo-clone");
    b.post("/api/run", json!({ "cwd": clone_b }));

    let s = bridge(&a).await;

    // Never every check off for another agent, here or there.
    let wild = json!({ "prompt": "Fix #7", "board": BOARD, "issue": "#7", "to": "bravo", "permission_mode": "bypassPermissions" });
    let r = call(&s, wild).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("skips every check"), "{}", text(&r));

    // An issue agent on bravo, for #7, with settings that don't ask a card
    // for each command.
    let r = call(
        &s,
        json!({ "prompt": "Fix #7", "board": BOARD, "issue": "#7", "to": "bravo", "cwd": clone_a,
                "permission_mode": "auto", "allow": ["Read", "Bash"] }),
    )
    .await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    let started = r.structured_content.clone().unwrap();
    assert_eq!(started["id"], b_id.as_str(), "{started}");
    assert_eq!(started["session"], "issue-7");
    let block = started["block"].as_u64().unwrap();
    let wt = started["cwd"].as_str().unwrap();
    // Bravo answers with git's own path for its clone, through any symlink
    // (macOS's /var is /private/var): compare it with the clone's real path.
    let real_b = std::fs::canonicalize(&clone_b).unwrap();
    let real_wt = std::fs::canonicalize(wt).unwrap();
    assert!(real_wt.starts_with(&real_b) && real_wt.ends_with("ladder/issue-7"), "{wt} isn't in {}", real_b.display());
    assert!(Path::new(wt).join("README").exists(), "a worktree of bravo's own clone");

    // Control was asked by alpha, for bravo; bravo attached its block with the ticket.
    let issued = f.issued.lock().unwrap().clone();
    assert_eq!(issued.len(), 1);
    assert_eq!((issued[0]["from"].as_str(), issued[0]["aud"].as_str()), (Some(a_id.as_str()), Some(b_id.as_str())));
    assert_eq!(issued[0]["rung"], "issue");
    let attached = f.attached.lock().unwrap().clone();
    assert_eq!(attached.len(), 1, "{attached:?}");
    assert_eq!(attached[0]["signer"], b_id.as_str());
    assert_eq!(attached[0]["machine"], b_id.as_str());
    assert_eq!(attached[0]["pane"], block.to_string());
    assert_eq!(attached[0]["ticket"], issued[0]["jti"]);
    assert_eq!(attached[0]["session"], "issue-7");
    assert_eq!(attached[0]["issue"], "#7");

    // Bravo keeps who started it on the block, and in its log.
    let config = |block: u64| -> Value {
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(b.state.join("layout.json")).unwrap_or_default())
                .unwrap_or_default();
        saved["panes"][block.to_string()]["config"].clone()
    };
    let kept = || config(block)["work"]["ladder"].clone();
    b.wait_for("the start kept with the block", || kept()["ticket"] == issued[0]["jti"]);
    let kept = kept();
    // Its settings as the lead gave them; its recipe (Arugula #1440) is in
    // its prompt (`whole_tests`), not worn.
    let issue_config = config(block);
    assert_eq!(issue_config["permission_mode"], "auto");
    assert_eq!(issue_config["allow"], json!([{ "tool": "Read" }, { "tool": "Bash" }]));
    assert!(issue_config.get("recipe").is_none(), "{issue_config}");
    // Named from the start, so it leads #7's thread by that name now.
    assert_eq!(issue_config["title"], "issue-7");
    assert_eq!(attached[0]["lead"], "issue-7", "{attached:?}");
    assert_eq!((kept["by_machine"].as_str(), kept["by_id"].as_str()), (Some("alpha"), Some(a_id.as_str())), "{kept}");
    assert_eq!(
        (kept["board"].as_str(), kept["rung"].as_str(), kept["grant"].as_str()),
        (Some(BOARD), Some("issue"), Some("grt_1"))
    );
    let log = std::fs::read_to_string(b.state.join("remote-sessions.jsonl")).unwrap();
    let line: Value =
        log.lines().filter_map(|l| serde_json::from_str(l).ok()).find(|l: &Value| l["event"] == "agent").unwrap();
    assert_eq!(
        (line["from"].as_str(), line["rung"].as_str(), line["issue"].as_str()),
        (Some("alpha"), Some("issue"), Some("#7"))
    );

    // Offline: refused at once, naming what else the grant allows.
    f.offline.lock().unwrap().push(b_id.clone());
    let r = call(&s, json!({ "prompt": "Lead it", "board": BOARD, "to": "bravo" })).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("bravo is offline") && text(&r).contains("alpha"), "{}", text(&r));
    f.offline.lock().unwrap().clear();

    // The board's lead on bravo: no issue.
    let r = call(&s, json!({ "prompt": "Lead it", "board": BOARD, "to": "bravo", "cwd": clone_a })).await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    let lead_block = r.structured_content.clone().unwrap()["block"].as_u64().unwrap();
    let leads = f.leads.lock().unwrap().clone();
    assert_eq!(leads.len(), 1);
    assert_eq!(leads[0]["signer"], b_id.as_str());
    assert_eq!(leads[0]["pane"], lead_block);
    assert_eq!(leads[0]["session"], "lead-brd1");

    // At the grant's cap: control's refusal, as it said it.
    let r = call(&s, json!({ "prompt": "Fix #8", "board": BOARD, "issue": "#8", "to": "bravo" })).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("allows 1 issue agents at once"), "{}", text(&r));
    drop(s);

    // Bravo checks a ticket itself, sent here by its owner (no channel:
    // from bravo itself).
    let start = |t: String, issue: Option<&str>| {
        let mut body = json!({ "ticket": t, "board": BOARD, "rung": "lead", "prompt": "hi" });
        if let Some(i) = issue {
            body["issue"] = json!(i);
        }
        b.raw("POST", "/api/a2a/start", Some(body))
    };
    let (status, said) = start(ticket(&url, &b_id, &a_id, |_| {}), None);
    assert_eq!(status, 403, "{said}");
    assert!(said.contains("another machine to send"), "{said}");
    let (status, said) = start(ticket(&url, &b_id, &b_id, |c| c["sub"] = json!("a2")), None);
    assert_eq!(status, 403);
    assert!(said.contains("another account's"), "{said}");
    let (status, said) = start(ticket(&url, &a_id, &b_id, |_| {}), None);
    assert_eq!(status, 403);
    assert!(said.contains("another machine"), "{said}");
    let (status, said) = start(ticket(&url, &b_id, &b_id, |_| {}), Some("#9"));
    assert_eq!(status, 403);
    assert!(said.contains("isn't the one its ticket names"), "{said}");
    let good = ticket(&url, &b_id, &b_id, |_| {});
    let (status, said) = start(good.clone(), None);
    assert_eq!(status, 200, "{said}");
    let (status, said) = start(good, None);
    assert_eq!(status, 403);
    assert!(said.contains("already used"), "{said}");
    std::fs::write(b.state.join("sharing.json"), json!({ "remote_input": false }).to_string()).unwrap();
    let (status, said) = start(ticket(&url, &b_id, &b_id, |_| {}), None);
    assert_eq!(status, 403);
    assert!(said.contains("remote input is off"), "{said}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lead_staffs_its_board_inside_the_grant_and_stops_to_free_a_slot() {
    let Pair { a, b, a_id, b_id, f, url: _ } = pair(2).await;
    let work = a.state.join("ladder-work");
    std::fs::create_dir_all(&work).unwrap();
    let clone_a = clone_in(&work, "alpha-clone");
    let clone_b = clone_in(&work, "bravo-clone");
    a.post("/api/run", json!({ "cwd": clone_a }));
    let shell_b = b.post("/api/run", json!({ "cwd": clone_b }))["pane"].as_u64().unwrap();
    let s = bridge(&a).await;
    let start = |issue: &str, to: Option<&str>| {
        let mut args = json!({ "prompt": format!("Fix {issue}"), "board": BOARD, "issue": issue, "cwd": clone_a });
        if let Some(to) = to {
            args["to"] = json!(to);
        }
        args
    };

    // Two issue agents under a cap of two: one on bravo, one here.
    let r = call(&s, start("#1", Some("bravo"))).await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    let on_b = r.structured_content.clone().unwrap();
    assert_eq!(on_b["id"], b_id.as_str());
    let r = call(&s, start("#2", None)).await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    let here = r.structured_content.clone().unwrap();
    assert_eq!(here["id"], a_id.as_str());
    let attached = f.attached.lock().unwrap().clone();
    assert_eq!(attached.len(), 2, "each start is on its row: {attached:?}");
    assert_eq!((attached[0]["issue"].as_str(), attached[0]["signer"].as_str()), (Some("#1"), Some(b_id.as_str())));
    assert_eq!((attached[1]["issue"].as_str(), attached[1]["signer"].as_str()), (Some("#2"), Some(a_id.as_str())));

    // A third is refused, naming the cap.
    let r = call(&s, start("#3", Some("bravo"))).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("allows 2 issue agents at once, and 2 are running"), "{}", text(&r));

    // Another machine's pane is closed only if a ladder start opened it.
    let r = call_tool(&s, "close", json!({ "pane": shell_b, "to": "bravo" })).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("isn't an agent a ladder start opened"), "{}", text(&r));

    // Stopping bravo's frees its slot at once: the next start fits without
    // waiting for bravo's next report.
    let block = on_b["block"].as_u64().unwrap();
    let r = call_tool(&s, "close", json!({ "pane": block, "to": "bravo" })).await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    assert!(text(&r).contains(&format!("Closed %{block} on bravo")), "{}", text(&r));
    assert!(text(&r).contains("its slot under the board's grant is free"), "{}", text(&r));
    let r = call(&s, start("#3", Some("bravo"))).await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));

    // Here, close does the same.
    let mine = here["block"].as_u64().unwrap();
    let r = call_tool(&s, "close", json!({ "pane": mine })).await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    assert_eq!(f.states.lock().unwrap().get(&(a_id.clone(), mine.to_string())).map(String::as_str), Some("closed"));
    assert_eq!(f.running(), 1);

    // Revoked: the next start is refused.
    f.revoked.store(true, Ordering::SeqCst);
    let r = call(&s, start("#4", Some("bravo"))).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("no grant to start agents"), "{}", text(&r));
    assert_eq!(f.attached.lock().unwrap().len(), 3, "nothing more started");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lead_closes_its_boards_agents_in_their_own_tabs_here() {
    let Pair { a, b: _b, a_id, b_id: _, f, url: _ } = pair(2).await;
    let work = a.state.join("ladder-work");
    std::fs::create_dir_all(&work).unwrap();
    let clone_a = clone_in(&work, "alpha-clone");
    let shell = a.post("/api/run", json!({ "cwd": clone_a }))["pane"].as_u64().unwrap();
    let s = bridge(&a).await;

    // The board's lead here, and two issue agents for its board, each in a
    // tab of its own.
    let r = call(&s, json!({ "prompt": "Lead it", "board": BOARD, "cwd": clone_a })).await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    let lead = r.structured_content.clone().unwrap()["block"].as_u64().unwrap();
    let mut issue = vec![];
    for i in ["#1", "#2"] {
        let r = call(&s, json!({ "prompt": format!("Fix {i}"), "board": BOARD, "issue": i, "cwd": clone_a })).await;
        assert_ne!(r.is_error, Some(true), "{}", text(&r));
        issue.push(r.structured_content.clone().unwrap()["block"].as_u64().unwrap());
    }
    drop(s);
    let panes = a.get("/api/panes");
    let tab = |id: u64| panes.as_array().unwrap().iter().find(|p| p["id"] == id).unwrap()["tab"].clone();
    assert_ne!(tab(lead), tab(issue[0]), "the issue agent is in another tab");
    assert_eq!(f.running(), 2);

    // Another issue agent can't close its sibling: it doesn't lead the board.
    let sibling = as_block(&a, issue[1]).await;
    let r = call_tool(&sibling, "close", json!({ "pane": issue[0] })).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("in another tab"), "{}", text(&r));
    drop(sibling);

    // The lead can't close a pane in another tab that no start opened.
    let lead_s = as_block(&a, lead).await;
    let r = call_tool(&lead_s, "close", json!({ "pane": shell })).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("in another tab"), "{}", text(&r));

    // It closes its board's issue agent, and control is told at once, so
    // its slot is free.
    let r = call_tool(&lead_s, "close", json!({ "pane": issue[0] })).await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    assert!(text(&r).contains(&format!("Closed %{}", issue[0])), "{}", text(&r));
    assert_eq!(f.states.lock().unwrap().get(&(a_id.clone(), issue[0].to_string())).map(String::as_str), Some("closed"));
    assert_eq!(f.running(), 1);
    assert!(a.get("/api/panes").as_array().unwrap().iter().any(|p| p["id"] == shell), "the shell is still open");
}
