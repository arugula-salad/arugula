//! A person moves a row between lanes (#494), in `lanes_tests`' world: the
//! lane they set shows while the work still derives the lane it did, Done
//! closes a native issue and moving out of Done reopens it, a GitHub or
//! Plane item is closed there, and a viewer moves nothing.

use serde_json::{Value, json};

use super::lanes_tests::{add, attach, fi, native, on, read, start};
use crate::{
    chat_tests::call,
    db::{Sql as _, sql::params},
};

async fn put_lane(base: &str, who: &str, board: &str, row: &str, body: Value) -> (u16, Value) {
    call(base, who, "PUT", &format!("/api/boards/{board}/rows/{row}/lane"), Some(body)).await
}

/// The native issue `id` as the project reads it: its state.
async fn state_of(base: &str, id: &str) -> String {
    let (s, i) = call(base, "bob", "GET", &format!("/api/issues/{id}"), None).await;
    assert_eq!(s, 200, "{i}");
    i["state"].as_str().unwrap().to_owned()
}

/// A running run of the team's project.
fn running_run(app: &crate::Platform, project: &str) -> String {
    let op = "arugula op v1\ndaemon d1\nkind run\nsession -\npane - -\ncwd -\nargv ZWNobw==\n";
    let new = crate::db::runs::NewRun {
        project,
        workspace: None,
        trigger: crate::db::runs::TriggerKind::Manual,
        source: &json!({}),
        op,
        started_by: "bob",
        started_by_agent: None,
        grant: Some("g1"),
    };
    let run = app.db.create_run(&new, 1).unwrap().unwrap().id;
    app.db.c().execute("UPDATE runs SET state = 'running' WHERE id = ?1", params![run]).unwrap();
    run
}

fn progress(v: &Value) -> (u64, u64, u64) {
    let p = &v["board"]["progress"];
    (p["review"].as_u64().unwrap(), p["doing"].as_u64().unwrap(), p["done"].as_u64().unwrap())
}

/// Review set to Next shows Next while the work stays in Review; when the
/// work changes the derived lane the row follows it, and it doesn't snap
/// back to Next when the work returns to Review. The board's progress
/// counts the lanes the rows show.
#[tokio::test]
async fn a_lane_set_by_hand_holds_until_the_work_changes() {
    let (base, app, team, id) = start().await;
    let (login, _) = native(&base, &team, "Fix login").await;
    let v = add(&base, &id, json!({ "kind": "issue", "id": login })).await;
    let row = on(&v, &login).1["id"].as_str().unwrap().to_owned();
    let v = attach(&base, &id, &row, "work", fi("fi_a1")).await;
    assert_eq!((on(&v, &login).0, progress(&v)), ("lane_review", (1, 0, 0)), "an open PR: Review");

    let (st, v) = put_lane(&base, "bob", &id, &row, json!({ "lane": "next" })).await;
    assert_eq!(st, 200, "{v}");
    let (lane, r) = on(&v, &login);
    assert_eq!(lane, "lane_next", "{v}");
    assert_eq!(r["lane"], json!("next"));
    assert_eq!(r["lane_set"]["lane"], json!("next"), "{r}");
    assert_eq!(r["lane_set"]["by"], json!("bob"), "{r}");
    assert!(r["lane_set"]["at"].as_u64().unwrap() > 0, "{r}");
    assert_eq!(progress(&v), (0, 0, 0), "the bar agrees with the lanes");
    // It survives a reload, and the project's list counts it the same.
    let v = read(&base, "carol", &id).await;
    assert_eq!((on(&v, &login).0, progress(&v)), ("lane_next", (0, 0, 0)), "{v}");
    let (_, list) = call(&base, "bob", "GET", &format!("/api/projects/{team}/boards"), None).await;
    assert_eq!(list["boards"][0]["progress"]["review"], json!(0), "{list}");

    // The work changes: the PR is closed (the derived lane is Next, not the
    // Review it was set in), and an agent's run starts (Doing). The row
    // shows what the work says, with no mark.
    app.db.c().execute("UPDATE forge_items SET state = 'closed' WHERE id = 'fi_a1'", params![]).unwrap();
    let run = running_run(&app, &team);
    let v = attach(&base, &id, &row, "work", json!({ "kind": "run", "id": run })).await;
    let (lane, r) = on(&v, &login);
    assert_eq!(lane, "lane_doing", "{v}");
    assert!(r.get("lane_set").is_none(), "{r}");
    assert_eq!(progress(&v), (0, 1, 0), "the bar agrees: {v}");
    let (_, list) = call(&base, "bob", "GET", &format!("/api/projects/{team}/boards"), None).await;
    assert_eq!(list["boards"][0]["progress"]["doing"], json!(1), "{list}");

    // The run ends and the PR opens again: Review, as the work says, and not
    // the Next that was set before.
    app.db.c().execute("UPDATE runs SET state = 'succeeded' WHERE id = ?1", params![run]).unwrap();
    app.db.c().execute("UPDATE forge_items SET state = 'open' WHERE id = 'fi_a1'", params![]).unwrap();
    let v = read(&base, "bob", &id).await;
    let (lane, r) = on(&v, &login);
    assert_eq!(lane, "lane_review", "the old override doesn't come back: {v}");
    assert!(r.get("lane_set").is_none(), "{r}");
    assert_eq!(progress(&v), (1, 0, 0));
}

/// Moving a row to the lane its work derives takes the override off.
#[tokio::test]
async fn moving_a_row_to_its_derived_lane_clears_the_mark() {
    let (base, _app, team, id) = start().await;
    let (login, _) = native(&base, &team, "Fix login").await;
    let v = add(&base, &id, json!({ "kind": "issue", "id": login })).await;
    let row = on(&v, &login).1["id"].as_str().unwrap().to_owned();
    let (st, v) = put_lane(&base, "bob", &id, &row, json!({ "lane": "doing" })).await;
    assert_eq!((st, on(&v, &login).0), (200, "lane_doing"), "{v}");
    assert!(on(&v, &login).1.get("lane_set").is_some());
    let (st, v) = put_lane(&base, "bob", &id, &row, json!({ "lane": "next" })).await;
    assert_eq!((st, on(&v, &login).0), (200, "lane_next"), "{v}");
    assert!(on(&v, &login).1.get("lane_set").is_none(), "{v}");
}

/// Done closes a native issue (with the reason given) and sets no lane;
/// moving out of Done reopens it and sets the lane as for any move.
#[tokio::test]
async fn done_closes_a_native_issue_and_moving_out_reopens_it() {
    let (base, _app, team, id) = start().await;
    let (login, _) = native(&base, &team, "Fix login").await;
    let v = add(&base, &id, json!({ "kind": "issue", "id": login })).await;
    let row = on(&v, &login).1["id"].as_str().unwrap().to_owned();
    let v = attach(&base, &id, &row, "work", fi("fi_a1")).await;
    assert_eq!(on(&v, &login).0, "lane_review");

    let (st, v) = put_lane(&base, "bob", &id, &row, json!({ "lane": "done", "reason": "not_planned" })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(on(&v, &login).0, "lane_done", "{v}");
    assert!(on(&v, &login).1.get("lane_set").is_none(), "Done is never stored: {v}");
    assert_eq!(state_of(&base, &login).await, "closed");
    let (_, i) = call(&base, "bob", "GET", &format!("/api/issues/{login}"), None).await;
    assert_eq!(i["close_reason"], json!("not_planned"), "{i}");

    // Out of Done: reopened, and Next holds while the PR (open) derives Review.
    let (st, v) = put_lane(&base, "bob", &id, &row, json!({ "lane": "next" })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(state_of(&base, &login).await, "open");
    assert_eq!(on(&v, &login).0, "lane_next", "{v}");
    assert_eq!(on(&v, &login).1["lane_set"]["lane"], json!("next"), "{v}");

    // A reason goes with Done only.
    let (st, v) = put_lane(&base, "bob", &id, &row, json!({ "lane": "review", "reason": "completed" })).await;
    assert_eq!(st, 400, "{v}");
}

/// A GitHub or Plane item is closed there: Done is refused with the words
/// that say so, and sets no override; a note has no issue to close. A closed
/// one isn't reopened from here either.
#[tokio::test]
async fn done_on_a_github_item_or_a_note_is_refused() {
    let (base, _app, _team, id) = start().await;
    let v = add(&base, &id, fi("fi_a2")).await;
    let gh = on(&v, "fi_a2").1["id"].as_str().unwrap().to_owned();
    let (st, v) = put_lane(&base, "bob", &id, &gh, json!({ "lane": "done" })).await;
    assert_eq!(st, 422, "{v}");
    assert_eq!(v["error"], json!("close it in GitHub/Plane to move it to Done"), "{v}");
    let v = read(&base, "bob", &id).await;
    assert_eq!(on(&v, "fi_a2").0, "lane_next");
    assert!(on(&v, "fi_a2").1.get("lane_set").is_none(), "{v}");

    let (st, v) = call(
        &base,
        "bob",
        "PUT",
        &format!("/api/boards/{id}/entries"),
        Some(json!({ "entry": "row", "note": "just a note" })),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    let note = v["zones"][0]["rows"].as_array().unwrap().iter().find(|r| r["note"] == "just a note").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (st, v) = put_lane(&base, "bob", &id, &note, json!({ "lane": "done" })).await;
    assert_eq!(st, 422, "{v}");

    // fi_a3 is a merged PR: Done, and not ours to reopen.
    let v = add(&base, &id, fi("fi_a3")).await;
    let merged = on(&v, "fi_a3").1["id"].as_str().unwrap().to_owned();
    let (st, v) = put_lane(&base, "bob", &id, &merged, json!({ "lane": "review" })).await;
    assert_eq!(st, 422, "{v}");
    assert!(v["error"].as_str().unwrap().contains("reopen it in GitHub/Plane"), "{v}");
}

/// A viewer moves nothing, and a stranger finds no board.
#[tokio::test]
async fn a_viewer_moves_no_row() {
    let (base, _app, team, id) = start().await;
    let (login, _) = native(&base, &team, "Fix login").await;
    let v = add(&base, &id, json!({ "kind": "issue", "id": login })).await;
    let row = on(&v, &login).1["id"].as_str().unwrap().to_owned();
    for lane in ["review", "done"] {
        let (st, v) = put_lane(&base, "carol", &id, &row, json!({ "lane": lane })).await;
        assert_eq!(st, 403, "{lane}: {v}");
    }
    assert_eq!(state_of(&base, &login).await, "open", "nothing was closed");
    let v = read(&base, "bob", &id).await;
    assert_eq!(on(&v, &login).0, "lane_next");
    let (st, v) = put_lane(&base, "bob", &id, "row_0123", json!({ "lane": "review" })).await;
    assert_eq!(st, 404, "no such row: {v}");
}
