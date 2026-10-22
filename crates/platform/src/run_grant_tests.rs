//! The standing project grant (M211) through the platform's routes,
//! against the test hub: offered once there's a hosted workspace, signed
//! and revoked by owners only, kept exactly as signed, relayed with a
//! start and a revoke, and what a machine's answers become. Runs on SQLite
//! and, with `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::sync::Arc;

use arugula_control_wire::forge::{DaemonOp, OpAnswer};
use arugula_e2e::{
    DeviceKeys,
    hosted::{Delegation, HostedKey},
    now_ms,
    project_grant::{GrantRevoke, ProjectGrant, Running},
    team::TeamRole,
};
use serde_json::{Value, json};

use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::sql::{Sql as _, params},
};

/// alice owns team t1, bob edits it, vic views it; olga is elsewhere.
async fn serve() -> (Arc<TestHub>, Arc<Platform>, String, String) {
    let hub = Arc::new(TestHub::default());
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("vic", TeamRole::Viewer)]);
    hub.team("t2", &[("olga", TeamRole::Owner)]);
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let p = Platform::new(cfg, crate::db::Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    let app = Arc::new(p);
    let project = app.db.team_project("t1", "t1", 0).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::cloud::tests::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (hub, app, base, project)
}

/// A hosted workspace of `project`, its box enrolled as `machine`.
fn workspace(app: &Platform, project: &str, machine: &str) -> String {
    let id = crate::hosted::new_id();
    app.db.add_hosted_workspace(&id, project, "dev", &Default::default(), "alice", 10, now_ms()).unwrap();
    app.db
        .c()
        .execute("UPDATE hosted_workspaces SET machine = ?1, state = 'ready' WHERE id = ?2", params![machine, id])
        .unwrap();
    id
}

fn grant(project: &str, owner: &str, keys: &DeviceKeys) -> ProjectGrant {
    let mut d = Delegation {
        project: project.into(),
        owner: owner.into(),
        key: HostedKey::from_seed(&[1; 32]).public(),
        at: 1,
        by: String::new(),
        sig: String::new(),
    };
    d.sign_with(keys);
    let mut g = ProjectGrant::new(&d, now_ms(), None);
    g.sign_with(keys);
    g
}

async fn sign(base: &str, who: &str, project: &str, g: &ProjectGrant) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/projects/{project}/runs/grant"), Some(json!({ "grant": g }))).await
}

async fn start(base: &str, who: &str, project: &str, w: &str) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/projects/{project}/runs/start"), Some(json!({ "workspace": w }))).await
}

async fn read(base: &str, who: &str, project: &str) -> Value {
    let (status, v) = call(base, who, "GET", &format!("/api/projects/{project}/runs/grant"), None).await;
    assert_eq!(status, 200, "{v}");
    v
}

/// Offered with the first hosted workspace; an owner's signed grant is
/// kept as signed; a start sends it to the workspace's machine with the
/// revokes; a revoke goes to every machine and stops the next start here.
#[tokio::test]
async fn a_grant_signed_used_and_revoked() {
    let (hub, app, base, project) = serve().await;
    let keys = DeviceKeys::generate();
    let v = read(&base, "vic", &project).await;
    assert_eq!((v["offer"].as_bool(), v["grant"].is_null(), v["owner"].as_str()), (Some(false), true, Some("team:t1")));
    let w = workspace(&app, &project, "m1");
    assert_eq!(read(&base, "alice", &project).await["offer"], true, "offered with a hosted workspace");

    let g = grant(&project, "team:t1", &keys);
    assert_eq!(sign(&base, "bob", &project, &g).await.0, 403, "an editor");
    assert_eq!(sign(&base, "olga", &project, &g).await.0, 404, "not a member");
    let (status, kept) = sign(&base, "alice", &project, &g).await;
    assert_eq!(status, 201, "{kept}");
    assert_eq!(kept["body"], g.body());
    assert_eq!(kept["grant"]["sig"], g.sig);
    let again = grant(&project, "team:t1", &keys);
    assert_eq!(sign(&base, "alice", &project, &again).await.0, 409, "one live grant a project");
    let v = read(&base, "vic", &project).await;
    assert_eq!((v["offer"].as_bool(), v["grant"]["grant"]["id"].as_str()), (Some(false), Some(g.id.as_str())));

    // A start: the grant and the project's revokes go to the machine.
    hub.answers_ops("m1", OpAnswer::Done { url: None });
    let (status, v) = start(&base, "bob", &project, "dev").await;
    assert_eq!(status, 200, "{v}");
    assert_eq!((v["started"].as_bool(), v["machine"].as_str()), (Some(true), Some("m1")));
    assert_eq!(start(&base, "vic", &project, "dev").await.0, 403, "a viewer");
    let asked = hub.ops_asked();
    let Some((m, DaemonOp::RunStart(s))) = asked.last() else { panic!("{asked:?}") };
    assert_eq!((m.as_str(), s.workspace.as_str(), s.cap.as_str()), ("m1", w.as_str(), "runs.start"));
    assert_eq!((&s.grant, s.revokes.len(), s.run.as_ref()), (&g, 0, None));

    // The machine refuses it: its reason, word for word.
    hub.answers_ops("m1", OpAnswer::Cant { why: "grant_unsigned: no owner's device signed the standing grant".into() });
    let (status, v) = start(&base, "alice", &project, "dev").await;
    assert_eq!((status, v["reason"].as_str()), (409, Some("grant_unsigned")), "{v}");
    hub.answers_ops("m1", OpAnswer::Offline);
    assert_eq!(start(&base, "alice", &project, "dev").await.0, 503);

    // Revoked: by an owner only; every machine hears it; no more starts.
    let r = GrantRevoke::new(&g, Running::Stop, now_ms(), &keys);
    let path = format!("/api/projects/{project}/runs/grant/revoke");
    assert_eq!(call(&base, "bob", "POST", &path, Some(json!({ "revoke": r }))).await.0, 403);
    hub.answers_ops("m1", OpAnswer::Done { url: None });
    let (status, v) = call(&base, "alice", "POST", &path, Some(json!({ "revoke": r }))).await;
    assert_eq!((status, v["told"].as_u64()), (200, Some(1)), "{v}");
    let Some((m, DaemonOp::RunRevoke(rr))) = hub.ops_asked().last().cloned() else { panic!() };
    assert_eq!((m.as_str(), &rr.grant, &rr.revoke), ("m1", &g, &r));
    assert_eq!(call(&base, "alice", "POST", &path, Some(json!({ "revoke": r }))).await.0, 409, "once");
    let (status, v) = start(&base, "alice", &project, "dev").await;
    assert_eq!((status, v["reason"].as_str()), (409, Some("no_grant")), "{v}");
    let v = read(&base, "alice", &project).await;
    assert_eq!((v["offer"].as_bool(), v["past"][0]["revoke"]["running"].as_str()), (Some(true), Some("stop")));

    // Signed again (from the phone, say): the next start carries the old
    // grant's revoke, for a machine that was asleep through it.
    let g2 = grant(&project, "team:t1", &keys);
    assert_eq!(sign(&base, "alice", &project, &g2).await.0, 201);
    assert_eq!(start(&base, "alice", &project, &w).await.0, 200);
    let Some((_, DaemonOp::RunStart(s))) = hub.ops_asked().last().cloned() else { panic!() };
    assert_eq!((s.grant, s.revokes), (g2, vec![r]));
}

/// What the platform refuses to keep: a grant for another project or
/// owner, narrowed, unsigned, from the wrong time, or already expired; and
/// a revoke that isn't this project's grant's.
#[tokio::test]
async fn what_isnt_kept() {
    let (_hub, _app, base, project) = serve().await;
    let keys = DeviceKeys::generate();
    let good = grant(&project, "team:t1", &keys);
    let mut bad = vec![
        grant("prj_other", "team:t1", &keys),
        grant(&project, "team:t2", &keys),
        grant(&project, "account:alice", &keys),
        ProjectGrant { sig: String::new(), ..good.clone() },
    ];
    let mut narrow = ProjectGrant { caps: vec!["runs.start".into()], ..good.clone() };
    narrow.sign_with(&keys);
    let mut old = ProjectGrant { made: now_ms() - 3_600_000, ..good.clone() };
    old.sign_with(&keys);
    let mut lapsed = ProjectGrant { made: now_ms() - 1000, expires: Some(now_ms() - 500), ..good.clone() };
    lapsed.sign_with(&keys);
    bad.extend([narrow, old, lapsed]);
    for g in bad {
        let (status, v) = sign(&base, "alice", &project, &g).await;
        assert_eq!(status, 400, "{v} for {g:?}");
    }
    assert_eq!(sign(&base, "alice", &project, &good).await.0, 201);
    let path = format!("/api/projects/{project}/runs/grant/revoke");
    let other = ProjectGrant { id: arugula_e2e::project_grant::new_id(), ..good.clone() };
    let r = GrantRevoke::new(&other, Running::Finish, now_ms(), &keys);
    assert_eq!(call(&base, "alice", "POST", &path, Some(json!({ "revoke": r }))).await.0, 404);
    let r = GrantRevoke { project: "prj_other".into(), ..GrantRevoke::new(&good, Running::Finish, now_ms(), &keys) };
    assert_eq!(call(&base, "alice", "POST", &path, Some(json!({ "revoke": r }))).await.0, 400);
    // No workspace by that name.
    assert_eq!(start(&base, "alice", &project, "nope").await.0, 404);
}
