//! An agent's relations (Arugula #403) through the platform's routes: it
//! needs `issues.write` over the project, the edge says an agent made it,
//! and it takes off only the ones its own machine made.

use serde_json::{Value, json};

use super::FLAG;
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
    let (hub, _app, base, t, _u) = seeded().await;
    hub.flag_named("team:t", FLAG, true);
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
