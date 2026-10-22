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
//!   in its pane.

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
    /// The bucket's objects, by key (M213).
    objects: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    /// The uploads control recorded, in order.
    recorded: Arc<Mutex<Vec<Upload>>>,
    /// Puts the bucket refused, and why.
    refused: Arc<Mutex<Vec<String>>>,
    /// No run-log bucket.
    off: Arc<AtomicBool>,
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
            objects: Default::default(),
            recorded: Default::default(),
            refused: Default::default(),
            off: Default::default(),
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

    // No bucket: the run runs, and nothing's sent.
    f.off.store(true, SeqCst);
    let b = run_id('b');
    f.items.lock().unwrap().push(f.item(&b, "echo quiet", f.grant(&f.root)));
    d.wait_for("run b reported", || f.reports_of(&b).len() == 2);
    assert_eq!(states(&f.reports_of(&b)), ran(1));
    assert_eq!(f.uploads_of(&b), (vec![], vec![]));
    assert!(!f.objects.lock().unwrap().keys().any(|k| k.starts_with(&b)));
}
