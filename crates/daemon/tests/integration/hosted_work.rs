//! M212: `arugulad --hosted` takes runs from the platform and runs each at
//! most once. Against a fake control that enrolls the box under an
//! account's project and hands out the same runs on every pull, as the
//! platform does until a run starts:
//! - a run delivered again and again runs once, its two reports signed by
//!   the machine;
//! - with control taking no reports (a platform restart), the run still
//!   runs once, and after the daemon restarts too it only sends its reports
//!   again;
//! - a run whose grant was signed by someone else, or whose operation
//!   doesn't hash to its hash, fails without running.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering::SeqCst},
    },
    time::Duration,
};

use arugula_control_wire::{
    hosted::{EnrollRequest, Enrollment},
    work::{Report, Work, WorkItem},
};
use arugula_e2e::{
    Cert, DeviceKeys, Kind, Trust,
    hosted::{Delegation, HostedCert, HostedKey},
    project_grant::{GrantRevoke, ProjectGrant, Running},
    run::{Event, Op, State},
};
use arugula_testkit::arugulad;
use axum::{
    Json, Router,
    extract::{State as Axum, ws::WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::json;
use sha2::{Digest as _, Sha256};

const PROJECT: &str = "prj_1";
const WORKSPACE: &str = "hw_1";

#[derive(Clone)]
struct Fake {
    url: Arc<Mutex<String>>,
    root: Arc<DeviceKeys>,
    root_cert: Cert,
    key: Arc<HostedKey>,
    delegation: Delegation,
    /// What it answered the box's enrollment, and the box's signing key.
    enrolled: Arc<Mutex<Option<(Enrollment, String)>>>,
    items: Arc<Mutex<Vec<WorkItem>>>,
    pulls: Arc<Mutex<u32>>,
    reports: Arc<Mutex<Vec<Report>>>,
    /// The project's revokes, with every pull.
    revokes: Arc<Mutex<Vec<GrantRevoke>>>,
    /// Reports sent again after they were taken.
    again: Arc<Mutex<u32>>,
    /// Agent turns the box asked for (M185), and the ones it ended.
    turns: Arc<Mutex<Vec<String>>>,
    /// Reports aren't taken: the platform is down.
    down: Arc<AtomicBool>,
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
            items: Default::default(),
            pulls: Default::default(),
            reports: Default::default(),
            revokes: Default::default(),
            turns: Default::default(),
            again: Default::default(),
            down: Default::default(),
        }
    }

    fn device(&self) -> String {
        let e = self.enrolled.lock().unwrap();
        e.as_ref().map(|(e, _)| e.cert.device.clone()).unwrap_or_default()
    }

    /// The project's standing grant, signed by its owner's device, or by
    /// someone else's.
    fn grant(&self, by: &DeviceKeys) -> ProjectGrant {
        let mut g = ProjectGrant::new(&self.delegation, arugula_e2e::now_ms(), None);
        g.sign_with(by);
        g
    }

    /// A run of `sh -c script` on the box, under `grant`.
    fn item(&self, run: &str, script: &str, grant: ProjectGrant) -> WorkItem {
        let op = Op { argv: vec!["sh".into(), "-c".into(), script.into()], ..Op::default() };
        self.item_of(run, &op, grant)
    }

    /// A run of `op` on the box, under `grant`.
    fn item_of(&self, run: &str, op: &Op, grant: ProjectGrant) -> WorkItem {
        let op = op.encode(&self.device());
        WorkItem {
            run: run.into(),
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            op_hash: hex::encode(Sha256::digest(op.as_bytes())),
            op,
            state: "pending".into(),
            last_seq: 1,
            grant: Some(grant),
            revokes: vec![],
        }
    }

    /// The reports of `run`, each checked against the box's key.
    fn reports_of(&self, run: &str) -> Vec<Report> {
        let sign = self.enrolled.lock().unwrap().as_ref().unwrap().1.clone();
        let device = self.device();
        let out: Vec<Report> = self.reports.lock().unwrap().iter().filter(|r| r.run == run).cloned().collect();
        for r in &out {
            let ev = Event {
                run: &r.run,
                seq: r.seq,
                from: State::parse(&r.from),
                to: State::parse(&r.to).unwrap(),
                at: r.at,
                detail: &r.detail,
                daemon: &device,
            };
            assert!(ev.verify(&sign, &r.sig), "{r:?} isn't signed by the box");
        }
        out
    }

    fn pulls(&self) -> u32 {
        *self.pulls.lock().unwrap()
    }
}

async fn about(Axum(f): Axum<Fake>) -> Json<serde_json::Value> {
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

async fn work(Axum(f): Axum<Fake>) -> Json<Work> {
    *f.pulls.lock().unwrap() += 1;
    Json(Work { items: f.items.lock().unwrap().clone(), revokes: f.revokes.lock().unwrap().clone() })
}

async fn report(Axum(f): Axum<Fake>, Json(r): Json<Report>) -> Response {
    if f.down.load(SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    // As the platform does: the same event again is kept once.
    let mut kept = f.reports.lock().unwrap();
    if kept.iter().any(|k| k.run == r.run && k.seq == r.seq) {
        *f.again.lock().unwrap() += 1;
        return Json(json!({ "kind": "already" })).into_response();
    }
    kept.push(r);
    Json(json!({ "kind": "taken" })).into_response()
}

/// M185's gate: a hosted box asks before each agent turn; any may run.
async fn turn(Axum(f): Axum<Fake>, Json(b): Json<serde_json::Value>) -> Json<serde_json::Value> {
    f.turns.lock().unwrap().push(format!("start {}", b["agent"].as_str().unwrap_or_default()));
    Json(json!({ "turn": "trn_1", "key": "platform" }))
}

async fn turn_end(Axum(f): Axum<Fake>, axum::extract::Path(t): axum::extract::Path<String>) -> Json<serde_json::Value> {
    f.turns.lock().unwrap().push(format!("end {t}"));
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
            .route("/api/hosted/work/report", post(report))
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

/// Wait until `ok` holds, for 30 s; else fail with what the platform heard
/// and the daemon's log, which says why.
fn wait(marks: &Path, f: &Fake, what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !ok() {
        if std::time::Instant::now() > deadline {
            let log = std::fs::read_to_string(marks.join("arugulad.log")).unwrap_or_default();
            let tail: Vec<&str> =
                log.lines().filter(|l| l.contains("hosted") || l.contains("WARN") || l.contains("ERROR")).collect();
            panic!(
                "timed out waiting for {what}\nreports: {:#?}\nlog:\n{}",
                f.reports.lock().unwrap(),
                tail[tail.len().saturating_sub(40)..].join("\n")
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn lines(p: &Path) -> usize {
    std::fs::read_to_string(p).map_or(0, |s| s.lines().count())
}

fn run_id(n: char) -> String {
    format!("run_{}", n.to_string().repeat(32))
}

fn states(rs: &[Report]) -> Vec<(u64, String, String)> {
    rs.iter().map(|r| (r.seq, r.from.clone(), r.to.clone())).collect()
}

fn ran(seq: u64) -> Vec<(u64, String, String)> {
    vec![(seq + 1, "pending".into(), "running".into()), (seq + 2, "running".into(), "succeeded".into())]
}

#[test]
fn a_hosted_machine_runs_each_run_once() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::new();
    let url = serve(&rt, f.clone());
    let marks: PathBuf = std::env::temp_dir().join(format!("arugula-hosted-work-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&marks);
    std::fs::create_dir_all(&marks).unwrap();
    let bin = marks.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let claude = bin.join("claude");
    std::fs::write(
        &claude,
        format!("#!/bin/sh\necho \"$* key:$ANTHROPIC_API_KEY\" > {}\n", marks.join("claude").display()),
    )
    .unwrap();
    std::fs::set_permissions(&claude, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut d = arugulad!("hostedwork")
        .no_wisp()
        .no_tailscale()
        .args(["--hosted"])
        .env("ARUGULA_HOSTED_TICKET", "ticket-1")
        .env("ARUGULA_CONTROL_URL", &url)
        .env("ARUGULA_WORK_PULL_MS", "150")
        // No `runner` user here: runs run as the daemon's own (tests only).
        .env("ARUGULA_HOSTED_RUN_USER", "")
        // A Claude Code that says what it was asked, and with which login.
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
        .env("ANTHROPIC_API_KEY", "sk-test")
        .env("RUST_LOG", "info")
        .args(["--log-file", &marks.join("arugulad.log").to_string_lossy()])
        .wait_secs(30)
        .start();
    wait(&marks, &f, "the box enrolled", || !f.device().is_empty());

    // Delivered on every pull: it runs once.
    let one = marks.join("one");
    let a = run_id('a');
    f.items.lock().unwrap().push(f.item(&a, &format!("echo ran >> {}", one.display()), f.grant(&f.root)));
    wait(&marks, &f, "run a reported", || f.reports_of(&a).len() == 2);
    let p = f.pulls();
    wait(&marks, &f, "more pulls", || f.pulls() > p + 5);
    assert_eq!(lines(&one), 1);
    let reports = f.reports_of(&a);
    assert_eq!(states(&reports), ran(1));
    assert_eq!(*f.again.lock().unwrap(), 0, "a report was sent again after it was taken");
    let running: serde_json::Value = serde_json::from_str(&reports[0].detail).unwrap();
    assert!(running["pane"].is_u64(), "{running}");
    assert_eq!(running["op"], json!(f.items.lock().unwrap()[0].op_hash), "it signs what it ran");
    assert_eq!(reports[1].detail, r#"{"exit":0}"#);
    // Its pane says exactly what it ran, first.
    let pane = running["pane"].as_u64().unwrap();
    let screen = d.get(&format!("/api/panes/{pane}/capture?format=text&scope=scrollback"));
    let text = screen["text"].as_str().map_or_else(|| screen.to_string(), str::to_owned);
    assert!(text.contains(&format!("arugula run {a} · operation sha256:")), "{text}");
    assert!(text.contains("$ 'sh' '-c' 'echo ran >>"), "{text}");

    // The platform goes down: the run runs anyway, once, its reports kept.
    f.down.store(true, SeqCst);
    let two = marks.join("two");
    let b = run_id('b');
    f.items.lock().unwrap().push(f.item(&b, &format!("echo ran >> {}", two.display()), f.grant(&f.root)));
    wait(&marks, &f, "run b ran", || lines(&two) == 1);
    std::thread::sleep(Duration::from_millis(600));
    assert!(f.reports_of(&b).is_empty());

    // The daemon restarts too; the platform comes back, still sending both.
    d.stop();
    f.down.store(false, SeqCst);
    d.start();
    wait(&marks, &f, "run b reported", || f.reports_of(&b).len() == 2);
    let p = f.pulls();
    wait(&marks, &f, "more pulls", || f.pulls() > p + 5);
    assert_eq!((lines(&one), lines(&two)), (1, 1), "nothing ran again");
    assert_eq!(states(&f.reports_of(&b)), ran(1));
    assert_eq!(f.reports_of(&a).len(), 2, "run a's were taken: not sent again");

    // A grant nobody in the project signed, and an operation changed after
    // it was hashed: each fails at the start, and nothing runs.
    let three = marks.join("three");
    let c = run_id('c');
    let forged = f.grant(&DeviceKeys::generate());
    f.items.lock().unwrap().push(f.item(&c, &format!("echo ran >> {}", three.display()), forged));
    let e = run_id('e');
    let mut changed = f.item(&e, &format!("echo ran >> {}", three.display()), f.grant(&f.root));
    changed.op = changed.op.replace("argv c2g=", "argv YmFzaA==");
    f.items.lock().unwrap().push(changed);
    wait(&marks, &f, "runs c and e refused", || f.reports_of(&c).len() == 1 && f.reports_of(&e).len() == 1);
    for (run, code) in [(&c, "grant_unsigned"), (&e, "op_hash")] {
        let r = &f.reports_of(run)[0];
        assert_eq!((r.from.as_str(), r.to.as_str()), ("pending", "failed"));
        assert!(r.detail.contains(code), "{}", r.detail);
    }
    assert_eq!(lines(&three), 0);

    // An owner revokes the grant a run is going under, saying its runs
    // stop: the next pull carries the revoke, and the run is stopped.
    let g = f.grant(&f.root);
    let s = run_id('f');
    let four = marks.join("four");
    f.items.lock().unwrap().push(f.item(&s, &format!("sleep 30; echo ran >> {}", four.display()), g.clone()));
    wait(&marks, &f, "run f running", || f.reports_of(&s).len() == 1);
    // C4: the revoke comes with every pull, not only with a run to start.
    let r = GrantRevoke::new(PROJECT, &f.delegation.id(), Running::Stop, arugula_e2e::now_ms(), &f.root);
    f.items.lock().unwrap().retain(|i| i.run != s);
    f.revokes.lock().unwrap().push(r);
    wait(&marks, &f, "run f stopped", || f.reports_of(&s).len() == 2);
    let stopped = &f.reports_of(&s)[1];
    assert_eq!((stopped.from.as_str(), stopped.to.as_str()), ("running", "failed"));
    assert!(stopped.detail.contains("standing grant was revoked"), "{}", stopped.detail);
    assert_eq!(lines(&four), 0);
    f.revokes.lock().unwrap().clear();

    // C1: a run copies only a directory under the box user's home.
    let g = run_id('1');
    let etc = Op { argv: vec!["ls".into()], directory: Some("/etc".into()), ..Op::default() };
    f.items.lock().unwrap().push(f.item_of(&g, &etc, f.grant(&f.root)));
    wait(&marks, &f, "run 1 refused", || f.reports_of(&g).len() == 1);
    assert!(f.reports_of(&g)[0].detail.contains("op_directory"), "{}", f.reports_of(&g)[0].detail);

    // D: an agent run is Claude Code, headless, with the instruction, and
    // its exit is the run's.
    let h = run_id('2');
    let agent = Op { instruction: Some("fix the build".into()), ..Op::default() };
    f.items.lock().unwrap().push(f.item_of(&h, &agent, f.grant(&f.root)));
    wait(&marks, &f, "run 2 reported", || f.reports_of(&h).len() == 2);
    assert_eq!(states(&f.reports_of(&h)), ran(1));
    let said = std::fs::read_to_string(marks.join("claude")).unwrap();
    // A hosted box's agent: a turn the platform said may run, on M185's
    // placeholder key (the provider's proxy fills in the real one), never
    // the daemon's own.
    assert_eq!(said, "-p fix the build --output-format text --dangerously-skip-permissions key:arugula-brokered\n");
    wait(&marks, &f, "the turn ended", || f.turns.lock().unwrap().len() == 2);
    assert_eq!(*f.turns.lock().unwrap(), ["start claude", "end trn_1"]);
    let _ = std::fs::remove_dir_all(&marks);
}
