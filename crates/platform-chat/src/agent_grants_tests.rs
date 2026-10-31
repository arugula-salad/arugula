//! Agents as principals (M197) in chat: a machine's grants to read and
//! post in its owner's team channels, DMs and object threads, never
//! reaching more than the owner reads, and what an agent posts labelled.
//! The rig is the base's (`agents_tests`).

use arugula_e2e::team::TeamRole;
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::{
    agents_tests::{geek, give, post, seeded},
    auth::Machine,
    chat::{agent_object_room, agent_room, objects::PRIVATE_REPO, post_as},
    chat_tests::call,
    db::{
        issues::{Actor, NewIssue},
        projects::{ISSUES_WRITE, THREADS_POST, THREADS_READ},
        sql::{Sql as _, params},
    },
    grants::{AGENT_UNGRANTED, AgentPrincipal, AgentScope, agent_may},
};

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
                 "powers": ["threads.post", "threads.read"],
                 "mentionable": true, "sessions": [] }])
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
