//! The platform's MCP endpoint for agents with no machine (M230): an
//! agent with only a bearer agent grant token files a native issue, starts
//! a run and plans a deploy, each as its owner's agent and labelled so, and
//! is refused an approval wherever it asks for one; the token is minted
//! over what its owner reads, shown once, kept only as a hash, listed with
//! the calls its agent made, and revoked; and it opens no other route.

use std::sync::Arc;

use arugula_e2e::{deploy::Decision, team::TeamRole};
use serde_json::{Value, json};

use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::{Db, agent_tokens::hash, sql::Sql as _, sql::params},
    deploys_tests::{devices, signed},
};

/// Team t (a owns it, b edits, x views) with chat on and a hosted
/// workspace `dev` (on the hosted beta's list: a, b, x); team u is c's
/// alone. The platform keeps secrets. Its routes, t's project and u's.
async fn platform() -> (Arc<TestHub>, Arc<Platform>, String, String, String) {
    let hub = Arc::new(TestHub::default());
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    hub.team("u", &[("c", TeamRole::Owner)]);
    hub.flag("team:t", true);
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    p.hosted = crate::hosted::Config { accounts: vec!["a".into(), "b".into(), "x".into()], ..Default::default() };
    p.secret_key = Some(Arc::new(crate::secrets::SecretCipher::new(&"07".repeat(32)).unwrap()));
    let app = Arc::new(p);
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let t = crate::projects::team_project(&app, "t").await.unwrap().unwrap();
    let u = crate::projects::team_project(&app, "u").await.unwrap().unwrap();
    app.db
        .c()
        .execute(
            "INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, machine, created_at, updated_at)
             VALUES ('hw_a', ?1, 'dev', '{}', 'h', 'ready', 'dev_a', 1, 1)",
            params![t],
        )
        .unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (hub, app, base, t, u)
}

/// `who` mints a token named `name` with `powers` over `projects`: the
/// answer (with the token, once).
async fn mint(base: &str, who: &str, name: &str, projects: &[&str], powers: &[&str]) -> (u16, Value) {
    call(base, who, "POST", "/api/agent-tokens", Some(json!({ "name": name, "projects": projects, "powers": powers })))
        .await
}

/// A request with only a bearer token, as an agent with no machine sends.
async fn bearer(base: &str, token: &str, method: &str, path: &str, body: Option<Value>) -> reqwest::Response {
    let mut req = reqwest::Client::new()
        .request(method.parse().unwrap(), format!("{base}{path}"))
        .header("authorization", format!("Bearer {token}"));
    if let Some(b) = body {
        req = req.json(&b);
    }
    req.send().await.unwrap()
}

/// One JSON-RPC request to the endpoint: its status and answer.
async fn rpc(base: &str, token: &str, method: &str, params: Value) -> (u16, Value) {
    let r = bearer(
        base,
        token,
        "POST",
        "/api/mcp",
        Some(json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params })),
    )
    .await;
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

/// A tool's result.
async fn tool(base: &str, token: &str, name: &str, args: Value) -> Value {
    let (st, v) = rpc(base, token, "tools/call", json!({ "name": name, "arguments": args })).await;
    assert_eq!((st, &v["id"]), (200, &json!(7)), "{v}");
    v["result"].clone()
}

fn ok(r: &Value) -> &Value {
    assert_eq!(r["isError"], json!(false), "{r}");
    &r["structuredContent"]
}

fn refused(r: &Value) -> String {
    assert_eq!(r["isError"], json!(true), "{r}");
    r["content"][0]["text"].as_str().unwrap().to_owned()
}

const ALL: [&str; 8] = [
    "threads.read",
    "threads.post",
    "issues.write",
    "deploys.plan",
    "runs.read",
    "runs.start",
    "runs.command",
    "secrets.write",
];

/// t's environment `production` (the chant-fly driver), made by a.
async fn production(base: &str, t: &str) -> String {
    let (st, env) = call(
        base,
        "a",
        "POST",
        &format!("/api/projects/{t}/environments"),
        Some(json!({
            "name": "production", "driver": "chant-fly", "secret_names": ["FLY_API_TOKEN", "GITHUB_TOKEN"],
            "settings": { "fly_app": "acme-app", "chant_env": "github" },
        })),
    )
    .await;
    assert_eq!(st, 200, "{env}");
    env["id"].as_str().unwrap().to_owned()
}

/// The done-when: an agent with only a token files a native issue, starts
/// a run and plans a deploy, each its owner's and labelled as their agent
/// on the token; sets a project secret it can't read back; and is refused
/// an approval by every route and tool there is for one, the owner's own
/// signature in hand. Every call is in its owner's list.
#[tokio::test]
async fn an_agent_with_only_a_token_files_runs_plans_and_never_approves() {
    let (hub, app, base, t, _) = platform().await;
    let a = devices(&hub, "a");
    production(&base, &t).await;

    let (st, minted) = mint(&base, "a", "redone", &[&t], &ALL).await;
    assert_eq!(st, 200, "{minted}");
    let token = minted["token"].as_str().unwrap().to_owned();
    let id = minted["agent_token"]["id"].as_str().unwrap().to_owned();
    assert!(token.starts_with("aru_agent_") && id.starts_with("agt_"), "{minted}");
    assert_eq!(minted["mcp_url"], json!("https://control.example/api/mcp"));
    assert_eq!(minted["agent_token"]["grants"].as_array().unwrap().len(), ALL.len());
    // Kept only as its hash; never listed again.
    let kept: String =
        app.db.c().query_row("SELECT token_hash FROM agent_tokens WHERE id = ?1", params![id], |r| r.get(0)).unwrap();
    assert_eq!(kept, hash(&token));
    let (_, listed) = call(&base, "a", "GET", "/api/agent-tokens", None).await;
    assert!(!listed.to_string().contains(&token), "{listed}");
    assert_eq!(listed["tokens"][0]["id"], json!(id));
    // Its grants aren't a machine's.
    let (_, grants) = call(&base, "a", "GET", "/api/grants", None).await;
    assert!(!grants.to_string().contains(&id), "{grants}");

    let (st, init) = rpc(&base, &token, "initialize", json!({ "protocolVersion": "2025-06-18" })).await;
    assert_eq!((st, &init["result"]["protocolVersion"]), (200, &json!("2025-06-18")), "{init}");
    let (_, listed) = rpc(&base, &token, "tools/list", json!({})).await;
    let names: Vec<&str> =
        listed["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in [
        "issue_write",
        "post_thread",
        "read_thread",
        "start_run",
        "read_run",
        "wait",
        "plan_deploy",
        "read_deploy",
        "set_secret",
    ] {
        assert!(names.contains(&n), "{n} in {names:?}");
    }
    assert!(!names.iter().any(|n| n.contains("approv") || n.contains("deny")), "{names:?}");

    // A native issue, as a's, labelled with the token.
    let r = tool(&base, &token, "issue_write", json!({ "kind": "create", "title": "From redone", "body": "hi" })).await;
    let issue = ok(&r).clone();
    assert_eq!((&issue["author"], &issue["agent"]["machine"]), (&json!("a"), &json!(id)), "{issue}");
    assert_eq!(issue["agent"]["machine_name"], json!("redone (MCP)"));
    let iss = issue["id"].as_str().unwrap().to_owned();
    let (st, seen) = call(&base, "b", "GET", &format!("/api/issues/{iss}"), None).await;
    assert_eq!((st, &seen["agent"]["machine_name"]), (200, &json!("redone (MCP)")), "{seen}");
    let r =
        tool(&base, &token, "issue_write", json!({ "kind": "comment", "issue": "#1", "text": "and a comment" })).await;
    ok(&r);
    let r = tool(&base, &token, "read_thread", json!({ "room": iss })).await;
    assert!(r["content"][0]["text"].as_str().unwrap().contains("a's agent on redone (MCP): and a comment"), "{r}");

    // A run, as a's start, by the token's agent: pending, with no standing
    // grant to start it.
    let r = tool(&base, &token, "start_run", json!({ "command": ["just", "test"], "box": "dev" })).await;
    let run = ok(&r)["run"].clone();
    assert_eq!(
        (&run["state"], &run["started_by"], &run["started_by_agent"]),
        (&json!("pending"), &json!("a"), &json!(id)),
        "{run}"
    );
    let r = tool(&base, &token, "read_run", json!({ "run": run["id"] })).await;
    assert_eq!(ok(&r)["run"]["id"], run["id"]);

    // A deploy, planned and asked for; a gets the card.
    let sha = "ab".repeat(20);
    let r =
        tool(&base, &token, "plan_deploy", json!({ "environment": "production", "commit": sha, "repo": "acme/app" }))
            .await;
    let dep = ok(&r)["deploy"].clone();
    assert_eq!(
        (&dep["state"], &dep["planned_by"], &dep["may_approve"]),
        (&json!("pending"), &json!("a"), &json!(false)),
        "{dep}"
    );
    assert_eq!(dep["agent"]["machine_name"], json!("redone (MCP)"));
    let dep_id = dep["id"].as_str().unwrap().to_owned();
    let op = dep["operation"]["hash"].as_str().unwrap().to_owned();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        hub.pushes().iter().any(|(to, m, _)| to == "a"
            && m["deploy"] == json!(dep_id)
            && m["body"].as_str().unwrap().contains("an agent on redone (MCP)")),
        "{:?}",
        hub.pushes()
    );

    // Never an approval: no tool, whatever it's called...
    for name in ["approve_deploy", "deny_deploy", "approve", "sign_grant"] {
        let why = refused(&tool(&base, &token, name, json!({ "id": dep_id })).await);
        assert!(why.contains("never approve"), "{name}: {why}");
    }
    // ...and no route, even with the owner's own signature and passkey.
    let good = signed(&a.phone, "a", &dep_id, &op, Decision::Once);
    for (path, body) in [
        (format!("/api/deploys/{dep_id}/approve"), Some(good.clone())),
        (format!("/api/deploys/{dep_id}/deny"), Some(signed(&a.phone, "a", &dep_id, &op, Decision::Deny))),
        (format!("/api/daemon/deploys/{dep_id}/approve"), Some(good.clone())),
        (format!("/api/projects/{t}/runs/grant"), Some(json!({}))),
        (format!("/api/projects/{t}/runs/grant/revoke"), Some(json!({}))),
        (format!("/api/projects/{t}/runs/start"), Some(json!({}))),
    ] {
        let st = bearer(&base, &token, "POST", &path, body).await.status().as_u16();
        assert!(matches!(st, 401 | 403 | 404 | 405), "{path}: {st}");
    }
    assert!(app.db.deploy_approvals(&dep_id).unwrap().is_empty());
    let (_, d) = call(&base, "a", "GET", &format!("/api/deploys/{dep_id}"), None).await;
    assert_eq!(d["state"], json!("pending"), "{d}");
    // The owner approves it, from their phone.
    let (st, d) = call(&base, "a", "POST", &format!("/api/deploys/{dep_id}/approve"), Some(good)).await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "{d}");

    // A project secret, set by name and never read back.
    let secret = "s3cr3t-value-m230";
    let r = tool(&base, &token, "set_secret", json!({ "name": "REDONE_TOKEN", "value": secret })).await;
    assert_eq!(ok(&r)["set"], json!(true));
    let master = app.secret_key.clone().unwrap();
    assert_eq!(crate::keys::open(&app.db, &master, &t, "project/REDONE_TOKEN").unwrap().as_deref(), Some(secret));
    let why = refused(&tool(&base, &token, "set_secret", json!({ "name": "ARUGULA_KEY", "value": "x" })).await);
    assert!(why.contains("reserved"), "{why}");
    assert!(!names.iter().any(|n| n.contains("secret") && *n != "set_secret"), "{names:?}");

    // Every call, as a reads them: never a value.
    let (st, calls) = call(&base, "a", "GET", &format!("/api/agent-tokens/{id}/calls?limit=200"), None).await;
    assert_eq!(st, 200, "{calls}");
    let calls = calls["calls"].as_array().unwrap();
    let of = |tool: &str| {
        calls.iter().filter(|c| c["tool"] == tool).map(|c| c["outcome"].as_str().unwrap()).collect::<Vec<_>>()
    };
    assert_eq!(of("issue_write"), ["ok", "ok"]);
    assert_eq!(of("start_run"), ["ok"]);
    assert_eq!(of("plan_deploy"), ["ok"]);
    assert_eq!(of("approve_deploy"), ["refused"]);
    assert!(calls.iter().any(|c| c["detail"] == json!("set secret REDONE_TOKEN") && c["project"] == json!(t)));
    assert!(!serde_json::to_string(calls).unwrap().contains(secret));
    // b and c don't read a's token's calls.
    assert_eq!(call(&base, "b", "GET", &format!("/api/agent-tokens/{id}/calls"), None).await.0, 404);
}

/// A token is minted only over projects its minter reads, with an
/// agent's powers (never cloud.drive); its agent never does more than its
/// grants or its owner's role allow, and a revoke refuses the very next
/// call.
#[tokio::test]
async fn a_token_reaches_its_grants_and_its_owner_and_is_revoked() {
    let (_hub, _app, base, t, u) = platform().await;
    assert_eq!(mint(&base, "a", "x", &[&u], &["issues.write"]).await.0, 403, "u isn't a's");
    assert_eq!(mint(&base, "a", "x", &[&t], &["cloud.drive"]).await.0, 400);
    assert_eq!(mint(&base, "a", "x", &[&t], &["deploys.approve"]).await.0, 400);
    assert_eq!(mint(&base, "a", "", &[&t], &["issues.write"]).await.0, 400);
    assert_eq!(mint(&base, "a", "x", &[], &["issues.write"]).await.0, 400);

    // threads.read alone: reads, never files.
    let (_, m) = mint(&base, "a", "reader", &[&t], &["threads.read"]).await;
    let reader = m["token"].as_str().unwrap().to_owned();
    let why = refused(&tool(&base, &reader, "issue_write", json!({ "kind": "create", "title": "no" })).await);
    assert!(why.contains("no issues.write grant") && why.contains("agent-tokens add"), "{why}");
    let r = tool(&base, &reader, "forge_read", json!({ "kind": "native_issues" })).await;
    ok(&r);
    let why = refused(
        &tool(
            &base,
            &reader,
            "plan_deploy",
            json!({ "environment": "production", "commit": "ab".repeat(20), "repo": "acme/app" }),
        )
        .await,
    );
    assert!(why.contains("deploys.plan"), "{why}");

    // A viewer's token never does more than a viewer.
    let (_, m) = mint(&base, "x", "viewer", &[&t], &["issues.write", "secrets.write"]).await;
    let viewer = m["token"].as_str().unwrap().to_owned();
    let why = refused(&tool(&base, &viewer, "issue_write", json!({ "kind": "create", "title": "no" })).await);
    assert!(why.contains("viewer"), "{why}");
    let why = refused(&tool(&base, &viewer, "set_secret", json!({ "name": "A", "value": "b" })).await);
    assert!(why.contains("viewer"), "{why}");

    // Revoked: the next request is refused, before any tool.
    let (_, m) = mint(&base, "a", "gone", &[&t], &["issues.write"]).await;
    let gone = m["token"].as_str().unwrap().to_owned();
    let id = m["agent_token"]["id"].as_str().unwrap().to_owned();
    assert_eq!(call(&base, "b", "POST", &format!("/api/agent-tokens/{id}/revoke"), None).await.0, 404, "not b's");
    let (st, r) = call(&base, "a", "POST", &format!("/api/agent-tokens/{id}/revoke"), None).await;
    assert_eq!((st, &r["live"]), (200, &json!(false)), "{r}");
    assert!(r["grants"].as_array().unwrap().iter().all(|g| g["live"] == json!(false)), "{r}");
    let res =
        bearer(&base, &gone, "POST", "/api/mcp", Some(json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }))).await;
    assert_eq!(res.status().as_u16(), 401);
    assert!(res.headers().get("www-authenticate").is_some());
    assert!(res.text().await.unwrap().contains("revoked"));

    // No token, or a made-up one.
    for t in ["", "aru_agent_00", "ghp_nope"] {
        let st = bearer(&base, t, "POST", "/api/mcp", Some(json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" })))
            .await
            .status()
            .as_u16();
        assert_eq!(st, 401, "{t:?}");
    }
}

/// JSON-RPC as MCP's Streamable HTTP takes it: a notification is 202 with
/// nothing, a batch answers each request, a bad message says why, and
/// `GET` has no stream.
#[tokio::test]
async fn it_speaks_json_rpc_over_post() {
    let (_hub, _app, base, t, _) = platform().await;
    let (_, m) = mint(&base, "a", "rpc", &[&t], &["threads.read"]).await;
    let token = m["token"].as_str().unwrap().to_owned();
    let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    assert_eq!(bearer(&base, &token, "POST", "/api/mcp", Some(note.clone())).await.status().as_u16(), 202);
    let batch =
        json!([note, { "jsonrpc": "2.0", "id": 1, "method": "ping" }, { "jsonrpc": "2.0", "id": 2, "method": "nope" }]);
    let v: Value = bearer(&base, &token, "POST", "/api/mcp", Some(batch)).await.json().await.unwrap();
    assert_eq!(v[0], json!({ "jsonrpc": "2.0", "id": 1, "result": {} }));
    assert_eq!(v[1]["error"]["code"], json!(-32601));
    let r = reqwest::Client::new()
        .post(format!("{base}/api/mcp"))
        .header("authorization", format!("Bearer {token}"))
        .body("{not json")
        .send()
        .await
        .unwrap();
    assert_eq!(r.json::<Value>().await.unwrap()["error"]["code"], json!(-32700));
    assert_eq!(bearer(&base, &token, "GET", "/api/mcp", None).await.status().as_u16(), 405);
    let r = tool(&base, &token, "capabilities", json!({})).await;
    let caps = ok(&r);
    assert_eq!(
        (&caps["approves"], &caps["projects"][0]["powers"]),
        (&json!(false), &json!(["threads.read"])),
        "{caps}"
    );
}

/// The token opens the MCP endpoint and nothing else: every other route the
/// platform answers, with every method, refuses a request that carries
/// only the token (no route reads it but the endpoint).
#[tokio::test]
async fn a_token_opens_no_other_route() {
    let (_hub, _app, base, t, _) = platform().await;
    let (_, m) = mint(&base, "a", "all", &[&t], &ALL).await;
    let token = m["token"].as_str().unwrap().to_owned();
    let mut opened = vec![];
    for path in crate::paths() {
        if path == crate::agent_mcp::MCP {
            continue;
        }
        let concrete = path
            .split('/')
            .map(|s| match s {
                "{project}" => t.as_str(),
                s if s.starts_with('{') => "probe",
                s => s,
            })
            .collect::<Vec<_>>()
            .join("/");
        for method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
            let st = bearer(&base, &token, method, &concrete, Some(json!({}))).await.status();
            if st.is_success() {
                opened.push(format!("{method} {path}: {st}"));
            }
        }
    }
    assert!(opened.is_empty(), "a bearer token opened: {opened:#?}");
}
