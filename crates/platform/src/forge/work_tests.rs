//! A project's work (live status board, the picker and the inbox), in
//! M165's world (`viewer_tests::world`): issues, PRs and runs in one list,
//! newest first; a repo the reader can't read left out; and untriaged
//! counting only this project's boards.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{links_tests::serve, viewer_tests::world};
use crate::{
    boards::FLAG,
    chat_tests::{TestHub, call},
    db::{Sql as _, sql::params},
};

async fn work(base: &str, who: &str, project: &str, query: &str) -> (u16, Vec<Value>) {
    let (s, v) = call(base, who, "GET", &format!("/api/projects/{project}/work{query}"), None).await;
    (s, v["items"].as_array().cloned().unwrap_or_default())
}

/// The pointers' ids of items, in order.
fn ids(items: &[Value]) -> Vec<&str> {
    items.iter().map(|i| i["pointer"]["id"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn one_list_of_issues_prs_and_runs_per_reader() {
    let hub = Arc::new(TestHub::default());
    let (_gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &_gh, &app).await;
    hub.flag("team:t1", true);
    hub.flag_named("team:t1", FLAG, true);
    {
        let c = app.db.c();
        // Another open issue on the private repo, a new one on the public
        // repo, and a closed one.
        for (id, repo, number, state, at) in
            [("fi_a4", 77u64, 4u64, "open", 25u64), ("fi_a5", 88, 5, "open", 5), ("fi_a6", 88, 6, "closed", 4)]
        {
            c.execute(
                "INSERT INTO forge_items (id, repo_id, number, kind, state, title, author_login, assignees,
                     created_at, updated_at, synced_at)
                 VALUES (?1, ?2, ?3, 'issue', ?4, ?1, 'alice-gh', '[\"bob-gh\"]', 1, ?5, 1)",
                params![id, repo, number, state, at],
            )
            .unwrap();
        }
        c.execute(
            "INSERT INTO runs (id, project_id, trigger_kind, op, op_hash, daemon, state, started_by, last_seq,
                 created_at, started_at)
             VALUES ('run_1', ?1, 'manual', 'arugula op v1\ndaemon d1\n', 'ab', 'd1', 'running', 'bob', 2, 22, 23)",
            params![team],
        )
        .unwrap();
    }
    let (s, native) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/issues"), Some(json!({ "title": "Fix login" })))
            .await;
    assert_eq!(s, 200, "{native}");
    let native = native["id"].as_str().unwrap().to_owned();
    // Assigned to bob and to an account that is gone; and exported to
    // acme/site as #5, whose mirror row (fi_a5) is there too.
    app.db
        .c()
        .execute(
            "UPDATE issues SET assignees = '[\"bob\", \"gone\"]', github_repo = 'acme/site', github_repo_id = 88,
                 github_number = 5 WHERE id = ?1",
            params![native],
        )
        .unwrap();

    // bob reads the private repo too: everything, newest first (the native
    // issue was just made).
    let (s, all) = work(&base, "bob", &team, "").await;
    assert_eq!(s, 200);
    assert_eq!(
        ids(&all),
        [&native, "fi_a1", "fi_a4", "run_1", "fi_a2", "fi_a3", "fi_a6"],
        "an exported issue is listed once: {all:?}"
    );
    assert_eq!(
        (all[0]["source"].as_str(), all[0]["assignees"].clone()),
        (Some("native"), json!(["bob", "someone who left"])),
        "a native issue's assignees are named"
    );
    assert_eq!(
        (all[1]["source"].as_str(), all[1]["kind"].as_str(), all[1]["repo"].as_str(), all[1]["number"].as_u64()),
        (Some("github"), Some("pr"), Some("acme/app"), Some(1))
    );
    assert_eq!(all[2]["assignees"], json!(["bob-gh"]));
    assert_eq!((all[3]["title"].as_str(), all[3]["state"].as_str()), (Some("manual run"), Some("running")));
    assert!(all[3]["repo"].is_null() && all[3]["number"].is_null(), "{:?}", all[3]);

    // carol has no GitHub: acme/app's items are not hers.
    let (s, carol) = work(&base, "carol", &team, "").await;
    assert_eq!(s, 200);
    assert_eq!(ids(&carol), [&native, "run_1", "fi_a2", "fi_a3", "fi_a6"], "{carol:?}");

    // kind, q and limit.
    let (_, prs) = work(&base, "bob", &team, "?kind=pr").await;
    assert_eq!(ids(&prs), ["fi_a1", "fi_a3"]);
    let (_, runs) = work(&base, "bob", &team, "?kind=run").await;
    assert_eq!(ids(&runs), ["run_1"]);
    let (_, issues) = work(&base, "bob", &team, "?kind=issue&limit=3").await;
    assert_eq!(ids(&issues), [&native, "fi_a4", "fi_a2"]);
    let (_, found) = work(&base, "bob", &team, "?q=LOGIN").await;
    assert_eq!(ids(&found), [&native]);
    let (_, found) = work(&base, "bob", &team, "?q=secret").await;
    assert_eq!(ids(&found), ["fi_a1"]);
    let (_, found) = work(&base, "bob", &team, "?q=MAN").await;
    assert_eq!(ids(&found), ["run_1"], "a run matches its trigger's name");
    let (s, _) = work(&base, "bob", &team, "?kind=board").await;
    assert_eq!(s, 400);
}

#[tokio::test]
async fn untriaged_is_open_issues_no_board_of_this_project_points_at() {
    let hub = Arc::new(TestHub::default());
    let (_gh, app, base) = serve(&hub).await;
    let (team, eve) = world(&hub, &_gh, &app).await;
    hub.flag("team:t1", true);
    hub.flag_named("team:t1", FLAG, true);
    {
        let c = app.db.c();
        for (id, repo, number, state, at) in
            [("fi_a4", 77u64, 4u64, "open", 25u64), ("fi_a5", 88, 5, "open", 5), ("fi_a6", 88, 6, "closed", 4)]
        {
            c.execute(
                "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
                 VALUES (?1, ?2, ?3, 'issue', ?4, ?1, 1, ?5, 1)",
                params![id, repo, number, state, at],
            )
            .unwrap();
        }
    }
    // The oldest native issue is loose, with the newer ones in front of it.
    let (_, c) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/issues"), Some(json!({ "title": "Older" }))).await;
    let c = c["id"].as_str().unwrap().to_owned();
    let (_, a) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/issues"), Some(json!({ "title": "On a board" })))
            .await;
    let (_, b) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/issues"), Some(json!({ "title": "Loose" }))).await;
    let (a, b) = (a["id"].as_str().unwrap().to_owned(), b["id"].as_str().unwrap().to_owned());

    // `a` was exported to acme/site as #7, and its mirror row is there: one
    // piece of work, so the copy is off the inbox with `a`.
    {
        let c = app.db.c();
        c.execute(
            "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
             VALUES ('fi_a7', 88, 7, 'issue', 'open', 'fi_a7', 1, 3, 1)",
            params![],
        )
        .unwrap();
        c.execute(
            "UPDATE issues SET github_repo = 'acme/site', github_repo_id = 88, github_number = 7 WHERE id = ?1",
            params![a],
        )
        .unwrap();
    }

    // Nothing on a board: every open issue, and no PR or run.
    let (s, v) = work(&base, "bob", &team, "?untriaged=true").await;
    assert_eq!(s, 200);
    assert_eq!(ids(&v), [&b, &a, &c, "fi_a4", "fi_a2", "fi_a5"], "{v:?}");

    // Eve's board, in another project, points at the team's fi_a2 and a:
    // that triages nothing here.
    {
        let c = app.db.c();
        c.execute(
            "INSERT INTO boards (id, project_id, title, created_by, created_at, updated_at)
             VALUES ('brd_eve', ?1, 'Mine', 'eve', 1, 1)",
            params![eve],
        )
        .unwrap();
        c.execute(
            "INSERT INTO board_entries (id, board_id, kind, position, title, created_at, updated_at)
             VALUES ('bez_eve', 'brd_eve', 'zone', 0, 'Now', 1, 1)",
            params![],
        )
        .unwrap();
        for (n, kind, target) in [(0, "forge_item", "fi_a2"), (1, "issue", a.as_str())] {
            c.execute(
                "INSERT INTO board_entries (id, board_id, kind, zone_id, position, note, target_kind, target,
                     created_at, updated_at)
                 VALUES (?1, 'brd_eve', 'row', 'bez_eve', ?2, '', ?3, ?4, 1, 1)",
                params![format!("ber_eve{n}"), n, kind, target],
            )
            .unwrap();
        }
    }
    let (_, v) = work(&base, "bob", &team, "?untriaged=true").await;
    assert_eq!(ids(&v), [&b, &a, &c, "fi_a4", "fi_a2", "fi_a5"], "another project's board triages nothing: {v:?}");

    // The team's own board points at a and fi_a5; closing b takes it out
    // (as does the closed fi_a6, throughout).
    let (_, brd) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/boards"), Some(json!({ "title": "Launch" }))).await;
    let id = brd["board"]["id"].as_str().unwrap().to_owned();
    let put = |body: Value| {
        let path = format!("/api/boards/{id}/entries");
        let base = base.clone();
        async move { call(&base, "bob", "PUT", &path, Some(body)).await }
    };
    let (_, z) = put(json!({ "entry": "zone", "title": "Now" })).await;
    let zone = z["zones"][0]["id"].as_str().unwrap().to_owned();
    for ptr in [json!({ "kind": "issue", "id": a }), json!({ "kind": "forge_item", "id": "fi_a5" })] {
        let (s, v) = put(json!({ "entry": "row", "zone": zone, "note": "n", "pointer": ptr })).await;
        assert_eq!(s, 200, "{v}");
    }
    let (s, _) = call(&base, "bob", "PATCH", &format!("/api/issues/{b}"), Some(json!({ "state": "closed" }))).await;
    assert_eq!(s, 200);
    let (_, v) = work(&base, "bob", &team, "?untriaged=true").await;
    assert_eq!(ids(&v), [&c, "fi_a4", "fi_a2"], "{v:?}");

    // carol can't read acme/app, so fi_a4 isn't in her inbox.
    let (_, v) = work(&base, "carol", &team, "?untriaged=true").await;
    assert_eq!(ids(&v), [&c, "fi_a2"], "{v:?}");
    // A limit is kept after the boards' rows are taken out.
    let (_, v) = work(&base, "bob", &team, "?untriaged=true&limit=1").await;
    assert_eq!(ids(&v), [&c], "{v:?}");
    // A stranger.
    let (s, _) = work(&base, "eve", &team, "").await;
    assert_eq!(s, 404);
}

#[tokio::test]
async fn work_is_not_there_with_boards_off() {
    let hub = Arc::new(TestHub::default());
    let (_gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &_gh, &app).await;
    hub.flag("team:t1", true);
    hub.flag_named("team:t1", FLAG, false);
    let (s, _) = work(&base, "alice", &team, "").await;
    assert_eq!(s, 404);
}
