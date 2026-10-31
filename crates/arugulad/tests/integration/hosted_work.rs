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
//!   doesn't hash to its hash, fails without running;
//! - M213: a run's log goes to the bucket in chunks through the URLs
//!   control signs, all of it, and its artifact with its digest, before
//!   its end is reported; with no bucket, the run runs and its log stays
//!   in its pane;
//! - a run the box's crash interrupts fails, and never runs again (M223).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering::SeqCst},
    },
    time::Duration,
};

use arugula_control_wire::{
    hosted::{EnrollRequest, Enrollment},
    run_logs::{Upload, UploadGiven, Uploaded},
    work::{DeployWork, PasskeyAssertion, Report, Secret, SecretsAsk, SecretsGiven, Work, WorkItem},
};
use arugula_e2e::{
    Cert, DeviceKeys, Kind, Trust,
    deploy::{Approval, Decision, Operation},
    hosted::{Delegation, HostedCert, HostedKey},
    project_grant::{GrantRevoke, ProjectGrant, Running},
    run::{DeployApply, Event, Op, State},
};
use arugula_testkit::arugulad;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path as AxumPath, RawQuery, State as Axum, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde_json::json;
use sha2::{Digest as _, Sha256};

const PROJECT: &str = "prj_1";
const WORKSPACE: &str = "hw_1";
/// A deploy's secret (M219): never on the box's disk, in its pane or its
/// log.
const SECRET: &str = "s3cr3t-m219-never-on-disk";

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
    /// Turns run on a Claude subscription's token, not an API key.
    subscription: Arc<AtomicBool>,
    /// Reports aren't taken: the platform is down.
    down: Arc<AtomicBool>,
    /// The bucket's objects, by key (M213).
    objects: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    /// The uploads control recorded, in order.
    recorded: Arc<Mutex<Vec<Upload>>>,
    /// Puts the bucket refused, and why.
    refused: Arc<Mutex<Vec<String>>>,
    /// No run-log bucket.
    off: Arc<AtomicBool>,
    /// The deploy runs whose secrets the box asked for (M219), each time.
    secrets_asked: Arc<Mutex<Vec<String>>>,
    /// What control's enrollment says the box is for (M219: `deploy`).
    purpose: Arc<Mutex<Option<String>>>,
    /// Where the deploy's token revokes itself, and the bearers it was
    /// revoked with (M219).
    revoke_at: Arc<Mutex<Option<String>>>,
    revoked: Arc<Mutex<Vec<String>>>,
    /// The secrets it gives a deploy's run, when not `APP_TOKEN` (M222).
    given: Arc<Mutex<Option<Vec<Secret>>>>,
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
            subscription: Default::default(),
            again: Default::default(),
            down: Default::default(),
            objects: Default::default(),
            recorded: Default::default(),
            refused: Default::default(),
            off: Default::default(),
            secrets_asked: Default::default(),
            purpose: Default::default(),
            revoke_at: Default::default(),
            revoked: Default::default(),
            given: Default::default(),
        }
    }

    /// What control recorded of `run`'s log, in order, and its artifacts.
    fn uploads_of(&self, run: &str) -> (Vec<Upload>, Vec<Upload>) {
        let all: Vec<Upload> = self.recorded.lock().unwrap().iter().filter(|u| u.run() == run).cloned().collect();
        all.into_iter().partition(|u| matches!(u, Upload::Log { .. }))
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
            deploy: None,
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
        locked: false,
        lock: None,
        legacy_app_children: None,
        purpose: f.purpose.lock().unwrap().clone(),
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
    Json(Work { items: f.items.lock().unwrap().clone(), revokes: f.revokes.lock().unwrap().clone(), sessions: vec![] })
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

fn key(u: &Upload) -> String {
    match u {
        Upload::Log { run, n, .. } => format!("{run}/log/{n}"),
        Upload::Artifact { run, name, .. } => format!("{run}/artifacts/{name}"),
    }
}

/// A URL as the platform signs one: for one key, its size signed.
async fn upload(Axum(f): Axum<Fake>, Json(u): Json<Upload>) -> Json<UploadGiven> {
    if f.off.load(SeqCst) {
        return Json(UploadGiven::Off);
    }
    let url = format!(
        "{}/bucket/{}?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=id%2F20261022%2Fauto%2Fs3%2Faws4_request\
         &X-Amz-Date=20261022T000000Z&X-Amz-Expires=900&X-Amz-SignedHeaders=content-length%3Bhost\
         &X-Amz-Signature={}&size={}",
        f.url.lock().unwrap(),
        key(&u),
        "ab".repeat(32),
        u.size()
    );
    Json(UploadGiven::Put { url, expires_at: u64::MAX })
}

/// The bucket: a put with the URL's shape and exactly its signed size.
async fn bucket(
    Axum(f): Axum<Fake>,
    AxumPath(key): AxumPath<String>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let q = q.unwrap_or_default();
    let size = q.split('&').find_map(|kv| kv.strip_prefix("size=")).unwrap_or_default().to_owned();
    let length = headers.get("content-length").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    let signed = ["X-Amz-Algorithm=AWS4-HMAC-SHA256", "X-Amz-SignedHeaders=content-length%3Bhost", "X-Amz-Signature="]
        .iter()
        .all(|p| q.contains(p));
    if !signed || length != size || body.len().to_string() != size {
        f.refused.lock().unwrap().push(format!("{key}: {q} length {length}"));
        return StatusCode::FORBIDDEN;
    }
    f.objects.lock().unwrap().insert(key, body.to_vec());
    StatusCode::OK
}

/// Recorded once the bucket has it, with its size and digest.
async fn uploaded(Axum(f): Axum<Fake>, Json(u): Json<Upload>) -> Json<Uploaded> {
    let there = f.objects.lock().unwrap().get(&key(&u)).cloned();
    if !there.is_some_and(|b| b.len() as u64 == u.size() && hex::encode(Sha256::digest(&b)) == u.sha256()) {
        return Json(Uploaded::Refused { why: "not in the bucket".into() });
    }
    let mut r = f.recorded.lock().unwrap();
    if !r.contains(&u) {
        r.push(u);
    }
    Json(Uploaded::Kept)
}

/// M185's gate: a hosted box asks before each agent turn; any may run.
async fn turn(Axum(f): Axum<Fake>, Json(b): Json<serde_json::Value>) -> Json<serde_json::Value> {
    f.turns.lock().unwrap().push(format!("start {}", b["agent"].as_str().unwrap_or_default()));
    let auth = if f.subscription.load(SeqCst) { "subscription" } else { "key" };
    Json(json!({ "turn": "trn_1", "pays": "platform", "auth": auth }))
}

async fn turn_end(Axum(f): Axum<Fake>, axum::extract::Path(t): axum::extract::Path<String>) -> Json<serde_json::Value> {
    f.turns.lock().unwrap().push(format!("end {t}"));
    Json(json!({ "ended": true }))
}

/// M219: a deploy run's secrets, given once per run, as the platform does.
async fn secrets(Axum(f): Axum<Fake>, Json(a): Json<SecretsAsk>) -> Json<SecretsGiven> {
    let mut asked = f.secrets_asked.lock().unwrap();
    let again = asked.contains(&a.run);
    asked.push(a.run);
    Json(if again {
        SecretsGiven::Refused { why: "once per run".into() }
    } else {
        let given = f.given.lock().unwrap().clone();
        SecretsGiven::Given {
            secrets: given.unwrap_or_else(|| {
                vec![Secret {
                    name: "APP_TOKEN".into(),
                    value: SECRET.into(),
                    revoke: f.revoke_at.lock().unwrap().clone(),
                }]
            }),
        }
    })
}

/// M219: a short-lived token revokes itself, its bearer itself.
async fn revoke(Axum(f): Axum<Fake>, headers: HeaderMap) -> StatusCode {
    let bearer = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    f.revoked.lock().unwrap().push(bearer);
    StatusCode::NO_CONTENT
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
            .route("/api/hosted/work/secrets", post(secrets))
            .route("/revoke", axum::routing::delete(revoke))
            .route("/api/hosted/turns", post(turn))
            .route("/api/hosted/turns/{turn}/end", post(turn_end))
            .route("/api/hosted/work/upload", post(upload))
            .route("/api/hosted/work/uploaded", post(uploaded))
            // A chunk is 4 MiB: past axum's 2 MB default.
            .route("/bucket/{*key}", put(bucket).layer(axum::extract::DefaultBodyLimit::max(16 << 20)))
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

/// Run `run`'s state in its ledger, `<state>/work/ID.json`.
fn ledger_state(state: &Path, run: &str) -> String {
    let file = std::fs::read_to_string(state.join("work").join(format!("{run}.json"))).unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(&file)
        .ok()
        .and_then(|l| l["state"].as_str().map(str::to_owned))
        .unwrap_or_default()
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
        format!(
            "#!/bin/sh\necho \"$* key:${{ANTHROPIC_API_KEY:-}} oauth:${{CLAUDE_CODE_OAUTH_TOKEN:-}}\" > {}\n",
            marks.join("claude").display()
        ),
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
        // Nor session users, whatever this machine has (an Arugula box has
        // `ses1`…): no firewall of theirs, no `kill -1` as one.
        .env("ARUGULA_HOSTED_SESSION_USERS", "")
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
    // Its end is kept before the daemon goes: a daemon gone before that
    // (a crash, M223) fails it instead.
    wait(&marks, &f, "run b's end kept", || !matches!(ledger_state(&d.state, &b).as_str(), "" | "running"));
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
    let reports = f.reports_of(&b);
    assert_eq!(states(&reports), ran(1), "{:?}", reports.iter().map(|r| &r.detail).collect::<Vec<_>>());
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
    assert_eq!(
        said,
        "-p fix the build --output-format text --dangerously-skip-permissions key:arugula-brokered oauth:\n"
    );
    wait(&marks, &f, "the turn ended", || f.turns.lock().unwrap().len() == 2);
    assert_eq!(*f.turns.lock().unwrap(), ["start claude", "end trn_1"]);

    // On a Claude subscription's token the placeholder is Claude Code's
    // OAuth token, and no API key goes first.
    f.subscription.store(true, SeqCst);
    let o = run_id('4');
    f.items.lock().unwrap().push(f.item_of(&o, &agent, f.grant(&f.root)));
    wait(&marks, &f, "run 4 reported", || f.reports_of(&o).len() == 2);
    let said = std::fs::read_to_string(marks.join("claude")).unwrap();
    assert_eq!(
        said,
        "-p fix the build --output-format text --dangerously-skip-permissions key: oauth:arugula-brokered\n"
    );
    wait(&marks, &f, "the turn ended", || f.turns.lock().unwrap().len() == 4);

    // M219: a people's workspace never applies a deploy, even one approved
    // by the account's own browser: only a deploy's own box does.
    let dep = run_id('3');
    f.items.lock().unwrap().push(deploy_item(&f, &dep, &format!("dep_{}", "3".repeat(32)), "good-3", &f.root));
    wait(&marks, &f, "the deploy refused", || f.reports_of(&dep).len() == 1);
    assert!(f.reports_of(&dep)[0].detail.contains("not_deploy_box"), "{}", f.reports_of(&dep)[0].detail);
    assert!(f.secrets_asked.lock().unwrap().is_empty(), "no secrets");
    let _ = std::fs::remove_dir_all(&marks);
}

/// A hosted box's Claude Code holds its placeholder as the kind of
/// credential each turn says: an API key, or a Claude subscription's
/// token, with no API key of the daemon's going first. One started
/// holding the other starts again on its session before the prompt goes,
/// and the conversation carries on.
#[test]
fn a_hosted_agent_holds_the_credential_its_turn_says() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::new();
    let url = serve(&rt, f.clone());
    let marks: PathBuf = std::env::temp_dir().join(format!("arugula-hosted-agent-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&marks);
    let sessions = marks.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let d = arugulad!("hostedagent")
        .no_wisp()
        .no_tailscale()
        .args(["--hosted"])
        .env("ARUGULA_HOSTED_TICKET", "ticket-1")
        .env("ARUGULA_CONTROL_URL", &url)
        .env("FAKE_ACP_DIR", &sessions)
        // No `runner` user here: never `kill -1` as one (on CI, that's the
        // Actions runner's own user).
        .env("ARUGULA_HOSTED_RUN_USER", "")
        .env("ARUGULA_HOSTED_SESSION_USERS", "")
        .env("ANTHROPIC_API_KEY", "sk-test")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env("RUST_LOG", "info")
        .args(["--log-file", &marks.join("arugulad.log").to_string_lossy()])
        .wait_secs(30)
        .start();
    wait(&marks, &f, "the box enrolled", || !f.device().is_empty());
    // Control answered; the daemon knows it's in once it reads what it
    // saved, and only then asks for work (or for a turn).
    wait(&marks, &f, "the daemon in control", || f.pulls() > 0);

    let fake = format!("{}/tests/fake_acp.py", concat!(env!("CARGO_MANIFEST_DIR"), "/../daemon"));
    let config = json!({ "agent": "claude", "command": ["python3", fake], "cwd": sessions, "prompt": "remember plum" });
    let id = d.post("/api/blocks", json!({ "type": "agent", "config": config }))["block"].as_u64().unwrap();
    let said = |prompt: &str| {
        d.post(&format!("/api/blocks/{id}/call/send"), json!({ "text": prompt }));
        let v = d.get(&format!("/api/panes/{id}/wait?until=idle&timeout=20"));
        assert_ne!(v["result"], "timeout", "{}", d.get(&format!("/api/blocks/{id}")));
        let state = d.get(&format!("/api/blocks/{id}"))["state"].clone();
        let last = state["entries"].as_array().into_iter().flatten().rev().find(|e| e["type"] != "user").cloned();
        last.and_then(|e| e["text"].as_str().map(str::to_owned)).unwrap_or_default()
    };
    let v = d.get(&format!("/api/panes/{id}/wait?until=idle&timeout=20"));
    assert_ne!(v["result"], "timeout", "{}", d.get(&format!("/api/blocks/{id}")));
    assert_eq!(said("env ANTHROPIC_API_KEY"), "ENV ANTHROPIC_API_KEY=arugula-brokered", "an API key's placeholder");

    f.subscription.store(true, SeqCst);
    assert_eq!(said("env CLAUDE_CODE_OAUTH_TOKEN"), "ENV CLAUDE_CODE_OAUTH_TOKEN=arugula-brokered", "an OAuth token's");
    assert_eq!(said("env ANTHROPIC_API_KEY"), "ENV ANTHROPIC_API_KEY unset", "no API key goes first");
    assert!(said("recall").contains("plum"), "the same session, carried on");

    f.subscription.store(false, SeqCst);
    assert_eq!(said("env ANTHROPIC_API_KEY"), "ENV ANTHROPIC_API_KEY=arugula-brokered", "a key again");
    drop(d);
    let _ = std::fs::remove_dir_all(&marks);
}

/// The box goes down mid-run (M223): the daemon is killed, and so is
/// everything it ran (a box's crash or reboot). After the restart its
/// ledger fails the run, "interrupted by a restart; not run again", and
/// never starts it again, though control still sends it as pending (it
/// never took the start, being down). A daemon that dies alone, its run's
/// pane surviving, follows the run to its end after the restart and
/// reports it, without starting it again.
#[test]
fn a_run_a_crash_interrupts_fails_and_never_runs_again() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::new();
    let url = serve(&rt, f.clone());
    let marks: PathBuf = std::env::temp_dir().join(format!("arugula-hosted-crash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&marks);
    std::fs::create_dir_all(&marks).unwrap();
    let mut d = arugulad!("hostedcrash")
        .no_wisp()
        .no_tailscale()
        .args(["--hosted"])
        .env("ARUGULA_HOSTED_TICKET", "ticket-1")
        .env("ARUGULA_CONTROL_URL", &url)
        .env("ARUGULA_WORK_PULL_MS", "150")
        .env("ARUGULA_HOSTED_RUN_USER", "")
        .env("ARUGULA_HOSTED_SESSION_USERS", "")
        .env("RUST_LOG", "info")
        .args(["--log-file", &marks.join("arugulad.log").to_string_lossy()])
        .wait_secs(30)
        .start();
    wait(&marks, &f, "the box enrolled", || !f.device().is_empty());
    // Control takes no reports: it sends the runs as pending on every pull.
    f.down.store(true, SeqCst);

    // The box goes down while the run runs.
    let one = marks.join("one");
    let a = run_id('a');
    let script = format!("echo started >> {0}; sleep 30; echo finished >> {0}", one.display());
    f.items.lock().unwrap().push(f.item(&a, &script, f.grant(&f.root)));
    wait(&marks, &f, "run a started", || lines(&one) == 1);
    // Its pane is in the saved layout (a quarter second after a change).
    std::thread::sleep(Duration::from_secs(1));
    d.kill();
    for (_, shim) in arugula_testkit::strays::kill_programs(&d.state) {
        let _ = std::process::Command::new("kill").args(["-KILL", &shim.to_string()]).status();
    }
    f.down.store(false, SeqCst);
    d.start();
    wait(&marks, &f, "run a reported", || f.reports_of(&a).len() == 2);
    let p = f.pulls();
    wait(&marks, &f, "more pulls", || f.pulls() > p + 5);
    let reports = f.reports_of(&a);
    assert_eq!(states(&reports), [(2, "pending".into(), "running".into()), (3, "running".into(), "failed".into())]);
    let why: serde_json::Value = serde_json::from_str(&reports[1].detail).unwrap();
    assert_eq!(why["reason"], json!("interrupted by a restart; not run again"));
    assert_eq!(std::fs::read_to_string(&one).unwrap(), "started\n", "started once, never again");

    // The daemon alone dies, the run's program going on in its pane: after
    // the restart the daemon finds the pane (tagged with the run) and
    // follows the run to its end, which it reports. Not started again.
    f.down.store(true, SeqCst);
    let two = marks.join("two");
    let b = run_id('b');
    let script = format!("echo started >> {0}; sleep 4; echo finished >> {0}", two.display());
    f.items.lock().unwrap().push(f.item(&b, &script, f.grant(&f.root)));
    wait(&marks, &f, "run b started", || lines(&two) == 1);
    // Its pane is in the saved layout (a quarter second after a change).
    std::thread::sleep(Duration::from_secs(1));
    d.kill();
    f.down.store(false, SeqCst);
    d.start();
    wait(&marks, &f, "run b reported", || f.reports_of(&b).len() == 2);
    let p = f.pulls();
    wait(&marks, &f, "more pulls", || f.pulls() > p + 5);
    let reports = f.reports_of(&b);
    assert_eq!(states(&reports), ran(1), "followed to its end: {reports:?}");
    assert_eq!(reports[1].detail, r#"{"exit":0}"#);
    assert_eq!(std::fs::read_to_string(&two).unwrap(), "started\nfinished\n", "it ran once, to its end");
    assert_eq!(std::fs::read_to_string(&one).unwrap(), "started\n");
    let _ = std::fs::remove_dir_all(&marks);
}

/// A deploy of `source` to production, approved by `by` (the account's
/// own browser, or someone else's key), with a passkey over the approval.
fn deploy_item(f: &Fake, run: &str, dep: &str, source: &str, by: &DeviceKeys) -> WorkItem {
    deploy_item_by(f, run, dep, source, by, ("fake", "acme/app", &["APP_TOKEN"]))
}

/// As [`deploy_item`], by `driver`, of a commit of `place`, with `secrets`.
fn deploy_item_by(
    f: &Fake,
    run: &str,
    dep: &str,
    source: &str,
    by: &DeviceKeys,
    driver: (&str, &str, &[&str]),
) -> WorkItem {
    deploy_item_with(f, run, dep, source, by, driver, Default::default())
}

/// [`deploy_item_by`] with its environment's driver settings (M221: the
/// release's bump and lane), which its plan names.
fn deploy_item_with(
    f: &Fake,
    run: &str,
    dep: &str,
    source: &str,
    by: &DeviceKeys,
    (driver, place, secrets): (&str, &str, &[&str]),
    settings: arugula_e2e::drivers::Settings,
) -> WorkItem {
    use base64::Engine as _;
    let mut op = Operation {
        project: PROJECT.into(),
        environment: "env_1".into(),
        environment_name: "production".into(),
        plan: String::new(),
        source_kind: "commit".into(),
        source_ref: source.into(),
        source_where: place.into(),
        planned: "cd".repeat(32),
    };
    op.plan = arugula_e2e::drivers::plan_digest(driver, &op, &settings, None);
    let approval = Approval::new(by, dep, &op, "acct1", Decision::Once, arugula_e2e::now_ms() + 3_600_000);
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let cd = json!({ "type": "webauthn.get", "challenge": approval.passkey_challenge(), "origin": "https://c.test" });
    let mut data = Sha256::digest(b"c.test").to_vec();
    data.extend_from_slice(&[0x05, 0, 0, 0, 0]);
    let run_op =
        Op { deploy: Some(DeployApply { deploy: dep.into(), driver: driver.into(), op: op.hash() }), ..Op::default() }
            .encode(&f.device());
    WorkItem {
        run: run.into(),
        project: PROJECT.into(),
        workspace: WORKSPACE.into(),
        op_hash: hex::encode(Sha256::digest(run_op.as_bytes())),
        op: run_op,
        state: "approved".into(),
        last_seq: 2,
        grant: None,
        revokes: vec![],
        deploy: Some(DeployWork {
            deploy: dep.into(),
            operation: op,
            approval,
            passkey: Some(PasskeyAssertion {
                id: "pk".into(),
                client_data_json: b64.encode(cd.to_string()),
                authenticator_data: b64.encode(&data),
                signature: "c2ln".into(),
            }),
            driver: driver.into(),
            secrets: secrets.iter().map(|s| s.to_string()).collect(),
            settings,
            roll_back: None,
            approval_id: None,
        }),
    }
}

/// Every file under `dir` whose bytes hold `needle`.
fn holding(dir: &Path, needle: &[u8]) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            out.extend(holding(&p, needle));
        } else if t.is_file()
            && let Ok(b) = std::fs::read(&p)
            && b.windows(needle.len()).any(|w| w == needle)
        {
            out.push(p);
        }
    }
    out
}

/// M219: a deploy's run, against the fake control. Its approval is checked
/// on the box against the account it pinned: one signed by someone else's
/// key fails there, without asking for secrets. An approved one asks for
/// its secrets once, after its start was taken; its driver gets them on
/// its standard input, prints them, and fails part way; its pane shows
/// `[redacted APP_TOKEN]` and never the secret; and afterwards no file
/// under the daemon's state directory (its home, its ledger, the panes'
/// logs on disk, the driver's own directory) or the test's holds it.
#[test]
fn a_hosted_machine_applies_a_deploy_and_its_secret_never_reaches_disk_or_a_pane() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::new();
    *f.purpose.lock().unwrap() = Some("deploy".into());
    let url = serve(&rt, f.clone());
    *f.revoke_at.lock().unwrap() = Some(format!("{url}/revoke"));
    let marks: PathBuf = std::env::temp_dir().join(format!("arugula-hosted-deploy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&marks);
    std::fs::create_dir_all(&marks).unwrap();
    let d = arugulad!("hosteddeploy")
        .no_wisp()
        .no_tailscale()
        .home_in_state()
        .args(["--hosted"])
        .env("ARUGULA_HOSTED_TICKET", "ticket-1")
        .env("ARUGULA_CONTROL_URL", &url)
        .env("ARUGULA_WORK_PULL_MS", "150")
        // No `runner` user here: a deploy's box refuses that, but in tests.
        .env("ARUGULA_HOSTED_RUN_USER", "")
        .env("ARUGULA_HOSTED_SESSION_USERS", "")
        .env("ARUGULA_HOSTED_TESTS_UNSAFE", "1")
        .env("ARUGULA_HOSTED_FAKE_DRIVER", "1")
        .env("RUST_LOG", "info")
        .args(["--log-file", &marks.join("arugulad.log").to_string_lossy()])
        .wait_secs(30)
        .start();
    wait(&marks, &f, "the box enrolled", || !f.device().is_empty());

    // Someone else's key: refused on the box, before any secret.
    let forged = run_id('7');
    f.items.lock().unwrap().push(deploy_item(
        &f,
        &forged,
        &format!("dep_{}", "f".repeat(32)),
        "good-0",
        &DeviceKeys::generate(),
    ));
    wait(&marks, &f, "the forged deploy refused", || f.reports_of(&forged).len() == 1);
    let r = &f.reports_of(&forged)[0];
    assert_eq!((r.from.as_str(), r.to.as_str()), ("approved", "failed"));
    assert!(r.detail.contains("approval_device"), "{}", r.detail);

    // Approved by the account's own browser: it runs, and fails part way.
    let dep = format!("dep_{}", "1".repeat(32));
    let run = run_id('8');
    f.items.lock().unwrap().push(deploy_item(&f, &run, &dep, "fail-1", &f.root));
    wait(&marks, &f, "the deploy's run ended", || f.reports_of(&run).len() == 2);
    let reports = f.reports_of(&run);
    assert_eq!(states(&reports), [(3, "approved".into(), "running".into()), (4, "running".into(), "failed".into())]);
    assert_eq!(reports[1].detail, r#"{"exit":3}"#);
    assert_eq!(*f.secrets_asked.lock().unwrap(), std::slice::from_ref(&run), "asked once, and not for the forged one");
    let running: serde_json::Value = serde_json::from_str(&reports[0].detail).unwrap();
    assert_eq!(running["deploy"], json!(dep));

    // Its log went to the bucket before its end was reported (the box is
    // deleted after): the pane's output, redacted.
    let (logs, _) = f.uploads_of(&run);
    assert!(!logs.is_empty(), "its log was sent");
    let mut all = Vec::new();
    for u in &logs {
        let Upload::Log { n, .. } = u else { unreachable!() };
        all.extend_from_slice(&f.objects.lock().unwrap()[&format!("{run}/log/{n}")]);
    }
    let text = String::from_utf8_lossy(&all);
    assert!(text.contains("fake driver: APP_TOKEN is [redacted APP_TOKEN] (25 bytes)"), "{text}");
    assert!(text.contains("fake driver: failing part way") && text.contains("the driver exited 3"), "{text}");
    assert!(text.contains(&format!("arugula run {run} · operation sha256:")), "{text}");
    assert!(text.contains("fake driver: wrote "), "its driver ran: {text}");
    assert!(!text.contains(SECRET), "the secret is in the uploaded log: {text}");
    // Its token was revoked by the box itself, as its own bearer.
    assert_eq!(*f.revoked.lock().unwrap(), [format!("Bearer {SECRET}")], "revoked once, by the box");
    let home = d.state.join("work").join(format!("{run}.home"));
    assert!(!home.exists(), "its directory (its HOME) is gone after the run");
    std::thread::sleep(Duration::from_millis(500));
    let found: Vec<PathBuf> =
        [d.state.as_path(), marks.as_path()].iter().flat_map(|dir| holding(dir, SECRET.as_bytes())).collect();
    assert!(found.is_empty(), "the secret is on disk: {found:?}");
    assert!(!holding(&d.state, b"[redacted APP_TOKEN]").is_empty(), "the redacted output is (the pane's log)");
    let _ = std::fs::remove_dir_all(&marks);
}

/// M219: a deploy's box (control's enrollment says `purpose: deploy`) takes
/// its one deploy's run and nothing else (a plain run, even under the
/// project's grant, fails there), applies it, and stops once control has
/// its end.
#[test]
fn a_deploy_box_takes_its_one_deploy_and_stops() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::new();
    *f.purpose.lock().unwrap() = Some("deploy".into());
    let url = serve(&rt, f.clone());
    let marks: PathBuf = std::env::temp_dir().join(format!("arugula-deploy-box-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&marks);
    std::fs::create_dir_all(&marks).unwrap();
    let d = arugulad!("deploybox")
        .no_wisp()
        .no_tailscale()
        .home_in_state()
        .args(["--hosted"])
        .env("ARUGULA_HOSTED_TICKET", "ticket-1")
        .env("ARUGULA_CONTROL_URL", &url)
        .env("ARUGULA_WORK_PULL_MS", "150")
        .env("ARUGULA_HOSTED_RUN_USER", "")
        .env("ARUGULA_HOSTED_SESSION_USERS", "")
        .env("ARUGULA_HOSTED_TESTS_UNSAFE", "1")
        .env("ARUGULA_HOSTED_FAKE_DRIVER", "1")
        .env("RUST_LOG", "info")
        .args(["--log-file", &marks.join("arugulad.log").to_string_lossy()])
        .wait_secs(30)
        .start();
    wait(&marks, &f, "the box enrolled", || !f.device().is_empty());
    let plain = run_id('a');
    f.items.lock().unwrap().push(f.item(&plain, "echo ran", f.grant(&f.root)));
    wait(&marks, &f, "the plain run refused", || f.reports_of(&plain).len() == 1);
    assert!(f.reports_of(&plain)[0].detail.contains("deploy_box"), "{}", f.reports_of(&plain)[0].detail);
    let run = run_id('b');
    let dep = format!("dep_{}", "2".repeat(32));
    f.items.lock().unwrap().push(deploy_item(&f, &run, &dep, "good-1", &f.root));
    wait(&marks, &f, "the deploy ran", || f.reports_of(&run).len() == 2);
    assert_eq!(
        states(&f.reports_of(&run)),
        [(3, "approved".into(), "running".into()), (4, "running".into(), "succeeded".into())]
    );
    // It stops once control has its end.
    let pid = d.pid().unwrap();
    let gone =
        || std::fs::read_to_string(format!("/proc/{pid}/stat")).map_or(true, |s| s.split(' ').nth(2) == Some("Z"));
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !gone() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(gone(), "the deploy's box is still up after its run");
    let _ = std::fs::remove_dir_all(&marks);
}

/// M213: a run's log goes to the bucket in chunks, every byte of its
/// pane's output in order, and its artifact with its digest, all before
/// its end is reported. With no bucket, a run runs, and nothing is sent.
#[test]
fn a_hosted_runs_log_and_artifact_go_to_the_bucket() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::new();
    let url = serve(&rt, f.clone());
    let d = arugulad!("hostedlogs")
        .no_wisp()
        .no_tailscale()
        .args(["--hosted"])
        .env("ARUGULA_HOSTED_TICKET", "ticket-1")
        .env("ARUGULA_CONTROL_URL", &url)
        .env("ARUGULA_WORK_PULL_MS", "150")
        .env("ARUGULA_HOSTED_RUN_USER", "")
        .env("ARUGULA_HOSTED_SESSION_USERS", "")
        .wait_secs(30)
        .start();
    d.wait_for("the box enrolled", || !f.device().is_empty());

    // More than a chunk (4 MiB) of output, then an artifact.
    let a = run_id('a');
    let script = "seq 1 900000; echo the-end; printf 'hi\\n' > \"$ARUGULA_ARTIFACTS/out.txt\"; \
                  ln -s /etc/passwd \"$ARUGULA_ARTIFACTS/passwd\"";
    f.items.lock().unwrap().push(f.item(&a, script, f.grant(&f.root)));
    d.wait_for("run a reported", || f.reports_of(&a).len() == 2);
    assert_eq!(states(&f.reports_of(&a)), ran(1));
    // Before its end was reported, everything went.
    let (logs, artifacts) = f.uploads_of(&a);
    assert!(logs.len() >= 2, "{logs:?}");
    let mut all = Vec::new();
    for (i, u) in logs.iter().enumerate() {
        let Upload::Log { n, offset, size, .. } = u else { unreachable!() };
        assert_eq!((*n as usize, *offset), (i, all.len() as u64), "chunks follow each other");
        assert!(*size <= 4 * 1024 * 1024);
        all.extend_from_slice(&f.objects.lock().unwrap()[&format!("{a}/log/{n}")]);
    }
    let text = String::from_utf8_lossy(&all);
    assert!(text.starts_with(&format!("arugula run {a} · operation sha256:")), "{}", &text[..200]);
    assert!(text.contains("\r\n899999\r\n900000\r\nthe-end"), "the whole log, in order");
    assert_eq!(text.matches("\r\n450000\r\n").count(), 1);
    let want = Upload::Artifact {
        run: a.clone(),
        name: "out.txt".into(),
        size: 3,
        sha256: hex::encode(Sha256::digest(b"hi\n")),
    };
    assert_eq!(artifacts, vec![want], "the file, not the link");
    assert!(f.refused.lock().unwrap().is_empty(), "{:?}", f.refused.lock().unwrap());

    // A failing run (#161): the pane, the run and the kept log agree on
    // its exit, and the kept log is the command's output alone, without
    // the terminal's own note after it.
    let c = run_id('c');
    f.items.lock().unwrap().push(f.item(&c, "echo boom; exit 1", f.grant(&f.root)));
    d.wait_for("run c reported", || f.reports_of(&c).len() == 2);
    assert_eq!(f.reports_of(&c)[1].detail, r#"{"exit":1}"#);
    let pane = (1..=8)
        .find(|n| {
            let (status, text) = d.raw("GET", &format!("/api/panes/{n}/capture?scope=scrollback"), None);
            status == 200 && text.contains(&format!("arugula run {c}"))
        })
        .expect("the run's pane");
    let shown = d.get(&format!("/api/panes/{pane}/capture?scope=scrollback"));
    assert!(shown.as_str().unwrap().contains("[exited with code 1 · press Enter for a shell]"), "{shown}");
    let (logs, _) = f.uploads_of(&c);
    let mut all = Vec::new();
    for u in &logs {
        let Upload::Log { n, .. } = u else { unreachable!() };
        all.extend_from_slice(&f.objects.lock().unwrap()[&format!("{c}/log/{n}")]);
    }
    let kept = String::from_utf8_lossy(&all).into_owned();
    assert!(kept.contains("boom\r\n"), "{kept:?}");
    assert!(!kept.contains("press Enter for a shell") && !kept.contains("exited with code"), "{kept:?}");

    // No bucket: the run runs, and nothing's sent.
    f.off.store(true, SeqCst);
    let b = run_id('b');
    f.items.lock().unwrap().push(f.item(&b, "echo quiet", f.grant(&f.root)));
    d.wait_for("run b reported", || f.reports_of(&b).len() == 2);
    assert_eq!(states(&f.reports_of(&b)), ran(1));
    assert_eq!(f.uploads_of(&b), (vec![], vec![]));
    assert!(!f.objects.lock().unwrap().keys().any(|k| k.starts_with(&b)));
}

/// git in `dir`, as a test's own user.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A bare repository at `<remotes>/<repo>.git` holding `files`, one
/// commit, fetchable by sha as GitHub's are: that commit.
fn bare_with(root: &Path, repo: &str, files: &[(&str, &str)]) -> String {
    let bare = root.join("remotes").join(format!("{repo}.git"));
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--quiet", "--bare", "--initial-branch=main"]);
    git(&bare, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
    let w = root.join("seed").join(repo);
    std::fs::create_dir_all(&w).unwrap();
    git(&w, &["init", "--quiet", "--initial-branch=main"]);
    for (f, body) in files {
        let p = w.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    git(&w, &["add", "-A"]);
    git(&w, &["commit", "--quiet", "-m", "seed"]);
    git(&w, &["push", "--quiet", &bare.to_string_lossy(), "main"]);
    git(&w, &["rev-parse", "HEAD"])
}

/// M222: the apps-platform driver on a deploy's box, through the daemon:
/// its tokens come on the driver's standard input, `arugulad
/// _apps-platform` pins the app's pointer in the list (a bare repository
/// standing in for GitHub), waits for the app (a fake answering its health
/// path), and the run succeeds. The commit changes the pointer only, and
/// neither token is on disk or in the pane.
#[test]
fn a_deploy_box_applies_an_apps_platform_deploy() {
    const WRITE: &str = "ghs_m222writeTOKENneverOnDisk000000000"; // gitleaks:allow
    const READ: &str = "ghs_m222readTOKENneverOnDisk0000000000"; // gitleaks:allow
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    // A deploy's box (M219): only one applies a deploy. It stops after,
    // so what its pane said is read from the bucket (M213).
    let f = Fake::new();
    *f.purpose.lock().unwrap() = Some("deploy".into());
    *f.given.lock().unwrap() = Some(vec![
        Secret { name: "APPS_PLATFORM_TOKEN".into(), value: WRITE.into(), revoke: None },
        Secret { name: "SOURCE_TOKEN".into(), value: READ.into(), revoke: None },
    ]);
    let url = serve(&rt, f.clone());
    let marks: PathBuf = std::env::temp_dir().join(format!("arugula-apps-platform-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&marks);
    std::fs::create_dir_all(&marks).unwrap();
    let list_head = bare_with(&marks, "arugula-salad/planter-apps", &[("apps/.gitkeep", "")]);
    let sha = bare_with(&marks, "arugula-salad/beds-smoke", &[("app.yaml", "name: beds-smoke\nhealth: /healthz\n")]);
    // The app: 200 at its health path, counted; and GitHub's API, taking
    // the tokens' revokes.
    let asked = Arc::new(Mutex::new(0u32));
    let revoked = Arc::new(Mutex::new(Vec::<String>::new()));
    let health = rt.block_on(async {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        let (asked, revoked) = (asked.clone(), revoked.clone());
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
            while let Ok((mut s, _)) = l.accept().await {
                let mut buf = vec![0u8; 2048];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                let ok = req.starts_with("GET /healthz ");
                *asked.lock().unwrap() += u32::from(ok);
                let revoke = req.starts_with("DELETE /installation/token ");
                if revoke {
                    let auth = req
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("authorization: bearer ").map(str::to_owned));
                    revoked.lock().unwrap().push(auth.unwrap_or_default());
                }
                let status = if ok {
                    "200 OK"
                } else if revoke {
                    "204 No Content"
                } else {
                    "404 Not Found"
                };
                let _ = s.write_all(format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\n\r\n").as_bytes()).await;
            }
        });
        at
    });
    let d = arugulad!("appsplatform")
        .no_wisp()
        .no_tailscale()
        .home_in_state()
        .args(["--hosted"])
        .env("ARUGULA_HOSTED_TICKET", "ticket-1")
        .env("ARUGULA_CONTROL_URL", &url)
        .env("ARUGULA_WORK_PULL_MS", "150")
        .env("ARUGULA_HOSTED_RUN_USER", "")
        .env("ARUGULA_HOSTED_SESSION_USERS", "")
        // No `runner` user here: a deploy's box refuses that, but in tests.
        .env("ARUGULA_HOSTED_TESTS_UNSAFE", "1")
        .env("ARUGULA_HOSTED_FAKE_DRIVER", "1")
        .env("ARUGULA_HOSTED_APPS_PLATFORM_GIT", format!("{}/", marks.join("remotes").display()))
        .env("ARUGULA_HOSTED_APPS_PLATFORM_HEALTH", &health)
        .env("ARUGULA_HOSTED_APPS_PLATFORM_API", &health)
        .env("RUST_LOG", "info")
        .args(["--log-file", &marks.join("arugulad.log").to_string_lossy()])
        .wait_secs(30)
        .start();
    wait(&marks, &f, "the box enrolled", || !f.device().is_empty());
    let run = run_id('c');
    let dep = format!("dep_{}", "3".repeat(32));
    let item = deploy_item_by(
        &f,
        &run,
        &dep,
        &sha,
        &f.root,
        ("apps-platform", "arugula-salad/beds-smoke", &["APPS_PLATFORM_TOKEN", "SOURCE_TOKEN"]),
    );
    f.items.lock().unwrap().push(item);
    wait(&marks, &f, "the deploy ran", || f.reports_of(&run).len() == 2);
    let reports = f.reports_of(&run);
    let (logs, _) = f.uploads_of(&run);
    let mut all = Vec::new();
    for u in &logs {
        let Upload::Log { n, .. } = u else { unreachable!() };
        all.extend_from_slice(&f.objects.lock().unwrap()[&format!("{run}/log/{n}")]);
    }
    let text = String::from_utf8_lossy(&all).into_owned();
    assert_eq!(
        states(&reports),
        [(3, "approved".into(), "running".into()), (4, "running".into(), "succeeded".into())],
        "{text}"
    );
    assert!(text.contains("apps-platform: pushed ") && text.contains("/healthz is ready"), "{text}");
    assert!(*asked.lock().unwrap() >= 3, "it waited for the app");
    assert_eq!(
        *revoked.lock().unwrap(),
        [WRITE.to_ascii_lowercase(), READ.to_ascii_lowercase()],
        "both tokens revoked from the box"
    );
    let list = marks.join("remotes/arugula-salad/planter-apps.git");
    assert_eq!(git(&list, &["rev-parse", "main^"]), list_head, "one commit");
    assert_eq!(git(&list, &["diff-tree", "--no-commit-id", "--name-only", "-r", "main"]), "apps/beds-smoke.yaml");
    let pointer = git(&list, &["show", "main:apps/beds-smoke.yaml"]);
    assert!(pointer.contains(&format!("  ref:\n    commit: {sha}")), "{pointer}");
    std::thread::sleep(Duration::from_millis(300));
    for token in [WRITE, READ] {
        assert!(!text.contains(token), "a token is in the pane: {text}");
        let found: Vec<PathBuf> =
            [d.state.as_path(), marks.as_path()].iter().flat_map(|dir| holding(dir, token.as_bytes())).collect();
        assert!(found.is_empty(), "a token is on disk: {found:?}");
    }
    let _ = std::fs::remove_dir_all(&marks);
}

/// M221: Arugula's own release on a deploy's box, through the daemon: its
/// minted token comes on the driver's standard input, `arugulad
/// _arugula-release` runs the commit's real `scripts/release bump` against
/// a bare repository standing in for GitHub, which tags v0.1.1 and pushes
/// it with main; the write token is revoked; the release workflow is
/// followed with the read token (a fake API shaped as GitHub's), which is
/// revoked after; the run succeeds and reports the tag on its result pipe.
/// Neither token is ever on disk or in the pane.
#[test]
fn a_deploy_box_releases_arugula() {
    const TOKEN: &str = "ghs_m221releaseTOKENneverOnDisk0000000"; // gitleaks:allow
    const READ: &str = "ghs_m221readTOKENneverOnDisk000000000"; // gitleaks:allow
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::new();
    *f.purpose.lock().unwrap() = Some("deploy".into());
    *f.given.lock().unwrap() = Some(vec![
        Secret { name: "RELEASE_TOKEN".into(), value: TOKEN.into(), revoke: None },
        Secret { name: "RELEASE_READ_TOKEN".into(), value: READ.into(), revoke: None },
    ]);
    let url = serve(&rt, f.clone());
    let marks: PathBuf = std::env::temp_dir().join(format!("arugula-release-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&marks);
    std::fs::create_dir_all(&marks).unwrap();
    let seed = include_str!("../../../arugulad-drivers/src/arugula_release_seed.sh");
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/release");
    let out = std::process::Command::new("bash").args(["-c", seed, "seed"]).arg(&marks).arg(script).output().unwrap();
    assert!(out.status.success(), "seed: {}", String::from_utf8_lossy(&out.stderr));
    let two = String::from_utf8_lossy(&out.stdout).split(' ').next().unwrap().to_owned();
    // GitHub's API: the release workflow's run, done; its jobs; the
    // release; and the token's revoke.
    let revoked = Arc::new(Mutex::new(Vec::<String>::new()));
    let api = rt.block_on(async {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        let revoked = revoked.clone();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
            while let Ok((mut s, _)) = l.accept().await {
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = req.split_whitespace().nth(1).unwrap_or_default().to_owned();
                let repo = "/repos/arugula-salad/arugula";
                let (status, body) = if req.starts_with("DELETE /installation/token ") {
                    let auth = req
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("authorization: bearer ").map(str::to_owned));
                    revoked.lock().unwrap().push(auth.unwrap_or_default());
                    ("204 No Content", String::new())
                } else if path.starts_with(&format!("{repo}/actions/runs?")) {
                    let body = r#"{"total_count":1,"workflow_runs":[{"conclusion":"success","event":"push","head_branch":"v0.1.1","html_url":"https://github.com/arugula-salad/arugula/actions/runs/39845854046","id":39845854046,"name":"release","path":".github/workflows/release.yml","status":"completed"}]}"#;
                    ("200 OK", body.to_owned())
                } else if path.starts_with(&format!("{repo}/actions/runs/39845854046/jobs")) {
                    ("200 OK", r#"{"total_count":1,"jobs":[{"name":"publish","status":"completed","conclusion":"success"}]}"#.into())
                } else if path.starts_with(&format!("{repo}/releases/tags/v0.1.1")) {
                    ("200 OK", r#"{"tag_name":"v0.1.1","draft":false,"prerelease":false}"#.into())
                } else {
                    ("404 Not Found", String::new())
                };
                let _ = s
                    .write_all(format!("HTTP/1.1 {status}\r\ncontent-length: {}\r\n\r\n{body}", body.len()).as_bytes())
                    .await;
            }
        });
        at
    });
    let d = arugulad!("arugularelease")
        .no_wisp()
        .no_tailscale()
        .home_in_state()
        .args(["--hosted"])
        .env("ARUGULA_HOSTED_TICKET", "ticket-1")
        .env("ARUGULA_CONTROL_URL", &url)
        .env("ARUGULA_WORK_PULL_MS", "150")
        .env("ARUGULA_HOSTED_RUN_USER", "")
        .env("ARUGULA_HOSTED_SESSION_USERS", "")
        // No `runner` user here: a deploy's box refuses that, but in tests.
        .env("ARUGULA_HOSTED_TESTS_UNSAFE", "1")
        .env("ARUGULA_HOSTED_FAKE_DRIVER", "1")
        .env("ARUGULA_HOSTED_ARUGULA_RELEASE_GIT", format!("{}/", marks.join("remotes").display()))
        .env("ARUGULA_HOSTED_ARUGULA_RELEASE_API", &api)
        .env("RUST_LOG", "info")
        .args(["--log-file", &marks.join("arugulad.log").to_string_lossy()])
        .wait_secs(30)
        .start();
    wait(&marks, &f, "the box enrolled", || !f.device().is_empty());
    let run = run_id('d');
    let dep = format!("dep_{}", "4".repeat(32));
    let settings = [("bump".to_owned(), "patch".to_owned()), ("lane".to_owned(), "daemon".to_owned())].into();
    let item = deploy_item_with(
        &f,
        &run,
        &dep,
        &two,
        &f.root,
        ("arugula-release", "arugula-salad/arugula", &["RELEASE_TOKEN", "RELEASE_READ_TOKEN"]),
        settings,
    );
    f.items.lock().unwrap().push(item);
    wait(&marks, &f, "the deploy ran", || f.reports_of(&run).len() == 2);
    let reports = f.reports_of(&run);
    let (logs, _) = f.uploads_of(&run);
    let mut all = Vec::new();
    for u in &logs {
        let Upload::Log { n, .. } = u else { unreachable!() };
        all.extend_from_slice(&f.objects.lock().unwrap()[&format!("{run}/log/{n}")]);
    }
    let text = String::from_utf8_lossy(&all).into_owned();
    assert_eq!(
        states(&reports),
        [(3, "approved".into(), "running".into()), (4, "running".into(), "succeeded".into())],
        "{text}"
    );
    assert!(text.contains("scripts/release: pushed v0.1.1") && text.contains("v0.1.1 is out"), "{text}");
    let end: serde_json::Value = serde_json::from_str(&reports[1].detail).unwrap();
    assert_eq!(end["result"]["tag"], "v0.1.1", "{end}");
    assert_eq!(end["result"]["published"], true, "{end}");
    assert_eq!(
        *revoked.lock().unwrap(),
        [TOKEN.to_ascii_lowercase(), READ.to_ascii_lowercase()],
        "both tokens revoked from the box, the write one first"
    );
    let bare = marks.join("remotes/arugula-salad/arugula.git");
    assert_eq!(git(&bare, &["rev-parse", "v0.1.1^{commit}^"]), two, "v0.1.1 is a bump on the commit");
    assert!(git(&bare, &["cat-file", "-p", "v0.1.1"]).contains(&format!("Arugula-Deploy: {dep}")));
    std::thread::sleep(Duration::from_millis(300));
    for token in [TOKEN, READ] {
        assert!(!text.contains(token), "a token is in the pane: {text}");
        let found: Vec<PathBuf> =
            [d.state.as_path(), marks.as_path()].iter().flat_map(|dir| holding(dir, token.as_bytes())).collect();
        assert!(found.is_empty(), "a token is on disk: {found:?}");
    }
    let _ = std::fs::remove_dir_all(&marks);
}
