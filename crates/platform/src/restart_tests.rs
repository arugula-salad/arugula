//! Threads and issues survive a platform restart (M206, #1277): a
//! platform writes native issues, their timelines and the thread on one
//! (an agent's post among them) and stops, and a new `Platform` on the same
//! database reads every one back through the routes: numbers go on with no
//! gap, search finds them, read marks and grants hold. On a SQLite file,
//! and on Postgres with `ARUGULA_TEST_POSTGRES_URL`. The hub outlives the
//! platform, as it does in production.

use std::{path::PathBuf, sync::Arc};

use arugula_control_wire::{
    chat::agent_messages,
    issues::{agent_issue, agent_project_issues},
};
use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};
use tokio::task::JoinHandle;

use crate::{
    Platform,
    agents_tests::give,
    chat_tests::{TestHub, call},
    db::{
        Db,
        projects::{ISSUES_WRITE, THREADS_POST, THREADS_READ},
    },
};

/// A running platform: its address, and the task serving it.
struct Running {
    app: Arc<Platform>,
    base: String,
    serving: JoinHandle<()>,
}

async fn start(hub: Arc<TestHub>, db: Db) -> Running {
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let app = Arc::new(Platform::new(cfg, db, hub, reqwest::Client::new(), Default::default()));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    let serving = tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    Running { app, base, serving }
}

impl Running {
    /// Stop serving and drop the platform, its database handle with it:
    /// nothing else may still hold it.
    async fn stop(self) {
        self.serving.abort();
        assert!(self.serving.await.unwrap_err().is_cancelled());
        assert!(Arc::into_inner(self.app).is_some(), "the old platform is gone");
    }
}

/// A call signed by a's machine geek, as the hub's gate vouches for it.
async fn geek(base: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req = reqwest::Client::new()
        .request(method.parse().unwrap(), format!("{base}{path}"))
        .header("x-test-daemon", "a geek");
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

async fn ok(base: &str, who: &str, method: &str, path: &str, body: Option<Value>) -> Value {
    let (st, v) = call(base, who, method, path, body).await;
    assert_eq!(st, 200, "{who} {method} {path}: {v}");
    v
}

fn say(text: &str) -> Option<Value> {
    Some(json!({ "text": text }))
}

/// What a reader sees of the project's issues, each one's timeline, the
/// thread on the first, and b's unread count there.
async fn seen(base: &str, project: &str, issues: &[String], room: &str) -> Value {
    let mut timelines = vec![];
    for id in issues {
        timelines.push(ok(base, "b", "GET", &format!("/api/issues/{id}/events"), None).await);
    }
    let rooms = ok(base, "b", "GET", "/api/chat", None).await;
    let mine = rooms["rooms"].as_array().unwrap().iter().find(|r| r["id"] == json!(room)).cloned();
    json!({
        "issues": ok(base, "b", "GET", &format!("/api/projects/{project}/issues?state=all"), None).await,
        "by_number": ok(base, "b", "GET", &format!("/api/projects/{project}/issues/1"), None).await,
        "timelines": timelines,
        "thread": ok(base, "b", "GET", &format!("/api/chat/rooms/{room}/messages"), None).await,
        "unread": mine.map(|r| json!([r["unread"], r["mentions"]])),
    })
}

/// The whole run, `open` giving a new handle on the same database each
/// time it's called.
async fn survives(open: impl Fn() -> Db) {
    let hub = Arc::new(TestHub::default());
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor)]);
    hub.flag("team:t", true);
    hub.daemon("geek", "a", Some("t"));

    let one = start(hub.clone(), open()).await;
    let base = one.base.clone();
    crate::projects::follow(&one.app).await.unwrap();
    crate::cache::catch_up(&one.app).await.unwrap();
    let t = crate::projects::team_project(&one.app, "t").await.unwrap().unwrap();
    for power in [THREADS_READ, THREADS_POST, ISSUES_WRITE] {
        ok(&base, "a", "POST", "/api/grants", give("geek", power, json!({ "kind": "project", "id": t }))).await;
    }

    // Three issues, one by geek's agent; a timeline on each.
    let issues = format!("/api/projects/{t}/issues");
    let first = json!({ "title": "The canary fails", "body": "on deploy since nine", "labels": ["bug"] });
    let first = ok(&base, "a", "POST", &issues, Some(first)).await;
    let second = ok(&base, "b", "POST", &issues, Some(json!({ "title": "Flaky login test" }))).await;
    let (st, third) =
        geek(&base, "POST", &agent_project_issues(&t), Some(json!({ "title": "Agents file zebra issues" }))).await;
    assert_eq!(st, 200, "{third}");
    let ids: Vec<String> = [&first, &second, &third].iter().map(|i| i["id"].as_str().unwrap().to_owned()).collect();
    assert_eq!([&first["number"], &second["number"], &third["number"]], [&json!(1), &json!(2), &json!(3)]);
    ok(&base, "a", "PATCH", &format!("/api/issues/{}", ids[0]), Some(json!({ "assignees": { "add": ["b"] } }))).await;
    ok(&base, "a", "PATCH", &format!("/api/issues/{}", ids[0]), Some(json!({ "title": "The canary still fails" })))
        .await;
    ok(&base, "b", "PATCH", &format!("/api/issues/{}", ids[1]), Some(json!({ "state": "closed" }))).await;

    // The first one's thread: a mentions b, geek's agent answers, b reads
    // that far, and a says one more.
    let room = ok(&base, "a", "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": ids[0] }))).await;
    let room = room["id"].as_str().unwrap().to_owned();
    let path = format!("/api/chat/rooms/{room}/messages");
    ok(&base, "a", "POST", &path, say("@b have a look at the zebra stripes")).await;
    let (st, v) = geek(&base, "POST", &agent_messages(&ids[0]), say("bisected it to the zebra commit")).await;
    assert_eq!(st, 200, "{v}");
    ok(&base, "b", "POST", &format!("/api/chat/rooms/{room}/read"), Some(json!({ "seq": 2 }))).await;
    ok(&base, "a", "POST", &path, say("one more")).await;

    let before = seen(&base, &t, &ids, &room).await;
    assert_eq!(before["issues"]["issues"].as_array().map(Vec::len), Some(3), "{before}");
    assert_eq!(before["thread"]["messages"][1]["agent"], json!({ "machine": "geek", "machine_name": "geek-box" }));
    assert_eq!(before["unread"], json!([1, 0]), "{before}");
    one.stop().await;

    // A new platform on the same database, the hub as it was.
    let two = start(hub.clone(), open()).await;
    let base = two.base.clone();
    assert_eq!(seen(&base, &t, &ids, &room).await, before, "everything reads back as it was");

    // Numbers go on with no gap, the agent's too; the thread's as well.
    let fourth = ok(&base, "b", "POST", &issues, Some(json!({ "title": "After the restart" }))).await;
    assert_eq!(fourth["number"], json!(4), "{fourth}");
    let (st, fifth) = geek(&base, "POST", &agent_project_issues(&t), Some(json!({ "title": "And another" }))).await;
    assert_eq!((st, &fifth["number"], &fifth["agent"]["machine"]), (200, &json!(5), &json!("geek")), "{fifth}");
    let v = ok(&base, "b", "POST", &path, say("still here")).await;
    assert_eq!(v["seq"], json!(4), "{v}");

    // Search finds what was there and what's new.
    let found = ok(&base, "b", "GET", &format!("{issues}?state=all&q=canary"), None).await;
    assert_eq!(found["issues"][0]["id"], json!(ids[0]), "{found}");
    let found = ok(&base, "b", "GET", &format!("{issues}?state=all&q=zebra"), None).await;
    assert_eq!(found["issues"][0]["number"], json!(3), "{found}");
    let found = ok(&base, "b", "GET", &format!("{issues}?q=restart"), None).await;
    assert_eq!(found["issues"][0]["number"], json!(4), "{found}");
    let found = ok(&base, "a", "GET", "/api/chat/search?q=zebra", None).await;
    let hits: Vec<(&Value, &Value)> =
        found["found"].as_array().unwrap().iter().map(|h| (&h["room"], &h["seq"])).collect();
    assert_eq!(hits.len(), 2, "both messages: {found}");
    assert!(hits.iter().all(|h| h.0 == &json!(room)), "{found}");

    // The grants held: geek's agent still reads the issue.
    let (st, v) = geek(&base, "GET", &agent_issue(&ids[0]), None).await;
    assert_eq!((st, &v["title"]), (200, &json!("The canary still fails")), "{v}");
    two.stop().await;
}

/// A directory of its own, removed when dropped.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn threads_and_issues_survive_a_restart_on_a_sqlite_file() {
    let d = Dir(std::env::temp_dir().join(format!("arugula-restart-{}", hex::encode(arugula_e2e::random::<8>()))));
    std::fs::create_dir_all(&d.0).unwrap();
    let file = d.0.join("platform.db");
    survives(|| Db::open(&file).unwrap()).await;
}

#[tokio::test]
async fn threads_and_issues_survive_a_restart_on_postgres() {
    let Ok(url) = std::env::var("ARUGULA_TEST_POSTGRES_URL") else {
        return eprintln!("skipped: needs ARUGULA_TEST_POSTGRES_URL");
    };
    // Holds the test's schema (dropped with it); each platform gets a
    // handle of its own on it.
    let schema = Db::test_on(&url);
    survives(|| schema.second().unwrap()).await;
}
