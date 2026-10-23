//! Object threads (M196) through the platform's own routes, with the hub
//! of chat's tests ([`TestHub`]): who opens, posts, reads and searches the
//! thread on a mirrored PR or issue, a native issue or a hosted workspace,
//! and who is refused.

use std::{sync::Arc, time::Duration};

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{
    Platform,
    chat::objects::PRIVATE_REPO,
    chat_tests::{TestHub, call, hears, open_live, serve},
    db::sql::{Sql as _, params},
};

/// Team t (a owns it, b views it) has its default project with a linked
/// public repo (PR `fi_pub`) and a private one (issue `fi_sec`); a's
/// personal project has a repo of its own (PR `fi_own`). c is in team u,
/// not t. Chat is on for both teams. The project ids.
fn seed(app: &Platform, hub: &TestHub) -> (String, String) {
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Viewer)]);
    hub.team("u", &[("c", TeamRole::Owner)]);
    hub.flag("team:t", true);
    hub.flag("team:u", true);
    let team = app.db.team_project("t", "t", 1).unwrap();
    let own = app.db.personal_project("a", 1).unwrap();
    let c = app.db.c();
    for (repo, name, private, project) in
        [(7, "o/public", false, &team), (8, "o/secret", true, &team), (9, "a/mine", false, &own)]
    {
        c.execute(
            "INSERT INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_at)
             VALUES (?1, ?2, ?3, ?4, 1, 1)",
            params![repo, name, private, project],
        )
        .unwrap();
    }
    for (id, repo, number, kind) in [("fi_pub", 7, 3, "pr"), ("fi_sec", 8, 4, "issue"), ("fi_own", 9, 1, "pr")] {
        c.execute(
            "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
             VALUES (?1, ?2, ?3, ?4, 'open', 'a title', 1, 1, 1)",
            params![id, repo, number, kind],
        )
        .unwrap();
    }
    (team, own)
}

async fn open(base: &str, who: &str, target: &str) -> (u16, Value) {
    call(base, who, "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": target }))).await
}

/// A member opens the thread on a public repo's PR, comments, mentions a
/// teammate (who is pushed, hears it live, has it unread and finds it in
/// search); a non-member is refused everywhere, as if it weren't there.
#[tokio::test]
async fn members_comment_on_a_mirrored_pr_and_others_are_refused() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (project, _) = seed(&app, &hub);

    let live = open_live(&base, "a", None).await;
    hears(&live, "\"type\":\"hello\"", Duration::from_secs(5)).await;

    // b, a viewer, opens it: made now, in the team's project, on no machine.
    let (st, room) = open(&base, "b", "fi_pub").await;
    assert_eq!(st, 200, "{room}");
    assert_eq!(
        (&room["kind"], &room["name"], &room["project"], &room["target"], &room["machine"], &room["writable"]),
        (&json!("object"), &json!("PR o/public#3"), &json!(project), &json!("fi_pub"), &Value::Null, &json!(true))
    );
    let id = room["id"].as_str().unwrap().to_owned();
    assert_eq!(open(&base, "a", "fi_pub").await.1["id"], json!(id), "one room per object");

    let (st, v) =
        call(&base, "b", "POST", &format!("/api/chat/rooms/{id}/messages"), Some(json!({ "text": "@a lgtm" }))).await;
    assert_eq!((st, &v["pushed"]), (200, &json!([{ "account": "a", "why": "mention" }])), "{v}");
    hears(&live, &format!("\"type\":\"message\",\"room\":\"{id}\",\"seq\":1"), Duration::from_secs(4)).await;

    let (st, v) = call(&base, "a", "GET", "/api/chat", None).await;
    assert_eq!(st, 200, "{v}");
    let mine: Vec<&Value> = v["rooms"].as_array().unwrap().iter().filter(|r| r["id"] == json!(id)).collect();
    assert_eq!((mine.len(), &mine[0]["unread"], &mine[0]["mentions"]), (1, &json!(1), &json!(1)), "{v}");
    let (st, v) = call(&base, "a", "POST", &format!("/api/chat/rooms/{id}/read"), Some(json!({ "seq": 1 }))).await;
    assert_eq!((st, &v["unread"]), (200, &json!(0)), "{v}");
    let (st, v) = call(&base, "a", "GET", &format!("/api/chat/rooms/{id}"), None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["people"].as_array().unwrap().len(), 2, "the project's members: {v}");
    let (_, v) = call(&base, "a", "GET", "/api/chat/search?q=lgtm", None).await;
    assert_eq!(v["found"][0]["room"], json!(id), "{v}");
    let (_, v) = call(&base, "a", "GET", "/api/chat/search?q=lgtm&team=t", None).await;
    assert_eq!(v["found"].as_array().unwrap().len(), 1, "in its team's search: {v}");

    // c isn't in t.
    assert_eq!(open(&base, "c", "fi_pub").await.0, 404);
    assert_eq!(call(&base, "c", "GET", &format!("/api/chat/rooms/{id}"), None).await.0, 404);
    assert_eq!(call(&base, "c", "GET", &format!("/api/chat/rooms/{id}/messages"), None).await.0, 404);
    let post = Some(json!({ "text": "hi" }));
    assert_eq!(call(&base, "c", "POST", &format!("/api/chat/rooms/{id}/messages"), post).await.0, 404);
    let (_, v) = call(&base, "c", "GET", "/api/chat", None).await;
    assert!(v["rooms"].as_array().unwrap().iter().all(|r| r["id"] != json!(id)), "{v}");
    let (_, v) = call(&base, "c", "GET", "/api/chat/search?q=lgtm", None).await;
    assert_eq!(v["found"], json!([]), "{v}");

    // b leaves t: out of its threads once the feed is read.
    hub.team("t", &[("a", TeamRole::Owner)]);
    hub.flag("account:b", true);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(call(&base, "b", "GET", &format!("/api/chat/rooms/{id}"), None).await.0, 404);
}

/// A personal project's objects are its account's alone.
#[tokio::test]
async fn a_personal_projects_threads_are_its_accounts() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (_, own) = seed(&app, &hub);
    let (st, room) = open(&base, "a", "fi_own").await;
    assert_eq!((st, &room["project"]), (200, &json!(own)), "{room}");
    assert_eq!(open(&base, "b", "fi_own").await.0, 404, "a's teammate isn't in a's personal project");
}

/// A private repo's threads need the reader's own GitHub access (M165):
/// here nobody signed in with GitHub and there's no App to ask, so every
/// member is refused, and a room made before its repo went private is
/// hidden from everyone. `forge::viewer_tests::leak_object_threads` and
/// the inbox's `forge::mentions_tests` have members GitHub lets in.
#[tokio::test]
async fn a_private_repos_threads_need_github_access() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    for who in ["a", "b"] {
        let (st, v) = open(&base, who, "fi_sec").await;
        assert_eq!((st, &v["reason"]), (403, &json!(PRIVATE_REPO)), "{who}: {v}");
    }
    assert_eq!(open(&base, "c", "fi_sec").await.0, 404, "a non-member doesn't learn it's there");

    let (_, room) = open(&base, "a", "fi_pub").await;
    let id = room["id"].as_str().unwrap();
    app.db.c().execute("UPDATE linked_repos SET private = TRUE WHERE repo_id = 7", params![]).unwrap();
    assert_eq!(call(&base, "a", "GET", &format!("/api/chat/rooms/{id}"), None).await.0, 404);
    let (_, v) = call(&base, "a", "GET", "/api/chat", None).await;
    assert!(v["rooms"].as_array().unwrap().iter().all(|r| r["id"] != json!(id)), "{v}");
}

/// What an id names, by its prefix: an unknown one or a missing row is no
/// object.
#[tokio::test]
async fn an_id_names_an_object_or_nothing() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    for target in ["prj_1", "fi_00", "hw_00", "cw_9d81", "nothing", ""] {
        assert_eq!(open(&base, "a", target).await.0, 404, "{target}");
    }
    // An unlinked repo's items have no threads.
    app.db.c().execute("UPDATE linked_repos SET unlinked_at = 5 WHERE repo_id = 7", params![]).unwrap();
    assert_eq!(open(&base, "a", "fi_pub").await.0, 404);
}

/// A native issue's comments are its thread's messages (M194): a member
/// (a team's viewer too) comments on it and reads it back; someone outside
/// its project is refused, and so is a teammate on a personal project's.
#[tokio::test]
async fn members_comment_on_a_native_issue_and_others_are_refused() {
    use crate::db::issues::{Actor, NewIssue};
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (team, own) = seed(&app, &hub);
    let new = |project: &str| {
        let n = NewIssue { project, title: "the canary fails", body: "", labels: &[], assignees: &[] };
        app.db.create_issue(&n, Actor::person("a"), 1).unwrap().unwrap()
    };
    let ours = new(&team);
    let mine = new(&own);

    let (st, room) = open(&base, "b", &ours.id).await;
    assert_eq!(st, 200, "{room}");
    assert_eq!((&room["name"], &room["project"]), (&json!(format!("issue #{}", ours.number)), &json!(team)));
    let id = room["id"].as_str().unwrap();
    let post = Some(json!({ "text": "seen it on main too" }));
    assert_eq!(call(&base, "b", "POST", &format!("/api/chat/rooms/{id}/messages"), post).await.0, 200);
    let (st, v) = call(&base, "a", "GET", &format!("/api/chat/rooms/{id}/messages"), None).await;
    assert_eq!((st, &v["messages"][0]["text"]), (200, &json!("seen it on main too")), "{v}");

    assert_eq!(open(&base, "c", &ours.id).await.0, 404);
    assert_eq!(call(&base, "c", "GET", &format!("/api/chat/rooms/{id}/messages"), None).await.0, 404);
    assert_eq!(open(&base, "b", &mine.id).await.0, 404, "a's personal project is a's");
    assert_eq!(open(&base, "a", &mine.id).await.0, 200);
    assert_eq!(open(&base, "a", "iss_0123456789abcdef0123456789abcdef").await.0, 404, "no such issue");
}

/// A repo relinked to another project (`linked_repos.repo_id` is its key,
/// so it moves): the new project's members get a room of their own, the old
/// one is hidden from everyone, and it's back when the repo comes back.
#[tokio::test]
async fn a_relinked_repo_gets_a_new_room_in_its_new_project() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (team, _) = seed(&app, &hub);
    let other = app.db.team_project("u", "u", 1).unwrap();
    let (_, room) = open(&base, "a", "fi_pub").await;
    let old = room["id"].as_str().unwrap().to_owned();
    let post = Some(json!({ "text": "before the move" }));
    assert_eq!(call(&base, "a", "POST", &format!("/api/chat/rooms/{old}/messages"), post).await.0, 200);
    let link = |project: &str| {
        app.db.c().execute("UPDATE linked_repos SET project_id = ?1 WHERE repo_id = 7", params![project]).unwrap();
    };

    link(&other);
    assert_eq!(call(&base, "a", "GET", &format!("/api/chat/rooms/{old}"), None).await.0, 404, "the old room hides");
    assert_eq!(open(&base, "a", "fi_pub").await.0, 404, "a isn't in u");
    let (st, room) = open(&base, "c", "fi_pub").await;
    assert_eq!((st, &room["project"]), (200, &json!(other)), "{room}");
    let new = room["id"].as_str().unwrap().to_owned();
    assert_ne!(new, old);
    let (_, v) = call(&base, "c", "GET", &format!("/api/chat/rooms/{new}/messages"), None).await;
    assert_eq!(v["messages"], json!([]), "nothing of the old project's: {v}");
    assert_eq!(call(&base, "c", "GET", &format!("/api/chat/rooms/{old}"), None).await.0, 404);

    link(&team);
    let (st, room) = open(&base, "a", "fi_pub").await;
    assert_eq!((st, &room["id"]), (200, &json!(old)), "{room}");
    assert_eq!(call(&base, "c", "GET", &format!("/api/chat/rooms/{new}"), None).await.0, 404);
}

/// The purge of an unlinked repo's rows (M163) takes the threads on its
/// items, with their messages, read marks and search; another repo's
/// thread stays.
#[tokio::test]
async fn the_purge_of_an_unlinked_repo_takes_its_threads() {
    use crate::db::migrate::Dialect;
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    let mut rooms = vec![];
    for (target, word) in [("fi_pub", "pubword"), ("fi_own", "ownword")] {
        let (st, room) = open(&base, "a", target).await;
        assert_eq!(st, 200, "{room}");
        let id = room["id"].as_str().unwrap().to_owned();
        let post = Some(json!({ "text": format!("canary {word}") }));
        assert_eq!(call(&base, "a", "POST", &format!("/api/chat/rooms/{id}/messages"), post).await.0, 200);
        let read = Some(json!({ "seq": 1 }));
        assert_eq!(call(&base, "a", "POST", &format!("/api/chat/rooms/{id}/read"), read).await.0, 200);
        rooms.push(id);
    }

    app.db
        .c()
        .execute("UPDATE linked_repos SET unlinked_at = 5, purge_after = 6 WHERE repo_id = 7", params![])
        .unwrap();
    assert_eq!(crate::forge::links::purge(&app, 10).unwrap(), 1);
    let count = |sql: &str, id: &str| -> u64 { app.db.c().query_row(sql, params![id], |r| r.get(0)).unwrap() };
    for sql in [
        "SELECT count(*) FROM chat_rooms WHERE id = ?1",
        "SELECT count(*) FROM chat_messages WHERE room = ?1",
        "SELECT count(*) FROM chat_reads WHERE room = ?1",
    ] {
        assert_eq!((count(sql, &rooms[0]), count(sql, &rooms[1])), (0, 1), "{sql}");
    }
    if app.db.c().dialect() == Dialect::Sqlite {
        let fts = "SELECT count(*) FROM chat_fts WHERE chat_fts MATCH ?1";
        assert_eq!((count(fts, "pubword"), count(fts, "ownword")), (0, 1));
    }
    let (_, v) = call(&base, "a", "GET", "/api/chat/search?q=canary", None).await;
    let found: Vec<&Value> = v["found"].as_array().unwrap().iter().map(|f| &f["room"]).collect();
    assert_eq!(found, [&json!(rooms[1])], "{v}");
}

/// A hosted workspace in t's project (M179), `hw_dev`.
fn workspace(app: &Platform, project: &str) -> String {
    let spec = crate::hosted::Spec { setup: "make".into(), ..Default::default() };
    match app.db.add_hosted_workspace("hw_dev", project, "dev", &spec, "a", 10, 1).unwrap() {
        crate::db::hosted::Added::Made(w) => w.id,
        other => panic!("{other:?}"),
    }
}

/// The worker deletes `id` for good, as `hosted::worker` does once its
/// boxes are gone.
fn delete_for_good(app: &Platform, id: &str) {
    use crate::hosted::Ask;
    assert!(app.db.ask_hosted(id, Ask::Delete, 20).unwrap());
    let mut job = app.db.claim_hosted(20).unwrap().unwrap();
    assert_eq!(job.w.id, id);
    job.w.state = "deleted".into();
    job.deleted_at = Some(20);
    app.db.checkpoint_hosted(&job, Some(Ask::Delete), 20, 20).unwrap();
}

/// A hosted workspace's thread (M179) is its project's: a member (a
/// viewer too) comments on it; someone outside the project is refused.
/// Deleted for good, the workspace takes its thread, with its messages,
/// read marks and search.
#[tokio::test]
async fn members_comment_on_a_hosted_workspace_until_its_deleted() {
    use crate::db::migrate::Dialect;
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (team, _) = seed(&app, &hub);
    let ws = workspace(&app, &team);

    let (st, room) = open(&base, "b", &ws).await;
    assert_eq!(st, 200, "{room}");
    assert_eq!(
        (&room["kind"], &room["name"], &room["project"], &room["target"]),
        (&json!("object"), &json!("workspace dev"), &json!(team), &json!(ws))
    );
    let id = room["id"].as_str().unwrap().to_owned();
    let post = Some(json!({ "text": "@a the box is warm" }));
    assert_eq!(call(&base, "b", "POST", &format!("/api/chat/rooms/{id}/messages"), post).await.0, 200);
    let (st, v) = call(&base, "a", "GET", &format!("/api/chat/rooms/{id}/messages"), None).await;
    assert_eq!((st, &v["messages"][0]["text"]), (200, &json!("@a the box is warm")), "{v}");
    assert_eq!(call(&base, "a", "POST", &format!("/api/chat/rooms/{id}/read"), Some(json!({ "seq": 1 }))).await.0, 200);

    assert_eq!(open(&base, "c", &ws).await.0, 404);
    assert_eq!(call(&base, "c", "GET", &format!("/api/chat/rooms/{id}/messages"), None).await.0, 404);
    let (_, v) = call(&base, "c", "GET", "/api/chat/search?q=warm", None).await;
    assert_eq!(v["found"], json!([]), "{v}");

    delete_for_good(&app, &ws);
    for who in ["a", "b"] {
        assert_eq!(open(&base, who, &ws).await.0, 404, "{who}");
        assert_eq!(call(&base, who, "GET", &format!("/api/chat/rooms/{id}"), None).await.0, 404, "{who}");
        let (_, v) = call(&base, who, "GET", "/api/chat", None).await;
        assert!(v["rooms"].as_array().unwrap().iter().all(|r| r["id"] != json!(id)), "{who}: {v}");
    }
    let count = |sql: &str, id: &str| -> u64 { app.db.c().query_row(sql, params![id], |r| r.get(0)).unwrap() };
    for sql in [
        "SELECT count(*) FROM chat_rooms WHERE id = ?1",
        "SELECT count(*) FROM chat_messages WHERE room = ?1",
        "SELECT count(*) FROM chat_reads WHERE room = ?1",
    ] {
        assert_eq!(count(sql, &id), 0, "{sql}");
    }
    if app.db.c().dialect() == Dialect::Sqlite {
        assert_eq!(count("SELECT count(*) FROM chat_fts WHERE chat_fts MATCH ?1", "warm"), 0);
    }
    let (_, v) = call(&base, "a", "GET", "/api/chat/search?q=warm", None).await;
    assert_eq!(v["found"], json!([]), "{v}");
}

/// An agent reaches a hosted workspace's thread through its project's
/// grant, as any object's (M197): refused without one, then it comments.
#[tokio::test]
async fn an_agent_comments_on_a_hosted_workspace() {
    use crate::{
        agents_tests::{geek, give, post, seeded},
        chat::{agent_object_room, post_as},
        db::projects::{THREADS_POST, THREADS_READ},
        grants::AGENT_UNGRANTED,
    };
    let (_hub, app, base, t, u) = seeded().await;
    let ours = workspace(&app, &t);
    let spec = crate::hosted::Spec::default();
    app.db.add_hosted_workspace("hw_theirs", &u, "dev", &spec, "c", 10, 1).unwrap();

    let e = agent_object_room(&app, &geek(), THREADS_POST, &ours).await.unwrap_err();
    assert_eq!(e.2, Some(AGENT_UNGRANTED), "{}", e.1);
    let e = agent_object_room(&app, &geek(), THREADS_POST, "hw_theirs").await.unwrap_err();
    assert_eq!(e.0.as_u16(), 404, "u's workspace isn't a's to see: {}", e.1);
    for power in [THREADS_READ, THREADS_POST] {
        let (st, v) =
            call(&base, "a", "POST", "/api/grants", give("geek", power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{v}");
    }
    let (room, _, agent) = agent_object_room(&app, &geek(), THREADS_POST, &ours).await.unwrap();
    assert_eq!((room.name.as_str(), room.target.as_deref()), ("workspace dev", Some(ours.as_str())));
    post_as(&app, "a", Some(&agent), &room.id, post("setup finished")).await.unwrap();
    let (st, v) = call(&base, "b", "GET", &format!("/api/chat/rooms/{}/messages", room.id), None).await;
    assert_eq!((st, &v["messages"][0]["agent"]["machine"]), (200, &json!("geek")), "{v}");
}
