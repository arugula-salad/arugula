//! Boards through the platform's routes (boards, step 1; lanes, aru-125),
//! with the hub of chat's tests: a board made, rows added and changed, work
//! and proof attached, a stale write refused, what lanes refuse, and the
//! `boards` flag off answering 404.

use std::sync::Arc;

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{
    chat_tests::{TestHub, call, serve},
    db::{
        issues::Actor,
        sql::{Sql as _, params},
    },
};

/// Team t: a owns it, v views. Chat is on for it. c is in team u, which has
/// no flags. t's project id. (The platform caches flags, so a test sets them
/// before its first request.)
fn seed(app: &crate::Platform, hub: &TestHub) -> String {
    hub.team("t", &[("a", TeamRole::Owner), ("v", TeamRole::Viewer)]);
    hub.team("u", &[("c", TeamRole::Owner)]);
    hub.flag("team:t", true);
    hub.flag("team:u", true);
    app.db.team_project("t", "t", 1).unwrap()
}

pub(super) async fn put(base: &str, who: &str, board: &str, body: Value) -> (u16, Value) {
    call(base, who, "PUT", &format!("/api/boards/{board}/entries"), Some(body)).await
}

async fn mv(base: &str, who: &str, board: &str, entry: &str, body: Value) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/boards/{board}/entries/{entry}/move"), Some(body)).await
}

pub(super) async fn attach(base: &str, who: &str, board: &str, body: Value) -> (u16, Value) {
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
    let p = seed(&app, &hub);

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
    assert_eq!(
        v["board"]["progress"],
        json!({ "done": 0, "total": 1, "proven": 0, "review": 0, "doing": 0 }),
        "a note is never counted"
    );
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

/// Nobody sets a lane (a row in a lane's place, a move to one, a lane's
/// change or delete) or a status: each is refused in the words "lanes follow
/// the work" (`needs_input`, in "ask with a question"), for a person; the rows
/// stay where the work puts them.
#[tokio::test]
async fn lanes_follow_the_work_and_refuse_the_rest() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let p = seed(&app, &hub);
    let (_, b) = call(&base, "a", "POST", &format!("/api/projects/{p}/boards"), Some(json!({ "title": "B" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "the plan" })).await;
    let plan = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let follows = |r: &(u16, Value)| {
        assert_eq!(r.0, 400, "{}", r.1);
        assert!(r.1["error"].as_str().unwrap().starts_with("lanes follow the work: "), "{}", r.1);
    };
    // A lane is no group: not a row's place, not changed, not deleted.
    follows(&put(&base, "a", &id, json!({ "entry": "row", "zone": "lane_done", "note": "x" })).await);
    follows(&put(&base, "a", &id, json!({ "id": "lane_next", "rev": 0, "title": "Later" })).await);
    follows(&call(&base, "a", "DELETE", &format!("/api/boards/{id}/entries/lane_next"), None).await);
    // Moves to a lane, or of one.
    follows(&mv(&base, "a", &id, &plan, json!({ "zone": "lane_done" })).await);
    follows(&mv(&base, "a", &id, "lane_doing", json!({ "before": "lane_next" })).await);
    // A group has no kind.
    let (st, v) = put(&base, "a", &id, json!({ "entry": "zone", "title": "Next", "kind": "lane" })).await;
    assert_eq!(st, 400, "{v}");
    // A status (and none, which clears it); needs_input is an ask now.
    for s in ["open", "working", "done", "failed"] {
        follows(&put(&base, "a", &id, json!({ "id": plan, "status": s })).await);
    }
    let (st, v) = put(&base, "a", &id, json!({ "id": plan, "status": "needs_input" })).await;
    assert_eq!(st, 400, "{v}");
    assert!(v["error"].as_str().unwrap().starts_with("ask with a question: "), "{v}");
    let (st, v) = put(&base, "a", &id, json!({ "id": plan, "status": "none" })).await;
    assert_eq!((st, &row_in(&v, &plan)["status"]), (200, &json!("none")), "{v}");
    // A new row with a status is refused the same way.
    follows(&put(&base, "a", &id, json!({ "entry": "row", "note": "x", "status": "done" })).await);
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(shape(&v), lanes(&["the plan"], &[], &[], &[]), "nothing moved, nothing was made");
    assert_eq!(v["groups"], json!([]), "{v}");
}

/// The notes of every row in the lane `next`, in order: a board's rows as it
/// lists them, by group and then as it has them.
fn order(v: &Value) -> Vec<String> {
    v["zones"][0]["rows"].as_array().unwrap().iter().map(|r| r["note"].as_str().unwrap().to_owned()).collect()
}

/// A group's id by its title.
fn group(v: &Value, title: &str) -> String {
    let g = v["groups"].as_array().unwrap().iter().find(|g| g["title"] == title);
    g.unwrap_or_else(|| panic!("no group {title}: {v}"))["id"].as_str().unwrap().to_owned()
}

/// A board's groups are its author's to set: made, renamed, moved and
/// deleted, with rows put in them and moved among them. A group lists its
/// rows first, in the groups' order, and rows with none last; each group
/// counts its own work; and a row's lane is the same in any group.
#[tokio::test]
async fn groups_are_the_authors_to_set() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let p = seed(&app, &hub);
    let (_, b) = call(&base, "a", "POST", &format!("/api/projects/{p}/boards"), Some(json!({ "title": "B" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();

    // Made last, each; moved before another.
    put(&base, "a", &id, json!({ "entry": "zone", "title": "Build" })).await;
    let (st, v) = put(&base, "a", &id, json!({ "entry": "zone", "title": "Decide" })).await;
    assert_eq!(st, 200, "{v}");
    let titles = |v: &Value| -> Vec<String> {
        v["groups"].as_array().unwrap().iter().map(|g| g["title"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(titles(&v), ["Build", "Decide"]);
    assert_eq!(v["groups"][0]["rev"], 1);
    assert_eq!(v["zones"].as_array().unwrap().len(), 4, "the lanes are the same four: {v}");
    let (build, decide) = (group(&v, "Build"), group(&v, "Decide"));
    let (st, v) = mv(&base, "a", &id, &decide, json!({ "before": build })).await;
    assert_eq!((st, titles(&v)), (200, vec!["Decide".to_owned(), "Build".to_owned()]), "{v}");

    // Rows: in a group, in none, and a group that isn't there.
    let add = |zone: Option<&str>, note: &str| json!({ "entry": "row", "note": note, "zone": zone });
    let (st, v) = put(&base, "a", &id, add(Some("zon_0123"), "x")).await;
    assert_eq!(st, 404, "{v}");
    put(&base, "a", &id, add(Some(&build), "b1")).await;
    put(&base, "a", &id, add(Some(&decide), "d1")).await;
    put(&base, "a", &id, add(None, "loose")).await;
    let (_, v) = put(&base, "a", &id, add(Some(&build), "b2")).await;
    assert_eq!(order(&v), ["d1", "b1", "b2", "loose"], "by group, ungrouped last: {v}");
    let on = |v: &Value, note: &str| -> Value { rows_in(v).into_iter().find(|r| r["note"] == note).unwrap().clone() };
    assert_eq!((on(&v, "b1")["group"].clone(), on(&v, "loose").get("group").cloned()), (json!(build), None));
    let (b1, b2, d1, loose) = (
        on(&v, "b1")["id"].as_str().unwrap().to_owned(),
        on(&v, "b2")["id"].as_str().unwrap().to_owned(),
        on(&v, "d1")["id"].as_str().unwrap().to_owned(),
        on(&v, "loose")["id"].as_str().unwrap().to_owned(),
    );

    // Moves: into a group (last in it), within one, before a row, out of one.
    let (st, v) = mv(&base, "a", &id, &loose, json!({ "zone": decide })).await;
    assert_eq!((st, order(&v)), (200, vec!["d1".into(), "loose".into(), "b1".into(), "b2".into()]), "{v}");
    let (_, v) = mv(&base, "a", &id, &b2, json!({ "before": b1 })).await;
    assert_eq!(order(&v), ["d1", "loose", "b2", "b1"], "within its group: {v}");
    let (_, v) = mv(&base, "a", &id, &d1, json!({ "zone": null })).await;
    assert_eq!(order(&v), ["loose", "b2", "b1", "d1"], "out of its group: {v}");
    assert!(on(&v, "d1").get("group").is_none(), "{v}");
    let (st, v) = mv(&base, "a", &id, &b1, json!({ "zone": decide, "before": b2 })).await;
    assert_eq!(st, 400, "before is a row of the group it goes to: {v}");
    let (st, v) = mv(&base, "a", &id, &b1, json!({ "zone": "zon_0123" })).await;
    assert_eq!(st, 404, "{v}");
    let (st, v) = mv(&base, "a", &id, &b1, json!({ "rev": 99 })).await;
    assert_eq!((st, v["reason"].as_str()), (409, Some(super::STALE_REV)), "{v}");

    // A group's title takes its rev; a row's group is not changed by a put.
    let (st, v) = put(&base, "a", &id, json!({ "id": build, "rev": 1, "title": "Make" })).await;
    assert_eq!((st, v["groups"][1]["rev"].clone(), titles(&v)), (200, json!(2), vec!["Decide".into(), "Make".into()]));
    let (st, v) = put(&base, "a", &id, json!({ "id": build, "rev": 1, "title": "Stale" })).await;
    assert_eq!((st, v["reason"].as_str()), (409, Some(super::STALE_REV)), "{v}");
    let (st, v) = put(&base, "a", &id, json!({ "id": build, "rev": 2, "note": "x" })).await;
    assert_eq!(st, 400, "a group has a title: {v}");
    let (st, v) = put(&base, "a", &id, json!({ "id": b1, "rev": 1, "zone": decide })).await;
    assert_eq!(st, 400, "a row changes group by a move: {v}");

    // Each group counts its own work: a closed issue in Decide.
    let shut = issue(&base, "a", &p, "Shut").await;
    app.db
        .c()
        .execute(
            "UPDATE issues SET state = 'closed', close_reason = 'completed', closed_at = 5 WHERE id = ?1",
            params![shut],
        )
        .unwrap();
    let (_, v) = put(
        &base,
        "a",
        &id,
        json!({ "entry": "row", "zone": decide, "note": "shut", "pointer": { "kind": "issue", "id": shut } }),
    )
    .await;
    assert_eq!(
        v["groups"][0]["progress"],
        json!({ "done": 1, "total": 1, "proven": 0, "review": 0, "doing": 0 }),
        "{v}"
    );
    assert_eq!(
        v["groups"][1]["progress"],
        json!({ "done": 0, "total": 0, "proven": 0, "review": 0, "doing": 0 }),
        "{v}"
    );
    assert_eq!(lane(&v, on(&v, "shut")["id"].as_str().unwrap()), "done", "a lane is the work's, in any group");

    // Deleting a group takes its rows with it (loose and shut were in
    // Decide), and nobody else's.
    let (st, v) = call(&base, "a", "DELETE", &format!("/api/boards/{id}/entries/{decide}"), None).await;
    assert_eq!(
        (st, titles(&v), order(&v)),
        (200, vec!["Make".to_owned()], vec!["b2".to_owned(), "b1".to_owned(), "d1".to_owned()]),
        "{v}"
    );
    assert!(on(&v, "d1").get("group").is_none(), "the row taken out of its group before is not in a group: {v}");

    // A viewer reads groups and changes none.
    let (st, v) = call(&base, "v", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!((st, titles(&v)), (200, vec!["Make".to_owned()]));
    let (st, _) = put(&base, "v", &id, json!({ "entry": "zone", "title": "No" })).await;
    assert_eq!(st, 403);
}

/// An agent with `boards.write` plans: it makes, renames, moves and deletes
/// groups and puts rows in them, a person's too. It deletes a group only
/// when every row in it is its own machine's (aru-123), and it still can't
/// set a lane.
#[tokio::test]
async fn an_agent_plans_with_groups_under_aru_123() {
    use crate::agents_tests::give;
    use arugula_control_wire::boards::{agent_board_entries, agent_board_entry, agent_board_entry_move};

    let (_hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
    let (_, b) =
        call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "Launch" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let (st, g) =
        call(&base, "a", "POST", "/api/grants", give("geek", "boards.write", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");

    // A person's group with a person's row; the agent's own group and row.
    let (_, v) = put(&base, "a", &id, json!({ "entry": "zone", "title": "Theirs" })).await;
    let theirs = group(&v, "Theirs");
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "their row", "zone": theirs })).await;
    let their_row = rows_in(&v)[0]["id"].as_str().unwrap().to_owned();
    let (st, v) = geek_put(&base, &id, json!({ "entry": "zone", "title": "Mine" })).await;
    assert_eq!(st, 200, "an agent makes a group: {v}");
    let mine = group(&v, "Mine");
    let (st, v) = geek_put(&base, &id, json!({ "entry": "row", "note": "my row", "zone": mine })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(order(&v), ["their row", "my row"]);

    // Planning is allowed on anyone's: rename, move a group, move a row.
    let (st, v) = geek_put(&base, &id, json!({ "id": theirs, "rev": 1, "title": "Phase 1" })).await;
    assert_eq!((st, group(&v, "Phase 1")), (200, theirs.clone()), "{v}");
    let (st, v) = geek(&base, "POST", &agent_board_entry_move(&id, &mine), Some(json!({ "before": theirs }))).await;
    assert_eq!((st, order(&v)), (200, vec!["my row".to_owned(), "their row".to_owned()]), "{v}");
    let (st, v) = geek(&base, "POST", &agent_board_entry_move(&id, &their_row), Some(json!({ "zone": mine }))).await;
    assert_eq!((st, order(&v)), (200, vec!["my row".to_owned(), "their row".to_owned()]), "{v}");
    let (st, v) =
        geek(&base, "POST", &agent_board_entry_move(&id, &their_row), Some(json!({ "zone": "lane_done" }))).await;
    assert_eq!(st, 400, "{v}");
    assert!(v["error"].as_str().unwrap().starts_with("lanes follow the work: "), "{v}");

    // Deleting: a group with a person's row in it is refused (their row went
    // to Mine); Phase 1 holds none now and goes.
    let (st, v) = geek(&base, "DELETE", &agent_board_entry(&id, &mine), None).await;
    assert_eq!(st, 403, "someone else's row is in it: {v}");
    assert!(v["error"].as_str().unwrap().contains("every row in it was added by its own machine"), "{v}");
    let (st, v) = geek(&base, "DELETE", &agent_board_entry(&id, &theirs), None).await;
    assert_eq!((st, v["groups"].as_array().unwrap().len()), (200, 1), "{v}");
    let (_, v) = geek_put(&base, &id, json!({ "entry": "zone", "title": "Empty" })).await;
    let (st, v) = geek(&base, "DELETE", &agent_board_entry(&id, &group(&v, "Empty")), None).await;
    assert_eq!(st, 200, "{v}");
    let _ = agent_board_entries(&id);
    assert_eq!(order(&geek(&base, "GET", &arugula_control_wire::boards::agent_board(&id), None).await.1).len(), 2);
}

/// What is attached to a row: work moves its lane, proof is a mark; a run
/// shows its state to a reader of its project and nothing to one who isn't,
/// and a link is kept as it was said and refused if it isn't plain https.
#[tokio::test]
async fn work_and_proof_attach_to_a_row() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let p = seed(&app, &hub);
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
    let p = seed(&app, &hub);
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
    let (st, _) = put(&base, "a", &id, json!({ "id": row, "status": "none" })).await;
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

/// Boards left Labs (#191): a team that never touched its flags can make a
/// board and read it, and so can one whose old `boards` flag says off, which
/// nothing reads any more.
#[tokio::test]
async fn boards_are_there_without_any_labs_flag() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    hub.team("n", &[("a", TeamRole::Owner)]);
    hub.team("o", &[("b", TeamRole::Owner)]);
    hub.flag_named("team:o", "boards", false);
    for (team, who) in [("n", "a"), ("o", "b")] {
        let p = app.db.team_project(team, team, 1).unwrap();
        let (st, list) = call(&base, who, "GET", &format!("/api/projects/{p}/boards"), None).await;
        assert_eq!(st, 200, "{team}: {list}");
        let (st, made) =
            call(&base, who, "POST", &format!("/api/projects/{p}/boards"), Some(json!({ "title": "Launch" }))).await;
        assert_eq!(st, 200, "{team}: {made}");
        let id = made["board"]["id"].as_str().unwrap();
        let (st, read) = call(&base, who, "GET", &format!("/api/boards/{id}"), None).await;
        assert_eq!((st, &read["board"]["title"]), (200, &json!("Launch")), "{team}: {read}");
        let (st, list) = call(&base, who, "GET", &format!("/api/projects/{p}/boards"), None).await;
        assert_eq!((st, list["boards"].as_array().map(Vec::len)), (200, Some(1)), "{team}: {list}");
    }
}

// ---------------------------------------------------------------- agents

/// A call signed by `who`'s machine, as the hub's gate vouches for it.
pub(super) async fn signed(base: &str, by: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req =
        reqwest::Client::new().request(method.parse().unwrap(), format!("{base}{path}")).header("x-test-daemon", by);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

pub(super) async fn geek(base: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
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

    // What lanes refuse an agent too: a status, a lane, a move to one.
    let follows = |what: &str, r: (u16, Value)| {
        assert_eq!(r.0, 400, "{what}: {}", r.1);
        assert!(r.1["error"].as_str().unwrap().starts_with("lanes follow the work: "), "{what}: {}", r.1);
    };
    follows(
        "a status",
        geek(&base, "PUT", &agent_board_entries(&id), Some(json!({ "id": green, "status": "done" }))).await,
    );
    follows("a lane", geek_put(&base, &id, json!({ "entry": "row", "zone": "lane_done", "note": "x" })).await);
    follows(
        "a move",
        geek(&base, "POST", &agent_board_entry_move(&id, &mine_id), Some(json!({ "zone": "lane_done" }))).await,
    );
    let (st, v) = geek_put(&base, &id, json!({ "id": green, "status": "needs_input" })).await;
    assert_eq!(st, 400, "needs_input is an ask now: {v}");
    assert!(v["error"].as_str().unwrap().starts_with("ask with a question: "), "{v}");

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
pub(super) async fn grant_write(base: &str, who: &str, machine: &str, t: &str) {
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

    let (_hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
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

/// A session leads a board (#414): the one an agent makes it with, or the one
/// that takes it; the board names its machine. Another session takes it over,
/// and only the lead's machine or a person lets it go. A name is needed, and a
/// person's board takes none.
#[tokio::test]
async fn a_session_leads_a_board() {
    use arugula_control_wire::boards::{LeadBoard, agent_board_lead, agent_project_boards};

    let (hub, app, base, t, _u) = crate::agents_tests::seeded().await;
    hub.daemon("mini", "a", Some("t"));
    grant_write(&base, "a", "geek", &t).await;
    grant_write(&base, "a", "mini", &t).await;
    let boards = agent_project_boards(&t);
    let mk = |body: Value| geek(&base, "POST", &boards, Some(body));

    let (st, b) = mk(json!({ "title": "Epic", "lead_session": "  ux-improvements " })).await;
    assert_eq!(st, 200, "{b}");
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let lead = &b["board"]["lead"];
    assert_eq!(
        (&lead["machine"], &lead["machine_name"], &lead["session"]),
        (&json!("geek"), &json!("geek-box"), &json!("ux-improvements")),
        "{b}"
    );
    assert!(lead["at"].as_u64().unwrap() > 0, "{b}");

    // No name, a blank one, or a person's make: no lead.
    for body in [json!({ "title": "A" }), json!({ "title": "B", "lead_session": "  " })] {
        let (_, b) = mk(body).await;
        assert!(b["board"].get("lead").is_none(), "{b}");
    }
    let body = Some(json!({ "title": "Mine", "lead_session": "ux" }));
    let (_, b) = call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), body).await;
    assert!(b["board"].get("lead").is_none(), "{b}");

    // A person reads it, in the list too.
    let (_, v) = call(&base, "x", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(v["board"]["lead"]["session"], "ux-improvements", "{v}");
    let (_, v) = call(&base, "x", "GET", &format!("/api/projects/{t}/boards"), None).await;
    assert_eq!(v["boards"][0]["lead"]["machine_name"], "geek-box", "{v}");

    // Taking needs a name (and says so); another machine's session takes over.
    let (st, v) = signed(&base, "a mini", "POST", &agent_board_lead(&id), Some(json!({}))).await;
    assert_eq!(st, 400, "{v}");
    let (st, v) = signed(&base, "a mini", "POST", &agent_board_lead(&id), Some(json!({ "session": "Docs" }))).await;
    assert_eq!(
        (st, &v["board"]["lead"]["machine"], &v["board"]["lead"]["session"]),
        (200, &json!("mini"), &json!("Docs")),
        "{v}"
    );

    // Only the lead's machine lets go; geek can't.
    let release = json!({ "release": true });
    let (st, v) = geek(&base, "POST", &agent_board_lead(&id), Some(release.clone())).await;
    assert_eq!(st, 403, "{v}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(v["board"]["lead"]["session"], "Docs", "{v}");
    let (st, v) = signed(&base, "a mini", "POST", &agent_board_lead(&id), Some(release.clone())).await;
    assert_eq!(st, 200, "{v}");
    assert!(v["board"].get("lead").is_none(), "{v}");

    // A person lets go of any.
    let (_, v) = signed(&base, "a mini", "POST", &agent_board_lead(&id), Some(json!({ "session": "Docs" }))).await;
    assert_eq!(v["board"]["lead"]["session"], "Docs", "{v}");
    let by = Actor::person("a");
    let released = LeadBoard { session: None, release: true };
    let v = crate::boards::lead(&app, by, &id, &released).await.unwrap();
    assert!(v.board.lead.is_none());
    // And a person can't take one: it is a session's.
    let took = LeadBoard { session: Some("Docs".into()), release: false };
    assert!(crate::boards::lead(&app, by, &id, &took).await.is_err());

    // Without boards.write the machine leads nothing.
    hub.daemon("ro", "a", Some("t"));
    let (st, v) = signed(&base, "a ro", "POST", &agent_board_lead(&id), Some(json!({ "session": "S" }))).await;
    assert_eq!(st, 403, "{v}");
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
    hub.daemon("xbox", "x", Some("t"));
    // A viewer's boards.write is given, since it still reads (Arugula
    // #683), and says so; every write is refused.
    let (st, g) = call(
        &base,
        "x",
        "POST",
        "/api/grants",
        crate::agents_tests::give("xbox", "boards.write", json!({ "kind": "project", "id": t })),
    )
    .await;
    assert_eq!((st, &g["limited"]["label"]), (200, &json!("reads only")), "{g}");
    let id = app.db.create_board(&t, "Launch", "", Actor::person("a"), None, 1, 10).unwrap().unwrap().id;

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

    let (_hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
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

/// A machine asks which projects its board grants reach: none is an empty
/// list (not an error), and a grant names the project with its powers and
/// its boards.
#[tokio::test]
async fn an_agent_asks_which_boards_its_grants_reach() {
    use crate::agents_tests::give;
    use arugula_control_wire::boards::AGENT_BOARD_REACH;

    let (_hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
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
    // Exactly the report: a released daemon refuses a block state with any
    // other field, so control's own `since` stays off the wire.
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

/// An agent that was replaced: its closed block's attachment goes after the
/// open ones and is marked `earlier`, and only the open one moves the row to
/// Doing.
#[tokio::test]
async fn a_replaced_agent_is_marked_earlier_and_comes_last() {
    use crate::agents_tests::give;
    use arugula_control_wire::boards::AGENT_BLOCK_STATE;

    let (_hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
    let (_, b) =
        call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "Launch" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let (st, _) =
        call(&base, "a", "POST", "/api/grants", give("geek", "boards.write", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200);
    let parser = issue(&base, "a", &t, "Parser").await;
    let (_, v) =
        put(&base, "a", &id, json!({ "entry": "row", "note": "parser", "pointer": { "kind": "issue", "id": parser } }))
            .await;
    let row = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    // Pane 7 first, then its replacement, pane 8.
    for pane in ["7", "8"] {
        let pointer = json!({ "kind": "block", "machine": "geek", "pane": pane });
        let (st, v) = attach(&base, "a", &id, json!({ "row": row, "role": "work", "pointer": pointer })).await;
        assert_eq!(st, 200, "{v}");
    }
    let state = |pane: &str, state: &str| json!({ "pane": pane, "block": { "state": state } });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(state("7", "closed"))).await.0, 200);
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(state("8", "working"))).await.0, 200);

    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    let r = row_in(&v, &row);
    assert_eq!(lane(&v, &row), "doing", "the open block moves it: {v}");
    let panes: Vec<_> = r["attachments"].as_array().unwrap().iter().map(|a| a["pointer"]["pane"].clone()).collect();
    assert_eq!(panes, [json!("8"), json!("7")], "the open one first: {v}");
    let earlier = r["attachments"][1]["id"].clone();
    assert_eq!(r["marks"], json!([{ "kind": "earlier", "attachment": earlier }]), "{v}");

    // Both closed: Next, and both earlier.
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(state("8", "closed"))).await.0, 200);
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(lane(&v, &row), "next", "{v}");
    assert_eq!(row_in(&v, &row)["marks"].as_array().unwrap().len(), 2, "{v}");
}

/// A board of `project` with a row on `issue`, made by `who`: the board's
/// id and the row's.
async fn board_on(base: &str, who: &str, project: &str, title: &str, issue: &str) -> (String, String) {
    let (_, b) =
        call(base, who, "POST", &format!("/api/projects/{project}/boards"), Some(json!({ "title": title }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let pointer = json!({ "kind": "issue", "id": issue });
    let (st, v) = put(base, who, &id, json!({ "entry": "row", "note": title, "pointer": pointer })).await;
    assert_eq!(st, 200, "{v}");
    (id.clone(), rows_in(&v)[0]["id"].as_str().unwrap().to_owned())
}

/// A block started on an issue is attached to that issue's row on every
/// board of the project, as the machine's work, and to nothing else: the
/// route is the machine's own (its grant over the project, its own block),
/// and another project's rows are never touched (#372).
#[tokio::test]
async fn a_block_started_on_an_issue_is_attached_to_its_rows() {
    use crate::grants::AGENT_UNGRANTED;
    use arugula_control_wire::boards::{AGENT_BLOCK_STATE, agent_project_attach_block as route};

    let (hub, _app, base, t, u) = crate::agents_tests::seeded().await;
    hub.flag("team:u", true);

    // Two issues, #1 and #2; two boards with a row for #1, one with #2. Team
    // u has an issue #1 of its own, and a board on it.
    let (one, two) = (issue(&base, "a", &t, "Parser").await, issue(&base, "a", &t, "Lexer").await);
    let (launch, launch_row) = board_on(&base, "a", &t, "Launch", &one).await;
    let (plan, plan_row) = board_on(&base, "a", &t, "Plan", &one).await;
    let (_, _) = board_on(&base, "a", &t, "Lexing", &two).await;
    let theirs = issue(&base, "c", &u, "Theirs").await;
    let (other, other_row) = board_on(&base, "c", &u, "Other", &theirs).await;

    let body = |issue: &str| json!({ "issue": issue, "machine": "geek", "pane": "7" });
    // No grant: refused, and nothing attached.
    let (st, v) = geek(&base, "POST", &route(&t), Some(body("#1"))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{launch}"), None).await;
    assert_eq!(row_in(&v, &launch_row)["attachments"], json!([]), "{v}");

    grant_write(&base, "a", "geek", &t).await;
    let (st, v) = geek(&base, "POST", &route(&t), Some(body("#1"))).await;
    assert_eq!(st, 200, "{v}");
    let mut on: Vec<_> = v["on"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| (o["title"].as_str().unwrap(), o["row"].as_str().unwrap()))
        .collect();
    on.sort();
    assert_eq!(on, vec![("Launch", launch_row.as_str()), ("Plan", plan_row.as_str())], "{v}");
    for (board, row) in [(&launch, &launch_row), (&plan, &plan_row)] {
        let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await;
        let at = &row_in(&v, row)["attachments"];
        assert_eq!(at[0]["pointer"], json!({ "kind": "block", "machine": "geek", "pane": "7" }), "{v}");
        assert_eq!((at[0]["role"].as_str(), at.as_array().unwrap().len()), (Some("work"), 1), "{v}");
    }

    // Again: the rows are the same and nothing is attached twice. Its
    // report shows on the row with the block's title, and the row is Doing.
    let (st, again) = geek(&base, "POST", &route(&t), Some(body("#1"))).await;
    assert_eq!((st, again["on"].as_array().unwrap().len()), (200, 2), "{again}");
    let report = json!({ "pane": "7", "block": { "state": "working", "title": "Parser lane" } });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report)).await.0, 200);
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{launch}"), None).await;
    let at = &row_in(&v, &launch_row)["attachments"];
    assert_eq!((at.as_array().unwrap().len(), at[0]["block"]["title"].as_str()), (1, Some("Parser lane")), "{v}");
    assert_eq!(lane(&v, &launch_row), "doing", "{v}");

    // A title is one line.
    let two_lines = json!({ "pane": "7", "block": { "state": "working", "title": "a\nb" } });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(two_lines)).await.0, 400);

    // Nothing for an issue with no row, or a name that is none.
    let (st, none) = geek(&base, "POST", &route(&t), Some(body("#9"))).await;
    assert_eq!((st, &none["on"]), (200, &json!([])), "{none}");
    assert_eq!(geek(&base, "POST", &route(&t), Some(body("one"))).await.0, 400);

    // The block is the signer's own, and the project is the grant's.
    let theirs_pane = json!({ "issue": "#1", "machine": "someone-else", "pane": "7" });
    assert_eq!(geek(&base, "POST", &route(&t), Some(theirs_pane)).await.0, 403);
    let (st, _) = geek(&base, "POST", &route(&u), Some(body("#1"))).await;
    assert!(st == 403 || st == 404, "no grant over u: {st}");
    let (_, v) = call(&base, "c", "GET", &format!("/api/boards/{other}"), None).await;
    assert_eq!(row_in(&v, &other_row)["attachments"], json!([]), "u's rows are not touched: {v}");
}

/// A block attached as `%640` is pane 640: the report for pane 640 finds it
/// and the row moves to Doing, and the pointer is stored as `640`.
#[tokio::test]
async fn a_block_attached_as_percent_n_is_pane_n() {
    use crate::agents_tests::give;
    use arugula_control_wire::boards::AGENT_BLOCK_STATE;

    let (_hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
    let (_, b) =
        call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "Launch" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let parser = issue(&base, "a", &t, "Parser").await;
    let (_, v) =
        put(&base, "a", &id, json!({ "entry": "row", "note": "parser", "pointer": { "kind": "issue", "id": parser } }))
            .await;
    let row_id = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let (st, g) =
        call(&base, "a", "POST", "/api/grants", give("geek", "boards.write", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");
    let pointer = json!({ "kind": "block", "machine": "geek", "pane": "%640" });
    let (st, v) = attach(&base, "a", &id, json!({ "row": row_id, "role": "work", "pointer": pointer })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(row_in(&v, &row_id)["attachments"][0]["pointer"]["pane"], "640", "stored plain: {v}");
    let report = json!({ "pane": "640", "block": { "state": "working" } });
    let (st, v) = geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report)).await;
    assert_eq!(st, 200, "{v}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(lane(&v, &row_id), "doing", "the report for pane 640 is found: {v}");
}

// ---------------------------------------------------------------- claims (#404)

/// A second machine's block on a row another machine's block holds is refused
/// 409, naming the holder; the holder's own machine isn't a conflict; `take`
/// folds the old block under `earlier`; and the claim lapses when its block
/// closes. The board_write attach (an agent block as work) keeps the same
/// rule.
#[tokio::test]
async fn a_second_machines_block_on_a_claimed_row_is_refused_naming_the_holder() {
    use arugula_control_wire::boards::{
        AGENT_BLOCK_STATE, CLAIMED, agent_board_attachments, agent_project_attach_block as route,
        agent_project_claim_check as claim_check,
    };

    let (hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
    hub.daemon("bmac", "b", Some("t"));
    let one = issue(&base, "a", &t, "Parser").await;
    let (board, row) = board_on(&base, "a", &t, "Launch", &one).await;
    grant_write(&base, "a", "geek", &t).await;
    grant_write(&base, "b", "bmac", &t).await;

    let body = |machine: &str, pane: &str, take: bool| {
        let mut b = json!({ "issue": "#1", "machine": machine, "pane": pane });
        if take {
            b["take"] = json!(true);
        }
        b
    };
    let by_b =
        |base: String, path: String, b: Value| async move { signed(&base, "b bmac", "POST", &path, Some(b)).await };

    // geek's block takes the row, and reports.
    assert_eq!(geek(&base, "POST", &route(&t), Some(body("geek", "7", false))).await.0, 200);
    let report = json!({ "pane": "7", "block": { "state": "working", "title": "Parser lane" } });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report)).await.0, 200);

    // bmac's is refused, naming geek's block, its machine and when.
    let (st, v) = by_b(base.clone(), route(&t), body("bmac", "3", false)).await;
    assert_eq!((st, v["reason"].as_str()), (409, Some(CLAIMED)), "{v}");
    let said = v["error"].as_str().or(v["message"].as_str()).unwrap_or_default().to_owned();
    for want in ["#1", "Parser lane", "geek-box", "since ", "UTC"] {
        assert!(said.contains(want), "{want:?} in {said:?}: {v}");
    }
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(row_in(&v, &row)["attachments"].as_array().unwrap().len(), 1, "nothing was attached: {v}");

    // The preflight says the same, without attaching anything: bmac is told
    // who holds it, geek (the holder's machine) is not.
    let check = |m: &str| json!({ "issue": "#1", "machine": m });
    let (st, v) = by_b(base.clone(), claim_check(&t), check("bmac")).await;
    assert_eq!(st, 200, "{v}");
    let held = v["held"].as_str().unwrap_or_default();
    assert!(held.contains("Parser lane") && held.contains("geek-box"), "{v}");
    let (_, v) = geek(&base, "POST", &claim_check(&t), Some(check("geek"))).await;
    assert_eq!(v.get("held"), None, "its own machine: {v}");
    let (st, _) = geek(&base, "POST", &claim_check(&t), Some(check("bmac"))).await;
    assert_eq!(st, 403, "only for its own blocks");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(row_in(&v, &row)["attachments"].as_array().unwrap().len(), 1, "a check attaches nothing: {v}");

    // The holder's own machine, another pane: no conflict.
    assert_eq!(geek(&base, "POST", &route(&t), Some(body("geek", "9", false))).await.0, 200);

    // bmac takes it: the old blocks are earlier agents and come last.
    let (st, v) = by_b(base.clone(), route(&t), body("bmac", "3", true)).await;
    assert_eq!(st, 200, "{v}");
    let working_again = || json!({ "pane": "3", "block": { "state": "working" } });
    let working = working_again();
    assert_eq!(signed(&base, "b bmac", "PUT", AGENT_BLOCK_STATE, Some(working)).await.0, 200);
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await;
    let r = row_in(&v, &row);
    let at = r["attachments"].as_array().unwrap();
    let panes: Vec<_> =
        at.iter().map(|a| (a["pointer"]["machine"].as_str().unwrap(), a["released"].as_bool())).collect();
    assert_eq!(panes[0], ("bmac", None), "a holder isn't released, and says nothing: {v}");
    assert!(panes[1..].iter().all(|p| p.0 == "geek" && p.1 == Some(true)), "{v}");
    assert_eq!(r["marks"].as_array().unwrap().len(), 2, "one earlier each: {v}");
    assert_eq!(lane(&v, &row), "doing", "bmac's block is not closed: {v}");

    // A block taken over doesn't hold the row up: with bmac's closed, the
    // row is Next though geek's old block still reports working.
    let closed = json!({ "pane": "3", "block": { "state": "closed" } });
    assert_eq!(signed(&base, "b bmac", "PUT", AGENT_BLOCK_STATE, Some(closed)).await.0, 200);
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(lane(&v, &row), "next", "{v}");
    assert_eq!(signed(&base, "b bmac", "PUT", AGENT_BLOCK_STATE, Some(working_again())).await.0, 200);

    // An agent's board_write attach honours it too: geek is refused now.
    let att = |pane: &str, take: bool| {
        let mut b =
            json!({ "row": row, "role": "work", "pointer": { "kind": "block", "machine": "geek", "pane": pane } });
        if take {
            b["take"] = json!(true);
        }
        b
    };
    let (st, v) = geek(&base, "POST", &agent_board_attachments(&board), Some(att("11", false))).await;
    assert_eq!((st, v["reason"].as_str()), (409, Some(CLAIMED)), "{v}");
    let (st, v) = geek(&base, "POST", &agent_board_attachments(&board), Some(att("11", true))).await;
    assert_eq!(st, 200, "{v}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await;
    let bmacs =
        row_in(&v, &row)["attachments"].as_array().unwrap().iter().find(|a| a["pointer"]["pane"] == "3").cloned();
    assert_eq!(bmacs.unwrap()["released"], json!(true), "geek took the row from bmac: {v}");

    // bmac's block was released by that take; geek's holds. Closing it
    // releases the claim: bmac attaches again, no take.
    let closed = json!({ "pane": "11", "block": { "state": "closed" } });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(closed)).await.0, 200);
    let (st, v) = by_b(base.clone(), route(&t), body("bmac", "4", false)).await;
    assert_eq!(st, 200, "a closed block holds nothing: {v}");
}

/// A person attaches only a block on a machine they may reach, asked of the
/// hub every time and refused while the hub can't answer; an agent only its
/// own machine's. A block's state, and its machine's id, show to a reader
/// who may reach the machine, and to everyone when its own machine's agent
/// attached it; an attachment from before the check shows nothing to the
/// rest. A hub that fails on a read shows less and the board still reads.
#[tokio::test]
async fn a_block_shows_to_those_who_reach_its_machine_or_by_its_own_agent() {
    use arugula_control_wire::boards::{AGENT_BLOCK_STATE, AttachRole, Pointer, agent_board_attachments};

    let (hub, app, base, t, _u) = crate::agents_tests::seeded().await;
    hub.daemon("apad", "a", None);
    hub.daemon("xbox", "c", Some("u"));
    crate::cache::catch_up(&app).await.unwrap();
    grant_write(&base, "a", "apad", &t).await;
    grant_write(&base, "a", "geek", &t).await;
    let one = issue(&base, "a", &t, "Parser").await;
    let (board, row) = board_on(&base, "a", &t, "Launch", &one).await;
    let block = |machine: &str, pane: &str| json!({ "row": row, "role": "work", "pointer": { "kind": "block", "machine": machine, "pane": pane } });
    let read = |who: &'static str| {
        let (base, board) = (base.clone(), board.clone());
        async move { call(&base, who, "GET", &format!("/api/boards/{board}"), None).await }
    };
    // The attachment on `pane`: its machine as the reader is told it, and the title it reported.
    let seen = |v: &Value, pane: &str| -> (String, Option<String>) {
        let at = row_in(v, &row)["attachments"].as_array().unwrap();
        let a = at.iter().find(|a| a["pointer"]["pane"] == pane).unwrap_or_else(|| panic!("pane {pane}: {v}"));
        (a["pointer"]["machine"].as_str().unwrap().to_owned(), a["block"]["title"].as_str().map(str::to_owned))
    };
    let told = |m: &str, title: &str| (m.to_owned(), Some(title.to_owned()));
    let untold = || (String::new(), None);

    // A person: not another's machine nor one that isn't there, asked each
    // time; not while the hub can't say.
    let (st, v) = attach(&base, "b", &board, block("apad", "1")).await;
    assert_eq!(st, 403, "{v}");
    assert!(v["error"].as_str().unwrap().contains("a machine they can reach"), "{v}");
    assert_eq!(attach(&base, "a", &board, block("xbox", "1")).await.0, 403);
    assert_eq!(attach(&base, "a", &board, block("nobox", "1")).await.0, 403);
    let asked = hub.calls_of("may_reach");
    assert_eq!(attach(&base, "a", &board, block("xbox", "1")).await.0, 403);
    assert_eq!(hub.calls_of("may_reach"), asked + 1, "a check asks again");
    hub.reach_down(true);
    let (st, v) = attach(&base, "a", &board, block("apad", "1")).await;
    assert!(st >= 500, "the hub down: nothing attached: {st} {v}");
    hub.reach_down(false);
    let (_, v) = read("a").await;
    assert_eq!(row_in(&v, &row)["attachments"], json!([]), "{v}");

    // An agent: only its own machine's blocks.
    let (st, v) = geek(&base, "POST", &agent_board_attachments(&board), Some(block("apad", "1"))).await;
    assert_eq!(st, 403, "{v}");
    assert!(v["error"].as_str().unwrap().contains("its own machine"), "{v}");
    let (st, v) = signed(&base, "a apad", "POST", &agent_board_attachments(&board), Some(block("apad", "2"))).await;
    assert_eq!(st, 200, "{v}");

    // a attaches apad's pane 1 (theirs) and geek's pane 3 (the team's);
    // each machine reports.
    assert_eq!(attach(&base, "a", &board, block("apad", "1")).await.0, 200);
    assert_eq!(attach(&base, "b", &board, block("geek", "3")).await.0, 200, "b reaches the team's machine");
    let report = |pane: &str, title: &str| json!({ "pane": pane, "block": { "state": "working", "title": title } });
    assert_eq!(signed(&base, "a apad", "PUT", AGENT_BLOCK_STATE, Some(report("1", "by a"))).await.0, 200);
    assert_eq!(signed(&base, "a apad", "PUT", AGENT_BLOCK_STATE, Some(report("2", "by apad"))).await.0, 200);
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report("3", "on geek"))).await.0, 200);

    // a reaches both machines; b and x reach geek only, and are told apad's
    // block only where apad's own agent attached it.
    let (_, v) = read("a").await;
    assert_eq!(
        (seen(&v, "1"), seen(&v, "2"), seen(&v, "3")),
        (told("apad", "by a"), told("apad", "by apad"), told("geek", "on geek")),
        "{v}"
    );
    for who in ["b", "x"] {
        let (st, v) = read(who).await;
        assert_eq!(st, 200, "{v}");
        assert_eq!(
            (seen(&v, "1"), seen(&v, "2"), seen(&v, "3")),
            (untold(), told("apad", "by apad"), told("geek", "on geek")),
            "{who}: {v}"
        );
        assert_eq!(lane(&v, &row), "doing", "the lane is the same whoever reads: {v}");
    }

    // An attachment from before the check: b's of apad's pane 4, and geek's
    // agent's of apad's pane 5. Only a, who reaches apad, is told either.
    let pane = |p: &str| Pointer::Block { machine: "apad".to_owned(), pane: p.to_owned() };
    let now = arugula_e2e::now_ms();
    app.db.attach(&board, &row, AttachRole::Work, &pane("4"), None, now, 50).unwrap().unwrap();
    app.db.attach(&board, &row, AttachRole::Work, &pane("5"), Some("geek"), now, 50).unwrap().unwrap();
    assert_eq!(signed(&base, "a apad", "PUT", AGENT_BLOCK_STATE, Some(report("4", "old"))).await.0, 200);
    assert_eq!(signed(&base, "a apad", "PUT", AGENT_BLOCK_STATE, Some(report("5", "older"))).await.0, 200);
    let (_, v) = read("b").await;
    assert_eq!((seen(&v, "4"), seen(&v, "5")), (untold(), untold()), "{v}");
    let (_, v) = read("a").await;
    assert_eq!((seen(&v, "4"), seen(&v, "5")), (told("apad", "old"), told("apad", "older")), "{v}");

    // A share reaches what the read shows.
    hub.share("apad", "b", true);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(seen(&read("b").await.1, "1"), told("apad", "by a"), "shared with b now");
    hub.share("apad", "b", false);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(seen(&read("b").await.1, "1"), untold(), "and no longer");

    // While the hub can't say, the board still reads, showing only what its
    // own agents attached; then it shows again.
    hub.reach_down(true);
    app.cache.advance(crate::cache::CACHE_TICK);
    let (st, v) = read("a").await;
    assert_eq!(st, 200, "the hub down: {v}");
    assert_eq!(
        (seen(&v, "1"), seen(&v, "2"), seen(&v, "3")),
        (untold(), told("apad", "by apad"), untold()),
        "the hub down: {v}"
    );
    hub.reach_down(false);
    assert_eq!(seen(&read("a").await.1, "1"), told("apad", "by a"), "nothing kept of the failure");
}

/// Stalled: a Doing row whose open blocks have all been idle longer than the
/// project's `stall_minutes`, with no PR on it. It is a mark for every
/// reader, in Needs you only for the person who owns the block, and a reader
/// who can't see the block (its machine's grant lapsed) is told nothing of
/// it. The setting is the owners' and editors'.
#[tokio::test]
async fn a_doing_row_with_only_idle_blocks_is_stalled_past_n_minutes() {
    use arugula_control_wire::boards::{AGENT_BLOCK_STATE, PROJECT_BOARD_SETTINGS};

    let (_hub, app, base, t, _u) = crate::agents_tests::seeded().await;
    let one = issue(&base, "a", &t, "Parser").await;
    let (board, row) = board_on(&base, "a", &t, "Launch", &one).await;
    let (st, granted) = call(
        &base,
        "a",
        "POST",
        "/api/grants",
        crate::agents_tests::give("geek", "boards.write", json!({ "kind": "project", "id": t })),
    )
    .await;
    assert_eq!(st, 200, "{granted}");
    let pointer = json!({ "kind": "block", "machine": "geek", "pane": "7" });
    let (st, v) = attach(&base, "a", &board, json!({ "row": row, "role": "work", "pointer": pointer })).await;
    assert_eq!(st, 200, "{v}");
    let idle = json!({ "pane": "7", "block": { "state": "idle" } });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(idle.clone())).await.0, 200);

    let read = |who: &'static str| {
        let (base, board) = (base.clone(), board.clone());
        async move { call(&base, who, "GET", &format!("/api/boards/{board}"), None).await.1 }
    };
    let marks = |v: &Value| row_in(v, &row)["marks"].clone();
    let asked = |v: &Value| row_in(v, &row)["ask"]["question"].as_str().map(str::to_owned);

    // Just now: not stalled.
    let v = read("a").await;
    assert_eq!(marks(&v), Value::Null, "{v}");
    // The idle report is repeated: it is still idle since the first.
    let long_ago = arugula_e2e::now_ms() - 42 * 60_000;
    app.db.c().execute("UPDATE board_block_states SET state_since = ?1", params![long_ago]).unwrap();
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(idle.clone())).await.0, 200);
    let v = read("a").await;
    assert_eq!(marks(&v), json!([{ "kind": "stalled", "since": long_ago }]), "42 minutes idle, 30 allowed: {v}");
    assert_eq!(lane(&v, &row), "doing", "{v}");
    // The block's owner has it in Needs you.
    assert!(asked(&v).is_some_and(|q| q.contains("stalled 42m")), "{v}");
    // Another member sees the mark, not the ask.
    let v = read("b").await;
    assert_eq!(marks(&v), json!([{ "kind": "stalled", "since": long_ago }]), "{v}");
    assert_eq!(asked(&v), None, "not b's block: {v}");

    // A working block un-stalls it; so does a PR; the setting moves the line.
    let working = json!({ "pane": "7", "block": { "state": "working" } });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(working)).await.0, 200);
    assert_eq!(marks(&read("a").await), Value::Null);
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(idle.clone())).await.0, 200);
    app.db.c().execute("UPDATE board_block_states SET state_since = ?1", params![long_ago]).unwrap();
    assert_eq!(marks(&read("a").await).as_array().map(Vec::len), Some(1));
    let path = PROJECT_BOARD_SETTINGS.replace("{project}", &t);
    let (st, v) = call(&base, "a", "GET", &path, None).await;
    assert_eq!((st, v["stall_minutes"].as_u64()), (200, Some(30)), "{v}");
    let (st, v) = call(&base, "b", "PUT", &path, Some(json!({ "stall_minutes": 60 }))).await;
    assert_eq!((st, v["stall_minutes"].as_u64()), (200, Some(60)), "an editor sets it: {v}");
    assert_eq!(marks(&read("a").await), Value::Null, "42m is under 60");
    assert_eq!(call(&base, "x", "PUT", &path, Some(json!({ "stall_minutes": 5 }))).await.0, 403, "a viewer may not");
    assert_eq!(call(&base, "a", "PUT", &path, Some(json!({ "stall_minutes": 0 }))).await.0, 400);
    assert_eq!(call(&base, "x", "GET", &path, None).await.1["stall_minutes"], 60, "a member reads it");
    assert_eq!(call(&base, "a", "PUT", &path, Some(json!({ "stall_minutes": 30 }))).await.0, 200);

    // A reader who can't see the block is told nothing: its machine's grant
    // lapsed, so there is no block, no claim and no stall.
    let grant = granted["id"].as_str().unwrap();
    let (st, _) = call(&base, "a", "POST", &format!("/api/grants/{grant}/revoke"), None).await;
    assert_eq!(st, 200);
    let v = read("b").await;
    assert_eq!((marks(&v), asked(&v)), (Value::Null, None), "{v}");
    assert_eq!(lane(&v, &row), "next", "{v}");
}

/// A report from a daemon that sends again (it says its age) is unknown to
/// lanes and claims once it is older than `BLOCK_STATE_STALE_MS`: the row
/// leaves Doing and another machine's block may take it, while the chip
/// still shows what was last said. A report older than the one kept
/// doesn't replace it (Arugula #745).
#[tokio::test]
async fn a_stale_report_is_unknown_to_lanes_and_claims() {
    use arugula_control_wire::boards::{AGENT_BLOCK_STATE, BLOCK_STATE_STALE_MS, agent_project_attach_block as route};

    let (hub, app, base, t, _u) = crate::agents_tests::seeded().await;
    hub.daemon("bmac", "b", Some("t"));
    let one = issue(&base, "a", &t, "Parser").await;
    let (board, row) = board_on(&base, "a", &t, "Launch", &one).await;
    grant_write(&base, "a", "geek", &t).await;
    grant_write(&base, "b", "bmac", &t).await;
    let body = |machine: &str, pane: &str| json!({ "issue": "#1", "machine": machine, "pane": pane });
    let read = || {
        let (base, board) = (base.clone(), board.clone());
        async move { call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await.1 }
    };

    assert_eq!(geek(&base, "POST", &route(&t), Some(body("geek", "7"))).await.0, 200);
    let working = json!({ "pane": "7", "block": { "state": "working" }, "age_ms": 0 });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(working)).await.0, 200);
    assert_eq!(lane(&read().await, &row), "doing");

    // A close seen a minute before the working that was kept: older, so the
    // working stands.
    let late = json!({ "pane": "7", "block": { "state": "closed" }, "age_ms": 60_000 });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(late)).await.0, 200);
    let v = read().await;
    assert_eq!(row_in(&v, &row)["attachments"][0]["block"]["state"], json!("working"), "{v}");
    let (st, v) = signed(&base, "b bmac", "POST", &route(&t), Some(body("bmac", "3"))).await;
    assert_eq!(st, 409, "geek's block holds it: {v}");

    // Nothing heard for longer than the stale line: unknown.
    let long_ago = arugula_e2e::now_ms() - BLOCK_STATE_STALE_MS - 60_000;
    app.db.c().execute("UPDATE board_block_states SET updated_at = ?1", params![long_ago]).unwrap();
    let v = read().await;
    assert_eq!(lane(&v, &row), "next", "{v}");
    assert_eq!(row_in(&v, &row)["attachments"][0]["block"]["state"], json!("working"), "still shown: {v}");
    let (st, v) = signed(&base, "b bmac", "POST", &route(&t), Some(body("bmac", "3"))).await;
    assert_eq!(st, 200, "a stale block holds nothing: {v}");

    // Heard from again: known again, and a report without the branch (a
    // daemon that restarted) keeps the one said before.
    let branch = json!({ "pane": "7", "block": { "state": "working", "branch": "feat/x" }, "age_ms": 0 });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(branch)).await.0, 200);
    let bare = json!({ "pane": "7", "block": { "state": "idle" }, "age_ms": 0 });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(bare)).await.0, 200);
    let v = read().await;
    assert_eq!(lane(&v, &row), "doing", "a fresh report brings it back: {v}");
    let geeks = |v: &Value| {
        row_in(v, &row)["attachments"].as_array().unwrap().iter().find(|a| a["pointer"]["machine"] == "geek").cloned()
    };
    assert_eq!(geeks(&v).unwrap()["block"]["branch"], json!("feat/x"), "{v}");

    // A close never goes stale: an old one still says closed, so the block
    // is an earlier agent of the row.
    let closed = json!({ "pane": "7", "block": { "state": "closed" }, "age_ms": 0 });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(closed)).await.0, 200);
    app.db
        .c()
        .execute("UPDATE board_block_states SET updated_at = ?1 WHERE machine = 'geek'", params![long_ago])
        .unwrap();
    let v = read().await;
    let earlier = geeks(&v).unwrap()["id"].clone();
    let marks = row_in(&v, &row)["marks"].clone();
    assert!(marks.as_array().unwrap().iter().any(|m| m["attachment"] == earlier), "{v}");
}

/// A person's lane outlasts a block that went stale: the row shows the lane
/// its other work derives while the block is unknown, and the person's lane
/// again once the block is heard from (Arugula #745).
#[tokio::test]
async fn a_persons_lane_outlasts_a_stale_block() {
    use arugula_control_wire::boards::{AGENT_BLOCK_STATE, BLOCK_STATE_STALE_MS, agent_project_attach_block as route};

    let (_hub, app, base, t, _u) = crate::agents_tests::seeded().await;
    let one = issue(&base, "a", &t, "Parser").await;
    let (board, row) = board_on(&base, "a", &t, "Launch", &one).await;
    grant_write(&base, "a", "geek", &t).await;
    let pick = json!({ "issue": "#1", "machine": "geek", "pane": "7" });
    assert_eq!(geek(&base, "POST", &route(&t), Some(pick)).await.0, 200);
    let working = json!({ "pane": "7", "block": { "state": "working" }, "age_ms": 0 });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(working.clone())).await.0, 200);
    let lane_path = format!("/api/boards/{board}/rows/{row}/lane");
    let (st, v) = call(&base, "a", "PUT", &lane_path, Some(json!({ "lane": "review" }))).await;
    assert_eq!((st, lane(&v, &row)), (200, "review".to_owned()), "{v}");

    let long_ago = arugula_e2e::now_ms() - BLOCK_STATE_STALE_MS - 60_000;
    app.db.c().execute("UPDATE board_block_states SET updated_at = ?1", params![long_ago]).unwrap();
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(lane(&v, &row), "next", "the block is unknown for now: {v}");

    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(working)).await.0, 200);
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(lane(&v, &row), "review", "the person's lane is back: {v}");
    assert_eq!(row_in(&v, &row)["lane_set"]["lane"], json!("review"), "{v}");
}

/// A daemon from before Arugula #745 says no age: its report is as of now
/// and never goes stale, as before. A machine that left control holds
/// nothing, whatever it last said.
#[tokio::test]
async fn an_old_daemons_report_holds_until_its_machine_leaves() {
    use arugula_control_wire::boards::{AGENT_BLOCK_STATE, agent_project_attach_block as route};

    let (hub, app, base, t, _u) = crate::agents_tests::seeded().await;
    hub.daemon("bmac", "b", Some("t"));
    let one = issue(&base, "a", &t, "Parser").await;
    let (board, row) = board_on(&base, "a", &t, "Launch", &one).await;
    grant_write(&base, "a", "geek", &t).await;
    grant_write(&base, "b", "bmac", &t).await;
    let body = |machine: &str, pane: &str| json!({ "issue": "#1", "machine": machine, "pane": pane });

    assert_eq!(geek(&base, "POST", &route(&t), Some(body("geek", "7"))).await.0, 200);
    let working = json!({ "pane": "7", "block": { "state": "working" } });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(working)).await.0, 200);
    let day_ago = arugula_e2e::now_ms() - 86_400_000;
    app.db.c().execute("UPDATE board_block_states SET updated_at = ?1", params![day_ago]).unwrap();
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(lane(&v, &row), "doing", "{v}");
    let (st, v) = signed(&base, "b bmac", "POST", &route(&t), Some(body("bmac", "3"))).await;
    assert_eq!(st, 409, "an old daemon's block still holds it: {v}");

    // Attached just now, so a block with no report would hold the row too:
    // only its machine having left lets it go.
    crate::cache::catch_up(&app).await.unwrap();
    hub.leave("geek");
    crate::cache::catch_up(&app).await.unwrap();
    let (st, v) = signed(&base, "b bmac", "POST", &route(&t), Some(body("bmac", "3"))).await;
    assert_eq!(st, 200, "geek left control: {v}");
}

/// A person's lane isn't cleared on a read that couldn't look up a block's
/// grant: that read can't tell the work's lane (Arugula #745).
#[tokio::test]
async fn a_grant_that_cant_be_read_keeps_a_persons_lane() {
    use arugula_control_wire::boards::{AGENT_BLOCK_STATE, agent_project_attach_block as route};

    let (_hub, app, base, t, _u) = crate::agents_tests::seeded().await;
    let one = issue(&base, "a", &t, "Parser").await;
    let (board, row) = board_on(&base, "a", &t, "Launch", &one).await;
    grant_write(&base, "a", "geek", &t).await;
    let pick = json!({ "issue": "#1", "machine": "geek", "pane": "7" });
    assert_eq!(geek(&base, "POST", &route(&t), Some(pick)).await.0, 200);
    let working = json!({ "pane": "7", "block": { "state": "working" }, "age_ms": 0 });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(working)).await.0, 200);
    let lane_path = format!("/api/boards/{board}/rows/{row}/lane");
    let (st, v) = call(&base, "a", "PUT", &lane_path, Some(json!({ "lane": "review" }))).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(lane(&v, &row), "review", "set by hand over Doing: {v}");

    app.db.c().execute("ALTER TABLE grants RENAME TO grants_away", params![]).unwrap();
    let (st, v) = call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(lane(&v, &row), "review", "the person's lane holds: {v}");
    let (st, _) = call(&base, "a", "PUT", &lane_path, Some(json!({ "lane": "next" }))).await;
    assert_eq!(st, 503, "no lane is set against a basis it can't read");
    app.db.c().execute("ALTER TABLE grants_away RENAME TO grants", params![]).unwrap();

    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(lane(&v, &row), "review", "and wasn't cleared: {v}");
    assert_eq!(row_in(&v, &row)["lane_set"]["lane"], json!("review"), "{v}");
}

/// A machine whose grant can't be read leaves the rows of other machines'
/// blocks alone: their lanes follow their work, and a person's lane there
/// is cleared once the work outgrows it (Arugula #745).
#[tokio::test]
async fn an_unsure_machine_leaves_other_rows_lanes_alone() {
    use arugula_control_wire::boards::{AGENT_BLOCK_STATE, agent_project_attach_block as route};

    let (hub, app, base, t, _u) = crate::agents_tests::seeded().await;
    if std::env::var_os("ARUGULA_TEST_POSTGRES_URL").is_some() {
        // The lookup is made to fail with a value SQLite keeps and Postgres
        // refuses.
        return eprintln!("skipped: SQLite only");
    }
    hub.daemon("bmac", "b", Some("t"));
    let one = issue(&base, "a", &t, "Parser").await;
    let two = issue(&base, "a", &t, "Lexer").await;
    let (board, row_one) = board_on(&base, "a", &t, "Launch", &one).await;
    let (_, v) =
        put(&base, "a", &board, json!({ "entry": "row", "note": "lexer", "pointer": { "kind": "issue", "id": two } }))
            .await;
    let row_two =
        rows_in(&v).iter().find(|r| r["pointer"]["id"] == two.as_str()).unwrap()["id"].as_str().unwrap().to_owned();
    grant_write(&base, "a", "geek", &t).await;
    grant_write(&base, "b", "bmac", &t).await;
    let body = |n: &str, machine: &str, pane: &str| json!({ "issue": n, "machine": machine, "pane": pane });
    assert_eq!(geek(&base, "POST", &route(&t), Some(body("#1", "geek", "7"))).await.0, 200);
    assert_eq!(signed(&base, "b bmac", "POST", &route(&t), Some(body("#2", "bmac", "3"))).await.0, 200);
    let working = |pane: &str| json!({ "pane": pane, "block": { "state": "working" }, "age_ms": 0 });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(working("7"))).await.0, 200);
    assert_eq!(signed(&base, "b bmac", "PUT", AGENT_BLOCK_STATE, Some(working("3"))).await.0, 200);
    for row in [&row_one, &row_two] {
        let path = format!("/api/boards/{board}/rows/{row}/lane");
        let (st, v) = call(&base, "a", "PUT", &path, Some(json!({ "lane": "review" }))).await;
        assert_eq!((st, lane(&v, row)), (200, "review".to_owned()), "{v}");
    }

    // bmac's grant can't be read, and geek's block closes.
    app.db.c().execute("UPDATE grants SET granted_by = x'ff' WHERE subject_id = 'bmac'", params![]).unwrap();
    let closed = json!({ "pane": "7", "block": { "state": "closed" }, "age_ms": 0 });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(closed)).await.0, 200);
    let (st, v) = call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(lane(&v, &row_two), "review", "bmac's row is left as it is: {v}");
    assert_eq!(lane(&v, &row_one), "next", "geek's row follows its work: {v}");
    assert_eq!(row_in(&v, &row_one)["lane_set"], Value::Null, "and its outgrown lane is cleared: {v}");

    app.db.c().execute("UPDATE grants SET granted_by = 'b' WHERE subject_id = 'bmac'", params![]).unwrap();
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(lane(&v, &row_two), "review", "{v}");
    assert_eq!(row_in(&v, &row_two)["lane_set"]["lane"], json!("review"), "{v}");
}

/// A report that says again what a fresh one said counts against the larger
/// limit alone, and pushes no change to readers; one that changes something
/// counts against the 2,400 too. A turn's empty diffstat clears the last
/// one, and a report that leaves the diffstat out keeps it (Arugula #745).
#[tokio::test]
async fn a_repeated_report_skips_the_change_limit_and_pushes_nothing() {
    use arugula_control_wire::boards::{AGENT_BLOCK_STATE, agent_project_attach_block as route};

    let (_hub, app, base, t, _u) = crate::agents_tests::seeded().await;
    let one = issue(&base, "a", &t, "Parser").await;
    let (board, row) = board_on(&base, "a", &t, "Launch", &one).await;
    grant_write(&base, "a", "geek", &t).await;
    let pick = json!({ "issue": "#1", "machine": "geek", "pane": "7" });
    assert_eq!(geek(&base, "POST", &route(&t), Some(pick)).await.0, 200);
    // The board's thread, so a change has somewhere to go.
    let thread = crate::testing::hub::room("ch_board", &format!("project:{t}"), "object", None, Some(&board));
    app.db.add_chat_room(&thread, &["a"], Some(&t)).unwrap();
    let report = |state: &str, diffstat: Value| {
        let mut b = json!({ "pane": "7", "block": { "state": state }, "age_ms": 0 });
        if !diffstat.is_null() {
            b["block"]["diffstat"] = diffstat;
        }
        b
    };
    let stat = json!({ "files": 3, "added": 40, "removed": 2 });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report("idle", stat.clone()))).await.0, 200);
    let pushed = app.chat.last();
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report("working", Value::Null))).await.0, 200);
    assert!(app.chat.last() > pushed, "a change is pushed");

    // An idle turn that left no diff clears the stat; said again, it is a
    // repeat.
    let zero = json!({ "files": 0, "added": 0, "removed": 0 });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report("idle", zero.clone()))).await.0, 200);
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await;
    let block = row_in(&v, &row)["attachments"][0]["block"].clone();
    assert_eq!((block["state"].as_str(), block.get("diffstat")), (Some("idle"), None), "{v}");
    let pushed = app.chat.last();
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report("idle", zero))).await.0, 200);
    assert_eq!(app.chat.last(), pushed, "an empty stat said again is a repeat");

    // The change limit used up: a repeat goes through, a change doesn't.
    for _ in 0..2_400 {
        let _ = app.limits.check_daemon(("board-block-state", 2_400), "geek");
    }
    let (st, v) = geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report("idle", Value::Null))).await;
    assert_eq!(st, 200, "a repeat isn't held to the change limit: {v}");
    assert_eq!(app.chat.last(), pushed, "and pushes nothing");
    let (st, _) = geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report("working", stat))).await;
    assert_eq!(st, 429, "a change is");
}

/// A stale block whose machine left control is released: the row reads it
/// as an earlier agent, and a person's lane it outgrew is cleared, even
/// before control has read the leave's grants (Arugula #745).
#[tokio::test]
async fn a_stale_block_from_a_machine_that_left_is_released() {
    use arugula_control_wire::boards::{AGENT_BLOCK_STATE, BLOCK_STATE_STALE_MS, agent_project_attach_block as route};

    let (hub, app, base, t, _u) = crate::agents_tests::seeded().await;
    let one = issue(&base, "a", &t, "Parser").await;
    let (board, row) = board_on(&base, "a", &t, "Launch", &one).await;
    grant_write(&base, "a", "geek", &t).await;
    let pick = json!({ "issue": "#1", "machine": "geek", "pane": "7" });
    assert_eq!(geek(&base, "POST", &route(&t), Some(pick)).await.0, 200);
    let working = json!({ "pane": "7", "block": { "state": "working" }, "age_ms": 0 });
    assert_eq!(geek(&base, "PUT", AGENT_BLOCK_STATE, Some(working)).await.0, 200);
    let lane_path = format!("/api/boards/{board}/rows/{row}/lane");
    assert_eq!(call(&base, "a", "PUT", &lane_path, Some(json!({ "lane": "review" }))).await.0, 200);
    let long_ago = arugula_e2e::now_ms() - BLOCK_STATE_STALE_MS - 60_000;
    app.db.c().execute("UPDATE board_block_states SET updated_at = ?1", params![long_ago]).unwrap();
    let read = || {
        let (base, board) = (base.clone(), board.clone());
        async move { call(&base, "a", "GET", &format!("/api/boards/{board}"), None).await.1 }
    };
    let kept =
        || app.db.board_stored(&board).unwrap().unwrap().rows.iter().find(|r| r.id == row).unwrap().lane_set.clone();
    let v = read().await;
    assert_eq!(lane(&v, &row), "next", "the block is unknown for now: {v}");
    assert_eq!(kept().map(|s| s.lane), Some(arugula_control_wire::boards::Lane::Review), "stale: kept");

    crate::cache::catch_up(&app).await.unwrap();
    hub.leave("geek");
    crate::cache::catch_up(&app).await.unwrap();
    let v = read().await;
    assert_eq!(lane(&v, &row), "next", "{v}");
    assert_eq!(kept(), None, "outgrown, and nothing in doubt: {v}");
    let earlier = row_in(&v, &row)["attachments"][0]["id"].clone();
    assert_eq!(row_in(&v, &row)["marks"], json!([{ "kind": "earlier", "attachment": earlier }]), "{v}");
}
