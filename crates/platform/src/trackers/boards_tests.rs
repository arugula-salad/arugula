//! A Plane item on a board and in the work search (Arugula #357), through
//! the platform's own routes against `FakeTracker`: a card reads
//! `ARUGU-12` with Plane's name for its state, closing it moves the
//! board's progress, and only the project's members read a connection's
//! items.

use std::sync::Arc;

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use super::{
    connect,
    connect_tests::{KEY, backfilled, connect_as, serve},
    fake::item,
};
use crate::{
    boards::FLAG,
    chat_tests::{TestHub, call},
    db::{Sql as _, sql::params, trackers::NewConnection},
    forge::viewer,
};

async fn board(base: &str, who: &str, project: &str, pointers: &[Value]) -> String {
    let (s, b) =
        call(base, who, "POST", &format!("/api/projects/{project}/boards"), Some(json!({ "title": "Plane" }))).await;
    assert_eq!(s, 200, "{b}");
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let path = format!("/api/boards/{id}/entries");
    for ptr in pointers {
        let body = json!({ "entry": "row", "note": "n", "pointer": ptr });
        let (s, v) = call(base, who, "PUT", &path, Some(body)).await;
        assert_eq!(s, 200, "{v}");
    }
    id
}

/// Every row of a board, lane by lane.
fn lane_rows(v: &Value) -> Vec<&Value> {
    v["zones"].as_array().unwrap().iter().flat_map(|z| z["rows"].as_array().unwrap()).collect()
}

async fn read(base: &str, who: &str, id: &str) -> Value {
    let (s, v) = call(base, who, "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(s, 200, "{v}");
    v
}

fn progress(v: &Value) -> (u64, u64) {
    (v["board"]["progress"]["done"].as_u64().unwrap(), v["board"]["progress"]["total"].as_u64().unwrap())
}

fn mirrored_id(app: &crate::Platform, mirror: i64, number: u64) -> String {
    app.db
        .c()
        .query_row("SELECT id FROM forge_items WHERE repo_id = ?1 AND number = ?2", params![mirror, number], |r| {
            r.get(0)
        })
        .unwrap()
}

async fn work(base: &str, who: &str, project: &str, query: &str) -> Vec<Value> {
    let (s, v) = call(base, who, "GET", &format!("/api/projects/{project}/work{query}"), None).await;
    assert_eq!(s, 200, "{v}");
    v["items"].as_array().cloned().unwrap()
}

/// A connected project's item is added to a board and reads `ARUGU-2` with
/// Plane's own state; closing it in Plane moves the progress. It's in the
/// picker and in Triage until a board points at it. Another project's board
/// that points at it reads nothing and counts nothing, and GitHub's and
/// native cards' JSON has no new keys.
#[tokio::test]
async fn a_plane_item_is_on_a_board_and_closing_it_moves_the_progress() {
    let hub = Arc::new(TestHub::default());
    let (_fake, app, base) = serve(&hub).await;
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("carol", TeamRole::Viewer)]);
    hub.flag_named("team:t1", FLAG, true);
    hub.team("t3", &[("erin", TeamRole::Owner)]);
    hub.flag("team:t3", true);
    hub.flag_named("team:t3", FLAG, true);
    let team = crate::projects::team_project(&app, "t1").await.unwrap().unwrap();
    let other = crate::projects::team_project(&app, "t3").await.unwrap().unwrap();

    let (s, made) = connect_as(&base, "bob", &team, KEY, "ARUGU").await;
    assert_eq!(s, 200, "{made}");
    let conn = made["id"].as_str().unwrap().to_owned();
    backfilled(&app, &team, &conn).await;
    let mirror = app.db.tracker_connection(&team, &conn).unwrap().unwrap().mirror_id;
    let (second, third) = (mirrored_id(&app, mirror, 2), mirrored_id(&app, mirror, 3));

    // Before it's on a board, Triage has the open items (newest first: 1, then 2), not the completed one.
    let triage = work(&base, "carol", &team, "?untriaged=true").await;
    let seen: Vec<_> = triage
        .iter()
        .map(|i| (i["source"].as_str().unwrap(), i["identifier"].as_str(), i["number"].as_u64()))
        .collect();
    assert_eq!(seen, [("plane", Some("ARUGU"), Some(1)), ("plane", Some("ARUGU"), Some(2))], "{triage:?}");
    assert_eq!(triage[0]["url"], "https://app.plane.so/acme/browse/ARUGU-1/");
    assert_eq!(
        (triage[1]["kind"].as_str(), triage[1]["repo"].as_str(), triage[1]["state"].as_str()),
        (Some("issue"), Some("Arugula"), Some("open"))
    );
    // The picker has all three, and a search and the kind filter apply.
    assert_eq!(work(&base, "bob", &team, "").await.len(), 3);
    assert_eq!(work(&base, "bob", &team, "?q=thir").await.len(), 1);
    assert!(work(&base, "bob", &team, "?kind=pr").await.is_empty());

    let id = board(&base, "bob", &team, &[json!({ "kind": "forge_item", "id": second })]).await;
    let v = read(&base, "bob", &id).await;
    let live = &lane_rows(&v)[0]["live"];
    assert_eq!(
        (live["identifier"].as_str(), live["number"].as_u64(), live["state_name"].as_str(), live["state"].as_str()),
        (Some("ARUGU"), Some(2), Some("started"), Some("open")),
        "{live}"
    );
    assert_eq!(
        (live["provider"].as_str(), live["kind"].as_str(), live["repo"].as_str()),
        (Some("plane"), Some("issue"), Some("Arugula"))
    );
    assert_eq!(live["url"], "https://app.plane.so/acme/browse/ARUGU-2/");
    assert_eq!(progress(&v), (0, 1));
    // It's on a board now: out of Triage.
    let triage = work(&base, "bob", &team, "?untriaged=true").await;
    assert_eq!(triage.iter().map(|i| i["number"].as_u64().unwrap()).collect::<Vec<_>>(), [1]);

    // Closed in Plane (an ingest of a completed state).
    let mut done = item(2, "Second", "completed", 5_000);
    done.state_name = "Done".into();
    assert_eq!(connect::ingest_page(&app, &conn, mirror, None, &[done], 6_000).unwrap().map(|ids| ids.len()), Some(1));
    let v = read(&base, "carol", &id).await;
    let live = &lane_rows(&v)[0]["live"];
    assert_eq!(
        (live["state"].as_str(), live["state_name"].as_str(), live["number"].as_u64()),
        (Some("closed"), Some("Done"), Some(2)),
        "{live}"
    );
    assert_eq!(progress(&v), (1, 1));

    // erin's project has a connection of its own, so the check isn't empty for her: it still isn't this one.
    let theirs_conn = NewConnection {
        id: "tc_theirs".into(),
        project_id: other.clone(),
        workspace_slug: "other".into(),
        source_project_id: "pl-proj-9".into(),
        identifier: "OTH".into(),
        name: "Other".into(),
        linked_by: "erin".into(),
    };
    app.db.connect_tracker(&theirs_conn, 1).unwrap().unwrap();

    // Another project's board that points at this item, and at a native issue of its own.
    let (_, native) =
        call(&base, "erin", "POST", &format!("/api/projects/{other}/issues"), Some(json!({ "title": "Mine" }))).await;
    let native = native["id"].as_str().unwrap().to_owned();
    let theirs = board(
        &base,
        "erin",
        &other,
        &[json!({ "kind": "forge_item", "id": third }), json!({ "kind": "issue", "id": native })],
    )
    .await;
    let v = read(&base, "erin", &theirs).await;
    let rows = lane_rows(&v);
    assert!(rows[0]["live"].is_null(), "{rows:?}");
    assert_eq!(progress(&v), (0, 2), "both rows count; the other project's item says nothing, not even closed");
    let native_live = rows[1]["live"].as_object().unwrap();
    for key in ["provider", "identifier", "state_name", "url"] {
        assert!(!native_live.contains_key(key), "{key} on a native card: {native_live:?}");
    }
    // Nor does erin's work search, or a read of the other project's, show it.
    assert!(work(&base, "erin", &other, "").await.iter().all(|i| i["source"] != "plane"));
    let (s, _) = call(&base, "erin", "GET", &format!("/api/projects/{team}/work"), None).await;
    assert_eq!(s, 404);

    // The check itself: a member of the project reads the connection (no GitHub sign-in needed), a stranger doesn't.
    let conns = app.db.tracker_connections(&team).unwrap();
    let who = |account: &str| viewer::Viewer { account: account.to_owned(), github: None };
    assert_eq!(viewer::check_connections(&app, &who("carol"), &conns).await.unwrap().mirror_ids(), [mirror]);
    assert!(viewer::check_connections(&app, &who("erin"), &conns).await.unwrap().is_empty());

    // Disconnected: the card reads nothing and its lane stops following the item, and the work search has none of it.
    let (s, _) = call(&base, "bob", "DELETE", &format!("/api/projects/{team}/trackers/{conn}"), None).await;
    assert_eq!(s, 200);
    let v = read(&base, "bob", &id).await;
    assert!(lane_rows(&v)[0]["live"].is_null());
    assert_eq!(progress(&v), (0, 1));
    assert!(work(&base, "bob", &team, "").await.iter().all(|i| i["source"] != "plane"));
}

/// A Plane `blocked_by` shows on the row (Arugula #403): the backfill reads
/// each item's relations and stores them as `plane` edges, the row of the
/// blocked item says "blocked by ARUGU's #1" and is out of the ready set,
/// the edge can't be deleted here, and a reconcile that finds it gone in
/// Plane takes it off.
#[tokio::test]
async fn a_plane_blocked_by_shows_on_the_row_and_stays_read_only() {
    use crate::trackers::connect::Reconciled;
    use arugula_control_wire::boards::project_relation;

    let hub = Arc::new(TestHub::default());
    let (fake, app, base) = serve(&hub).await;
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("carol", TeamRole::Viewer)]);
    hub.flag_named("team:t1", FLAG, true);
    let team = crate::projects::team_project(&app, "t1").await.unwrap().unwrap();
    // Plane says item-1 blocks item-2 (and, from the other side, item-2 is blocked by it).
    fake.set_blocks(vec![("item-1", "item-2")]);
    let (s, made) = connect_as(&base, "bob", &team, KEY, "ARUGU").await;
    assert_eq!(s, 200, "{made}");
    let conn = made["id"].as_str().unwrap().to_owned();
    backfilled(&app, &team, &conn).await;
    let mirror = app.db.tracker_connection(&team, &conn).unwrap().unwrap().mirror_id;
    let (first, second) = (mirrored_id(&app, mirror, 1), mirrored_id(&app, mirror, 2));

    let id = board(
        &base,
        "bob",
        &team,
        &[json!({ "kind": "forge_item", "id": first }), json!({ "kind": "forge_item", "id": second })],
    )
    .await;
    for who in ["bob", "carol"] {
        let v = read(&base, who, &id).await;
        let blocked = lane_rows(&v).into_iter().find(|r| r["pointer"]["id"] == json!(second)).unwrap().clone();
        assert_eq!(
            blocked["marks"],
            json!([{ "kind": "blocked_by", "number": 1, "title": "First", "repo": "Arugula" }]),
            "{who}: {blocked}"
        );
        assert_eq!(blocked["relations"][0]["source"], json!("plane"), "{who}");
        assert_eq!(blocked["relations"][0]["removable"], json!(false), "{who}: it is Plane's");
        let ready: Vec<_> = v["ready"].as_array().unwrap().iter().collect();
        let first_row = lane_rows(&v).into_iter().find(|r| r["pointer"]["id"] == json!(first)).unwrap()["id"].clone();
        assert_eq!(ready, [&first_row], "{who}: only the blocker can start: {v}");
    }

    // Read-only here: deleting it is refused, saying where it comes from.
    let v = read(&base, "bob", &id).await;
    let blocked = lane_rows(&v).into_iter().find(|r| r["pointer"]["id"] == json!(second)).unwrap().clone();
    let rel = blocked["relations"][0]["id"].as_str().unwrap().to_owned();
    let (s, v) = call(&base, "bob", "DELETE", &project_relation(&team, &rel), None).await;
    assert_eq!((s, v["error"].as_str()), (403, Some("it comes from Plane")), "{v}");

    // Plane drops it; the next reconcile reads the item again and the edge goes.
    fake.set_blocks(vec![]);
    fake.set_items(vec![
        item(1, "First", "backlog", 9_000_000_000_000),
        item(2, "Second", "started", 9_000_000_000_001),
        item(3, "Third", "completed", 1_000),
    ]);
    let due = arugula_e2e::now_ms() + 24 * 3_600_000;
    assert!(matches!(connect::reconcile(&app, due).await.unwrap(), Some(Reconciled::Done { .. })));
    let v = read(&base, "bob", &id).await;
    let blocked = lane_rows(&v).into_iter().find(|r| r["pointer"]["id"] == json!(second)).unwrap().clone();
    assert!(blocked.get("marks").is_none() && blocked.get("relations").is_none(), "{blocked}");
    assert_eq!(v["ready"].as_array().unwrap().len(), 2, "{v}");
}
