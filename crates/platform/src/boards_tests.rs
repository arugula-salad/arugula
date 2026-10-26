//! Boards through the platform's routes (live status board, step 1), with
//! the hub of chat's tests: a board made, zones and rows added and
//! reordered, a stale write refused, and the `boards` flag off answering
//! 404.

use std::sync::Arc;

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use super::FLAG;
use crate::chat_tests::{TestHub, call, serve};

/// Team t: a owns it, v views. Chat is on for it, and boards when `boards`
/// is. c is in team u, which has neither. t's project id. (The platform
/// caches flags, so a test sets them before its first request.)
fn seed(app: &crate::Platform, hub: &TestHub, boards: bool) -> String {
    hub.team("t", &[("a", TeamRole::Owner), ("v", TeamRole::Viewer)]);
    hub.team("u", &[("c", TeamRole::Owner)]);
    hub.flag("team:t", true);
    hub.flag_named("team:t", FLAG, boards);
    hub.flag("team:u", true);
    hub.flag_named("team:u", FLAG, boards);
    app.db.team_project("t", "t", 1).unwrap()
}

async fn put(base: &str, who: &str, board: &str, body: Value) -> (u16, Value) {
    call(base, who, "PUT", &format!("/api/boards/{board}/entries"), Some(body)).await
}

async fn mv(base: &str, who: &str, board: &str, entry: &str, body: Value) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/boards/{board}/entries/{entry}/move"), Some(body)).await
}

/// A board's zones as (title, [row notes]).
fn shape(v: &Value) -> Vec<(String, Vec<String>)> {
    v["zones"]
        .as_array()
        .unwrap()
        .iter()
        .map(|z| {
            let rows = z["rows"].as_array().unwrap().iter().map(|r| r["note"].as_str().unwrap().to_owned()).collect();
            (z["title"].as_str().unwrap().to_owned(), rows)
        })
        .collect()
}

fn zone(title: &str, rows: &[&str]) -> (String, Vec<String>) {
    (title.to_owned(), rows.iter().map(|r| (*r).to_owned()).collect())
}

/// Make a board, add zones and rows, reorder them, and see a stale write
/// refused; the status of a row changes with no rev and never conflicts
/// with a note.
#[tokio::test]
async fn a_board_from_creation_to_a_stale_write() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let p = seed(&app, &hub, true);

    let (st, b) =
        call(&base, "a", "POST", &format!("/api/projects/{p}/boards"), Some(json!({ "title": "Launch" }))).await;
    assert_eq!((st, &b["board"]["title"], &b["zones"]), (200, &json!("Launch"), &json!([])), "{b}");
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("brd_"), "{id}");
    let (st, list) = call(&base, "v", "GET", &format!("/api/projects/{p}/boards"), None).await;
    assert_eq!((st, &list["boards"][0]["id"]), (200, &json!(id)), "a viewer lists: {list}");

    // Zones: the proofs zone is made first and stays last.
    let (st, v) = put(&base, "a", &id, json!({ "entry": "zone", "title": "PROOFS", "kind": "proofs" })).await;
    assert_eq!(st, 200, "{v}");
    let (_, v) = put(&base, "a", &id, json!({ "entry": "zone", "title": "Now" })).await;
    let (_, v2) = put(&base, "a", &id, json!({ "entry": "zone", "title": "Later" })).await;
    assert_eq!(shape(&v2).iter().map(|z| z.0.as_str()).collect::<Vec<_>>(), ["Now", "Later", "PROOFS"], "{v} {v2}");
    assert_eq!(v2["zones"][2]["kind"], json!("proofs"));
    let (st, dup) = put(&base, "a", &id, json!({ "entry": "zone", "title": "More proofs", "kind": "proofs" })).await;
    assert_eq!(st, 400, "one proofs zone: {dup}");
    let (now, later, proofs) = (
        v2["zones"][0]["id"].as_str().unwrap().to_owned(),
        v2["zones"][1]["id"].as_str().unwrap().to_owned(),
        v2["zones"][2]["id"].as_str().unwrap().to_owned(),
    );

    // Rows: free text, an issue, and an agent block.
    let (st, _) = put(&base, "a", &id, json!({ "entry": "row", "zone": now, "note": "write the plan" })).await;
    assert_eq!(st, 200);
    let (_, _) = put(
        &base,
        "a",
        &id,
        json!({ "entry": "row", "zone": now, "note": "fix login",
                "pointer": { "kind": "issue", "id": "iss_0123456789abcdef0123456789abcdef" }, "status": "working" }),
    )
    .await;
    let (st, v) = put(
        &base,
        "a",
        &id,
        json!({ "entry": "row", "zone": later, "note": "agent on it",
                "pointer": { "kind": "block", "machine": "m1", "pane": "p2" }, "status": "needs_input" }),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        shape(&v),
        [zone("Now", &["write the plan", "fix login"]), zone("Later", &["agent on it"]), zone("PROOFS", &[])]
    );
    let plan = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let login = v["zones"][0]["rows"][1]["id"].as_str().unwrap().to_owned();
    let agent = v["zones"][1]["rows"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(v["zones"][0]["rows"][0]["pointer"], json!({ "kind": "none" }));
    assert_eq!(v["zones"][0]["rows"][1]["pointer"]["id"], json!("iss_0123456789abcdef0123456789abcdef"));
    assert_eq!(v["zones"][1]["rows"][0]["status"], json!("needs_input"));

    // Reorder within a zone, then between zones, then the zones.
    let (st, v) = mv(&base, "a", &id, &login, json!({ "before": plan })).await;
    assert_eq!((st, shape(&v)[0].clone()), (200, zone("Now", &["fix login", "write the plan"])), "{v}");
    let (_, v) = mv(&base, "a", &id, &agent, json!({ "zone": now, "before": plan })).await;
    assert_eq!(
        shape(&v),
        [zone("Now", &["fix login", "agent on it", "write the plan"]), zone("Later", &[]), zone("PROOFS", &[])]
    );
    let (_, v) = mv(&base, "a", &id, &now, json!({ "before": later })).await;
    assert_eq!(shape(&v).len(), 3, "{v}");
    let (_, v) = mv(&base, "a", &id, &later, json!({ "before": now })).await;
    assert_eq!(shape(&v).iter().map(|z| z.0.as_str()).collect::<Vec<_>>(), ["Later", "Now", "PROOFS"], "{v}");
    let (st, v) = mv(&base, "a", &id, &now, json!({})).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        shape(&v).iter().map(|z| z.0.as_str()).collect::<Vec<_>>(),
        ["Later", "Now", "PROOFS"],
        "last means before proofs: {v}"
    );
    assert_eq!(mv(&base, "a", &id, &proofs, json!({ "before": later })).await.0, 400, "proofs stays last");

    // A note takes the rev it read; a second write on the old rev is stale.
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    let rev = v["zones"][1]["rows"][0]["rev"].clone();
    assert_eq!(rev, json!(1));
    let (st, v) = put(&base, "a", &id, json!({ "id": login, "rev": rev, "note": "fix login, again" })).await;
    assert_eq!((st, &v["zones"][1]["rows"][0]["rev"]), (200, &json!(2)), "{v}");
    let (st, stale) = put(&base, "a", &id, json!({ "id": login, "rev": rev, "note": "a stale note" })).await;
    assert_eq!((st, &stale["reason"]), (409, &json!("stale_rev")), "{stale}");
    assert_eq!(put(&base, "a", &id, json!({ "id": login, "note": "no rev" })).await.0, 400, "a note names its rev");
    // A status needs no rev, and leaves it: the next note still lands.
    let (st, v) = put(&base, "a", &id, json!({ "id": login, "status": "done" })).await;
    assert_eq!(
        (st, &v["zones"][1]["rows"][0]["status"], &v["zones"][1]["rows"][0]["rev"]),
        (200, &json!("done"), &json!(2)),
        "{v}"
    );
    let (st, v) = put(&base, "a", &id, json!({ "id": login, "rev": 2, "note": "fixed" })).await;
    assert_eq!(
        (st, &v["zones"][1]["rows"][0]["note"], &v["zones"][1]["rows"][0]["status"]),
        (200, &json!("fixed"), &json!("done")),
        "{v}"
    );

    // A viewer reads and changes nothing; a stranger finds nothing.
    assert_eq!(call(&base, "v", "GET", &format!("/api/boards/{id}"), None).await.0, 200);
    assert_eq!(put(&base, "v", &id, json!({ "id": login, "status": "open" })).await.0, 403);
    assert_eq!(mv(&base, "v", &id, &login, json!({})).await.0, 403);
    assert_eq!(call(&base, "v", "DELETE", &format!("/api/boards/{id}/entries/{login}"), None).await.0, 403);
    assert_eq!(call(&base, "c", "GET", &format!("/api/boards/{id}"), None).await.0, 404);

    // Deleting a row, then a zone with its rows.
    let (st, v) = call(&base, "a", "DELETE", &format!("/api/boards/{id}/entries/{login}"), None).await;
    assert_eq!((st, shape(&v)[1].clone()), (200, zone("Now", &["agent on it", "write the plan"])), "{v}");
    let (st, v) = call(&base, "a", "DELETE", &format!("/api/boards/{id}/entries/{now}"), None).await;
    assert_eq!((st, shape(&v)), (200, vec![zone("Later", &[]), zone("PROOFS", &[])]), "{v}");
    assert_eq!(call(&base, "a", "DELETE", &format!("/api/boards/{id}/entries/{now}"), None).await.0, 404);

    // The board's thread is an object thread, as an issue's is (with chat on).
    let open = |who: &'static str| {
        call(&base, who, "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": id })))
    };
    let (st, room) = open("a").await;
    assert_eq!((st, &room["name"]), (200, &json!("board Launch")), "{room}");
    assert_eq!(open("c").await.0, 404, "a stranger doesn't find it");
}

/// With the flag off for an account (chat on doesn't turn it on), every
/// route answers 404 as if boards weren't there, a board that exists
/// included.
#[tokio::test]
async fn with_the_flag_off_there_are_no_boards() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let p = seed(&app, &hub, false);
    let id = app.db.create_board(&p, "Launch", "a", 1, 10).unwrap().unwrap().id;

    for (method, path, body) in [
        ("GET", format!("/api/projects/{p}/boards"), None),
        ("POST", format!("/api/projects/{p}/boards"), Some(json!({ "title": "More" }))),
        ("GET", format!("/api/boards/{id}"), None),
        ("PUT", format!("/api/boards/{id}/entries"), Some(json!({ "entry": "zone", "title": "Now" }))),
        ("POST", format!("/api/boards/{id}/entries/zon_0123/move"), Some(json!({}))),
        ("DELETE", format!("/api/boards/{id}/entries/zon_0123"), None),
    ] {
        assert_eq!(call(&base, "a", method, &path, body).await.0, 404, "{method} {path}");
    }
    assert_eq!(app.db.boards(&p).unwrap().len(), 1, "nothing was made");
}
