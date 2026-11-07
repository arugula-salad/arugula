//! CI alerts as native issues (native #774) through the platform's routes:
//! team t1's project (alice owns it, bob edits, carol views, `acme/app`
//! linked) turns them on, and chant's signed events open, comment on and
//! close its red issue. The signature (good, bad, old, repeated), one
//! answer for every refusal, one project's key on another's URL, the
//! lifecycle and its order, inert text, the setting's people, once-only
//! events, off and on again, and the limits. Runs on SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use arugula_e2e::{now_ms, team::TeamRole};
use arugula_platform::PlatformPort as _;
use serde_json::{Value, json};

use crate::{
    Platform,
    chat_tests::{TestHub, call},
    ci_alerts::{BODY_MAX, FAIL_AFTER_WRITE, PER_PROJECT, inert, sign},
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

/// The platform with a key to seal with, t1's project with `acme/app`
/// linked, alerts on (alice made the key) and chat on for t1.
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
    app.db
        .c()
        .execute(
            "INSERT INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_by, linked_at)
             VALUES (77, 'acme/app', ?1, ?2, 9, 'alice', 1)",
            params![false, project],
        )
        .unwrap();
    let key = make_key(&base, "alice", &project).await;
    World { hub, app, base, project, key }
}

async fn make_key(base: &str, who: &str, project: &str) -> String {
    let (s, v) = call(base, who, "POST", &format!("/api/projects/{project}/ci-alerts/key"), None).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["alerts"]["on"], true, "{v}");
    v["key"].as_str().unwrap().to_owned()
}

/// Each event a second after the one before, from an hour ago.
static CLOCK: AtomicU64 = AtomicU64::new(0);

fn next_at() -> u64 {
    let start = now_ms() - 3_600_000;
    let _ = CLOCK.compare_exchange(0, start, Ordering::SeqCst, Ordering::SeqCst);
    CLOCK.fetch_add(1_000, Ordering::SeqCst)
}

fn iso(ms: u64) -> String {
    jiff::Timestamp::from_millisecond(ms as i64).unwrap().to_string()
}

fn event_at(id: &str, kind: &str, ci: Value, at: u64) -> Value {
    json!({
        "schema": "chant.notify/v1", "id": id, "event": kind, "at": iso(at),
        "source": "chant", "project": "acme/app", "summary": format!("main {kind}"), "targets": [],
        "ci": ci, "run": { "forge": "github", "repository": "acme/app", "commit": RED },
    })
}

fn event(id: &str, kind: &str, ci: Value) -> Value {
    event_at(id, kind, ci, next_at())
}

fn red(id: &str) -> Value {
    event(id, "ci.red", json!({ "branch": "main", "commit": RED }))
}

/// Post `body` as chant would, at `ts` (seconds), signed as given.
async fn post_raw(base: &str, project: &str, body: &[u8], ts: u64, signature: Option<String>) -> (u16, Value) {
    let mut r = reqwest::Client::new()
        .post(format!("{base}/ci/notify/{project}"))
        .header("content-type", "application/json")
        .header("x-chant-timestamp", ts.to_string())
        .body(body.to_vec());
    if let Some(s) = signature {
        r = r.header("x-chant-signature", s);
    }
    let r = r.send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or_default())
}

async fn post(base: &str, project: &str, key: &str, body: &Value) -> (u16, Value) {
    let bytes = serde_json::to_vec(body).unwrap();
    let ts = now_ms() / 1000;
    post_raw(base, project, &bytes, ts, Some(sign(key, &ts.to_string(), &bytes))).await
}

async fn did(w: &World, body: &Value) -> Value {
    let (s, v) = post(&w.base, &w.project, &w.key, body).await;
    assert_eq!(s, 200, "{v}");
    v
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

/// `text` with its code spans (a run of backticks to the next run of the
/// same length) left out, as markdown reads them.
fn without_code(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let (mut out, mut i) = (String::new(), 0);
    while i < b.len() {
        if b[i] != '`' {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let n = b[i..].iter().take_while(|c| **c == '`').count();
        let mut j = i + n;
        let close = loop {
            if j >= b.len() {
                break None;
            }
            let m = b[j..].iter().take_while(|c| **c == '`').count();
            if m == n {
                break Some(j + m);
            }
            j += m.max(1);
        };
        match close {
            Some(end) => {
                out.push_str("CODE");
                i = end;
            }
            None => {
                out.extend(&b[i..]);
                break;
            }
        }
    }
    out
}

fn open_issues(w: &World, branch: &str) -> i64 {
    w.app
        .db
        .c()
        .query_row(
            "SELECT count(*) FROM issues i JOIN ci_alert_issues c ON c.issue_id = i.id
             WHERE c.project_id = ?1 AND c.branch = ?2 AND i.state = 'open'",
            params![w.project, branch],
            |r| r.get(0),
        )
        .unwrap()
}

#[tokio::test]
async fn the_signature_is_checked_and_a_repeat_dropped() {
    let w = world().await;
    let e1 = red("e1");
    let bytes = serde_json::to_vec(&e1).unwrap();
    let now = now_ms() / 1000;

    // No signature, a bad one, one with another key, one over another body:
    // each the same answer.
    let refused = (401, json!({ "error": "refused" }));
    assert_eq!(post_raw(&w.base, &w.project, &bytes, now, None).await, refused);
    assert_eq!(post_raw(&w.base, &w.project, &bytes, now, Some("sha256=00".into())).await, refused);
    let other = sign("not-the-key", &now.to_string(), &bytes);
    assert_eq!(post_raw(&w.base, &w.project, &bytes, now, Some(other)).await, refused);
    let mut changed = e1.clone();
    changed["ci"]["branch"] = json!("release");
    let over_red = sign(&w.key, &now.to_string(), &bytes);
    let changed = serde_json::to_vec(&changed).unwrap();
    assert_eq!(post_raw(&w.base, &w.project, &changed, now, Some(over_red)).await, refused);
    // Signed, but six minutes old, or six minutes ahead.
    for ts in [now - 360, now + 360] {
        let sig = sign(&w.key, &ts.to_string(), &bytes);
        assert_eq!(post_raw(&w.base, &w.project, &bytes, ts, Some(sig)).await, refused);
    }
    // A project that isn't there, and one with alerts never on: the same.
    assert_eq!(post(&w.base, "prj_nothere", &w.key, &e1).await, refused);
    w.hub.team("t3", &[("alice", TeamRole::Owner)]);
    let never = crate::projects::team_project(&w.app, "t3").await.unwrap().unwrap();
    assert_eq!(post(&w.base, &never, &w.key, &e1).await, refused);
    assert_eq!(w.app.db.ci_alert_opens(&w.project).unwrap().len(), 0, "nothing refused wrote anything");

    // Good: it opens the issue. The same id again is a no-op.
    let v = did(&w, &e1).await;
    assert_eq!(v["did"], "opened");
    let again = did(&w, &e1).await;
    assert_eq!(again["duplicate"], true);
    assert_eq!(comments(&w, v["issue"].as_str().unwrap()).await.len(), 0, "the repeat said nothing");

    // Events it doesn't act on are taken and ignored.
    for kind in ["waiting", "ci.quarantine-passed", "drift"] {
        let v = did(&w, &event(&format!("x-{kind}"), kind, json!({}))).await;
        assert_eq!(v["ignored"], true, "{kind}: {v}");
    }
    // Not chant.notify/v1, or no `at`.
    let mut bad = red_on("e9", "main");
    bad["schema"] = json!("chant.notify/v2");
    assert_eq!(post(&w.base, &w.project, &w.key, &bad).await.0, 400);
    let mut no_at = red_on("e10", "main");
    no_at["at"] = json!("yesterday");
    assert_eq!(post(&w.base, &w.project, &w.key, &no_at).await.0, 400);
}

fn red_on(id: &str, branch: &str) -> Value {
    event(id, "ci.red", json!({ "branch": branch, "commit": RED }))
}

#[tokio::test]
async fn one_projects_key_cant_post_to_another() {
    let w = world().await;
    // t2's project, alice's too, with its own key.
    w.hub.team("t2", &[("alice", TeamRole::Owner)]);
    let other = crate::projects::team_project(&w.app, "t2").await.unwrap().unwrap();
    let other_key = make_key(&w.base, "alice", &other).await;
    assert_ne!(other_key, w.key);
    let e1 = red("e1");
    assert_eq!(post(&w.base, &w.project, &other_key, &e1).await.0, 401);
    assert_eq!(post(&w.base, &other, &w.key, &e1).await.0, 401);
    assert!(w.app.db.ci_alert_opens(&w.project).unwrap().is_empty());
    assert!(w.app.db.ci_alert_opens(&other).unwrap().is_empty());
    // Each with its own.
    assert_eq!(post(&w.base, &other, &other_key, &e1).await.1["did"], "opened");

    // Rotated: the old key stops at once.
    let new = make_key(&w.base, "bob", &w.project).await;
    let red2 = red("e2");
    assert_eq!(post(&w.base, &w.project, &w.key, &red2).await.0, 401);
    assert_eq!(post(&w.base, &w.project, &new, &red2).await.1["did"], "opened");
}

/// Off removes the key and only the key: the open issue and the people
/// carry on when it's on again, and a green then closes the old issue.
#[tokio::test]
async fn off_and_on_again_keeps_the_open_issue() {
    let w = world().await;
    let path = format!("/api/projects/{}/ci-alerts", w.project);
    let (s, _) = call(&w.base, "alice", "PATCH", &path, Some(json!({ "people": ["bob"] }))).await;
    assert_eq!(s, 200);
    let opened = did(&w, &red("r1")).await;
    let id = opened["issue"].as_str().unwrap().to_owned();
    let (s, v) = call(&w.base, "bob", "PATCH", &path, Some(json!({ "on": false }))).await;
    assert_eq!((s, &v["on"]), (200, &json!(false)), "{v}");
    assert_eq!(v["open"][0]["issue"], id.as_str(), "off keeps the open issue: {v}");
    assert_eq!(v["people"], json!(["bob"]));
    let kept: i64 = w
        .app
        .db
        .c()
        .query_row("SELECT count(*) FROM project_secrets WHERE project_id = ?1", params![w.project], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 0, "the key is gone");
    assert_eq!(post(&w.base, &w.project, &w.key, &red("r2")).await.0, 401, "off: refused");
    let (s, _) = call(&w.base, "alice", "PATCH", &path, Some(json!({ "on": true }))).await;
    assert_eq!(s, 422, "on is by making a key");

    let key = make_key(&w.base, "alice", &w.project).await;
    let w = World { key, ..w };
    let green = event("g1", "ci.green", json!({ "branch": "main", "commit": GREEN, "red": RED }));
    let v = did(&w, &green).await;
    assert_eq!((v["did"].as_str(), v["issue"].as_str()), (Some("closed"), Some(id.as_str())), "{v}");
}

#[tokio::test]
async fn red_opens_events_comment_and_green_closes() {
    let w = world().await;
    let red = |id: &str| {
        event(
            id,
            "ci.red",
            json!({ "branch": "main", "commit": RED, "phase": "test",
                "failedRun": { "name": "test-2", "conclusion": "failure", "url": "https://github.com/acme/app/actions/runs/7/job/8" } }),
        )
    };
    let v = did(&w, &red("r1")).await;
    assert_eq!(v["did"], "opened", "{v}");
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
    assert!(
        body.contains(
            "Phase `test`: `test-2` (`failure`), [the failed run](https://github.com/acme/app/actions/runs/7/job/8)."
        ),
        "{body}"
    );
    assert!(body.contains("[`aaaaaaaaaaaa`](https://github.com/acme/app/commit/aaaa"), "{body}");

    // Red again: a comment on the same issue.
    let v = did(&w, &red("r2")).await;
    assert_eq!((v["did"].as_str(), v["issue"].as_str()), (Some("commented"), Some(id.as_str())), "{v}");
    // The culprit and stale: comments.
    let culprit = event("c1", "ci.culprit", json!({ "branch": "main", "commit": RED, "culprit": CULPRIT }));
    let v = did(&w, &culprit).await;
    assert_eq!((v["did"].as_str(), v["issue"].as_str()), (Some("commented"), Some(id.as_str())), "{v}");
    let stale = event(
        "s1",
        "ci.stale",
        json!({ "branch": "main", "commit": RED, "since": CULPRIT, "lastGreen": GREEN, "hours": 7 }),
    );
    assert_eq!(did(&w, &stale).await["did"], "commented");

    // Green: closed as completed by chant CI, and a comment.
    let green = event("g1", "ci.green", json!({ "branch": "main", "commit": GREEN, "red": RED }));
    let v = did(&w, &green).await;
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
    assert!(texts[1].contains("bbbbbbbbbbbb") && texts[1].contains("turned `main` red"), "{}", texts[1]);
    assert!(texts[2].contains("no green commit for 7 hours"), "{}", texts[2]);
    assert!(texts[3].contains("green again at") && texts[3].contains("cccccccccccc"), "{}", texts[3]);

    // A red after green opens a new issue.
    let v = did(&w, &red("r3")).await;
    assert_eq!(v["did"], "opened", "{v}");
    assert_ne!(v["issue"].as_str(), Some(id.as_str()));
    assert_eq!(v["number"], 2);
    // Another branch has its own.
    let o = did(&w, &red_on("r4", "release")).await;
    assert_eq!(o["did"], "opened", "{o}");
    assert_eq!(issue(&w, o["issue"].as_str().unwrap()).await["title"], "release is red at aaaaaaaaaaaa");

    // An issue a person closed is left closed: the next red opens another.
    let now_open = v["issue"].as_str().unwrap();
    let (s, _) =
        call(&w.base, "alice", "PATCH", &format!("/api/issues/{now_open}"), Some(json!({ "state": "closed" }))).await;
    assert_eq!(s, 200);
    let v = did(&w, &red("r5")).await;
    assert_eq!(v["did"], "opened", "{v}");

    // The settings say which are open.
    let (_, s) = call(&w.base, "carol", "GET", &format!("/api/projects/{}/ci-alerts", w.project), None).await;
    let branches: Vec<&str> = s["open"].as_array().unwrap().iter().map(|o| o["branch"].as_str().unwrap()).collect();
    assert_eq!(branches, ["release", "main"], "{s}");
    assert_eq!(s["url"], format!("https://control.example/ci/notify/{}", w.project));
    assert!(s.get("key").is_none());
}

/// Order is the events' `at`: a culprit, stale or red from before the
/// branch's newest green changes nothing; culprit and stale never open an
/// issue; a green closes only an issue opened before it.
#[tokio::test]
async fn late_events_change_nothing() {
    let w = world().await;
    let t0 = next_at();
    let [t_red, t_culprit, t_stale, t_green] = [t0 + 10_000, t0 + 20_000, t0 + 30_000, t0 + 40_000];
    for _ in 0..60 {
        next_at();
    }
    let opened = did(&w, &event_at("r1", "ci.red", json!({ "branch": "main", "commit": RED }), t_red)).await;
    let id = opened["issue"].as_str().unwrap().to_owned();
    let green = did(&w, &event_at("g1", "ci.green", json!({ "branch": "main", "commit": GREEN }), t_green)).await;
    assert_eq!(green["did"], "closed");
    // Late: each sent before the green, arriving after it.
    for (n, kind, at) in [("c1", "ci.culprit", t_culprit), ("s1", "ci.stale", t_stale), ("r2", "ci.red", t_red + 1)] {
        let ci = json!({ "branch": "main", "commit": RED, "culprit": CULPRIT, "since": RED, "hours": 6 });
        let v = did(&w, &event_at(n, kind, ci, at)).await;
        assert_eq!(v["ignored"], true, "{kind}: {v}");
    }
    assert_eq!(open_issues(&w, "main"), 0);
    assert_eq!(comments(&w, &id).await.len(), 1, "only the green's");
    // A culprit or stale after the green, with nothing open: nothing.
    for (n, kind) in [("c2", "ci.culprit"), ("s2", "ci.stale")] {
        let ci = json!({ "branch": "main", "commit": RED, "culprit": CULPRIT, "since": RED, "hours": 6 });
        assert_eq!(did(&w, &event(n, kind, ci)).await["ignored"], true, "{kind}");
    }
    assert_eq!(open_issues(&w, "main"), 0, "culprit and stale never open one");

    // A green racing a red: the red sent after the green arrives first and
    // opens an issue; the older green comes after and leaves it open.
    let t1 = next_at();
    let t2 = next_at();
    let newer = did(&w, &event_at("r3", "ci.red", json!({ "branch": "main", "commit": CULPRIT }), t2)).await;
    assert_eq!(newer["did"], "opened");
    let older_green = did(&w, &event_at("g2", "ci.green", json!({ "branch": "main", "commit": GREEN }), t1)).await;
    assert_eq!(older_green["ignored"], true, "{older_green}");
    assert_eq!(open_issues(&w, "main"), 1);
    // And the other way: the green sent after the red arrives first, so the
    // red is late.
    let g = did(&w, &event("g3", "ci.green", json!({ "branch": "main", "commit": GREEN }))).await;
    assert_eq!(g["did"], "closed");
    let t3 = next_at();
    let t4 = next_at();
    did(&w, &event_at("g4", "ci.green", json!({ "branch": "main", "commit": GREEN }), t4)).await;
    let v = did(&w, &event_at("r4", "ci.red", json!({ "branch": "main", "commit": RED }), t3)).await;
    assert_eq!(v["ignored"], true, "{v}");
    assert_eq!(open_issues(&w, "main"), 0);
}

/// Nothing from the event (summary, branch, phase, run name, conclusion,
/// URL) or from GitHub can mention anyone or link, show an image or
/// emphasise: each is a code span with its `@` broken. The only links are
/// the platform's own on the linked repo.
#[tokio::test]
async fn event_text_is_inert() {
    let w = world().await;
    let path = format!("/api/projects/{}/ci-alerts", w.project);
    let (s, _) = call(&w.base, "alice", "PATCH", &path, Some(json!({ "people": ["bob"] }))).await;
    assert_eq!(s, 200);
    let mut e = event(
        "r1",
        "ci.red",
        json!({ "branch": "fix/@alice", "commit": RED, "phase": "test @carol",
            "failedRun": { "name": "notify @alice [x](https://evil.example) ![i](https://evil.example/i.png)",
                "conclusion": "**@carol**", "url": "https://evil.example/acme/app/actions/runs/1" } }),
    );
    e["summary"] = json!("ask @alice: [click](https://evil.example) ![img](https://evil.example/x.png) `tick` _em_");
    let v = did(&w, &e).await;
    let id = v["issue"].as_str().unwrap().to_owned();
    let body = issue(&w, &id).await["body"].as_str().unwrap().to_owned();
    // Only bob, whom the setting names, hears of it.
    assert!(inbox(&w.app, "alice").is_empty(), "{body}");
    assert!(inbox(&w.app, "carol").is_empty(), "{body}");
    assert_eq!(inbox(&w.app, "bob"), [(id.clone(), "mentioned".into())]);
    assert!(!body.contains("evil.example/acme"), "a run URL off the repo isn't linked: {body}");
    // Outside its code spans, every link is the platform's own, on
    // github.com/acme/app, and there's no image or emphasis.
    let prose = without_code(&body);
    assert_eq!(prose.matches("](").count(), 1, "the commit's, not the run off the repo: {prose}");
    for (i, _) in prose.match_indices("](") {
        assert!(prose[i..].starts_with("](https://github.com/acme/app/"), "{prose}");
    }
    let prose = prose.replace("cc @bob", "");
    assert!(!prose.contains("![") && !prose.contains("**") && !prose.contains('@'), "{prose}");
    // Each of the event's words is a code span.
    for s in ["fix/@alice", "test @carol", "**@carol**"] {
        assert!(body.contains(&inert(s)), "{s}: {body}");
    }
    assert!(
        body.contains(&inert(
            "ask @alice: [click](https://evil.example) ![img](https://evil.example/x.png) `tick` _em_"
        ))
    );
    assert_eq!(inert("a `b` c"), "``a `b` c``");
    assert_eq!(inert("`x`"), "`` `x` ``");
    assert_eq!(inert("@x"), "`@\u{2060}x`");

    // The comments too: a culprit's and a stale's words.
    let ci = json!({ "branch": "fix/@alice", "commit": RED, "culprit": CULPRIT, "since": RED, "hours": 6 });
    for (n, kind) in [("c1", "ci.culprit"), ("s1", "ci.stale")] {
        let mut e = event(n, kind, ci.clone());
        e["ci"]["branch"] = json!("main");
        // An inert branch on main's issue: nothing names alice.
        e["summary"] = json!("@alice");
        did(&w, &e).await;
    }
    assert!(inbox(&w.app, "alice").is_empty());
}

#[tokio::test]
async fn the_setting_mentions_and_assigns_its_people() {
    let w = world().await;
    let path = format!("/api/projects/{}/ci-alerts", w.project);
    // A viewer changes nothing.
    let (s, _) = call(&w.base, "carol", "PATCH", &path, Some(json!({ "people": ["carol"] }))).await;
    assert_eq!(s, 403);
    let (s, _) = call(&w.base, "carol", "POST", &format!("{path}/key"), None).await;
    assert_eq!(s, 403);
    // Someone outside the project is left out, not refused.
    let (s, v) = call(
        &w.base,
        "alice",
        "PATCH",
        &path,
        Some(json!({ "people": ["bob", "mallory", "carol", "bob"], "assign": true })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!((&v["people"], &v["assign"]), (&json!(["bob", "carol"]), &json!(true)));

    let v = did(&w, &red("r1")).await;
    let id = v["issue"].as_str().unwrap().to_owned();
    let i = issue(&w, &id).await;
    assert_eq!(i["assignees"], json!(["bob", "carol"]));
    assert!(i["body"].as_str().unwrap().contains("cc @bob @carol"), "{}", i["body"]);
    assert_eq!(inbox(&w.app, "bob"), [(id.clone(), "assigned".into()), (id.clone(), "mentioned".into())]);
    assert_eq!(inbox(&w.app, "carol"), [(id.clone(), "assigned".into()), (id.clone(), "mentioned".into())]);
    // One push each, saying chant CI did it.
    let mut p = w.hub.pushes();
    for _ in 0..100 {
        if p.len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        p = w.hub.pushes();
    }
    let mut to: Vec<&str> = p.iter().map(|(a, _, _)| a.as_str()).collect();
    to.sort();
    assert_eq!(to, ["bob", "carol"]);
    assert!(p[0].1["title"].as_str().unwrap().contains("assigned to you by chant CI"), "{:?}", p[0].1);

    // A stale comment mentions them in the thread: an entry each.
    w.app.db.c().execute("DELETE FROM inbox_items", params![]).unwrap();
    let stale = event("s1", "ci.stale", json!({ "branch": "main", "commit": RED, "since": RED, "hours": 6 }));
    assert_eq!(did(&w, &stale).await["did"], "commented");
    assert_eq!(inbox(&w.app, "bob"), [(id.clone(), "mentioned".into())]);
    assert_eq!(inbox(&w.app, "carol"), [(id.clone(), "mentioned".into())]);
    // Red again is said in the thread without telling them.
    w.app.db.c().execute("DELETE FROM inbox_items", params![]).unwrap();
    assert_eq!(did(&w, &red("r2")).await["did"], "commented");
    assert!(inbox(&w.app, "bob").is_empty());

    // Carol leaves the team: the roster's change takes her off the
    // setting, the settings no longer list her, and she's told no more.
    w.hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor)]);
    w.app.roster_changed("t1").await.unwrap();
    let kept: Vec<String> = w
        .app
        .db
        .c()
        .query_map("SELECT account FROM ci_alert_people WHERE project_id = ?1", params![w.project], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, ["bob"]);
    let (_, v) = call(&w.base, "alice", "GET", &path, None).await;
    assert_eq!(v["people"], json!(["bob"]));
    w.app.db.c().execute("DELETE FROM inbox_items", params![]).unwrap();
    let green = event("g1", "ci.green", json!({ "branch": "main", "commit": GREEN }));
    assert_eq!(did(&w, &green).await["did"], "closed");
    assert_eq!(inbox(&w.app, "bob"), [(id.clone(), "mentioned".into())]);
    assert!(inbox(&w.app, "carol").is_empty());
    // Someone who left is dropped on read too, before any roster hook: the
    // UI sends the list back as it reads it.
    w.app.db.set_ci_alerts(&w.project, Some(&["bob".into(), "carol".into()]), None).unwrap();
    let (_, v) = call(&w.base, "alice", "GET", &path, None).await;
    assert_eq!(v["people"], json!(["bob"]));
    let (s, v) = call(&w.base, "alice", "PATCH", &path, Some(json!({ "people": v["people"] }))).await;
    assert_eq!((s, &v["people"]), (200, &json!(["bob"])));
}

/// What follows an event's first write failing (a push, a mention) leaves
/// the event noted: chant's retry is a duplicate, with no second comment or
/// push.
#[tokio::test]
async fn a_failure_after_the_write_isnt_repeated() {
    let w = world().await;
    let path = format!("/api/projects/{}/ci-alerts", w.project);
    call(&w.base, "alice", "PATCH", &path, Some(json!({ "people": ["bob"] }))).await;
    let id = did(&w, &red("r1")).await["issue"].as_str().unwrap().to_owned();
    let stale = event("s1", "ci.stale", json!({ "branch": "main", "commit": RED, "since": RED, "hours": 6 }));
    FAIL_AFTER_WRITE.store(true, Ordering::SeqCst);
    let first = post(&w.base, &w.project, &w.key, &stale).await;
    let again = post(&w.base, &w.project, &w.key, &stale).await;
    let green = event("g1", "ci.green", json!({ "branch": "main", "commit": GREEN }));
    let closed = post(&w.base, &w.project, &w.key, &green).await;
    let green_again = post(&w.base, &w.project, &w.key, &green).await;
    FAIL_AFTER_WRITE.store(false, Ordering::SeqCst);
    assert_eq!(first.1["did"], "commented", "{first:?}");
    assert_eq!(again.1["duplicate"], true, "{again:?}");
    assert_eq!(closed.1["did"], "closed", "{closed:?}");
    assert_eq!(green_again.1["duplicate"], true, "{green_again:?}");
    let said = comments(&w, &id).await;
    assert_eq!(said.len(), 2, "the stale's and the green's, once each: {said:?}");
    assert_eq!(issue(&w, &id).await["state"], "closed");
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
        assert_eq!(open_issues(&w, &branch), 1, "{branch}");
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
    use crate::db::ci_alerts::{Created, Noted};
    let w = world().await;
    let by = crate::db::issues::Actor { account: "", agent: Some("ci:chant") };
    let new = |title: &'static str| crate::db::issues::NewIssue {
        project: &w.project,
        title,
        body: "",
        labels: &[],
        assignees: &[],
    };
    let n = |d: &'static str| Noted { delivery: d, event: "ci.red", now: 1 };
    let Created::Opened(first) = w.app.db.create_ci_issue(&new("one"), by, "main", (RED, 1), n("d1")).unwrap() else {
        panic!()
    };
    assert!(matches!(w.app.db.create_ci_issue(&new("two"), by, "main", (RED, 2), n("d2")).unwrap(), Created::Taken));
    assert!(w.app.db.issue_by_number(&w.project, 2).unwrap().is_none(), "no issue, and no number, for the refused one");
    assert!(w.app.db.delivery_event("d2").unwrap().is_none(), "nor its event noted");
    assert!(matches!(w.app.db.create_ci_issue(&new("one"), by, "x", (RED, 1), n("d1")).unwrap(), Created::Applied));
    // Another branch is its own.
    assert!(matches!(
        w.app.db.create_ci_issue(&new("other"), by, "release", (RED, 3), n("d3")).unwrap(),
        Created::Opened(_)
    ));
    w.app.db.ci_alert_closed(&w.project, "main", &first.id, 4).unwrap();
    let Created::Opened(again) = w.app.db.create_ci_issue(&new("three"), by, "main", (GREEN, 5), n("d4")).unwrap()
    else {
        panic!()
    };
    assert_eq!(w.app.db.ci_alert_open(&w.project, "main").unwrap().unwrap().issue, again.id);
}

/// A body over 64 KiB is refused before anything is read, and signed events
/// past the project's hourly share wait.
#[tokio::test]
async fn the_endpoints_limits() {
    let w = world().await;
    let mut big = red("big");
    big["detail"] = json!({ "pad": "x".repeat(BODY_MAX) });
    assert_eq!(post(&w.base, &w.project, &w.key, &big).await.0, 413);
    while w.app.limits.check_client(PER_PROJECT, &w.project).is_ok() {}
    assert_eq!(post(&w.base, &w.project, &w.key, &red("r1")).await.0, 429);
    assert!(w.app.db.ci_alert_opens(&w.project).unwrap().is_empty());
}
