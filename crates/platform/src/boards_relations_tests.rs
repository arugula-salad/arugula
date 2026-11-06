//! An agent's relations (Arugula #403) through the platform's routes: it
//! needs `issues.write` over the project, the edge says an agent made it,
//! and it takes off only the ones its own machine made.

use serde_json::{Value, json};

use super::tests::issue;
use crate::{
    agents_tests::{give, seeded},
    chat_tests::call,
};
use arugula_control_wire::boards::{agent_project_relation, agent_project_relations, project_relations};

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

#[tokio::test]
async fn an_agent_relates_with_issues_write_and_takes_off_only_its_own() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let (a, b) = (issue(&base, "a", &t, "Design").await, issue(&base, "a", &t, "Build").await);
    let body = json!({ "from": a, "to": b, "kind": "blocks" });

    // Without the grant: refused, and nothing is written.
    let (st, v) = geek(&base, "POST", &agent_project_relations(&t), Some(body.clone())).await;
    assert_eq!(st, 403, "{v}");
    // A board grant is not enough: the items' own power is issues.write.
    let (st, _) =
        call(&base, "a", "POST", "/api/grants", give("geek", "boards.write", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200);
    let (st, v) = geek(&base, "POST", &agent_project_relations(&t), Some(body.clone())).await;
    assert_eq!(st, 403, "{v}");

    let (st, g) =
        call(&base, "a", "POST", "/api/grants", give("geek", "issues.write", json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");
    let (st, v) = geek(&base, "POST", &agent_project_relations(&t), Some(body)).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["source"], json!("agent"));
    let theirs = v["id"].as_str().unwrap().to_owned();

    // A cycle is refused for an agent as for anyone.
    let back = json!({ "from": b, "to": a, "kind": "blocks" });
    let (st, v) = geek(&base, "POST", &agent_project_relations(&t), Some(back)).await;
    assert_eq!(st, 409, "{v}");

    // A person's edge isn't the agent's to take off; its own is.
    let (st, mine) =
        call(&base, "b", "POST", &project_relations(&t), Some(json!({ "from": a, "to": b, "kind": "relates" }))).await;
    assert_eq!(st, 200, "{mine}");
    let (st, v) = geek(&base, "DELETE", &agent_project_relation(&t, mine["id"].as_str().unwrap()), None).await;
    assert_eq!(st, 403, "{v}");
    let (st, v) = geek(&base, "DELETE", &agent_project_relation(&t, &theirs), None).await;
    assert_eq!(st, 200, "{v}");
    // And a person may take the agent's off.
    let (st, again) =
        geek(&base, "POST", &agent_project_relations(&t), Some(json!({ "from": a, "to": b, "kind": "blocks" }))).await;
    assert_eq!(st, 200, "{again}");
    let path = arugula_control_wire::boards::project_relation(&t, again["id"].as_str().unwrap());
    let (st, v) = call(&base, "b", "DELETE", &path, None).await;
    assert_eq!(st, 200, "{v}");
}

/// A board's steps come from its open `blocks` edges, and a loop Plane made
/// (an edge the routes refuse) reads without end: its rows share a step and
/// are marked (Arugula #762).
#[tokio::test]
async fn a_boards_rows_have_steps_and_a_loop_reads_without_end() {
    use crate::db::sql::{Sql as _, params};
    let (_hub, app, base, t, _u) = seeded().await;
    let [a, b, c, d, e] = [
        issue(&base, "a", &t, "Design").await,
        issue(&base, "a", &t, "Build").await,
        issue(&base, "a", &t, "Ship").await,
        issue(&base, "a", &t, "Chicken").await,
        issue(&base, "a", &t, "Egg").await,
    ];
    for (from, to) in [(&a, &b), (&b, &c), (&d, &e)] {
        let n = json!({ "from": from, "to": to, "kind": "blocks" });
        let (st, v) = call(&base, "a", "POST", &project_relations(&t), Some(n)).await;
        assert_eq!(st, 200, "{v}");
    }
    // The route refuses `e` blocking `d`; the Plane sync doesn't check.
    app.db
        .c()
        .execute(
            "INSERT INTO work_relations (id, project_id, from_kind, from_id, to_kind, to_id, kind, source,
                 created_by, created_at)
             VALUES ('rel_loop', ?1, 'issue', ?2, 'issue', ?3, 'blocks', 'plane', '', 1)",
            params![t, e, d],
        )
        .unwrap();
    let (st, board) =
        call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "L" }))).await;
    assert_eq!(st, 200, "{board}");
    let id = board["board"]["id"].as_str().unwrap().to_owned();
    let mut rows = vec![];
    for item in [&a, &b, &c, &d, &e] {
        let (st, v) = super::tests::put(
            &base,
            "a",
            &id,
            json!({ "entry": "row", "note": "", "pointer": { "kind": "issue", "id": item } }),
        )
        .await;
        assert_eq!(st, 200, "{v}");
        rows.push(v["zones"][0]["rows"].as_array().unwrap().last().unwrap()["id"].as_str().unwrap().to_owned());
    }
    // An item on another board blocks Ship: one step, and the mark names that board.
    let far = issue(&base, "a", &t, "Elsewhere").await;
    let n = json!({ "from": far, "to": c, "kind": "blocks" });
    let (st, v) = call(&base, "a", "POST", &project_relations(&t), Some(n)).await;
    assert_eq!(st, 200, "{v}");
    let (st, other) =
        call(&base, "a", "POST", &format!("/api/projects/{t}/boards"), Some(json!({ "title": "Other" }))).await;
    assert_eq!(st, 200, "{other}");
    let entry = json!({ "entry": "row", "note": "", "pointer": { "kind": "issue", "id": far } });
    let (st, v) = super::tests::put(&base, "a", other["board"]["id"].as_str().unwrap(), entry).await;
    assert_eq!(st, 200, "{v}");
    let (st, v) = call(&base, "a", "GET", &format!("/api/boards/{id}"), None).await;
    assert_eq!(st, 200, "{v}");
    let all: Vec<&Value> = v["zones"].as_array().unwrap().iter().flat_map(|z| z["rows"].as_array().unwrap()).collect();
    let of = |row: &str| all.iter().find(|r| r["id"] == json!(row)).unwrap();
    let steps: Vec<_> = rows.iter().map(|r| of(r)["step"].clone()).collect();
    assert_eq!(steps, [json!(1), json!(2), json!(3), json!(1), json!(1)], "{v}");
    let looped = |row: &str| of(row)["marks"].as_array().is_some_and(|m| m.contains(&json!({ "kind": "cycle" })));
    assert_eq!(rows.iter().map(|r| looped(r)).collect::<Vec<_>>(), [false, false, false, true, true]);
    assert_eq!(v["chain"], json!([rows[0], rows[1], rows[2]]), "{v}");
    let marks = of(&rows[2])["marks"].as_array().unwrap().clone();
    assert!(marks.iter().any(|m| m["kind"] == json!("blocked_by") && m["board"] == json!("Other")), "{marks:?}");
}
