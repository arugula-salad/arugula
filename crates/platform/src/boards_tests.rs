//! Boards through the platform's routes (boards, step 1; lanes, aru-125),
//! with the hub of chat's tests: a board made, rows added and changed, work
//! and proof attached, a stale write refused, what lanes refuse, and the
//! `boards` flag off answering 404.

use std::sync::Arc;

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use super::FLAG;
use crate::{
    chat_tests::{TestHub, call, serve},
    db::{
        issues::Actor,
        sql::{Sql as _, params},
    },
};

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

async fn attach(base: &str, who: &str, board: &str, body: Value) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/boards/{board}/attachments"), Some(body)).await
}

/// Every row of a board, lane by lane.
pub(super) fn rows_in(v: &Value) -> Vec<&Value> {
    v["zones"].as_array().unwrap().iter().flat_map(|z| z["rows"].as_array().unwrap()).collect()
}

/// The row `id`.
pub(super) fn row_in<'a>(v: &'a Value, id: &str) -> &'a Value {
    rows_in(v).into_iter().find(|r| r["id"] == id).unwrap_or_else(|| panic!("no row {id}: {v}"))
}

/// The lane the row `id` is in: `next`, `doing`, `review` or `done`.
pub(super) fn lane(v: &Value, id: &str) -> String {
    let z = v["zones"].as_array().unwrap().iter().find(|z| z["rows"].as_array().unwrap().iter().any(|r| r["id"] == id));
    let z = z.unwrap_or_else(|| panic!("no row {id}: {v}"));
    assert_eq!(row_in(v, id)["lane"], z["id"].as_str().unwrap().replace("lane_", ""), "a row's lane is its zone");
    row_in(v, id)["lane"].as_str().unwrap().to_owned()
}

/// A board's lanes as (title, [row notes]).
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

fn lanes(next: &[&str], doing: &[&str], review: &[&str], done: &[&str]) -> Vec<(String, Vec<String>)> {
    [("Next", next), ("Doing", doing), ("Review", review), ("Done", done)]
        .into_iter()
        .map(|(t, r)| (t.to_owned(), r.iter().map(|r| (*r).to_owned()).collect()))
        .collect()
}

/// A native issue in `project`: its `iss_` id.
pub(super) async fn issue(base: &str, who: &str, project: &str, title: &str) -> String {
    let (st, i) =
        call(base, who, "POST", &format!("/api/projects/{project}/issues"), Some(json!({ "title": title }))).await;
    assert_eq!(st, 200, "{i}");
    i["id"].as_str().unwrap().to_owned()
}

/// A run of `project` in `state`: its `run_` id.
fn run_in(app: &crate::Platform, project: &str, state: &str) -> String {
    let op = "arugula op v1\ndaemon d1\nkind run\nsession -\npane - -\ncwd -\nargv ZWNobw==\n";
    let new = crate::db::runs::NewRun {
        project,
        workspace: None,
        trigger: crate::db::runs::TriggerKind::Manual,
        source: &json!({}),
        op,
        started_by: "a",
        started_by_agent: None,
        grant: Some("g1"),
    };
    let run = app.db.create_run(&new, 1).unwrap().unwrap().id;
    app.db.c().execute("UPDATE runs SET state = ?2 WHERE id = ?1", params![run, state]).unwrap();
    run
}

/// Make a board, add rows (a note, an issue), see a stale write refused, and
/// the four lanes it is always read as.
#[tokio::test]
async fn a_board_from_creation_to_a_stale_write() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let p = seed(&app, &hub, true);

    let (st, b) =
        call(&base, "a", "POST", &format!("/api/projects/{p}/boards"), Some(json!({ "title": "Launch" }))).await;
    assert_eq!((st, &b["board"]["title"]), (200, &json!("Launch")), "{b}");
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("brd_"), "{id}");
    // Always the four lanes, in order, with ids of their own and rev 0.
    let zones = b["zones"].as_array().unwrap();
    let got: Vec<_> =
        zones.iter().map(|z| (z["id"].as_str().unwrap(), z["kind"].as_str().unwrap(), &z["rev"])).collect();
    assert_eq!(
        got,
        [
            ("lane_next", "lane", &json!(0)),
            ("lane_doing", "lane", &json!(0)),
            ("lane_review", "lane", &json!(0)),
            ("lane_done", "lane", &json!(0))
        ]
    );
    assert_eq!(shape(&b), lanes(&[], &[], &[], &[]));
    let (st, list) = call(&base, "v", "GET", &format!("/api/projects/{p}/boards"), None).await;
    assert_eq!((st, &list["boards"][0]["id"]), (200, &json!(id)), "a viewer lists: {list}");

    // Rows: free text, and an issue (an id the platform has no issue for
    // reads as no state: it stays in Next). A zone in the body is ignored by
    // nobody: it is refused (below).
    let (st, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "write the plan" })).await;
    assert_eq!(st, 200, "{v}");
    let login = issue(&base, "a", &p, "Fix login").await;
    let (st, v) = put(
        &base,
        "a",
        &id,
        json!({ "entry": "row", "note": "fix login", "pointer": { "kind": "issue", "id": login } }),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(shape(&v), lanes(&["write the plan", "fix login"], &[], &[], &[]));
    let plan = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let fix = v["zones"][0]["rows"][1]["id"].as_str().unwrap().to_owned();
    assert_eq!(v["zones"][0]["rows"][0]["pointer"], json!({ "kind": "none" }));
    assert_eq!(
        (v["zones"][0]["rows"][1]["pointer"]["id"].as_str(), v["zones"][0]["rows"][1]["lane"].as_str()),
        (Some(login.as_str()), Some("next"))
    );
    assert_eq!(v["board"]["progress"], json!({ "done": 0, "total": 1, "proven": 0 }), "a note is never counted");
    // A run, an agent block or a workspace is attached, not a row's item.
    for ptr in [
        json!({ "kind": "run", "id": "run_ab12" }),
        json!({ "kind": "block", "machine": "m1", "pane": "p2" }),
        json!({ "kind": "workspace", "id": "hw_ab12" }),
    ] {
        let (st, v) = put(&base, "a", &id, json!({ "entry": "row", "pointer": ptr })).await;
        assert_eq!(st, 400, "{ptr}: {v}");
        assert!(v.to_string().contains("attach"), "{v}");
    }

    // A note takes the rev it read; a second write on the old rev is stale.
    let (st, v) = put(&base, "a", &id, json!({ "id": fix, "rev": 1, "note": "fix login, again" })).await;
    assert_eq!((st, &row_in(&v, &fix)["rev"]), (200, &json!(2)), "{v}");
    let (st, stale) = put(&base, "a", &id, json!({ "id": fix, "rev": 1, "note": "a stale note" })).await;
    assert_eq!((st, &stale["reason"]), (409, &json!("stale_rev")), "{stale}");
    assert_eq!(put(&base, "a", &id, json!({ "id": fix, "note": "no rev" })).await.0, 400, "a note names its rev");

    // A viewer reads and changes nothing; a stranger finds nothing.
    assert_eq!(call(&base, "v", "GET", &format!("/api/boards/{id}"), None).await.0, 200);
    assert_eq!(put(&base, "v", &id, json!({ "id": fix, "rev": 2, "note": "x" })).await.0, 403);
    assert_eq!(mv(&base, "v", &id, &fix, json!({})).await.0, 403);
    assert_eq!(
        attach(
            &base,
            "v",
            &id,
            json!({ "row": fix, "role": "proof", "pointer": { "kind": "link", "url": "https://x.example/1" } })
        )
        .await
        .0,
        403
    );
    assert_eq!(call(&base, "v", "DELETE", &format!("/api/boards/{id}/entries/{fix}"), None).await.0, 403);
    assert_eq!(call(&base, "c", "GET", &format!("/api/boards/{id}"), None).await.0, 404);

    // Deleting a row.
    let (st, v) = call(&base, "a", "DELETE", &format!("/api/boards/{id}/entries/{fix}"), None).await;
    assert_eq!((st, shape(&v)), (200, lanes(&["write the plan"], &[], &[], &[])), "{v}");
    assert_eq!(call(&base, "a", "DELETE", &format!("/api/boards/{id}/entries/{fix}"), None).await.0, 404);
    let _ = plan;
}

/// Nobody moves a row or makes a zone, and nobody sets a status but
/// `needs_input`: each is refused in the words "lanes follow the work", for
/// a person; the rows stay where the work puts them.
#[tokio::test]
async fn lanes_follow_the_work_and_refuse_the_rest() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let p = seed(&app, &hub, true);
    let (_, b) = call(&base, "a", "POST", &format!("/api/projects/{p}/boards"), Some(json!({ "title": "B" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "the plan" })).await;
    let plan = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let follows = |r: &(u16, Value)| {
        assert_eq!(r.0, 400, "{}", r.1);
        assert!(r.1["error"].as_str().unwrap().starts_with("lanes follow the work: "), "{}", r.1);
    };
    // Zones.
    follows(&put(&base, "a", &id, json!({ "entry": "zone", "title": "Now" })).await);
    follows(&put(&base, "a", &id, json!({ "entry": "zone", "title": "Next", "kind": "lane" })).await);
    follows(&put(&base, "a", &id, json!({ "entry": "row", "zone": "zon_0123", "note": "x" })).await);
    follows(&put(&base, "a", &id, json!({ "id": "zon_0123", "rev": 1, "title": "Later" })).await);
    follows(&call(&base, "a", "DELETE", &format!("/api/boards/{id}/entries/zon_0123"), None).await);
    // Moves.
    follows(&mv(&base, "a", &id, &plan, json!({})).await);
    follows(&mv(&base, "a", &id, &plan, json!({ "zone": "lane_done" })).await);
    follows(&mv(&base, "a", &id, "zon_0123", json!({ "before": "zon_0456" })).await);
    // A status, but needs_input (and none, which clears it).
    for s in ["open", "working", "done", "failed"] {
        follows(&put(&base, "a", &id, json!({ "id": plan, "status": s })).await);
    }
    let (st, v) = put(&base, "a", &id, json!({ "id": plan, "status": "needs_input" })).await;
    assert_eq!(
        (st, &row_in(&v, &plan)["status"], &row_in(&v, &plan)["rev"]),
        (200, &json!("needs_input"), &json!(1)),
        "{v}"
    );
    let (st, v) = put(&base, "a", &id, json!({ "id": plan, "status": "none" })).await;
    assert_eq!((st, &row_in(&v, &plan)["status"]), (200, &json!("none")), "{v}");
    // A new row with a status is refused the same way.
    follows(&put(&base, "a", &id, json!({ "entry": "row", "note": "x", "status": "done" })).await);
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(shape(&v), lanes(&["the plan"], &[], &[], &[]), "nothing moved, nothing was made");
}

/// What is attached to a row: work moves its lane, proof is a mark; a run
/// shows its state to a reader of its project and nothing to one who isn't,
/// and a link is kept as it was said and refused if it isn't plain https.
#[tokio::test]
async fn work_and_proof_attach_to_a_row() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let p = seed(&app, &hub, true);
    let (_, b) = call(&base, "a", "POST", &format!("/api/projects/{p}/boards"), Some(json!({ "title": "B" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let login = issue(&base, "a", &p, "Fix login").await;
    let (_, v) =
        put(&base, "a", &id, json!({ "entry": "row", "note": "fix", "pointer": { "kind": "issue", "id": login } }))
            .await;
    let fix = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "just a note" })).await;
    let note = v["zones"][0]["rows"][1]["id"].as_str().unwrap().to_owned();

    // Proof: a link, kept as said; the shapes refused.
    let link = |url: &str| json!({ "row": fix, "role": "proof", "pointer": { "kind": "link", "url": url } });
    let url = "https://ci.example.com/runs/42?x=1#top";
    let (st, v) = attach(&base, "a", &id, link(url)).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(row_in(&v, &fix)["attachments"][0]["pointer"], json!({ "kind": "link", "url": url }), "{v}");
    assert!(row_in(&v, &fix)["attachments"][0]["id"].as_str().unwrap().starts_with("att_"));
    assert_eq!(row_in(&v, &fix)["attachments"][0].get("live"), None, "a link has no live state");
    let long = format!("https://example.com/{}", "a".repeat(2048));
    for bad in [
        "http://ci.example.com/x",
        "https://user:pw@ci.example.com/x",
        "https://ci.example.com@evil.example/x",
        "https://",
        "https://ci.example.com/a b",
        "https://ci.example.com/a\u{7}b",
        "javascript:alert(1)",
        long.as_str(),
    ] {
        let (st, v) = attach(&base, "a", &id, link(bad)).await;
        assert_eq!(st, 400, "{bad}: {v}");
        assert!(v.to_string().contains("https"), "{bad}: {v}");
    }
    // The roles take their own pointers.
    let (st, v) =
        attach(&base, "a", &id, json!({ "row": fix, "role": "work", "pointer": { "kind": "link", "url": url } })).await;
    assert_eq!(st, 400, "a link isn't work: {v}");
    let (st, v) = attach(
        &base,
        "a",
        &id,
        json!({ "row": fix, "role": "proof", "pointer": { "kind": "block", "machine": "m", "pane": "1" } }),
    )
    .await;
    assert_eq!(st, 400, "a block isn't proof: {v}");
    let (st, v) = attach(
        &base,
        "a",
        &id,
        json!({ "row": "row_0123", "role": "proof", "pointer": { "kind": "link", "url": url } }),
    )
    .await;
    assert_eq!(st, 404, "no such row: {v}");
    // The same link again changes nothing, and says where it is.
    let (st, v) = attach(&base, "a", &id, link(url)).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(row_in(&v, &fix)["attachments"].as_array().unwrap().len(), 1, "{v}");
    assert!(v["said"].as_str().unwrap().contains(&fix), "{v}");

    // Work: runs show their state to a reader of their project.
    let theirs = app.db.team_project("u", "u", 1).unwrap();
    let (ok, running, canceled, hidden) = (
        run_in(&app, &p, "succeeded"),
        run_in(&app, &p, "running"),
        run_in(&app, &p, "canceled"),
        run_in(&app, &theirs, "running"),
    );
    for r in [&ok, &running, &canceled, &hidden] {
        let (st, v) =
            attach(&base, "a", &id, json!({ "row": fix, "role": "work", "pointer": { "kind": "run", "id": r } })).await;
        assert_eq!(st, 200, "{v}");
    }
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    let at = |i: usize| row_in(&v, &fix)["attachments"][i].clone();
    assert_eq!(at(1)["live"], json!({ "state": "succeeded", "kind": "run", "number": 0, "title": "manual run" }));
    assert_eq!(at(2)["live"]["state"], json!("running"));
    assert_eq!(at(3)["live"]["state"], json!("other"));
    assert_eq!(at(4).get("live"), None, "a run of a project the reader isn't in says nothing");
    assert_eq!(at(4)["pointer"]["id"], json!(hidden));
    assert_eq!(lane(&v, &fix), "doing", "a run running: Doing, a hidden one included");
    assert_eq!(lane(&v, &note), "next", "a note is never moved");

    // A run on a note row doesn't move it.
    let (st, v) =
        attach(&base, "a", &id, json!({ "row": note, "role": "work", "pointer": { "kind": "run", "id": running } }))
            .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        row_in(&v, &note)["attachments"].as_array().unwrap().len(),
        0,
        "an item on the board is not attached again: {v}"
    );
    let nrun = run_in(&app, &p, "running");
    let (st, v) =
        attach(&base, "a", &id, json!({ "row": note, "role": "work", "pointer": { "kind": "run", "id": nrun } })).await;
    assert_eq!((st, lane(&v, &note)), (200, "next".to_owned()), "{v}");

    // Taking one off.
    let att = row_in(&v, &fix)["attachments"][0]["id"].as_str().unwrap().to_owned();
    let (st, v) = call(&base, "a", "DELETE", &format!("/api/boards/{id}/attachments/{att}"), None).await;
    assert_eq!((st, row_in(&v, &fix)["attachments"].as_array().unwrap().len()), (200, 4), "{v}");
    assert_eq!(call(&base, "a", "DELETE", &format!("/api/boards/{id}/attachments/{att}"), None).await.0, 404);
    assert_eq!(call(&base, "v", "DELETE", &format!("/api/boards/{id}/attachments/{att}"), None).await.0, 403);
    // Deleting the row takes what was attached to it.
    let (st, _) = call(&base, "a", "DELETE", &format!("/api/boards/{id}/entries/{fix}"), None).await;
    assert_eq!(st, 200);
    let left: u64 = app
        .db
        .c()
        .query_row("SELECT count(*) FROM board_attachments WHERE row_id = ?1", params![fix], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

/// A row whose pointer a newer build wrote (a kind this one doesn't know,
/// after a rollback) reads as no pointer, and the board still reads; a note
/// edit or a status change leaves the stored pointer as it was.
#[tokio::test]
async fn a_pointer_from_a_newer_build_reads_as_none_and_is_kept() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let p = seed(&app, &hub, true);
    let (_, b) = call(&base, "a", "POST", &format!("/api/projects/{p}/boards"), Some(json!({ "title": "B" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "proof" })).await;
    let row = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    {
        // What a newer build's schema allows and this one's doesn't.
        // A test's database is Postgres only when this names one (`Db::test`).
        let pg = std::env::var("ARUGULA_TEST_POSTGRES_URL").is_ok_and(|u| !u.is_empty());
        let c = app.db.c();
        if pg {
            c.execute_batch("ALTER TABLE board_entries DROP CONSTRAINT board_entries_target_kind_check").unwrap();
        } else {
            c.execute_batch("PRAGMA ignore_check_constraints = ON").unwrap();
        }
        c.execute(
            "UPDATE board_entries SET target_kind = 'from_a_newer_build', target = 'x_1' WHERE id = ?1",
            params![row],
        )
        .unwrap();
        if !pg {
            c.execute_batch("PRAGMA ignore_check_constraints = OFF").unwrap();
        }
    }
    let (st, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!((st, &v["zones"][0]["rows"][0]["pointer"]), (200, &json!({ "kind": "none" })), "{v}");
    let rev = v["zones"][0]["rows"][0]["rev"].clone();
    let (st, _) = put(&base, "a", &id, json!({ "id": row, "rev": rev, "note": "proof, edited" })).await;
    assert_eq!(st, 200);
    let (st, _) = put(&base, "a", &id, json!({ "id": row, "status": "needs_input" })).await;
    assert_eq!(st, 200);
    let kept: (String, String) = app
        .db
        .c()
        .query_row("SELECT target_kind, target FROM board_entries WHERE id = ?1", params![row], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(kept, ("from_a_newer_build".to_owned(), "x_1".to_owned()));
}

/// With the flag off for an account (chat on doesn't turn it on), every
/// route answers 404 as if boards weren't there, a board that exists
/// included.
#[tokio::test]
async fn with_the_flag_off_there_are_no_boards() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let p = seed(&app, &hub, false);
    let id = app.db.create_board(&p, "Launch", "", Actor::person("a"), 1, 10).unwrap().unwrap().id;

    for (method, path, body) in [
        ("GET", format!("/api/projects/{p}/boards"), None),
        ("POST", format!("/api/projects/{p}/boards"), Some(json!({ "title": "More" }))),
        ("GET", format!("/api/boards/{id}"), None),
        ("PUT", format!("/api/boards/{id}/entries"), Some(json!({ "entry": "row", "note": "x" }))),
        ("POST", format!("/api/boards/{id}/entries/row_0123/move"), Some(json!({}))),
        ("DELETE", format!("/api/boards/{id}/entries/row_0123"), None),
        (
            "POST",
            format!("/api/boards/{id}/attachments"),
            Some(
                json!({ "row": "row_0123", "role": "proof", "pointer": { "kind": "link", "url": "https://x.example/1" } }),
            ),
        ),
        ("DELETE", format!("/api/boards/{id}/attachments/att_0123"), None),
    ] {
        assert_eq!(call(&base, "a", method, &path, body).await.0, 404, "{method} {path}");
    }
    assert_eq!(app.db.boards(&p).unwrap().len(), 1, "nothing was made");
}

// ---------------------------------------------------------------- agents

/// A call signed by `who`'s machine, as the hub's gate vouches for it.
async fn signed(base: &str, by: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req =
        reqwest::Client::new().request(method.parse().unwrap(), format!("{base}{path}")).header("x-test-daemon", by);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

async fn geek(base: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    signed(base, "a geek", method, path, body).await
}

/// A machine with `boards.write` adds rows, sets notes, attaches work and
/// proof, labelled as its agent's; it can't repoint or delete a row a person
/// added, move a row, or set a status. Without the grant it is refused,
/// saying how to give it.
#[tokio::test]
async fn an_agent_with_boards_write_works_the_rows() {
    use crate::{agents_tests::give, grants::AGENT_UNGRANTED};
    use arugula_control_wire::boards::{
        agent_board, agent_board_attachment, agent_board_attachments, agent_board_entries, agent_board_entry,
        agent_board_entry_move, agent_project_boards,
    };

    let (hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
    hub.flag_named("team:t", FLAG, true);

    // A person's board: two rows.
    let (_, b) =
        call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "Launch" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    put(&base, "a", &id, json!({ "entry": "row", "note": "the plan" })).await;
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "tests green" })).await;
    let (plan, green) = (
        v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned(),
        v["zones"][0]["rows"][1]["id"].as_str().unwrap().to_owned(),
    );

    // No grant: refused, with how to give it. Read or write alike.
    let (st, v) = geek(&base, "GET", &agent_board(&id), None).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    assert!(
        v["error"].as_str().unwrap().contains(&format!("arugula grants add geek-box boards.read --project {t}")),
        "{v}"
    );
    let add = json!({ "entry": "row", "note": "sneaky" });
    let (st, v) = geek(&base, "PUT", &agent_board_entries(&id), Some(add.clone())).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(shape(&v), lanes(&["the plan", "tests green"], &[], &[], &[]), "nothing was written");

    // boards.read alone reads and doesn't write.
    let (st, g) =
        call(&base, "a", "POST", "/api/grants", give("geek", "boards.read", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");
    let (st, v) = geek(&base, "GET", &agent_project_boards(&t), None).await;
    assert_eq!((st, &v["boards"][0]["id"]), (200, &json!(id)), "{v}");
    assert_eq!(geek(&base, "GET", &agent_board(&id), None).await.0, 200);
    let (st, v) = geek(&base, "PUT", &agent_board_entries(&id), Some(add.clone())).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    assert!(v["error"].as_str().unwrap().contains("boards.write"), "{v}");

    // boards.write: add a row (labelled), set its note.
    let (st, g) =
        call(&base, "a", "POST", "/api/grants", give("geek", "boards.write", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");
    let (st, v) =
        geek(&base, "PUT", &agent_board_entries(&id), Some(json!({ "entry": "row", "note": "agent row" }))).await;
    assert_eq!(st, 200, "{v}");
    let mine = v["zones"][0]["rows"][2].clone();
    assert_eq!(mine["agent"], json!({ "machine": "geek", "machine_name": "geek-box" }), "{v}");
    assert_eq!(v["zones"][0]["rows"][0]["agent"], Value::Null, "a person's row has none: {v}");
    let mine_id = mine["id"].as_str().unwrap().to_owned();
    let (st, v) = geek(
        &base,
        "PUT",
        &agent_board_entries(&id),
        Some(json!({ "id": plan, "rev": 1, "note": "the plan, revised" })),
    )
    .await;
    assert_eq!(st, 200, "a note on a person's row: {v}");
    let (st, v) =
        geek(&base, "PUT", &agent_board_entries(&id), Some(json!({ "id": plan, "rev": 1, "note": "stale" }))).await;
    assert_eq!((st, v["reason"].as_str()), (409, Some(super::STALE_REV)), "{v}");

    // What lanes refuse an agent too: a status, a zone, a move.
    let follows = |what: &str, r: (u16, Value)| {
        assert_eq!(r.0, 400, "{what}: {}", r.1);
        assert!(r.1["error"].as_str().unwrap().starts_with("lanes follow the work: "), "{what}: {}", r.1);
    };
    follows(
        "a status",
        geek(&base, "PUT", &agent_board_entries(&id), Some(json!({ "id": green, "status": "done" }))).await,
    );
    follows("a zone", geek_put(&base, &id, json!({ "entry": "zone", "title": "Now" })).await);
    follows(
        "a move",
        geek(&base, "POST", &agent_board_entry_move(&id, &mine_id), Some(json!({ "zone": "lane_done" }))).await,
    );
    let (st, v) = geek_put(&base, &id, json!({ "id": green, "status": "needs_input" })).await;
    assert_eq!(st, 200, "needs_input stays: {v}");

    // Attach: proof on a person's row, and take it off again; a person's
    // attachment is a person's.
    let link = json!({ "kind": "link", "url": "https://ci.example.com/1" });
    let (st, v) = geek(
        &base,
        "POST",
        &agent_board_attachments(&id),
        Some(json!({ "row": green, "role": "proof", "pointer": link })),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    let theirs_att = row_in(&v, &green)["attachments"][0]["id"].as_str().unwrap().to_owned();
    let (st, v) = call(
        &base,
        "a",
        "POST",
        &format!("/api/boards/{id}/attachments"),
        Some(
            json!({ "row": green, "role": "proof", "pointer": { "kind": "link", "url": "https://ci.example.com/2" } }),
        ),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    let persons = row_in(&v, &green)["attachments"][1]["id"].as_str().unwrap().to_owned();
    let (st, v) = geek(&base, "DELETE", &agent_board_attachment(&id, &persons), None).await;
    assert_eq!(st, 403, "a person's attachment: {v}");
    let (st, v) = geek(&base, "DELETE", &agent_board_attachment(&id, &theirs_att), None).await;
    assert_eq!((st, row_in(&v, &green)["attachments"].as_array().unwrap().len()), (200, 1), "its own: {v}");

    // What it may never do to a person's row: repoint it or delete it.
    let no = |what: &str, r: (u16, Value)| assert_eq!(r.0, 403, "{what}: {}", r.1);
    no(
        "repoint a person's row",
        geek(
            &base,
            "PUT",
            &agent_board_entries(&id),
            Some(json!({ "id": plan, "rev": 2, "pointer": { "kind": "issue", "id": "iss_ab12" } })),
        )
        .await,
    );
    no("delete a person's row", geek(&base, "DELETE", &agent_board_entry(&id, &plan), None).await);

    // A row it added, it may delete; another machine's agents may not.
    hub.daemon("other", "a", Some("t"));
    let (st, _) =
        call(&base, "a", "POST", "/api/grants", give("other", "boards.write", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200);
    let (st, v) = signed(&base, "a other", "DELETE", &agent_board_entry(&id, &mine_id), None).await;
    assert_eq!(st, 403, "another machine's row: {v}");
    let (st, v) = geek(&base, "DELETE", &agent_board_entry(&id, &mine_id), None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(shape(&v), lanes(&["the plan, revised", "tests green"], &[], &[], &[]));

    // A person still deletes anything.
    let (st, v) = call(&base, "a", "DELETE", &format!("/api/boards/{id}/entries/{plan}"), None).await;
    assert_eq!(st, 200, "{v}");
}

async fn geek_put(base: &str, board: &str, body: Value) -> (u16, Value) {
    geek(base, "PUT", &arugula_control_wire::boards::agent_board_entries(board), Some(body)).await
}

/// Give `machine` `boards.write` over project `t`, as its owner `who`.
async fn grant_write(base: &str, who: &str, machine: &str, t: &str) {
    let g = call(
        base,
        who,
        "POST",
        "/api/grants",
        crate::agents_tests::give(machine, "boards.write", json!({ "kind": "project", "id": t })),
    )
    .await;
    assert_eq!(g.0, 200, "{}", g.1);
}

/// An agent makes a board with a goal and rows, and closes it with a note;
/// a read names the agent on both, and the person it acted for stays
/// `created_by` and `closed_by`. A board a person made says no agent.
#[tokio::test]
async fn an_agent_makes_and_runs_a_board() {
    use arugula_control_wire::boards::{agent_board, agent_board_close, agent_board_reopen, agent_project_boards};

    let (hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
    hub.flag_named("team:t", FLAG, true);
    grant_write(&base, "a", "geek", &t).await;
    let who = json!({ "machine": "geek", "machine_name": "geek-box" });

    let (st, b) =
        geek(&base, "POST", &agent_project_boards(&t), Some(json!({ "title": "Epic", "goal": "Ship it" }))).await;
    assert_eq!(st, 200, "{b}");
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    assert_eq!((&b["board"]["title"], &b["board"]["goal"]), (&json!("Epic"), &json!("Ship it")), "{b}");
    assert_eq!((&b["board"]["created_by"], &b["board"]["created_by_agent"]), (&json!("a"), &who), "{b}");
    assert_eq!(b["board"]["closed_by_agent"], Value::Null, "{b}");
    assert_eq!(shape(&b), lanes(&[], &[], &[], &[]), "four lanes, made by no one");

    let (st, v) = geek_put(&base, &id, json!({ "entry": "row", "note": "first ticket" })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["zones"][0]["rows"][0]["agent"], who, "{v}");

    let (st, v) =
        geek(&base, "PATCH", &agent_board(&id), Some(json!({ "title": "Epic 1", "goal": "Ship it today" }))).await;
    assert_eq!(
        (st, &v["board"]["title"], &v["board"]["goal"]),
        (200, &json!("Epic 1"), &json!("Ship it today")),
        "{v}"
    );
    let (st, v) = geek(&base, "POST", &agent_board_close(&id), Some(json!({ "done_note": "" }))).await;
    assert_eq!(st, 400, "a board closes with a note: {v}");
    let (st, v) = geek(&base, "POST", &agent_board_close(&id), Some(json!({ "done_note": "You can log in." }))).await;
    assert_eq!(st, 200, "{v}");

    // A person reads it all, labelled, in the list too.
    let (_, v) = call(&base, "x", "GET", &format!("/api/boards/{id}"), None).await;
    let board = &v["board"];
    assert_eq!(board["done_note"], json!("You can log in."), "{v}");
    assert_eq!((&board["created_by"], &board["created_by_agent"]), (&json!("a"), &who), "{v}");
    assert_eq!((&board["closed_by"], &board["closed_by_agent"]), (&json!("a"), &who), "{v}");
    let (_, list) = call(&base, "x", "GET", &format!("/api/projects/{t}/boards"), None).await;
    assert_eq!(list["boards"][0]["closed_by_agent"], who, "{list}");

    // Opening it again takes the closer away; a person's close is a person's.
    let (st, v) = geek(&base, "POST", &agent_board_reopen(&id), None).await;
    assert_eq!(
        (st, &v["board"]["closed_at"], &v["board"]["closed_by_agent"]),
        (200, &Value::Null, &Value::Null),
        "{v}"
    );
    assert_eq!(v["board"]["done_note"], json!("You can log in."), "{v}");
    let (st, v) =
        call(&base, "a", "POST", &format!("/api/boards/{id}/close"), Some(json!({ "done_note": "Done." }))).await;
    assert_eq!((st, &v["board"]["closed_by"], &v["board"]["closed_by_agent"]), (200, &json!("a"), &Value::Null), "{v}");

    // A board a person makes has no agent on it.
    let (_, b) = call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "Mine" }))).await;
    assert!(b["board"].get("created_by_agent").is_none(), "{b}");
}

async fn xbox(base: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    signed(base, "x xbox", method, path, body).await
}

/// The person's role is the ceiling: a viewer's machine holding
/// `boards.write` still makes no board and changes none.
#[tokio::test]
async fn a_viewers_machine_with_boards_write_makes_no_board() {
    use arugula_control_wire::boards::{
        agent_board, agent_board_attachments, agent_board_close, agent_board_entries, agent_project_boards,
    };

    let (hub, app, base, t, _u) = crate::agents_tests::seeded().await;
    hub.flag_named("team:t", FLAG, true);
    hub.daemon("xbox", "x", Some("t"));
    grant_write(&base, "x", "xbox", &t).await;
    let id = app.db.create_board(&t, "Launch", "", Actor::person("a"), 1, 10).unwrap().unwrap().id;

    let (st, v) = xbox(&base, "POST", &agent_project_boards(&t), Some(json!({ "title": "Mine" }))).await;
    assert_eq!(st, 403, "make a board: {v}");
    assert_eq!(app.db.boards(&t).unwrap().len(), 1, "nothing was made");
    let (st, v) = xbox(&base, "PATCH", &agent_board(&id), Some(json!({ "title": "Mine" }))).await;
    assert_eq!(st, 403, "edit: {v}");
    let (st, v) = xbox(&base, "POST", &agent_board_close(&id), Some(json!({ "done_note": "Done." }))).await;
    assert_eq!(st, 403, "close: {v}");
    let (st, v) = xbox(&base, "PUT", &agent_board_entries(&id), Some(json!({ "entry": "row", "note": "x" }))).await;
    assert_eq!(st, 403, "add a row: {v}");
    let link =
        json!({ "row": "row_0123", "role": "proof", "pointer": { "kind": "link", "url": "https://x.example/1" } });
    let (st, v) = xbox(&base, "POST", &agent_board_attachments(&id), Some(link)).await;
    assert_eq!(st, 403, "attach: {v}");
    assert_eq!(xbox(&base, "GET", &agent_board(&id), None).await.0, 200, "it reads");
}

/// An agent repoints a row its own machine added, and not one another hand
/// added.
#[tokio::test]
async fn an_agent_repoints_only_its_own_rows() {
    use arugula_control_wire::boards::agent_board_entries;

    let (hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
    hub.flag_named("team:t", FLAG, true);
    grant_write(&base, "a", "geek", &t).await;
    let (_, b) =
        call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "Launch" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "theirs" })).await;
    let theirs = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let (_, v) = geek(&base, "PUT", &agent_board_entries(&id), Some(json!({ "entry": "row", "note": "mine" }))).await;
    let mine = v["zones"][0]["rows"][1]["id"].as_str().unwrap().to_owned();

    let pointer = json!({ "kind": "issue", "id": "iss_ab12" });
    let (st, v) =
        geek(&base, "PUT", &agent_board_entries(&id), Some(json!({ "id": theirs, "rev": 1, "pointer": pointer })))
            .await;
    assert_eq!(st, 403, "{v}");
    let (st, v) =
        geek(&base, "PUT", &agent_board_entries(&id), Some(json!({ "id": mine, "rev": 1, "pointer": pointer }))).await;
    assert_eq!((st, &row_in(&v, &mine)["pointer"]), (200, &pointer), "{v}");
    assert_eq!(row_in(&v, &theirs)["pointer"], json!({ "kind": "none" }), "{v}");
}

/// With the `boards` flag off for the owner, an agent's routes are 404
/// whatever it was granted.
#[tokio::test]
async fn an_agent_finds_no_boards_with_the_flag_off() {
    use crate::agents_tests::give;
    use arugula_control_wire::boards::agent_project_boards;

    let (_hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
    let (st, _) =
        call(&base, "a", "POST", "/api/grants", give("geek", "boards.read", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200);
    let (st, v) = geek(&base, "GET", &agent_project_boards(&t), None).await;
    assert_eq!(st, 404, "{v}");
    let (st, v) = geek(&base, "GET", arugula_control_wire::boards::AGENT_BOARD_REACH, None).await;
    assert_eq!(st, 404, "{v}");
}

/// A machine asks which projects its board grants reach: none is an empty
/// list (not an error), and a grant names the project with its powers and
/// its boards.
#[tokio::test]
async fn an_agent_asks_which_boards_its_grants_reach() {
    use crate::agents_tests::give;
    use arugula_control_wire::boards::AGENT_BOARD_REACH;

    let (hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
    hub.flag_named("team:t", FLAG, true);
    let (st, v) = geek(&base, "GET", AGENT_BOARD_REACH, None).await;
    assert_eq!((st, &v["projects"]), (200, &json!([])), "{v}");

    call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "Launch" }))).await;
    let (st, g) =
        call(&base, "a", "POST", "/api/grants", give("geek", "boards.write", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");
    let (st, v) = geek(&base, "GET", AGENT_BOARD_REACH, None).await;
    assert_eq!(st, 200, "{v}");
    let p = &v["projects"][0];
    assert_eq!((&p["id"], &p["powers"], &p["boards"]), (&json!(t), &json!(["boards.write"]), &json!(1)), "{v}");
    assert!(p["name"].as_str().is_some_and(|n| !n.is_empty()), "{v}");
}

/// A machine with `boards.write` reports its block's state and a board row
/// that has the block attached shows it, and its lane follows: Doing while
/// the block works, Next once it is closed. Without the grant the report is
/// refused, and a machine reports only its own blocks: the body names no
/// machine.
#[tokio::test]
async fn a_machine_reports_its_blocks_and_the_row_shows_it() {
    use crate::{agents_tests::give, grants::AGENT_UNGRANTED};
    use arugula_control_wire::boards::AGENT_BLOCK_STATE;

    let (hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
    hub.flag_named("team:t", FLAG, true);

    // A board with a row on an issue, geek's pane 7 attached to it.
    let (_, b) =
        call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "Launch" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let parser = issue(&base, "a", &t, "Parser").await;
    let (_, v) =
        put(&base, "a", &id, json!({ "entry": "row", "note": "parser", "pointer": { "kind": "issue", "id": parser } }))
            .await;
    let row_id = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let pointer = json!({ "kind": "block", "machine": "geek", "pane": "7" });
    let (st, v) = attach(&base, "a", &id, json!({ "row": row_id, "role": "work", "pointer": pointer })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(lane(&v, &row_id), "next", "nothing reported yet");

    let report = json!({ "pane": "7", "block": {
        "state": "needs_input", "branch": "feat/parser", "head": "abc1234", "pushed": false,
        "uncommitted": 2, "diffstat": { "files": 3, "added": 40, "removed": 2 } } });
    let (st, v) = geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report.clone())).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "no grant: {v}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(row_in(&v, &row_id)["attachments"][0].get("block"), None, "nothing was kept: {v}");

    let (st, granted) =
        call(&base, "a", "POST", "/api/grants", give("geek", "boards.write", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{granted}");
    let (st, v) = geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report)).await;
    assert_eq!(st, 200, "{v}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(
        row_in(&v, &row_id)["attachments"][0]["block"],
        json!({ "state": "needs_input", "branch": "feat/parser", "head": "abc1234", "pushed": false,
                "uncommitted": 2, "diffstat": { "files": 3, "added": 40, "removed": 2 } }),
        "the row shows the state: {v}"
    );
    assert_eq!(lane(&v, &row_id), "doing", "a block that isn't closed: {v}");

    // Another machine with the grant sets its own pane 7, not geek's; a body
    // with a field the ruling doesn't list is refused.
    hub.daemon("other", "a", Some("t"));
    let (st, _) =
        call(&base, "a", "POST", "/api/grants", give("other", "boards.write", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200);
    let idle = json!({ "pane": "7", "block": { "state": "idle" } });
    let (st, v) = signed(&base, "a other", "PUT", AGENT_BLOCK_STATE, Some(idle)).await;
    assert_eq!(st, 200, "{v}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(
        row_in(&v, &row_id)["attachments"][0]["block"]["state"],
        json!("needs_input"),
        "geek's block is geek's: {v}"
    );
    let spoof = json!({ "pane": "7", "machine": "geek", "block": { "state": "idle" } });
    let (st, _) = signed(&base, "a other", "PUT", AGENT_BLOCK_STATE, Some(spoof)).await;
    assert!(st == 400 || st == 422, "no machine in the body: {st}");
    let said = json!({ "pane": "7", "block": { "state": "idle", "message": "all done" } });
    let (st, _) = geek(&base, "PUT", AGENT_BLOCK_STATE, Some(said)).await;
    assert!(st == 400 || st == 422, "no message text: {st}");

    // Closed, the block no longer moves the row.
    let closed = json!({ "pane": "7", "block": { "state": "closed" } });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(closed)).await.0, 200);
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(lane(&v, &row_id), "next", "{v}");

    // Revoking the grant hides what it reported.
    let (st, _) =
        call(&base, "a", "POST", &format!("/api/grants/{}/revoke", granted["id"].as_str().unwrap()), None).await;
    assert_eq!(st, 200);
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(row_in(&v, &row_id)["attachments"][0].get("block"), None, "{v}");
}
