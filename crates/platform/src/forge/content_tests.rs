//! Bodies on read and content mirroring (M166) through the platform's own
//! routes, webhook and poller, against a fake GitHub: no content rows
//! without the opt-in, rows with it, none after opting out; a refused
//! reader costs GitHub nothing; the read cache and what drops it; search
//! over mirrored text; and no text in the logs. Runs on SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL` (`just test-platform-postgres`), Postgres.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use arugula_e2e::{now_ms, team::TeamRole};
use aws_lc_rs::{encoding::AsDer, rsa::KeySize};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

use super::{GithubApp, links_tests::hook, poller};
use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::{
        Db, Sql as _,
        forge::{Installation, LinkedRepo},
        sql::params,
    },
};

/// acme/app: private, linked to team t1's project.
const APP: u64 = 77;
/// acme/site: public, linked too, never mirrored.
const SITE: u64 = 88;

/// What the fake GitHub has: who has which role on acme/app, the two
/// repos' PRs and issues, their comments, reviews and review comments,
/// and how many times content was read.
#[derive(Default)]
struct Gh {
    roles: BTreeMap<String, (String, u64)>,
    prs: BTreeMap<(u64, u64), Value>,
    issues: BTreeMap<(u64, u64), Value>,
    comments: BTreeMap<(u64, u64), Vec<Value>>,
    reviews: BTreeMap<(u64, u64), Vec<Value>>,
    threads: BTreeMap<(u64, u64), Vec<Value>>,
    content_reads: usize,
}

type Shared = Arc<Mutex<Gh>>;

fn limits() -> HeaderMap {
    let mut m = HeaderMap::new();
    m.insert("x-ratelimit-limit", HeaderValue::from_static("5000"));
    m.insert("x-ratelimit-remaining", HeaderValue::from_static("4000"));
    m.insert("x-ratelimit-resource", HeaderValue::from_static("core"));
    m.insert("x-ratelimit-reset", HeaderValue::from_str(&(now_ms() / 1000 + 3600).to_string()).unwrap());
    m
}

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

fn pr(number: u64, body: &str, at: u64) -> Value {
    json!({ "id": 5000 + number, "number": number, "title": format!("PR {number}"), "state": "open", "body": body,
        "user": { "login": "alice-gh", "id": 1001 }, "labels": [], "assignees": [], "requested_reviewers": [],
        "requested_teams": [], "draft": false, "merged_at": null,
        "head": { "ref": format!("b{number}"), "sha": format!("sha{number}") }, "base": { "ref": "main", "sha": "base" },
        "created_at": iso(at.saturating_sub(1000)), "updated_at": iso(at) })
}

fn issue(number: u64, body: &str, at: u64) -> Value {
    json!({ "id": 6000 + number, "number": number, "title": format!("Issue {number}"), "state": "open", "body": body,
        "user": { "login": "bob-gh", "id": 1002 }, "labels": [], "assignees": [],
        "created_at": iso(at.saturating_sub(1000)), "updated_at": iso(at) })
}

fn comment(id: u64, body: &str, at: u64) -> Value {
    json!({ "id": id, "body": body, "user": { "login": "bob-gh", "id": 1002 },
        "created_at": iso(at), "updated_at": iso(at) })
}

async fn installation(Path((o, r)): Path<(String, String)>) -> Response {
    match format!("{o}/{r}").as_str() {
        "acme/app" | "acme/site" => {
            Json(json!({ "id": 9, "account": { "id": 500, "login": "acme", "type": "Organization" } })).into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn token(Json(b): Json<Value>) -> Response {
    // Read only, for one repository, whatever it's for: the installation
    // here grants no more, and GitHub refuses a mint that asks for more.
    if super::links_tests::exceeds(&b["permissions"], &super::links_tests::read_grant()) {
        return super::links_tests::not_granted();
    }
    // GitHub's answer names the one repository it covers, as M167 checks.
    let covers = match (b["repository_ids"][0].as_u64(), b["repositories"][0].as_str()) {
        (Some(id), _) => json!({ "id": id }),
        (None, Some("app")) => json!({ "id": APP, "full_name": "acme/app" }),
        (None, Some("site")) => json!({ "id": SITE, "full_name": "acme/site" }),
        _ => json!({}),
    };
    Json(json!({ "token": "ghs_fake", "expires_at": "2099-01-01T00:00:00Z", "repositories": [covers] })).into_response()
}

async fn repo(Path((o, r)): Path<(String, String)>) -> Response {
    match format!("{o}/{r}").as_str() {
        "acme/app" => Json(json!({ "id": APP, "full_name": "acme/app", "private": true })).into_response(),
        "acme/site" => Json(json!({ "id": SITE, "full_name": "acme/site", "private": false })).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn repo_read(Path(id): Path<u64>) -> Response {
    match id {
        APP => (limits(), Json(json!({ "id": APP, "full_name": "acme/app", "private": true }))).into_response(),
        SITE => (limits(), Json(json!({ "id": SITE, "full_name": "acme/site", "private": false }))).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn app_installation(Path(id): Path<u64>) -> Response {
    Json(json!({ "id": id, "account": { "id": 500, "login": "acme", "type": "Organization" }, "suspended_at": null }))
        .into_response()
}

async fn permission(State(h): State<Shared>, Path((_, _, login)): Path<(String, String, String)>) -> Response {
    let Some((role, id)) = h.lock().unwrap().roles.get(&login).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let permission = if role == "maintain" { "write".to_owned() } else { role.clone() };
    Json(json!({ "permission": permission, "role_name": role, "user": { "id": id, "login": login } })).into_response()
}

fn listed(all: Vec<Value>) -> Response {
    (limits(), Json(Value::Array(all))).into_response()
}

async fn pulls(State(h): State<Shared>, Path(id): Path<u64>, Query(_): Query<BTreeMap<String, String>>) -> Response {
    let h = h.lock().unwrap();
    listed(h.prs.iter().filter(|((r, _), _)| *r == id).map(|(_, v)| v.clone()).collect())
}

async fn issues(State(h): State<Shared>, Path(id): Path<u64>, Query(_): Query<BTreeMap<String, String>>) -> Response {
    let h = h.lock().unwrap();
    listed(h.issues.iter().filter(|((r, _), _)| *r == id).map(|(_, v)| v.clone()).collect())
}

fn one(found: Option<&Value>) -> Response {
    match found {
        Some(v) => (limits(), Json(v.clone())).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn a_pr(State(h): State<Shared>, Path((id, n)): Path<(u64, u64)>) -> Response {
    let mut h = h.lock().unwrap();
    h.content_reads += 1;
    one(h.prs.get(&(id, n)))
}

async fn an_issue(State(h): State<Shared>, Path((id, n)): Path<(u64, u64)>) -> Response {
    let mut h = h.lock().unwrap();
    h.content_reads += 1;
    one(h.issues.get(&(id, n)))
}

async fn comments(State(h): State<Shared>, Path((id, n)): Path<(u64, u64)>) -> Response {
    let mut h = h.lock().unwrap();
    h.content_reads += 1;
    listed(h.comments.get(&(id, n)).cloned().unwrap_or_default())
}

async fn reviews(State(h): State<Shared>, Path((id, n)): Path<(u64, u64)>) -> Response {
    let mut h = h.lock().unwrap();
    h.content_reads += 1;
    listed(h.reviews.get(&(id, n)).cloned().unwrap_or_default())
}

async fn threads(State(h): State<Shared>, Path((id, n)): Path<(u64, u64)>) -> Response {
    let mut h = h.lock().unwrap();
    h.content_reads += 1;
    listed(h.threads.get(&(id, n)).cloned().unwrap_or_default())
}

async fn runs() -> Response {
    (limits(), Json(json!({ "total_count": 0, "check_runs": [] }))).into_response()
}

async fn statuses() -> Response {
    (limits(), Json(json!({ "state": "pending", "statuses": [] }))).into_response()
}

async fn fake_github(h: Shared) -> String {
    let r = Router::new()
        .route("/repos/{o}/{r}/installation", get(installation))
        .route("/app/installations/{id}", get(app_installation))
        .route("/app/installations/{id}/access_tokens", post(token))
        .route("/repositories/{id}", get(repo_read))
        .route("/repos/{o}/{r}", get(repo))
        .route("/repos/{o}/{r}/collaborators/{login}/permission", get(permission))
        .route("/repositories/{id}/pulls", get(pulls))
        .route("/repositories/{id}/issues", get(issues))
        .route("/repositories/{id}/pulls/{n}", get(a_pr))
        .route("/repositories/{id}/issues/{n}", get(an_issue))
        .route("/repositories/{id}/issues/{n}/comments", get(comments))
        .route("/repositories/{id}/pulls/{n}/reviews", get(reviews))
        .route("/repositories/{id}/pulls/{n}/comments", get(threads))
        .route("/repositories/{id}/commits/{sha}/check-runs", get(runs))
        .route("/repositories/{id}/commits/{sha}/status", get(statuses))
        .with_state(h);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
    base
}

fn throwaway_key() -> aws_lc_rs::rsa::KeyPair {
    let k = aws_lc_rs::rsa::KeyPair::generate(KeySize::Rsa2048).unwrap();
    let der = AsDer::<aws_lc_rs::encoding::Pkcs8V1Der>::as_der(&k).unwrap();
    aws_lc_rs::rsa::KeyPair::from_pkcs8(der.as_ref()).unwrap()
}

/// The world:
/// - team t1: alice (owner, GitHub 1001, maintains acme/app), bob (editor,
///   GitHub 1002, writes to it), carol (viewer, not signed in with
///   GitHub), dave (editor, GitHub 1004, no access to acme/app);
/// - its project has acme/app (77, private) and acme/site (88, public)
///   linked through installation 9.
struct World {
    gh: Shared,
    app: Arc<Platform>,
    base: String,
    project: String,
}

async fn world() -> World {
    let hub = Arc::new(TestHub::default());
    let gh: Shared = Arc::default();
    let api = fake_github(gh.clone()).await;
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    p.github = Some(GithubApp::new(
        "1".into(),
        "arugula-test".into(),
        super::links_tests::SECRET.into(),
        &api,
        throwaway_key(),
    ));
    let app = Arc::new(p);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });

    hub.github("alice", 1001, "alice-gh");
    hub.github("bob", 1002, "bob-gh");
    hub.github("dave", 1004, "dave-gh");
    hub.team(
        "t1",
        &[
            ("alice", TeamRole::Owner),
            ("bob", TeamRole::Editor),
            ("carol", TeamRole::Viewer),
            ("dave", TeamRole::Editor),
        ],
    );
    {
        let mut g = gh.lock().unwrap();
        g.roles.insert("alice-gh".into(), ("maintain".into(), 1001));
        g.roles.insert("bob-gh".into(), ("write".into(), 1002));
    }
    let project = crate::projects::team_project(&app, "t1").await.unwrap().unwrap();
    let i = Installation {
        id: 9,
        account_id: 500,
        login: "acme".into(),
        account_type: "Organization".into(),
        permissions: "{}".into(),
    };
    for (repo_id, name, private) in [(APP, "acme/app", true), (SITE, "acme/site", false)] {
        let l = LinkedRepo {
            repo_id,
            full_name: name.into(),
            private,
            project_id: project.clone(),
            installation_id: 9,
            linked_by: Some("alice".into()),
            linked_at: 0,
            unlinked_at: None,
            purge_after: None,
            mirror_content: false,
        };
        app.db.link_repo(&l, &i, "alice", 1).unwrap();
    }
    World { gh, app, base, project }
}

fn repository(repo: u64) -> Value {
    json!({ "id": repo, "full_name": if repo == APP { "acme/app" } else { "acme/site" } })
}

/// Every content webhook there is, for PR 5 and issue 6 of `repo`, each
/// with SECRET text.
async fn content_webhooks(w: &World, repo: u64, tag: &str) {
    let t = now_ms();
    let r = repository(repo);
    let sender = json!({ "login": "alice-gh", "id": 1001 });
    let hooks = [
        (
            "pull_request",
            json!({ "action": "opened", "repository": r, "sender": sender,
            "pull_request": pr(5, "SECRET-PR-BODY", t) }),
        ),
        (
            "issues",
            json!({ "action": "opened", "repository": r, "sender": sender,
            "issue": issue(6, "SECRET-ISSUE-BODY", t) }),
        ),
        (
            "issue_comment",
            json!({ "action": "created", "repository": r, "sender": sender,
            "issue": { "number": 5, "pull_request": {} }, "comment": comment(900, "SECRET-COMMENT", t) }),
        ),
        (
            "pull_request_review",
            json!({ "action": "submitted", "repository": r, "sender": sender,
            "pull_request": pr(5, "SECRET-PR-BODY", t),
            "review": { "id": 901, "state": "COMMENTED", "body": "SECRET-REVIEW", "user": { "login": "bob-gh", "id": 1002 },
                "submitted_at": iso(t) } }),
        ),
        (
            "pull_request_review_comment",
            json!({ "action": "created", "repository": r, "sender": sender,
            "pull_request": { "number": 5 },
            "comment": { "id": 902, "body": "SECRET-LINE", "path": "src/main.rs", "line": 3,
                "user": { "login": "bob-gh", "id": 1002 }, "created_at": iso(t), "updated_at": iso(t) } }),
        ),
    ];
    for (n, (event, body)) in hooks.iter().enumerate() {
        assert_eq!(hook(&w.base, event, &format!("{tag}-{repo}-{n}"), body).await, 202, "{event}");
    }
}

/// GitHub's side of PR 5 and issue 6 of `repo`, for the poller and reads.
fn on_github(w: &World, repo: u64) {
    let t = now_ms();
    let mut g = w.gh.lock().unwrap();
    g.prs.insert((repo, 5), pr(5, "SECRET-PR-BODY", t));
    g.issues.insert((repo, 6), issue(6, "SECRET-ISSUE-BODY", t));
    g.comments.insert((repo, 5), vec![comment(900, "SECRET-COMMENT", t)]);
    g.reviews.insert(
        (repo, 5),
        vec![json!({ "id": 901, "state": "COMMENTED", "body": "SECRET-REVIEW",
            "user": { "login": "bob-gh", "id": 1002 }, "submitted_at": iso(t) })],
    );
    g.threads.insert(
        (repo, 5),
        vec![
            json!({ "id": 902, "body": "SECRET-LINE", "path": "src/main.rs", "line": 3,
                "user": { "login": "bob-gh", "id": 1002 }, "created_at": iso(t), "updated_at": iso(t) }),
            json!({ "id": 903, "body": "SECRET-REPLY", "path": "src/main.rs", "line": 3, "in_reply_to_id": 902,
                "user": { "login": "alice-gh", "id": 1001 }, "created_at": iso(t + 1000), "updated_at": iso(t + 1000) }),
        ],
    );
}

fn count(app: &Platform, sql: &str) -> u64 {
    app.db.c().query_row(sql, params![], |r| r.get(0)).unwrap()
}

fn item_id(app: &Platform, repo: u64, number: u64) -> String {
    app.db
        .c()
        .query_row("SELECT id FROM forge_items WHERE repo_id = ?1 AND number = ?2", params![repo, number], |r| r.get(0))
        .unwrap()
}

async fn mirror(w: &World, who: &str, repo: &str, on: bool) -> (u16, Value) {
    call(
        &w.base,
        who,
        "PATCH",
        &format!("/api/projects/{}/repos/{repo}", w.project),
        Some(json!({ "mirror_content": on })),
    )
    .await
}

async fn content(w: &World, who: &str, item: &str) -> (u16, Value) {
    call(&w.base, who, "GET", &format!("/api/forge/items/{item}/content"), None).await
}

/// Every repo whose turn has come at `now`, polled: the turns.
async fn poll(app: &Platform, now: u64) -> Vec<poller::Turn> {
    let mut turns = Vec::new();
    while let Some(t) = poller::tick(app, now).await.unwrap() {
        turns.push(t);
    }
    turns
}

fn reads(w: &World) -> usize {
    w.gh.lock().unwrap().content_reads
}

/// Every log line of this test binary, from every thread, at every level:
/// the platform's tests write `SECRET` text through webhooks, the poller
/// and the routes, and none of it may be logged.
fn captured() -> Arc<Mutex<Vec<u8>>> {
    static LOGS: std::sync::OnceLock<Arc<Mutex<Vec<u8>>>> = std::sync::OnceLock::new();
    LOGS.get_or_init(|| {
        #[derive(Clone)]
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Sink {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let logs = Arc::new(Mutex::new(Vec::new()));
        let sink = Sink(logs.clone());
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || sink.clone())
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("no other test sets a global subscriber");
        logs
    })
    .clone()
}

/// Without the opt-in, webhooks and a poll that carry text leave no
/// content row: the text is fetched on read, and not kept in the
/// database.
#[tokio::test]
async fn no_opt_in_no_content_rows() {
    let w = world().await;
    on_github(&w, APP);
    content_webhooks(&w, APP, "a").await;
    assert_eq!(poll(&w.app, now_ms()).await.len(), 2);
    assert_eq!(count(&w.app, "SELECT count(*) FROM forge_items WHERE repo_id = 77"), 2);
    assert_eq!(count(&w.app, "SELECT count(*) FROM forge_content"), 0);
    assert!(w.app.db.mentions("SECRET").is_empty(), "{:?}", w.app.db.mentions("SECRET"));

    // Read: from GitHub, with a read token minted for the reader.
    let (s, v) = content(&w, "alice", &item_id(&w.app, APP, 5)).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!((v["from"].as_str(), v["body"]["body"].as_str()), (Some("github"), Some("SECRET-PR-BODY")));
    let kinds: Vec<&str> = v["comments"].as_array().unwrap().iter().map(|c| c["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["comment", "review", "review_comment", "review_comment"], "{v}");
    let reply = &v["comments"][3];
    assert_eq!(
        (reply["reply_to"].as_u64(), reply["path"].as_str(), reply["line"].as_u64()),
        (Some(902), Some("src/main.rs"), Some(3))
    );
    let (s, v) = content(&w, "bob", &item_id(&w.app, APP, 6)).await;
    assert_eq!(
        (s, v["body"]["body"].as_str(), v["comments"].as_array().map(Vec::len)),
        (200, Some("SECRET-ISSUE-BODY"), Some(0))
    );
    assert_eq!(count(&w.app, "SELECT count(*) FROM forge_content"), 0, "a read keeps nothing in the database");
    assert!(w.app.db.mentions("SECRET").is_empty());
}

/// Opted in, webhooks and the poller write content rows and reads come
/// from them; opting out deletes every one at once. Who may switch it:
/// on, a member who maintains the repo on GitHub; off, any member who
/// may change links. And no text reaches the logs on the way.
#[tokio::test]
async fn opt_in_then_out() {
    let logs = captured();
    let w = world().await;
    on_github(&w, APP);
    // The poller's first turn, before the opt-in: metadata only.
    assert_eq!(poll(&w.app, now_ms()).await.len(), 2);
    assert_eq!(count(&w.app, "SELECT count(*) FROM forge_content"), 0);

    let (s, v) = mirror(&w, "carol", "acme/app", true).await;
    assert_eq!(s, 403, "a viewer: {v}");
    let (s, v) = mirror(&w, "bob", "acme/app", true).await;
    assert_eq!((s, v["reason"].as_str()), (403, Some("github_role")), "write isn't maintain: {v}");
    let (s, v) = mirror(&w, "alice", "acme/nope", true).await;
    assert_eq!(s, 404, "{v}");
    let (s, v) = mirror(&w, "alice", "acme/app", true).await;
    assert_eq!((s, v["mirror_content"].as_bool()), (200, Some(true)), "{v}");

    // The next turn backfills, content and all.
    let turns = poll(&w.app, now_ms()).await;
    assert!(matches!(turns[..], [poller::Turn::Done { repo: APP, backfill: true, .. }]), "{turns:?}");
    assert_eq!(count(&w.app, "SELECT count(*) FROM forge_content WHERE repo_id = 77 AND number = 5"), 5);
    assert_eq!(count(&w.app, "SELECT count(*) FROM forge_content WHERE repo_id = 77 AND number = 6"), 1);
    let mints: u64 = count(&w.app, "SELECT count(*) FROM github_token_mints WHERE purpose = 'ingest'");
    assert!(mints >= 1);

    // Reads come from the mirror: GitHub isn't asked.
    let before = reads(&w);
    let (s, v) = content(&w, "alice", &item_id(&w.app, APP, 5)).await;
    assert_eq!((s, v["from"].as_str(), v["comments"].as_array().map(Vec::len)), (200, Some("mirror"), Some(4)), "{v}");
    assert_eq!(reads(&w), before);

    // Webhooks keep it current: an edit, a new comment, a deletion.
    let t = now_ms() + 2000;
    let r = repository(APP);
    let edited =
        json!({ "action": "edited", "repository": r, "sender": {}, "pull_request": pr(5, "SECRET-EDITED", t) });
    assert_eq!(hook(&w.base, "pull_request", "e1", &edited).await, 202);
    let new = json!({ "action": "created", "repository": r, "issue": { "number": 5, "pull_request": {} },
        "comment": comment(910, "SECRET-NEW", t) });
    assert_eq!(hook(&w.base, "issue_comment", "e2", &new).await, 202);
    let gone = json!({ "action": "deleted", "repository": r, "issue": { "number": 5, "pull_request": {} },
        "comment": comment(900, "SECRET-COMMENT", t) });
    assert_eq!(hook(&w.base, "issue_comment", "e3", &gone).await, 202);
    let (_, v) = content(&w, "bob", &item_id(&w.app, APP, 5)).await;
    assert_eq!(v["body"]["body"].as_str(), Some("SECRET-EDITED"), "{v}");
    let bodies: Vec<&str> = v["comments"].as_array().unwrap().iter().map(|c| c["body"].as_str().unwrap()).collect();
    assert!(bodies.contains(&"SECRET-NEW") && !bodies.contains(&"SECRET-COMMENT"), "{bodies:?}");
    // And one opened since: whole from its webhook.
    let opened = json!({ "action": "opened", "repository": r, "sender": {}, "pull_request": pr(7, "SECRET-SEVEN", t) });
    assert_eq!(hook(&w.base, "pull_request", "e4", &opened).await, 202);
    let before = reads(&w);
    let (_, v) = content(&w, "alice", &item_id(&w.app, APP, 7)).await;
    assert_eq!((v["from"].as_str(), v["body"]["body"].as_str()), (Some("mirror"), Some("SECRET-SEVEN")), "{v}");
    assert_eq!(reads(&w), before);

    // The other repo isn't mirrored: its webhooks keep nothing.
    content_webhooks(&w, SITE, "c").await;
    assert_eq!(count(&w.app, "SELECT count(*) FROM forge_content WHERE repo_id = 88"), 0);

    // Off: any member who may change links; every row goes at once.
    let (s, v) = mirror(&w, "carol", "acme/app", false).await;
    assert_eq!(s, 403, "{v}");
    let (s, v) = mirror(&w, "bob", "acme/app", false).await;
    assert_eq!((s, v["mirror_content"].as_bool()), (200, Some(false)), "{v}");
    assert_eq!(count(&w.app, "SELECT count(*) FROM forge_content"), 0);
    assert!(w.app.db.mentions("SECRET").is_empty(), "{:?}", w.app.db.mentions("SECRET"));
    // Webhooks and polls after it keep nothing either.
    content_webhooks(&w, APP, "d").await;
    let every = w.app.github.as_ref().unwrap().ingest.settings.every_ms;
    assert_eq!(poll(&w.app, now_ms() + every).await.len(), 2);
    assert_eq!(count(&w.app, "SELECT count(*) FROM forge_content"), 0);

    let log = String::from_utf8_lossy(&logs.lock().unwrap()).into_owned();
    assert!(log.contains("a linked repo's content mirroring"), "the logs were captured: {log}");
    assert!(!log.contains("SECRET"), "{log}");
}

/// A reader the viewer check refuses gets a 404, and GitHub is never
/// asked, whether or not the content is kept in memory.
#[tokio::test]
async fn a_refused_reader_costs_github_nothing() {
    let w = world().await;
    on_github(&w, APP);
    content_webhooks(&w, APP, "a").await;
    let item = item_id(&w.app, APP, 5);
    for who in ["dave", "carol", "mallory"] {
        let (s, v) = content(&w, who, &item).await;
        assert_eq!(s, 404, "{who}: {v}");
    }
    assert_eq!(reads(&w), 0);
    assert!(!w.app.github.as_ref().unwrap().bodies.has(APP, 5));
    // Kept now, for alice: still nothing for the others.
    assert_eq!(content(&w, "alice", &item).await.0, 200);
    let after = reads(&w);
    for who in ["dave", "carol", "mallory"] {
        let (s, v) = content(&w, who, &item).await;
        assert_eq!(s, 404, "{who}");
        assert!(!v.to_string().contains("SECRET"), "{v}");
    }
    assert_eq!(reads(&w), after);
}

/// A second read is the cache's; a webhook for the item drops it on this
/// machine, and a bump of its rev (another machine's webhook) on every
/// machine.
#[tokio::test]
async fn the_cache_and_what_drops_it() {
    let w = world().await;
    on_github(&w, APP);
    content_webhooks(&w, APP, "a").await;
    let item = item_id(&w.app, APP, 5);
    assert_eq!(content(&w, "alice", &item).await.0, 200);
    let first = reads(&w);
    assert_eq!(first, 4, "the PR, its comments, reviews and review comments");
    let (s, v) = content(&w, "bob", &item).await;
    assert_eq!((s, v["from"].as_str()), (200, Some("github")));
    assert_eq!(reads(&w), first, "kept");

    // A comment's webhook: read again, with it.
    let t = now_ms() + 1000;
    w.gh.lock().unwrap().comments.get_mut(&(APP, 5)).unwrap().push(comment(920, "added", t));
    let c = json!({ "action": "created", "repository": repository(APP), "issue": { "number": 5, "pull_request": {} },
        "comment": comment(920, "added", t) });
    assert_eq!(hook(&w.base, "issue_comment", "c1", &c).await, 202);
    assert!(!w.app.github.as_ref().unwrap().bodies.has(APP, 5));
    let (_, v) = content(&w, "alice", &item).await;
    assert!(v.to_string().contains("added"), "{v}");
    assert_eq!(reads(&w), first * 2);
    assert_eq!(content(&w, "alice", &item).await.0, 200);
    assert_eq!(reads(&w), first * 2, "kept again");

    // Another machine took a webhook: only the rev says so here.
    w.app.db.c().execute("UPDATE forge_items SET content_rev = content_rev + 1 WHERE id = ?1", params![item]).unwrap();
    assert!(w.app.github.as_ref().unwrap().bodies.has(APP, 5));
    assert_eq!(content(&w, "alice", &item).await.0, 200);
    assert_eq!(reads(&w), first * 3);
}

/// Search finds a body's text only in a repo that mirrors content, and
/// only for a reader who may read that repo.
#[tokio::test]
async fn search_finds_mirrored_text_for_readers_only() {
    let w = world().await;
    assert_eq!(mirror(&w, "alice", "acme/app", true).await.0, 200);
    content_webhooks(&w, APP, "a").await;
    content_webhooks(&w, SITE, "b").await;
    // Rows of an unmirrored repo (as if left behind) are never searched.
    w.app
        .db
        .c()
        .execute(
            "INSERT INTO forge_content (repo_id, number, kind, github_id, body, updated_at)
             VALUES (88, 5, 'body', 1, 'SECRET-PR-BODY', 1)",
            params![],
        )
        .unwrap();
    let search = |who: &'static str, q: &'static str| {
        let (base, project) = (w.base.clone(), w.project.clone());
        async move {
            let (s, v) = call(&base, who, "GET", &format!("/api/projects/{project}/items?q={q}"), None).await;
            assert_eq!(s, 200, "{v}");
            let mut found: Vec<(String, u64)> = v["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| (i["repo"].as_str().unwrap().to_owned(), i["number"].as_u64().unwrap()))
                .collect();
            found.sort();
            found
        }
    };
    assert_eq!(search("alice", "secret-pr").await, [("acme/app".to_owned(), 5)]);
    assert_eq!(search("alice", "SECRET-LINE").await, [("acme/app".to_owned(), 5)], "a review comment");
    assert_eq!(search("alice", "issue-body").await, [("acme/app".to_owned(), 6)]);
    assert_eq!(search("bob", "SECRET-REVIEW").await, [("acme/app".to_owned(), 5)]);
    // dave may read acme/site but not acme/app.
    assert!(search("dave", "SECRET").await.is_empty());
    assert!(search("carol", "SECRET").await.is_empty());
    // Titles as before, in both repos.
    assert_eq!(search("dave", "PR%205").await, [("acme/site".to_owned(), 5)]);
    // Off: the text is gone, and so is what search found.
    assert_eq!(mirror(&w, "alice", "acme/app", false).await.0, 200);
    assert!(search("alice", "SECRET").await.is_empty());
}
