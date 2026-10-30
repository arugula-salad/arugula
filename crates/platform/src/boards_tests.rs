//! Boards through the platform's routes (boards, step 1), with
//! the hub of chat's tests: a board made, zones and rows added and
//! reordered, a stale write refused, and the `boards` flag off answering
//! 404.

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

    // The proofs zone: a link is kept as it was said and refused if it isn't
    // plain https; a run row shows the run's state to a reader of its
    // project, and nothing to one who isn't.
    let proof = |pointer: Value| put(&base, "a", &id, json!({ "entry": "row", "zone": proofs, "pointer": pointer }));
    let (st, v) = proof(json!({ "kind": "link", "url": "https://ci.example.com/runs/42?x=1#top" })).await;
    assert_eq!(
        (st, &v["zones"][1]["rows"][0]["pointer"]),
        (200, &json!({ "kind": "link", "url": "https://ci.example.com/runs/42?x=1#top" })),
        "{v}"
    );
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
        let (st, v) = proof(json!({ "kind": "link", "url": bad })).await;
        assert_eq!(st, 400, "{bad}: {v}");
        assert!(v.to_string().contains("https"), "{bad}: {v}");
    }
    let mk = |project: &str, state: &str| {
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
    };
    let theirs = app.db.team_project("u", "u", 1).unwrap();
    let (ok, running, canceled, hidden) =
        (mk(&p, "succeeded"), mk(&p, "running"), mk(&p, "canceled"), mk(&theirs, "succeeded"));
    for r in [&ok, &running, &canceled, &hidden] {
        let (st, v) = proof(json!({ "kind": "run", "id": r })).await;
        assert_eq!(st, 200, "{v}");
    }
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    let live = |i: usize| v["zones"][1]["rows"][i]["live"].clone();
    assert_eq!(v["zones"][1]["rows"][0].get("live"), None, "a link has no live state");
    assert_eq!(live(1), json!({ "state": "succeeded", "kind": "run", "number": 0, "title": "manual run" }));
    assert_eq!(live(2)["state"], json!("running"));
    assert_eq!(live(3)["state"], json!("other"));
    assert_eq!(v["zones"][1]["rows"][4].get("live"), None, "a run of a project the reader isn't in");
    assert_eq!(v["zones"][1]["rows"][4]["pointer"]["id"], json!(hidden));
    // Its thread (an object thread, as an issue's is) is chat's:
    // `arugula-platform-chat`'s `boards_tests`.
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
    let (_, v) = put(&base, "a", &id, json!({ "entry": "zone", "title": "Now" })).await;
    let zone = v["zones"][0]["id"].as_str().unwrap().to_owned();
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "zone": zone, "note": "proof" })).await;
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
    let (st, _) = put(&base, "a", &id, json!({ "id": row, "status": "done" })).await;
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
        ("PUT", format!("/api/boards/{id}/entries"), Some(json!({ "entry": "zone", "title": "Now" }))),
        ("POST", format!("/api/boards/{id}/entries/zon_0123/move"), Some(json!({}))),
        ("DELETE", format!("/api/boards/{id}/entries/zon_0123"), None),
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

/// A machine with `boards.write` adds rows, sets notes and statuses (a
/// PROOFS row's too) and moves rows, labelled as its agent's; it can't
/// repoint or delete a row a person added. Without the grant it is refused,
/// saying how to give it.
#[tokio::test]
async fn an_agent_with_boards_write_works_the_rows() {
    use crate::{agents_tests::give, grants::AGENT_UNGRANTED};
    use arugula_control_wire::boards::{
        agent_board, agent_board_entries, agent_board_entry, agent_board_entry_move, agent_project_boards,
    };

    let (hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
    hub.flag_named("team:t", FLAG, true);

    // A person's board: zones Now and PROOFS, a row in each.
    let (_, b) =
        call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "Launch" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    put(&base, "a", &id, json!({ "entry": "zone", "title": "PROOFS", "kind": "proofs" })).await;
    let (_, v) = put(&base, "a", &id, json!({ "entry": "zone", "title": "Now" })).await;
    let (now, proofs) =
        (v["zones"][0]["id"].as_str().unwrap().to_owned(), v["zones"][1]["id"].as_str().unwrap().to_owned());
    put(&base, "a", &id, json!({ "entry": "row", "zone": now, "note": "the plan" })).await;
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "zone": proofs, "note": "tests green" })).await;
    let (plan, green) = (
        v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned(),
        v["zones"][1]["rows"][0]["id"].as_str().unwrap().to_owned(),
    );

    // No grant: refused, with how to give it. Read or write alike.
    let (st, v) = geek(&base, "GET", &agent_board(&id), None).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    assert!(
        v["error"].as_str().unwrap().contains(&format!("arugula grants add geek-box boards.read --project {t}")),
        "{v}"
    );
    let add = json!({ "entry": "row", "zone": now, "note": "sneaky" });
    let (st, v) = geek(&base, "PUT", &agent_board_entries(&id), Some(add.clone())).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(shape(&v), [zone("Now", &["the plan"]), zone("PROOFS", &["tests green"])], "nothing was written");

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

    // boards.write: add a row (labelled), set its note and status.
    let (st, g) =
        call(&base, "a", "POST", "/api/grants", give("geek", "boards.write", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");
    let (st, v) = geek(
        &base,
        "PUT",
        &agent_board_entries(&id),
        Some(json!({ "entry": "row", "zone": now, "note": "agent row", "status": "working" })),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    let mine = v["zones"][0]["rows"][1].clone();
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
    let (st, v) = geek(&base, "PUT", &agent_board_entries(&id), Some(json!({ "id": green, "status": "done" }))).await;
    assert_eq!((st, &v["zones"][1]["rows"][0]["status"]), (200, &json!("done")), "a PROOFS row's status: {v}");
    assert_eq!(v["zones"][1]["rows"][0]["rev"], json!(1), "a status leaves the rev");

    // Move a row, to another zone and back.
    let (st, v) = geek(&base, "POST", &agent_board_entry_move(&id, &mine_id), Some(json!({ "zone": proofs }))).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(shape(&v), [zone("Now", &["the plan, revised"]), zone("PROOFS", &["tests green", "agent row"])]);
    let (st, _) = geek(&base, "POST", &agent_board_entry_move(&id, &mine_id), Some(json!({ "zone": now }))).await;
    assert_eq!(st, 200);

    // What it may never do to a person's row: repoint it or delete it.
    let no = |what: &str, r: (u16, Value)| assert_eq!(r.0, 403, "{what}: {}", r.1);
    no(
        "repoint a person's row",
        geek(
            &base,
            "PUT",
            &agent_board_entries(&id),
            Some(json!({ "id": plan, "rev": 2, "pointer": { "kind": "run", "id": "run_ab12" } })),
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
    assert_eq!(shape(&v), [zone("Now", &["the plan, revised"]), zone("PROOFS", &["tests green"])]);

    // A person sees the zones as they were, and still deletes anything.
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

/// An agent makes a board with a goal, its zones and rows, and closes it
/// with a note; a read names the agent on both, and the person it acted for
/// stays `created_by` and `closed_by`. A board a person made says no agent.
#[tokio::test]
async fn an_agent_makes_and_runs_a_board() {
    use arugula_control_wire::boards::{
        agent_board, agent_board_close, agent_board_entry_move, agent_board_reopen, agent_project_boards,
    };

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

    // Zones: a plain one, PROOFS, one more before the first, renamed.
    let zid = |v: &Value, i: usize| v["zones"][i]["id"].as_str().unwrap().to_owned();
    let (st, v) = geek_put(&base, &id, json!({ "entry": "zone", "title": "Now" })).await;
    assert_eq!(st, 200, "{v}");
    let (st, v) = geek_put(&base, &id, json!({ "entry": "zone", "title": "PROOFS", "kind": "proofs" })).await;
    assert_eq!(st, 200, "{v}");
    let (_, v) = geek_put(&base, &id, json!({ "entry": "zone", "title": "Later" })).await;
    let (now, later) = (zid(&v, 0), zid(&v, 1));
    let (st, v) = geek(&base, "POST", &agent_board_entry_move(&id, &later), Some(json!({ "before": now }))).await;
    assert_eq!(st, 200, "{v}");
    let (st, v) = geek_put(&base, &id, json!({ "id": now, "rev": 1, "title": "Next" })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(shape(&v), [zone("Later", &[]), zone("Next", &[]), zone("PROOFS", &[])], "{v}");
    let (st, v) = geek_put(&base, &id, json!({ "entry": "row", "zone": now, "note": "first ticket" })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["zones"][1]["rows"][0]["agent"], who, "{v}");

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
    use arugula_control_wire::boards::{agent_board, agent_board_close, agent_board_entries, agent_project_boards};

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
    let (st, v) = xbox(&base, "PUT", &agent_board_entries(&id), Some(json!({ "entry": "zone", "title": "Now" }))).await;
    assert_eq!(st, 403, "make a zone: {v}");
    assert_eq!(xbox(&base, "GET", &agent_board(&id), None).await.0, 200, "it reads");
}

/// An agent deletes a zone that is empty or holds only its own machine's
/// rows, and not one that holds a row another hand added.
#[tokio::test]
async fn an_agent_deletes_only_a_zone_without_others_rows() {
    use arugula_control_wire::boards::{agent_board_entries, agent_board_entry};

    let (hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
    hub.flag_named("team:t", FLAG, true);
    grant_write(&base, "a", "geek", &t).await;
    let (_, b) =
        call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "Launch" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    for title in ["Theirs", "Mine", "Empty"] {
        put(&base, "a", &id, json!({ "entry": "zone", "title": title })).await;
    }
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    let [theirs, mine, empty] = [0, 1, 2].map(|i| v["zones"][i]["id"].as_str().unwrap().to_owned());
    put(&base, "a", &id, json!({ "entry": "row", "zone": theirs, "note": "the plan" })).await;
    // One of each in Theirs; only the agent's in Mine.
    geek(&base, "PUT", &agent_board_entries(&id), Some(json!({ "entry": "row", "zone": theirs, "note": "agent" })))
        .await;
    geek(&base, "PUT", &agent_board_entries(&id), Some(json!({ "entry": "row", "zone": mine, "note": "agent" }))).await;

    let (st, v) = geek(&base, "DELETE", &agent_board_entry(&id, &theirs), None).await;
    assert_eq!(st, 403, "{v}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(shape(&v), [zone("Theirs", &["the plan", "agent"]), zone("Mine", &["agent"]), zone("Empty", &[])]);
    let (st, v) = geek(&base, "DELETE", &agent_board_entry(&id, &mine), None).await;
    assert_eq!(st, 200, "its own rows only: {v}");
    let (st, v) = geek(&base, "DELETE", &agent_board_entry(&id, &empty), None).await;
    assert_eq!(st, 200, "empty: {v}");
    assert_eq!(shape(&v), [zone("Theirs", &["the plan", "agent"])]);
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
    let (_, v) = put(&base, "a", &id, json!({ "entry": "zone", "title": "Now" })).await;
    let now = v["zones"][0]["id"].as_str().unwrap().to_owned();
    let (_, v) = put(&base, "a", &id, json!({ "entry": "row", "zone": now, "note": "theirs" })).await;
    let theirs = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let (_, v) =
        geek(&base, "PUT", &agent_board_entries(&id), Some(json!({ "entry": "row", "zone": now, "note": "mine" })))
            .await;
    let mine = v["zones"][0]["rows"][1]["id"].as_str().unwrap().to_owned();

    let pointer = json!({ "kind": "run", "id": "run_ab12" });
    let (st, v) =
        geek(&base, "PUT", &agent_board_entries(&id), Some(json!({ "id": theirs, "rev": 1, "pointer": pointer })))
            .await;
    assert_eq!(st, 403, "{v}");
    let (st, v) =
        geek(&base, "PUT", &agent_board_entries(&id), Some(json!({ "id": mine, "rev": 1, "pointer": pointer }))).await;
    assert_eq!((st, &v["zones"][0]["rows"][1]["pointer"]), (200, &pointer), "{v}");
    assert_eq!(v["zones"][0]["rows"][0]["pointer"], json!({ "kind": "none" }), "{v}");
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
/// pointing at the block shows it. Without the grant the report is refused,
/// and a machine reports only its own blocks: the body names no machine.
#[tokio::test]
async fn a_machine_reports_its_blocks_and_the_row_shows_it() {
    use crate::{agents_tests::give, grants::AGENT_UNGRANTED};
    use arugula_control_wire::boards::AGENT_BLOCK_STATE;

    let (hub, _app, base, t, _u) = crate::agents_tests::seeded().await;
    hub.flag_named("team:t", FLAG, true);

    // A board with a row on geek's pane 7.
    let (_, b) =
        call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "Launch" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let (_, v) = put(&base, "a", &id, json!({ "entry": "zone", "title": "Now" })).await;
    let now = v["zones"][0]["id"].as_str().unwrap().to_owned();
    let pointer = json!({ "kind": "block", "machine": "geek", "pane": "7" });
    put(&base, "a", &id, json!({ "entry": "row", "zone": now, "note": "parser", "pointer": pointer })).await;

    let report = json!({ "pane": "7", "block": {
        "state": "needs_input", "branch": "feat/parser", "head": "abc1234", "pushed": false,
        "uncommitted": 2, "diffstat": { "files": 3, "added": 40, "removed": 2 } } });
    let (st, v) = geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report.clone())).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "no grant: {v}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(v["zones"][0]["rows"][0]["block"], Value::Null, "nothing was kept: {v}");

    let (st, granted) =
        call(&base, "a", "POST", "/api/grants", give("geek", "boards.write", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{granted}");
    let (st, v) = geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report)).await;
    assert_eq!(st, 200, "{v}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(
        v["zones"][0]["rows"][0]["block"],
        json!({ "state": "needs_input", "branch": "feat/parser", "head": "abc1234", "pushed": false,
                "uncommitted": 2, "diffstat": { "files": 3, "added": 40, "removed": 2 } }),
        "the row shows the state: {v}"
    );

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
    assert_eq!(v["zones"][0]["rows"][0]["block"]["state"], json!("needs_input"), "geek's block is geek's: {v}");
    let spoof = json!({ "pane": "7", "machine": "geek", "block": { "state": "idle" } });
    let (st, _) = signed(&base, "a other", "PUT", AGENT_BLOCK_STATE, Some(spoof)).await;
    assert!(st == 400 || st == 422, "no machine in the body: {st}");
    let said = json!({ "pane": "7", "block": { "state": "idle", "message": "all done" } });
    let (st, _) = geek(&base, "PUT", AGENT_BLOCK_STATE, Some(said)).await;
    assert!(st == 400 || st == 422, "no message text: {st}");

    // Revoking the grant hides what it reported.
    let (st, _) =
        call(&base, "a", "POST", &format!("/api/grants/{}/revoke", granted["id"].as_str().unwrap()), None).await;
    assert_eq!(st, 200);
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(v["zones"][0]["rows"][0]["block"], Value::Null, "{v}");
}
