//! What the platform page's runs need of the platform (M215), through its
//! own routes with chat's test hub ([`TestHub`]): a project's runs listed
//! by trigger and by spec beside M216's state; a failed run in the inbox of
//! whoever started it and of the project's owners, with one push each; and
//! a run's thread for its project's members. On SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::sync::Arc;

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{
    Platform,
    chat_tests::{TestHub, call, serve},
    db::runs::{Move, NewRun, Run, State, TriggerKind},
};

/// Team t: a owns it, o owns it too, e edits, v views. c is in team u,
/// not t. t's project id.
fn seed(app: &Platform, hub: &TestHub) -> String {
    hub.team("t", &[("a", TeamRole::Owner), ("o", TeamRole::Owner), ("e", TeamRole::Editor), ("v", TeamRole::Viewer)]);
    hub.team("u", &[("c", TeamRole::Owner)]);
    hub.flag("team:t", true);
    hub.flag("team:u", true);
    app.db.team_project("t", "t", 1).unwrap()
}

/// A run of `echo hi` in `project`, started by `by` with `trigger`, from
/// `source`.
fn run(app: &Platform, project: &str, trigger: TriggerKind, source: Value, by: &str) -> Run {
    let op = "arugula op v1\ndaemon d1\nkind run\nsession -\npane - -\ncwd -\nargv ZWNobw==\nargv aGk=\n";
    let new = NewRun {
        project,
        workspace: Some("hw_1"),
        trigger,
        source: &source,
        op,
        started_by: by,
        started_by_agent: None,
        grant: None,
    };
    app.db.create_run(&new, 10).unwrap().unwrap()
}

fn ids(v: &Value) -> Vec<String> {
    v["runs"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap().to_owned()).collect()
}

#[tokio::test]
async fn a_projects_runs_narrow_by_trigger_and_spec() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let p = seed(&app, &hub);
    let by_hand = run(&app, &p, TriggerKind::Manual, json!({ "by": "e" }), "e");
    let spec_by_hand = run(&app, &p, TriggerKind::Manual, json!({ "spec": "rsp_1", "by": "e" }), "e");
    let scheduled = run(&app, &p, TriggerKind::Schedule, json!({ "schedule": "rsc_1", "spec": "rsp_1" }), "rsc_1");
    let other = run(&app, &p, TriggerKind::Event, json!({ "rule": "rru_1", "spec": "rsp_2" }), "rru_1");
    let list = |q: &str| {
        let base = base.clone();
        let path = format!("/api/projects/{p}/runs{q}");
        async move { call(&base, "v", "GET", &path, None).await }
    };

    let (st, v) = list("").await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(ids(&v).len(), 4);
    let (_, v) = list("?trigger=manual").await;
    let mut got = ids(&v);
    got.sort();
    let mut want = vec![by_hand.id.clone(), spec_by_hand.id.clone()];
    want.sort();
    assert_eq!(got, want);
    let (_, v) = list("?spec=rsp_1").await;
    let mut got = ids(&v);
    got.sort();
    let mut want = vec![spec_by_hand.id.clone(), scheduled.id.clone()];
    want.sort();
    assert_eq!(got, want);
    assert_eq!(ids(&list("?trigger=schedule&spec=rsp_1").await.1), vec![scheduled.id.clone()]);
    assert_eq!(ids(&list("?trigger=event&state=pending").await.1), vec![other.id.clone()]);
    assert!(ids(&list("?trigger=event&state=failed").await.1).is_empty());
    assert_eq!(list("?trigger=sometimes").await.0, 400);
    assert_eq!(call(&base, "c", "GET", &format!("/api/projects/{p}/runs?trigger=manual"), None).await.0, 404);

    // A deploy's run is manual underneath: `deploy` lists it, and
    // `manual` no longer does (UI audit #164).
    use crate::db::sql::{Sql as _, params};
    let deploy = run(&app, &p, TriggerKind::Manual, json!({ "deploy": "dep_1", "approval": "dap_1" }), "e");
    app.db
        .c()
        .execute(
            "INSERT INTO deploy_runs (deploy_id, run_id, approval_id, workspace_id, driver, created_at) VALUES ('dep_1', ?1, 'dap_1', 'hw_1', 'fake', 1)",
            params![deploy.id],
        )
        .unwrap();
    assert_eq!(ids(&list("?trigger=deploy").await.1), vec![deploy.id.clone()]);
    assert_eq!(ids(&list("?trigger=manual").await.1).len(), 2, "a deploy's run isn't a run by hand");
}

async fn inbox(base: &str, who: &str) -> Vec<Value> {
    let (st, v) = call(base, who, "GET", "/api/inbox", None).await;
    assert_eq!(st, 200, "{v}");
    v["entries"].as_array().unwrap().iter().filter(|e| e["kind"] == "run").cloned().collect()
}

/// A run e started fails: e and the owners a and o each get an entry and
/// one push that opens the run; v, a viewer, and c, outside the team, get
/// nothing. Done is per person; someone outside can't mark it.
#[tokio::test]
async fn a_failed_run_is_in_its_starters_and_owners_inbox() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let p = seed(&app, &hub);
    let r = run(&app, &p, TriggerKind::Manual, json!({ "by": "e" }), "e");
    let detail = json!({ "reason": "not_hosted: a test" }).to_string();
    let (failed, _) = app
        .db
        .move_run(&r.id, &Move::Control { from: State::Pending, to: State::Failed, at: 20, detail: &detail })
        .unwrap()
        .unwrap();
    crate::inbox::run_failed(&app, &failed).await;

    for who in ["e", "a", "o"] {
        let got = inbox(&base, who).await;
        assert_eq!(got.len(), 1, "{who}: {got:?}");
        let e = &got[0];
        assert_eq!((&e["item"], &e["reason"], &e["state"]), (&json!(r.id), &json!("run_failed"), &json!("failed")));
        assert_eq!(
            (&e["repo"], &e["title"], &e["url"]),
            (&json!("t"), &json!("echo hi"), &json!(format!("/runs/{}", r.id)))
        );
        assert_eq!((&e["project"], &e["author"]), (&json!(p), &json!("e")));
    }
    assert!(inbox(&base, "v").await.is_empty());
    assert!(inbox(&base, "c").await.is_empty());

    // One push each, opening the run.
    let mut pushes = Vec::new();
    for _ in 0..100 {
        pushes = hub.pushes();
        if pushes.len() >= 3 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let mut to: Vec<&str> = pushes.iter().map(|(a, _, _)| a.as_str()).collect();
    to.sort();
    assert_eq!(to, ["a", "e", "o"]);
    for (who, msg, _) in &pushes {
        assert_eq!(msg["inbox"], json!({ "item": r.id }));
        let mine = if who == "e" { "your run failed" } else { "a run failed" };
        assert_eq!(msg["title"], json!(format!("t: {mine}")), "{who}");
        assert_eq!(msg["body"], json!("echo hi"));
    }

    let done = Some(json!({ "reason": "run_failed", "done": true }));
    let (st, v) = call(&base, "e", "POST", &format!("/api/inbox/{}", r.id), done.clone()).await;
    assert_eq!((st, &v["marked"]), (200, &json!(1)), "{v}");
    assert!(inbox(&base, "e").await.is_empty());
    assert_eq!(inbox(&base, "a").await.len(), 1, "done is e's alone");
    assert_eq!(call(&base, "c", "POST", &format!("/api/inbox/{}", r.id), done).await.0, 404);
}

/// A scheduled run's person is whoever made its schedule.
#[tokio::test]
async fn a_scheduled_runs_failure_goes_to_who_made_the_schedule() {
    use crate::db::triggers::Operation;
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let p = seed(&app, &hub);
    let op = Operation { argv: vec!["make".into(), "nightly".into()], ..Default::default() };
    let spec = app.db.add_run_spec(&p, "nightly", "hw_1", &op, "e", 1).unwrap();
    let s = app.db.add_run_schedule(&spec, "0 3 * * *", "UTC", 100, "e", 1).unwrap();
    let r = run(&app, &p, TriggerKind::Schedule, json!({ "schedule": s.id, "spec": spec.id }), &s.id);
    let (failed, _) = app
        .db
        .move_run(
            &r.id,
            &Move::Control { from: State::Pending, to: State::Failed, at: 20, detail: r#"{"reason":"not_taken"}"# },
        )
        .unwrap()
        .unwrap();
    crate::inbox::run_failed(&app, &failed).await;
    let got = inbox(&base, "e").await;
    assert_eq!(got.len(), 1);
    assert_eq!((&got[0]["title"], &got[0]["author"]), (&json!("nightly"), &json!("e")));
    assert_eq!(inbox(&base, "a").await.len(), 1);
    assert!(inbox(&base, "v").await.is_empty());
}

/// A run's thread is its project's: a viewer opens it and comments, and
/// someone outside gets 404, as for a run that isn't there.
#[tokio::test]
async fn a_runs_thread_is_its_projects_members() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let p = seed(&app, &hub);
    let r = run(&app, &p, TriggerKind::Manual, json!({ "by": "e" }), "e");
    let open = |who: &'static str, target: String| {
        let base = base.clone();
        async move {
            call(&base, who, "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": target }))).await
        }
    };
    let (st, room) = open("v", r.id.clone()).await;
    assert_eq!(st, 200, "{room}");
    let short = &r.id["run_".len().."run_".len() + 8];
    assert_eq!(
        (&room["name"], &room["project"], &room["target"]),
        (&json!(format!("run {short}")), &json!(p), &json!(r.id))
    );
    let id = room["id"].as_str().unwrap();
    let post = Some(json!({ "text": "flaky again" }));
    assert_eq!(call(&base, "v", "POST", &format!("/api/chat/rooms/{id}/messages"), post).await.0, 200);
    let (st, v) = call(&base, "e", "GET", &format!("/api/chat/rooms/{id}/messages"), None).await;
    assert_eq!((st, &v["messages"][0]["text"]), (200, &json!("flaky again")), "{v}");
    assert_eq!(open("c", r.id.clone()).await.0, 404);
    assert_eq!(call(&base, "c", "GET", &format!("/api/chat/rooms/{id}/messages"), None).await.0, 404);
    assert_eq!(open("a", "run_0123456789abcdef0123456789abcdef".into()).await.0, 404, "no such run");
}
