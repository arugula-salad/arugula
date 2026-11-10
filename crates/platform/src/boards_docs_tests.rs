//! A board's Docs (Arugula #517) through the platform's routes: the strip
//! (link, unlink, move, the limit), a Doc on a row that moves
//! no lane and proves nothing, a Doc of another project refused, a deleted
//! Doc read as missing, and what the Doc's page says of its boards.

use arugula_control_wire::{
    boards::{BOARD_DOCS_MAX, agent_board_doc, agent_board_docs},
    project_docs::machine_doc,
};
use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use super::tests::{attach, geek, grant_write, issue, lane, put, row_in, rows_in, signed};
use crate::{
    agents_tests::seeded,
    chat_tests::call,
    db::sql::{Sql as _, params},
};

/// A board of project `t`, made by a.
async fn board(base: &str, t: &str) -> String {
    let (st, b) =
        call(base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "Launch" }))).await;
    assert_eq!(st, 200, "{b}");
    b["board"]["id"].as_str().unwrap().to_owned()
}

/// A project Doc with `sections` sections, the first `written` of them
/// written: its `pdc_` id.
async fn doc(base: &str, who: &str, project: &str, title: &str, sections: usize, written: usize) -> String {
    let sections: Vec<Value> = (0..sections)
        .map(|i| {
            let mut s = json!({ "heading": format!("Part {i}"), "intent": "later" });
            if i < written {
                s["markdown"] = json!("text");
            }
            s
        })
        .collect();
    let (st, d) = call(
        base,
        who,
        "POST",
        &format!("/api/projects/{project}/docs"),
        Some(json!({ "title": title, "sections": sections })),
    )
    .await;
    assert_eq!(st, 200, "{d}");
    d["doc"]["id"].as_str().unwrap().to_owned()
}

async fn link(base: &str, who: &str, board: &str, pointer: Value) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/boards/{board}/docs"), Some(json!({ "pointer": pointer }))).await
}

/// Link a Doc to a row.
async fn link_row(base: &str, who: &str, board: &str, row: &str, pointer: Value) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/boards/{board}/docs"), Some(json!({ "pointer": pointer, "row": row }))).await
}

/// The titles of a row's Docs.
fn row_titles(v: &Value, row: &str) -> Vec<String> {
    let docs = row_in(v, row).get("docs").and_then(Value::as_array).cloned().unwrap_or_default();
    docs.iter().map(|d| d["title"].as_str().unwrap().to_owned()).collect()
}

fn rand_hex() -> String {
    hex::encode(arugula_e2e::random::<16>())
}

fn titles(v: &Value) -> Vec<&str> {
    v["docs"].as_array().unwrap().iter().map(|d| d["title"].as_str().unwrap()).collect()
}

/// Link two Docs, see them in order with their sections, link one again (it
/// stays once), move one, take one off. A viewer reads and links nothing.
#[tokio::test]
async fn link_move_and_unlink_a_docs_strip() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let id = board(&base, &t).await;
    let plan = doc(&base, "a", &t, "The plan", 5, 3).await;
    let spike = doc(&base, "a", &t, "The spike", 1, 1).await;

    let (st, v) = link(&base, "a", &id, json!({ "kind": "doc", "id": plan })).await;
    assert_eq!((st, titles(&v)), (200, vec!["The plan"]), "{v}");
    assert_eq!((&v["docs"][0]["sections"], &v["docs"][0]["written"]), (&json!(5), &json!(3)), "{v}");
    assert!(v["docs"][0]["id"].as_str().unwrap().starts_with("bdc_"), "{v}");
    assert_eq!(v["docs"][0]["added_by"], "a", "{v}");
    let (_, v) = link(&base, "b", &id, json!({ "kind": "doc", "id": spike })).await;
    assert_eq!(titles(&v), ["The plan", "The spike"], "an editor links: {v}");

    // Again: once, and it says so.
    let (st, v) = link(&base, "a", &id, json!({ "kind": "doc", "id": plan })).await;
    assert_eq!((st, titles(&v).len()), (200, 2), "{v}");
    assert!(v["said"].as_str().unwrap().contains("already"), "{v}");

    // Move the spike first, then the plan before nothing: last.
    let (spike_entry, plan_entry) = (v["docs"][1]["id"].as_str().unwrap(), v["docs"][0]["id"].as_str().unwrap());
    let mv = |doc: &str, before: Option<&str>| {
        let (path, body) = (format!("/api/boards/{id}/docs/{doc}/move"), json!({ "before": before }));
        let base = base.clone();
        async move { call(&base, "a", "POST", &path, Some(body)).await }
    };
    let (st, v) = mv(spike_entry, Some(plan_entry)).await;
    assert_eq!((st, titles(&v)), (200, vec!["The spike", "The plan"]), "{v}");
    let (_, v) = mv(spike_entry, None).await;
    assert_eq!(titles(&v), ["The plan", "The spike"], "{v}");
    assert_eq!(mv(spike_entry, Some("bdc_nothere")).await.0, 404);
    assert_eq!(mv(spike_entry, Some(spike_entry)).await.0, 400, "not before itself");

    // A viewer reads it and changes nothing.
    let (st, v) = call(&base, "x", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!((st, titles(&v).len()), (200, 2), "{v}");
    assert_eq!(link(&base, "x", &id, json!({ "kind": "doc", "id": plan })).await.0, 403);
    let (st, _) = call(&base, "x", "DELETE", &format!("/api/boards/{id}/docs/{plan_entry}"), None).await;
    assert_eq!(st, 403);
    assert_eq!(mv(plan_entry, None).await.0, 200);
    let (st, _) = call(&base, "x", "POST", &format!("/api/boards/{id}/docs/{plan_entry}/move"), Some(json!({}))).await;
    assert_eq!(st, 403);
    // Someone outside the project doesn't see the board at all.
    assert_eq!(link(&base, "c", &id, json!({ "kind": "doc", "id": plan })).await.0, 404);

    let (st, v) = call(&base, "a", "DELETE", &format!("/api/boards/{id}/docs/{plan_entry}"), None).await;
    assert_eq!((st, titles(&v)), (200, vec!["The spike"]), "{v}");
    let (st, _) = call(&base, "a", "DELETE", &format!("/api/boards/{id}/docs/{plan_entry}"), None).await;
    assert_eq!(st, 404, "gone already");
    // Linking again after the unlink works.
    let (_, v) = link(&base, "a", &id, json!({ "kind": "doc", "id": plan })).await;
    assert_eq!(titles(&v), ["The spike", "The plan"], "{v}");
}

/// A board lists at most BOARD_DOCS_MAX Docs.
#[tokio::test]
async fn a_strip_holds_at_most_twenty_docs() {
    let (_hub, app, base, t, _u) = seeded().await;
    let id = board(&base, &t).await;
    for i in 0..=BOARD_DOCS_MAX {
        let new = crate::db::project_docs::NewDoc {
            project: &t,
            title: &format!("Doc {i}"),
            made_by: "a",
            made_by_session: None,
            sections: &[],
        };
        let d = app.db.create_project_doc(&new, 1, 100).unwrap().unwrap().id;
        let (st, v) = link(&base, "a", &id, json!({ "kind": "doc", "id": d })).await;
        if i < BOARD_DOCS_MAX {
            assert_eq!(st, 200, "{i}: {v}");
        } else {
            assert_eq!(st, 400, "the 21st: {v}");
            assert!(v["error"].as_str().unwrap().contains("at most 20"), "{v}");
        }
    }
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(titles(&v).len() as u64, BOARD_DOCS_MAX, "{v}");
}

/// A Doc of another project is no Doc here: 404 in the strip and on a row,
/// and a made-up id too.
#[tokio::test]
async fn another_projects_doc_is_refused() {
    let (hub, app, base, t, u) = seeded().await;
    hub.flag("team:u", true);
    let id = board(&base, &t).await;
    let theirs = doc(&base, "c", &u, "Their plan", 1, 1).await;
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "the plan" })).await;
    let row = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();

    let (st, v) = link(&base, "a", &id, json!({ "kind": "doc", "id": theirs })).await;
    assert_eq!(st, 404, "{v}");
    let (st, v) = link_row(&base, "a", &id, &row, json!({ "kind": "doc", "id": theirs })).await;
    assert_eq!(st, 404, "{v}");
    assert_eq!(
        link(&base, "a", &id, json!({ "kind": "doc", "id": "pdc_00000000000000000000000000000000" })).await.0,
        404
    );
    assert_eq!(link(&base, "a", &id, json!({ "kind": "doc", "id": "nonsense" })).await.0, 404);
    // Nor does its page say which boards it is on.
    let (st, v) = call(&base, "a", "GET", &format!("/api/projects/{t}/docs/{theirs}/boards"), None).await;
    assert_eq!(st, 404, "{v}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert!(v["docs"].as_array().unwrap().is_empty(), "{v}");
    let n: u64 = app.db.c().query_row("SELECT count(*) FROM board_docs", params![], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

/// A Doc is never an attachment or a row's item; linked to a row it is that
/// row's, once, in its order, and an editor takes it off.
#[tokio::test]
async fn a_doc_is_linked_to_a_row_and_is_never_its_item_or_an_attachment() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let id = board(&base, &t).await;
    let d = doc(&base, "a", &t, "The spike", 2, 1).await;
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "the plan" })).await;
    let row = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let doc_ptr = json!({ "kind": "doc", "id": d });

    for role in ["work", "proof"] {
        let (st, v) = attach(&base, "a", &id, json!({ "row": row, "role": role, "pointer": doc_ptr })).await;
        assert_eq!(st, 400, "{role}: {v}");
    }
    let (st, v) = put(&base, "a", &id, json!({ "entry": "row", "pointer": doc_ptr })).await;
    assert_eq!(st, 400, "a Doc is not a row's item: {v}");

    let (st, v) = link_row(&base, "a", &id, &row, doc_ptr.clone()).await;
    assert_eq!(st, 200, "{v}");
    let doc0 = &row_in(&v, &row)["docs"][0];
    assert_eq!(
        (&doc0["title"], &doc0["written"], &doc0["sections"]),
        (&json!("The spike"), &json!(1), &json!(2)),
        "{v}"
    );
    assert!(row_in(&v, &row)["attachments"].as_array().is_none_or(Vec::is_empty), "no attachment: {v}");
    assert!(v["docs"].as_array().unwrap().is_empty(), "not in the strip: {v}");
    // Once on a row; the strip is a list of its own.
    let (_, v) = link_row(&base, "a", &id, &row, doc_ptr.clone()).await;
    assert_eq!(row_titles(&v, &row), ["The spike"], "{v}");
    assert!(v["said"].as_str().unwrap().contains("already"), "{v}");
    let (_, v) = link(&base, "a", &id, doc_ptr.clone()).await;
    assert_eq!((titles(&v), row_titles(&v, &row)), (vec!["The spike"], vec!["The spike".to_owned()]), "{v}");
    // A row that isn't there.
    assert_eq!(link_row(&base, "a", &id, "row_00000000000000000000000000000000", doc_ptr.clone()).await.0, 404);

    // A viewer links nothing; an editor takes it off, by the same route as the strip's.
    assert_eq!(link_row(&base, "x", &id, &row, doc_ptr.clone()).await.0, 403);
    let entry = row_in(&v, &row)["docs"][0]["id"].as_str().unwrap().to_owned();
    let (st, v) = call(&base, "b", "DELETE", &format!("/api/boards/{id}/docs/{entry}"), None).await;
    assert_eq!(st, 200, "{v}");
    assert!(row_titles(&v, &row).is_empty() && titles(&v) == ["The spike"], "the strip's stays: {v}");
}

/// A Doc is once in the strip and once on each row (the strip's NULL row
/// counts as one place); a row's Docs have their own order and limit, move
/// only among themselves, and go with the row.
#[tokio::test]
async fn row_docs_are_once_per_row_in_their_own_order_and_go_with_the_row() {
    let (_hub, app, base, t, _u) = seeded().await;
    let id = board(&base, &t).await;
    let (a, b, c) = (
        doc(&base, "a", &t, "A", 1, 1).await,
        doc(&base, "a", &t, "B", 1, 1).await,
        doc(&base, "a", &t, "C", 1, 1).await,
    );
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "one" })).await;
    let r1 = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "two" })).await;
    let r2 = v["zones"][0]["rows"][1]["id"].as_str().unwrap().to_owned();
    let ptr = |d: &str| json!({ "kind": "doc", "id": d });

    // The same Doc on two rows and in the strip, three places; twice in none.
    for _ in 0..2 {
        link(&base, "a", &id, ptr(&a)).await;
        link_row(&base, "a", &id, &r1, ptr(&a)).await;
        link_row(&base, "a", &id, &r2, ptr(&a)).await;
    }
    let (_, v) = link_row(&base, "a", &id, &r1, ptr(&b)).await;
    let (_, v2) = link_row(&base, "a", &id, &r1, ptr(&c)).await;
    assert_eq!(
        (titles(&v2), row_titles(&v2, &r1), row_titles(&v2, &r2)),
        (vec!["A"], vec!["A".into(), "B".into(), "C".into()], vec!["A".into()]),
        "{v}"
    );

    // Moving: within the row only.
    let (first, last) =
        (row_in(&v2, &r1)["docs"][0]["id"].as_str().unwrap(), row_in(&v2, &r1)["docs"][2]["id"].as_str().unwrap());
    let strip = v2["docs"][0]["id"].as_str().unwrap();
    let mv = |doc: &str, before: Option<&str>| {
        let (path, body) = (format!("/api/boards/{id}/docs/{doc}/move"), json!({ "before": before }));
        let base = base.clone();
        async move { call(&base, "a", "POST", &path, Some(body)).await }
    };
    let (st, v) = mv(last, Some(first)).await;
    assert_eq!((st, row_titles(&v, &r1)), (200, vec!["C".to_owned(), "A".into(), "B".into()]), "{v}");
    assert_eq!(mv(last, Some(strip)).await.0, 404, "not before a Doc of another place");
    assert_eq!(titles(&v), ["A"], "{v}");

    // The limit is per row.
    for i in 0..BOARD_DOCS_MAX {
        let new = crate::db::project_docs::NewDoc {
            project: &t,
            title: &format!("Doc {i}"),
            made_by: "a",
            made_by_session: None,
            sections: &[],
        };
        let d = app.db.create_project_doc(&new, 1, 100).unwrap().unwrap().id;
        let (st, v) = link_row(&base, "a", &id, &r2, ptr(&d)).await;
        assert_eq!(st, if i + 1 < BOARD_DOCS_MAX { 200 } else { 400 }, "{i}: {v}");
    }

    // The table itself refuses a second copy in the strip (a NULL row is one place), and on a row.
    let twice = |row: Option<&str>| {
        app.db.c().execute(
            "INSERT INTO board_docs (id, board_id, row_id, position, target_kind, target, machine, title, added_by, created_at)
             VALUES (?1, ?2, ?3, 9, 'doc', ?4, '', '', 'a', 1)",
            params![format!("bdc_{}", rand_hex()), id, row, a],
        )
    };
    assert!(twice(None).is_err(), "the strip has it");
    assert!(twice(Some(&r2)).is_err(), "so does the row");

    // A row's Docs go with it; the strip's and the other row's stay.
    let (st, v) = call(&base, "a", "DELETE", &format!("/api/boards/{id}/entries/{r1}"), None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(titles(&v), ["A"], "{v}");
    assert_eq!(row_titles(&v, &r2).len() as u64, BOARD_DOCS_MAX, "{v}");
    let n: u64 =
        app.db.c().query_row("SELECT count(*) FROM board_docs WHERE row_id = ?1", params![r1], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

/// A row's Doc moves no lane, is no work, proves nothing and doesn't change
/// the board's progress or its ready set.
#[tokio::test]
async fn a_row_doc_never_moves_a_lane_or_proves() {
    let (_hub, app, base, t, _u) = seeded().await;
    let id = board(&base, &t).await;
    let open = issue(&base, "a", &t, "Open one").await;
    let shut = issue(&base, "a", &t, "Shut one").await;
    app.db
        .c()
        .execute(
            "UPDATE issues SET state = 'closed', close_reason = 'completed', closed_at = 5 WHERE id = ?1",
            params![shut],
        )
        .unwrap();
    put(&base, "a", &id, json!({ "entry": "row", "pointer": { "kind": "issue", "id": open } })).await;
    let (_, before) = put(&base, "a", &id, json!({ "entry": "row", "pointer": { "kind": "issue", "id": shut } })).await;
    let (open_row, shut_row) = (
        rows_in(&before).iter().find(|r| r["pointer"]["id"] == open.as_str()).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned(),
        rows_in(&before).iter().find(|r| r["pointer"]["id"] == shut.as_str()).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned(),
    );
    assert_eq!((lane(&before, &open_row), lane(&before, &shut_row)), ("next".to_owned(), "done".to_owned()));
    assert!(before["zones"][3]["rows"][0]["proof"].is_null(), "shut, not proven: {before}");
    assert_eq!(before["ready"], json!([open_row]), "{before}");

    let d = doc(&base, "a", &t, "The spike", 1, 1).await;
    let mut after = Value::Null;
    for row in [&open_row, &shut_row] {
        let (st, v) = link_row(&base, "a", &id, row, json!({ "kind": "doc", "id": d })).await;
        assert_eq!(st, 200, "{v}");
        after = v;
    }
    assert_eq!((lane(&after, &open_row), lane(&after, &shut_row)), ("next".to_owned(), "done".to_owned()), "{after}");
    assert!(row_in(&after, &shut_row)["proof"].is_null(), "a Doc is no proof: {after}");
    assert_eq!(after["ready"], before["ready"], "{after}");
    assert_eq!(after["board"]["progress"], before["board"]["progress"], "{after}");
    assert_eq!(after["board"]["progress"]["proven"], 0, "{after}");
    let (_, list) = call(&base, "a", "GET", &format!("/api/projects/{t}/boards"), None).await;
    assert_eq!(list["boards"][0]["progress"], before["board"]["progress"], "the list's progress: {list}");
    let held = row_in(&after, &open_row);
    assert!(held["marks"].as_array().is_none_or(Vec::is_empty), "no mark: {held}");
}

/// A project Doc that is deleted reads as missing, in the strip and on a
/// row, and the board still reads.
#[tokio::test]
async fn a_deleted_doc_reads_as_missing() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let id = board(&base, &t).await;
    let d = doc(&base, "a", &t, "The spike", 2, 2).await;
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "the plan" })).await;
    let row = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    link(&base, "a", &id, json!({ "kind": "doc", "id": d })).await;
    link_row(&base, "a", &id, &row, json!({ "kind": "doc", "id": d })).await;

    let (st, _) = call(&base, "a", "DELETE", &format!("/api/projects/{t}/docs/{d}"), None).await;
    assert_eq!(st, 200);
    let (st, v) = call(&base, "x", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(st, 200, "{v}");
    let strip = &v["docs"][0];
    assert_eq!((&strip["title"], &strip["missing"]), (&json!("a deleted Doc"), &json!(true)), "{v}");
    assert!(strip.get("sections").is_none(), "{v}");
    let chip = &row_in(&v, &row)["docs"][0];
    assert_eq!((&chip["title"], &chip["missing"]), (&json!("a deleted Doc"), &json!(true)), "{v}");
    // It can still be taken off.
    let entry = strip["id"].as_str().unwrap();
    assert_eq!(call(&base, "a", "DELETE", &format!("/api/boards/{id}/docs/{entry}"), None).await.0, 200);
}

/// The Doc's page asks which boards list it: in the strip, on rows, both.
#[tokio::test]
async fn a_doc_says_which_boards_it_is_on() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let (one, two) = (board(&base, &t).await, board(&base, &t).await);
    let d = doc(&base, "a", &t, "The spike", 1, 1).await;
    let other = doc(&base, "a", &t, "Not linked", 1, 1).await;
    let on = |who: &'static str, d: String| {
        let (base, t) = (base.clone(), t.clone());
        async move { call(&base, who, "GET", &format!("/api/projects/{t}/docs/{d}/boards"), None).await }
    };
    let (st, v) = on("x", d.clone()).await;
    assert_eq!((st, &v["boards"]), (200, &json!([])), "{v}");

    link(&base, "a", &one, json!({ "kind": "doc", "id": d })).await;
    let (_, v) = put(&base, "a", &two, json!({ "entry": "row", "note": "a" })).await;
    let r1 = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let (_, v) = put(&base, "a", &two, json!({ "entry": "row", "note": "b" })).await;
    let r2 = v["zones"][0]["rows"][1]["id"].as_str().unwrap().to_owned();
    for r in [&r1, &r2] {
        link_row(&base, "a", &two, r, json!({ "kind": "doc", "id": d })).await;
    }
    let (st, v) = on("x", d.clone()).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        v["boards"],
        json!([
            { "board": one, "title": "Launch", "strip": true, "rows": 0 },
            { "board": two, "title": "Launch", "strip": false, "rows": 2 },
        ]),
        "{v}"
    );
    assert_eq!(on("a", other).await.1["boards"], json!([]));
    // A stranger has no such project.
    assert_eq!(on("c", d.clone()).await.0, 404);
}

/// A machine's agent with boards.write links a Doc on its own machine (the
/// title kept), and only its own; it takes off only what it linked. Without
/// boards.write it is refused. A person sees the machine's name.
#[tokio::test]
async fn an_agent_links_its_machines_docs() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let id = board(&base, &t).await;
    let mine = json!({ "kind": "machine_doc", "machine": "geek", "doc": "d7", "title": "  Perf notes  " });

    let (st, v) = geek(&base, "POST", &agent_board_docs(&id), Some(json!({ "pointer": mine }))).await;
    assert_eq!(st, 404, "no grant (a board it can't see is no board): {v}");
    grant_write(&base, "a", "geek", &t).await;
    let (st, v) = geek(&base, "POST", &agent_board_docs(&id), Some(json!({ "pointer": mine }))).await;
    assert_eq!(st, 200, "{v}");
    let entry = &v["docs"][0];
    assert_eq!(entry["title"], "Perf notes", "trimmed: {v}");
    assert_eq!(
        entry["pointer"],
        json!({ "kind": "machine_doc", "machine": "geek", "doc": "d7", "title": "Perf notes" })
    );
    assert_eq!((&entry["agent"]["machine"], &entry["added_by"]), (&json!("geek"), &json!("a")), "{v}");
    assert!(entry.get("missing").is_none(), "a machine Doc isn't missing: {v}");
    assert!(entry["machine_name"].is_string(), "{v}");
    let entry = entry["id"].as_str().unwrap().to_owned();

    // The same Doc again answers the one there.
    let (_, v) = geek(&base, "POST", &agent_board_docs(&id), Some(json!({ "pointer": mine }))).await;
    assert_eq!(titles(&v), ["Perf notes"], "{v}");
    // Another machine's Doc is not its to link, and a project Doc of
    // another project is no Doc.
    let theirs = json!({ "kind": "machine_doc", "machine": "xbox", "doc": "d1", "title": "x" });
    assert_eq!(geek(&base, "POST", &agent_board_docs(&id), Some(json!({ "pointer": theirs }))).await.0, 403);
    // A person can see it, and an agent can't take off a person's.
    let d = doc(&base, "a", &t, "The plan", 1, 1).await;
    let (_, v) = link(&base, "a", &id, json!({ "kind": "doc", "id": d })).await;
    let persons = v["docs"][1]["id"].as_str().unwrap().to_owned();
    let (st, v) = geek(&base, "DELETE", &agent_board_doc(&id, &persons), None).await;
    assert_eq!(st, 403, "{v}");
    let (st, v) = geek(&base, "DELETE", &agent_board_doc(&id, &entry), None).await;
    assert_eq!((st, titles(&v)), (200, vec!["The plan"]), "{v}");

    // On a row; the agent can take off its own.
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "the plan" })).await;
    let row = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let (st, v) = geek(&base, "POST", &agent_board_docs(&id), Some(json!({ "pointer": mine, "row": row }))).await;
    assert_eq!(st, 200, "{v}");
    let on_row = &row_in(&v, &row)["docs"][0];
    assert_eq!((&on_row["title"], &on_row["agent"]["machine"]), (&json!("Perf notes"), &json!("geek")), "{v}");
    assert!(on_row["machine_name"].is_string(), "{v}");
    let entry = on_row["id"].as_str().unwrap();
    let (st, v) = geek(&base, "DELETE", &agent_board_doc(&id, entry), None).await;
    assert_eq!((st, row_titles(&v, &row)), (200, Vec::<String>::new()), "{v}");
}

/// A person links a machine's Doc only from a machine they may reach: their
/// own, their team's, one shared with them; another's is refused, asked of
/// the hub every time, and the check fails closed while the hub can't
/// answer. A reader is told the machine's name, and its id, only when they
/// may reach it too, on the strip and on a row, so a link from before the
/// check, or by someone the project has since lost, names nothing to the
/// rest. That label asks the hub once a tick for what it hides, and a hub
/// that fails names nothing while the read still answers. An agent's link
/// names its machine while the machine writes the project.
#[tokio::test]
async fn a_person_links_docs_on_machines_they_reach() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.daemon("apad", "a", None);
    hub.daemon("xbox", "c", Some("u"));
    crate::cache::catch_up(&app).await.unwrap();
    let id = board(&base, &t).await;
    let on = |machine: &str, doc: &str| json!({ "kind": "machine_doc", "machine": machine, "doc": doc, "title": doc });
    let read = |who: &'static str| {
        let (base, id) = (base.clone(), id.clone());
        async move { call(&base, who, "GET", &format!("/api/boards/{id}"), None).await }
    };
    let find = |v: &Value, title: &str| -> Value {
        v["docs"].as_array().unwrap().iter().find(|d| d["title"] == title).unwrap_or_else(|| panic!("{v}")).clone()
    };
    let named = |v: &Value, title: &str| -> String { find(v, title)["machine_name"].as_str().unwrap().to_owned() };
    let tick = || app.cache.advance(crate::cache::CACHE_TICK);

    // Another's machine, and one that isn't: refused, asked every time.
    let (st, v) = link(&base, "a", &id, on("xbox", "theirs")).await;
    assert_eq!(st, 403, "{v}");
    assert!(v["error"].as_str().unwrap().contains("a machine they can reach"), "{v}");
    assert_eq!(link(&base, "a", &id, on("nobox", "nowhere")).await.0, 403);
    let asked = hub.calls_of("may_reach");
    assert_eq!(link(&base, "a", &id, on("xbox", "theirs")).await.0, 403);
    assert_eq!(hub.calls_of("may_reach"), asked + 1, "a check asks again");
    // While the hub can't answer, no link.
    hub.reach_down(true);
    let (st, v) = link(&base, "a", &id, on("apad", "my notes")).await;
    assert!(st >= 500, "the hub down: no link: {st} {v}");
    hub.reach_down(false);

    // The team's machine: its owner and a member alike; a member's own, on
    // the strip and on a row.
    let (st, v) = link(&base, "a", &id, on("geek", "team notes")).await;
    assert_eq!((st, named(&v, "team notes")), (200, "geek-box".to_owned()), "{v}");
    let (st, v) = link(&base, "b", &id, on("geek", "more notes")).await;
    assert_eq!((st, named(&v, "more notes")), (200, "geek-box".to_owned()), "an editor, the team's machine: {v}");
    assert!(find(&v, "more notes")["agent"].is_null(), "a person's link, not an agent's: {v}");
    let (st, v) = link(&base, "a", &id, on("apad", "my notes")).await;
    assert_eq!((st, named(&v, "my notes")), (200, "apad-box".to_owned()), "{v}");
    assert_eq!(find(&v, "my notes")["pointer"]["machine"], "apad", "its owner is told the id: {v}");
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "the plan" })).await;
    let row = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let (st, v) = link_row(&base, "a", &id, &row, on("apad", "row notes")).await;
    assert_eq!(st, 200, "{v}");
    let on_row = |v: &Value| -> (String, String) {
        let d = &row_in(v, &row)["docs"][0];
        (d["machine_name"].as_str().unwrap().to_owned(), d["pointer"]["machine"].as_str().unwrap().to_owned())
    };
    assert_eq!(on_row(&v), ("apad-box".to_owned(), "apad".to_owned()), "{v}");

    // Readers: the team's machine is named to every member; a's own only to
    // a, and blanked for the rest, on the strip and on the row.
    let (_, v) = read("b").await;
    assert_eq!(
        (named(&v, "team notes"), named(&v, "my notes")),
        ("geek-box".to_owned(), "a machine".to_owned()),
        "{v}"
    );
    assert_eq!(find(&v, "team notes")["pointer"]["machine"], "geek", "{v}");
    assert_eq!(
        find(&v, "my notes")["pointer"],
        json!({ "kind": "machine_doc", "machine": "", "doc": "my notes", "title": "my notes" }),
        "{v}"
    );
    assert_eq!(on_row(&v), ("a machine".to_owned(), String::new()), "{v}");
    assert_eq!(named(&read("x").await.1, "my notes"), "a machine");
    assert_eq!(named(&read("a").await.1, "my notes"), "apad-box");
    // What a label hides costs one hub call a tick, not one a read.
    let asked = hub.calls_of("may_reach");
    read("b").await;
    assert_eq!(hub.calls_of("may_reach"), asked, "a \"no\" is kept for the label");
    tick();
    read("b").await;
    assert_eq!(hub.calls_of("may_reach"), asked + 1, "and asked again a tick later");
    // A share reaches the label through the feed.
    hub.share("apad", "b", true);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(named(&read("b").await.1, "my notes"), "apad-box", "shared with b now");
    hub.share("apad", "b", false);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(named(&read("b").await.1, "my notes"), "a machine", "and no longer");

    // A link from before the check names its machine to nobody here.
    app.db
        .c()
        .execute(
            "INSERT INTO board_docs (id, board_id, row_id, position, target_kind, target, machine, title, added_by, created_at)
             VALUES (?1, ?2, NULL, 9, 'machine_doc', 'd1', 'xbox', 'old link', 'a', 1)",
            params![format!("bdc_{}", rand_hex()), id],
        )
        .unwrap();
    for who in ["a", "b", "x"] {
        let (_, v) = read(who).await;
        assert_eq!(
            (named(&v, "old link"), find(&v, "old link")["pointer"]["machine"].clone()),
            ("a machine".to_owned(), json!("")),
            "{who}: {v}"
        );
    }
    // While the hub can't say, the board still reads, naming nothing; then
    // it says again.
    hub.reach_down(true);
    tick();
    let (st, v) = read("a").await;
    assert_eq!((st, named(&v, "my notes")), (200, "a machine".to_owned()), "the hub down: {v}");
    assert_eq!(find(&v, "my notes")["pointer"]["machine"], "", "{v}");
    hub.reach_down(false);
    assert_eq!(named(&read("a").await.1, "my notes"), "apad-box", "nothing kept of the failure");

    // An agent's link names its machine while the machine writes the
    // project, to a reader who can't reach it too; a revoked grant hides
    // it, though its owner, who reaches it, is still told.
    let (st, g) = call(
        &base,
        "a",
        "POST",
        "/api/grants",
        crate::agents_tests::give("geek", "boards.write", json!({ "kind": "project", "id": t })),
    )
    .await;
    assert_eq!(st, 200, "{g}");
    let mine = json!({ "kind": "machine_doc", "machine": "geek", "doc": "d7", "title": "agent notes" });
    let (st, v) = geek(&base, "POST", &agent_board_docs(&id), Some(json!({ "pointer": mine }))).await;
    assert_eq!(st, 200, "{v}");
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer), ("c", TeamRole::Viewer)]);
    hub.daemon("geek", "a", None);
    crate::cache::catch_up(&app).await.unwrap();
    let (_, v) = read("c").await;
    assert_eq!(
        (named(&v, "agent notes"), named(&v, "team notes")),
        ("geek-box".to_owned(), "a machine".to_owned()),
        "{v}"
    );
    assert_eq!(find(&v, "agent notes")["agent"]["machine"], "geek", "{v}");
    let (st, _) = call(&base, "a", "POST", &format!("/api/grants/{}/revoke", g["id"].as_str().unwrap()), None).await;
    assert_eq!(st, 200);
    let (_, v) = read("c").await;
    let d = find(&v, "agent notes");
    assert_eq!(
        (
            named(&v, "agent notes"),
            d["pointer"]["machine"].clone(),
            d["agent"]["machine"].clone(),
            d["agent"]["machine_name"].clone()
        ),
        ("a machine".to_owned(), json!(""), json!(""), json!("")),
        "revoked: {v}"
    );
    assert_eq!(named(&read("a").await.1, "agent notes"), "geek-box");
}

/// A closed board keeps its rows editable, and so its Docs.
#[tokio::test]
async fn a_closed_boards_docs_stay_editable() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let id = board(&base, &t).await;
    let d = doc(&base, "a", &t, "The spike", 1, 1).await;
    let (st, _) =
        call(&base, "a", "POST", &format!("/api/boards/{id}/close"), Some(json!({ "done_note": "Done." }))).await;
    assert_eq!(st, 200);
    let (st, v) = link(&base, "a", &id, json!({ "kind": "doc", "id": d })).await;
    assert_eq!((st, titles(&v)), (200, vec!["The spike"]), "{v}");
}

/// A machine Doc's chip reads the machine's copy (#418) only where the copy
/// sits, the board's own project (native #673): title and sections as the
/// machine last put them, and the copy's id, which opens it while the
/// machine is away. Anywhere else the chip keeps the title it was linked
/// with: on the team's board, on a teammate's own board, and on the board
/// of someone the team took out, who can still link the pointer they saw.
#[tokio::test]
async fn a_machine_docs_chip_reads_its_copy_only_in_the_copys_project() {
    let (hub, app, base, t, _u) = seeded().await;
    let id = board(&base, &t).await;
    grant_write(&base, "a", "geek", &t).await;
    let mine = json!({ "kind": "machine_doc", "machine": "geek", "doc": "d7", "title": "Perf notes" });
    let (st, v) = geek(&base, "POST", &agent_board_docs(&id), Some(json!({ "pointer": mine }))).await;
    assert_eq!(st, 200, "{v}");
    assert!(v["docs"][0].get("copy").is_none() && v["docs"][0].get("sections").is_none(), "no copy yet: {v}");

    let put_doc = json!({
        "title": "Perf notes, now private",
        "sections": [{ "id": "s1", "heading": "Setup", "body": "x" }, { "id": "s2", "heading": "Numbers" }],
    });
    // geek, a team's machine, copies no more Docs up (native #673): this is
    // a copy it made before, put from a's own machine and keyed as geek's.
    hub.daemon("apad", "a", None);
    crate::cache::catch_up(&app).await.unwrap();
    let (st, p) = signed(&base, "a apad", "PUT", &machine_doc("d7"), Some(put_doc)).await;
    assert_eq!(st, 200, "{p}");
    let pdc = p["id"].as_str().unwrap();
    let n = app.db.c().execute("UPDATE project_docs SET machine = 'geek' WHERE id = ?1", params![pdc]).unwrap();
    assert_eq!(n, 1);
    // The team's board: the copy is in a's personal project, so the chip
    // keeps the title it was linked with.
    let snapshot = |d: &Value| {
        assert_eq!(d["title"], "Perf notes", "{d}");
        assert!(d.get("sections").is_none() && d.get("written").is_none() && d.get("copy").is_none(), "{d}");
    };
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    snapshot(&v["docs"][0]);
    assert_eq!(v["docs"][0]["machine_name"], "geek-box", "{v}");

    // On a board of a's own project, where the copy is, the chip reads it.
    let (st, b) = call(&base, "a", "POST", "/api/projects/personal/boards", Some(json!({ "title": "Mine" }))).await;
    assert_eq!(st, 200, "{b}");
    let own = b["board"]["id"].as_str().unwrap().to_owned();
    let (st, v) = link(&base, "a", &own, mine.clone()).await;
    assert_eq!(st, 200, "{v}");
    let d = &v["docs"][0];
    assert_eq!(
        (&d["copy"], &d["title"], &d["sections"], &d["written"]),
        (&json!(pdc), &json!("Perf notes, now private"), &json!(2), &json!(1)),
        "{v}"
    );
    assert_eq!(d["pointer"]["title"], "Perf notes", "the pointer keeps its snapshot: {v}");
    // On a row too.
    let (_, v) = put(&base, "a", &own, json!({ "entry": "row", "note": "the numbers" })).await;
    let row = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let (st, v) = link_row(&base, "a", &own, &row, mine.clone()).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(row_in(&v, &row)["docs"][0]["copy"], json!(pdc), "{v}");

    // A teammate links the pointer on a board of their own: the snapshot.
    let (_, b) = call(&base, "b", "POST", "/api/projects/personal/boards", Some(json!({ "title": "B's" }))).await;
    let bs = b["board"]["id"].as_str().unwrap().to_owned();
    let (st, v) = link(&base, "b", &bs, mine.clone()).await;
    assert_eq!(st, 200, "{v}");
    snapshot(&v["docs"][0]);
    // Taken out of the team, b still has the pointer, and still only that.
    hub.team("t", &[("a", TeamRole::Owner), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    let (_, v) = call(&base, "b", "GET", &format!("/api/boards/{bs}"), None).await;
    snapshot(&v["docs"][0]);
    assert_eq!(
        call(&base, "b", "GET", &format!("/api/boards/{id}"), None).await.0,
        404,
        "the team's board is gone for b"
    );
}

/// Native #673 with the machine's id blanked: a's own machine Doc, linked
/// by a to the team's board, whose copy sits in the team's project (as a
/// move left it). b, who doesn't reach a's machine, is told "a machine"
/// with the id blanked, and still reads the copy, which sits where b may
/// read: its title, its counts and its id. a reads the same, with the id.
#[tokio::test]
async fn a_reader_not_told_the_machine_still_reads_a_copy_in_the_boards_project() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.daemon("apad", "a", None);
    crate::cache::catch_up(&app).await.unwrap();
    let id = board(&base, &t).await;
    let put_doc = json!({ "title": "Pad notes, now", "sections": [{ "id": "s1", "heading": "One", "body": "x" }] });
    let (st, p) = signed(&base, "a apad", "PUT", &machine_doc("d9"), Some(put_doc)).await;
    assert_eq!(st, 200, "{p}");
    let pdc = p["id"].as_str().unwrap().to_owned();
    let n = app.db.c().execute("UPDATE project_docs SET project_id = ?1 WHERE id = ?2", params![t, pdc]).unwrap();
    assert_eq!(n, 1);
    let pointer = json!({ "kind": "machine_doc", "machine": "apad", "doc": "d9", "title": "Pad notes" });
    let (st, v) = link(&base, "a", &id, pointer).await;
    assert_eq!(st, 200, "{v}");

    let (_, v) = call(&base, "b", "GET", &format!("/api/boards/{id}"), None).await;
    let d = &v["docs"][0];
    assert_eq!((&d["machine_name"], &d["pointer"]["machine"]), (&json!("a machine"), &json!("")), "blanked: {v}");
    assert_eq!(
        (&d["copy"], &d["title"], &d["sections"], &d["written"]),
        (&json!(pdc), &json!("Pad notes, now"), &json!(1), &json!(1)),
        "the copy still read: {v}"
    );
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    let d = &v["docs"][0];
    assert_eq!((&d["pointer"]["machine"], &d["copy"]), (&json!("apad"), &json!(pdc)), "{v}");
}
