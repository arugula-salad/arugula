//! M255: `arugulad --hosted` runs agent sessions (thread = session,
//! Arugula #129). Against a fake control that enrolls the box and lists its
//! workspace's sessions on every pull, as the platform does, with
//! `fake_acp.py` as the session's Claude Code, and a stand-in for sudo
//! (`ARUGULA_HOSTED_SUDO`) that runs as the test's user what the box runs
//! as a session user, and says so:
//! - a session's first prompt runs as its slot's user, in a clone of the
//!   workspace's repo on the session's branch, with the repo's CLAUDE.md as
//!   text, and its reports are signed by the box, numbered from 1;
//! - a follow-up queued mid-turn waits for the turn, then runs, credited to
//!   its own author; a cancel stops the turn;
//! - a question and a permission go out as `needs_input`, and the answers
//!   people give come back to the agent;
//! - a daemon restart, between turns or mid-turn, neither loses nor doubles
//!   a prompt, and the agent's conversation goes on;
//! - with three sessions on the box a fourth waits, until one is stopped;
//! - the workspace's finish step (M261) runs after each turn as the
//!   session's user: passing, the turn is pushed and the session done;
//!   failing, the agent gets one fix turn with its output, pushed if it
//!   passes then, and failing again it's done, unpushed;
//! - a session that ends leaves its slot clean: what its pushes left, its
//!   checkout and its user's `~/.claude` go.
//! - a turn that commits pushes the session's branch, and only it, with a
//!   token control mints for that push and the box revokes after (M259); a
//!   turn with nothing new pushes nothing.
//! - a Try 3's three sessions (M264) run at once, each in a slot of its
//!   own, on the harness asked for, and a review waits for a slot, then
//!   runs read-only: no finish step, no push, and its reply reported.
//! - before a rebuild (Arugula #209), control's `hosted.archive` over the
//!   relay sends each open agent block's transcript, a session's too, and
//!   the blocks stay open.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_control_wire::{
    hosted::{EnrollRequest, Enrollment},
    work::{FinishResult, FinishStep, SessionAction, SessionDetail, SessionPrompt, SessionReport, SessionWork, Work},
};
use arugula_e2e::{
    Cert, DeviceKeys, Kind, Trust,
    hosted::{Delegation, HostedCert, HostedKey},
    session::{Event, Kind as EvKind, State},
};
use arugula_testkit::arugulad;
use axum::{
    Json, Router,
    extract::{State as Axum, ws::WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

const PROJECT: &str = "prj_1";
const WORKSPACE: &str = "hw_1";

/// A session as the platform has it.
#[derive(Clone)]
struct Ses {
    work: SessionWork,
    /// Stopped by a person: no longer listed, and its reports refused.
    stopped: bool,
}

#[derive(Clone)]
struct Fake {
    url: Arc<Mutex<String>>,
    root: Arc<DeviceKeys>,
    root_cert: Cert,
    key: Arc<HostedKey>,
    delegation: Delegation,
    enrolled: Arc<Mutex<Option<(Enrollment, String)>>>,
    sessions: Arc<Mutex<BTreeMap<String, Ses>>>,
    /// Every report taken, in order.
    reports: Arc<Mutex<Vec<SessionReport>>>,
    /// Reports refused, and why.
    refused: Arc<Mutex<Vec<String>>>,
    pulls: Arc<Mutex<u32>>,
    /// Every turn the box asked M185's gate for, as it asked.
    asked: Arc<Mutex<Vec<Value>>>,
    /// Every push token the box asked for (M259), and those it revoked.
    pushes: Arc<Mutex<Vec<Value>>>,
    revoked: Arc<Mutex<Vec<String>>>,
    /// The relay socket, to ask the box an op on (Arugula #209), and what
    /// the box said on it.
    relay: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>>,
    said: Arc<Mutex<Vec<Value>>>,
    /// The transcripts the box archived (M184).
    transcripts: Arc<Mutex<Vec<Value>>>,
}

impl Fake {
    fn new() -> Self {
        let root = DeviceKeys::generate();
        let mut root_cert = Cert::new(&root, "acct1", Kind::Browser, "laptop");
        root_cert.sign_with(&root);
        let key = HostedKey::generate();
        let mut delegation = Delegation {
            project: PROJECT.into(),
            owner: "account:acct1".into(),
            key: key.public(),
            at: 1,
            by: String::new(),
            sig: String::new(),
        };
        delegation.sign_with(&root);
        Fake {
            url: Default::default(),
            root: Arc::new(root),
            root_cert,
            key: Arc::new(key),
            delegation,
            enrolled: Default::default(),
            sessions: Default::default(),
            reports: Default::default(),
            refused: Default::default(),
            pulls: Default::default(),
            asked: Default::default(),
            pushes: Default::default(),
            revoked: Default::default(),
            relay: Default::default(),
            said: Default::default(),
            transcripts: Default::default(),
        }
    }

    fn device(&self) -> String {
        self.enrolled.lock().unwrap().as_ref().map(|(e, _)| e.cert.device.clone()).unwrap_or_default()
    }

    /// A new session, queued with its first prompt.
    fn start(&self, id: &str, slug: &str, prompt: SessionPrompt) {
        self.start_with(id, slug, prompt, None);
    }

    /// [`Fake::start`], on a workspace whose spec has a finish step.
    fn start_with(&self, id: &str, slug: &str, prompt: SessionPrompt, finish: Option<FinishStep>) {
        self.start_as(id, slug, prompt, finish, None, false);
    }

    /// [`Fake::start_with`], on `harness`, or a review (M264).
    fn start_as(
        &self,
        id: &str,
        slug: &str,
        prompt: SessionPrompt,
        finish: Option<FinishStep>,
        harness: Option<&str>,
        review: bool,
    ) {
        let work = SessionWork {
            session: id.into(),
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            asker: prompt.author.clone(),
            repo: Some("acme/app".into()),
            branch: format!("arugula/{}-{slug}", &id[4..12]),
            state: "queued".into(),
            n: 0,
            prompts: vec![prompt],
            actions: vec![],
            finish,
            harness: harness.map(str::to_owned),
            review,
        };
        self.sessions.lock().unwrap().insert(id.into(), Ses { work, stopped: false });
    }

    fn queue(&self, id: &str, p: SessionPrompt) {
        self.sessions.lock().unwrap().get_mut(id).unwrap().work.prompts.push(p);
    }

    fn ask(&self, id: &str, a: SessionAction) {
        self.sessions.lock().unwrap().get_mut(id).unwrap().work.actions.push(a);
    }

    fn stop(&self, id: &str) {
        self.sessions.lock().unwrap().get_mut(id).unwrap().stopped = true;
    }

    fn state(&self, id: &str) -> String {
        self.sessions.lock().unwrap()[id].work.state.clone()
    }

    /// Session `id`'s reports, each checked against the box's key.
    fn of(&self, id: &str) -> Vec<(SessionReport, SessionDetail)> {
        let sign = self.enrolled.lock().unwrap().as_ref().unwrap().1.clone();
        let device = self.device();
        let out: Vec<SessionReport> =
            self.reports.lock().unwrap().iter().filter(|r| r.session == id).cloned().collect();
        out.into_iter()
            .map(|r| {
                let ev = Event {
                    session: &r.session,
                    n: r.n,
                    kind: EvKind::parse(&r.kind).unwrap(),
                    from: State::parse(&r.from).unwrap(),
                    to: State::parse(&r.to).unwrap(),
                    at: r.at,
                    detail: &r.detail,
                    daemon: &device,
                };
                assert!(ev.verify(&sign, &r.sig), "{r:?} isn't signed by the box");
                let d = serde_json::from_str(&r.detail).unwrap();
                (r, d)
            })
            .collect()
    }

    /// Session `id`'s moves and turns (no heartbeats or errors), as
    /// `(kind, from, to)`.
    fn moves(&self, id: &str) -> Vec<(String, String, String)> {
        self.of(id)
            .into_iter()
            .filter(|(r, _)| r.kind == "state" || r.kind == "turn")
            .map(|(r, _)| (r.kind, r.from, r.to))
            .collect()
    }

    /// The prompts each turn of session `id` started with, in order.
    fn turns(&self, id: &str) -> Vec<Vec<String>> {
        self.of(id)
            .into_iter()
            .filter(|(r, d)| r.kind == "state" && r.to == "working" && !d.prompts.is_empty())
            .map(|(_, d)| d.prompts)
            .collect()
    }

    fn pulls(&self) -> u32 {
        *self.pulls.lock().unwrap()
    }

    /// Session `id`'s turn reports' details, in order.
    fn turn_reports(&self, id: &str) -> Vec<SessionDetail> {
        self.of(id).into_iter().filter(|(r, _)| r.kind == "turn").map(|(_, d)| d).collect()
    }
}

async fn about(Axum(f): Axum<Fake>) -> Json<Value> {
    let url = f.url.lock().unwrap().clone();
    Json(json!({ "control": true, "daemon_auth": 2, "url": url }))
}

async fn enroll(Axum(f): Axum<Fake>, Json(b): Json<EnrollRequest>) -> Response {
    let mut cert = HostedCert::new(&f.delegation, WORKSPACE, "box", &b.noise, &b.sign, arugula_e2e::now_ms());
    f.key.sign(&mut cert);
    let e = Enrollment {
        url: f.url.lock().unwrap().clone(),
        cert,
        delegation: f.delegation.clone(),
        revocations: vec![],
        team: None,
        roster: None,
        certs: Default::default(),
        trust: Some(Trust { account: "acct1".into(), root: f.root.id() }),
        account_certs: vec![f.root_cert.clone()],
        account_revocations: vec![],
        legacy_app_children: None,
        purpose: None,
    };
    *f.enrolled.lock().unwrap() = Some((e.clone(), b.sign));
    Json(e).into_response()
}

async fn trust(Axum(f): Axum<Fake>) -> Response {
    match f.enrolled.lock().unwrap().clone() {
        Some((e, _)) => Json(e).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

/// The relay socket: what the box says on it is kept, and what's sent to
/// [`Fake::relay`] goes to it.
async fn dial(Axum(f): Axum<Fake>, up: WebSocketUpgrade) -> Response {
    up.on_upgrade(|ws| async move {
        use axum::extract::ws::Message;
        use futures_util::{SinkExt as _, StreamExt as _};
        let (mut tx, mut rx) = ws.split();
        let (asks, mut asked) = tokio::sync::mpsc::unbounded_channel::<String>();
        *f.relay.lock().unwrap() = Some(asks);
        loop {
            tokio::select! {
                m = rx.next() => match m {
                    Some(Ok(Message::Text(t))) => {
                        if let Ok(v) = serde_json::from_str::<Value>(t.as_str()) {
                            f.said.lock().unwrap().push(v);
                        }
                    }
                    Some(Ok(_)) => {}
                    _ => break,
                },
                Some(a) = asked.recv() => {
                    if tx.send(Message::Text(a.into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    })
}

/// A transcript the box archived (M184), kept as the platform would.
async fn archived(Axum(f): Axum<Fake>, Json(b): Json<Value>) -> Json<Value> {
    let mut all = f.transcripts.lock().unwrap();
    all.push(b);
    Json(json!({ "id": format!("htx_{}", all.len()) }))
}

/// Every session that hasn't ended, as the platform lists them (the box
/// keeps to its own cap).
async fn work(Axum(f): Axum<Fake>) -> Json<Work> {
    *f.pulls.lock().unwrap() += 1;
    let sessions = f.sessions.lock().unwrap().values().filter(|s| !s.stopped).map(|s| s.work.clone()).collect();
    Json(Work { items: vec![], revokes: vec![], sessions })
}

/// As the platform takes a report: the next number only, a move the box
/// may make, then its prompts and actions are taken off the lists.
async fn report(Axum(f): Axum<Fake>, Json(r): Json<SessionReport>) -> Json<Value> {
    let mut all = f.sessions.lock().unwrap();
    let Some(s) = all.get_mut(&r.session) else { return Json(json!({ "kind": "refused", "why": "no such session" })) };
    let refuse = |why: String, now: Option<&str>, next: Option<u64>| {
        f.refused.lock().unwrap().push(format!("{} n {}: {why}", r.session, r.n));
        Json(json!({ "kind": "refused", "why": why, "now": now, "next": next }))
    };
    if s.stopped {
        return refuse("stopped".into(), Some("stopped"), None);
    }
    if r.n != s.work.n + 1 {
        return refuse(format!("n {} isn't {}", r.n, s.work.n + 1), None, Some(s.work.n + 1));
    }
    if r.from != s.work.state {
        return refuse(format!("from {} but it's {}", r.from, s.work.state), None, None);
    }
    let d: SessionDetail = serde_json::from_str(&r.detail).unwrap();
    let starts = r.kind == "state" && r.to == "working" && (r.from == "queued" || r.from == "done");
    if starts && d.prompts.is_empty() && !(d.in_block && r.from == "done") {
        return refuse("a turn starts with the queued prompts it names".into(), None, None);
    }
    s.work.n = r.n;
    s.work.state = r.to.clone();
    s.work.prompts.retain(|p| !d.prompts.contains(&p.id));
    s.work.actions.retain(|a| !d.actions.contains(&a.id));
    f.reports.lock().unwrap().push(r);
    Json(json!({ "kind": "taken" }))
}

/// M185's gate: every turn may run.
async fn turn(Axum(f): Axum<Fake>, Json(b): Json<Value>) -> Json<Value> {
    f.asked.lock().unwrap().push(b);
    Json(json!({ "turn": "trn_1", "pays": "project", "auth": "key" }))
}

/// The push token for a session's branch, as the platform mints it (M259).
async fn push_token(Axum(f): Axum<Fake>, Json(b): Json<Value>) -> Json<Value> {
    f.pushes.lock().unwrap().push(b);
    let revoke = format!("{}/installation/token", f.url.lock().unwrap());
    Json(json!({ "kind": "given", "token": PUSH_TOKEN, "expires_at": "2099-01-01T00:00:00Z", "revoke": revoke }))
}

async fn revoke(Axum(f): Axum<Fake>, h: axum::http::HeaderMap) -> StatusCode {
    let bearer = h.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    f.revoked.lock().unwrap().push(bearer);
    StatusCode::NO_CONTENT
}

const PUSH_TOKEN: &str = "ghs_m259_push_token_for_the_test";

async fn turn_end() -> Json<Value> {
    Json(json!({ "ended": true }))
}

fn serve(rt: &tokio::runtime::Runtime, f: Fake) -> String {
    rt.block_on(async {
        let app = Router::new()
            .route("/control.json", get(about))
            .route("/api/hosted/enroll", post(enroll))
            .route("/api/hosted/trust", get(trust))
            .route("/api/relay/dial", get(dial))
            .route("/api/hosted/work", get(work))
            .route("/api/hosted/work/session", post(report))
            .route("/api/hosted/work/session/push", post(push_token))
            .route("/installation/token", axum::routing::delete(revoke))
            .route("/api/hosted/turns", post(turn))
            .route("/api/hosted/turns/{turn}/end", post(turn_end))
            .route("/api/hosted/transcripts", post(archived))
            .with_state(f.clone());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        *f.url.lock().unwrap() = at.clone();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    })
}

/// Wait until `ok` holds, for 40 s; else fail with what the platform heard
/// and the daemon's log.
fn wait(marks: &Path, f: &Fake, what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(40);
    while !ok() {
        if std::time::Instant::now() > deadline {
            let log = std::fs::read_to_string(marks.join("arugulad.log")).unwrap_or_default();
            let tail: Vec<&str> = log
                .lines()
                .filter(|l| l.contains("hosted") || l.contains("agent") || l.contains("WARN") || l.contains("ERROR"))
                .collect();
            let reports: Vec<String> = f
                .reports
                .lock()
                .unwrap()
                .iter()
                .map(|r| format!("{} {} {} {}->{} {}", &r.session[..6], r.n, r.kind, r.from, r.to, r.detail))
                .collect();
            panic!(
                "timed out waiting for {what}\nreports:\n{}\nrefused: {:?}\npulls: {}\nlog:\n{}",
                reports.join("\n"),
                f.refused.lock().unwrap(),
                f.pulls(),
                tail[tail.len().saturating_sub(60)..].join("\n")
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn ses(c: char) -> String {
    format!("ses_{}", c.to_string().repeat(32))
}

fn prompt(n: u32, author: &str, text: &str) -> SessionPrompt {
    SessionPrompt {
        id: format!("spr_{n:032}"),
        author: author.into(),
        author_name: String::new(),
        source: "member".into(),
        association: None,
        text: text.into(),
        at: n as u64,
    }
}

fn action(n: u32, by: &str, kind: &str) -> SessionAction {
    SessionAction {
        id: format!("sac_{n:032}"),
        by: by.into(),
        by_name: String::new(),
        kind: kind.into(),
        request: None,
        answer: None,
        option: None,
        reason: None,
        at: n as u64,
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// The block's transcript, as `"type: text"` lines.
fn transcript(d: &arugula_testkit::Daemon, block: u64) -> Vec<String> {
    let s = d.get(&format!("/api/blocks/{block}"))["state"].clone();
    s["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| format!("{}: {}", e["type"].as_str().unwrap_or_default(), e["text"].as_str().unwrap_or_default()))
        .collect()
}

/// The block session `id` runs in, as its first report says.
fn block_of(f: &Fake, id: &str) -> u64 {
    f.of(id).iter().find_map(|(_, d)| d.block.as_ref()?.parse().ok()).expect("its first report names its block")
}

/// The user prompts in a transcript that start with `text`.
fn asked(lines: &[String], text: &str) -> usize {
    lines.iter().filter(|l| l.starts_with(&format!("user: {text}"))).count()
}

/// The box under test, its fake control, and where they keep things.
struct Rig {
    _rt: tokio::runtime::Runtime,
    f: Fake,
    marks: PathBuf,
    homes: PathBuf,
    sudo_log: PathBuf,
    d: arugula_testkit::Daemon,
    /// The box's state dir, the workspace's clone, and the bare repo that
    /// stands in for GitHub for the push (M259).
    state: PathBuf,
    repo: PathBuf,
    remote: PathBuf,
}

/// A hosted box, enrolled into a fake control, whose workspace has a clone
/// of acme/app, with stand-ins for its session users and their agent.
fn rig(tag: &str) -> Rig {
    rig_with(tag, &[])
}

/// [`rig`], with more in the daemon's environment.
fn rig_with(tag: &str, more: &[(&str, &str)]) -> Rig {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::new();
    let url = serve(&rt, f.clone());
    let marks: PathBuf = std::env::temp_dir().join(format!("arugula-hosted-sessions-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&marks);
    std::fs::create_dir_all(&marks).unwrap();

    // The workspace's clone of acme/app, as its setup made it.
    let repo = marks.join("app");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "box@example.invalid"]);
    git(&repo, &["config", "user.name", "box"]);
    std::fs::write(repo.join("README.md"), "# app\n").unwrap();
    std::fs::write(repo.join("CLAUDE.md"), "Run the tests with make check.\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "init"]);
    git(&repo, &["remote", "add", "origin", "https://github.com/acme/app.git"]);
    // GitHub, for the push: a bare repo with the workspace's main.
    let github = marks.join("github");
    let remote = github.join("acme/app.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "-q", "--bare", "-b", "main"]);
    git(&repo, &["push", "-q", remote.to_str().unwrap(), "main"]);
    let state = marks.join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(
        state.join("hosted-setup.json"),
        json!({ "status": "ready", "reported": true, "repos": [{ "url": "https://github.com/acme/app", "dir": repo }] })
            .to_string(),
    )
    .unwrap();

    // sudo's stand-in: says what it was asked, then runs it as the test's
    // user.
    let bin = marks.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let sudo = bin.join("sudo");
    let sudo_log = marks.join("sudo.log");
    std::fs::write(
        &sudo,
        format!(
            "#!/bin/sh\necho \"$*\" >> {}\n[ \"$1\" = -n ] && shift\n[ \"$1\" = -u ] && shift 2\n[ \"$1\" = -- ] && shift\nexec \"$@\"\n",
            sudo_log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&sudo, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let homes = marks.join("homes");
    // What the setup adds to session users' environment (Arugula #215).
    let session_env = marks.join("tools-env");
    std::fs::write(&session_env, "CARGO_TARGET_DIR=~/.cache/arugula/cargo-target\n").unwrap();
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake_acp.py");
    let agent = format!("sh -c 'FAKE_ACP_DIR=$HOME/.fake-acp exec python3 {}'", fake.display());

    let mut b = arugulad!(tag);
    for (k, v) in more {
        b = b.env(k, v);
    }
    let d = b
        .no_wisp()
        .no_tailscale()
        .state_dir(&state)
        .args(["--hosted"])
        .env("ARUGULA_HOSTED_TICKET", "ticket-1")
        .env("ARUGULA_CONTROL_URL", &url)
        .env("ARUGULA_WORK_PULL_MS", "150")
        // No `runner` user here, and the session users are stand-ins.
        .env("ARUGULA_HOSTED_RUN_USER", "")
        .env("ARUGULA_HOSTED_SESSION_HOMES", &homes)
        .env("ARUGULA_HOSTED_SUDO", &sudo)
        .env("ARUGULA_HOSTED_SESSION_ENV", &session_env)
        .env("ARUGULA_HOSTED_SESSION_AGENT", &agent)
        .env("ARUGULA_SESSION_HEARTBEAT_MS", "400")
        .env("ARUGULA_GITHUB_GIT", format!("file://{}", github.display()))
        .env("RUST_LOG", "info")
        .args(["--log-file", &marks.join("arugulad.log").to_string_lossy()])
        .wait_secs(30)
        .start();
    wait(&marks, &f, "the box enrolled", || !f.device().is_empty());
    Rig { _rt: rt, f, marks, homes, sudo_log, d, state, repo, remote }
}

#[test]
fn a_hosted_machine_runs_sessions() {
    let Rig { _rt, f, marks, homes, sudo_log, mut d, state, repo, remote } = rig("main");

    // A session's first prompt: a turn as ses1, in its own clone, on its
    // branch, with the repo's CLAUDE.md as text.
    let a = ses('a');
    // Its author's login comes with it, for the transcript's note.
    let mut first = prompt(1, "acct1", "hello");
    first.author_name = "jhgaylor".into();
    f.start(&a, "fix-readme", first);
    wait(&marks, &f, "session a's first turn", || f.state(&a) == "done");
    assert_eq!(
        f.moves(&a),
        [("state", "queued", "working"), ("turn", "working", "working"), ("state", "working", "done")]
            .map(|(k, a, b)| (k.to_owned(), a.to_owned(), b.to_owned()))
    );
    let of = f.of(&a);
    assert_eq!(of.iter().map(|(r, _)| r.n).collect::<Vec<_>>(), (1..=of.len() as u64).collect::<Vec<_>>());
    let branch = format!("arugula/{}-fix-readme", &a[4..12]);
    let (_, turn) = of.iter().find(|(r, _)| r.kind == "turn").unwrap();
    assert_eq!(turn.branch.as_deref(), Some(branch.as_str()), "{turn:?}");
    assert_eq!(turn.pushed, Some(false), "nothing of its own to push");
    assert!(f.pushes.lock().unwrap().is_empty(), "so it asked for no token");
    let gated = f.asked.lock().unwrap().clone();
    assert_eq!(gated, [json!({ "agent": "claude", "session": a })], "its turns are its asker's");
    let checkout = homes.join("ses1").join("work").join(&a);
    assert_eq!(git(&checkout, &["rev-parse", "--abbrev-ref", "HEAD"]), branch);
    assert_eq!(git(&checkout, &["remote", "get-url", "origin"]), "https://github.com/acme/app.git");
    assert!(git(&checkout, &["rev-parse", "--git-dir"]) == ".git", "a clone of its own, not a worktree");
    let said = std::fs::read_to_string(&sudo_log).unwrap();
    assert!(said.contains(&format!("-n -u ses1 -- env -i HOME={} USER=ses1", homes.join("ses1").display())), "{said}");
    assert!(said.contains("ANTHROPIC_API_KEY=arugula-brokered sh -c"), "the agent, with the placeholder: {said}");
    // Arugula #215: the setup's tools first on its PATH, and the setup's
    // environment, `~/` its user's home.
    let theirs = format!(
        "LOGNAME=ses1 PATH=/opt/arugula/tools/bin:/usr/local/bin:/usr/bin:/bin LANG=C.UTF-8 CARGO_TARGET_DIR={}",
        homes.join("ses1/.cache/arugula/cargo-target").display()
    );
    assert!(
        said.lines().any(|l| l.contains(&theirs) && l.contains("ANTHROPIC_API_KEY=arugula-brokered sh -c")),
        "the agent gets the setup's tools and env: {said}"
    );
    assert!(
        said.lines().any(|l| l.starts_with("-n -u ses1 -- env -i") && l.contains(" git -C ") && l.contains(" switch ")),
        "the session's git runs as its user: {said}"
    );
    assert!(
        said.lines().any(|l| l.starts_with("-n -u ses1 -- env -i") && l.contains("GIT_OPTIONAL_LOCKS=0 sh -c")),
        "so does the turn's work: {said}"
    );
    let fake_dir = homes.join("ses1/.fake-acp");
    let sent: String = std::fs::read_dir(&fake_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("prompt-"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect();
    assert!(sent.contains("Run the tests with make check."), "the repo's CLAUDE.md, in the first prompt: {sent}");
    let mcp: String = std::fs::read_dir(&fake_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("mcp-"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect();
    assert!(!mcp.contains("arugula"), "a session gets no Arugula MCP server: {mcp}");
    let block = block_of(&f, &a);
    let lines = transcript(&d, block);
    let at = lines.iter().position(|l| l.starts_with("user: hello")).unwrap();
    assert_eq!(lines[at - 1], "note: A follow-up from jhgaylor", "{lines:#?}");

    // A follow-up queued mid-turn waits for the turn, then goes as its own
    // author's (by id, with no login); a cancel stops the turn.
    f.queue(&a, prompt(2, "acct1", "slow"));
    wait(&marks, &f, "the slow turn", || f.turns(&a).len() == 2);
    f.queue(&a, prompt(3, "acct2", "work README.md NOTES.md"));
    wait(&marks, &f, "a heartbeat while it works", || f.of(&a).iter().any(|(r, _)| r.kind == "heartbeat"));
    assert_eq!(f.turns(&a).len(), 2, "the follow-up waits for the turn");
    f.ask(&a, action(1, "acct2", "cancel"));
    wait(&marks, &f, "the follow-up's turn", || f.turns(&a).len() == 3 && f.state(&a) == "done");
    assert_eq!(f.turns(&a)[1..], [vec![prompt(2, "", "").id], vec![prompt(3, "", "").id]]);
    let lines = transcript(&d, block);
    let at = lines.iter().position(|l| l == "user: work README.md NOTES.md").unwrap();
    assert_eq!(lines[at - 1], "note: A follow-up from acct2", "{lines:#?}");
    // Its work, as the turn left it.
    let (_, turn) = f.of(&a).into_iter().rfind(|(r, _)| r.kind == "turn").unwrap();
    assert!(turn.commits.iter().any(|c| c.ends_with(" add NOTES.md")), "{turn:?}");
    assert_eq!(turn.uncommitted, Some(1), "README.md, edited, not committed");
    assert_eq!(git(&checkout, &["log", "-1", "--format=%s"]), "add NOTES.md");
    // That turn pushed its branch, and only it, with a token for that push,
    // revoked after.
    assert_eq!(turn.pushed, Some(true), "{turn:?}");
    assert_eq!(*f.pushes.lock().unwrap(), [json!({ "session": a, "branch": branch })]);
    assert_eq!(*f.revoked.lock().unwrap(), [format!("Bearer {PUSH_TOKEN}")]);
    let head = git(&checkout, &["rev-parse", "HEAD"]);
    assert_eq!(
        git(&remote, &["for-each-ref", "--format=%(refname) %(objectname)"]),
        format!("refs/heads/{branch} {head}\nrefs/heads/main {}", git(&repo, &["rev-parse", "main"]))
    );
    let slow = lines.iter().position(|l| l == "user: slow").unwrap();
    assert_eq!(lines[slow - 1], "note: A follow-up from acct1", "{lines:#?}");
    assert!(f.of(&a).iter().any(|(_, d)| d.actions == [action(1, "", "").id]), "the cancel is named once it's done");
    assert!(f.sessions.lock().unwrap()[&a].work.actions.is_empty());

    // A question: it goes out, and the answer comes back.
    f.queue(&a, prompt(4, "acct1", "ask one"));
    wait(&marks, &f, "the question", || f.state(&a) == "needs_input");
    let (_, asked_d) = f.of(&a).into_iter().rfind(|(r, _)| r.to == "needs_input").unwrap();
    assert_eq!(asked_d.ask.as_deref(), Some("question"));
    assert!(asked_d.question.as_deref().is_some_and(|q| q.contains("colour")), "{asked_d:?}");
    // As its asker's login, which the platform sends (else the account id).
    let mut answer = action(2, "acct2", "answer");
    answer.by_name = "ann".into();
    answer.request = asked_d.request.clone();
    answer.answer = Some(json!({ "question_0": "Blue" }));
    f.ask(&a, answer);
    wait(&marks, &f, "the answered turn", || f.state(&a) == "done" && f.turns(&a).len() == 4);
    let lines = transcript(&d, block);
    assert!(lines.iter().any(|l| l.starts_with("note: Answered by ann")), "{lines:#?}");
    assert!(lines.iter().any(|l| l.starts_with("agent: ") && l.contains("Blue")), "{lines:#?}");

    // A permission: nobody's asked (aru-115). The agent runs in
    // bypassPermissions, and what still asks, the block allows.
    let questions = f.of(&a).iter().filter(|(r, _)| r.to == "needs_input").count();
    f.queue(&a, prompt(5, "acct1", "run touch it"));
    wait(&marks, &f, "the allowed turn", || f.state(&a) == "done" && f.turns(&a).len() == 5);
    assert_eq!(f.of(&a).iter().filter(|(r, _)| r.to == "needs_input").count(), questions, "it asked nobody");
    let lines = transcript(&d, block);
    assert!(lines.iter().any(|l| l == "agent: Ran it."), "{lines:#?}");
    assert!(lines.iter().any(|l| l == "note: Allowed touch it (a session on its box)"), "{lines:#?}");
    let modes: String = std::fs::read_dir(&fake_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("fake-"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect();
    assert!(modes.contains(r#""mode": "bypassPermissions""#), "its session's mode: {modes}");

    // A restart between turns: what was queued while the daemon was down
    // runs once, and the agent's conversation goes on, as the same user.
    d.stop();
    f.queue(&a, prompt(6, "acct1", "remember plum"));
    d.start();
    wait(&marks, &f, "the prompt queued while it was down", || f.turns(&a).len() == 6 && f.state(&a) == "done");
    f.queue(&a, prompt(7, "acct1", "recall"));
    wait(&marks, &f, "recall", || f.turns(&a).len() == 7 && f.state(&a) == "done");
    assert!(transcript(&d, block).iter().any(|l| l == "agent: You said plum."), "{:#?}", transcript(&d, block));

    // A restart mid-turn, with a follow-up waiting: the cut turn ends, the
    // follow-up runs once, and nothing runs twice.
    f.queue(&a, prompt(8, "acct1", "slow"));
    wait(&marks, &f, "the second slow turn", || f.turns(&a).len() == 8);
    f.queue(&a, prompt(9, "acct2", "hello"));
    std::thread::sleep(Duration::from_millis(500));
    d.stop();
    d.start();
    wait(&marks, &f, "the follow-up after the restart", || f.turns(&a).len() == 9 && f.state(&a) == "done");
    let p = f.pulls();
    wait(&marks, &f, "more pulls", || f.pulls() > p + 10);
    let turns = f.turns(&a);
    assert_eq!(turns.len(), 9, "{turns:?}");
    let ids: Vec<String> = turns.into_iter().flatten().collect();
    assert_eq!(ids, (1..=9).map(|n| prompt(n, "", "").id).collect::<Vec<_>>(), "each prompt once, in order");
    let lines = transcript(&d, block);
    assert_eq!(
        (asked(&lines, "slow"), asked(&lines, "hello"), asked(&lines, "remember plum")),
        (2, 2, 1),
        "{lines:#?}"
    );
    assert!(f.refused.lock().unwrap().is_empty(), "{:?}", f.refused.lock().unwrap());

    // Three sessions on the box (a is done, b and c work): a fourth waits,
    // until one is stopped.
    let (b, c, e) = (ses('b'), ses('c'), ses('e'));
    f.start(&b, "one", prompt(10, "acct1", "slow"));
    f.start(&c, "two", prompt(11, "acct1", "slow"));
    wait(&marks, &f, "b and c working", || f.state(&b) == "working" && f.state(&c) == "working");
    f.start(&e, "three", prompt(12, "acct2", "hello"));
    let p = f.pulls();
    wait(&marks, &f, "more pulls", || f.pulls() > p + 10);
    assert_eq!(f.state(&e), "queued", "a fourth waits");
    assert!(f.of(&e).is_empty());
    let b_block = block_of(&f, &b);
    f.stop(&b);
    wait(&marks, &f, "e takes b's slot", || f.state(&e) == "done");
    assert!(homes.join("ses2").join("work").join(&e).join(".git").exists(), "in the slot b had");
    let (status, _) = d.raw("GET", &format!("/api/blocks/{b_block}"), None);
    assert_eq!(status, 404, "b's agent block closed when it was stopped");
    assert!(!homes.join("ses2").join("work").join(&b).exists(), "b's checkout went with it");

    // No turn after the one that committed pushed again (nothing new), and
    // the token is in no file on the box.
    assert_eq!(f.pushes.lock().unwrap().len(), 1);
    let mut todo = vec![state.clone(), homes.clone()];
    while let Some(dir) = todo.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() && !p.is_symlink() {
                todo.push(p);
            } else if let Ok(b) = std::fs::read(&p) {
                assert!(!b.windows(PUSH_TOKEN.len()).any(|w| w == PUSH_TOKEN.as_bytes()), "the token is in {p:?}");
            }
        }
    }
}

/// M261: the workspace's finish step, after each turn and before the push,
/// as the session's user in its checkout. It passes: one turn, pushed,
/// done. It fails, and the agent's one fix makes it pass: the fixed turn
/// is pushed. It fails twice: no third try, done, unpushed, with the
/// failure on its turn's report (the card's). And an ended session's slot
/// is left clean, its push's repo too.
#[test]
fn a_finish_step_runs_after_each_turn() {
    let Rig { _rt, f, marks, homes, sudo_log, d, state, remote, .. } = rig("finish");
    let step = |cmd: &str| Some(FinishStep { command: cmd.into(), timeout_s: 0 });
    let finish = |d: &SessionDetail| d.finish.clone().expect("the turn says what its finish step did");
    // The push tokens asked for session `id` (M259), and where its branch
    // is on GitHub.
    let pushes = |id: &str| f.pushes.lock().unwrap().iter().filter(|p| p["session"] == id).count();
    let on_github =
        |branch: &str| git(&remote, &["for-each-ref", "--format=%(objectname)", &format!("refs/heads/{branch}")]);

    // It passes: one turn, pushed, and it's done. (Each turn commits, so
    // each has something to push.)
    let p = ses('1');
    let work = "work README.md NOTES.md";
    f.start_with(&p, "pass", prompt(1, "acct1", work), step("test -d .git && echo all good"));
    wait(&marks, &f, "the passing session's turn", || f.state(&p) == "done");
    let turns = f.turn_reports(&p);
    assert_eq!(turns.len(), 1, "{turns:?}");
    let ok = FinishResult { ok: true, command: "test -d .git && echo all good".into(), output_tail: String::new() };
    assert_eq!(finish(&turns[0]), ok);
    assert_eq!(turns[0].pushed, Some(true), "{turns:?}");
    assert_eq!(pushes(&p), 1);
    let head = git(&homes.join("ses1").join("work").join(&p), &["rev-parse", "HEAD"]);
    assert_eq!(on_github(&format!("arugula/{}-pass", &p[4..12])), head);
    let said = std::fs::read_to_string(&sudo_log).unwrap();
    // Under coreutils' `timeout` where there is one (every hosted box);
    // where there's none (macOS), the daemon's own limit is the one.
    let timeout = ["/usr/bin/timeout", "/bin/timeout"].into_iter().any(|t| std::path::Path::new(t).exists());
    let how = if timeout { "timeout -k 10 900 sh -c " } else { " sh -c " };
    assert!(
        said.lines().any(|l| l.starts_with("-n -u ses1 -- env -i HOME=") && l.contains(how)),
        "it runs as the session's user, for 15 minutes at most: {said}"
    );
    // With the setup's tools and environment, as the agent has them
    // (Arugula #215).
    assert!(
        said.lines().any(|l| l.contains(how)
            && l.contains(" PATH=/opt/arugula/tools/bin:/usr/local/bin:/usr/bin:/bin ")
            && l.contains(" CARGO_TARGET_DIR=")),
        "the finish step gets the setup's tools and env: {said}"
    );

    // It fails, and the agent's fix (a turn from the finish step, with its
    // output) makes it pass: still one turn of the thread's.
    let x = ses('2');
    let cmd = "test -f FIXED || { echo FIXED is missing; exit 1; }";
    f.start_with(&x, "fix", prompt(2, "acct1", work), step(cmd));
    wait(&marks, &f, "the fixed session", || f.state(&x) == "done" && f.turn_reports(&x).len() == 2);
    let turns = f.turn_reports(&x);
    let first = finish(&turns[0]);
    assert!(
        !first.ok && first.output_tail.contains("FIXED is missing") && first.output_tail.ends_with("(exit 1)"),
        "{first:?}"
    );
    assert_eq!(finish(&turns[1]), FinishResult { ok: true, command: cmd.into(), output_tail: String::new() });
    // The push waits for the fix: the failed turn isn't pushed, the fixed
    // one is, once.
    assert_eq!((turns[0].pushed, turns[1].pushed), (Some(false), Some(true)), "{turns:?}");
    assert_eq!(pushes(&x), 1);
    let head = git(&homes.join("ses2").join("work").join(&x), &["rev-parse", "HEAD"]);
    assert_eq!(on_github(&format!("arugula/{}-fix", &x[4..12])), head);
    assert_eq!(
        f.moves(&x),
        [
            ("state", "queued", "working"),
            ("turn", "working", "working"),
            ("turn", "working", "working"),
            ("state", "working", "done")
        ]
        .map(|(k, a, b)| (k.to_owned(), a.to_owned(), b.to_owned()))
    );
    assert_eq!(f.turns(&x).len(), 1, "the fix isn't a prompt of the thread's");
    let lines = transcript(&d, block_of(&f, &x));
    let at = lines.iter().position(|l| l.starts_with("user: The finish step failed")).expect("the fix prompt");
    assert_eq!(lines[at - 1], "note: A follow-up from the finish step", "{lines:#?}");
    assert!(lines[at].contains("FIXED is missing"), "with its output: {lines:#?}");
    assert!(homes.join("ses2").join("work").join(&x).join("FIXED").exists());

    // It fails twice: done, unpushed, with no third try.
    let z = ses('3');
    f.start_with(&z, "broken", prompt(3, "acct1", work), step("echo it is broken; exit 3"));
    wait(&marks, &f, "the broken session", || f.state(&z) == "done" && f.turn_reports(&z).len() == 2);
    let n = f.pulls();
    wait(&marks, &f, "more pulls", || f.pulls() > n + 10);
    let turns = f.turn_reports(&z);
    assert_eq!(turns.len(), 2, "no third try: {turns:?}");
    for t in &turns {
        let r = finish(t);
        assert!(!r.ok && r.output_tail == "it is broken\n(exit 3)", "{r:?}");
        assert_eq!(t.pushed, Some(false), "a failed finish step isn't pushed");
    }
    assert_eq!(f.state(&z), "done");
    assert_eq!(pushes(&z), 0, "it asked for no token");
    assert_eq!(on_github(&format!("arugula/{}-broken", &z[4..12])), "", "nothing of it reached GitHub");

    // A session that ends leaves its slot clean: its checkout and its
    // user's ~/.claude go.
    let claude = homes.join("ses2").join(".claude");
    std::fs::create_dir_all(claude.join("projects")).unwrap();
    std::fs::write(claude.join("projects").join("x.jsonl"), "{}\n").unwrap();
    // And what its push left: the daemon's own repo for it.
    let pushed = state.join("work").join("sessions").join(format!("{x}.push"));
    assert!(pushed.join("repo.git").exists(), "{pushed:?}");
    f.stop(&x);
    let checkout = homes.join("ses2").join("work").join(&x);
    wait(&marks, &f, "x's slot cleaned", || !checkout.exists() && !claude.exists() && !pushed.exists());
    assert!(homes.join("ses1").join("work").join(&p).exists(), "the others' are left alone");
}

/// A block's method, as the web client calls it.
fn call(d: &arugula_testkit::Daemon, block: u64, method: &str, args: Value) -> Value {
    d.post(&format!("/api/blocks/{block}/call/{method}"), args)
}

/// The live test's fifth bug: someone opens the session's block from its
/// card and types there. The session follows the block, not only what the
/// box hands it: that turn is reported (`in_block`), and goes to the same
/// end as the box's own (the finish step, the push, the turn's report). A
/// question asked in such a turn goes out as `needs_input`, and answered
/// in the block it moves on with no action; a tool call asks nobody.
#[test]
fn a_turn_typed_in_the_block_is_the_sessions() {
    let Rig { _rt, f, marks, homes, d, remote, .. } = rig("in-block");
    let s = ses('4');
    let step = FinishStep { command: "test -d .git && echo checked".into(), timeout_s: 0 };
    f.start_with(&s, "typed", prompt(1, "acct1", "hello"), Some(step.clone()));
    wait(&marks, &f, "the session's first turn", || f.state(&s) == "done" && f.turn_reports(&s).len() == 1);
    let block = block_of(&f, &s);
    let branch = format!("arugula/{}-typed", &s[4..12]);

    // Typed in the block, as the web composer sends it.
    call(&d, block, "send", json!({ "text": "work README.md NOTES.md" }));
    wait(&marks, &f, "the typed turn's end", || f.state(&s) == "done" && f.turn_reports(&s).len() == 2);
    assert_eq!(
        f.moves(&s)[3..],
        [("state", "done", "working"), ("turn", "working", "working"), ("state", "working", "done")]
            .map(|(k, a, b)| (k.to_owned(), a.to_owned(), b.to_owned()))
    );
    let (_, start) = f.of(&s).into_iter().rfind(|(r, _)| r.to == "working" && r.from == "done").unwrap();
    assert!(start.in_block && start.prompts.is_empty(), "{start:?}");
    // What was typed goes with it, for the thread's line (Arugula #215).
    assert_eq!(start.typed.as_deref(), Some("work README.md NOTES.md"), "{start:?}");
    assert!(f.refused.lock().unwrap().is_empty(), "{:?}", f.refused.lock().unwrap());
    let turn = f.turn_reports(&s).pop().unwrap();
    assert_eq!(turn.finish, Some(FinishResult { ok: true, command: step.command.clone(), output_tail: String::new() }));
    assert!(turn.commits.iter().any(|c| c.ends_with(" add NOTES.md")), "{turn:?}");
    assert_eq!(turn.pushed, Some(true), "{turn:?}");
    let head = git(&homes.join("ses1").join("work").join(&s), &["rev-parse", "HEAD"]);
    assert_eq!(git(&remote, &["for-each-ref", "--format=%(objectname)", &format!("refs/heads/{branch}")]), head);

    // A question in a typed turn goes out; answered in the block, it goes
    // on with no action named.
    call(&d, block, "send", json!({ "text": "ask one" }));
    wait(&marks, &f, "the question", || f.state(&s) == "needs_input");
    let (_, asked) = f.of(&s).into_iter().rfind(|(r, _)| r.to == "needs_input").unwrap();
    assert_eq!(asked.ask.as_deref(), Some("question"), "{asked:?}");
    let request = asked.request.clone().expect("its request");
    call(&d, block, "answer", json!({ "id": request, "content": { "question_0": "Blue" } }));
    wait(&marks, &f, "the answered turn", || f.state(&s) == "done" && f.turn_reports(&s).len() == 3);
    let (_, back) = f.of(&s).into_iter().rfind(|(r, _)| r.from == "needs_input").unwrap();
    assert!(back.actions.is_empty(), "{back:?}");
    assert!(transcript(&d, block).iter().any(|l| l.starts_with("agent: ") && l.contains("Blue")));

    // A tool call in a typed turn asks nobody.
    call(&d, block, "send", json!({ "text": "run touch it" }));
    wait(&marks, &f, "the allowed turn", || f.state(&s) == "done" && f.turn_reports(&s).len() == 4);
    assert_eq!(f.of(&s).iter().filter(|(r, _)| r.to == "needs_input").count(), 1, "only the question");
    assert!(transcript(&d, block).iter().any(|l| l == "agent: Ran it."));

    // And the thread's prompts still go, after.
    f.queue(&s, prompt(2, "acct2", "hello"));
    wait(&marks, &f, "the thread's next prompt", || f.turns(&s).len() == 2 && f.state(&s) == "done");
    assert!(f.refused.lock().unwrap().is_empty(), "{:?}", f.refused.lock().unwrap());
}

/// M264: a Try 3's three sessions run at once, each in a slot of its own,
/// on the harness each asks for (Codex's turns asked of the gate as
/// Codex's), and a review asked meanwhile waits for a slot. When one try
/// stops (not picked), the review takes its slot and runs read-only: no
/// finish step and no push, though its agent committed, and its turn's
/// report carries the agent's reply.
#[test]
fn three_tries_run_at_once_and_a_review_waits() {
    let Rig { _rt, f, marks, homes, d: _d, .. } =
        rig_with("tries", &[("ARUGULA_HOSTED_SESSION_HARNESSES", "claude,codex")]);
    let step = || Some(FinishStep { command: "echo checked".into(), timeout_s: 0 });
    let (p, s1, s2, r) = (ses('4'), ses('5'), ses('6'), ses('7'));
    f.start_as(&p, "try", prompt(1, "acct1", "slowly"), step(), Some("claude"), false);
    f.start_as(&s1, "try", prompt(2, "acct1", "slowly"), step(), Some("claude"), false);
    f.start_as(&s2, "try", prompt(3, "acct1", "slowly"), step(), Some("codex"), false);
    wait(&marks, &f, "the three working at once", || [&p, &s1, &s2].iter().all(|s| f.state(s) == "working"));
    let taken = |id: &str| f.of(id).into_iter().find(|(r, _)| r.from == "queued").map(|(_, d)| d).unwrap();
    assert_eq!(taken(&p).harness.as_deref(), Some("claude"));
    assert_eq!(taken(&s2).harness.as_deref(), Some("codex"));
    // Each turn asks the gate as its own agent's.
    let gated = |agent: &str, id: &str| f.asked.lock().unwrap().contains(&json!({ "agent": agent, "session": id }));
    wait(&marks, &f, "the turns' gate", || gated("codex", &s2) && gated("claude", &s1) && gated("claude", &p));
    // Each in a slot of its own.
    let slot = |id: &str| {
        let held: Vec<&str> =
            ["ses1", "ses2", "ses3"].into_iter().filter(|u| homes.join(u).join("work").join(id).exists()).collect();
        assert_eq!(held.len(), 1, "{id} in {held:?}");
        held[0]
    };
    let mut slots = vec![slot(&p), slot(&s1), slot(&s2)];
    slots.sort();
    assert_eq!(slots, ["ses1", "ses2", "ses3"]);

    // A review, asked while every slot is held, waits.
    f.start_as(&r, "review", prompt(4, "acct2", "work README.md REVIEW.md"), step(), Some("codex"), true);
    let n = f.pulls();
    wait(&marks, &f, "more pulls", || f.pulls() > n + 10);
    assert_eq!(f.state(&r), "queued", "no slot for the review yet");
    assert!(f.of(&r).is_empty());
    // One try isn't picked: its slot goes to the review.
    let freed = slot(&s1);
    f.stop(&s1);
    wait(&marks, &f, "the review's turn", || f.state(&r) == "done");
    assert_eq!(slot(&r), freed, "in the slot the try had");
    let turns = f.turn_reports(&r);
    assert_eq!(turns.len(), 1, "{turns:?}");
    assert_eq!(turns[0].pushed, Some(false), "a review pushes nothing");
    assert_eq!(turns[0].finish, None, "and runs no finish step");
    assert!(turns[0].commits.iter().any(|c| c.ends_with(" add REVIEW.md")), "though its agent committed: {turns:?}");
    assert_eq!(turns[0].reply.as_deref(), Some("done"), "its reply, for the thread");
    assert!(f.pushes.lock().unwrap().iter().all(|x| x["session"] != r.as_str()), "it asked for no push token");
    assert_eq!(taken(&r).harness.as_deref(), Some("codex"));

    // The two tries left finish their turns, each with its finish step.
    wait(&marks, &f, "the tries' turns", || f.state(&p) == "done" && f.state(&s2) == "done");
    for id in [&p, &s2] {
        let t = f.turn_reports(id);
        assert_eq!(t.last().and_then(|t| t.finish.as_ref()).map(|f| f.ok), Some(true), "{t:?}");
        // Every turn carries the agent's last message, for its thread
        // (Arugula #213): a try's too.
        let reply = t.last().and_then(|t| t.reply.clone()).unwrap_or_default();
        assert!(
            !reply.is_empty() && reply.len() <= arugula_control_wire::work::REPLY_MAX,
            "a try's turn carries its answer: {t:?}"
        );
    }
}

/// A Codex session asks nobody either (aru-115): its block runs codex-acp
/// in `full-access`, Codex's counterpart of Claude Code's bypass, and what
/// still asks the block allows. Its questions are elicitations, not
/// permissions, so they still go to its card and come back answered.
#[test]
fn a_codex_session_asks_nobody_but_its_questions_reach_the_card() {
    let Rig { _rt, f, marks, homes, d, .. } =
        rig_with("codex", &[("ARUGULA_HOSTED_SESSION_HARNESSES", "claude,codex")]);
    let s = ses('8');
    f.start_as(&s, "codex", prompt(1, "acct1", "hello"), None, Some("codex"), false);
    wait(&marks, &f, "its first turn", || f.state(&s) == "done");
    let taken = f.of(&s).into_iter().find(|(r, _)| r.from == "queued").map(|(_, d)| d).unwrap();
    assert_eq!(taken.harness.as_deref(), Some("codex"));
    let block = block_of(&f, &s);

    // A tool call: nobody's asked.
    f.queue(&s, prompt(2, "acct1", "run touch it"));
    wait(&marks, &f, "the allowed turn", || f.state(&s) == "done" && f.turns(&s).len() == 2);
    assert!(f.of(&s).iter().all(|(r, _)| r.to != "needs_input"), "it asked nobody");
    let lines = transcript(&d, block);
    assert!(lines.iter().any(|l| l == "agent: Ran it."), "{lines:#?}");
    assert!(lines.iter().any(|l| l == "note: Allowed touch it (a session on its box)"), "{lines:#?}");
    let modes: String = std::fs::read_dir(homes.join("ses1/.fake-acp"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("fake-"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect();
    assert!(modes.contains(r#""mode": "full-access""#), "its session's mode: {modes}");
    assert!(!lines.iter().any(|l| l.contains("Couldn't switch")), "{lines:#?}");

    // A question goes to the card, and its answer comes back.
    f.queue(&s, prompt(3, "acct1", "ask one"));
    wait(&marks, &f, "the question", || f.state(&s) == "needs_input");
    let (_, asked_d) = f.of(&s).into_iter().rfind(|(r, _)| r.to == "needs_input").unwrap();
    assert_eq!(asked_d.ask.as_deref(), Some("question"), "{asked_d:?}");
    let mut answer = action(1, "acct1", "answer");
    answer.request = asked_d.request.clone();
    answer.answer = Some(json!({ "question_0": "Blue" }));
    f.ask(&s, answer);
    wait(&marks, &f, "the answered turn", || f.state(&s) == "done" && f.turns(&s).len() == 3);
    assert!(transcript(&d, block).iter().any(|l| l.starts_with("agent: ") && l.contains("Blue")));
}

/// Before a rebuild (Arugula #209): control asks the box `hosted.archive`
/// over its relay. Each open agent block's transcript goes, as closing it
/// would send it, a session's with what it was asked; then it answers. The
/// blocks stay open.
#[test]
fn a_box_archives_its_open_agents_before_a_rebuild() {
    let Rig { _rt, f, marks, d, .. } = rig("archive");
    let a = ses('a');
    f.start(&a, "fix-readme", prompt(1, "acct1", "hello"));
    wait(&marks, &f, "session a's first turn", || f.state(&a) == "done");
    let block = block_of(&f, &a);
    wait(&marks, &f, "the box said its ops", || {
        f.said.lock().unwrap().iter().any(|v| v["t"] == "ops" && v["ops"].to_string().contains("hosted.archive"))
    });
    assert!(f.transcripts.lock().unwrap().is_empty(), "nothing closed yet");

    let ask = json!({ "t": "op", "id": "rebuild-1", "op": { "op": "hosted.archive" } }).to_string();
    f.relay.lock().unwrap().as_ref().expect("the relay socket").send(ask).unwrap();
    let answer = || {
        let said = f.said.lock().unwrap();
        said.iter().find(|v| v["t"] == "op.done" && v["id"] == "rebuild-1").map(|v| v["answer"].clone())
    };
    wait(&marks, &f, "the archive's answer", || answer().is_some());
    assert_eq!(answer().unwrap()["answer"], "done");
    let kept = f.transcripts.lock().unwrap().clone();
    let ours = kept.iter().find(|t| t["block"] == block).unwrap_or_else(|| panic!("the session's block: {kept:#?}"));
    assert!(ours["text"].as_str().unwrap().contains("hello"), "{ours}");
    assert_eq!(ours["agent"], "claude");
    assert!(ours["closed_at"].as_u64().unwrap() > 0);
    // Still open: its transcript is there to read.
    assert!(transcript(&d, block).iter().any(|l| l.starts_with("user: hello")));
}
