//! Runs for a machine's agents (M216): off until its owner grants
//! `runs.start` (a run spec), `runs.command` (a command of its own) or
//! `runs.read` over a project, refused saying how; a start
//! is its owner's by hand, recorded with the machine, and never goes past
//! the owner's role; a revoked grant refuses the very next call; and a read
//! waits for the run to move. The CLI's reads, for the project's members.

use std::{sync::Arc, time::Duration};

use arugula_control_wire::runs::{agent_project_runs, agent_run, agent_run_log};
use serde_json::{Value, json};

use super::tests::serve_hub;
use crate::{
    Platform,
    agents_tests::give,
    chat_tests::{TestHub, call},
    db::{
        runs::{Event, Move, State},
        triggers::tests::run_jobs,
    },
    grants::AGENT_UNGRANTED,
};

/// A call signed by `account`'s machine `machine`.
async fn signed(base: &str, who: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req =
        reqwest::Client::new().request(method.parse().unwrap(), format!("{base}{path}")).header("x-test-daemon", who);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

/// [`serve_hub`]'s team t and its project (hosted workspaces `hw_a`, on
/// machine `dev_a`, and `hw_b`), with a machine each: alice's `geek`,
/// carol's `lap` (an editor off the hosted beta's list), vic's `vbox` (a
/// viewer), and olga's `ob` (in team u only).
async fn machines() -> (Arc<Platform>, String, String, Arc<TestHub>) {
    let (app, base, p, hub) = serve_hub().await;
    for (d, a, t) in [("geek", "alice", "t"), ("lap", "carol", "t"), ("vbox", "vic", "t"), ("ob", "olga", "u")] {
        hub.daemon(d, a, Some(t));
    }
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    (app, base, p, hub)
}

/// `account` gives its machine `power` over `project`: the grant's id.
async fn grant(base: &str, account: &str, machine: &str, power: &str, project: &str) -> String {
    let (st, g) =
        call(base, account, "POST", "/api/grants", give(machine, power, json!({ "kind": "project", "id": project })))
            .await;
    assert_eq!(st, 200, "{g}");
    g["id"].as_str().unwrap().to_owned()
}

fn refused(v: &(u16, Value)) -> (u16, Option<&str>) {
    (v.0, v.1["reason"].as_str())
}

/// The done-when's platform half: ungranted, refused saying how; granted,
/// started as alice's by her agent on geek, `pending` with its job queued,
/// then read (listed, in full, its log) with `runs.read`; each refused on
/// the next call once its grant is revoked. A run spec takes `runs.start`;
/// a command of the agent's own takes `runs.command` too.
#[tokio::test]
async fn an_agent_starts_and_reads_runs_where_granted() {
    let (app, base, p, _hub) = machines().await;
    let runs = agent_project_runs(&p);
    let spec = json!({ "name": "test", "workspace": "dev", "argv": ["just", "test"], "directory": "/work/app" });
    assert_eq!(call(&base, "bob", "POST", &format!("/api/projects/{p}/run-specs"), Some(spec)).await.0, 201);
    let by_spec = json!({ "spec": "test" });
    let cmd = json!({ "argv": ["just", "test"], "workspace": "dev", "directory": "/work/app" });

    let v = signed(&base, "alice geek", "POST", &runs, Some(by_spec.clone())).await;
    assert_eq!(refused(&v), (403, Some(AGENT_UNGRANTED)), "{v:?}");
    let why = v.1["error"].as_str().unwrap();
    assert!(why.contains(&format!("`arugula grants add geek-box runs.start --project {p}`")), "{why}");
    assert!(why.contains("Agents and runs"), "{why}");
    let v = signed(&base, "alice geek", "GET", &runs, None).await;
    assert_eq!(refused(&v), (403, Some(AGENT_UNGRANTED)), "{v:?}");
    let (_, reach) = signed(&base, "alice geek", "GET", "/api/daemon/runs/reach", None).await;
    assert_eq!(reach, json!({ "projects": [] }));

    let start = grant(&base, "alice", "geek", "runs.start", &p).await;
    // Only over a project.
    let (st, _) =
        call(&base, "alice", "POST", "/api/grants", give("geek", "runs.read", json!({ "kind": "account" }))).await;
    assert_eq!(st, 400, "runs.read is over a project");

    let (st, run) = signed(&base, "alice geek", "POST", &runs, Some(by_spec.clone())).await;
    assert_eq!(st, 201, "{run}");
    assert_eq!(
        (&run["state"], &run["trigger"], &run["started_by"], &run["started_by_agent"]),
        (&json!("pending"), &json!("manual"), &json!("alice"), &json!("geek")),
        "{run}"
    );
    assert_eq!((&run["source"]["by"], &run["source"]["agent"]), (&json!("alice"), &json!("geek")), "{run}");
    assert!(run["source"]["spec"].as_str().unwrap().starts_with("rsp_"), "{run}");
    assert_eq!((&run["workspace"], &run["daemon"]), (&json!("hw_a"), &json!("dev_a")));
    let id = run["id"].as_str().unwrap().to_owned();
    assert_eq!(run_jobs(&app.db, &id), 1, "dispatched like any start");
    let kept = app.db.run(&id).unwrap().unwrap();
    assert_eq!((kept.started_by.as_str(), kept.started_by_agent.as_deref()), ("alice", Some("geek")));

    // Its own command takes runs.command: runs.start alone doesn't.
    let v = signed(&base, "alice geek", "POST", &runs, Some(cmd.clone())).await;
    assert_eq!(refused(&v), (403, Some(AGENT_UNGRANTED)), "{v:?}");
    let why = v.1["error"].as_str().unwrap();
    assert!(why.contains(&format!("`arugula grants add geek-box runs.command --project {p}`")), "{why}");
    let command = grant(&base, "alice", "geek", "runs.command", &p).await;
    let (st, own) = signed(&base, "alice geek", "POST", &runs, Some(cmd.clone())).await;
    assert_eq!((st, &own["source"]), (201, &json!({ "by": "alice", "agent": "geek" })), "{own}");

    // Reading takes runs.read: runs.start alone doesn't.
    let v = signed(&base, "alice geek", "GET", &agent_run(&id), None).await;
    assert_eq!(refused(&v), (403, Some(AGENT_UNGRANTED)), "{v:?}");
    let read = grant(&base, "alice", "geek", "runs.read", &p).await;
    let (_, reach) = signed(&base, "alice geek", "GET", "/api/daemon/runs/reach", None).await;
    let powers = json!(["runs.command", "runs.read", "runs.start"]);
    assert_eq!(reach, json!({ "projects": [{ "id": p, "name": "t", "powers": powers }] }));
    let (st, listed) = signed(&base, "alice geek", "GET", &runs, None).await;
    assert_eq!((st, &listed["runs"][1]["id"]), (200, &json!(id)), "{listed}");
    assert!(listed["runs"][0].get("events").is_none(), "a list has no timelines: {listed}");
    let (st, full) = signed(&base, "alice geek", "GET", &agent_run(&id), None).await;
    assert_eq!((st, &full["events"][0]["to"], &full["last_seq"]), (200, &json!("pending"), &json!(1)), "{full}");
    let (st, log) = signed(&base, "alice geek", "GET", &format!("{}?offset=0", agent_run_log(&id)), None).await;
    assert_eq!((st, &log["kept"], &log["end"]), (200, &json!(false), &json!(false)), "{log}");
    assert!(log["note"].as_str().unwrap().contains("keeps no log"), "{log}");
    let (st, mine) = signed(&base, "alice geek", "GET", &format!("{runs}?state=pending&limit=1"), None).await;
    assert_eq!((st, mine["runs"].as_array().map(Vec::len)), (200, Some(1)), "{mine}");
    let (st, _) = signed(&base, "alice geek", "GET", &format!("{}?offset=3&tail=true", agent_run_log(&id)), None).await;
    assert_eq!(st, 400, "an offset or the tail");
    let (st, tail) = signed(&base, "alice geek", "GET", &format!("{}?tail=true", agent_run_log(&id)), None).await;
    assert_eq!((st, &tail["kept"]), (200, &json!(false)), "{tail}");
    assert_eq!(signed(&base, "alice geek", "GET", &format!("{runs}?state=nope"), None).await.0, 400);

    // Revoked: the very next call is refused.
    assert_eq!(call(&base, "alice", "POST", &format!("/api/grants/{read}/revoke"), None).await.0, 200);
    let v = signed(&base, "alice geek", "GET", &agent_run(&id), None).await;
    assert_eq!(refused(&v), (403, Some(AGENT_UNGRANTED)), "{v:?}");
    assert_eq!(call(&base, "alice", "POST", &format!("/api/grants/{start}/revoke"), None).await.0, 200);
    let v = signed(&base, "alice geek", "POST", &runs, Some(by_spec)).await;
    assert_eq!(refused(&v), (403, Some(AGENT_UNGRANTED)), "{v:?}");
    assert_eq!(call(&base, "alice", "POST", &format!("/api/grants/{command}/revoke"), None).await.0, 200);
    let v = signed(&base, "alice geek", "POST", &runs, Some(cmd)).await;
    assert_eq!(refused(&v), (403, Some(AGENT_UNGRANTED)), "{v:?}");
    assert_eq!(app.db.project_runs(&p, 10).unwrap().len(), 2, "nothing more started");
}

/// The owner is the ceiling: a viewer's or an off-list editor's machine,
/// granted `runs.start`, starts nothing (a viewer's reads); someone
/// outside the project neither gives a grant over it nor reads its runs,
/// which are no runs at all to them.
#[tokio::test]
async fn an_agent_never_starts_past_its_owner() {
    let (app, base, p, _hub) = machines().await;
    let runs = agent_project_runs(&p);
    let cmd = json!({ "argv": ["make"], "workspace": "hw_a" });
    grant(&base, "carol", "lap", "runs.command", &p).await;
    let (st, v) = signed(&base, "carol lap", "POST", &runs, Some(cmd.clone())).await;
    assert_eq!(st, 403, "off the hosted beta's list: {v}");
    assert!(v["error"].as_str().unwrap().contains("limited beta"), "{v}");
    grant(&base, "vic", "vbox", "runs.command", &p).await;
    grant(&base, "vic", "vbox", "runs.read", &p).await;
    let (st, v) = signed(&base, "vic vbox", "POST", &runs, Some(cmd.clone())).await;
    assert_eq!(st, 403, "a viewer: {v}");
    assert!(app.db.project_runs(&p, 10).unwrap().is_empty());

    grant(&base, "alice", "geek", "runs.command", &p).await;
    let (st, run) = signed(&base, "alice geek", "POST", &runs, Some(json!({ "argv": ["make"] }))).await;
    assert_eq!(st, 400, "two workspaces: say which: {run}");
    let (st, run) = signed(&base, "alice geek", "POST", &runs, Some(cmd)).await;
    assert_eq!(st, 201, "{run}");
    let id = run["id"].as_str().unwrap();
    assert_eq!(signed(&base, "vic vbox", "GET", &agent_run(id), None).await.0, 200, "a viewer's agent reads");

    let (st, _) =
        call(&base, "olga", "POST", "/api/grants", give("ob", "runs.read", json!({ "kind": "project", "id": p })))
            .await;
    assert_eq!(st, 403, "only over what the giver reads");
    assert_eq!(signed(&base, "olga ob", "GET", &agent_run(id), None).await.0, 404, "no run to olga");
    assert_eq!(signed(&base, "olga ob", "GET", &runs, None).await.0, 404);
}

/// A read with `since` and `wait` answers once the run moves, not before;
/// one that's ended answers at once. The CLI's reads are any member's.
#[tokio::test]
async fn a_read_waits_for_the_run_to_move() {
    let (app, base, p, _hub) = machines().await;
    grant(&base, "alice", "geek", "runs.command", &p).await;
    grant(&base, "alice", "geek", "runs.read", &p).await;
    let (_, run) = signed(
        &base,
        "alice geek",
        "POST",
        &agent_project_runs(&p),
        Some(json!({ "instruction": "triage the failure", "workspace": "dev" })),
    )
    .await;
    let id = run["id"].as_str().unwrap().to_owned();

    let t = std::time::Instant::now();
    let (_, same) = signed(&base, "alice geek", "GET", &format!("{}?since=1&wait=1", agent_run(&id)), None).await;
    assert!(t.elapsed() >= Duration::from_millis(900), "waited");
    assert_eq!((&same["state"], &same["last_seq"]), (&json!("pending"), &json!(1)));

    let db = app.clone();
    let cancel = id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let m = Move::Control { from: State::Pending, to: State::Canceled, at: 2, detail: "{}" };
        db.db.move_run(&cancel, &m).unwrap().unwrap();
    });
    let t = std::time::Instant::now();
    let (st, moved) = signed(&base, "alice geek", "GET", &format!("{}?since=1&wait=10", agent_run(&id)), None).await;
    assert_eq!((st, &moved["state"], &moved["last_seq"]), (200, &json!("canceled"), &json!(2)), "{moved}");
    assert!(t.elapsed() < Duration::from_millis(1500), "woken when it moved, not at the next look");
    let t = std::time::Instant::now();
    let (_, ended) = signed(&base, "alice geek", "GET", &format!("{}?since=2&wait=10", agent_run(&id)), None).await;
    assert_eq!(ended["state"], "canceled");
    assert!(t.elapsed() < Duration::from_secs(2), "an ended run answers at once");
    let (_, log) = signed(&base, "alice geek", "GET", &agent_run_log(&id), None).await;
    assert_eq!((&log["kept"], &log["end"]), (&json!(false), &json!(true)), "{log}");

    // A person's reads: any member; nobody else.
    let (st, listed) = call(&base, "vic", "GET", &format!("/api/projects/{p}/runs"), None).await;
    assert_eq!((st, &listed["runs"][0]["started_by_agent"]), (200, &json!("geek")), "{listed}");
    let (st, full) = call(&base, "bob", "GET", &format!("/api/runs/{id}"), None).await;
    assert_eq!((st, full["events"].as_array().map(Vec::len)), (200, Some(2)), "{full}");
    assert_eq!(call(&base, "bob", "GET", &format!("/api/runs/{id}/log"), None).await.0, 200);
    assert_eq!(call(&base, "olga", "GET", &format!("/api/runs/{id}"), None).await.0, 404);
    assert_eq!(call(&base, "olga", "GET", &format!("/api/runs/{id}/log"), None).await.0, 404);
    assert_eq!(call(&base, "olga", "GET", &format!("/api/projects/{p}/runs"), None).await.0, 404);
    assert_eq!(call(&base, "bob", "GET", "/api/runs/run_nope", None).await.0, 404);
}

/// At most `OPEN_WAITS` waits open per account and per machine:
/// the next is refused until one ends.
#[test]
fn waits_open_at_once_are_capped() {
    use super::read::{OPEN_WAITS, hold};
    let me = || vec!["account:capped".to_owned(), "daemon:capped-box".to_owned()];
    let held: Vec<_> = (0..OPEN_WAITS).map(|_| hold(&me()).unwrap()).collect();
    let e = hold(&["daemon:capped-box".to_owned()]).err().unwrap();
    assert_eq!(e.0, axum::http::StatusCode::TOO_MANY_REQUESTS, "{}", e.1);
    assert!(hold(&["daemon:other-box".to_owned()]).is_ok(), "another machine's own");
    drop(held);
    assert!(hold(&me()).is_ok(), "let go when they end");
}

/// A run's pane is what its latest `running` event says.
#[test]
fn the_pane_is_the_running_events() {
    let e = |seq, to, detail: &str| Event {
        run: "run_1".into(),
        seq,
        from: None,
        to,
        at: seq,
        detail: detail.into(),
        daemon: None,
        sig: None,
    };
    assert_eq!(super::read::pane_of(&[e(1, State::Pending, "{}")]), None);
    let running = [e(1, State::Pending, "{}"), e(2, State::Running, r#"{"pane":7,"op":"x"}"#)];
    assert_eq!(super::read::pane_of(&running), Some(7));
    let ended = [running[0].clone(), running[1].clone(), e(3, State::Failed, r#"{"exit":1}"#)];
    assert_eq!(super::read::pane_of(&ended), Some(7));
}
