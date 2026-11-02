//! A board's Docs (Arugula #517) through the platform's routes: the strip
//! (link, unlink, move, the limit), a Doc on a row as a reference that moves
//! no lane and proves nothing, a Doc of another project refused, a deleted
//! Doc read as missing, and what the Doc's page says of its boards.

use arugula_control_wire::boards::{BOARD_DOCS_MAX, agent_board_attachments, agent_board_doc, agent_board_docs};
use serde_json::{Value, json};

use super::{
    FLAG,
    tests::{attach, geek, grant_write, issue, lane, put, row_in, rows_in},
};
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

fn titles(v: &Value) -> Vec<&str> {
    v["docs"].as_array().unwrap().iter().map(|d| d["title"].as_str().unwrap()).collect()
}

/// Link two Docs, see them in order with their sections, link one again (it
/// stays once), move one, take one off. A viewer reads and links nothing.
#[tokio::test]
async fn link_move_and_unlink_a_docs_strip() {
    let (hub, _app, base, t, _u) = seeded().await;
    hub.flag_named("team:t", FLAG, true);
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
    let (hub, app, base, t, _u) = seeded().await;
    hub.flag_named("team:t", FLAG, true);
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
    hub.flag_named("team:t", FLAG, true);
    hub.flag_named("team:u", FLAG, true);
    hub.flag("team:u", true);
    let id = board(&base, &t).await;
    let theirs = doc(&base, "c", &u, "Their plan", 1, 1).await;
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "the plan" })).await;
    let row = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();

    let (st, v) = link(&base, "a", &id, json!({ "kind": "doc", "id": theirs })).await;
    assert_eq!(st, 404, "{v}");
    let (st, v) =
        attach(&base, "a", &id, json!({ "row": row, "role": "reference", "pointer": { "kind": "doc", "id": theirs } }))
            .await;
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

/// A reference is a Doc on a row: it takes only a Doc, and a Doc takes only
/// a reference; it is never a row's item.
#[tokio::test]
async fn only_a_doc_is_a_reference_and_a_doc_is_only_a_reference() {
    let (hub, _app, base, t, _u) = seeded().await;
    hub.flag_named("team:t", FLAG, true);
    let id = board(&base, &t).await;
    let d = doc(&base, "a", &t, "The spike", 2, 1).await;
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "the plan" })).await;
    let row = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let doc_ptr = json!({ "kind": "doc", "id": d });

    for role in ["work", "proof"] {
        let (st, v) = attach(&base, "a", &id, json!({ "row": row, "role": role, "pointer": doc_ptr })).await;
        assert_eq!(st, 400, "{role}: {v}");
    }
    let link_ptr = json!({ "kind": "link", "url": "https://example.com/x" });
    let (st, v) = attach(&base, "a", &id, json!({ "row": row, "role": "reference", "pointer": link_ptr })).await;
    assert_eq!(st, 400, "{v}");
    let (st, v) = put(&base, "a", &id, json!({ "entry": "row", "pointer": doc_ptr })).await;
    assert_eq!(st, 400, "a Doc is not a row's item: {v}");

    let (st, v) = attach(&base, "a", &id, json!({ "row": row, "role": "reference", "pointer": doc_ptr })).await;
    assert_eq!(st, 200, "{v}");
    let att = &row_in(&v, &row)["attachments"][0];
    assert_eq!(
        (&att["role"], &att["doc"]["title"], &att["doc"]["written"]),
        (&json!("reference"), &json!("The spike"), &json!(1)),
        "{v}"
    );
    // Once on a row; the strip is a list of its own.
    let (_, v) = attach(&base, "a", &id, json!({ "row": row, "role": "reference", "pointer": doc_ptr })).await;
    assert_eq!(row_in(&v, &row)["attachments"].as_array().unwrap().len(), 1, "{v}");
    assert!(v["said"].as_str().unwrap().contains("already"), "{v}");
    let (_, v) = link(&base, "a", &id, doc_ptr.clone()).await;
    assert_eq!(titles(&v), ["The spike"], "{v}");

    // A viewer attaches nothing; an editor takes it off.
    let (st, _) = attach(&base, "x", &id, json!({ "row": row, "role": "reference", "pointer": doc_ptr })).await;
    assert_eq!(st, 403);
    let att_id = row_in(&v, &row)["attachments"][0]["id"].as_str().unwrap().to_owned();
    let (st, v) = call(&base, "b", "DELETE", &format!("/api/boards/{id}/attachments/{att_id}"), None).await;
    assert_eq!(st, 200, "{v}");
    assert!(row_in(&v, &row)["attachments"].as_array().unwrap().is_empty(), "{v}");
}

/// A reference moves no lane, is no work, proves nothing and doesn't change
/// the board's progress or its ready set.
#[tokio::test]
async fn a_reference_never_moves_a_lane_or_proves() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.flag_named("team:t", FLAG, true);
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
        let (st, v) =
            attach(&base, "a", &id, json!({ "row": row, "role": "reference", "pointer": { "kind": "doc", "id": d } }))
                .await;
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
    let (hub, _app, base, t, _u) = seeded().await;
    hub.flag_named("team:t", FLAG, true);
    let id = board(&base, &t).await;
    let d = doc(&base, "a", &t, "The spike", 2, 2).await;
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "the plan" })).await;
    let row = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    link(&base, "a", &id, json!({ "kind": "doc", "id": d })).await;
    attach(&base, "a", &id, json!({ "row": row, "role": "reference", "pointer": { "kind": "doc", "id": d } })).await;

    let (st, _) = call(&base, "a", "DELETE", &format!("/api/projects/{t}/docs/{d}"), None).await;
    assert_eq!(st, 200);
    let (st, v) = call(&base, "x", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(st, 200, "{v}");
    let strip = &v["docs"][0];
    assert_eq!((&strip["title"], &strip["missing"]), (&json!("a deleted Doc"), &json!(true)), "{v}");
    assert!(strip.get("sections").is_none(), "{v}");
    let chip = &row_in(&v, &row)["attachments"][0]["doc"];
    assert_eq!((&chip["title"], &chip["missing"]), (&json!("a deleted Doc"), &json!(true)), "{v}");
    // It can still be taken off.
    let entry = strip["id"].as_str().unwrap();
    assert_eq!(call(&base, "a", "DELETE", &format!("/api/boards/{id}/docs/{entry}"), None).await.0, 200);
}

/// The Doc's page asks which boards list it: in the strip, on rows, both.
#[tokio::test]
async fn a_doc_says_which_boards_it_is_on() {
    let (hub, _app, base, t, _u) = seeded().await;
    hub.flag_named("team:t", FLAG, true);
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
        attach(&base, "a", &two, json!({ "row": r, "role": "reference", "pointer": { "kind": "doc", "id": d } })).await;
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
    // A stranger has no such project, and with boards off nothing is said.
    hub.flag_named("team:u", FLAG, true);
    assert_eq!(on("c", d.clone()).await.0, 404);
}

/// With boards off, the Doc's page hears of no boards, and no route answers.
#[tokio::test]
async fn with_boards_off_a_doc_is_on_no_boards() {
    let (hub, _app, base, t, _u) = seeded().await;
    hub.flag_named("team:t", FLAG, false);
    let d = doc(&base, "a", &t, "The spike", 1, 1).await;
    let (st, v) = call(&base, "a", "GET", &format!("/api/projects/{t}/docs/{d}/boards"), None).await;
    assert_eq!((st, &v["boards"]), (200, &json!([])), "{v}");
    assert_eq!(link(&base, "a", "brd_00", json!({ "kind": "doc", "id": d })).await.0, 404);
}

/// A machine's agent with boards.write links a Doc on its own machine (the
/// title kept), and only its own; it takes off only what it linked. Without
/// boards.write it is refused. A person sees the machine's name.
#[tokio::test]
async fn an_agent_links_its_machines_docs() {
    let (hub, _app, base, t, _u) = seeded().await;
    hub.flag_named("team:t", FLAG, true);
    let id = board(&base, &t).await;
    let mine = json!({ "kind": "machine_doc", "machine": "geek", "doc": "d7", "title": "  Perf notes  " });

    let (st, v) = geek(&base, "POST", &agent_board_docs(&id), Some(json!({ "pointer": mine }))).await;
    assert_eq!(st, 403, "no grant: {v}");
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

    // On a row, as a reference; the agent can take off its own.
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "note": "the plan" })).await;
    let row = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let (st, v) = geek(
        &base,
        "POST",
        &agent_board_attachments(&id),
        Some(json!({ "row": row, "role": "reference", "pointer": mine })),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    let att = &row_in(&v, &row)["attachments"][0];
    assert_eq!((&att["role"], &att["doc"]["title"]), (&json!("reference"), &json!("Perf notes")), "{v}");
    assert!(att["doc"]["machine_name"].is_string(), "{v}");
}

/// A closed board keeps its rows editable, and so its Docs.
#[tokio::test]
async fn a_closed_boards_docs_stay_editable() {
    let (hub, _app, base, t, _u) = seeded().await;
    hub.flag_named("team:t", FLAG, true);
    let id = board(&base, &t).await;
    let d = doc(&base, "a", &t, "The spike", 1, 1).await;
    let (st, _) =
        call(&base, "a", "POST", &format!("/api/boards/{id}/close"), Some(json!({ "done_note": "Done." }))).await;
    assert_eq!(st, 200);
    let (st, v) = link(&base, "a", &id, json!({ "kind": "doc", "id": d })).await;
    assert_eq!((st, titles(&v)), (200, vec!["The spike"]), "{v}");
}
