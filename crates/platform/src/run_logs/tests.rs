//! Run logs and artifacts (M213) against a fake S3 in this process, which
//! checks each request's presigned signature as S3 would (from what it
//! received: the method, the path, the `Host` and the signed headers) and
//! keeps the objects in memory:
//! - a hosted machine puts its run's log in two chunks and an artifact,
//!   and a member reads all of it through the platform with the box gone;
//! - what the machine may send: its own runs only, the next chunk only, a
//!   chunk sent again is kept once, the caps, nothing before the bucket has
//!   it, nothing without a bucket;
//! - after the run-log days (short here) the sweep forgets them, and the
//!   bucket's files go too;
//! - the operator's usage, by project.
//!
//! On SQLite and, with `ARUGULA_TEST_POSTGRES_URL`, on Postgres.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use arugula_control_wire::run_logs::{Upload, UploadGiven, Uploaded};
use axum::{
    Router,
    body::Bytes,
    extract::{Path, RawQuery, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::json;
use sha2::{Digest as _, Sha256};

use super::{
    bucket::{Bucket, Sign, encode, signature},
    *,
};
use crate::{
    chat_tests::{TestHub, call},
    db::{
        Db,
        runs::{NewRun, TriggerKind},
        sql::{Sql as _, params},
    },
};

const BUCKET: &str = "arugula-run-logs";
const KEY_ID: &str = "test-key-id";
const SECRET: &str = "test-secret"; // gitleaks:allow
const DAY: u64 = 86_400_000;

/// An S3 that checks presigned URLs and keeps objects in memory.
#[derive(Clone, Default)]
struct FakeS3 {
    objects: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    /// Requests it refused, and why.
    refused: Arc<Mutex<Vec<String>>>,
    /// GETs it answered.
    gets: Arc<std::sync::atomic::AtomicUsize>,
}

impl FakeS3 {
    fn has(&self, key: &str) -> Option<Vec<u8>> {
        self.objects.lock().unwrap().get(key).cloned()
    }

    /// Whether the request's presigned signature holds, as S3 checks it:
    /// `None` when it does.
    fn check(&self, method: &Method, path: &str, query: &str, headers: &HeaderMap) -> Option<String> {
        let q: BTreeMap<String, String> =
            query.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_owned(), percent(v))).collect();
        let get = |k: &str| q.get(k).cloned().unwrap_or_default();
        if get("X-Amz-Algorithm") != "AWS4-HMAC-SHA256" {
            return Some("not SigV4".into());
        }
        let cred = get("X-Amz-Credential");
        let parts: Vec<&str> = cred.split('/').collect();
        if parts.len() != 5 || parts[0] != KEY_ID || parts[3] != "s3" || parts[4] != "aws4_request" {
            return Some(format!("a credential that isn't ours: {cred}"));
        }
        let at = get("X-Amz-Date");
        let expires: u64 = get("X-Amz-Expires").parse().unwrap_or(0);
        let signed_at = jiff::fmt::strtime::parse("%Y%m%dT%H%M%SZ", &at)
            .and_then(|t| t.to_datetime())
            .and_then(|t| t.to_zoned(jiff::tz::TimeZone::UTC))
            .map(|t| t.timestamp().as_millisecond() as u64)
            .unwrap_or(0);
        if arugula_e2e::now_ms() > signed_at + expires * 1000 {
            return Some("expired".into());
        }
        let signed = get("X-Amz-SignedHeaders");
        let host = headers.get("host").and_then(|h| h.to_str().ok()).unwrap_or_default().to_owned();
        let mut extra: Vec<(&str, String)> = Vec::new();
        for name in signed.split(';').filter(|n| *n != "host") {
            let Some(v) = headers.get(name).and_then(|v| v.to_str().ok()) else {
                return Some(format!("{name} is signed but not sent"));
            };
            extra.push((name, v.to_owned()));
        }
        let canonical = q
            .iter()
            .filter(|(k, _)| *k != "X-Amz-Signature")
            .map(|(k, v)| format!("{}={}", encode(k, false), encode(v, false)))
            .collect::<Vec<_>>()
            .join("&");
        let s = Sign {
            method: method.as_str(),
            scheme: "http",
            host: &host,
            path,
            region: parts[2],
            key_id: KEY_ID,
            secret: SECRET,
            at: &at,
            expires_s: expires,
            headers: &extra,
            query: &[],
        };
        (signature(&s, &canonical) != get("X-Amz-Signature")).then(|| "the signature doesn't hold".into())
    }
}

/// `%XX` back to bytes.
fn percent(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

async fn object(
    State(s): State<FakeS3>,
    method: Method,
    uri: axum::http::Uri,
    Path((bucket, key)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = uri.path().to_owned();
    if let Some(why) = s.check(&method, &path, query.as_deref().unwrap_or_default(), &headers) {
        s.refused.lock().unwrap().push(format!("{method} {path}: {why}"));
        return (StatusCode::FORBIDDEN, why).into_response();
    }
    if bucket != BUCKET {
        return StatusCode::NOT_FOUND.into_response();
    }
    match method {
        Method::PUT => {
            s.objects.lock().unwrap().insert(key, body.to_vec());
            StatusCode::OK.into_response()
        }
        Method::DELETE => {
            s.objects.lock().unwrap().remove(&key);
            StatusCode::NO_CONTENT.into_response()
        }
        _ => {
            if method == Method::GET {
                s.gets.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            // What S3 answers a GET signed with `response-content-*`.
            let q: BTreeMap<String, String> = query
                .unwrap_or_default()
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k.to_owned(), percent(v)))
                .collect();
            match s.has(&key) {
                Some(b) => {
                    let mut r = b.into_response();
                    for (param, header) in [
                        ("response-content-type", "content-type"),
                        ("response-content-disposition", "content-disposition"),
                    ] {
                        if let Some(v) = q.get(param) {
                            r.headers_mut().insert(header, v.parse().unwrap());
                        }
                    }
                    r
                }
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }
    }
}

async fn serve_s3() -> (FakeS3, String) {
    let s = FakeS3::default();
    let app = Router::new().route("/{bucket}/{*key}", get(object).put(object).delete(object)).with_state(s.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (s, at)
}

struct World {
    app: Arc<Platform>,
    base: String,
    project: String,
    s3: FakeS3,
    me: HostedMachine,
}

/// Team `t` (alice owns it, vic views it), project `prj_t`'s hosted
/// workspace `hw_a` whose box enrolled as `dev_a`, and team `u` (olga);
/// the platform's routes served, with a run-log bucket kept `ttl_ms`
/// unless `bucket` is false.
async fn world(bucket: bool, ttl_ms: u64) -> World {
    world_with(bucket, ttl_ms, super::bucket::DAILY_BYTES, None).await
}

/// [`world`], its projects sending at most `daily` bytes a day, and its
/// bucket at `at` instead of the fake's (one that can't be reached).
async fn world_with(bucket: bool, ttl_ms: u64, daily: u64, at: Option<&str>) -> World {
    let hub = Arc::new(TestHub::default());
    hub.team("t", &[("alice", arugula_e2e::team::TeamRole::Owner), ("vic", arugula_e2e::team::TeamRole::Viewer)]);
    hub.team("u", &[("olga", arugula_e2e::team::TeamRole::Owner)]);
    let (s3, s3_at) = serve_s3().await;
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, Db::test(), hub, reqwest::Client::new(), Default::default());
    if bucket {
        let mut b = Bucket::new(at.unwrap_or(&s3_at), BUCKET, "auto", KEY_ID, SECRET, ttl_ms).unwrap();
        b.daily_bytes = daily;
        p.run_logs = Some(b);
    }
    let app = Arc::new(p);
    let project = app.db.team_project("t", "t", 0).unwrap();
    app.db
        .c()
        .execute(
            "INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, machine, created_at, updated_at)
             VALUES ('hw_a', ?1, 'dev', '{}', 'h', 'ready', 'dev_a', 1, 1)",
            params![project],
        )
        .unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    let me =
        HostedMachine { project: project.clone(), workspace: "hw_a".into(), machine: "dev_a".into(), sign: "".into() };
    World { app, base, project, s3, me }
}

impl World {
    /// A run on `hw_a`, as its machine started it at `started` (`None`:
    /// it hasn't).
    fn run(&self, started: Option<u64>) -> String {
        let op = "arugula op v1\ndaemon dev_a\nkind run\nsession -\npane - -\ncwd -\nargv dHJ1ZQ==\n";
        let new = NewRun {
            project: &self.project,
            workspace: Some("hw_a"),
            trigger: TriggerKind::Manual,
            source: &json!({}),
            op,
            started_by: "alice",
            started_by_agent: None,
            grant: Some("g1"),
        };
        let run = self.app.db.create_run(&new, 1).unwrap().unwrap().id;
        if let Some(at) = started {
            self.app
                .db
                .c()
                .execute("UPDATE runs SET state = 'running', started_at = ?2 WHERE id = ?1", params![run, at])
                .unwrap();
        }
        run
    }

    fn end(&self, run: &str, at: u64) {
        self.app
            .db
            .c()
            .execute(
                "UPDATE runs SET state = 'succeeded', ended_at = ?2, exit_code = 0 WHERE id = ?1",
                params![run, at],
            )
            .unwrap();
    }

    /// Ask, put and record `bytes` as `u` says, as the machine does.
    async fn send(&self, u: &Upload, bytes: &[u8]) -> Uploaded {
        let UploadGiven::Put { url, .. } = upload(&self.app, &self.me, u).await.unwrap() else {
            panic!("no URL for {u:?}")
        };
        let r = reqwest::Client::new().put(&url).body(bytes.to_vec()).send().await.unwrap();
        assert!(r.status().is_success(), "{}: {:?}", r.status(), self.s3.refused.lock().unwrap());
        uploaded(&self.app, &self.me, u).await.unwrap()
    }
}

fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

fn log(run: &str, n: u32, offset: u64, b: &[u8]) -> Upload {
    Upload::Log { run: run.into(), n, offset, size: b.len() as u64, sha256: sha(b) }
}

fn artifact(run: &str, name: &str, b: &[u8]) -> Upload {
    Upload::Artifact { run: run.into(), name: name.into(), size: b.len() as u64, sha256: sha(b) }
}

/// A member reads a run's whole log through M216's route, `max` bytes a
/// page: the status of the first page that isn't 200, and the text.
async fn read_all(base: &str, who: &str, run: &str, max: u64) -> (u16, String) {
    let mut out = String::new();
    let mut offset = 0;
    loop {
        let (st, v) = call(base, who, "GET", &format!("/api/runs/{run}/log?offset={offset}&max={max}"), None).await;
        if st != 200 {
            return (st, out);
        }
        assert_eq!(v["kept"], json!(true), "{v}");
        out.push_str(v["text"].as_str().unwrap());
        let next = v["next_offset"].as_u64().unwrap();
        if next == offset {
            return (200, out);
        }
        offset = next;
    }
}

/// The done-when: a run's whole log and an artifact, read through the
/// platform's routes with the box gone.
#[tokio::test]
async fn a_runs_log_and_artifact_outlive_its_box() {
    let w = world(true, 30 * DAY).await;
    let now = arugula_e2e::now_ms();
    let run = w.run(Some(now));
    let (one, two) = (b"arugula run ...\nbuilding\n".repeat(100), b"tests passed\n".to_vec());
    assert_eq!(w.send(&log(&run, 0, 0, &one), &one).await, Uploaded::Kept);
    assert_eq!(w.send(&log(&run, 1, one.len() as u64, &two), &two).await, Uploaded::Kept);
    w.end(&run, now);
    // After it ended, within the grace: its artifact.
    let report = b"<testsuite tests=\"3\"/>".to_vec();
    assert_eq!(w.send(&artifact(&run, "junit.xml", &report), &report).await, Uploaded::Kept);
    let key = format!("{}/{run}/log/0", w.project);
    assert_eq!(w.s3.has(&key).as_deref(), Some(&one[..]), "at the run's prefix");

    // The box goes.
    w.app
        .db
        .c()
        .execute(
            "UPDATE hosted_workspaces SET state = 'deleted', machine = NULL, deleted_at = 5 WHERE id = 'hw_a'",
            params![],
        )
        .unwrap();

    let p = &w.project;
    // The whole log, through the platform a page at a time (pages that
    // cross chunks), and from the middle.
    let want = String::from_utf8([one.clone(), two.clone()].concat()).unwrap();
    assert_eq!(read_all(&w.base, "vic", &run, 1000).await, (200, want.clone()));
    let (st, v) = call(&w.base, "alice", "GET", &format!("/api/runs/{run}/log?offset=10"), None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["text"].as_str().unwrap(), &want[10..]);
    assert_eq!((v["next_offset"].clone(), v["end"].clone()), (json!(want.len()), json!(true)));
    // Its artifacts, each with a signed GET that works as it is.
    let (st, v) = call(&w.base, "vic", "GET", &format!("/api/projects/{p}/runs/{run}/artifacts"), None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["on"], json!(true));
    assert_eq!(v["artifacts"][0]["digest"], json!(format!("sha256:{}", sha(&report))));
    let url = v["artifacts"][0]["url"].as_str().unwrap();
    assert_eq!(reqwest::get(url).await.unwrap().bytes().await.unwrap().to_vec(), report);
    // The artifact, by its redirect to a signed GET.
    let r = reqwest::Client::new()
        .get(format!("{}/api/projects/{p}/runs/{run}/artifacts/junit.xml", w.base))
        .header("cookie", "ilg_session=session-alice")
        .send()
        .await
        .unwrap();
    assert_eq!(r.bytes().await.unwrap().to_vec(), report);

    // Someone outside the project, and a run that isn't the project's.
    for path in
        [format!("/api/projects/{p}/runs/{run}/artifacts"), format!("/api/projects/{p}/runs/run_nope/artifacts")]
    {
        let who = if path.contains("nope") { "alice" } else { "olga" };
        assert_eq!(call(&w.base, who, "GET", &path, None).await.0, 404, "{who} {path}");
    }
    assert_eq!(read_all(&w.base, "olga", &run, 1000).await.0, 404);
    assert!(w.s3.refused.lock().unwrap().is_empty(), "{:?}", w.s3.refused.lock().unwrap());

    // The bucket changed under it after it was read and checked: what was
    // checked is what's served (a chunk's key is never written again). A
    // chunk read changed is refused: `sizes_are_checked…`.
    w.s3.objects.lock().unwrap().insert(key, b"something else".to_vec());
    assert_eq!(read_all(&w.base, "alice", &run, 1000).await, (200, want));
}

/// What a machine may send, and what it may not.
#[tokio::test]
async fn only_the_machines_own_run_and_its_next_chunk() {
    let w = world(true, 30 * DAY).await;
    let now = arugula_e2e::now_ms();
    let run = w.run(Some(now));
    let a = b"hello\n".to_vec();
    let refused = |g: UploadGiven| match g {
        UploadGiven::Refused { why, next } => (why, next),
        g => panic!("not refused: {g:?}"),
    };

    // Another machine, another workspace's, another project's.
    for m in [
        HostedMachine { machine: "dev_b".into(), ..w.me.clone() },
        HostedMachine { workspace: "hw_b".into(), ..w.me.clone() },
        HostedMachine { project: "prj_other".into(), ..w.me.clone() },
    ] {
        let (why, _) = refused(upload(&w.app, &m, &log(&run, 0, 0, &a)).await.unwrap());
        assert!(why.contains("isn't this machine's"), "{why}");
    }
    // Not chunk 0 first; nothing recorded before the bucket has it.
    assert_eq!(refused(upload(&w.app, &w.me, &log(&run, 1, 0, &a)).await.unwrap()).1, Some(0));
    assert!(matches!(uploaded(&w.app, &w.me, &log(&run, 0, 0, &a)).await.unwrap(), Uploaded::Refused { .. }));
    assert_eq!(w.send(&log(&run, 0, 0, &a), &a).await, Uploaded::Kept);
    // Sent again (an answer lost): kept once, and the next is 1.
    assert_eq!(upload(&w.app, &w.me, &log(&run, 0, 0, &a)).await.unwrap(), UploadGiven::Kept);
    assert_eq!(uploaded(&w.app, &w.me, &log(&run, 0, 0, &a)).await.unwrap(), Uploaded::Kept);
    assert_eq!(w.app.db.run_log_chunks(&run, 0).unwrap().len(), 1);
    // Chunk 0 again with other bytes; chunk 1 overlapping chunk 0.
    assert_eq!(refused(upload(&w.app, &w.me, &log(&run, 0, 0, b"other")).await.unwrap()).1, Some(1));
    assert!(refused(upload(&w.app, &w.me, &log(&run, 1, 2, &a)).await.unwrap()).0.contains("before the last"));
    // A gap (the pane's log dropped bytes) is fine.
    assert_eq!(w.send(&log(&run, 1, 100, &a), &a).await, Uploaded::Kept);
    // The caps and the forms.
    let big = Upload::Log { run: run.clone(), n: 2, offset: 200, size: 9 * 1024 * 1024, sha256: sha(b"") };
    assert!(refused(upload(&w.app, &w.me, &big).await.unwrap()).0.contains("8 MiB"));
    for name in [".env", "a/b", "../x", ""] {
        let (why, _) = refused(upload(&w.app, &w.me, &artifact(&run, name, &a)).await.unwrap());
        assert!(why.contains("artifact's name"), "{name}: {why}");
    }
    let huge = Upload::Artifact { run: run.clone(), name: "big".into(), size: 101 * 1024 * 1024, sha256: sha(b"") };
    assert!(refused(upload(&w.app, &w.me, &huge).await.unwrap()).0.contains("100 MiB"));
    let bad = Upload::Log { run: run.clone(), n: 2, offset: 200, size: 1, sha256: "nope".into() };
    assert!(refused(upload(&w.app, &w.me, &bad).await.unwrap()).0.contains("SHA-256"));
    // The URL takes exactly its size: a longer body is refused by the
    // bucket, and nothing's recorded.
    let u = log(&run, 2, 200, &a);
    let UploadGiven::Put { url, .. } = upload(&w.app, &w.me, &u).await.unwrap() else { panic!() };
    let r = reqwest::Client::new().put(&url).body(b"hello, longer\n".to_vec()).send().await.unwrap();
    assert_eq!(r.status(), 403);
    assert!(matches!(uploaded(&w.app, &w.me, &u).await.unwrap(), Uploaded::Refused { .. }));
    // An artifact's name once.
    assert_eq!(w.send(&artifact(&run, "out.txt", &a), &a).await, Uploaded::Kept);
    assert!(refused(upload(&w.app, &w.me, &artifact(&run, "out.txt", b"x")).await.unwrap()).0.contains("by that name"));

    // A run that hasn't started, and one that ended past the grace.
    let waiting = w.run(None);
    assert!(refused(upload(&w.app, &w.me, &log(&waiting, 0, 0, &a)).await.unwrap()).0.contains("takes no uploads"));
    let old = w.run(Some(now - 3 * GRACE_MS));
    w.end(&old, now - 2 * GRACE_MS);
    assert!(refused(upload(&w.app, &w.me, &log(&old, 0, 0, &a)).await.unwrap()).0.contains("takes no uploads"));

    // Without a bucket, it's off.
    let off = world(false, DAY).await;
    let run = off.run(Some(now));
    assert_eq!(upload(&off.app, &off.me, &log(&run, 0, 0, &a)).await.unwrap(), UploadGiven::Off);
    let (st, v) =
        call(&off.base, "alice", "GET", &format!("/api/projects/{}/runs/{run}/artifacts", off.project), None).await;
    assert_eq!((st, v["on"].clone()), (200, json!(false)));
    let (st, v) = call(&off.base, "alice", "GET", &format!("/api/runs/{run}/log"), None).await;
    assert_eq!((st, v["kept"].clone()), (200, json!(false)), "{v}");
}

/// After the run-log days (a minute here) the sweep forgets a run's log
/// and artifacts, the reads stop, and the bucket's files go. The usage
/// the operator reads counts what's there.
#[tokio::test]
async fn gone_after_the_days() {
    let ttl = 60_000;
    let w = world(true, ttl).await;
    let now = arugula_e2e::now_ms();
    let run = w.run(Some(now));
    let a = b"one\ntwo\n".to_vec();
    assert_eq!(w.send(&log(&run, 0, 0, &a), &a).await, Uploaded::Kept);
    assert_eq!(w.send(&artifact(&run, "a.bin", &a), &a).await, Uploaded::Kept);
    let usage = w.app.db.run_log_usage().unwrap();
    assert_eq!(
        usage,
        vec![crate::db::run_logs::Usage { project: w.project.clone(), runs: 1, log_bytes: 8, artifact_bytes: 8 }]
    );
    let shown = render_usage(&usage, "test.db");
    assert!(shown.contains(&w.project) && shown.contains("16 B in all"), "{shown}");

    // Not yet.
    assert_eq!(sweep(&w.app, now + ttl / 2).await.unwrap(), 0);
    assert_eq!(w.s3.objects.lock().unwrap().len(), 2);
    // Past the days.
    assert_eq!(sweep(&w.app, arugula_e2e::now_ms() + ttl + 1_000).await.unwrap(), 2);
    assert!(w.s3.objects.lock().unwrap().is_empty(), "the files went too");
    let p = &w.project;
    let (st, v) = call(&w.base, "alice", "GET", &format!("/api/projects/{p}/runs/{run}/artifacts"), None).await;
    assert_eq!((st, v["artifacts"].clone()), (200, json!([])));
    let (st, v) = call(&w.base, "alice", "GET", &format!("/api/runs/{run}/log"), None).await;
    assert_eq!((st, v["kept"].clone()), (200, json!(false)), "{v}");
    assert_eq!(
        call(&w.base, "alice", "GET", &format!("/api/projects/{p}/runs/{run}/artifacts/a.bin"), None).await.0,
        404
    );
    assert!(w.app.db.run_log_usage().unwrap().is_empty());
    // The run and its history stay.
    assert!(w.app.db.run(&run).unwrap().is_some());
    assert!(!w.app.db.run_events(&run).unwrap().is_empty());
}

/// Review 1: a log followed a page at a time reads each chunk from the
/// bucket once; people's reads of logs are limited per minute. Review 6: a
/// chunk the bucket's rule already took is skipped, not the whole log.
#[tokio::test]
async fn a_followed_log_reads_each_chunk_once() {
    use std::sync::atomic::Ordering::SeqCst;
    let w = world(true, 30 * DAY).await;
    let now = arugula_e2e::now_ms();
    let run = w.run(Some(now));
    let (one, two) = (b"first chunk\n".to_vec(), b"second chunk\n".to_vec());
    assert_eq!(w.send(&log(&run, 0, 0, &one), &one).await, Uploaded::Kept);
    assert_eq!(w.send(&log(&run, 1, one.len() as u64, &two), &two).await, Uploaded::Kept);
    for _ in 0..10 {
        let (st, v) = call(&w.base, "alice", "GET", &format!("/api/runs/{run}/log?offset=3&max=4"), None).await;
        assert_eq!((st, v["text"].clone()), (200, json!("st c")), "{v}");
    }
    assert_eq!(read_all(&w.base, "alice", &run, 5).await.1, "first chunk\nsecond chunk\n");
    // Its tail.
    let (st, v) = call(&w.base, "alice", "GET", &format!("/api/runs/{run}/log?tail=true&max=5"), None).await;
    assert_eq!((st, v["text"].clone()), (200, json!("hunk\n")), "{v}");
    assert_eq!(w.s3.gets.load(SeqCst), 2, "each chunk from the bucket once");

    // The rule took chunk 0 (a fresh platform's cache is empty): the log
    // reads from chunk 1.
    let w2 = world(true, 30 * DAY).await;
    let run2 = w2.run(Some(now));
    assert_eq!(w2.send(&log(&run2, 0, 0, &one), &one).await, Uploaded::Kept);
    assert_eq!(w2.send(&log(&run2, 1, one.len() as u64, &two), &two).await, Uploaded::Kept);
    w2.s3.objects.lock().unwrap().remove(&format!("{}/{run2}/log/0", w2.project));
    assert_eq!(read_all(&w2.base, "alice", &run2, 1000).await, (200, "second chunk\n".to_owned()));

    // People's reads are limited: 120 a minute.
    let mut last = 0;
    for _ in 0..125 {
        last = call(&w.base, "vic", "GET", &format!("/api/runs/{run}/log"), None).await.0;
        if last == 429 {
            break;
        }
    }
    assert_eq!(last, 429);
}

/// Review 2: every URL out counts, so asking and never putting can't fill
/// the bucket: a project's daily bytes, a run's URLs a minute, and its
/// artifacts (out or kept) are capped; the sweep deletes what was put at a
/// URL nobody said they used. Review 3: an offset that would wrap.
#[tokio::test]
async fn upload_urls_out_count_against_the_caps() {
    let w = world_with(true, 30 * DAY, 100, None).await;
    let now = arugula_e2e::now_ms();
    let run = w.run(Some(now));
    let sixty = vec![b'x'; 60];
    let refused = |g: UploadGiven| match g {
        UploadGiven::Refused { why, .. } => why,
        g => panic!("not refused: {g:?}"),
    };
    // Asked for, never put: still 60 of the day's 100.
    let UploadGiven::Put { url, .. } = upload(&w.app, &w.me, &log(&run, 0, 0, &sixty)).await.unwrap() else { panic!() };
    let why = refused(upload(&w.app, &w.me, &artifact(&run, "a", &sixty)).await.unwrap());
    assert!(why.contains("most for a day"), "{why}");
    // Put there, never said: the sweep deletes it once its URL is long
    // expired, and its row.
    assert!(reqwest::Client::new().put(&url).body(sixty.clone()).send().await.unwrap().status().is_success());
    let key = format!("{}/{run}/log/0", w.project);
    assert!(w.s3.has(&key).is_some());
    assert_eq!(sweep(&w.app, now + 5 * 60_000).await.unwrap(), 0, "its URL is still good");
    assert_eq!(sweep(&w.app, now + (PUT_EXPIRES_S * 1000) + UNRECORDED_MS + 60_000).await.unwrap(), 1);
    assert!(w.s3.has(&key).is_none(), "deleted from the bucket");
    let rows: u64 = w.app.db.c().query_row("SELECT count(*) FROM run_upload_urls", params![], |r| r.get(0)).unwrap();
    assert_eq!(rows, 0);

    // A run's URLs a minute.
    let w = world(true, 30 * DAY).await;
    let run = w.run(Some(now));
    let a = b"a".to_vec();
    for _ in 0..PER_MINUTE {
        assert!(matches!(upload(&w.app, &w.me, &log(&run, 0, 0, &a)).await.unwrap(), UploadGiven::Put { .. }));
    }
    assert!(refused(upload(&w.app, &w.me, &log(&run, 0, 0, &a)).await.unwrap()).contains("in a minute"));

    // Artifacts out count: 50 names out, the 51st is refused; asking again
    // for one that's out is fine.
    let run = w.run(Some(now));
    w.app.db.c().execute("DELETE FROM run_upload_urls", params![]).unwrap();
    for i in 0..MAX_ARTIFACTS {
        let g = upload(&w.app, &w.me, &artifact(&run, &format!("f{i}"), &a)).await.unwrap();
        assert!(matches!(g, UploadGiven::Put { .. }), "{i}: {g:?}");
    }
    assert!(refused(upload(&w.app, &w.me, &artifact(&run, "one-more", &a)).await.unwrap()).contains("most artifacts"));
    assert!(matches!(upload(&w.app, &w.me, &artifact(&run, "f0", &a)).await.unwrap(), UploadGiven::Put { .. }));
    // Kept ones are counted in the transaction that adds one.
    let full = |i: usize| crate::db::runs::Artifact {
        run: run.clone(),
        name: format!("k{i}"),
        size: 1,
        digest: "sha256:x".into(),
        bucket_key: format!("k{i}"),
        created_at: now,
    };
    for i in 0..3 {
        assert!(w.app.db.add_run_artifact_capped(&full(i), 3).unwrap().is_some());
    }
    assert!(w.app.db.add_run_artifact_capped(&full(3), 3).unwrap().is_none());

    // An offset that would wrap, or reach past any pane's.
    let run = w.run(Some(now));
    let wraps = Upload::Log { run: run.clone(), n: 0, offset: u64::MAX - 1, size: 2, sha256: sha(b"ab") };
    assert!(refused(upload(&w.app, &w.me, &wraps).await.unwrap()).contains("offset"));
}

/// Review 3, 4: a chunk in the bucket of another size is refused; an
/// unreachable bucket's errors never carry a signed URL.
#[tokio::test]
async fn sizes_are_checked_and_signed_urls_stay_out_of_errors() {
    let w = world(true, 30 * DAY).await;
    let now = arugula_e2e::now_ms();
    let run = w.run(Some(now));
    let a = b"twelve bytes".to_vec();
    assert_eq!(w.send(&log(&run, 0, 0, &a), &a).await, Uploaded::Kept);
    w.s3.objects.lock().unwrap().insert(format!("{}/{run}/log/0", w.project), b"short".to_vec());
    let r = w.app.db.run(&run).unwrap().unwrap();
    let e = crate::run_log::page(&w.app, &r, crate::run_log::From::Offset(0), 100).await.unwrap_err().to_string();
    assert!(e.contains("5 bytes, not the 12"), "{e}");

    let down = world_with(true, 30 * DAY, super::bucket::DAILY_BYTES, Some("http://127.0.0.1:9")).await;
    let run = down.run(Some(now));
    let e = uploaded(&down.app, &down.me, &log(&run, 0, 0, &a)).await.unwrap_err().to_string();
    assert!(e.contains("can't reach") && !e.contains("X-Amz"), "{e}");
    down.app
        .db
        .add_run_log_chunk(&crate::db::run_logs::LogChunk {
            run: run.clone(),
            n: 0,
            offset: 0,
            size: 12,
            sha256: sha(&a),
            bucket_key: "k".into(),
            created_at: now,
        })
        .unwrap();
    let r = down.app.db.run(&run).unwrap().unwrap();
    let e =
        format!("{:#}", crate::run_log::page(&down.app, &r, crate::run_log::From::Offset(0), 100).await.unwrap_err());
    assert!(!e.contains("X-Amz") && !e.contains("127.0.0.1:9/"), "{e}");
}

/// Review 5: an artifact downloads as bytes, never shows as a page.
/// Review 6: deleting the account its project goes with deletes its runs'
/// files from the bucket, not only the rows.
#[tokio::test]
async fn artifacts_download_and_go_with_their_project() {
    let w = world(true, 30 * DAY).await;
    let now = arugula_e2e::now_ms();
    let run = w.run(Some(now));
    let page = b"<script>alert(1)</script>".to_vec();
    let a = b"log\n".to_vec();
    assert_eq!(w.send(&log(&run, 0, 0, &a), &a).await, Uploaded::Kept);
    assert_eq!(w.send(&artifact(&run, "page.html", &page), &page).await, Uploaded::Kept);
    let r = reqwest::Client::new()
        .get(format!("{}/api/projects/{}/runs/{run}/artifacts/page.html", w.base, w.project))
        .header("cookie", "ilg_session=session-alice")
        .send()
        .await
        .unwrap();
    assert_eq!(r.headers()["content-type"], "application/octet-stream");
    assert_eq!(r.headers()["content-disposition"], "attachment; filename=\"page.html\"");
    assert_eq!(r.bytes().await.unwrap().to_vec(), page);
    // An upload URL out, too.
    let UploadGiven::Put { .. } = upload(&w.app, &w.me, &artifact(&run, "later", &a)).await.unwrap() else { panic!() };

    // Alice deletes her account and disbands the team: its project goes.
    let d = crate::Deleted { account: "alice".into(), machines: vec![], disbanded: vec!["t".into()] };
    crate::PlatformPort::account_deleted(&*w.app, &d).await.unwrap();
    assert!(w.app.db.run(&run).unwrap().is_none(), "the run went");
    assert!(w.s3.objects.lock().unwrap().is_empty(), "{:?}", w.s3.objects.lock().unwrap().keys().collect::<Vec<_>>());
    for t in ["run_log_chunks", "run_artifacts", "run_upload_urls"] {
        let n: u64 = w.app.db.c().query_row(&format!("SELECT count(*) FROM {t}"), params![], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "{t}");
    }
}
