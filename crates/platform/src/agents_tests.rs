//! Agents as principals (M197): a machine's grants to read and post in its
//! owner's team channels and DMs, given and revoked by the owner through
//! `/api/grants`, asked about on every use ([`agent_may`]), never reaching
//! more than the owner reads, and what an agent posts labelled and
//! disclosed to everyone in the room.

// Also the feature crates' test harness (#314, the `testing` feature): its
// helpers are `pub`, and only its own tests use the rest.
#![cfg_attr(not(test), allow(dead_code, unused_imports))]

use std::sync::Arc;

use arugula_control_wire::chat::ChatPost;
use arugula_e2e::team::TeamRole;
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::{
    auth::Machine,
    chat::{agent_object_room, agent_room, objects::PRIVATE_REPO, post_as},
    chat_tests::{TestHub, call, serve},
    db::{
        chat::Room,
        issues::{Actor, NewIssue},
        projects::{ISSUES_WRITE, THREADS_POST, THREADS_READ},
        sql::{Sql as _, params},
    },
    grants::{AGENT_UNGRANTED, AgentPrincipal, AgentScope, agent_may},
};

fn room(id: &str, kind: &str) -> Room {
    Room {
        id: id.into(),
        scope: "team:t".into(),
        kind: kind.into(),
        name: id.into(),
        machine: None,
        target: None,
        created_by: "a".into(),
        created: 1,
        archived: None,
        last_seq: 0,
    }
}

pub fn post(text: &str) -> ChatPost {
    ChatPost { text: text.into(), ..Default::default() }
}

pub fn give(machine: &str, power: &str, scope: Value) -> Option<Value> {
    Some(json!({ "daemon": machine, "power": power, "scope": scope }))
}

/// a owns team t (b and x in it too) and the machine geek; c alone is in
/// team u. #general is t's, and there's a DM between a and b and one
/// between b and x. Chat is on for t. t's and u's default projects.
pub async fn seeded() -> (Arc<TestHub>, Arc<crate::Platform>, String, String, String) {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    hub.team("u", &[("c", TeamRole::Owner)]);
    hub.flag("team:t", true);
    hub.daemon("geek", "a", Some("t"));
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let t = crate::projects::team_project(&app, "t").await.unwrap().unwrap();
    let u = crate::projects::team_project(&app, "u").await.unwrap().unwrap();
    app.db.add_chat_room(&room("ch_general", "channel"), &["a", "b", "x"], Some(&t)).unwrap();
    app.db.add_chat_room(&room("dm_ab", "dm"), &["a", "b"], None).unwrap();
    app.db.add_chat_room(&room("dm_bx", "dm"), &["b", "x"], None).unwrap();
    (hub, app, base, t, u)
}

pub fn geek() -> Machine {
    Machine { account: "a".into(), daemon: "geek".into() }
}

/// Off by default, and refused saying how to grant it; given only by the
/// machine's owner, only over what they read; then the agent acts as
/// "<owner>'s agent on <machine>", until the grant is revoked (the very
/// next call) or the owner stops reading the scope.
#[tokio::test]
async fn an_agent_reaches_what_its_owner_granted_and_reads() {
    let (hub, app, base, t, u) = seeded().await;
    let over_t = AgentScope::Project(t.clone());

    // Nothing is given on join: cloud.drive only.
    let e = agent_may(&app, &geek(), THREADS_POST, &over_t).await.unwrap_err();
    assert_eq!((e.0, e.2), (StatusCode::FORBIDDEN, Some(AGENT_UNGRANTED)));
    let how = format!("`arugula grants add geek-box threads.post --project {t}`");
    assert!(e.1.contains("geek-box has no threads.post grant over t") && e.1.contains(&how), "{}", e.1);
    assert!(e.1.contains("Agents in conversations"), "and the page's switch: {}", e.1);
    let (st, v) = call(&base, "a", "GET", "/api/grants", None).await;
    let powers: Vec<&Value> = v["grants"].as_array().unwrap().iter().map(|g| &g["power"]).collect();
    assert_eq!((st, powers), (200, vec![&json!("cloud.drive")]));
    let e = agent_room(&app, &geek(), THREADS_READ, "dm_ab").await.unwrap_err();
    assert!(e.1.contains("--dms"), "{}", e.1);

    // Only over what a reads, only a's own machine, only the powers there are.
    let grants = "/api/grants";
    let (st, v) =
        call(&base, "a", "POST", grants, give("geek", THREADS_POST, json!({ "kind": "project", "id": u }))).await;
    assert_eq!(st, 403, "a isn't in u: {v}");
    let (st, _) =
        call(&base, "b", "POST", grants, give("geek", THREADS_POST, json!({ "kind": "project", "id": t }))).await;
    assert_eq!(st, 404, "not b's machine");
    let (st, _) = call(&base, "a", "POST", grants, give("geek", ISSUES_WRITE, json!({ "kind": "account" }))).await;
    assert_eq!(st, 400, "issues are a project's");
    let (st, _) = call(&base, "a", "POST", grants, Some(json!({ "daemon": "geek", "power": THREADS_POST }))).await;
    assert_eq!(st, 400, "say over what");
    let (st, _) = call(&base, "a", "POST", grants, give("geek", "pane.drive", json!({ "kind": "account" }))).await;
    assert_eq!(st, 400);

    let (st, posting) =
        call(&base, "a", "POST", grants, give("geek", THREADS_POST, json!({ "kind": "project", "id": t }))).await;
    assert_eq!(st, 200, "{posting}");
    assert_eq!(posting["scope"], json!({ "kind": "project", "id": t, "name": "t" }));
    assert_eq!(posting["subject"]["name"], "geek-box");
    call(&base, "a", "POST", grants, give("geek", THREADS_READ, json!({ "kind": "project", "id": t }))).await;
    let (st, _) = call(&base, "a", "POST", grants, give("geek", THREADS_READ, json!({ "kind": "account" }))).await;
    assert_eq!(st, 200);
    let (_, v) = call(&base, "a", "GET", grants, None).await;
    assert_eq!(v["grants"].as_array().unwrap().len(), 4, "listed with cloud.drive: {v}");

    let me = AgentPrincipal { machine: "geek".into(), machine_name: "geek-box".into(), owner: "a".into() };
    assert_eq!(agent_may(&app, &geek(), THREADS_POST, &over_t).await.unwrap(), me);
    assert!(agent_may(&app, &geek(), THREADS_POST, &AgentScope::Account("a".into())).await.is_err(), "read only there");
    assert!(agent_room(&app, &geek(), THREADS_READ, "dm_ab").await.is_ok());
    assert_eq!(
        agent_room(&app, &geek(), THREADS_READ, "dm_bx").await.unwrap_err().0,
        StatusCode::NOT_FOUND,
        "a's DMs only"
    );
    // Another account's machine by the same id gets nothing from a's grant.
    let other = Machine { account: "c".into(), daemon: "geek".into() };
    assert!(agent_may(&app, &other, THREADS_POST, &AgentScope::Project(u.clone())).await.is_err());

    // It posts, as a's, labelled; b reads the label, and the room says whose
    // agents reach it.
    let (_, _, agent) = agent_room(&app, &geek(), THREADS_POST, "ch_general").await.unwrap();
    let posted = post_as(&app, "a", Some(&agent), "ch_general", post("built it")).await.unwrap();
    let (st, v) = call(&base, "b", "GET", "/api/chat/rooms/ch_general/messages", None).await;
    assert_eq!(st, 200, "{v}");
    // After the line that said its grants (#122).
    let m = v["messages"].as_array().unwrap().last().unwrap();
    assert_eq!((&m["author"], &m["text"]), (&json!("a"), &json!("built it")));
    assert_eq!(m["agent"], json!({ "machine": "geek", "machine_name": "geek-box" }));
    let a_rooms = call(&base, "a", "GET", "/api/chat", None).await.1;
    let general = a_rooms["rooms"].as_array().unwrap().iter().find(|r| r["id"] == "ch_general").unwrap().clone();
    assert_eq!(general["unread"], 1, "a hasn't read what their agent wrote");
    let (_, detail) = call(&base, "b", "GET", "/api/chat/rooms/ch_general", None).await;
    assert_eq!(
        detail["agents"],
        json!([{ "owner": "a", "owner_name": "a", "machine": "geek", "machine_name": "geek-box",
                 "powers": ["threads.post", "threads.read"] }])
    );
    let (_, detail) = call(&base, "b", "GET", "/api/chat/rooms/dm_ab", None).await;
    assert_eq!(detail["agents"][0]["powers"], json!(["threads.read"]), "a DM says so too: {detail}");
    let (_, detail) = call(&base, "b", "GET", "/api/chat/rooms/dm_bx", None).await;
    assert_eq!(detail["agents"], json!([]), "a's grant doesn't reach b and x's DM");
    let path = format!("/api/chat/rooms/ch_general/messages/{}", posted.seq);
    let (st, _) = call(&base, "a", "PATCH", &path, Some(json!({ "text": "I built it" }))).await;
    assert_eq!(st, 403, "nobody rewrites what an agent said");

    // Revoked: refused on the next call; what it wrote stays labelled.
    let id = posting["id"].as_str().unwrap();
    let (st, _) = call(&base, "a", "POST", &format!("/api/grants/{id}/revoke"), None).await;
    assert_eq!(st, 200);
    assert_eq!(agent_room(&app, &geek(), THREADS_POST, "ch_general").await.unwrap_err().2, Some(AGENT_UNGRANTED));
    assert!(agent_room(&app, &geek(), THREADS_READ, "ch_general").await.is_ok(), "only that grant");
    let (_, v) = call(&base, "b", "GET", "/api/chat/rooms/ch_general/messages", None).await;
    assert_eq!(v["messages"].as_array().unwrap().last().unwrap()["agent"]["machine"], "geek");
    let (_, detail) = call(&base, "b", "GET", "/api/chat/rooms/ch_general", None).await;
    assert_eq!(detail["agents"][0]["powers"], json!(["threads.read"]));

    // a leaves t: the read grant is still live, and reaches nothing.
    hub.team("t", &[("b", TeamRole::Owner), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    let e = agent_may(&app, &geek(), THREADS_READ, &over_t).await.unwrap_err();
    assert!(e.1.contains("never reaches more than its owner"), "{}", e.1);
    let (_, detail) = call(&base, "b", "GET", "/api/chat/rooms/ch_general", None).await;
    assert_eq!(detail["agents"], json!([]), "nobody's agents read here now");
}

/// An agent comments on a native issue (M196's object thread, in its
/// project): refused without a grant over the project (and nothing made),
/// then labelled once it has one, with the room saying so; an issue
/// outside the owner's projects isn't there, and a private repo's item is
/// refused to an owner GitHub doesn't let read it (M165).
#[tokio::test]
async fn an_agent_comments_on_a_native_issue() {
    let (hub, app, base, t, u) = seeded().await;
    let issue = |project: &str| {
        let n = NewIssue { project, title: "the canary fails", body: "", labels: &[], assignees: &[] };
        app.db.create_issue(&n, Actor::person("a"), 1).unwrap().unwrap()
    };
    let ours = issue(&t);
    let theirs = issue(&u);
    let c = app.db.c();
    c.execute(
        "INSERT INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_at)
         VALUES (8, 'o/secret', TRUE, ?1, 1, 1)",
        params![t],
    )
    .unwrap();
    c.execute(
        "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
         VALUES ('fi_sec', 8, 4, 'issue', 'open', 'a title', 1, 1, 1)",
        params![],
    )
    .unwrap();
    drop(c);

    let e = agent_object_room(&app, &geek(), THREADS_POST, &ours.id).await.unwrap_err();
    assert_eq!(e.2, Some(AGENT_UNGRANTED), "{}", e.1);
    assert!(e.1.contains(&format!("--project {t}")), "{}", e.1);
    assert_eq!(
        app.db.chat_room_on(&format!("project:{t}"), &ours.id).unwrap(),
        None,
        "an agent without a grant makes nothing"
    );
    let e = agent_object_room(&app, &geek(), THREADS_POST, &theirs.id).await.unwrap_err();
    assert_eq!(e.0, StatusCode::NOT_FOUND, "u's issue isn't a's to see: {}", e.1);

    for power in [THREADS_READ, THREADS_POST] {
        let (st, v) =
            call(&base, "a", "POST", "/api/grants", give("geek", power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{v}");
    }
    let (room, _, agent) = agent_object_room(&app, &geek(), THREADS_POST, &ours.id).await.unwrap();
    assert_eq!((room.kind.as_str(), room.target.as_deref()), ("object", Some(ours.id.as_str())));
    post_as(&app, "a", Some(&agent), &room.id, post("bisected: it's the retry change")).await.unwrap();
    let (again, _, _) = agent_room(&app, &geek(), THREADS_READ, &room.id).await.unwrap();
    assert_eq!(again.id, room.id, "one thread per issue");

    let (st, v) = call(&base, "b", "GET", &format!("/api/chat/rooms/{}/messages", room.id), None).await;
    assert_eq!(st, 200, "{v}");
    let m = &v["messages"][0];
    assert_eq!((&m["author"], &m["agent"]["machine_name"]), (&json!("a"), &json!("geek-box")));
    let (_, detail) = call(&base, "b", "GET", &format!("/api/chat/rooms/{}", room.id), None).await;
    assert_eq!(detail["agents"][0]["powers"], json!(["threads.post", "threads.read"]), "{detail}");

    // An agent that may only write issues is disclosed on an issue's
    // thread, where it applies, and not in the team's channels.
    hub.daemon("mini", "a", Some("t"));
    let (st, v) =
        call(&base, "a", "POST", "/api/grants", give("mini", ISSUES_WRITE, json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{v}");
    let (_, detail) = call(&base, "b", "GET", &format!("/api/chat/rooms/{}", room.id), None).await;
    let on_issue: Vec<(&Value, &Value)> =
        detail["agents"].as_array().unwrap().iter().map(|a| (&a["machine"], &a["powers"])).collect();
    assert_eq!(
        on_issue,
        [(&json!("geek"), &json!(["threads.post", "threads.read"])), (&json!("mini"), &json!(["issues.write"]))]
    );
    let (_, detail) = call(&base, "b", "GET", "/api/chat/rooms/ch_general", None).await;
    assert!(detail["agents"].as_array().unwrap().iter().all(|a| a["machine"] != "mini"), "{detail}");

    let e = agent_object_room(&app, &geek(), THREADS_READ, "fi_sec").await.unwrap_err();
    assert_eq!((e.0, e.2), (StatusCode::FORBIDDEN, Some(PRIVATE_REPO)), "{}", e.1);

    // Chat off for the owner: their agents are refused too, and make nothing.
    hub.flag("account:a", false);
    crate::cache::catch_up(&app).await.unwrap();
    let off = Some(crate::chat::CHAT_OFF);
    assert_eq!(agent_room(&app, &geek(), THREADS_READ, &room.id).await.unwrap_err().2, off);
    assert_eq!(agent_room(&app, &geek(), THREADS_READ, "ch_general").await.unwrap_err().2, off);
    let later = issue(&t);
    assert_eq!(agent_object_room(&app, &geek(), THREADS_POST, &later.id).await.unwrap_err().2, off);
    assert_eq!(app.db.chat_room_on(&format!("project:{t}"), &later.id).unwrap(), None);
}

/// A call signed by a's machine geek.
async fn as_geek(base: &str, path: &str) -> (u16, Value) {
    let r = reqwest::Client::new().get(format!("{base}{path}")).header("x-test-daemon", "a geek").send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

/// A machine reads a native issue for its owner (M203's pinned issue
/// blocks): refused saying how without `threads.read` over the project,
/// then by id or by number; another team's issue isn't there, and a revoke
/// stops the next read.
#[tokio::test]
async fn a_machine_reads_its_owners_issue_with_a_grant() {
    let (_hub, app, base, t, u) = seeded().await;
    let issue = |project: &str| {
        let n = NewIssue { project, title: "the canary fails", body: "on deploy", labels: &[], assignees: &[] };
        app.db.create_issue(&n, Actor::person("a"), 1).unwrap().unwrap()
    };
    let ours = issue(&t);
    let theirs = issue(&u);
    let by_id = format!("/api/daemon/issues/{}", ours.id);
    let (st, v) = as_geek(&base, &by_id).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    assert!(v["error"].as_str().unwrap().contains("threads.read"), "{v}");
    assert_eq!(as_geek(&base, &format!("/api/daemon/issues/{}", theirs.id)).await.0, 404, "u's isn't a's");

    let (st, g) =
        call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");
    let (st, v) = as_geek(&base, &by_id).await;
    assert_eq!((st, &v["title"], &v["number"]), (200, &json!("the canary fails"), &json!(1)), "{v}");
    let (st, v) = as_geek(&base, &format!("/api/daemon/projects/{t}/issues/1")).await;
    assert_eq!((st, &v["id"]), (200, &json!(ours.id)), "{v}");
    assert_eq!(as_geek(&base, &format!("/api/daemon/projects/{u}/issues/1")).await.0, 404);
    assert_eq!(as_geek(&base, "/api/daemon/issues/iss_00000000000000000000000000000000").await.0, 404);
    // Only as a machine: a person's session isn't one.
    assert_eq!(call(&base, "a", "GET", &by_id, None).await.0, 401);

    let (st, _) = call(&base, "a", "POST", &format!("/api/grants/{}/revoke", g["id"].as_str().unwrap()), None).await;
    assert_eq!(st, 200);
    assert_eq!(as_geek(&base, &by_id).await.0, 403, "revoked: the next read is refused");
}
