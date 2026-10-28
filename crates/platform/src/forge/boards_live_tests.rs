//! A board's rows say what they point at now (live status board, step 2),
//! in M165's world (`viewer_tests::world`): a row on a merged PR reads
//! `merged`, on an open one `open`, then `checks_running` once a check is
//! pending; one on a private repo's item is left out for someone who can't
//! read the repo, and a native issue reads open, then closed. A webhook
//! that changes a mirrored item tells the boards that point at it.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{links_tests::serve, viewer_tests::world};
use crate::{
    boards::FLAG,
    chat_tests::{TestHub, call},
    db::{Sql as _, sql::params},
};

async fn put(base: &str, board: &str, body: Value) -> (u16, Value) {
    call(base, "bob", "PUT", &format!("/api/boards/{board}/entries"), Some(body)).await
}

async fn live(base: &str, who: &str, board: &str) -> Vec<Value> {
    let (s, v) = call(base, who, "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(s, 200, "{v}");
    v["zones"][0]["rows"].as_array().unwrap().iter().map(|r| r["live"].clone()).collect()
}

#[tokio::test]
async fn a_row_reports_what_it_points_at_now() {
    let hub = Arc::new(TestHub::default());
    let (_gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &_gh, &app).await;
    hub.flag("team:t1", true);
    hub.flag_named("team:t1", FLAG, true);

    // bob (an editor, signed in with GitHub) makes a board of four rows: the
    // merged PR, the open PR on the private repo, a native issue and a note.
    let (_, b) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/boards"), Some(json!({ "title": "Launch" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let (_, v) = put(&base, &id, json!({ "entry": "zone", "title": "Now" })).await;
    let zone = v["zones"][0]["id"].as_str().unwrap().to_owned();
    let (_, issue) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/issues"), Some(json!({ "title": "Fix login" })))
            .await;
    let issue = issue["id"].as_str().unwrap().to_owned();
    for ptr in [
        json!({ "kind": "forge_item", "id": "fi_a3" }),
        json!({ "kind": "forge_item", "id": "fi_a1" }),
        json!({ "kind": "issue", "id": issue }),
        json!({ "kind": "none" }),
    ] {
        let (s, v) =
            put(&base, &id, json!({ "entry": "row", "zone": zone, "note": "n", "pointer": ptr, "status": "working" }))
                .await;
        assert_eq!(s, 200, "{v}");
    }

    let rows = live(&base, "bob", &id).await;
    assert_eq!(rows[0]["state"], "merged", "{rows:?}");
    assert_eq!((rows[0]["repo"].as_str(), rows[0]["number"].as_u64()), (Some("acme/site"), Some(3)));
    assert_eq!(rows[1]["state"], "open", "its one check is done, with no verdict: {rows:?}");
    assert_eq!((rows[2]["state"].as_str(), rows[2]["title"].as_str()), (Some("open"), Some("Fix login")), "{rows:?}");
    assert!(rows[3].is_null(), "a note points at nothing");
    // The row's own status is its own.
    let (_, v) = call(&base, "bob", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(v["zones"][0]["rows"][0]["status"], "working");

    // carol (a viewer, without GitHub) can't read the private repo's item.
    let rows = live(&base, "carol", &id).await;
    assert_eq!((rows[0]["state"].as_str(), rows[1].is_null()), (Some("merged"), true), "{rows:?}");

    // A check running on the open PR's head, and the issue closed.
    app.db
        .c()
        .execute(
            "INSERT INTO forge_checks (repo_id, head_sha, kind, name, status, updated_at)
             VALUES (77, 'abc', 'check_run', 'tests', 'in_progress', 2)",
            params![],
        )
        .unwrap();
    let (s, v) = call(&base, "bob", "PATCH", &format!("/api/issues/{issue}"), Some(json!({ "state": "closed" }))).await;
    assert_eq!(s, 200, "{v}");
    let rows = live(&base, "bob", &id).await;
    assert_eq!(
        (rows[1]["state"].as_str(), rows[2]["state"].as_str()),
        (Some("checks_running"), Some("closed")),
        "{rows:?}"
    );
}

/// A row on an issue or a PR carries who it is assigned to: a mirrored
/// item's logins, a native issue's as its issue list names them. A stale mirror row (`null`, or
/// not a list) reads as no one; a note has no `live` at all.
#[tokio::test]
async fn a_row_on_an_issue_or_pr_names_its_assignees() {
    let hub = Arc::new(TestHub::default());
    let (_gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &_gh, &app).await;
    hub.flag("team:t1", true);
    hub.flag_named("team:t1", FLAG, true);

    let (_, b) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/boards"), Some(json!({ "title": "Launch" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let (_, v) = put(&base, &id, json!({ "entry": "zone", "title": "Now" })).await;
    let zone = v["zones"][0]["id"].as_str().unwrap().to_owned();
    let (s, issue) = call(
        &base,
        "bob",
        "POST",
        &format!("/api/projects/{team}/issues"),
        Some(json!({ "title": "Fix login", "assignees": ["bob", "carol"] })),
    )
    .await;
    assert_eq!(s, 200, "{issue}");
    let issue = issue["id"].as_str().unwrap().to_owned();
    for ptr in [
        json!({ "kind": "forge_item", "id": "fi_a3" }),
        json!({ "kind": "forge_item", "id": "fi_a1" }),
        json!({ "kind": "issue", "id": issue }),
    ] {
        let (s, v) = put(&base, &id, json!({ "entry": "row", "zone": zone, "note": "n", "pointer": ptr })).await;
        assert_eq!(s, 200, "{v}");
    }
    app.db
        .c()
        .execute("UPDATE forge_items SET assignees = '[\"octo\",\"cat\"]' WHERE id = 'fi_a3'", params![])
        .unwrap();
    app.db.c().execute("UPDATE forge_items SET assignees = 'null' WHERE id = 'fi_a1'", params![]).unwrap();

    let rows = live(&base, "bob", &id).await;
    assert_eq!(rows[0]["assignees"], json!(["octo", "cat"]), "{rows:?}");
    assert!(rows[1]["assignees"].is_null(), "a stale row reads as no one: {rows:?}");
    assert_eq!(rows[2]["assignees"], json!(["bob", "carol"]), "{rows:?}");
}
