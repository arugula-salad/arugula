//! CI alerts as native issues (native #774) through the platform's routes:
//! team t1's project (alice owns it, bob edits, carol views) turns them on,
//! and chant's signed events open, comment on and close its red issue. The
//! signature (good, bad, old, repeated), one project's key on another's
//! URL, the lifecycle, the setting's mentions and who may change it. Runs
//! on SQLite and, with `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::{sync::Arc, time::Duration};

use arugula_e2e::{now_ms, team::TeamRole};
use serde_json::{Value, json};

use crate::{
    Platform,
    chat_tests::{TestHub, call},
    ci_alerts::sign,
    db::{Sql as _, sql::params},
    secrets::SecretCipher,
};

const MASTER: &str = "0505050505050505050505050505050505050505050505050505050505050505"; // gitleaks:allow
const RED: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CULPRIT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const GREEN: &str = "cccccccccccccccccccccccccccccccccccccccc";

struct World {
    hub: Arc<TestHub>,
    app: Arc<Platform>,
    base: String,
    project: String,
    key: String,
}

/// The platform with a key to seal with, t1's project with alerts on
/// (alice made the key) and chat on for t1.
async fn world() -> World {
    let hub = Arc::new(TestHub::default());
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, crate::db::Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    p.secret_key = Some(Arc::new(SecretCipher::new(MASTER).unwrap()));
    let app = Arc::new(p);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("carol", TeamRole::Viewer)]);
    hub.flag("team:t1", true);
    let project = crate::projects::team_project(&app, "t1").await.unwrap().unwrap();
    let key = make_key(&base, "alice", &project).await;
    World { hub, app, base, project, key }
}

async fn make_key(base: &str, who: &str, project: &str) -> String {
    let (s, v) = call(base, who, "POST", &format!("/api/projects/{project}/ci-alerts/key"), None).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["alerts"]["on"], true, "{v}");
    v["key"].as_str().unwrap().to_owned()
}

fn event(id: &str, kind: &str, ci: Value) -> Value {
    json!({
        "schema": "chant.notify/v1", "id": id, "event": kind, "at": "2026-11-06T00:00:00.000Z",
        "source": "chant", "project": "acme/app", "summary": format!("main {kind}"), "targets": [],
        "ci": ci, "run": { "forge": "github", "repository": "acme/app", "commit": RED },
    })
}

/// Post `body` as chant would, signed with `key` at `ts` (seconds), or
/// with `signature` as given.
async fn post_raw(base: &str, project: &str, body: &Value, ts: u64, signature: Option<String>) -> (u16, Value) {
    let bytes = serde_json::to_vec(body).unwrap();
    let mut r = reqwest::Client::new()
        .post(format!("{base}/ci/notify/{project}"))
        .header("content-type", "application/json")
        .header("x-chant-timestamp", ts.to_string())
        .header("x-chant-event", body["event"].as_str().unwrap_or_default())
        .header("x-chant-delivery", body["id"].as_str().unwrap_or_default())
        .body(bytes);
    if let Some(s) = signature {
        r = r.header("x-chant-signature", s);
    }
    let r = r.send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or_default())
}

async fn post(base: &str, project: &str, key: &str, body: &Value) -> (u16, Value) {
    let ts = now_ms() / 1000;
    let sig = sign(key, &ts.to_string(), &serde_json::to_vec(body).unwrap());
    post_raw(base, project, body, ts, Some(sig)).await
}

async fn issue(w: &World, id: &str) -> Value {
    let (s, v) = call(&w.base, "alice", "GET", &format!("/api/issues/{id}"), None).await;
    assert_eq!(s, 200, "{v}");
    v
}

async fn comments(w: &World, id: &str) -> Vec<Value> {
    let (s, room) =
        call(&w.base, "alice", "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": id }))).await;
    assert_eq!(s, 200, "{room}");
    let room = room["id"].as_str().unwrap();
    let (s, v) = call(&w.base, "alice", "GET", &format!("/api/chat/rooms/{room}/messages"), None).await;
    assert_eq!(s, 200, "{v}");
    v["messages"].as_array().unwrap().clone()
}

fn inbox(app: &Platform, account: &str) -> Vec<(String, String)> {
    app.db
        .c()
        .query_map(
            "SELECT item, reason FROM inbox_items WHERE account = ?1 ORDER BY item, reason",
            params![account],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

#[tokio::test]
async fn the_signature_is_checked_and_a_repeat_dropped() {
    let w = world().await;
    let red = event("e1", "ci.red", json!({ "branch": "main", "commit": RED, "phase": "test" }));
    let bytes = serde_json::to_vec(&red).unwrap();
    let now = now_ms() / 1000;

    // No signature, a bad one, one with another key, one over another body.
    assert_eq!(post_raw(&w.base, &w.project, &red, now, None).await.0, 401);
    assert_eq!(post_raw(&w.base, &w.project, &red, now, Some("sha256=00".into())).await.0, 401);
    let other = sign("not-the-key", &now.to_string(), &bytes);
    assert_eq!(post_raw(&w.base, &w.project, &red, now, Some(other)).await.0, 401);
    let mut changed = red.clone();
    changed["ci"]["branch"] = json!("release");
    let over_red = sign(&w.key, &now.to_string(), &bytes);
    assert_eq!(post_raw(&w.base, &w.project, &changed, now, Some(over_red)).await.0, 401);
    // Signed, but six minutes old, or six minutes ahead.
    for ts in [now - 360, now + 360] {
        let sig = sign(&w.key, &ts.to_string(), &bytes);
        let (s, v) = post_raw(&w.base, &w.project, &red, ts, Some(sig)).await;
        assert_eq!(s, 401, "{v}");
        assert!(v["error"].as_str().unwrap().contains("timestamp"), "{v}");
    }
    assert_eq!(w.app.db.ci_alert_opens(&w.project).unwrap().len(), 0, "nothing refused wrote anything");

    // Good: it opens the issue. The same id again is a no-op.
    let (s, v) = post(&w.base, &w.project, &w.key, &red).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["did"], "opened");
    let (s, again) = post(&w.base, &w.project, &w.key, &red).await;
    assert_eq!(s, 200, "{again}");
    assert_eq!(again["duplicate"], true);
    assert_eq!(comments(&w, v["issue"].as_str().unwrap()).await.len(), 0, "the repeat said nothing");

    // Events it doesn't act on are taken and ignored.
    for kind in ["waiting", "ci.quarantine-passed", "drift"] {
        let (s, v) = post(&w.base, &w.project, &w.key, &event(&format!("x-{kind}"), kind, json!({}))).await;
        assert_eq!((s, &v["ignored"]), (200, &json!(true)), "{kind}: {v}");
    }
    // Not chant.notify/v1.
    let mut bad = event("e9", "ci.red", json!({ "branch": "main", "commit": RED }));
    bad["schema"] = json!("chant.notify/v2");
    assert_eq!(post(&w.base, &w.project, &w.key, &bad).await.0, 400);

    // A project with alerts off, or none at all, is no such webhook.
    assert_eq!(post(&w.base, "prj_nothere", &w.key, &red).await.0, 404);
}

#[tokio::test]
async fn one_projects_key_cant_post_to_another() {
    let w = world().await;
    // t2's project, alice's too, with its own key.
    w.hub.team("t2", &[("alice", TeamRole::Owner)]);
    let other = crate::projects::team_project(&w.app, "t2").await.unwrap().unwrap();
    let other_key = make_key(&w.base, "alice", &other).await;
    assert_ne!(other_key, w.key);
    let red = event("e1", "ci.red", json!({ "branch": "main", "commit": RED }));
    assert_eq!(post(&w.base, &w.project, &other_key, &red).await.0, 401);
    assert_eq!(post(&w.base, &other, &w.key, &red).await.0, 401);
    assert!(w.app.db.ci_alert_opens(&w.project).unwrap().is_empty());
    assert!(w.app.db.ci_alert_opens(&other).unwrap().is_empty());
    // Each with its own.
    assert_eq!(post(&w.base, &other, &other_key, &red).await.1["did"], "opened");

    // Rotated: the old key stops at once. Off: no such webhook, and the
    // key is gone.
    let new = make_key(&w.base, "bob", &w.project).await;
    let red2 = event("e2", "ci.red", json!({ "branch": "main", "commit": RED }));
    assert_eq!(post(&w.base, &w.project, &w.key, &red2).await.0, 401);
    assert_eq!(post(&w.base, &w.project, &new, &red2).await.1["did"], "opened");
    let (s, v) =
        call(&w.base, "bob", "PATCH", &format!("/api/projects/{}/ci-alerts", w.project), Some(json!({ "on": false })))
            .await;
    assert_eq!((s, &v["on"]), (200, &json!(false)), "{v}");
    let red3 = event("e3", "ci.red", json!({ "branch": "main", "commit": RED }));
    assert_eq!(post(&w.base, &w.project, &new, &red3).await.0, 404);
    let kept: i64 = w
        .app
        .db
        .c()
        .query_row("SELECT count(*) FROM project_secrets WHERE project_id = ?1", params![w.project], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 0);
}

#[tokio::test]
async fn red_opens_events_comment_and_green_closes() {
    let w = world().await;
    let red = |id: &str| {
        event(
            id,
            "ci.red",
            json!({ "branch": "main", "commit": RED, "phase": "test",
                "failedRun": { "name": "test-2", "conclusion": "failure", "url": "https://github.com/acme/app/actions/runs/7" } }),
        )
    };
    let (s, v) = post(&w.base, &w.project, &w.key, &red("r1")).await;
    assert_eq!((s, &v["did"]), (200, &json!("opened")), "{v}");
    let id = v["issue"].as_str().unwrap().to_owned();
    let i = issue(&w, &id).await;
    assert_eq!(i["title"], "main is red at aaaaaaaaaaaa");
    assert_eq!(i["state"], "open");
    assert_eq!(i["labels"], json!(["ci"]));
    // Not a person's, nor anyone's agent: chant CI's.
    assert_eq!(i["author"], "");
    assert_eq!(i["agent"]["machine"], "ci:chant");
    assert_eq!(i["agent"]["machine_name"], "chant CI");
    let body = i["body"].as_str().unwrap();
    assert!(body.contains("Phase test: [test-2 (failure)](https://github.com/acme/app/actions/runs/7)"), "{body}");
    assert!(body.contains("https://github.com/acme/app/commit/aaaa"), "{body}");

    // Red again: a comment on the same issue.
    let (_, v) = post(&w.base, &w.project, &w.key, &red("r2")).await;
    assert_eq!((v["did"].as_str(), v["issue"].as_str()), (Some("commented"), Some(id.as_str())), "{v}");
    // The culprit and stale: comments.
    let culprit = event("c1", "ci.culprit", json!({ "branch": "main", "commit": RED, "culprit": CULPRIT }));
    let (_, v) = post(&w.base, &w.project, &w.key, &culprit).await;
    assert_eq!((v["did"].as_str(), v["issue"].as_str()), (Some("commented"), Some(id.as_str())), "{v}");
    let stale = event(
        "s1",
        "ci.stale",
        json!({ "branch": "main", "commit": RED, "since": CULPRIT, "lastGreen": GREEN, "hours": 7 }),
    );
    let (_, v) = post(&w.base, &w.project, &w.key, &stale).await;
    assert_eq!(v["did"], "commented", "{v}");

    // Green: a comment, and closed as completed by chant CI.
    let green = event("g1", "ci.green", json!({ "branch": "main", "commit": GREEN, "red": RED }));
    let (_, v) = post(&w.base, &w.project, &w.key, &green).await;
    assert_eq!((v["did"].as_str(), v["issue"].as_str()), (Some("closed"), Some(id.as_str())), "{v}");
    let i = issue(&w, &id).await;
    assert_eq!((i["state"].as_str(), i["close_reason"].as_str()), (Some("closed"), Some("completed")));
    let (_, list) =
        call(&w.base, "alice", "GET", &format!("/api/projects/{}/issues?state=closed", w.project), None).await;
    assert_eq!(list["issues"][0]["closed_by_agent"]["machine_name"], "chant CI", "{list}");

    let said = comments(&w, &id).await;
    let texts: Vec<&str> = said.iter().map(|m| m["text"].as_str().unwrap()).collect();
    assert_eq!(said.len(), 4, "{texts:?}");
    for m in &said {
        assert_eq!((m["author"].as_str(), m["via"].as_str()), (Some(""), Some("ci:chant")), "{m}");
        assert_eq!(m["name"], "chant CI");
        assert!(m["agent"].is_null());
    }
    assert!(texts[0].contains("Still red after its retry"), "{}", texts[0]);
    assert!(texts[1].contains("bbbbbbbbbbbb") && texts[1].contains("turned **main** red"), "{}", texts[1]);
    assert!(texts[2].contains("no green commit for 7 hours"), "{}", texts[2]);
    assert!(texts[3].contains("green again at") && texts[3].contains("cccccccccccc"), "{}", texts[3]);

    // A green with nothing open does nothing; a red after green opens a new
    // issue.
    let (_, v) =
        post(&w.base, &w.project, &w.key, &event("g2", "ci.green", json!({ "branch": "main", "commit": GREEN }))).await;
    assert_eq!(v["ignored"], true, "{v}");
    let (_, v) = post(&w.base, &w.project, &w.key, &red("r3")).await;
    assert_eq!(v["did"], "opened", "{v}");
    assert_ne!(v["issue"].as_str(), Some(id.as_str()));
    assert_eq!(v["number"], 2);
    // Another branch has its own.
    let other = event("r4", "ci.red", json!({ "branch": "release", "commit": RED }));
    let (_, o) = post(&w.base, &w.project, &w.key, &other).await;
    assert_eq!(o["did"], "opened", "{o}");
    assert_eq!(issue(&w, o["issue"].as_str().unwrap()).await["title"], "release is red at aaaaaaaaaaaa");

    // An issue a person closed is left closed: the next red opens another.
    let now_open = v["issue"].as_str().unwrap();
    let (s, _) =
        call(&w.base, "alice", "PATCH", &format!("/api/issues/{now_open}"), Some(json!({ "state": "closed" }))).await;
    assert_eq!(s, 200);
    let (_, v) = post(&w.base, &w.project, &w.key, &culprit_again()).await;
    assert_eq!(v["did"], "opened", "{v}");
    let i = issue(&w, v["issue"].as_str().unwrap()).await;
    assert_eq!(i["title"], "main is red at bbbbbbbbbbbb");

    // The settings say which are open.
    let (_, s) = call(&w.base, "carol", "GET", &format!("/api/projects/{}/ci-alerts", w.project), None).await;
    let branches: Vec<&str> = s["open"].as_array().unwrap().iter().map(|o| o["branch"].as_str().unwrap()).collect();
    assert_eq!(branches, ["release", "main"], "{s}");
    assert_eq!(s["url"], format!("https://control.example/ci/notify/{}", w.project));
    assert!(s.get("key").is_none());
}

fn culprit_again() -> Value {
    event("c2", "ci.culprit", json!({ "branch": "main", "commit": RED, "culprit": CULPRIT }))
}

#[tokio::test]
async fn the_setting_mentions_and_assigns_its_people() {
    let w = world().await;
    let path = format!("/api/projects/{}/ci-alerts", w.project);
    // A viewer changes nothing; someone outside the project can't be named.
    let (s, _) = call(&w.base, "carol", "PATCH", &path, Some(json!({ "people": ["carol"] }))).await;
    assert_eq!(s, 403);
    let (s, _) = call(&w.base, "carol", "POST", &format!("{path}/key"), None).await;
    assert_eq!(s, 403);
    let (s, v) = call(&w.base, "alice", "PATCH", &path, Some(json!({ "people": ["mallory"] }))).await;
    assert_eq!(s, 422, "{v}");
    let (s, v) =
        call(&w.base, "alice", "PATCH", &path, Some(json!({ "people": ["bob", "carol", "bob"], "assign": true })))
            .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!((&v["people"], &v["assign"]), (&json!(["bob", "carol"]), &json!(true)));

    // An `@` in the event's own words names nobody.
    let mut red = event("r1", "ci.red", json!({ "branch": "main", "commit": RED }));
    red["summary"] = json!("main is red, ask @alice");
    let (_, v) = post(&w.base, &w.project, &w.key, &red).await;
    let id = v["issue"].as_str().unwrap().to_owned();
    let i = issue(&w, &id).await;
    assert_eq!(i["assignees"], json!(["bob", "carol"]));
    assert!(i["body"].as_str().unwrap().contains("cc @bob @carol"), "{}", i["body"]);
    assert_eq!(inbox(&w.app, "bob"), [(id.clone(), "assigned".into()), (id.clone(), "mentioned".into())]);
    assert_eq!(inbox(&w.app, "carol"), [(id.clone(), "assigned".into()), (id.clone(), "mentioned".into())]);
    assert!(inbox(&w.app, "alice").is_empty(), "the summary's @alice is text");
    // One push each, saying chant CI did it.
    let pushed = async || {
        for _ in 0..100 {
            let p = w.hub.pushes();
            if p.len() >= 2 {
                return p;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        w.hub.pushes()
    };
    let p = pushed().await;
    let mut to: Vec<&str> = p.iter().map(|(a, _, _)| a.as_str()).collect();
    to.sort();
    assert_eq!(to, ["bob", "carol"]);
    assert!(p[0].1["title"].as_str().unwrap().contains("assigned to you by chant CI"), "{:?}", p[0].1);

    // A stale comment mentions them in the thread: an entry each (the push
    // waits out M171's minute per item).
    w.app.db.c().execute("DELETE FROM inbox_items", params![]).unwrap();
    let stale = event("s1", "ci.stale", json!({ "branch": "main", "commit": RED, "since": RED, "hours": 6 }));
    assert_eq!(post(&w.base, &w.project, &w.key, &stale).await.1["did"], "commented");
    assert_eq!(inbox(&w.app, "bob"), [(id.clone(), "mentioned".into())]);
    assert_eq!(inbox(&w.app, "carol"), [(id.clone(), "mentioned".into())]);
    // Red again is said in the thread without telling them.
    w.app.db.c().execute("DELETE FROM inbox_items", params![]).unwrap();
    let again = event("r2", "ci.red", json!({ "branch": "main", "commit": RED }));
    assert_eq!(post(&w.base, &w.project, &w.key, &again).await.1["did"], "commented");
    assert!(inbox(&w.app, "bob").is_empty());

    // Carol leaves the team: she's told no more.
    w.hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor)]);
    w.app.cache.team_changed("t1");
    w.app.db.c().execute("DELETE FROM inbox_items", params![]).unwrap();
    let green = event("g1", "ci.green", json!({ "branch": "main", "commit": GREEN }));
    assert_eq!(post(&w.base, &w.project, &w.key, &green).await.1["did"], "closed");
    assert_eq!(inbox(&w.app, "bob"), [(id.clone(), "mentioned".into())]);
    assert!(inbox(&w.app, "carol").is_empty());
}

/// The platform runs on two machines, so two events for a branch can be
/// taken at once: whichever opens the issue, the other comments on it, and
/// the branch has one open issue (the unique index on the open rows).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_reds_at_once_leave_one_open_issue() {
    let w = world().await;
    for n in 0..8 {
        let branch = format!("b{n}");
        let one = event(&format!("{branch}-1"), "ci.red", json!({ "branch": branch, "commit": RED }));
        let two = event(&format!("{branch}-2"), "ci.red", json!({ "branch": branch, "commit": CULPRIT }));
        let (a, b) = tokio::join!(post(&w.base, &w.project, &w.key, &one), post(&w.base, &w.project, &w.key, &two));
        assert_eq!((a.0, b.0), (200, 200), "{a:?} {b:?}");
        let mut did = [a.1["did"].as_str().unwrap(), b.1["did"].as_str().unwrap()];
        did.sort();
        assert_eq!(did, ["commented", "opened"], "{branch}: {a:?} {b:?}");
        assert_eq!(a.1["issue"], b.1["issue"], "{branch}: both on one issue");
        let open: i64 = w
            .app
            .db
            .c()
            .query_row(
                "SELECT count(*) FROM issues i JOIN ci_alert_issues c ON c.issue_id = i.id
                 WHERE c.project_id = ?1 AND c.branch = ?2 AND i.state = 'open'",
                params![w.project, branch],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(open, 1, "{branch}");
    }
    // Numbers have no gaps: the one that lost wrote nothing.
    let (_, list) = call(&w.base, "alice", "GET", &format!("/api/projects/{}/issues?state=all", w.project), None).await;
    let mut numbers: Vec<u64> =
        list["issues"].as_array().unwrap().iter().map(|i| i["number"].as_u64().unwrap()).collect();
    numbers.sort();
    assert_eq!(numbers, (1..=8).collect::<Vec<_>>());
}

/// The database holds it whoever asks: a second open issue for a branch is
/// refused and leaves nothing behind, and once the first is closed another
/// opens.
#[tokio::test]
async fn the_database_keeps_one_open_issue_per_branch() {
    let w = world().await;
    let by = crate::db::issues::Actor { account: "", agent: Some("ci:chant") };
    let new = |title: &'static str| crate::db::issues::NewIssue {
        project: &w.project,
        title,
        body: "",
        labels: &[],
        assignees: &[],
    };
    let first = w.app.db.create_ci_issue(&new("one"), by, "main", RED, 1).unwrap().unwrap();
    assert!(w.app.db.create_ci_issue(&new("two"), by, "main", RED, 2).unwrap().is_none());
    assert!(w.app.db.issue_by_number(&w.project, 2).unwrap().is_none(), "no issue, and no number, for the refused one");
    // Another branch is its own.
    assert!(w.app.db.create_ci_issue(&new("other"), by, "release", RED, 3).unwrap().is_some());
    w.app.db.ci_alert_closed(&w.project, "main", &first.id, 4).unwrap();
    let again = w.app.db.create_ci_issue(&new("three"), by, "main", GREEN, 5).unwrap().unwrap();
    assert_eq!(w.app.db.ci_alert_open(&w.project, "main").unwrap().unwrap().issue, again.id);
}
