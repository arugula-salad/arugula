//! A board as an epic (#301), in M165's world (`viewer_tests::world`): its
//! goal, its progress over native and mirrored issues as each reader may
//! read them, and closing and opening it.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{links_tests::serve, viewer_tests::world};
use crate::{
    boards::FLAG,
    chat_tests::{TestHub, call},
    db::{Sql as _, issues::Actor, sql::params},
};

const CLOSE: &str = "UPDATE issues SET state = 'closed', close_reason = 'completed', closed_at = 5 WHERE id = ?1";

/// A native issue in `project`, closed or not: its `iss_` id.
async fn issue(base: &str, app: &crate::Platform, project: &str, title: &str, closed: bool) -> String {
    let (s, i) =
        call(base, "bob", "POST", &format!("/api/projects/{project}/issues"), Some(json!({ "title": title }))).await;
    assert_eq!(s, 200, "{i}");
    let id = i["id"].as_str().unwrap().to_owned();
    if closed {
        app.db.c().execute(CLOSE, params![id]).unwrap();
    }
    id
}

/// A row on `pointer`: its id.
async fn row(base: &str, board: &str, kind: &str, id: &str) -> String {
    let (s, v) = call(
        base,
        "bob",
        "PUT",
        &format!("/api/boards/{board}/entries"),
        Some(json!({ "entry": "row", "pointer": { "kind": kind, "id": id } })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    v["zones"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|z| z["rows"].as_array().unwrap())
        .find(|r| r["pointer"]["id"] == id)
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .unwrap_or_else(|| panic!("{v}"))
}

fn progress(v: &Value) -> (u64, u64, u64) {
    let p = &v["progress"];
    (p["done"].as_u64().unwrap(), p["total"].as_u64().unwrap(), p["proven"].as_u64().unwrap())
}

/// Progress counts the rows with an issue or a PR as their item, whoever
/// reads: a lane follows the work, not the reader's access to the repo.
#[tokio::test]
async fn progress_counts_the_rows_in_done_and_those_proven() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    hub.flag("team:t1", true);
    hub.flag_named("team:t1", FLAG, true);
    {
        // Two more issues on each repo: an open and a closed one.
        let c = app.db.c();
        for (id, repo, number, state) in [
            ("fi_i1", 77u64, 4u64, "open"),
            ("fi_i2", 77, 5, "closed"),
            ("fi_i3", 88, 6, "open"),
            ("fi_i4", 88, 7, "closed"),
        ] {
            c.execute(
                "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
                 VALUES (?1, ?2, ?3, 'issue', ?4, ?1, 1, 1, 1)",
                params![id, repo, number, state],
            )
            .unwrap();
        }
    }
    let open = issue(&base, &app, &team, "Open", false).await;
    let shut = issue(&base, &app, &team, "Shut", true).await;

    let (s, b) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/boards"), Some(json!({ "title": "E" }))).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(progress(&b["board"]), (0, 0, 0), "a board with no rows");
    let id = b["board"]["id"].as_str().unwrap().to_owned();

    // Counted: two native issues, four mirrored issues and two PRs (an open
    // one in Review, a merged one in Done). Not counted: a note, and a
    // second row for an issue that is a row already, which isn't added.
    let mut shut_row = String::new();
    for (kind, ptr) in [
        ("issue", open.as_str()),
        ("issue", shut.as_str()),
        ("issue", shut.as_str()),
        ("forge_item", "fi_i3"),
        ("forge_item", "fi_i4"),
        ("forge_item", "fi_i1"),
        ("forge_item", "fi_i2"),
        ("forge_item", "fi_a1"),
        ("forge_item", "fi_a3"),
    ] {
        let r = row(&base, &id, kind, ptr).await;
        if ptr == shut {
            shut_row = r;
        }
    }
    let (s, _) =
        call(&base, "bob", "PUT", &format!("/api/boards/{id}/entries"), Some(json!({ "entry": "row", "note": "n" })))
            .await;
    assert_eq!(s, 200);

    // Done: the closed native issue, fi_i2, fi_i4 and the merged PR (with
    // no check on its head, it is not proven by CI).
    let (_, v) = call(&base, "bob", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(progress(&v["board"]), (4, 8, 0), "{v}");
    let (_, l) = call(&base, "bob", "GET", &format!("/api/projects/{team}/boards"), None).await;
    assert_eq!(progress(&l["boards"][0]), (4, 8, 0), "the list says what the view does");

    // carol can't read acme/app, and the board is the same for her.
    let (_, v) = call(&base, "carol", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(progress(&v["board"]), (4, 8, 0), "{v}");
    let (_, l) = call(&base, "carol", "GET", &format!("/api/projects/{team}/boards"), None).await;
    assert_eq!(progress(&l["boards"][0]), (4, 8, 0));

    // An issue closing at its source moves the count.
    app.db.c().execute(CLOSE, params![open]).unwrap();
    let (_, v) = call(&base, "bob", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(progress(&v["board"]), (5, 8, 0));

    // Evidence on a Done row proves it.
    let (s, v) = call(
        &base,
        "bob",
        "POST",
        &format!("/api/boards/{id}/attachments"),
        Some(json!({ "row": shut_row, "role": "proof", "pointer": { "kind": "link", "url": "https://ci.example.com/1" } })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(progress(&v["board"]), (5, 8, 1), "{v}");
}

#[tokio::test]
async fn a_goal_is_set_and_a_board_is_closed_and_opened_again() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    hub.flag("team:t1", true);
    hub.flag_named("team:t1", FLAG, true);
    let mk = |t: &'static str| {
        let (base, team) = (base.clone(), team.clone());
        async move {
            let (_, b) =
                call(&base, "bob", "POST", &format!("/api/projects/{team}/boards"), Some(json!({ "title": t }))).await;
            b["board"]["id"].as_str().unwrap_or_else(|| panic!("{b}")).to_owned()
        }
    };
    let first = mk("First").await;
    let second = mk("Second").await;
    let path = format!("/api/boards/{first}");

    // The goal: multi-line, trimmed, cleared by an empty one.
    let (s, v) =
        call(&base, "bob", "PATCH", &path, Some(json!({ "goal": "  Ship it.\nThen rest.  ", "title": "One" }))).await;
    assert_eq!(
        (s, &v["board"]["goal"], &v["board"]["title"]),
        (200, &json!("Ship it.\nThen rest."), &json!("One")),
        "{v}"
    );
    let (s, v) = call(&base, "bob", "PATCH", &path, Some(json!({ "goal": "x".repeat(2001) }))).await;
    assert_eq!(s, 400, "{v}");
    let (s, _) = call(&base, "bob", "PATCH", &path, Some(json!({ "title": " " }))).await;
    assert_eq!(s, 400);
    let (s, v) = call(&base, "bob", "PATCH", &path, Some(json!({ "title": "Uno" }))).await;
    assert_eq!((s, &v["board"]["goal"]), (200, &json!("Ship it.\nThen rest.")), "a goal left out stays: {v}");
    let (s, v) = call(&base, "bob", "PATCH", &path, Some(json!({ "goal": "" }))).await;
    assert_eq!((s, &v["board"]["goal"]), (200, &json!("")), "{v}");

    // A viewer changes none of it.
    let (s, _) = call(&base, "carol", "PATCH", &path, Some(json!({ "goal": "mine" }))).await;
    assert_eq!(s, 403);
    let (s, _) = call(&base, "carol", "POST", &format!("{path}/close"), Some(json!({ "done_note": "done" }))).await;
    assert_eq!(s, 403);
    let (s, _) = call(&base, "carol", "POST", &format!("{path}/reopen"), None).await;
    assert_eq!(s, 403);

    // No note, no close.
    for note in ["", "  \n "] {
        let (s, v) = call(&base, "bob", "POST", &format!("{path}/close"), Some(json!({ "done_note": note }))).await;
        assert_eq!(s, 400, "{v}");
    }
    let (_, v) = call(&base, "bob", "GET", &path, None).await;
    assert!(v["board"]["closed_at"].is_null(), "still open: {v}");

    // Closed: the rows stay editable and the list puts it last.
    let (s, v) = call(&base, "bob", "POST", &format!("{path}/close"), Some(json!({ "done_note": " Shipped. " }))).await;
    assert_eq!(
        (s, &v["board"]["done_note"], &v["board"]["closed_by"]),
        (200, &json!("Shipped."), &json!("bob")),
        "{v}"
    );
    let closed_at = v["board"]["closed_at"].as_u64().unwrap();
    let (s, v) =
        call(&base, "bob", "PUT", &format!("{path}/entries"), Some(json!({ "entry": "row", "note": "Late" }))).await;
    assert_eq!(s, 200, "a closed board's rows are editable: {v}");
    let (_, l) = call(&base, "carol", "GET", &format!("/api/projects/{team}/boards"), None).await;
    let order: Vec<&str> = l["boards"].as_array().unwrap().iter().map(|b| b["id"].as_str().unwrap()).collect();
    assert_eq!(order, [second.as_str(), first.as_str()], "open first: {l}");

    // Closing again takes the new note and keeps the first close.
    app.db.c().execute("UPDATE boards SET closed_at = ?2 WHERE id = ?1", params![first, closed_at - 1]).unwrap();
    let (_, v) =
        call(&base, "alice", "POST", &format!("{path}/close"), Some(json!({ "done_note": "Really shipped." }))).await;
    assert_eq!(
        (&v["board"]["done_note"], &v["board"]["closed_by"], v["board"]["closed_at"].as_u64()),
        (&json!("Really shipped."), &json!("bob"), Some(closed_at - 1)),
        "{v}"
    );

    // Opened again, it keeps the note.
    let (s, v) = call(&base, "alice", "POST", &format!("{path}/reopen"), None).await;
    assert_eq!(s, 200, "{v}");
    assert!(v["board"]["closed_at"].is_null() && v["board"]["closed_by"] == json!(""), "{v}");
    assert_eq!(v["board"]["done_note"], json!("Really shipped."));
    let (_, l) = call(&base, "carol", "GET", &format!("/api/projects/{team}/boards"), None).await;
    assert_eq!(l["boards"][0]["id"], json!(first), "oldest first once both are open: {l}");
}

#[tokio::test]
async fn a_deleted_accounts_boards_keep_no_creator_or_closer() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, _base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    let b = app.db.create_board(&team, "E", "", Actor::person("gone"), 1, 10).unwrap().unwrap();
    assert!(app.db.close_board(&b.id, "Shipped.", Actor::person("gone"), 2).unwrap());
    let d = crate::Deleted { account: "gone".into(), machines: vec![], disbanded: vec![] };
    crate::db::boards::erase_account(&app.db.c(), &d).unwrap();
    let b = app.db.board(&b.id).unwrap().unwrap();
    assert_eq!((b.created_by.as_str(), b.closed_by.as_str(), b.done_note.as_str()), ("", "", "Shipped."));
}
