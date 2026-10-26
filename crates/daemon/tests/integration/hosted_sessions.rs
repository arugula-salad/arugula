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
//! - with three sessions on the box a fourth waits, until one is stopped.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_control_wire::{
    hosted::{EnrollRequest, Enrollment},
    work::{SessionAction, SessionDetail, SessionPrompt, SessionReport, SessionWork, Work},
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
        }
    }

    fn device(&self) -> String {
        self.enrolled.lock().unwrap().as_ref().map(|(e, _)| e.cert.device.clone()).unwrap_or_default()
    }

    /// A new session, queued with its first prompt.
    fn start(&self, id: &str, slug: &str, prompt: SessionPrompt) {
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

async fn dial(up: WebSocketUpgrade) -> Response {
    up.on_upgrade(|mut ws| async move { while let Some(Ok(_)) = ws.recv().await {} })
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
    s.work.n = r.n;
    s.work.state = r.to.clone();
    s.work.prompts.retain(|p| !d.prompts.contains(&p.id));
    s.work.actions.retain(|a| !d.actions.contains(&a.id));
    f.reports.lock().unwrap().push(r);
    Json(json!({ "kind": "taken" }))
}

/// M185's gate: every turn may run.
async fn turn(Json(_): Json<Value>) -> Json<Value> {
    Json(json!({ "turn": "trn_1", "pays": "project", "auth": "key" }))
}

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
            .route("/api/hosted/turns", post(turn))
            .route("/api/hosted/turns/{turn}/end", post(turn_end))
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
                "timed out waiting for {what}\nreports:\n{}\nrefused: {:?}\nlog:\n{}",
                reports.join("\n"),
                f.refused.lock().unwrap(),
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

#[test]
fn a_hosted_machine_runs_sessions() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::new();
    let url = serve(&rt, f.clone());
    let marks: PathBuf = std::env::temp_dir().join(format!("arugula-hosted-sessions-{}", std::process::id()));
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
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake_acp.py");
    let agent = format!("sh -c 'FAKE_ACP_DIR=$HOME/.fake-acp exec python3 {}'", fake.display());

    let mut d = arugulad!("hostedsessions")
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
        .env("ARUGULA_HOSTED_SESSION_AGENT", &agent)
        .env("ARUGULA_SESSION_HEARTBEAT_MS", "400")
        .env("RUST_LOG", "info")
        .args(["--log-file", &marks.join("arugulad.log").to_string_lossy()])
        .wait_secs(30)
        .start();
    wait(&marks, &f, "the box enrolled", || !f.device().is_empty());

    // A session's first prompt: a turn as ses1, in its own clone, on its
    // branch, with the repo's CLAUDE.md as text.
    let a = ses('a');
    f.start(&a, "fix-readme", prompt(1, "acct1", "hello"));
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
    assert_eq!(turn.pushed, Some(false), "M259 pushes");
    let checkout = homes.join("ses1").join("work").join(&a);
    assert_eq!(git(&checkout, &["rev-parse", "--abbrev-ref", "HEAD"]), branch);
    assert_eq!(git(&checkout, &["remote", "get-url", "origin"]), "https://github.com/acme/app.git");
    assert!(git(&checkout, &["rev-parse", "--git-dir"]) == ".git", "a clone of its own, not a worktree");
    let said = std::fs::read_to_string(&sudo_log).unwrap();
    assert!(said.contains(&format!("-n -u ses1 -- env -i HOME={} USER=ses1", homes.join("ses1").display())), "{said}");
    assert!(said.contains("ANTHROPIC_API_KEY=arugula-brokered sh -c"), "the agent, with the placeholder: {said}");
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
    assert_eq!(lines[at - 1], "note: A follow-up from acct1", "{lines:#?}");

    // A follow-up queued mid-turn waits for the turn, then goes as its own
    // author's; a cancel stops the turn.
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
    let mut answer = action(2, "acct2", "answer");
    answer.request = asked_d.request.clone();
    answer.answer = Some(json!({ "question_0": "Blue" }));
    f.ask(&a, answer);
    wait(&marks, &f, "the answered turn", || f.state(&a) == "done" && f.turns(&a).len() == 4);
    let lines = transcript(&d, block);
    assert!(lines.iter().any(|l| l.starts_with("note: Answered by acct2")), "{lines:#?}");
    assert!(lines.iter().any(|l| l.starts_with("agent: ") && l.contains("Blue")), "{lines:#?}");

    // A permission: approved, it runs.
    f.queue(&a, prompt(5, "acct1", "run touch it"));
    wait(&marks, &f, "the permission", || f.state(&a) == "needs_input");
    let (_, perm) = f.of(&a).into_iter().rfind(|(r, _)| r.to == "needs_input").unwrap();
    assert_eq!(perm.ask.as_deref(), Some("permission"));
    assert!(perm.options.iter().any(|o| o == "allow-once"), "{perm:?}");
    let mut approve = action(3, "acct2", "approve");
    approve.request = perm.request.clone();
    f.ask(&a, approve);
    wait(&marks, &f, "the approved turn", || f.state(&a) == "done" && f.turns(&a).len() == 5);
    assert!(transcript(&d, block).iter().any(|l| l == "agent: Ran it."));

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
}
