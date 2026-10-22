//! The forge mirror (M164) through the platform's own webhook route and
//! poller, against a fake GitHub: webhooks write rows (in order, metadata
//! only, linked repos only), the poller backfills, reconciles and counts
//! drift, leaves the rate reserve be, and polls a repo on one machine at
//! a time. Runs on SQLite and, with `ARUGULA_TEST_POSTGRES_URL`
//! (`just test-platform-postgres`), Postgres.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use arugula_e2e::now_ms;
use aws_lc_rs::{encoding::AsDer, rsa::KeySize};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

use super::{GithubApp, ingest::Settings, poller};
use crate::{
    Platform,
    chat_tests::TestHub,
    db::{
        Db, Sql as _,
        forge::{Installation, LinkedRepo},
        sql::params,
    },
};

const SECRET: &str = "whsec-test";
const REPO: u64 = 77;
/// How many items a fake list page holds, so lists page.
const PAGE: usize = 2;

/// What the fake GitHub has of `acme/app` (repo 77): its name and
/// privacy, its PRs and issues by number, check runs and statuses by head,
/// reviews by PR; its rate limit; installation 9, uninstalled or
/// suspended; repos it says are gone (and how); whether its lists fail;
/// and what was asked of it.
#[derive(Default)]
struct Gh {
    base: String,
    name: String,
    private: bool,
    uninstalled: bool,
    suspended: bool,
    gone: BTreeMap<u64, u16>,
    broken: bool,
    prs: BTreeMap<u64, Value>,
    issues: BTreeMap<u64, Value>,
    runs: BTreeMap<String, Vec<Value>>,
    statuses: BTreeMap<String, Vec<Value>>,
    reviews: BTreeMap<u64, Vec<Value>>,
    /// Each PR's `mergeable_state`, which only its own read carries.
    merge: BTreeMap<u64, &'static str>,
    /// The PRs read one by one, in order.
    pr_reads: Vec<u64>,
    remaining: u64,
    mints: usize,
    reads: usize,
}

type Shared = Arc<Mutex<Gh>>;

/// GitHub's time for `ms`.
fn iso(ms: u64) -> String {
    let s = ms / 1000;
    let (days, rem) = ((s / 86_400) as i64, s % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// A PR as GitHub says it, updated at `at` (ms), with its head `sha`.
fn pr(number: u64, title: &str, state: &str, at: u64, sha: &str) -> Value {
    json!({ "number": number, "title": title, "state": state, "body": "SECRET-PR-BODY",
        "user": { "login": "bob", "id": 2 }, "labels": [], "assignees": [], "requested_reviewers": [],
        "requested_teams": [], "draft": false, "merged_at": null,
        "head": { "ref": format!("b{number}"), "sha": sha }, "base": { "ref": "main", "sha": "base" },
        "created_at": iso(at.saturating_sub(1000)), "updated_at": iso(at) })
}

fn issue(number: u64, title: &str, state: &str, at: u64) -> Value {
    json!({ "number": number, "title": title, "state": state, "body": "SECRET-ISSUE-BODY",
        "user": { "login": "bob", "id": 2 }, "labels": [], "assignees": [],
        "created_at": iso(at.saturating_sub(1000)), "updated_at": iso(at) })
}

fn limits(h: &mut Gh) -> HeaderMap {
    h.reads += 1;
    h.remaining = h.remaining.saturating_sub(1);
    let mut m = HeaderMap::new();
    let mut set = |k: &'static str, v: String| {
        m.insert(k, HeaderValue::from_str(&v).unwrap());
    };
    set("x-ratelimit-limit", "5000".into());
    set("x-ratelimit-remaining", h.remaining.to_string());
    set("x-ratelimit-reset", (now_ms() / 1000 + 3600).to_string());
    set("x-ratelimit-resource", "core".into());
    m
}

async fn token(State(h): State<Shared>, Json(b): Json<Value>) -> Response {
    // Read only, for one repo: what the poller's mints ask for, and all the
    // installation here grants (GitHub refuses a mint that asks for more).
    if super::links_tests::exceeds(&b["permissions"], &super::links_tests::read_grant()) {
        return super::links_tests::not_granted();
    }
    let repo = b["repository_ids"][0].as_u64().unwrap();
    assert_eq!(b["repository_ids"], json!([repo]), "{b}");
    let mut h = h.lock().unwrap();
    if h.uninstalled {
        return (StatusCode::NOT_FOUND, Json(json!({ "message": "Not Found" }))).into_response();
    }
    h.mints += 1;
    Json(json!({ "token": "ghs_fake", "expires_at": "2099-01-01T00:00:00Z", "repositories": [{ "id": repo }] }))
        .into_response()
}

/// The App's own view of installation 9 (its JWT), or of all of them.
fn installation_9(h: &Gh) -> Value {
    json!({ "id": 9, "account": { "login": "acme" }, "suspended_at": if h.suspended { json!("2026-10-01T00:00:00Z") } else { Value::Null } })
}

async fn installation(State(h): State<Shared>, Path(id): Path<u64>) -> Response {
    let h = h.lock().unwrap();
    if h.uninstalled || id != 9 {
        return StatusCode::NOT_FOUND.into_response();
    }
    Json(installation_9(&h)).into_response()
}

async fn installations(State(h): State<Shared>) -> Response {
    let h = h.lock().unwrap();
    let all = if h.uninstalled { vec![] } else { vec![installation_9(&h)] };
    Json(json!(all)).into_response()
}

async fn repo_read(State(h): State<Shared>, Path(id): Path<u64>) -> Response {
    let mut h = h.lock().unwrap();
    if let Some(s) = h.gone.get(&id) {
        let s = StatusCode::from_u16(*s).unwrap();
        return (s, Json(json!({ "message": "Repository access blocked" }))).into_response();
    }
    let name = if id == REPO { h.name.clone() } else { format!("acme/r{id}") };
    let private = h.private;
    (limits(&mut h), Json(json!({ "id": id, "full_name": name, "private": private }))).into_response()
}

async fn one_pr(State(h): State<Shared>, Path((_, n)): Path<(u64, u64)>) -> Response {
    let mut h = h.lock().unwrap();
    h.pr_reads.push(n);
    match h.prs.get(&n).cloned() {
        Some(mut v) => {
            if let Some(m) = h.merge.get(&n) {
                v["mergeable_state"] = json!(m);
            }
            (limits(&mut h), Json(v)).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn one_issue(State(h): State<Shared>, Path((_, n)): Path<(u64, u64)>) -> Response {
    let mut h = h.lock().unwrap();
    match h.issues.get(&n).cloned() {
        Some(v) => (limits(&mut h), Json(v)).into_response(),
        None => (StatusCode::GONE, Json(json!({ "message": "This issue was deleted" }))).into_response(),
    }
}

/// A list page: newest first when sorted by update, else by number.
fn paged(h: &mut Gh, path: &str, mut all: Vec<Value>, q: &BTreeMap<String, String>) -> Response {
    if h.broken {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let state = q.get("state").map_or("open", String::as_str);
    all.retain(|v| state == "all" || v["state"] == state);
    if q.get("sort").map(String::as_str) == Some("updated") {
        all.sort_by_key(|v| std::cmp::Reverse(super::ingest::ms(&v["updated_at"])));
    }
    pages(h, path, all, q, None)
}

/// A page of `all` (under `key`, when GitHub wraps it in an object).
fn pages(h: &mut Gh, path: &str, all: Vec<Value>, q: &BTreeMap<String, String>, key: Option<&str>) -> Response {
    let page: usize = q.get("page").and_then(|p| p.parse().ok()).unwrap_or(1);
    let chunk: Vec<Value> = all.iter().skip((page - 1) * PAGE).take(PAGE).cloned().collect();
    let mut headers = limits(h);
    if all.len() > page * PAGE {
        let mut next: Vec<String> = q.iter().filter(|(k, _)| *k != "page").map(|(k, v)| format!("{k}={v}")).collect();
        next.push(format!("page={}", page + 1));
        let link = format!("<{}/{path}?{}>; rel=\"next\"", h.base, next.join("&"));
        headers.insert("link", HeaderValue::from_str(&link).unwrap());
    }
    let body = match key {
        Some(k) => json!({ "total_count": all.len(), k: chunk }),
        None => Value::Array(chunk),
    };
    (headers, Json(body)).into_response()
}

async fn pulls(State(h): State<Shared>, Path(id): Path<u64>, Query(q): Query<BTreeMap<String, String>>) -> Response {
    let mut h = h.lock().unwrap();
    let all = h.prs.values().cloned().collect();
    paged(&mut h, &format!("repositories/{id}/pulls"), all, &q)
}

async fn issues(State(h): State<Shared>, Path(id): Path<u64>, Query(q): Query<BTreeMap<String, String>>) -> Response {
    let mut h = h.lock().unwrap();
    // GitHub lists PRs as issues too.
    let mut all: Vec<Value> = h.issues.values().cloned().collect();
    all.extend(h.prs.values().map(|p| {
        json!({ "number": p["number"], "title": p["title"], "state": p["state"], "pull_request": {},
            "updated_at": p["updated_at"], "created_at": p["created_at"], "user": p["user"] })
    }));
    paged(&mut h, &format!("repositories/{id}/issues"), all, &q)
}

type Q = Query<BTreeMap<String, String>>;

async fn reviews(State(h): State<Shared>, Path((id, n)): Path<(u64, u64)>, Query(q): Q) -> Response {
    let mut h = h.lock().unwrap();
    let list = h.reviews.get(&n).cloned().unwrap_or_default();
    pages(&mut h, &format!("repositories/{id}/pulls/{n}/reviews"), list, &q, None)
}

async fn runs(State(h): State<Shared>, Path((id, sha)): Path<(u64, String)>, Query(q): Q) -> Response {
    let mut h = h.lock().unwrap();
    let list = h.runs.get(&sha).cloned().unwrap_or_default();
    pages(&mut h, &format!("repositories/{id}/commits/{sha}/check-runs"), list, &q, Some("check_runs"))
}

async fn statuses(State(h): State<Shared>, Path((id, sha)): Path<(u64, String)>, Query(q): Q) -> Response {
    let mut h = h.lock().unwrap();
    let list = h.statuses.get(&sha).cloned().unwrap_or_default();
    pages(&mut h, &format!("repositories/{id}/commits/{sha}/status"), list, &q, Some("statuses"))
}

async fn fake_github() -> (Shared, String) {
    let gh: Shared =
        Arc::new(Mutex::new(Gh { remaining: 5000, name: "acme/app".into(), private: true, ..Default::default() }));
    let r = Router::new()
        .route("/app/installations", get(installations))
        .route("/app/installations/{id}", get(installation))
        .route("/app/installations/{id}/access_tokens", post(token))
        .route("/repositories/{id}", get(repo_read))
        .route("/repositories/{id}/pulls/{n}", get(one_pr))
        .route("/repositories/{id}/issues/{n}", get(one_issue))
        .route("/repositories/{id}/pulls", get(pulls))
        .route("/repositories/{id}/issues", get(issues))
        .route("/repositories/{id}/pulls/{n}/reviews", get(reviews))
        .route("/repositories/{id}/commits/{sha}/check-runs", get(runs))
        .route("/repositories/{id}/commits/{sha}/status", get(statuses))
        .with_state(gh.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    gh.lock().unwrap().base = base.clone();
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
    (gh, base)
}

fn throwaway_key() -> aws_lc_rs::rsa::KeyPair {
    let k = aws_lc_rs::rsa::KeyPair::generate(KeySize::Rsa2048).unwrap();
    let der = AsDer::<aws_lc_rs::encoding::Pkcs8V1Der>::as_der(&k).unwrap();
    aws_lc_rs::rsa::KeyPair::from_pkcs8(der.as_ref()).unwrap()
}

/// The platform on `db`, with the App against the fake GitHub at `api`.
fn platform(db: Db, hub: &Arc<TestHub>, api: &str) -> Arc<Platform> {
    platform_with(db, hub, api, Settings::default())
}

fn platform_with(db: Db, hub: &Arc<TestHub>, api: &str, settings: Settings) -> Arc<Platform> {
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, db, hub.clone(), reqwest::Client::new(), Default::default());
    // Event rules' makers (M214) are on the hosted beta's list.
    p.hosted = crate::hosted::Config { accounts: vec!["alice".into()], ..Default::default() };
    let mut app = GithubApp::new("1".into(), "arugula-test".into(), SECRET.into(), api, throwaway_key());
    app.ingest.settings = settings;
    p.github = Some(app);
    Arc::new(p)
}

/// The fake GitHub, the platform, its address and its hub.
async fn serve() -> (Shared, Arc<Platform>, String, Arc<TestHub>) {
    serve_with(Settings::default()).await
}

async fn serve_with(settings: Settings) -> (Shared, Arc<Platform>, String, Arc<TestHub>) {
    let (gh, api) = fake_github().await;
    let hub = Arc::new(TestHub::default());
    let app = platform_with(Db::test(), &hub, &api, settings);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::cloud::tests::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (gh, app, base, hub)
}

/// Link `repo` (`acme/app` for 77) to a project, through installation 9,
/// as of `at`.
fn link(app: &Platform, repo: u64, at: u64) {
    let l = LinkedRepo {
        repo_id: repo,
        full_name: if repo == REPO { "acme/app".into() } else { format!("acme/r{repo}") },
        private: true,
        project_id: "prj_a".into(),
        installation_id: 9,
        linked_by: Some("alice".into()),
        linked_at: 0,
        unlinked_at: None,
        purge_after: None,
        mirror_content: false,
    };
    let i = Installation {
        id: 9,
        account_id: 500,
        login: "acme".into(),
        account_type: "Organization".into(),
        permissions: "{}".into(),
    };
    app.db.link_repo(&l, &i, "alice", at).unwrap();
}

async fn hook(base: &str, event: &str, delivery: &str, body: &Value) -> u16 {
    use hmac::{KeyInit, Mac};
    let body = body.to_string();
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(SECRET.as_bytes()).unwrap();
    mac.update(body.as_bytes());
    let sig = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
    reqwest::Client::new()
        .post(format!("{base}/github/webhook"))
        .header("x-github-event", event)
        .header("x-github-delivery", delivery)
        .header("x-hub-signature-256", sig)
        .body(body)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

fn repository(repo: u64) -> Value {
    json!({ "id": repo, "full_name": if repo == REPO { "acme/app".to_owned() } else { format!("acme/r{repo}") } })
}

/// An item's row: its title, state and GitHub time.
fn item(app: &Platform, number: u64) -> Option<(String, String, u64)> {
    use crate::db::sql::Optional as _;
    app.db
        .c()
        .query_row(
            "SELECT title, state, updated_at FROM forge_items WHERE repo_id = ?1 AND number = ?2",
            params![REPO, number],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .unwrap()
}

fn count(app: &Platform, sql: &str) -> u64 {
    app.db.c().query_row(sql, params![], |r| r.get(0)).unwrap()
}

/// A PR opened on GitHub is a row by the time the webhook's answered,
/// well inside 5 s, and its pokes still go to the hub.
#[tokio::test]
async fn a_pr_opened_is_a_row_at_once() {
    let (_gh, app, base, hub) = serve().await;
    link(&app, REPO, now_ms());
    let opened = json!({ "action": "opened", "repository": repository(REPO), "sender": { "login": "bob", "id": 2 },
        "pull_request": pr(5, "Fix the thing", "open", now_ms(), "abc") });
    let sent = Instant::now();
    assert_eq!(hook(&base, "pull_request", "d1", &opened).await, 202);
    let took = sent.elapsed();
    assert_eq!(item(&app, 5).map(|i| i.0).as_deref(), Some("Fix the thing"));
    assert!(took.as_millis() < 5000, "{took:?}");
    let l = app.github.as_ref().unwrap().ingest.stats.latency();
    assert_eq!((l.samples, l.event_samples), (1, 1));
    assert!(l.arrival_p95 < 1000, "{l:?}");
    assert!(l.event_p95 < 5000, "GitHub's second, then the row: {l:?}");
    assert_eq!(count(&app, "SELECT count(*) FROM forge_events WHERE kind = 'opened'"), 1);
    // The pokes, as before.
    let pokes = hub.pokes();
    assert_eq!((pokes.len(), pokes[0].number, pokes[0].repo.as_str()), (1, Some(5), "acme/app"));
}

/// A late redelivery of an older change doesn't overwrite a newer row.
#[tokio::test]
async fn an_older_delivery_leaves_the_newer_row() {
    let (_gh, app, base, _hub) = serve().await;
    link(&app, REPO, now_ms());
    let t = now_ms();
    let at = |title: &str, at: u64, action: &str| {
        json!({ "action": action, "repository": repository(REPO), "sender": { "login": "bob", "id": 2 },
            "pull_request": pr(5, title, "open", at, "abc") })
    };
    assert_eq!(hook(&base, "pull_request", "d2", &at("Renamed", t, "edited")).await, 202);
    assert_eq!(hook(&base, "pull_request", "d1", &at("First", t - 60_000, "opened")).await, 202);
    let (title, _, updated) = item(&app, 5).unwrap();
    assert_eq!((title.as_str(), updated), ("Renamed", t / 1000 * 1000));
}

/// Two answers from the same second that disagree (GitHub's times are
/// whole seconds): the row stays, and the poller's next turn, at once,
/// reads the item, or the head's checks, again and takes GitHub's answer.
#[tokio::test]
async fn a_same_second_tie_is_read_again() {
    let (gh, app, base, _hub) = serve().await;
    let t = now_ms();
    link(&app, REPO, t);
    poller::tick(&app, t).await.unwrap();
    let at = |title: &str, d: &str| {
        let body = json!({ "action": "edited", "repository": repository(REPO), "sender": {},
            "pull_request": pr(5, title, "open", t, "abc") });
        let base = base.clone();
        let d = d.to_owned();
        async move { hook(&base, "pull_request", &d, &body).await }
    };
    assert_eq!(at("First", "d1").await, 202);
    assert_eq!(at("Second", "d2").await, 202);
    assert_eq!(item(&app, 5).unwrap().0, "First", "a tie doesn't overwrite");
    let run = |conclusion: &str, d: &str| {
        let body = json!({ "action": "completed", "repository": repository(REPO),
            "check_run": { "name": "build", "head_sha": "abc", "status": "completed", "conclusion": conclusion,
                "started_at": iso(t), "completed_at": iso(t), "pull_requests": [{ "number": 5 }] } });
        let base = base.clone();
        let d = d.to_owned();
        async move { hook(&base, "check_run", &d, &body).await }
    };
    assert_eq!(run("failure", "c1").await, 202);
    assert_eq!(run("success", "c2").await, 202);
    let mut waiting = app.db.refetches(REPO).unwrap();
    waiting.sort();
    assert_eq!(waiting, [("head".to_owned(), "abc".to_owned()), ("pr".to_owned(), "5".to_owned())]);
    {
        let mut g = gh.lock().unwrap();
        g.prs.insert(5, pr(5, "Second", "open", t, "abc"));
        g.runs.insert(
            "abc".into(),
            vec![json!({ "name": "build", "head_sha": "abc", "status": "completed", "conclusion": "success",
                "started_at": iso(t), "completed_at": iso(t) })],
        );
    }
    // Not a reconcile (that's minutes away): only what tied.
    let turn = poller::tick(&app, t + 1000).await.unwrap();
    assert_eq!(turn, Some(poller::Turn::Refetched { repo: REPO, fixed: 2 }));
    assert_eq!(item(&app, 5).unwrap().0, "Second");
    assert_eq!(count(&app, "SELECT count(*) FROM forge_checks WHERE conclusion = 'success'"), 1);
    assert!(app.db.refetches(REPO).unwrap().is_empty());
    assert_eq!(poller::tick(&app, t + 2000).await.unwrap(), None);
}

/// A check run updates `forge_checks`, never back to an older state. A
/// commit status is kept for any commit, so one that comes before its
/// PR's new head is there when the head is; the poller prunes those on a
/// commit no item has as its head after an hour.
#[tokio::test]
async fn checks_follow_check_runs_and_statuses() {
    let (_gh, app, base, _hub) = serve().await;
    link(&app, REPO, now_ms());
    let t = now_ms();
    let run = |status: &str, conclusion: Value, at: u64| {
        json!({ "action": "completed", "repository": repository(REPO),
            "check_run": { "id": 1, "name": "build", "head_sha": "abc", "status": status, "conclusion": conclusion,
                "started_at": iso(t - 120_000), "completed_at": if conclusion.is_null() { Value::Null } else { json!(iso(at)) },
                "html_url": "https://github.com/acme/app/runs/1", "pull_requests": [{ "number": 5 }] } })
    };
    assert_eq!(hook(&base, "check_run", "c1", &run("completed", json!("success"), t)).await, 202);
    let check = || -> (String, Option<String>) {
        app.db
            .c()
            .query_row(
                "SELECT status, conclusion FROM forge_checks WHERE repo_id = ?1 AND head_sha = 'abc' AND name = 'build'",
                params![REPO],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    };
    assert_eq!(check(), ("completed".into(), Some("success".into())));
    assert_eq!(hook(&base, "check_run", "c0", &run("in_progress", Value::Null, 0)).await, 202);
    assert_eq!(check(), ("completed".into(), Some("success".into())), "an older delivery");

    let status = |sha: &str, d: &str, at: u64| {
        let body = json!({ "repository": repository(REPO), "sha": sha, "context": "lint", "state": "failure",
            "target_url": "https://ci.example/1", "updated_at": iso(at) });
        let base = base.clone();
        let d = d.to_owned();
        async move { hook(&base, "status", &d, &body).await }
    };
    assert_eq!(status("early", "s1", t).await, 202);
    assert_eq!(status("main-sha", "s2", t - 2 * 3_600_000).await, 202);
    assert_eq!(
        count(&app, "SELECT count(*) FROM forge_checks WHERE kind = 'status'"),
        2,
        "kept before any PR has them"
    );
    let opened = json!({ "action": "opened", "repository": repository(REPO), "sender": {},
        "pull_request": pr(5, "x", "open", t, "abc") });
    hook(&base, "pull_request", "p1", &opened).await;
    poller::tick(&app, t).await.unwrap();
    let shas: Vec<String> =
        app.db.c().query_map("SELECT head_sha FROM forge_checks ORDER BY head_sha", params![], |r| r.get(0)).unwrap();
    assert_eq!(shas, ["abc", "early"], "an hour-old status on no head is pruned");
}

/// A review lands as an event with its state; neither its text, the PR's
/// body, nor any comment's text is kept anywhere.
#[tokio::test]
async fn a_review_is_an_event_and_no_text_is_kept() {
    let (_gh, app, base, _hub) = serve().await;
    link(&app, REPO, now_ms());
    let t = now_ms();
    let review = json!({ "action": "submitted", "repository": repository(REPO),
        "pull_request": pr(5, "x", "open", t, "abc"),
        "review": { "id": 900, "state": "APPROVED", "body": "SECRET-REVIEW-TEXT", "user": { "login": "dan", "id": 4 },
            "submitted_at": iso(t) } });
    assert_eq!(hook(&base, "pull_request_review", "r1", &review).await, 202);
    let rc = json!({ "action": "created", "repository": repository(REPO), "pull_request": { "number": 5 },
        "comment": { "id": 901, "body": "SECRET-LINE-COMMENT", "user": { "login": "dan", "id": 4 }, "created_at": iso(t) } });
    assert_eq!(hook(&base, "pull_request_review_comment", "r2", &rc).await, 202);
    let ic = json!({ "action": "created", "repository": repository(REPO), "issue": issue(6, "An issue", "open", t),
        "comment": { "id": 902, "body": "SECRET-COMMENT", "user": { "login": "dan", "id": 4 }, "created_at": iso(t) } });
    assert_eq!(hook(&base, "issue_comment", "r3", &ic).await, 202);
    let (state, actor): (String, String) = app
        .db
        .c()
        .query_row(
            "SELECT review_state, actor_login FROM forge_events WHERE kind = 'review' AND github_id = 900",
            params![],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((state.as_str(), actor.as_str()), ("approved", "dan"));
    assert_eq!(count(&app, "SELECT count(*) FROM forge_events WHERE kind IN ('review_comment', 'comment')"), 2);
    assert_eq!(item(&app, 6).map(|i| i.0).as_deref(), Some("An issue"));
    assert!(app.db.mentions("SECRET").is_empty(), "{:?}", app.db.mentions("SECRET"));
}

/// A repo that isn't linked, or was unlinked, gets no rows; its pokes
/// still go.
#[tokio::test]
async fn an_unlinked_repo_gets_no_rows() {
    let (_gh, app, base, hub) = serve().await;
    let opened = |repo: u64| {
        json!({ "action": "opened", "repository": repository(repo), "sender": {},
            "pull_request": pr(5, "x", "open", now_ms(), "abc") })
    };
    assert_eq!(hook(&base, "pull_request", "u1", &opened(88)).await, 202);
    link(&app, REPO, now_ms());
    app.db.unlink_repo("prj_a", "acme/app", now_ms()).unwrap().unwrap();
    assert_eq!(hook(&base, "pull_request", "u2", &opened(REPO)).await, 202);
    for t in ["forge_items", "forge_events", "forge_checks"] {
        assert_eq!(count(&app, &format!("SELECT count(*) FROM {t}")), 0, "{t}");
    }
    assert_eq!(hub.pokes().len(), 2);
    // Nor does the poller read it.
    assert_eq!(poller::tick(&app, now_ms()).await.unwrap(), None);
}

/// Old open PRs and issues, and what changed in 90 days (paged), a PR's
/// reviews and its head's checks.
fn fill(gh: &Shared, now: u64) {
    let day = 86_400_000;
    let mut g = gh.lock().unwrap();
    g.prs.insert(1, pr(1, "old but open", "open", now - 100 * day, "sha1"));
    g.prs.insert(2, pr(2, "closed lately", "closed", now - 10 * day, "sha2"));
    g.prs.insert(3, pr(3, "closed long ago", "closed", now - 200 * day, "sha3"));
    for n in 4..=6 {
        g.prs.insert(n, pr(n, &format!("open {n}"), "open", now - n * 3_600_000, &format!("sha{n}")));
    }
    g.issues.insert(10, issue(10, "open issue", "open", now - 300 * day));
    g.issues.insert(11, issue(11, "closed issue", "closed", now - 5 * day));
    g.issues.insert(12, issue(12, "ancient issue", "closed", now - 120 * day));
    g.runs.insert(
        "sha1".into(),
        vec![json!({ "id": 1, "name": "build", "head_sha": "sha1", "status": "completed", "conclusion": "failure",
            "started_at": iso(now - day), "completed_at": iso(now - day), "html_url": "https://x/1" })],
    );
    g.statuses.insert(
        "sha4".into(),
        vec![json!({ "context": "lint", "state": "pending", "target_url": "", "updated_at": iso(now - day) })],
    );
    g.reviews.insert(
        2,
        vec![
            json!({ "id": 50, "state": "APPROVED", "body": "SECRET-REVIEW", "user": { "login": "dan", "id": 4 }, "submitted_at": iso(now - 11 * day) }),
            json!({ "id": 51, "state": "PENDING", "user": { "login": "eve", "id": 5 } }),
        ],
    );
}

/// A newly linked repo is backfilled on its poller's first turn, with
/// read-only tokens for it alone, logged as the mirror's.
#[tokio::test]
async fn backfill_fills_a_newly_linked_repo() {
    let (gh, app, _base, _hub) = serve().await;
    let now = now_ms();
    fill(&gh, now);
    link(&app, REPO, now);
    let turn = poller::tick(&app, now).await.unwrap();
    assert_eq!(turn, Some(poller::Turn::Done { repo: REPO, backfill: true, fixed: 10, drift: 0 }));
    let numbers: Vec<u64> = app
        .db
        .c()
        .query_map("SELECT number FROM forge_items WHERE repo_id = ?1 ORDER BY number", params![REPO], |r| r.get(0))
        .unwrap();
    assert_eq!(numbers, [1, 2, 4, 5, 6, 10, 11]);
    assert_eq!(item(&app, 2).unwrap().1, "closed");
    assert_eq!(count(&app, "SELECT count(*) FROM forge_checks WHERE head_sha = 'sha1' AND conclusion = 'failure'"), 1);
    assert_eq!(count(&app, "SELECT count(*) FROM forge_checks WHERE head_sha = 'sha4' AND status = 'pending'"), 1);
    assert_eq!(count(&app, "SELECT count(*) FROM forge_events WHERE kind = 'review'"), 1, "not the pending one");
    assert!(app.db.mentions("SECRET").is_empty(), "{:?}", app.db.mentions("SECRET"));
    let (backfilled, reconciled): (Option<u64>, Option<u64>) = app
        .db
        .c()
        .query_row("SELECT backfilled_at, reconciled_at FROM linked_repos WHERE repo_id = ?1", params![REPO], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((backfilled, reconciled), (Some(now), Some(now)));
    let mints = app.db.token_mints(0).unwrap();
    assert_eq!(mints.len(), 1, "one token, kept for the turn");
    assert_eq!(
        (mints[0].caller.as_str(), mints[0].purpose.as_str(), mints[0].repo_id, mints[0].installation_id),
        ("platform", "ingest", Some(REPO), Some(9))
    );
    // Not again until its turn.
    assert_eq!(poller::tick(&app, now + 1000).await.unwrap(), None);
}

/// A PR changed on GitHub with no webhook, and one opened with none, are
/// fixed by the next reconcile and counted as drift; one changed in the
/// last minute is fixed but not counted (its webhook may be coming). A
/// row a webhook already wrote isn't drift.
#[tokio::test]
async fn a_missed_webhook_is_fixed_and_counted() {
    let (gh, app, base, _hub) = serve().await;
    let t0 = now_ms();
    fill(&gh, t0);
    link(&app, REPO, t0);
    poller::tick(&app, t0).await.unwrap();
    let every = app.github.as_ref().unwrap().ingest.settings.every_ms;
    let t1 = t0 + every;
    {
        let mut g = gh.lock().unwrap();
        g.prs.insert(4, pr(4, "renamed with no webhook", "open", t1 - 180_000, "sha4"));
        g.prs.insert(7, pr(7, "opened with no webhook", "open", t1 - 120_000, "sha7"));
        g.prs.insert(8, pr(8, "a webhook on its way", "open", t1 - 10_000, "sha8"));
        g.prs.insert(9, pr(9, "its webhook came", "open", t1 - 100_000, "sha9"));
        g.runs.insert(
            "sha1".into(),
            vec![json!({ "id": 2, "name": "build", "head_sha": "sha1", "status": "completed", "conclusion": "success",
                "started_at": iso(t1 - 200_000), "completed_at": iso(t1 - 200_000), "html_url": "https://x/2" })],
        );
    }
    let came = json!({ "action": "opened", "repository": repository(REPO), "sender": {},
        "pull_request": gh.lock().unwrap().prs[&9].clone() });
    assert_eq!(hook(&base, "pull_request", "w9", &came).await, 202);

    let turn = poller::tick(&app, t1).await.unwrap();
    assert_eq!(turn, Some(poller::Turn::Done { repo: REPO, backfill: false, fixed: 4, drift: 3 }));
    assert_eq!(item(&app, 4).unwrap().0, "renamed with no webhook");
    assert_eq!(item(&app, 7).unwrap().0, "opened with no webhook");
    assert_eq!(item(&app, 8).unwrap().0, "a webhook on its way");
    assert_eq!(count(&app, "SELECT count(*) FROM forge_checks WHERE head_sha = 'sha1' AND conclusion = 'success'"), 1);
    let stats = &app.github.as_ref().unwrap().ingest.stats;
    assert_eq!(stats.drift_today(t1).get(&REPO), Some(&3));
    // And the next reconcile finds none.
    let turn = poller::tick(&app, t1 + every).await.unwrap();
    assert_eq!(turn, Some(poller::Turn::Done { repo: REPO, backfill: false, fixed: 0, drift: 0 }));
}

/// With less than the reserve left of the installation's limit, the
/// poller stops reading, gives its lease up and waits a minute; once the
/// limit has room again, it carries on.
#[tokio::test]
async fn the_poller_leaves_the_reserve_be() {
    let (gh, app, _base, _hub) = serve().await;
    let now = now_ms();
    fill(&gh, now);
    link(&app, REPO, now);
    gh.lock().unwrap().remaining = 1001; // 20% of 5000 after one read: under 30%.
    let turn = poller::tick(&app, now).await.unwrap();
    assert_eq!(turn, Some(poller::Turn::BackedOff { repo: REPO }));
    assert_eq!(gh.lock().unwrap().reads, 1, "one read heard the limit, and no more");
    let stats = &app.github.as_ref().unwrap().ingest.stats;
    let budgets = stats.budgets(now / 1000);
    assert_eq!((budgets[0].0, budgets[0].1.remaining, budgets[0].2), (9, 1000, true));
    assert_eq!(poller::tick(&app, now_ms() + 1000).await.unwrap(), None, "it waits");
    // Reads on demand still may (it's their share), and once the limit
    // has room again, the poller does.
    gh.lock().unwrap().remaining = 4000;
    stats.heard(9, Some(5000), Some(4000), Some(now / 1000 + 3600));
    let turn = poller::tick(&app, now_ms() + poller::BACKOFF_MS).await.unwrap();
    assert!(matches!(turn, Some(poller::Turn::Done { backfill: true, .. })), "{turn:?}");
}

/// Two platform machines on one database, polling at once: one polls
/// the repo, the other finds nothing to do, and one token is minted.
#[tokio::test]
async fn two_machines_never_poll_one_repo() {
    let (gh, api) = fake_github().await;
    let now = now_ms();
    fill(&gh, now);
    let a = Db::test();
    let (b, file) = match a.second() {
        Some(b) => (b, None),
        None => {
            let path =
                std::env::temp_dir().join(format!("arugula-ingest-{}.db", hex::encode(arugula_e2e::random::<8>())));
            (Db::open(&path).unwrap(), Some(path))
        }
    };
    let a = match &file {
        Some(path) => Db::open(path).unwrap(),
        None => a,
    };
    let hub = Arc::new(TestHub::default());
    let (a, b) = (platform(a, &hub, &api), platform(b, &hub, &api));
    link(&a, REPO, now);
    let (x, y) = tokio::join!(poller::tick(&a, now), poller::tick(&b, now));
    let mut turns = [x.unwrap(), y.unwrap()];
    turns.sort_by_key(Option::is_some);
    assert_eq!(turns[0], None);
    assert!(matches!(turns[1], Some(poller::Turn::Done { backfill: true, .. })), "{turns:?}");
    assert_eq!(gh.lock().unwrap().mints, 1);
    drop((a, b));
    if let Some(f) = file {
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", f.display()));
        }
    }
}

/// Unlinking stops a turn at its next read, and the purge doesn't take a
/// repo whose poller holds the lease.
#[tokio::test]
async fn the_purge_waits_for_the_lease() {
    let (_gh, app, _base, _hub) = serve().await;
    let now = now_ms();
    link(&app, REPO, now);
    let job = app.db.claim_poll(now, 1).unwrap().unwrap();
    app.db.unlink_repo("prj_a", "acme/app", now).unwrap().unwrap();
    app.db.c().execute("UPDATE linked_repos SET purge_after = ?1 WHERE repo_id = ?2", params![now, REPO]).unwrap();
    assert_eq!(app.db.purge_one(now + 1).unwrap(), None, "leased");
    assert!(!app.db.renew_poll(&job, now + 2).unwrap(), "the turn stops");
    app.db.release_poll(&job, now + 2).unwrap();
    assert_eq!(app.db.purge_one(now + 3).unwrap(), Some(REPO));
}

/// `linked_repos` as the poller leaves it: name, privacy, unlinked and
/// purge times, failures in a row, lease until, and whether a cursor waits.
fn link_row(app: &Platform, repo: u64) -> (String, bool, Option<u64>, Option<u64>, u64, u64, bool) {
    app.db
        .c()
        .query_row(
            "SELECT full_name, private, unlinked_at, purge_after, poll_failures, lease_until, poll_cursor IS NOT NULL
             FROM linked_repos WHERE repo_id = ?1",
            params![repo],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .unwrap()
}

/// GitHub won't mint a token for the repo, and the App's own view says
/// the installation is gone: an uninstall whose webhook never came. Its
/// repos are unlinked, for the purge, as the webhook would have.
#[tokio::test]
async fn a_lost_uninstall_unlinks() {
    let (gh, app, _base, _hub) = serve().await;
    let now = now_ms();
    link(&app, REPO, now);
    link(&app, 88, now);
    gh.lock().unwrap().uninstalled = true;
    let turn = poller::tick(&app, now).await.unwrap();
    assert_eq!(turn, Some(poller::Turn::Gone { repo: REPO, unlinked: 2 }));
    for r in [REPO, 88] {
        let (_, _, unlinked, purge, ..) = link_row(&app, r);
        assert!(unlinked.is_some() && purge.is_some(), "repo {r}");
    }
    assert_eq!(
        count(&app, "SELECT count(*) FROM github_installations WHERE installation_id = 9 AND deleted_at IS NOT NULL"),
        1
    );
    assert_eq!(poller::tick(&app, now).await.unwrap(), None);
}

/// The repo's own read says it's gone (451: blocked; 404: deleted or
/// taken out of the installation) while the installation is there: that
/// repo is unlinked, and the installation's others stay.
#[tokio::test]
async fn a_repo_gone_from_github_unlinks_it() {
    let (gh, app, _base, _hub) = serve().await;
    let now = now_ms();
    link(&app, REPO, now);
    link(&app, 88, now);
    gh.lock().unwrap().gone.insert(REPO, 451);
    let turn = poller::tick(&app, now).await.unwrap();
    assert_eq!(turn, Some(poller::Turn::Gone { repo: REPO, unlinked: 1 }));
    assert!(link_row(&app, REPO).2.is_some());
    assert!(link_row(&app, 88).2.is_none(), "the installation's other repo");
    assert_eq!(count(&app, "SELECT count(*) FROM github_installations WHERE deleted_at IS NOT NULL"), 0);
    assert!(matches!(poller::tick(&app, now).await.unwrap(), Some(poller::Turn::Done { repo: 88, .. })));
}

/// The sweep unlinks an installation GitHub no longer lists, or lists as
/// suspended; one it lists as it is stays.
#[tokio::test]
async fn the_sweep_unlinks_what_github_no_longer_lists() {
    let (gh, app, _base, _hub) = serve().await;
    let now = now_ms();
    link(&app, REPO, now);
    assert_eq!(poller::sweep(&app, now).await.unwrap(), 0);
    assert!(link_row(&app, REPO).2.is_none());
    gh.lock().unwrap().suspended = true;
    assert_eq!(poller::sweep(&app, now).await.unwrap(), 1);
    assert_eq!(count(&app, "SELECT count(*) FROM github_installations WHERE suspended_at IS NOT NULL"), 1);
    link(&app, REPO, now);
    gh.lock().unwrap().uninstalled = true;
    assert_eq!(poller::sweep(&app, now).await.unwrap(), 1);
    assert_eq!(count(&app, "SELECT count(*) FROM github_installations WHERE deleted_at IS NOT NULL"), 1);
    assert!(link_row(&app, REPO).3.is_some(), "the purge follows");
}

/// A turn that fails for another reason (GitHub's 500) waits a minute,
/// then two, and so on, up to an hour; a turn that works starts again.
#[tokio::test]
async fn failures_back_off_longer_each_time() {
    let (gh, app, _base, _hub) = serve().await;
    let now = now_ms();
    link(&app, REPO, now);
    gh.lock().unwrap().broken = true;
    let mut waits = vec![];
    for _ in 0..3 {
        let at = link_row(&app, REPO).5.max(now_ms());
        assert!(poller::tick(&app, at).await.is_err());
        let (.., failures, until, _) = link_row(&app, REPO);
        waits.push((failures, (until - now_ms() + 30_000) / 60_000));
    }
    assert_eq!(waits, [(1, 1), (2, 2), (3, 4)]);
    assert!(link_row(&app, REPO).2.is_none(), "a 500 isn't gone");
    gh.lock().unwrap().broken = false;
    let at = link_row(&app, REPO).5;
    assert!(matches!(poller::tick(&app, at).await.unwrap(), Some(poller::Turn::Done { backfill: true, .. })));
    assert_eq!(link_row(&app, REPO).4, 0);
}

/// A backfill longer than a turn's pages keeps a cursor and carries on
/// at the next turns, at once, and drops nothing; it's backfilled only at
/// the end.
#[tokio::test]
async fn a_long_backfill_carries_on() {
    let (gh, app, _base, _hub) = serve_with(Settings { pages: 2, ..Settings::default() }).await;
    let now = now_ms();
    fill(&gh, now);
    link(&app, REPO, now);
    let mut more = 0;
    loop {
        match poller::tick(&app, now).await.unwrap() {
            Some(poller::Turn::More { .. }) => {
                more += 1;
                assert!(link_row(&app, REPO).6, "a cursor");
                let backfilled: Option<u64> = app
                    .db
                    .c()
                    .query_row("SELECT backfilled_at FROM linked_repos WHERE repo_id = ?1", params![REPO], |r| r.get(0))
                    .unwrap();
                assert_eq!(backfilled, None);
            }
            Some(poller::Turn::Done { backfill: true, .. }) => break,
            t => panic!("{t:?}"),
        }
        assert!(more < 20);
    }
    assert!(more >= 2, "{more}");
    let numbers: Vec<u64> = app
        .db
        .c()
        .query_map("SELECT number FROM forge_items WHERE repo_id = ?1 ORDER BY number", params![REPO], |r| r.get(0))
        .unwrap();
    assert_eq!(numbers, [1, 2, 4, 5, 6, 10, 11]);
    assert!(!link_row(&app, REPO).6);
}

/// Reviews, check runs and statuses longer than a page are read to the end.
#[tokio::test]
async fn long_lists_are_read_to_the_end() {
    let (gh, app, _base, _hub) = serve().await;
    let now = now_ms();
    {
        let mut g = gh.lock().unwrap();
        g.prs.insert(1, pr(1, "busy", "open", now - 3_600_000, "sha1"));
        g.reviews.insert(
            1,
            (0..5)
                .map(|i| json!({ "id": 100 + i, "state": "COMMENTED", "user": { "login": "dan", "id": 4 }, "submitted_at": iso(now - 3_600_000) }))
                .collect(),
        );
        g.runs.insert(
            "sha1".into(),
            (0..5)
                .map(|i| json!({ "name": format!("job {i}"), "head_sha": "sha1", "status": "completed", "conclusion": "success", "started_at": iso(now - 3_600_000) }))
                .collect(),
        );
        g.statuses.insert(
            "sha1".into(),
            (0..3)
                .map(
                    |i| json!({ "context": format!("ci {i}"), "state": "success", "updated_at": iso(now - 3_600_000) }),
                )
                .collect(),
        );
    }
    link(&app, REPO, now);
    poller::tick(&app, now).await.unwrap();
    assert_eq!(count(&app, "SELECT count(*) FROM forge_events WHERE kind = 'review'"), 5);
    assert_eq!(count(&app, "SELECT count(*) FROM forge_checks WHERE kind = 'check_run'"), 5);
    assert_eq!(count(&app, "SELECT count(*) FROM forge_checks WHERE kind = 'status'"), 3);
}

/// Every turn reads the repo, and keeps its name and privacy current.
#[tokio::test]
async fn the_poller_keeps_name_and_privacy_current() {
    let (gh, app, _base, _hub) = serve().await;
    let now = now_ms();
    link(&app, REPO, now);
    poller::tick(&app, now).await.unwrap();
    assert!(link_row(&app, REPO).1);
    {
        let mut g = gh.lock().unwrap();
        g.name = "acme/renamed".into();
        g.private = false;
    }
    let every = app.github.as_ref().unwrap().ingest.settings.every_ms;
    poller::tick(&app, now + every).await.unwrap();
    let (name, private, ..) = link_row(&app, REPO);
    assert_eq!((name.as_str(), private), ("acme/renamed", false));
}

/// Drift is only what GitHub changed since the last reconcile: an old PR
/// updated again is drift (its webhook didn't come), but its old reviews
/// are history, not drift. A status that came before its PR's new head
/// is there when the head is, and isn't drift either.
#[tokio::test]
async fn history_and_early_statuses_are_not_drift() {
    let (gh, app, base, _hub) = serve().await;
    let t0 = now_ms();
    fill(&gh, t0);
    link(&app, REPO, t0);
    poller::tick(&app, t0).await.unwrap();
    let every = app.github.as_ref().unwrap().ingest.settings.every_ms;
    let t1 = t0 + every;
    let day = 86_400_000;
    {
        let mut g = gh.lock().unwrap();
        g.prs.insert(3, pr(3, "reopened, no webhook", "open", t1 - 120_000, "sha3"));
        g.reviews.insert(
            3,
            vec![json!({ "id": 60, "state": "APPROVED", "user": { "login": "dan", "id": 4 }, "submitted_at": iso(t0 - 190 * day) })],
        );
        g.prs.insert(5, pr(5, "open 5", "open", t1 - 140_000, "new5"));
        g.statuses.insert(
            "new5".into(),
            vec![json!({ "context": "lint", "state": "success", "updated_at": iso(t1 - 150_000) })],
        );
    }
    // The status comes first, then the push that makes new5 PR 5's head.
    let status = json!({ "repository": repository(REPO), "sha": "new5", "context": "lint", "state": "success",
        "updated_at": iso(t1 - 150_000) });
    assert_eq!(hook(&base, "status", "s1", &status).await, 202);
    let sync = json!({ "action": "synchronize", "repository": repository(REPO), "sender": {},
        "pull_request": gh.lock().unwrap().prs[&5].clone() });
    assert_eq!(hook(&base, "pull_request", "p1", &sync).await, 202);

    let turn = poller::tick(&app, t1).await.unwrap();
    assert_eq!(turn, Some(poller::Turn::Done { repo: REPO, backfill: false, fixed: 2, drift: 1 }));
    assert_eq!(count(&app, "SELECT count(*) FROM forge_events WHERE github_id = 60"), 1, "written all the same");
}

/// An issue deleted, or transferred away, goes with its events and inbox
/// rows; its drafts stay (M170's).
#[tokio::test]
async fn a_deleted_issue_goes() {
    let (_gh, app, base, _hub) = serve().await;
    link(&app, REPO, now_ms());
    let t = now_ms();
    for (n, d) in [(6, "o6"), (7, "o7")] {
        let opened = json!({ "action": "opened", "repository": repository(REPO), "sender": { "login": "bob", "id": 2 },
            "issue": issue(n, "doomed", "open", t) });
        assert_eq!(hook(&base, "issues", d, &opened).await, 202);
    }
    let id: String =
        app.db.c().query_row("SELECT id FROM forge_items WHERE number = 6", params![], |r| r.get(0)).unwrap();
    app.db
        .c()
        .execute(
            "INSERT INTO inbox_items (account, item, reason, created_at, updated_at) VALUES ('alice', ?1, 'mention', 1, 1)",
            params![id],
        )
        .unwrap();
    app.db
        .c()
        .execute(
            "INSERT INTO drafts (id, project_id, author, repo_id, number, kind, body, created_at, updated_at)
             VALUES ('dft_1', 'prj_a', 'alice', ?1, 6, 'comment', 'lgtm', 1, 1)",
            params![REPO],
        )
        .unwrap();
    for (n, action, d) in [(6, "deleted", "x6"), (7, "transferred", "x7")] {
        let gone = json!({ "action": action, "repository": repository(REPO), "sender": {}, "issue": issue(n, "doomed", "open", t) });
        assert_eq!(hook(&base, "issues", d, &gone).await, 202);
    }
    assert_eq!(item(&app, 6), None);
    assert_eq!(item(&app, 7), None);
    assert_eq!(count(&app, "SELECT count(*) FROM forge_events"), 0);
    assert_eq!(count(&app, "SELECT count(*) FROM inbox_items"), 0);
    assert_eq!(count(&app, "SELECT count(*) FROM drafts"), 1, "M170's");
}

/// M214: a linked repo's PR opened, through the webhook, starts the run
/// its project's rule asks for, once however often GitHub delivers it.
/// One whose rule failed is answered 500 and forgotten, so GitHub's
/// redelivery of it starts the run.
#[tokio::test]
async fn a_webhook_starts_its_rules_runs() {
    use crate::db::triggers::{RuleEvent, tests as t};
    use arugula_e2e::team::TeamRole;
    let (_gh, app, base, hub) = serve().await;
    hub.team("t", &[("alice", TeamRole::Owner)]);
    t::seed(&app.db);
    link(&app, REPO, now_ms());
    let spec = app.db.add_run_spec("prj_a", "review", "hw_a", &t::cmd(&["just", "review"]), "alice", 1).unwrap();
    app.db.add_run_rule(&spec, RuleEvent::PrOpened, None, Some("main"), false, 20, "alice", 1).unwrap();
    let opened = |n: u64| {
        let mut v = json!({ "action": "opened", "repository": repository(REPO), "sender": { "login": "bob", "id": 2 },
            "pull_request": pr(n, "Fix the thing", "open", now_ms(), "abc") });
        v["pull_request"]["author_association"] = json!("MEMBER");
        v["pull_request"]["head"]["repo"] = repository(REPO);
        v
    };
    assert_eq!(hook(&base, "pull_request", "d1", &opened(5)).await, 202);
    let runs = t::runs_in(&app.db, "prj_a");
    assert_eq!(runs.len(), 1);
    assert_eq!(
        (runs[0].source["event"].as_str(), runs[0].source["detail"]["number"].as_u64()),
        (Some("pr_opened"), Some(5))
    );
    assert_eq!(t::run_jobs(&app.db, &runs[0].id), 1);
    // A duplicate delivery is dropped; a redelivery under a new id makes no
    // second run either.
    assert_eq!(hook(&base, "pull_request", "d1", &opened(5)).await, 200);
    assert_eq!(hook(&base, "pull_request", "d2", &opened(5)).await, 202);
    assert_eq!(t::runs_in(&app.db, "prj_a").len(), 1);

    // The runs table away: the rule fails, the delivery is forgotten.
    app.db.c().execute("ALTER TABLE runs RENAME TO runs_away", params![]).unwrap();
    assert_eq!(hook(&base, "pull_request", "d3", &opened(6)).await, 500);
    app.db.c().execute("ALTER TABLE runs_away RENAME TO runs", params![]).unwrap();
    assert_eq!(hook(&base, "pull_request", "d3", &opened(6)).await, 202, "redelivered");
    assert_eq!(t::runs_in(&app.db, "prj_a").len(), 2);
}

/// A PR's merge state, as the row has it.
fn merge_state(app: &Platform, number: u64) -> Option<String> {
    app.db
        .c()
        .query_row(
            "SELECT merge_state FROM forge_items WHERE repo_id = ?1 AND number = ?2",
            params![REPO, number],
            |r| r.get(0),
        )
        .unwrap()
}

/// Lists don't carry a PR's merge state: the poller reads each open PR
/// whose state it doesn't know, one by one, and learns a conflict a later
/// turn; a green PR's again once its row is old (its base may have moved).
/// Neither is drift: no webhook carries it.
#[tokio::test]
async fn the_poller_learns_merge_states() {
    let (gh, app, _base, _hub) = serve().await;
    let t0 = now_ms();
    fill(&gh, t0);
    {
        let mut g = gh.lock().unwrap();
        // PR 4 green; GitHub hasn't worked its state out yet.
        g.statuses.insert(
            "sha4".into(),
            vec![
                json!({ "context": "lint", "state": "success", "target_url": "", "updated_at": iso(t0 - 86_400_000) }),
            ],
        );
        g.merge = BTreeMap::from([(1, "clean"), (4, "unknown"), (5, "clean"), (6, "clean")]);
    }
    link(&app, REPO, t0);
    poller::tick(&app, t0).await.unwrap();
    let mut read = std::mem::take(&mut gh.lock().unwrap().pr_reads);
    read.sort();
    assert_eq!(read, [1, 4, 5, 6], "the open PRs, which a list didn't say");
    assert_eq!(merge_state(&app, 4).as_deref(), Some("unknown"));
    assert_eq!(merge_state(&app, 2), None, "a closed one isn't read");

    // GitHub works it out: a conflict. The next reconcile reads PR 4 alone.
    gh.lock().unwrap().merge.insert(4, "dirty");
    let every = app.github.as_ref().unwrap().ingest.settings.every_ms;
    let t1 = t0 + every;
    let turn = poller::tick(&app, t1).await.unwrap();
    assert_eq!(turn, Some(poller::Turn::Done { repo: REPO, backfill: false, fixed: 1, drift: 0 }));
    assert_eq!(std::mem::take(&mut gh.lock().unwrap().pr_reads), [4]);
    assert_eq!(merge_state(&app, 4).as_deref(), Some("dirty"));
    let stats = &app.github.as_ref().unwrap().ingest.stats;
    assert_eq!(stats.drift_today(t1).get(&REPO).copied().unwrap_or(0), 0);

    // Known, and read lately: not again yet.
    poller::tick(&app, t1 + every).await.unwrap();
    assert!(gh.lock().unwrap().pr_reads.is_empty());
    // The base moved, and the conflict's gone: a green PR's read again once
    // its row is old. Not PR 1 (red), nor 5 and 6 (no checks).
    gh.lock().unwrap().merge.insert(4, "clean");
    let t2 = t1 + poller::MERGE_RECHECK_MS;
    let turn = poller::tick(&app, t2).await.unwrap();
    assert_eq!(turn, Some(poller::Turn::Done { repo: REPO, backfill: false, fixed: 1, drift: 0 }));
    assert_eq!(std::mem::take(&mut gh.lock().unwrap().pr_reads), [4]);
    assert_eq!(merge_state(&app, 4).as_deref(), Some("clean"));
    assert_eq!(stats.drift_today(t2).get(&REPO).copied().unwrap_or(0), 0);
}
