//! Linking repos to projects (M163) through the platform's own routes,
//! against a fake GitHub: who may link, installing first, an uninstall
//! unlinking, and the purge. Runs on SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL` (`just test-platform-postgres`), Postgres.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use arugula_e2e::team::TeamRole;
use aws_lc_rs::{encoding::AsDer, rsa::KeySize};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

use super::{GithubApp, links};
use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::{
        Sql as _,
        forge::PURGE_GRACE_MS,
        sql::{Param, params},
    },
};

pub(crate) const SECRET: &str = "whsec-test";

/// What the fake GitHub knows: where the App is installed (repo →
/// installation, account id, login), each repo's id and privacy, and
/// each person's role on a repo (repo, login → role, user id).
#[derive(Default)]
pub(crate) struct Gh {
    pub(crate) installed: BTreeMap<String, (u64, u64, String)>,
    pub(crate) repos: BTreeMap<String, (u64, bool)>,
    pub(crate) roles: BTreeMap<(String, String), (String, u64)>,
    /// How many times a person's permission was asked.
    pub(crate) asked: usize,
    /// User tokens, and whose GitHub id each is (M168).
    pub(super) users: BTreeMap<String, u64>,
    /// What was posted as a person: the path, the `Authorization` header
    /// and the body.
    pub(super) posted: Vec<(String, String, Value)>,
    /// A status every post is refused with, while set.
    pub(super) refuse: Option<u16>,
    /// What an installation grants the App, by repo, when not the real
    /// App's today ([`granted`]).
    pub(crate) grants: BTreeMap<String, Value>,
    /// Commits with a `ci/green/<sha>` tag, a `ci/revoked/<sha>` one, and
    /// those on main (M221's release checks them).
    pub(crate) green: std::collections::BTreeSet<String>,
    pub(crate) revoked: std::collections::BTreeSet<String>,
    pub(crate) on_main: std::collections::BTreeSet<String>,
    /// Logins whose installations are a user's, not an organization's
    /// (M232).
    pub(crate) user_accounts: std::collections::BTreeSet<String>,
    /// Files on a repo's default branch, by `owner/name:path` (M259's PR
    /// template).
    pub(crate) files: BTreeMap<String, String>,
    /// The permissions of each installation token minted, in order.
    pub(crate) minted: Vec<Value>,
}

/// What the platform's real App (`arugula-io`) is granted today: it reads
/// contents and writes issues and pull requests (to send).
pub(crate) fn granted() -> Value {
    let mut g = read_grant();
    g["issues"] = "write".into();
    g["pull_requests"] = "write".into();
    g
}

/// An installation that reads everything the App reads, and writes
/// nothing.
pub(super) fn read_grant() -> Value {
    json!({
        "metadata": "read", "contents": "read", "pull_requests": "read", "issues": "read",
        "checks": "read", "statuses": "read", "actions": "read",
    })
}

/// Whether a mint `asked` for more than an installation `grants`, as
/// GitHub refuses one (422 "The permissions requested are not granted to
/// this installation.").
pub(super) fn exceeds(asked: &Value, grants: &Value) -> bool {
    let rank = |v: &Value| match v.as_str() {
        Some("read") => 1,
        Some("write") => 2,
        Some("admin") => 3,
        _ => 0,
    };
    asked.as_object().is_none_or(|a| a.iter().any(|(k, v)| rank(v) > rank(&grants[k])))
}

/// GitHub's answer to a mint that asks for more than it's granted.
pub(super) fn not_granted() -> Response {
    let m = "The permissions requested are not granted to this installation.";
    (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "message": m }))).into_response()
}

pub(crate) type Shared = Arc<Mutex<Gh>>;

async fn installation(State(h): State<Shared>, Path((o, r)): Path<(String, String)>) -> Response {
    let h = h.lock().unwrap();
    let repo = format!("{o}/{r}");
    match h.installed.get(&repo) {
        Some((id, account, login)) => Json(json!({
            "id": id,
            "account": {
                "id": account, "login": login,
                "type": if h.user_accounts.contains(login) { "User" } else { "Organization" },
            },
            "permissions": h.grants.get(&repo).cloned().unwrap_or_else(granted),
        }))
        .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn token(State(h): State<Shared>, Json(b): Json<Value>) -> Response {
    // For one repository, no more than its installation grants. GitHub's
    // answer names the repository it covers, as M167 checks.
    assert_eq!(b["permissions"]["metadata"], "read");
    let mut h = h.lock().unwrap();
    h.minted.push(b["permissions"].clone());
    // By GitHub's id (a deploy's own repository), or by its short name.
    let by_id = b["repository_ids"][0].as_u64().and_then(|id| h.repos.iter().find(|(_, (i, _))| *i == id));
    if let Some((name, (id, _))) = by_id {
        assert_eq!(b["repository_ids"].as_array().map(Vec::len), Some(1));
        if exceeds(&b["permissions"], &h.grants.get(name).cloned().unwrap_or_else(granted)) {
            return not_granted();
        }
        let covers = json!([{ "id": id, "full_name": name }]);
        return Json(json!({ "token": "ghs_fake", "expires_at": "2099-01-01T00:00:00Z", "repositories": covers }))
            .into_response();
    }
    assert_eq!(b["repositories"].as_array().map(Vec::len), Some(1));
    let short = b["repositories"][0].as_str().unwrap();
    let grants = h.grants.iter().find(|(name, _)| name.rsplit('/').next() == Some(short)).map(|(_, g)| g.clone());
    if exceeds(&b["permissions"], &grants.unwrap_or_else(granted)) {
        return not_granted();
    }
    // A repo the test made, or else one the App is installed on (id 0: a
    // test that only installs the App doesn't say the repo's id).
    let mut covers: Vec<Value> = h
        .repos
        .iter()
        .filter(|(name, _)| name.rsplit('/').next() == Some(short))
        .map(|(name, (id, _))| json!({ "id": id, "full_name": name }))
        .collect();
    if covers.is_empty() {
        covers = h
            .installed
            .keys()
            .filter(|name| name.rsplit('/').next() == Some(short))
            .map(|name| json!({ "id": 0, "full_name": name }))
            .collect();
    }
    Json(json!({ "token": "ghs_fake", "expires_at": "2099-01-01T00:00:00Z", "repositories": covers })).into_response()
}

/// `ci/green/<sha>` or `ci/revoked/<sha>`, as GitHub answers a ref: 200
/// with it, or 404.
async fn ci_ref(State(h): State<Shared>, Path((_, _, kind, sha)): Path<(String, String, String, String)>) -> Response {
    let h = h.lock().unwrap();
    let has = match kind.as_str() {
        "green" => h.green.contains(&sha),
        "revoked" => h.revoked.contains(&sha),
        _ => false,
    };
    if has {
        Json(json!({ "ref": format!("refs/tags/ci/{kind}/{sha}"), "object": { "type": "tag" } })).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({ "message": "Not Found", "status": "404" }))).into_response()
    }
}

/// `compare/{sha}...main`, as GitHub answered for real commits: `ahead`
/// with `behind_by` 0 for one on main, `diverged` for a branch's.
async fn compare(
    State(h): State<Shared>,
    Path((o, r, spec)): Path<(String, String, String)>,
    headers: axum::http::HeaderMap,
) -> Response {
    // A diff (M264's review): the file kept as `owner/name:compare:<spec>`.
    if headers.get("accept").and_then(|v| v.to_str().ok()) == Some("application/vnd.github.diff") {
        return match h.lock().unwrap().files.get(&format!("{o}/{r}:compare:{spec}")) {
            Some(diff) => diff.clone().into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        };
    }
    // A session's branch (M259): its commits, oldest first, as GitHub
    // lists them, and the files it changes (one a line, kept as
    // `owner/name:compare-files`).
    if spec.contains("...arugula/") {
        let files: Vec<Value> = h
            .lock()
            .unwrap()
            .files
            .get(&format!("{o}/{r}:compare-files"))
            .map(|f| f.lines().map(|f| json!({ "filename": f, "status": "modified" })).collect())
            .unwrap_or_default();
        return Json(json!({ "status": "ahead", "ahead_by": 2, "commits": [
            { "sha": "a1a1a1a1a1", "commit": { "message": "Add farewell(name)\n\nNext to greet." } },
            { "sha": "b2b2b2b2b2", "commit": { "message": "Add a test for farewell" } },
        ], "files": files }))
        .into_response();
    }
    let sha = spec.split("...").next().unwrap_or_default();
    if h.lock().unwrap().on_main.contains(sha) {
        Json(json!({ "ahead_by": 3, "behind_by": 0, "status": "ahead" })).into_response()
    } else {
        Json(json!({ "ahead_by": 3, "behind_by": 1, "status": "diverged" })).into_response()
    }
}

async fn repo(State(h): State<Shared>, Path((o, r)): Path<(String, String)>) -> Response {
    let name = format!("{o}/{r}");
    match h.lock().unwrap().repos.get(&name) {
        Some((id, private)) => {
            Json(json!({ "id": id, "full_name": name, "private": private, "default_branch": "main" })).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn permission(State(h): State<Shared>, Path((o, r, login)): Path<(String, String, String)>) -> Response {
    let mut h = h.lock().unwrap();
    h.asked += 1;
    let Some((role, id)) = h.roles.get(&(format!("{o}/{r}"), login.clone())) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let permission = match role.as_str() {
        "maintain" | "write" | "triage" => "write",
        r => r,
    };
    Json(json!({ "permission": permission, "role_name": role, "user": { "id": id, "login": login } })).into_response()
}

fn bearer(headers: &axum::http::HeaderMap) -> String {
    headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned()
}

async fn user(State(h): State<Shared>, headers: axum::http::HeaderMap) -> Response {
    let token = bearer(&headers).trim_start_matches("Bearer ").to_owned();
    match h.lock().unwrap().users.get(&token) {
        Some(id) => Json(json!({ "id": id, "login": format!("user-{id}") })).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

/// A comment, a review or an issue, as the token's person.
async fn post_as(
    State(h): State<Shared>,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    Json(b): Json<Value>,
) -> Response {
    let mut h = h.lock().unwrap();
    let path = uri.path().to_owned();
    h.posted.push((path.clone(), bearer(&headers), b));
    if let Some(s) = h.refuse {
        let status = StatusCode::from_u16(s).unwrap();
        return (status, Json(json!({ "message": "Resource not accessible by integration" }))).into_response();
    }
    Json(json!({ "number": 12, "html_url": format!("https://github.com{path}#posted") })).into_response()
}

/// A file on a repo's default branch, as GitHub's contents API answers.
async fn contents(State(h): State<Shared>, Path((o, r, path)): Path<(String, String, String)>) -> Response {
    use base64::Engine as _;
    match h.lock().unwrap().files.get(&format!("{o}/{r}:{path}")) {
        Some(text) => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(text);
            Json(json!({ "type": "file", "path": path, "encoding": "base64", "content": b64 })).into_response()
        }
        None => (StatusCode::NOT_FOUND, Json(json!({ "message": "Not Found" }))).into_response(),
    }
}

pub(crate) async fn fake_github(h: Shared) -> String {
    let r = Router::new()
        .route("/user", get(user))
        .route("/repos/{o}/{r}/issues/{n}/comments", post(post_as))
        .route("/repos/{o}/{r}/pulls/{n}/reviews", post(post_as))
        .route("/repos/{o}/{r}/issues", post(post_as))
        .route("/repos/{o}/{r}/pulls", post(post_as))
        .route("/repos/{o}/{r}/contents/{*path}", get(contents))
        .route("/repos/{o}/{r}/installation", get(installation))
        .route("/app/installations/{id}/access_tokens", post(token))
        .route("/repos/{o}/{r}", get(repo))
        .route("/repos/{o}/{r}/collaborators/{login}/permission", get(permission))
        .route("/repos/{o}/{r}/git/ref/tags/ci/{kind}/{sha}", get(ci_ref))
        .route("/repos/{o}/{r}/compare/{*spec}", get(compare))
        .with_state(h);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
    base
}

pub(crate) fn throwaway_key() -> aws_lc_rs::rsa::KeyPair {
    let k = aws_lc_rs::rsa::KeyPair::generate(KeySize::Rsa2048).unwrap();
    let der = AsDer::<aws_lc_rs::encoding::Pkcs8V1Der>::as_der(&k).unwrap();
    aws_lc_rs::rsa::KeyPair::from_pkcs8(der.as_ref()).unwrap()
}

/// The platform with the App, over `hub`, against a fake GitHub: its
/// state, the platform, and the platform's address.
pub(super) async fn serve(hub: &Arc<TestHub>) -> (Shared, Arc<Platform>, String) {
    let gh: Shared = Arc::default();
    let api = fake_github(gh.clone()).await;
    let cfg = crate::Config {
        public_url: "https://control.example".into(),
        page_url: "https://app.example".into(),
        ..Default::default()
    };
    let mut p = Platform::new(cfg, crate::db::Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    p.github = Some(GithubApp::new("1".into(), "arugula-test".into(), SECRET.into(), &api, throwaway_key()));
    let app = Arc::new(p);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (gh, app, base)
}

/// `acme/app` (repo 77, private), with the App installed on acme
/// (installation 9), and alice (GitHub 1001) its maintainer.
fn acme(gh: &Shared, hub: &TestHub) {
    let mut g = gh.lock().unwrap();
    g.installed.insert("acme/app".into(), (9, 500, "acme".into()));
    g.repos.insert("acme/app".into(), (77, true));
    g.roles.insert(("acme/app".into(), "alice-gh".into()), ("maintain".into(), 1001));
    hub.github("alice", 1001, "alice-gh");
}

async fn link(base: &str, who: &str, project: &str, repo: &str) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/projects/{project}/repos"), Some(json!({ "repo": repo }))).await
}

pub(super) async fn hook(base: &str, event: &str, delivery: &str, body: &Value) -> u16 {
    use hmac::{KeyInit, Mac};
    let body = body.to_string();
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(SECRET.as_bytes()).unwrap();
    mac.update(body.as_bytes());
    let sig = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
    let r = reqwest::Client::new()
        .post(format!("{base}/github/webhook"))
        .header("x-github-event", event)
        .header("x-github-delivery", delivery)
        .header("x-hub-signature-256", sig)
        .body(body)
        .send()
        .await
        .unwrap();
    r.status().as_u16()
}

fn count(app: &Platform, table: &str, filter: &str, p: &[&dyn Param]) -> u64 {
    app.db.c().query_row(&format!("SELECT count(*) FROM {table} WHERE {filter}"), p, |r| r.get(0)).unwrap()
}

/// Rows of every kind the mirror keeps, for a repo: an item, its check,
/// event, content, a draft on it, and an inbox row.
fn mirror_rows(app: &Platform, repo: u64, project: &str) {
    let item = format!("fi_{repo}");
    let c = app.db.c();
    c.execute(
        "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
         VALUES (?1, ?2, 1, 'pr', 'open', 'a title', 1, 1, 1)",
        params![item, repo],
    )
    .unwrap();
    c.execute(
        "INSERT INTO forge_checks (repo_id, head_sha, kind, name, status, updated_at) VALUES (?1, 'abc', 'status', 'ci', 'ok', 1)",
        params![repo],
    )
    .unwrap();
    c.execute(
        "INSERT INTO forge_events (id, repo_id, number, kind, at) VALUES (?1, ?2, 1, 'review', 1)",
        params![format!("fev_{repo}"), repo],
    )
    .unwrap();
    c.execute(
        "INSERT INTO forge_content (repo_id, number, kind, github_id, body, updated_at) VALUES (?1, 1, 'body', 5, 'b', 1)",
        params![repo],
    )
    .unwrap();
    c.execute(
        "INSERT INTO drafts (id, project_id, author, repo_id, number, kind, body, created_at, updated_at)
         VALUES (?1, ?2, 'alice', ?3, 1, 'comment', 'lgtm', 1, 1)",
        params![format!("dft_{repo}"), project, repo],
    )
    .unwrap();
    c.execute(
        "INSERT INTO inbox_items (account, item, reason, created_at, updated_at) VALUES ('alice', ?1, 'review_requested', 1, 1)",
        params![item],
    )
    .unwrap();
}

/// How many rows of each kind the repo has left, its link last.
fn left(app: &Platform, repo: u64) -> Vec<u64> {
    let by_repo = |t| count(app, t, "repo_id = ?1", params![repo]);
    vec![
        by_repo("forge_items"),
        by_repo("forge_checks"),
        by_repo("forge_events"),
        by_repo("forge_content"),
        by_repo("drafts"),
        count(app, "inbox_items", "item = ?1", params![format!("fi_{repo}")]),
        by_repo("linked_repos"),
    ]
}

/// A maintainer links a repo to their own project, and sees it listed;
/// someone with write or read on it, someone who isn't in the project, a
/// team's viewer, and an account not signed in with GitHub are refused.
#[tokio::test]
async fn a_maintainer_links_and_others_are_refused() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    acme(&gh, &hub);
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("carol", TeamRole::Viewer)]);
    {
        let mut g = gh.lock().unwrap();
        g.roles.insert(("acme/app".into(), "bob-gh".into()), ("write".into(), 1002));
        g.roles.insert(("acme/app".into(), "carol-gh".into()), ("admin".into(), 1003));
        g.roles.insert(("acme/app".into(), "dave-gh".into()), ("admin".into(), 1004));
    }
    hub.github("bob", 1002, "bob-gh");
    hub.github("carol", 1003, "carol-gh");
    hub.github("dave", 1004, "dave-gh");

    let (s, v) = link(&base, "alice", "personal", "acme/app").await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(
        (v["repo_id"].as_u64(), v["full_name"].as_str(), v["private"].as_bool()),
        (Some(77), Some("acme/app"), Some(true))
    );
    assert_eq!(v["linked_by"], "alice");
    let (s, listed) = call(&base, "alice", "GET", "/api/projects/personal/repos", None).await;
    assert_eq!(s, 200);
    let mine = listed["project"].as_str().unwrap().to_owned();
    assert!(mine.starts_with("prj_"), "{listed}");
    assert_eq!(listed["repos"][0]["full_name"], "acme/app");
    assert_eq!(listed["repos"][0]["unlinked_at"], Value::Null);
    assert_eq!(count(&app, "github_installations", "installation_id = 9", params![]), 1);
    // The role was checked with a read token, minted for alice and logged
    // (M167), never the token itself.
    assert_eq!(
        count(&app, "github_token_mints", "caller = 'alice' AND purpose = 'read' AND installation_id = 9", params![]),
        1
    );
    assert!(app.db.mentions("ghs_fake").is_empty());

    // Someone else's project is no project to them, to list or link.
    hub.github("eve", 1005, "eve-gh");
    let (s, _) = call(&base, "eve", "GET", &format!("/api/projects/{mine}/repos"), None).await;
    assert_eq!(s, 404);
    let (s, _) = link(&base, "eve", &mine, "acme/app").await;
    assert_eq!(s, 404);

    // A team's project: write on GitHub isn't enough, nor is a viewer's
    // place in the team, admin or not.
    let team = crate::projects::team_project(&app, "t1").await.unwrap().unwrap();
    let (s, v) = link(&base, "bob", &team, "acme/app").await;
    assert_eq!((s, v["reason"].as_str()), (403, Some("github_role")), "{v}");
    assert!(v["error"].as_str().unwrap().contains("bob-gh has write"), "{v}");
    gh.lock().unwrap().roles.insert(("acme/app".into(), "bob-gh".into()), ("read".into(), 1002));
    app.github.as_ref().unwrap().forget();
    let (s, _) = link(&base, "bob", &team, "acme/app").await;
    assert_eq!(s, 403);
    let (s, v) = link(&base, "carol", &team, "acme/app").await;
    assert_eq!(s, 403, "{v}");
    assert!(v["error"].as_str().unwrap().contains("viewer"), "{v}");
    let (s, listed) = call(&base, "carol", "GET", &format!("/api/projects/{team}/repos"), None).await;
    assert_eq!((s, listed["repos"].as_array().map(Vec::len)), (200, Some(0)), "a viewer lists");
    // Someone outside the team, admin on GitHub or not.
    let (s, _) = link(&base, "dave", &team, "acme/app").await;
    assert_eq!(s, 404);

    // A GitHub login that changed hands is refused, by id.
    gh.lock().unwrap().roles.insert(("acme/app".into(), "bob-gh".into()), ("admin".into(), 4242));
    app.github.as_ref().unwrap().forget();
    let (s, v) = link(&base, "bob", &team, "acme/app").await;
    assert_eq!(s, 403);
    assert!(v["error"].as_str().unwrap().contains("isn't yours any more"), "{v}");

    // Not signed in with GitHub.
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("frank", TeamRole::Editor)]);
    let (s, v) = link(&base, "frank", &team, "acme/app").await;
    assert_eq!(s, 403);
    assert!(v["error"].as_str().unwrap().contains("sign in to control with GitHub"), "{v}");

    // Already linked to alice's own project: not to the team's as well.
    let (s, v) = link(&base, "alice", &team, "acme/app").await;
    assert_eq!(s, 409, "{v}");
    assert_eq!(left(&app, 77)[6], 1);
}

/// Where the App isn't installed, the answer is GitHub's page to install
/// it, with a state that brings them back to the project; once it's
/// installed, the same ask links.
#[tokio::test]
async fn install_first() {
    let hub = Arc::new(TestHub::default());
    let (gh, _app, base) = serve(&hub).await;
    acme(&gh, &hub);
    gh.lock().unwrap().installed.clear();
    let (s, v) = link(&base, "alice", "personal", "acme/app").await;
    assert_eq!((s, v["reason"].as_str()), (409, Some("install_app")), "{v}");
    let url = v["install_url"].as_str().unwrap();
    assert!(url.starts_with("https://github.com/apps/arugula-test/installations/new?state=prj_"), "{url}");
    assert!(url.ends_with("%3Aacme%2Fapp"), "{url}");
    gh.lock().unwrap().installed.insert("acme/app".into(), (9, 500, "acme".into()));
    let (s, v) = link(&base, "alice", "personal", "acme/app").await;
    assert_eq!(s, 200, "{v}");
}

/// An uninstall unlinks the installation's repos at once, and the purge
/// leaves none of their rows; another installation's repo keeps its own.
/// A repo taken out of an installation is unlinked too.
#[tokio::test]
async fn an_uninstall_unlinks_and_the_purge_leaves_nothing() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    acme(&gh, &hub);
    {
        let mut g = gh.lock().unwrap();
        g.installed.insert("alice-gh/dots".into(), (10, 1001, "alice-gh".into()));
        g.repos.insert("alice-gh/dots".into(), (88, false));
        g.roles.insert(("alice-gh/dots".into(), "alice-gh".into()), ("admin".into(), 1001));
        g.installed.insert("acme/web".into(), (9, 500, "acme".into()));
        g.repos.insert("acme/web".into(), (99, false));
        g.roles.insert(("acme/web".into(), "alice-gh".into()), ("admin".into(), 1001));
    }
    for r in ["acme/app", "alice-gh/dots", "acme/web"] {
        assert_eq!(link(&base, "alice", "personal", r).await.0, 200);
    }
    let project = app.db.linked_repo(77).unwrap().unwrap().project_id;
    for r in [77, 88, 99] {
        mirror_rows(&app, r, &project);
    }

    // acme/web is taken out of the installation.
    let removed = json!({ "action": "removed", "installation": { "id": 9, "account": { "id": 500, "login": "acme", "type": "Organization" } },
        "repositories_removed": [{ "id": 99, "full_name": "acme/web" }] });
    assert_eq!(hook(&base, "installation_repositories", "d1", &removed).await, 202);
    assert!(app.db.linked_repo(99).unwrap().unwrap().unlinked_at.is_some());
    assert!(app.db.linked_repo(77).unwrap().unwrap().unlinked_at.is_none(), "the rest of the installation stays");
    // The uninstall below must come at least a millisecond later, or both
    // purges are due together (seen on a fast macOS runner).
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    // acme uninstalls the App.
    let deleted = json!({ "action": "deleted", "installation": { "id": 9, "account": { "id": 500, "login": "acme", "type": "Organization" } } });
    assert_eq!(hook(&base, "installation", "d2", &deleted).await, 202);
    let gone = app.db.linked_repo(77).unwrap().unwrap();
    let now = gone.unlinked_at.expect("unlinked at once");
    assert_eq!(gone.purge_after, Some(now + PURGE_GRACE_MS));
    assert!(count(&app, "github_installations", "installation_id = 9 AND deleted_at IS NOT NULL", params![]) == 1);
    assert!(app.db.linked_repo(88).unwrap().unwrap().unlinked_at.is_none(), "another installation's");
    let (_, listed) = call(&base, "alice", "GET", "/api/projects/personal/repos", None).await;
    assert_eq!(listed["repos"].as_array().unwrap().len(), 3, "listed until the purge");

    // Not before it's due (acme/web was taken out first); then nothing
    // of them.
    assert_eq!(links::purge(&app, now + PURGE_GRACE_MS - 1).unwrap(), 1);
    assert_eq!(left(&app, 99), [0; 7]);
    assert_eq!(left(&app, 77), [1; 7], "not due yet");
    assert_eq!(links::purge(&app, now + PURGE_GRACE_MS).unwrap(), 1);
    for r in [77, 99] {
        assert_eq!(left(&app, r), [0; 7], "repo {r}");
    }
    assert_eq!(left(&app, 88), [1; 7], "a linked repo keeps its rows");
    assert_eq!(links::purge(&app, now + 2 * PURGE_GRACE_MS).unwrap(), 0);
}

/// Unlinking stops ingest at once and waits for the purge; linking it
/// again first cancels the purge and keeps its rows. Linking it to
/// another project instead takes the old project's rows now.
#[tokio::test]
async fn linking_again_cancels_the_purge() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    acme(&gh, &hub);
    hub.team("t1", &[("alice", TeamRole::Owner)]);
    assert_eq!(link(&base, "alice", "personal", "acme/app").await.0, 200);
    let mine = app.db.linked_repo(77).unwrap().unwrap().project_id;
    mirror_rows(&app, 77, &mine);

    let (s, v) = call(&base, "alice", "DELETE", "/api/projects/personal/repos/ACME/app", None).await;
    assert_eq!(s, 200, "{v}");
    let purge_after = v["purge_after"].as_u64().unwrap();
    let (s, _) = call(&base, "alice", "DELETE", "/api/projects/personal/repos/acme/app", None).await;
    assert_eq!(s, 404, "unlinked already");

    let (s, v) = link(&base, "alice", "personal", "acme/app").await;
    assert_eq!(s, 200, "{v}");
    assert_eq!((v["unlinked_at"].clone(), v["purge_after"].clone()), (Value::Null, Value::Null));
    assert_eq!(links::purge(&app, purge_after + 1).unwrap(), 0);
    assert_eq!(left(&app, 77), [1; 7], "kept");

    // Unlinked from alice's project and linked to the team's: the old
    // project's rows go now.
    assert_eq!(call(&base, "alice", "DELETE", "/api/projects/personal/repos/acme/app", None).await.0, 200);
    let team = crate::projects::team_project(&app, "t1").await.unwrap().unwrap();
    let (s, v) = link(&base, "alice", &team, "acme/app").await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(left(&app, 77), [0, 0, 0, 0, 0, 0, 1]);
    assert_eq!(app.db.linked_repo(77).unwrap().unwrap().project_id, team);
}

/// A machine with a live `cloud.drive` grant links for its owner (the
/// agents on it, through MCP); one whose grant was revoked can't.
#[tokio::test]
async fn a_machine_links_for_its_owner_with_its_grant() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    acme(&gh, &hub);
    hub.daemon("geek", "alice", None);
    let as_machine = |method: &str, path: &str, body: Option<Value>| {
        let mut r = reqwest::Client::new()
            .request(method.parse().unwrap(), format!("{base}{path}"))
            .header("x-test-daemon", "alice geek");
        if let Some(b) = body {
            r = r.json(&b);
        }
        async move {
            let r = r.send().await.unwrap();
            (r.status().as_u16(), r.json::<Value>().await.unwrap_or_default())
        }
    };
    let (s, v) = as_machine("POST", "/api/projects/personal/repos", Some(json!({ "repo": "acme/app" }))).await;
    assert_eq!(s, 200, "{v}");
    let g = app.db.grants("alice").unwrap().remove(0);
    app.db.revoke_grant("alice", &g.id, 1).unwrap();
    let (s, _) = as_machine("GET", "/api/projects/personal/repos", None).await;
    assert_eq!(s, 403);
}

/// Link `repo` (GitHub id `id`, installation 9) to `project`.
fn linked(app: &Platform, project: &str, repo: &str, id: u64) {
    let l = crate::db::forge::LinkedRepo {
        repo_id: id,
        full_name: repo.into(),
        private: true,
        project_id: project.into(),
        installation_id: 9,
        linked_by: None,
        linked_at: 1,
        unlinked_at: None,
        purge_after: None,
        mirror_content: false,
    };
    let i = crate::db::forge::Installation {
        id: 9,
        account_id: 500,
        login: "acme".into(),
        account_type: "Organization".into(),
        permissions: "{}".into(),
    };
    assert!(matches!(app.db.link_repo(&l, &i, "alice", 1).unwrap(), crate::db::forge::Linked::Now { .. }));
}

/// A deploy whose run would mint a token that writes (M220's chant→Fly
/// ledger, M222's list) is refused when it's planned unless the App's
/// installation on that repository grants `contents: write`, as GitHub
/// itself refuses such a mint; an environment's own token needs no mint.
/// A token for the deploy's own repository, read or write, is for a
/// repository linked to the deploy's project only: when it's planned, and
/// again when it's minted (a link may go after the approval).
#[tokio::test]
async fn a_write_mint_needs_an_installation_that_grants_it() {
    use crate::{
        db::deploys::{Deploy, Environment, Source, SourceKind, State},
        drivers::{
            Mint,
            mint::{mint, plan_refuses},
        },
    };
    let hub = Arc::new(TestHub::default());
    let (gh, app, _) = serve(&hub).await;
    {
        let mut g = gh.lock().unwrap();
        for (repo, id) in [("acme/app", 77), ("acme/site", 78), ("acme/other", 79)] {
            g.installed.insert(repo.into(), (9, 500, "acme".into()));
            g.repos.insert(repo.into(), (id, true));
        }
        g.grants.insert("acme/site".into(), json!({ "metadata": "read", "contents": "write" }));
        g.grants.insert("acme/other".into(), json!({ "metadata": "read", "contents": "write" }));
    }
    linked(&app, "prj_1", "acme/app", 77);
    linked(&app, "prj_1", "acme/site", 78);
    linked(&app, "prj_2", "acme/other", 79);
    let env = |driver: &str, secrets: &[&str]| Environment {
        id: "env_1".into(),
        project: "prj_1".into(),
        name: "production".into(),
        driver: driver.into(),
        secret_names: secrets.iter().map(|s| (*s).to_owned()).collect(),
        settings: Default::default(),
        created_at: 0,
        updated_at: 0,
    };
    let at = |repo: &str| Source { kind: SourceKind::Commit, reference: "a".repeat(40), place: repo.into() };
    let chant = env("chant-fly", &[]);
    // Read-only, as the real App was: refused, and GitHub would refuse it too.
    let why = plan_refuses(&app, &chant, &at("acme/app")).await.unwrap();
    assert!(why.contains("can't write acme/app's contents") && why.contains("GITHUB_TOKEN of its own"), "{why}");
    let gha = app.github.as_ref().unwrap();
    let caller = super::Caller::of(&app, "platform");
    let e = gha.deploy_write_token(&caller, 9, super::Repo::Name("acme/app")).await.unwrap_err();
    assert!(format!("{e:#}").contains("not granted to this installation"), "{e:#}");
    // Granted: planned, and minted.
    assert_eq!(plan_refuses(&app, &chant, &at("ACME/Site")).await, None, "its name in any case");
    let deploy = |repo: &str| Deploy {
        id: "dep_1".into(),
        environment: "env_1".into(),
        source: at(repo),
        plan_digest: "d".into(),
        state: State::Running,
        planned_seq: 1,
        run: Some("run_1".into()),
        planned_by: "alice".into(),
        planned_by_agent: None,
        created_at: 1,
        updated_at: 1,
    };
    let write = Mint::GithubWriteSource { name: "GITHUB_TOKEN" };
    let read = Mint::GithubRead { name: "SOURCE_TOKEN" };
    assert!(mint(&app, write, &deploy("acme/site"), "prj_1", "run_1").await.is_ok());
    assert!(mint(&app, read, &deploy("acme/app"), "prj_1", "run_1").await.is_ok());
    // Another project's repository, or one linked nowhere (the App is
    // installed on both): refused when it's planned and when it's minted,
    // read or write.
    for repo in ["acme/other", "evil/app"] {
        let why = plan_refuses(&app, &chant, &at(repo)).await.unwrap();
        assert!(why.contains(&format!("{repo} isn't a repository linked to this project")), "{why}");
        let why = plan_refuses(&app, &env("apps-platform", &["APPS_PLATFORM_TOKEN"]), &at(repo)).await.unwrap();
        assert!(why.contains("SOURCE_TOKEN"), "{why}");
        for m in [write, read] {
            let e = mint(&app, m, &deploy(repo), "prj_1", "run_1").await.unwrap_err();
            assert!(format!("{e:#}").contains("isn't a repository linked to this project"), "{e:#}");
        }
    }
    // Unlinked after it was planned (and approved): not minted.
    assert!(app.db.unlink_repo("prj_1", "acme/site", 2).unwrap().is_some());
    let e = mint(&app, write, &deploy("acme/site"), "prj_1", "run_1").await.unwrap_err();
    assert!(format!("{e:#}").contains("isn't a repository linked"), "{e:#}");
    // The environment's own token: nothing to mint, nothing to check.
    assert_eq!(plan_refuses(&app, &env("chant-fly", &["GITHUB_TOKEN"]), &at("acme/app")).await, None);
    // apps-platform writes its list, wherever the app is.
    let why = plan_refuses(&app, &env("apps-platform", &[]), &at("acme/app")).await.unwrap();
    assert!(why.contains("arugula-salad/planter-apps") && why.contains("APPS_PLATFORM_TOKEN of its own"), "{why}");
    assert_eq!(plan_refuses(&app, &env("apps-platform", &["APPS_PLATFORM_TOKEN"]), &at("acme/app")).await, None);
}
