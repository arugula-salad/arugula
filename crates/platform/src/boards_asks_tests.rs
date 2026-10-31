//! Asks on boards (Arugula #367) through the platform's routes: an agent
//! asks and a person answers, who may answer, the limits on an ask, and the
//! asks the board raises itself. What a reader without access to a private
//! PR is not told is in `forge::lanes_tests`.

use serde_json::{Value, json};

use super::FLAG;
use super::tests::{issue, row_in};
use crate::db::sql::Sql as _;
use crate::{
    agents_tests::{give, seeded},
    chat_tests::call,
};
use arugula_control_wire::boards::{AGENT_BLOCK_STATE, agent_board_row_ask, board_ask_answer, board_row_ask};

/// A call signed by a's machine geek.
async fn geek(base: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req = reqwest::Client::new()
        .request(method.parse().unwrap(), format!("{base}{path}"))
        .header("x-test-daemon", "a geek");
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

/// Team t's world (a owns, b edits, x views, a's machine geek holds
/// `boards.write`) and a board with a row on an issue and a note row:
/// (base, app, board, issue row, note row).
async fn world() -> (String, std::sync::Arc<crate::Platform>, String, String, String) {
    let (hub, app, base, t, _u) = seeded().await;
    hub.flag_named("team:t", FLAG, true);
    let (st, g) =
        call(&base, "a", "POST", "/api/grants", give("geek", "boards.write", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");
    let (_, b) = call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "Ask" }))).await;
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    let parser = issue(&base, "a", &t, "Parser").await;
    let path = format!("/api/boards/{id}/entries");
    let put = |body| call(&base, "a", "PUT", &path, Some(body));
    let (_, v) = put(json!({ "entry": "row", "note": "parser", "pointer": { "kind": "issue", "id": parser } })).await;
    let row = v["zones"][0]["rows"][0]["id"].as_str().unwrap().to_owned();
    let (_, v) = put(json!({ "entry": "row", "note": "just a note" })).await;
    let note = v["zones"][0]["rows"].as_array().unwrap().iter().find(|r| r["note"] == "just a note").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (base, app, id, row, note)
}

async fn get(base: &str, who: &str, board: &str) -> Value {
    let (st, v) = call(base, who, "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(st, 200, "{v}");
    v
}

fn question() -> Value {
    json!({ "who": "x", "question": "Ship the parser with the old grammar?", "why": "The new one is slower.",
            "options": ["Ship it", "Wait"], "recommend": "Wait" })
}

/// An agent asks with options; the person named taps one; the row stops
/// asking, keeps the answer, and the agent reads it from the board.
#[tokio::test]
async fn an_agent_asks_and_the_person_it_names_answers() {
    let (base, _app, id, row, _) = world().await;

    let (st, v) = geek(&base, "POST", &agent_board_row_ask(&id, &row), Some(question())).await;
    assert_eq!(st, 200, "{v}");
    let r = row_in(&v, &row);
    assert_eq!(r["status"], json!("needs_input"), "the legacy status reads the ask: {r}");
    let ask = &r["ask"];
    assert_eq!(
        (ask["who"].as_str(), ask["question"].as_str(), ask["recommend"].as_str(), ask["raised"].clone()),
        (Some("x"), Some("Ship the parser with the old grammar?"), Some("Wait"), json!(false)),
        "{ask}"
    );
    assert_eq!(ask["options"], json!(["Ship it", "Wait"]));
    assert!(ask["asked_by"].as_str().unwrap().starts_with("an agent on "), "{ask}");
    let ask_id = ask["id"].as_str().unwrap().to_owned();
    assert!(ask_id.starts_with("ask_"));

    // For x it is theirs to answer; for a (an owner) it is answerable, not theirs.
    let mine = |v: &Value| (v["mine"].clone(), v["can_answer"].clone());
    assert_eq!(mine(&row_in(&get(&base, "x", &id).await, &row)["ask"]), (json!(true), json!(true)));
    assert_eq!(mine(&row_in(&get(&base, "a", &id).await, &row)["ask"]), (json!(false), json!(true)));
    assert_eq!(mine(&row_in(&get(&base, "b", &id).await, &row)["ask"]), (json!(false), json!(true)));

    // One open ask per row.
    let (st, v) = geek(&base, "POST", &agent_board_row_ask(&id, &row), Some(question())).await;
    assert_eq!(st, 409, "{v}");
    assert!(v["error"].as_str().unwrap().starts_with("this row already asks"), "{v}");

    // The answer is one of the options.
    let (st, v) = call(&base, "x", "POST", &board_ask_answer(&id, &ask_id), Some(json!({ "option": "Maybe" }))).await;
    assert_eq!(st, 400, "{v}");
    let (st, v) = call(&base, "x", "POST", &board_ask_answer(&id, &ask_id), Some(json!({ "text": "Maybe" }))).await;
    assert_eq!(st, 400, "an ask with options takes one of them: {v}");
    let (st, v) = call(&base, "x", "POST", &board_ask_answer(&id, &ask_id), Some(json!({ "option": "Ship it" }))).await;
    assert_eq!(st, 200, "{v}");
    let r = row_in(&v, &row);
    assert!(r.get("ask").is_none(), "answering clears the ask: {r}");
    assert_eq!(r["status"], json!("none"));
    assert_eq!(r["asked"][0]["answer"], json!("Ship it"), "kept in the history: {r}");
    assert_eq!(r["asked"][0]["question"], json!("Ship the parser with the old grammar?"));

    // The agent reads it from the board, and the first answer stands.
    let (st, v) = geek(&base, "GET", &format!("/api/daemon/boards/{id}"), None).await;
    assert_eq!((st, row_in(&v, &row)["asked"][0]["answer"].clone()), (200, json!("Ship it")), "{v}");
    let (st, v) = call(&base, "x", "POST", &board_ask_answer(&id, &ask_id), Some(json!({ "option": "Wait" }))).await;
    assert_eq!(st, 409, "{v}");

    // The row can ask again; free text takes text, at most three are kept.
    for n in 0..4 {
        let free = json!({ "who": "b", "question": format!("Which name, take {n}?") });
        let (st, v) = geek(&base, "POST", &agent_board_row_ask(&id, &row), Some(free)).await;
        assert_eq!(st, 200, "{v}");
        let ask_id = row_in(&v, &row)["ask"]["id"].as_str().unwrap().to_owned();
        let (st, v) = call(&base, "b", "POST", &board_ask_answer(&id, &ask_id), Some(json!({ "option": "x" }))).await;
        assert_eq!(st, 400, "free text takes text: {v}");
        let (st, v) =
            call(&base, "b", "POST", &board_ask_answer(&id, &ask_id), Some(json!({ "text": "  Roux " }))).await;
        assert_eq!(st, 200, "{v}");
        assert_eq!(row_in(&v, &row)["asked"][0]["answer"], json!("Roux"));
    }
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(row_in(&v, &row)["asked"].as_array().unwrap().len(), 3, "the last three: {v}");
}

/// Only the person an ask names answers it, or an owner or editor for them:
/// a viewer it doesn't name gets 403, a machine 401, an agent has no route,
/// and a stranger finds no board.
#[tokio::test]
async fn only_a_person_answers() {
    let (base, _app, id, row, _) = world().await;
    let (_, v) = geek(
        &base,
        "POST",
        &agent_board_row_ask(&id, &row),
        Some(json!({
        "who": "b", "question": "Rename it?", "options": ["Yes", "No"] })),
    )
    .await;
    let ask_id = row_in(&v, &row)["ask"]["id"].as_str().unwrap().to_owned();
    let answer = board_ask_answer(&id, &ask_id);
    let yes = json!({ "option": "Yes" });

    // x is a viewer the ask doesn't name: it isn't theirs, and they can't answer it.
    let (_, v) = call(&base, "x", "GET", &format!("/api/boards/{id}"), None).await;
    let ask = &row_in(&v, &row)["ask"];
    assert_eq!((ask["mine"].clone(), ask["can_answer"].clone()), (json!(false), json!(false)), "{ask}");
    // A viewer it doesn't name; a machine, even its owner's own.
    let (st, v) = call(&base, "x", "POST", &answer, Some(yes.clone())).await;
    assert_eq!(st, 403, "{v}");
    let (st, v) = geek(&base, "POST", &answer, Some(yes.clone())).await;
    assert_eq!(st, 401, "{v}");
    // An agent has no route to answer.
    let (st, _) =
        geek(&base, "POST", &format!("/api/daemon/boards/{id}/asks/{ask_id}/answer"), Some(yes.clone())).await;
    assert!(st == 404 || st == 405, "{st}");
    // A stranger finds no board.
    let (st, _) = call(&base, "c", "POST", &answer, Some(yes.clone())).await;
    assert_eq!(st, 404);
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert!(row_in(&v, &row).get("asked").is_none() && row_in(&v, &row)["ask"]["id"] == json!(ask_id), "{v}");

    // An owner answers for them, and it says who gave the answer.
    let (st, v) = call(&base, "a", "POST", &answer, Some(yes)).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(row_in(&v, &row)["asked"][0]["answer"], json!("Yes"));
}

/// An ask is for a member, on a row with an issue or a PR, from someone who
/// may change the board, in one line of at most 200 characters.
#[tokio::test]
async fn an_ask_has_limits() {
    let (base, app, id, row, note) = world().await;
    let ask = |who: &str, body: Value| {
        let (base, path, who) = (base.clone(), board_row_ask(&id, &row), who.to_owned());
        async move { call(&base, &who, "POST", &path, Some(body)).await }
    };

    let (st, v) = call(&base, "a", "POST", &board_row_ask(&id, &note), Some(question())).await;
    assert_eq!(st, 422, "{v}");
    assert_eq!(v["error"], json!("a note row has nothing to act on: ask on the issue's row"));
    let (st, v) = call(&base, "a", "POST", &board_row_ask(&id, "row_nope"), Some(question())).await;
    assert_eq!(st, 404, "{v}");
    assert_eq!(ask("x", question()).await.0, 403, "a viewer asks nothing");
    assert_eq!(ask("c", question()).await.0, 404, "a stranger finds no board");
    for (why, body) in [
        ("not a member", json!({ "who": "c", "question": "q?" })),
        ("no one", json!({ "who": "", "question": "q?" })),
        ("empty", json!({ "who": "b", "question": "  " })),
        ("two lines", json!({ "who": "b", "question": "a\nb" })),
        ("too long", json!({ "who": "b", "question": "q".repeat(201) })),
        ("five options", json!({ "who": "b", "question": "q?", "options": ["1", "2", "3", "4", "5"] })),
        ("a long option", json!({ "who": "b", "question": "q?", "options": ["o".repeat(81)] })),
        ("twice", json!({ "who": "b", "question": "q?", "options": ["same", "same"] })),
        ("recommend off the list", json!({ "who": "b", "question": "q?", "options": ["1"], "recommend": "2" })),
        ("a long why", json!({ "who": "b", "question": "q?", "why": "w".repeat(2001) })),
    ] {
        let (st, v) = ask("a", body).await;
        assert_eq!(st, 400, "{why}: {v}");
    }
    let (st, v) = ask("a", json!({ "who": "b", "question": "q".repeat(200), "options": ["o".repeat(80)] })).await;
    assert_eq!(st, 200, "the limits themselves: {v}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(row_in(&v, &row)["ask"]["why"], Value::Null, "{v}");

    // Deleting the row takes its asks.
    let (st, _) = call(&base, "a", "DELETE", &format!("/api/boards/{id}/entries/{row}"), None).await;
    assert_eq!(st, 200);
    let left: i64 = app
        .db
        .c()
        .query_row("SELECT count(*) FROM board_asks WHERE board_id = ?1", crate::db::sql::params![id], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0, "the row's asks went with it");
}

/// An agent block that reports `needs_input` raises an ask on its row, for
/// whoever reads it, with no answer; it clears when the block works again,
/// and an asked question comes before it.
#[tokio::test]
async fn the_board_raises_an_ask_for_a_waiting_agent() {
    let (base, _app, id, row, _) = world().await;
    let block = json!({ "kind": "block", "machine": "geek", "pane": "7" });
    let (st, v) = call(
        &base,
        "a",
        "POST",
        &format!("/api/boards/{id}/attachments"),
        Some(json!({ "row": row, "role": "work", "pointer": block })),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    assert!(row_in(&v, &row).get("ask").is_none(), "nothing reported yet: {v}");

    let report = |state: &str| json!({ "pane": "7", "block": { "state": state } });
    let (st, v) = geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report("needs_input"))).await;
    assert_eq!(st, 200, "{v}");
    for who in ["a", "x"] {
        let r = get(&base, who, &id).await;
        let ask = &row_in(&r, &row)["ask"];
        assert_eq!(
            (ask["raised"].clone(), ask["mine"].clone(), ask["can_answer"].clone()),
            (json!(true), json!(true), json!(false)),
            "{who}: {r}"
        );
        assert!(ask["question"].as_str().unwrap().ends_with("agent is waiting for you"), "{ask}");
        assert_eq!(ask["id"], json!(""), "a raised ask has no id to answer");
    }

    // A question asked on the row comes first.
    let (st, v) = call(&base, "a", "POST", &board_row_ask(&id, &row), Some(question())).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(row_in(&v, &row)["ask"]["raised"], json!(false), "{v}");

    // It clears with its cause.
    let (st, _) = geek(&base, "PUT", AGENT_BLOCK_STATE, Some(report("working"))).await;
    assert_eq!(st, 200);
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(row_in(&v, &row)["ask"]["question"], json!("Ship the parser with the old grammar?"));
    let ask_id = row_in(&v, &row)["ask"]["id"].as_str().unwrap().to_owned();
    call(&base, "x", "POST", &board_ask_answer(&id, &ask_id), Some(json!({ "option": "Wait" }))).await;
    let (_, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert!(row_in(&v, &row).get("ask").is_none(), "no cause, no ask: {v}");

    // Answering a raised one: there is no ask to name.
    let (st, _) = call(&base, "a", "POST", &board_ask_answer(&id, ""), Some(json!({ "text": "x" }))).await;
    assert!(st == 400 || st == 404, "{st}");
}
