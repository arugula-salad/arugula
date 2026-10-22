//! Mentions and assignments in the inbox (M202, `crate::inbox`) in M165's
//! world (`viewer_tests::world`), with chat on for team t1: each reason on
//! a native issue, a mention in a mirrored item's thread, who may see
//! them, one push for a thread's mention (never chat's too), agents'
//! mentions left out, and Seen and Done. Runs on SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL` (`just test-platform-postgres`), Postgres.

use std::{sync::Arc, time::Duration};

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use super::{links_tests::serve, viewer_tests::world};
use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::sql::{Sql as _, params},
    hub::PushOptions,
};

/// Every entry, as `account item reason`, sorted.
fn rows(app: &Platform) -> Vec<String> {
    app.db
        .c()
        .query_map("SELECT account, item, reason FROM inbox_items ORDER BY 1, 2, 3", params![], |r| {
            Ok(format!("{} {} {}", r.get::<String>(0)?, r.get::<String>(1)?, r.get::<String>(2)?))
        })
        .unwrap()
}

/// The pushes so far, once those in flight have gone (they go in the
/// background).
async fn pushes(hub: &TestHub) -> Vec<(String, Value, PushOptions)> {
    tokio::time::sleep(Duration::from_millis(300)).await;
    hub.pushes()
}

/// The world, with chat (and so native issues) on for t1: its project.
async fn setup() -> (Arc<TestHub>, Arc<Platform>, String, String) {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    hub.flag("team:t1", true);
    app.cache.drop_all();
    (hub, app, base, team)
}

async fn new_issue(base: &str, who: &str, project: &str, body: Value) -> String {
    let (st, v) = call(base, who, "POST", &format!("/api/projects/{project}/issues"), Some(body)).await;
    assert_eq!(st, 200, "{v}");
    v["id"].as_str().unwrap().to_owned()
}

async fn patch(base: &str, who: &str, id: &str, body: Value) {
    let (st, v) = call(base, who, "PATCH", &format!("/api/issues/{id}"), Some(body)).await;
    assert_eq!(st, 200, "{v}");
}

/// The thread on an object, opened by `who`: its room.
async fn thread(base: &str, who: &str, target: &str) -> String {
    let (st, v) =
        call(base, who, "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": target }))).await;
    assert_eq!(st, 200, "{v}");
    v["id"].as_str().unwrap().to_owned()
}

async fn say(base: &str, who: &str, room: &str, text: &str) -> Value {
    let (st, v) =
        call(base, who, "POST", &format!("/api/chat/rooms/{room}/messages"), Some(json!({ "text": text }))).await;
    assert_eq!(st, 200, "{v}");
    v
}

async fn inbox(base: &str, who: &str, query: &str) -> Vec<Value> {
    let (st, v) = call(base, who, "GET", &format!("/api/inbox{query}"), None).await;
    assert_eq!(st, 200, "{v}");
    v["entries"].as_array().cloned().unwrap_or_default()
}

/// Opening an issue gives its assignees and those its body mentions an
/// entry and one push each (not its author, nor anyone outside the
/// project); an edit adds those newly assigned or mentioned, and an
/// assignee taken off loses theirs.
#[tokio::test]
async fn assignments_and_mentions_on_a_native_issue() {
    let (hub, app, base, team) = setup().await;
    let body =
        json!({ "title": "The canary fails", "body": "@carol can you look? cc @eve @alice", "assignees": ["bob"] });
    let id = new_issue(&base, "alice", &team, body).await;
    assert_eq!(rows(&app), [format!("bob {id} assigned"), format!("carol {id} mentioned")], "not eve, not alice");
    let sent = pushes(&hub).await;
    assert_eq!(sent.len(), 2, "{sent:?}");
    let bob = sent.iter().find(|(a, ..)| a == "bob").unwrap();
    assert_eq!(bob.1["title"], json!("t1#1 assigned to you by alice"), "{:?}", bob.1);
    assert_eq!((&bob.1["body"], &bob.1["inbox"]), (&json!("The canary fails"), &json!({ "item": id })));
    let carol = sent.iter().find(|(a, ..)| a == "carol").unwrap();
    assert_eq!(carol.1["title"], json!("t1#1 alice mentioned you"));

    // carol is given it too; bob is taken off; alice gives it herself.
    app.inbox.forget_pushes();
    patch(&base, "alice", &id, json!({ "assignees": { "set": ["carol", "alice"] } })).await;
    assert_eq!(rows(&app), [format!("carol {id} assigned"), format!("carol {id} mentioned")]);
    assert_eq!(pushes(&hub).await.len(), 3, "carol's, once");

    // An edit pushes only whom it newly mentions.
    app.inbox.forget_pushes();
    patch(&base, "alice", &id, json!({ "body": "@carol can you look? @bob too" })).await;
    let sent = pushes(&hub).await;
    assert_eq!(sent.len(), 4, "{sent:?}");
    assert_eq!((&sent[3].0, &sent[3].1["title"]), (&"bob".to_owned(), &json!("t1#1 alice mentioned you")));
    patch(&base, "alice", &id, json!({ "title": "The canary still fails" })).await;
    assert_eq!(pushes(&hub).await.len(), 4, "nothing new mentioned");

    // On the page: the issue, by its project's name, linking to its path.
    let e = inbox(&base, "carol", "").await;
    assert_eq!(e.len(), 2, "{e:?}");
    assert_eq!(
        (&e[0]["item"], &e[0]["repo"], &e[0]["number"], &e[0]["project"], &e[0]["url"], &e[0]["title"]),
        (
            &json!(id),
            &json!("t1"),
            &json!(1),
            &json!(team),
            &json!(format!("/issues/{id}")),
            &json!("The canary still fails")
        )
    );
    assert_eq!(e[0]["author"], json!("alice"));
}

/// A mention in an issue's thread is one entry and one push: the inbox's,
/// at the message, and not chat's too. Seen and Done work on it, and a
/// new mention brings a done one back.
#[tokio::test]
async fn a_mention_in_an_issues_thread_pushes_once() {
    let (hub, app, base, team) = setup().await;
    let id = new_issue(&base, "alice", &team, json!({ "title": "Flaky test" })).await;
    assert!(pushes(&hub).await.is_empty());
    let room = thread(&base, "bob", &id).await;
    let v = say(&base, "bob", &room, "@alice it's the clock").await;
    assert_eq!(v["pushed"], json!([{ "account": "alice", "why": "mention" }]), "{v}");
    let sent = pushes(&hub).await;
    assert_eq!(sent.len(), 1, "one push, not chat's and the inbox's: {sent:?}");
    let (to, msg, _) = &sent[0];
    assert_eq!(to, "alice");
    assert_eq!(msg["inbox"], json!({ "item": id, "room": room, "seq": 1 }), "{msg}");
    assert_eq!((&msg["title"], &msg["body"]), (&json!("t1#1 bob mentioned you"), &json!("@alice it's the clock")));
    assert!(msg.get("chat").is_none(), "{msg}");
    assert_eq!(rows(&app), [format!("alice {id} mentioned")]);

    // Seen, then Done: out of the list, until a new mention.
    let path = format!("/api/inbox/{id}");
    let mark = |b: Value| call(&base, "alice", "POST", &path, Some(b));
    let (st, v) = mark(json!({ "seen": true })).await;
    assert_eq!((st, &v["marked"]), (200, &json!(1)), "{v}");
    assert!(inbox(&base, "alice", "").await[0]["seen_at"].is_u64());
    mark(json!({ "reason": "mentioned", "done": true })).await;
    assert!(inbox(&base, "alice", "").await.is_empty());
    assert!(inbox(&base, "alice", "?done=true").await[0]["done_at"].is_u64());
    app.inbox.forget_pushes();
    say(&base, "carol", &room, "@alice and the timezone").await;
    let e = inbox(&base, "alice", "").await;
    assert_eq!((e.len(), &e[0]["seen_at"], &e[0]["done_at"]), (1, &Value::Null, &Value::Null), "{e:?}");
    assert_eq!(pushes(&hub).await.len(), 2);

    // Within a minute: an entry, no push. With the thread muted: no push.
    say(&base, "bob", &room, "@alice again").await;
    assert_eq!(pushes(&hub).await.len(), 2);
    call(&base, "alice", "POST", &format!("/api/chat/rooms/{room}/mute"), Some(json!({ "muted": true }))).await;
    app.inbox.forget_pushes();
    let v = say(&base, "bob", &room, "@alice muted?").await;
    assert_eq!((v["pushed"].clone(), pushes(&hub).await.len()), (json!([]), 2));

    // Someone outside the project can't mark it, or learn it's there.
    let (st, _) = call(&base, "eve", "POST", &format!("/api/inbox/{id}"), Some(json!({ "done": true }))).await;
    assert_eq!(st, 404);
}

/// A mention in the thread on a mirrored PR or issue is a `thread_mention`
/// on the item, beside (and kept by) its GitHub reasons; on a private
/// repo's, only those GitHub lets read it are mentioned (M165).
#[tokio::test]
async fn a_mention_in_a_mirrored_items_thread() {
    let (hub, app, base, _) = setup().await;
    let room = thread(&base, "bob", "fi_a2").await;
    say(&base, "bob", &room, "@alice see this").await;
    assert_eq!(rows(&app), ["alice fi_a2 thread_mention"]);
    let sent = pushes(&hub).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].1["title"], json!("issue acme/site#2 bob mentioned you in its thread"));
    assert_eq!(sent[0].1["inbox"], json!({ "item": "fi_a2", "room": room, "seq": 1 }));
    let e = inbox(&base, "alice", "").await;
    assert_eq!(
        (e.len(), &e[0]["reason"], &e[0]["room"], &e[0]["project"]),
        (1, &json!("thread_mention"), &json!(room), &Value::Null)
    );
    // Working out the item's GitHub reasons leaves it.
    super::inbox::recompute(&app, 88, &[2], true).await.unwrap();
    assert_eq!(rows(&app), ["alice fi_a2 thread_mention"]);

    // acme/app is private: carol didn't sign in with GitHub, so isn't
    // mentioned (or let in); bob has access.
    let secret = thread(&base, "alice", "fi_a1").await;
    let (st, _) =
        call(&base, "carol", "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": "fi_a1" }))).await;
    assert_eq!(st, 403);
    let v = say(&base, "alice", &secret, "@carol @bob thoughts?").await;
    assert_eq!(v["unreached"], json!(["carol"]), "{v}");
    assert_eq!(rows(&app), ["alice fi_a2 thread_mention", "bob fi_a1 thread_mention"]);
    assert!(inbox(&base, "carol", "").await.is_empty());
}

/// Only a project's members read its issues' entries, while they're
/// members; a machine's agents reading their owner's inbox get the GitHub
/// reasons, never native issues; and a machine's agents mentioned by name
/// aren't entries or pushes (they read mentions over MCP, M198).
#[tokio::test]
async fn who_sees_native_entries() {
    let (hub, app, base, team) = setup().await;
    hub.daemon("geek", "bob", Some("t1"));
    let (st, v) = call(
        &base,
        "bob",
        "POST",
        "/api/grants",
        Some(json!({ "daemon": "geek", "power": "threads.read", "scope": { "kind": "project", "id": team } })),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    let id = new_issue(&base, "alice", &team, json!({ "title": "Ship it", "assignees": ["bob"] })).await;
    let room = thread(&base, "alice", &id).await;
    say(&base, "alice", &room, "@geek-box what do you think? @geek").await;
    assert_eq!(rows(&app), [format!("bob {id} assigned")], "no entry for an agent");
    assert_eq!(pushes(&hub).await.len(), 1, "and no push");

    assert_eq!(inbox(&base, "bob", "").await.len(), 1);
    let r = reqwest::Client::new().get(format!("{base}/api/inbox")).header("x-test-daemon", "bob geek").send().await;
    let v: Value = r.unwrap().json().await.unwrap();
    assert_eq!(v["entries"], json!([]), "a machine's agents: {v}");
    assert!(inbox(&base, "eve", "").await.is_empty());

    // bob leaves the team: his entry stays, unread by anyone.
    hub.team("t1", &[("alice", TeamRole::Owner), ("carol", TeamRole::Viewer)]);
    app.cache.drop_all();
    assert!(inbox(&base, "bob", "").await.is_empty());
    let (st, _) = call(&base, "bob", "POST", &format!("/api/inbox/{id}"), Some(json!({ "done": true }))).await;
    assert_eq!(st, 404);
}

/// An assignee with chat off (and so no native issues) gets no entry and
/// no push, as a mention doesn't; turned on, the next assignment reaches
/// them.
#[tokio::test]
async fn an_assignee_without_chat_gets_nothing() {
    let (hub, app, base, team) = setup().await;
    hub.flag("account:bob", false);
    app.cache.drop_all();
    let id = new_issue(&base, "alice", &team, json!({ "title": "Ship it", "assignees": ["bob", "carol"] })).await;
    assert_eq!(rows(&app), [format!("carol {id} assigned")], "not bob");
    let sent = pushes(&hub).await;
    assert_eq!(sent.iter().map(|(a, ..)| a.as_str()).collect::<Vec<_>>(), ["carol"], "{sent:?}");

    hub.flag("account:bob", true);
    app.cache.drop_all();
    let other = new_issue(&base, "alice", &team, json!({ "title": "Ship it too", "assignees": ["bob"] })).await;
    assert!(rows(&app).contains(&format!("bob {other} assigned")), "{:?}", rows(&app));
}
