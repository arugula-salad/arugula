//! Moving beta Cloud workspaces off Managoat (M189), through the
//! platform's routes against a fake Managoat and the fake substrate: what's
//! left behind and its download, the spec mapped, secrets sealed again with
//! the project's key, transcripts copied, a move asked twice making one
//! workspace, and who may move. And the retire command against a fake
//! Managoat. Runs on SQLite and, with `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{Method, StatusCode, Uri},
    routing::{any, get},
};
use serde_json::{Value, json};

use super::{
    Cloud, CloudModel,
    moving::{self, Source, hosted_name, map, tar, transcript_text},
    provider::Provider,
    retire,
    secrets::SecretCipher,
};
use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::cloud::{AgentDefinition, CloudTabSpec, CloudWorkspaceSpec},
    db::sql::{Sql as _, params},
    hosted::tests::Fake,
    keys,
};

const KEY: &str = "5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e";

fn spec(v: Value) -> CloudWorkspaceSpec {
    serde_json::from_value(v).unwrap()
}

// ---- the mapping

#[test]
fn a_spec_maps_onto_a_hosted_one() {
    let s = spec(json!({
        "name": "App box!",
        "repositories": [
            {"url": "https://github.com/acme/app.git", "ref": "feature/x", "mount": "/home/sprite/app", "secret": "GH"},
            {"url": "https://git.example/acme/api", "mount": "/work/api", "secret": "OTHER"},
            {"url": "https://git.example/other/app", "mount": "/home/sprite/app2"},
        ],
        "packages": {"apt": ["ffmpeg"], "npm": ["pnpm", "tsx"]},
        "setup_script": "make setup",
        "setup_timeout_seconds": 600,
        "env_vars": {"RUST_LOG": "debug", "lower": "x"},
        "secrets": ["API_KEY", "RUST_LOG"],
    }));
    let m = map("App box!", &s);
    assert_eq!(m.name, "App-box");
    assert_eq!(
        m.spec.repos,
        ["https://github.com/acme/app.git", "https://git.example/acme/api", "https://git.example/other/app"]
    );
    assert_eq!(m.spec.packages, ["ffmpeg"]);
    assert_eq!(
        m.spec.setup,
        "# Carried from the beta Cloud workspace: its refs and npm packages.\n\
         git -C \"$HOME\"/'app' checkout 'feature/x'\n\
         npm install -g -- 'pnpm' 'tsx'\n\n\
         make setup"
    );
    assert_eq!(m.spec.env, ["RUST_LOG", "API_KEY"]);
    let carried: Vec<(&str, &Source)> = m.carry.iter().map(|c| (c.name.as_str(), &c.source)).collect();
    assert_eq!(
        carried,
        [
            ("RUST_LOG", &Source::Value("debug".into())),
            ("API_KEY", &Source::Secret("API_KEY".into())),
            ("GIT_TOKEN", &Source::Secret("GH".into())),
        ]
    );
    let notes = m.notes.join("\n");
    for said in [
        "/work/api; it's cloned to ~/api now",
        "/home/sprite/app2; it's cloned to ~/app-2 now",
        "setup timeout",
        "lower isn't a name",
        "cloned with GH, OTHER",
    ] {
        assert!(notes.contains(said), "{said:?} in {notes}");
    }
    assert!(m.spec.check().is_ok());

    // Nothing to carry: nothing added to its setup, no notes.
    let m = map(
        "x",
        &spec(json!({"name": "x", "repositories": [{"url": "https://github.com/a/b", "mount": "/home/sprite/b"}]})),
    );
    assert_eq!((m.spec.setup.as_str(), m.carry.len(), m.notes.len()), ("", 0, 0));
}

#[test]
fn names_become_hosted_names() {
    for (from, to) in [
        ("App box", "App-box"),
        ("  --weird//name__ ", "weird-name__"),
        ("日本", "workspace"),
        ("a.b_c-d", "a.b_c-d"),
        (&"x".repeat(100), &"x".repeat(60)),
    ] {
        assert_eq!(hosted_name(from), to, "{from:?}");
        assert!(crate::hosted::good_name(&hosted_name(from)));
    }
}

#[test]
fn events_read_as_a_transcript() {
    let events = vec![
        json!({"id": 1, "blocks": [{"kind": "init", "body": "x"}, {"kind": "prompt", "body": "fix it\nplease"}]}),
        json!({"id": 2, "blocks": [{"kind": "text", "body": "On "}, {"kind": "text", "body": "it."}]}),
        json!({"id": 3, "blocks": [{"kind": "tool_call", "name": "Bash", "summary": "cargo test"}, {"kind": "tool_result", "body": "ok\ndone"}, {"kind": "tool_result", "body": "{}"}]}),
        json!({"id": 4, "blocks": [{"kind": "permission_request", "summary": "rm -rf target", "request_id": "r1"}]}),
        json!({"id": 5, "stage": "turn", "state": "done"}),
    ];
    assert_eq!(
        transcript_text(&events),
        "> fix it\n> please\n\nOn it.\n\n[tool] Bash: cargo test\n\n  ok\n  done\n\n[asked to approve] rm -rf target"
    );
}

/// The entries of a ustar archive.
fn untar(mut b: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    while b.len() >= 512 && b[..512].iter().any(|x| *x != 0) {
        let h = &b[..512];
        let field = |r: std::ops::Range<usize>| {
            String::from_utf8(h[r].iter().copied().take_while(|c| *c != 0).collect()).unwrap()
        };
        let sum: u32 =
            h.iter().enumerate().map(|(i, x)| if (148..156).contains(&i) { 32 } else { u32::from(*x) }).sum();
        assert_eq!(u32::from_str_radix(field(148..154).trim(), 8).unwrap(), sum, "checksum");
        assert_eq!(&h[257..263], b"ustar\0");
        let size = usize::from_str_radix(field(124..135).trim(), 8).unwrap();
        let (prefix, name) = (field(345..500), field(0..100));
        let path = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        out.insert(path, b[512..512 + size].to_vec());
        b = &b[512 + size.div_ceil(512) * 512..];
    }
    out
}

#[test]
fn the_download_is_a_tar() {
    let long = format!("{}/{}", "d".repeat(120), "f".repeat(90));
    let files = vec![
        ("README.txt".to_owned(), b"hi".to_vec()),
        ("app/untracked/empty".to_owned(), vec![]),
        (long.clone(), vec![7u8; 600]),
        ("x".repeat(300), b"left out".to_vec()),
    ];
    let t = tar(&files);
    assert_eq!(t.len() % 512, 0);
    let got = untar(&t);
    assert_eq!(got.len(), 3, "{:?}", got.keys());
    assert_eq!(got["README.txt"], b"hi");
    assert_eq!(got["app/untracked/empty"], b"");
    assert_eq!(got[&long], vec![7u8; 600]);
}

// ---- a fake Managoat

/// Managoat's machine reads of workspace `mg1`, and every request.
#[derive(Default)]
struct Managoat {
    asked: Mutex<Vec<String>>,
}

async fn managoat(
    State(m): State<Arc<Managoat>>,
    method: Method,
    uri: Uri,
    Path((ws, read)): Path<(String, String)>,
    Query(q): Query<BTreeMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    m.asked.lock().unwrap().push(format!("{method} {uri}"));
    let path = q.get("path").map(String::as_str).unwrap_or_default();
    let refused = |code: &str| (StatusCode::NOT_FOUND, Json(json!({"error": code, "message": "secret-ish text"})));
    if ws != "mg1" || method != Method::GET {
        return refused("not_found");
    }
    let app = path == "/home/sprite/app";
    let ok = |v: Value| (StatusCode::OK, Json(json!({ "data": v })));
    match (read.as_str(), q.get("ref").map(String::as_str)) {
        ("git-status", _) if app => ok(json!({
            "path": path, "repo_root": "/home/sprite/app", "branch": "feature", "untracked": "all", "truncated": false,
            "entries": [
                {"path": "src/main.rs", "index": "unchanged", "worktree": "modified"},
                {"path": "notes/todo.txt", "index": "unchanged", "worktree": "untracked"},
                {"path": "../escape", "index": "unchanged", "worktree": "untracked"},
            ],
        })),
        ("git-status", _) => {
            ok(json!({"path": path, "repo_root": path, "branch": "main", "entries": [], "truncated": false}))
        }
        ("diff", Some("HEAD")) if app => {
            ok(json!({"repo_root": path, "diff": "diff --git a/src/main.rs b/src/main.rs\n+new\n", "truncated": false}))
        }
        ("diff", Some("HEAD@{upstream}")) if app => refused("ref_not_found"),
        ("diff", Some("origin/HEAD")) if app => ok(
            json!({"repo_root": path, "diff": "diff --git a/src/lib.rs b/src/lib.rs\n+committed\ndiff --git a/src/main.rs b/src/main.rs\n+new\n", "truncated": false}),
        ),
        ("diff", _) => ok(json!({"repo_root": path, "diff": "", "truncated": false})),
        ("file", _) if path == "/home/sprite/app/notes/todo.txt" => {
            ok(json!({"path": path, "size": 8, "truncated": false, "encoding": "base64", "content": "cmVtZW1iZXI="}))
        }
        _ => refused("path_not_found"),
    }
}

/// alice, bob and carol are in the beta; alice and bob may make hosted
/// workspaces. Managoat is the fake; the substrate is the fake.
async fn serve() -> (Arc<Platform>, Arc<Managoat>, String) {
    let mg = Arc::new(Managoat::default());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mg_url = format!("http://{}", l.local_addr().unwrap());
    let r = Router::new().route("/api/workspaces/{ws}/{read}", any(managoat)).with_state(mg.clone());
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });

    let hub = Arc::new(TestHub::default());
    use arugula_e2e::team::TeamRole::Owner;
    hub.team("t1", &[("alice", Owner), ("bob", Owner), ("carol", Owner)]);
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, crate::db::Db::test(), hub, reqwest::Client::new(), Default::default());
    p.compute = Some(Arc::new(Fake::default()));
    p.hosted = crate::hosted::Config {
        template: Some("tpl".into()),
        accounts: vec!["alice".into(), "bob".into()],
        cipher: Some(Arc::new(SecretCipher::new(KEY).unwrap())),
        managoat_delete_after: Some("2026-11-05".parse().unwrap()),
        ..Default::default()
    };
    p.cloud = Some(Cloud {
        secret_cipher: Some(SecretCipher::new(KEY).unwrap()),
        provider: Provider::new(&mg_url, "platform-key".into()).unwrap(),
        accounts: vec!["alice".into(), "bob".into(), "carol".into()],
        models: vec![CloudModel { runtime: "claude".into(), model: "anthropic/test".into() }],
    });
    let app = Arc::new(p);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = super::tests::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (app, mg, base)
}

/// A beta Cloud workspace of `account`'s on Managoat workspace `mg1`, with
/// two secrets of the account's and two tabs (one closed) with events.
async fn beta(app: &Platform, account: &str) -> (String, String) {
    let project = crate::projects::personal_project(app, account).await.unwrap().unwrap();
    let cipher = app.cloud.as_ref().unwrap().secret_cipher.as_ref().unwrap();
    for (name, value) in [("GH_TOKEN", "ghp-secret-value"), ("API_KEY", "api-secret-value")] {
        let sealed = keys::seal_value(&app.db.c(), cipher, &project, &keys::cloud_name(account, name), value).unwrap();
        app.db.put_cloud_secret(account, &project, name, &sealed).unwrap();
    }
    let s = spec(json!({
        "name": "App box",
        "repositories": [
            {"url": "https://github.com/acme/app", "ref": "feature", "secret": "GH_TOKEN"},
            {"url": "https://git.example/acme/api", "mount": "/work/api"},
        ],
        "packages": {"apt": ["jq"]},
        "setup_script": "make",
        "env_vars": {"RUST_LOG": "debug"},
        "secrets": ["API_KEY"],
    }));
    let mut s2 = s.clone();
    crate::db::cloud::SpecRules::normalize(&mut s2).unwrap();
    let w = app.db.create_cloud_workspace(account, &project, "k", &s2, 1).unwrap();
    app.db
        .c()
        .execute(
            "UPDATE cloud_workspaces SET status = 'ready', sandbox_status = 'ready', provider_id = 'mg1' WHERE id = ?1",
            params![w.id],
        )
        .unwrap();
    let tab = |cwd: &str| CloudTabSpec {
        agent_id: "curated_coding".into(),
        agent_version: 1,
        agent: AgentDefinition {
            name: "Coding agent".into(),
            instructions: "x".into(),
            mcp_servers: Default::default(),
            skills: vec![],
            secret_names: vec![],
        },
        runtime: "claude".into(),
        model: "anthropic/test".into(),
        cwd: cwd.into(),
        prompt: None,
    };
    let t1 = app.db.create_cloud_tab(account, &w.id, "t1", &tab("/home/sprite/app"), 10).unwrap();
    let t2 = app.db.create_cloud_tab(account, &w.id, "t2", &tab("/work/api"), 20).unwrap();
    for (t, cursor, body) in [
        (&t1.id, 1, json!({"id": 1, "blocks": [{"kind": "prompt", "body": "fix the build"}]})),
        (&t1.id, 2, json!({"id": 2, "blocks": [{"kind": "text", "body": "Fixed."}]})),
        (&t2.id, 1, json!({"id": 1, "blocks": [{"kind": "prompt", "body": "add a route"}]})),
    ] {
        app.db
            .c()
            .execute(
                "INSERT INTO cloud_events(conversation,account,cursor,body) VALUES(?1,?2,?3,?4)",
                params![t, account, cursor as u64, body.to_string()],
            )
            .unwrap();
    }
    app.db.c().execute("UPDATE cloud_tabs SET closed = 30 WHERE id = ?1", params![t2.id]).unwrap();
    (w.id, project)
}

#[tokio::test]
async fn a_beta_workspace_moves_once_with_its_secrets_and_transcripts() {
    let (app, mg, base) = serve().await;
    let (cw, project) = beta(&app, "alice").await;

    let (status, o) = call(&base, "alice", "GET", "/api/cloud/moving", None).await;
    assert_eq!(status, 200, "{o}");
    assert_eq!((o["delete_after"].as_str(), o["can_move"].as_bool()), (Some("2026-11-05"), Some(true)));
    let (_, o) = call(&base, "carol", "GET", "/api/cloud/moving", None).await;
    assert_eq!(o["can_move"], false, "carol isn't on the hosted allowlist");

    // What's left behind.
    let (status, plan) = call(&base, "alice", "GET", &format!("/api/cloud/workspaces/{cw}/move"), None).await;
    assert_eq!(status, 200, "{plan}");
    assert_eq!(plan["moved"], Value::Null);
    assert_eq!(plan["makes"]["name"], "App-box");
    assert_eq!(plan["makes"]["secrets"], json!(["RUST_LOG", "API_KEY", "GIT_TOKEN"]));
    assert_eq!(plan["transcripts"], 2);
    let app_repo = &plan["repos"][0];
    assert_eq!(app_repo["mount"], "/home/sprite/app");
    assert_eq!(app_repo["branch"], "feature");
    assert_eq!((app_repo["uncommitted"].as_bool(), app_repo["unpushed"].as_bool()), (Some(true), Some(true)));
    assert_eq!(app_repo["compared_to"], "origin/HEAD", "no upstream: the remote's default branch");
    assert_eq!(app_repo["changes"].as_array().unwrap().len(), 3);
    let api = &plan["repos"][1];
    assert_eq!((api["uncommitted"].as_bool(), api["unpushed"].as_bool()), (Some(false), Some(false)));
    let said = plan.to_string();
    for secret in ["ghp-secret-value", "api-secret-value", "debug", "platform-key", "secret-ish"] {
        assert!(!said.contains(secret), "{secret} in {said}");
    }

    // Its download.
    let r = reqwest::Client::new()
        .get(format!("{base}/api/cloud/workspaces/{cw}/move/bundle"))
        .header("cookie", "ilg_session=session-alice")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "application/x-tar");
    assert!(r.headers()["content-disposition"].to_str().unwrap().contains("App-box-left-behind.tar"));
    let files = untar(&r.bytes().await.unwrap());
    assert_eq!(
        files.keys().map(String::as_str).collect::<Vec<_>>(),
        ["README.txt", "app/uncommitted.patch", "app/unpushed.patch", "app/untracked/notes/todo.txt"]
    );
    assert_eq!(files["app/untracked/notes/todo.txt"], b"remember");
    assert!(String::from_utf8_lossy(&files["app/unpushed.patch"]).contains("+committed"));
    assert!(String::from_utf8_lossy(&files["README.txt"]).contains("Left out \"../escape\""));
    assert!(mg.asked.lock().unwrap().iter().all(|a| a.starts_with("GET ")), "a move only reads Managoat");

    // Bob's isn't his; carol may not move.
    let path = format!("/api/cloud/workspaces/{cw}/move");
    assert_eq!(call(&base, "bob", "POST", &path, None).await.0, 404);
    assert_eq!(call(&base, "bob", "GET", &path, None).await.0, 404);
    let (carol_cw, _) = beta(&app, "carol").await;
    assert_eq!(call(&base, "carol", "POST", &format!("/api/cloud/workspaces/{carol_cw}/move"), None).await.0, 403);

    // The move.
    let (status, moved) = call(&base, "alice", "POST", &path, None).await;
    assert_eq!(status, 200, "{moved}");
    let hw = moved["workspace"]["id"].as_str().unwrap().to_owned();
    assert!(hw.starts_with("hw_"), "{hw}");
    assert_eq!((moved["project"].as_str(), moved["workspace"]["name"].as_str()), (Some(&*project), Some("App-box")));
    assert_eq!(moved["workspace"]["state"], "creating");
    assert_eq!(moved["transcripts"], 2);
    assert_eq!(moved["secrets"], json!(["RUST_LOG", "API_KEY", "GIT_TOKEN"]));
    assert!(!moved.to_string().contains("secret-value"));
    let w = app.db.hosted_workspace(&project, &hw).unwrap().unwrap();
    assert_eq!(w.spec.repos, ["https://github.com/acme/app", "https://git.example/acme/api"]);
    assert_eq!(w.spec.packages, ["jq"]);
    assert_eq!(w.spec.env, ["RUST_LOG", "API_KEY"]);
    assert!(w.spec.setup.contains("checkout 'feature'") && w.spec.setup.ends_with("make"), "{}", w.spec.setup);

    // Its secrets, sealed again with the project's key, bound to it.
    let master = SecretCipher::new(KEY).unwrap();
    for (name, value) in [("RUST_LOG", "debug"), ("API_KEY", "api-secret-value"), ("GIT_TOKEN", "ghp-secret-value")] {
        let sealed = app.db.hosted_secret_sealed(&hw, name).unwrap().unwrap();
        assert_ne!(sealed.key_version, keys::MASTER, "{name} is sealed with the project's key");
        assert_eq!(keys::open_hosted(&app.db.c(), &master, &project, &hw, name, &sealed).unwrap(), value);
        assert!(
            keys::open_value(&app.db.c(), &master, &project, &keys::hosted_name("hw_other", name), &sealed).is_err()
        );
    }

    // Its tabs' transcripts, read only under the project.
    let (status, t) =
        call(&base, "alice", "GET", &format!("/api/projects/{project}/transcripts?workspace={hw}"), None).await;
    assert_eq!(status, 200, "{t}");
    let list = t["transcripts"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    let first = list.iter().find(|t| t["cwd"] == "/home/sprite/app").unwrap();
    assert_eq!(first["agent"], "claude");
    assert_eq!(first["title"], "Coding agent (beta, in App box)");
    let (_, one) = call(
        &base,
        "alice",
        "GET",
        &format!("/api/projects/{project}/transcripts/{}", first["id"].as_str().unwrap()),
        None,
    )
    .await;
    assert_eq!(one["text"], "> fix the build\n\nFixed.");

    // Again: the same workspace, nothing more made or copied.
    let (status, again) = call(&base, "alice", "POST", &path, None).await;
    assert_eq!(status, 200, "{again}");
    assert_eq!(again["workspace"]["id"], hw.as_str());
    assert_eq!(app.db.hosted_workspaces(&project).unwrap().len(), 1);
    assert_eq!(app.db.hosted_transcripts(&project, None).unwrap().len(), 2);
    let asked = mg.asked.lock().unwrap().len();
    let (_, plan) = call(&base, "alice", "GET", &format!("{path}?read=false"), None).await;
    assert_eq!((plan["moved"]["id"].as_str(), plan["moved"]["done"].as_bool()), (Some(&*hw), Some(true)));
    assert_eq!((plan["unread"].as_str(), plan["repos"].clone()), (Some("Not read."), json!([])));
    assert_eq!(mg.asked.lock().unwrap().len(), asked, "?read=false reads nothing");

    // The Managoat one is left as it was.
    let left = app.db.cloud_workspace("alice", &cw).unwrap().unwrap();
    assert_eq!(left.status, "ready");

    // Its hosted workspace deleted: a move makes a new one.
    app.db
        .c()
        .execute("UPDATE hosted_workspaces SET deleted_at = 1, state = 'deleted' WHERE id = ?1", params![hw])
        .unwrap();
    let (status, fresh) = call(&base, "alice", "POST", &path, None).await;
    assert_eq!(status, 200, "{fresh}");
    assert_ne!(fresh["workspace"]["id"], hw.as_str());
}

/// A move cut short (its id kept, nothing made) is finished by the next
/// one, under the same id; a name taken in the project gets `-2`.
#[tokio::test]
async fn a_move_cut_short_finishes_under_its_id() {
    let (app, _, base) = serve().await;
    let (cw, project) = beta(&app, "alice").await;
    let spec = crate::hosted::Spec::default();
    assert!(matches!(
        app.db.add_hosted_workspace("hw_taken", &project, "App-box", &spec, "alice", 10, 1).unwrap(),
        crate::db::hosted::Added::Made(_)
    ));
    app.db.claim_cloud_move("alice", &cw, "hw_0123456789abcdef0123456789abcdef").unwrap();
    let (status, moved) = call(&base, "alice", "POST", &format!("/api/cloud/workspaces/{cw}/move"), None).await;
    assert_eq!(status, 200, "{moved}");
    assert_eq!(moved["workspace"]["id"], "hw_0123456789abcdef0123456789abcdef");
    assert_eq!(moved["workspace"]["name"], "App-box-2");
}

/// A parked machine isn't read (a read never wakes it), and the download
/// says why; the move still works.
#[tokio::test]
async fn a_parked_machine_is_not_read() {
    let (app, mg, base) = serve().await;
    let (cw, _) = beta(&app, "alice").await;
    app.db.c().execute("UPDATE cloud_workspaces SET sandbox_status = 'suspended' WHERE id = ?1", params![cw]).unwrap();
    let (status, plan) = call(&base, "alice", "GET", &format!("/api/cloud/workspaces/{cw}/move"), None).await;
    assert_eq!(status, 200);
    assert!(plan["unread"].as_str().unwrap().contains("asleep"), "{plan}");
    assert_eq!(plan["repos"], json!([]));
    assert_eq!(call(&base, "alice", "GET", &format!("/api/cloud/workspaces/{cw}/move/bundle"), None).await.0, 409);
    assert!(mg.asked.lock().unwrap().is_empty());
    assert_eq!(call(&base, "alice", "POST", &format!("/api/cloud/workspaces/{cw}/move"), None).await.0, 200);
}

// ---- the retire command

#[derive(Default)]
struct Account {
    /// By kind: each resource.
    listed: Mutex<BTreeMap<String, Vec<Value>>>,
    deleted: Mutex<Vec<String>>,
    /// The next delete of this id fails with a 500.
    fail: Mutex<Option<String>>,
}

async fn account_list(State(a): State<Arc<Account>>, Path(kind): Path<String>) -> Json<Value> {
    Json(json!({ "data": a.listed.lock().unwrap().get(&kind).cloned().unwrap_or_default() }))
}

async fn account_delete(State(a): State<Arc<Account>>, Path((kind, id)): Path<(String, String)>) -> StatusCode {
    if a.fail.lock().unwrap().take_if(|f| *f == id).is_some() {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    let mut listed = a.listed.lock().unwrap();
    let list = listed.entry(kind.clone()).or_default();
    let before = list.len();
    list.retain(|v| v["id"] != id);
    if list.len() == before {
        return StatusCode::NOT_FOUND;
    }
    a.deleted.lock().unwrap().push(format!("{kind}/{id}"));
    StatusCode::NO_CONTENT
}

#[tokio::test]
async fn retire_lists_then_deletes_what_control_made() {
    let a = Arc::new(Account::default());
    a.listed.lock().unwrap().extend([
        (
            "workspaces".to_owned(),
            vec![json!({"id": "ws1", "name": "cw_1", "environment_id": "env1"}), json!({"id": "ws2", "name": "mine"})],
        ),
        (
            "vaults".to_owned(),
            vec![
                json!({"id": "v1", "name": "cw_1", "metadata": {"arugula_workspace": "cw_1"}}),
                json!({"id": "v2", "name": "personal"}),
            ],
        ),
        (
            "environments".to_owned(),
            vec![
                json!({"id": "env1", "metadata": {"arugula_workspace": "cw_1"}}),
                json!({"id": "env2", "metadata": {"arugula_run": "run_1"}}),
            ],
        ),
    ]);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let r = Router::new()
        .route("/api/{kind}", get(account_list))
        .route("/api/{kind}/{id}", axum::routing::delete(account_delete))
        .with_state(a.clone());
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
    let p = Provider::new(&url, "platform-key".into()).unwrap();

    let found = retire::retire(&p, false, false).await.unwrap();
    assert_eq!(found.len(), 6);
    assert!(a.deleted.lock().unwrap().is_empty(), "a dry run deletes nothing");
    let said = retire::render(&found, false, false);
    assert!(said.contains("ws1  cw_1  would delete") && said.contains("ws2  mine  kept"), "{said}");
    assert!(said.contains("4 to delete. Nothing was deleted"), "{said}");

    // One fails; running it again finishes, workspaces before vaults
    // before environments, and only control's.
    *a.fail.lock().unwrap() = Some("v1".into());
    let found = retire::retire(&p, true, false).await.unwrap();
    let said = retire::render(&found, true, false);
    assert!(said.contains("deleted 3 of 4; 1 failed"), "{said}");
    assert_eq!(*a.deleted.lock().unwrap(), ["workspaces/ws1", "environments/env1", "environments/env2"]);
    let found = retire::retire(&p, true, false).await.unwrap();
    assert!(retire::render(&found, true, false).contains("deleted 1"));
    assert_eq!(a.deleted.lock().unwrap().len(), 4);
    assert_eq!(a.deleted.lock().unwrap()[3], "vaults/v1");

    // --all: the rest.
    retire::retire(&p, true, true).await.unwrap();
    let deleted = a.deleted.lock().unwrap().clone();
    assert_eq!(&deleted[4..], ["workspaces/ws2", "vaults/v2"]);
    assert!(a.listed.lock().unwrap().values().all(Vec::is_empty));
}

#[test]
fn retire_wants_one_of_dry_run_or_execute() {
    use clap::Parser as _;
    let parse = |args: &[&str]| {
        retire::Args::try_parse_from(["managoat-retire"].into_iter().chain(args.iter().copied()).chain(["--key", "k"]))
    };
    assert!(parse(&[]).is_err());
    assert!(parse(&["--dry-run", "--execute"]).is_err());
    assert!(parse(&["--dry-run"]).is_ok_and(|a| a.dry_run && !a.execute));
    assert!(parse(&["--execute", "--all"]).is_ok_and(|a| a.execute && a.all));
    let _ = moving::managoat_machine("cwc_1");
}
