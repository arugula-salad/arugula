//! MCP's issue routes (M200): a machine's agents read and write native
//! issues signed as the machine, through its owner's grants (M197) and
//! never past its owner's role; what they write lands at once, labelled.

use arugula_control_wire::{
    chat::agent_messages,
    issues::{AGENT_ISSUE_HOME, agent_issue, agent_issue_events, agent_project_issue, agent_project_issues},
};
use serde_json::{Value, json};

use crate::{
    agents_tests::{give, seeded},
    chat_tests::call,
    db::{
        projects::{ISSUES_WRITE, THREADS_POST, THREADS_READ},
        sql::{Sql as _, params},
    },
    grants::AGENT_UNGRANTED,
};

/// A call signed by `who`'s machine `machine`, as the hub's gate vouches
/// for it.
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

/// Refused without a grant, saying how to give it; then filed, read,
/// commented on, labelled, assigned, edited and closed at once, as a's
/// agent on geek-box; the next call after a revoke is refused.
#[tokio::test]
async fn an_agent_files_comments_on_and_closes_an_issue_where_granted() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let issues = agent_project_issues(&t);
    let new = json!({ "title": "the canary fails", "body": "since 9:00", "labels": ["bug"] });

    let (st, v) = geek(&base, "POST", &issues, Some(new.clone())).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    let how = format!("arugula grants add geek-box issues.write --project {t}");
    assert!(v["error"].as_str().unwrap().contains(&how), "{v}");
    let (st, v) = geek(&base, "GET", &issues, None).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    assert!(v["error"].as_str().unwrap().contains("threads.read"), "{v}");

    let mut grants = vec![];
    for power in [ISSUES_WRITE, THREADS_READ, THREADS_POST] {
        let (st, g) =
            call(&base, "a", "POST", "/api/grants", give("geek", power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{g}");
        grants.push(g["id"].as_str().unwrap().to_owned());
    }
    let (st, made) = geek(&base, "POST", &issues, Some(new)).await;
    assert_eq!(st, 200, "{made}");
    assert_eq!((&made["number"], &made["author"]), (&json!(1), &json!("a")));
    assert_eq!(made["agent"], json!({ "machine": "geek", "machine_name": "geek-box" }));
    let id = made["id"].as_str().unwrap().to_owned();

    // Read back by its id and by its number; listed and searched.
    let (st, v) = geek(&base, "GET", &agent_issue(&id), None).await;
    assert_eq!((st, &v["title"]), (200, &json!("the canary fails")), "{v}");
    let (_, v) = geek(&base, "GET", &agent_project_issue(&t, 1), None).await;
    assert_eq!(v["id"], json!(id));
    let (_, v) = geek(&base, "GET", &format!("{issues}?q=canary"), None).await;
    assert_eq!(v["issues"][0]["id"], json!(id), "{v}");

    // A comment is a post to its thread, by the issue's id.
    let (st, v) = geek(&base, "POST", &agent_messages(&id), Some(json!({ "text": "bisected it" }))).await;
    assert_eq!(st, 200, "{v}");

    // Label, assign, edit and close, each at once.
    let patch =
        json!({ "labels": { "add": ["infra"] }, "assignees": { "set": ["b"] }, "body": "since 9:00, every run" });
    let (st, v) = geek(&base, "PATCH", &agent_issue(&id), Some(patch)).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!((&v["issue"]["labels"], &v["issue"]["assignees"]), (&json!(["bug", "infra"]), &json!(["b"])));
    let (st, v) =
        geek(&base, "PATCH", &agent_issue(&id), Some(json!({ "state": "closed", "reason": "completed" }))).await;
    assert_eq!((st, &v["issue"]["state"]), (200, &json!("closed")), "{v}");
    let (_, v) = geek(&base, "GET", &agent_issue_events(&id), None).await;
    let kinds: Vec<&str> = v["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["opened", "edited", "labelled", "assigned", "closed"], "{v}");
    assert!(v["events"].as_array().unwrap().iter().all(|e| e["agent"]["machine"] == "geek"), "{v}");

    // A person sees it all as a's agent's.
    let (_, v) = call(&base, "b", "GET", &format!("/api/issues/{id}"), None).await;
    assert_eq!((&v["state"], &v["agent"]["machine_name"]), (&json!("closed"), &json!("geek-box")), "{v}");

    // issues.write revoked: the next write is refused, reads go on.
    let revoke = format!("/api/grants/{}/revoke", grants[0]);
    assert_eq!(call(&base, "a", "POST", &revoke, None).await.0, 200);
    let (st, v) = geek(&base, "PATCH", &agent_issue(&id), Some(json!({ "state": "open" }))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    assert_eq!(geek(&base, "GET", &agent_issue(&id), None).await.0, 200);
}

/// The owner's role is the ceiling: a viewer's machine, granted
/// issues.write, reads and comments but never files or closes. Someone
/// else's project is 404, as for a person.
#[tokio::test]
async fn an_agent_never_writes_past_its_owners_role() {
    let (hub, app, base, t, u) = seeded().await;
    hub.daemon("xbox", "x", Some("t"));
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    for power in [ISSUES_WRITE, THREADS_READ, THREADS_POST] {
        let (st, g) =
            call(&base, "x", "POST", "/api/grants", give("xbox", power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{g}");
    }
    let x = |method: &'static str, path: String, body: Option<Value>| {
        let base = base.clone();
        async move { signed(&base, "x xbox", method, &path, body).await }
    };
    let (st, v) = x("POST", agent_project_issues(&t), Some(json!({ "title": "from a viewer's agent" }))).await;
    assert_eq!(st, 403, "{v}");
    assert!(v["error"].as_str().unwrap().contains("viewer"), "{v}");

    let (st, made) =
        call(&base, "a", "POST", &format!("/api/projects/{t}/issues"), Some(json!({ "title": "a's" }))).await;
    assert_eq!(st, 200, "{made}");
    let id = made["id"].as_str().unwrap();
    assert_eq!(x("GET", agent_issue(id), None).await.0, 200);
    let (st, v) = x("POST", agent_messages(id), Some(json!({ "text": "seen it too" }))).await;
    assert_eq!(st, 200, "a viewer comments: {v}");
    let (st, _) = x("PATCH", agent_issue(id), Some(json!({ "state": "closed" }))).await;
    assert_eq!(st, 403);

    // u is c's team: not there for a's machine, granted or not.
    let (st, _) = geek(&base, "GET", &agent_project_issues(&u), None).await;
    assert_eq!(st, 404);
}

/// The project an agent's `#N` means: one it names, the one its clone's
/// repo is linked to, else its owner's personal project; a repo linked to
/// nobody's project of the owner's is refused, not guessed.
#[tokio::test]
async fn an_agents_current_project() {
    let (_hub, app, base, t, u) = seeded().await;
    for scope in [json!({ "kind": "project", "id": t }), json!({ "kind": "project", "id": u })] {
        call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, scope)).await;
    }
    let personal = crate::projects::personal_project(&app, "a").await.unwrap().unwrap();
    call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, json!({ "kind": "project", "id": personal })))
        .await;
    for (repo, project) in [("acme/app", &t), ("other/thing", &u)] {
        app.db
            .c()
            .execute(
                "INSERT INTO linked_repos (repo_id, full_name, project_id, installation_id, linked_at) VALUES (?1, ?2, ?3, 1, 1)",
                params![repo.len() as i64, repo, project],
            )
            .unwrap();
    }
    let (st, h) = geek(&base, "GET", &format!("{AGENT_ISSUE_HOME}?repo=Acme/App"), None).await;
    assert_eq!((st, &h["project"], &h["home"]), (200, &json!(t), &json!("github")), "{h}");
    let (st, h) = geek(&base, "GET", AGENT_ISSUE_HOME, None).await;
    assert_eq!((st, &h["project"], &h["home"]), (200, &json!(personal), &json!("native")), "{h}");
    assert!(h["name"].as_str().is_some_and(|n| !n.is_empty()), "the project's name comes with its id: {h}");
    let (_, h) = geek(&base, "GET", &format!("{AGENT_ISSUE_HOME}?project={t}&repo=nobody/here"), None).await;
    assert_eq!(h["project"], json!(t), "a named project wins");
    for repo in ["other/thing", "nobody/here"] {
        let (st, v) = geek(&base, "GET", &format!("{AGENT_ISSUE_HOME}?repo={repo}"), None).await;
        assert_eq!(st, 404, "{repo}: {v}");
        assert!(v["error"].as_str().unwrap().contains("name the project"), "{v}");
    }
}

/// A machine's agents write at most [`AGENT_ISSUE_WRITES`] an hour: past
/// it a new issue or a change is refused (429), and reads still answer.
#[tokio::test]
async fn an_agents_writes_are_rate_limited() {
    use crate::issues::agent::AGENT_ISSUE_WRITES;
    let (_hub, app, base, t, _u) = seeded().await;
    for power in [ISSUES_WRITE, THREADS_READ] {
        let (st, g) =
            call(&base, "a", "POST", "/api/grants", give("geek", power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{g}");
    }
    let issues = agent_project_issues(&t);
    let (st, made) = geek(&base, "POST", &issues, Some(json!({ "title": "one" }))).await;
    assert_eq!(st, 200, "{made}");
    let id = made["id"].as_str().unwrap().to_owned();
    // The rest of the hour's writes, used up.
    while app.limits.check_daemon(AGENT_ISSUE_WRITES, "geek").is_ok() {}
    assert_eq!(geek(&base, "POST", &issues, Some(json!({ "title": "two" }))).await.0, 429);
    let close = Some(json!({ "state": "closed" }));
    assert_eq!(geek(&base, "PATCH", &agent_issue(&id), close).await.0, 429);
    assert_eq!(geek(&base, "GET", &agent_issue(&id), None).await.0, 200, "reads still answer");
}
