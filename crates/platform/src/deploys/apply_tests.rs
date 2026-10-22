//! Applying an approved deploy (M219): its approval makes a box of its own
//! (a hosted workspace of purpose `deploy`, on the fake substrate, listed
//! to nobody) and a run on it once it enrolls, approved by the deploy's
//! approval, which
//! the box can check against the owners it pinned; only that run starts
//! the deploy, before the approval expires; the box gets the
//! environment's secrets once, while its run applies the deploy; and the
//! run's end ends the deploy, paging the owners with its log when it
//! failed; then the box is deleted, whatever happened. On SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::{sync::Arc, time::Duration};

use arugula_control_wire::work::{Report, Reported, SecretsAsk, SecretsGiven, TAKES_DEPLOY, TAKES_RUN, Work};
use arugula_e2e::{
    DeviceKeys,
    deploy::{Decision, PasskeyParts, check_apply},
    hosted::Owners,
    now_ms,
    run::{Op, State as RunState},
    team::TeamRole,
};
use serde_json::{Value, json};

use super::*;
use crate::{
    Platform,
    chat_tests::{TestHub, call},
    cloud::secrets::SecretCipher,
    db::sql::{Sql as _, params},
    deploys_tests::{asked, devices, signed, signed_until},
    hosted::tests::{Fake, drive},
    jobs::{Clock, Worker},
    work::{self, Dispatch},
};

const KEY: &str = "0303030303030303030303030303030303030303030303030303030303030303"; // gitleaks:allow
const SECRET: &str = "s3cr3t-m219-value";

/// Team t (a owns it, b edits, x views), with a's devices and passkey, on a
/// platform that keeps secrets and makes hosted boxes (the fake substrate,
/// enrolling by sandbox id), its project delegated.
async fn world() -> (Arc<TestHub>, Arc<Platform>, String, String, Arc<Fake>) {
    world_with(false).await
}

/// [`world`], with a run-log bucket (M213) when `logs`.
async fn world_with(logs: bool) -> (Arc<TestHub>, Arc<Platform>, String, String, Arc<Fake>) {
    let hub = Arc::new(TestHub::default());
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, crate::db::Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    p.secret_key = Some(Arc::new(SecretCipher::new(KEY).unwrap()));
    let fake = Arc::new(Fake::default());
    p.compute = Some(fake.clone());
    p.hosted = crate::hosted::Config { template: Some("tpl".into()), ..Default::default() };
    if logs {
        let b = crate::run_logs::Bucket::new("http://127.0.0.1:9", "runs", "auto", "id", "secret", 30 * 86_400_000);
        p.run_logs = Some(b.unwrap());
    }
    let app = Arc::new(p);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::cloud::tests::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let t = crate::projects::team_project(&app, "t").await.unwrap().unwrap();
    hub.delegate(&t);
    (hub, app, base, t, fake)
}

/// A hosted machine: its keys, and what the hub says of it.
struct Machine {
    keys: DeviceKeys,
    m: HostedMachine,
}

impl Machine {
    /// A machine of workspace `ws` of `project`, as the hub would say.
    fn new(project: &str, ws: &str) -> Self {
        let keys = DeviceKeys::generate();
        let m = HostedMachine {
            project: project.into(),
            workspace: ws.into(),
            machine: keys.id(),
            sign: hex::encode(keys.sign_public()),
        };
        Self { keys, m }
    }

    /// Its signed report of `run` moving `from` `to` at `at`.
    fn report(&self, run: &Run, from: RunState, to: RunState, at: u64, detail: &str) -> Report {
        let seq = run.last_seq + 1;
        let ev =
            arugula_e2e::run::Event { run: &run.id, seq, from: Some(from), to, at, detail, daemon: &self.m.machine };
        Report {
            run: run.id.clone(),
            seq,
            from: from.as_str().into(),
            to: to.as_str().into(),
            at,
            detail: detail.into(),
            sig: ev.sign(&self.keys),
        }
    }
}

fn worker(app: &Arc<Platform>) -> Worker {
    Worker::new().with(Apply { app: app.clone() }).with(Dispatch { app: app.clone() })
}

/// Every job step due now, a few times over.
async fn settle(app: &Arc<Platform>) {
    let w = worker(app);
    for _ in 0..10 {
        let _ = w.tick(&app.db).await;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Every job step due at `at`, by a worker whose clock says so.
async fn settle_at(app: &Arc<Platform>, at: u64) {
    let w = worker(app).clock(Clock::new(move || at));
    for _ in 0..10 {
        let _ = w.tick(&app.db).await;
    }
}

/// A deploy's box, by the deploy: its id and project.
fn box_of(app: &Platform, deploy: &str) -> Option<(String, String)> {
    app.db
        .c()
        .query_row(
            "SELECT id, project_id FROM hosted_workspaces WHERE name = ?1 AND purpose = 'deploy'",
            params![deploy],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()
}

/// The approved deploy's job makes its box, the hosted worker makes it on
/// the provider and records its enrollment, the box enrolls as a new
/// machine, and the job makes its run there: that machine.
async fn boot(app: &Arc<Platform>, hub: &TestHub, deploy: &str) -> Machine {
    settle(app).await;
    let now = now_ms();
    drive(app, now).await;
    let (b, project) = box_of(app, deploy).expect("its box is made");
    let m = Machine::new(&project, &b);
    hub.enroll(&b, &m.m.machine);
    drive(app, now + 3_000).await;
    settle_at(app, now + 5_000).await;
    m
}

/// The deploy's box, as the platform keeps it.
fn its_box(app: &Platform, deploy: &str) -> crate::db::hosted::HostedWorkspace {
    let (b, project) = box_of(app, deploy).unwrap();
    app.db.hosted_any(&project, &b).unwrap().unwrap()
}

/// The deploy's job deletes its box, and the hosted worker does it: the
/// provider has no box left, and the workspace is `deleted`.
async fn deleted(app: &Arc<Platform>, fake: &Fake, deploy: &str) {
    for i in 1..4 {
        settle_at(app, now_ms() + i * 40_000).await;
        drive(app, now_ms() + i * 40_000 + 5_000).await;
    }
    assert_eq!(its_box(app, deploy).state, "deleted");
    assert!(fake.boxes().is_empty(), "the provider has no box left");
}

/// t's environment `production` with the fake driver and one secret, set.
async fn production(base: &str, t: &str) -> String {
    let (st, env) = call(
        base,
        "a",
        "POST",
        &format!("/api/projects/{t}/environments"),
        Some(json!({ "name": "production", "driver": "fake", "secret_names": ["APP_TOKEN"] })),
    )
    .await;
    assert_eq!(st, 200, "{env}");
    env["id"].as_str().unwrap().to_owned()
}

async fn set_secret(base: &str, who: &str, env: &str, name: &str) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/environments/{env}/secrets/{name}"), Some(json!({ "value": SECRET }))).await
}

/// Planned by b and approved by a's phone, good until `expires`.
async fn approved(base: &str, hub: &TestHub, t: &str, env: &str, sha: &str, expires: u64) -> (String, String) {
    let a = devices(hub, "a");
    let (id, op) = asked(base, "b", t, env, sha).await;
    let body = if expires == 0 {
        signed(&a.phone, "a", &id, &op, Decision::Once)
    } else {
        signed_until(&a.phone, "a", &id, &op, Decision::Once, expires)
    };
    let (st, d) = call(base, "a", "POST", &format!("/api/deploys/{id}/approve"), Some(body)).await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "{d}");
    (id, op)
}

fn state(app: &Platform, deploy: &str) -> State {
    app.db.deploy(deploy).unwrap().unwrap().state
}

async fn pull(app: &Platform, m: &Machine, takes: &[&str]) -> Work {
    let takes: Vec<String> = takes.iter().map(|t| (*t).to_owned()).collect();
    work::work(app, &m.m, &takes).await.unwrap()
}

async fn ask(app: &Platform, m: &Machine, run: &str) -> SecretsGiven {
    secrets(app, &m.m, &SecretsAsk { run: run.into() }).await.unwrap()
}

/// The whole of it, through a failed apply: the approval enqueues the run,
/// approved with the deploy's approval, which checks out against the
/// team's roster as a box pins it; a box that doesn't take deploys never
/// sees it; its `running` starts the deploy; the secrets come once, only
/// to its machine, only while it applies; its failure fails the deploy and
/// pages the owner with the run's log.
#[tokio::test]
async fn an_approved_deploy_is_applied_by_its_run_with_its_secrets_once() {
    let (hub, app, base, t, fake) = world().await;
    let env = production(&base, &t).await;
    assert_eq!(set_secret(&base, "b", &env, "APP_TOKEN").await.0, 403, "an editor doesn't set secrets");
    assert_eq!(set_secret(&base, "a", &env, "OTHER").await.0, 400, "one the environment doesn't list");
    assert_eq!(set_secret(&base, "a", &env, "APP_TOKEN").await.0, 200);
    let (id, op) = approved(&base, &hub, &t, &env, "failing", 0).await;
    let job: String =
        app.db.c().query_row("SELECT kind FROM platform_jobs WHERE id = ?1", params![id], |r| r.get(0)).unwrap();
    assert_eq!(job, "deploy", "the approval queued its apply");

    let m = boot(&app, &hub, &id).await;
    // Its own box: made from the template, enrolled as a deploy's box,
    // listed and opened by nobody.
    let b = its_box(&app, &id);
    assert_eq!(
        (b.purpose.as_str(), b.state.as_str(), b.machine.as_deref()),
        ("deploy", "ready", Some(m.m.machine.as_str()))
    );
    let asked = hub.enrollments();
    assert_eq!((asked[0].workspace.as_str(), asked[0].purpose.as_deref()), (b.id.as_str(), Some("deploy")));
    let (st, listed) = call(&base, "a", "GET", &format!("/api/projects/{t}/workspaces"), None).await;
    assert_eq!((st, listed["workspaces"].as_array().map(Vec::len)), (200, Some(0)), "{listed}");
    assert_eq!(call(&base, "a", "GET", &format!("/api/projects/{t}/workspaces/{}", b.id), None).await.0, 404);
    let dr = app.db.deploy_run(&id).unwrap().expect("its run is made");
    let run = app.db.run(&dr.run).unwrap().unwrap();
    assert_eq!(
        (run.state, run.workspace.as_deref(), run.daemon.as_str()),
        (RunState::Approved, Some(b.id.as_str()), m.m.machine.as_str())
    );
    let (daemon, o) = Op::decode(&run.op).unwrap();
    assert_eq!(daemon, m.m.machine);
    let apply = o.deploy.unwrap();
    assert_eq!((apply.deploy.as_str(), apply.driver.as_str(), apply.op.as_str()), (id.as_str(), "fake", op.as_str()));
    assert_eq!(state(&app, &id), State::Approved, "approved until its run starts it");

    // A machine that doesn't apply deploys isn't sent it.
    assert!(pull(&app, &m, &[TAKES_RUN]).await.items.is_empty());
    let w = pull(&app, &m, &[TAKES_RUN, TAKES_DEPLOY]).await;
    let item = &w.items[0];
    let dw = item.deploy.as_ref().expect("the deploy, to check");
    assert_eq!((item.run.as_str(), item.state.as_str()), (run.id.as_str(), "approved"));
    assert_eq!(
        (dw.operation.hash(), dw.approval.op.clone(), dw.secrets.clone()),
        (op.clone(), op.clone(), vec!["APP_TOKEN".to_owned()])
    );
    // The box's own check, with the team's roster and certificates as it
    // pins them.
    use crate::HubPort as _;
    let roster = hub.team_rosters("t").await.unwrap().pop().unwrap();
    let trust = hub.device_trust("a").await.unwrap().unwrap();
    let certs: arugula_e2e::team::AccountCerts = [("a".to_owned(), (trust.certs, trust.revocations))].into();
    let owners = Owners::Team { roster: &roster, certs: &certs };
    let pk = dw.passkey.as_ref().unwrap();
    let parts = PasskeyParts { client_data_json: &pk.client_data_json, authenticator_data: &pk.authenticator_data };
    assert_eq!(check_apply(&dw.approval, &dw.operation, &id, &owners, Some(parts), now_ms()), Ok(()));

    // No secrets before it runs.
    assert!(matches!(ask(&app, &m, &run.id).await, SecretsGiven::Refused { .. }));
    let r =
        work::report(&app, &m.m, &m.report(&run, RunState::Approved, RunState::Running, now_ms(), "{}")).await.unwrap();
    assert_eq!(r, Reported::Taken);
    let d = app.db.deploy(&id).unwrap().unwrap();
    assert_eq!((d.state, d.run.as_deref()), (State::Running, Some(run.id.as_str())));
    // Its approval is used up by that start: it starts nothing again, and
    // goes to no other run.
    assert_eq!(app.db.approval_consumed_by(&id).unwrap().as_deref(), Some(run.id.as_str()));
    assert!(!crate::db::deploy_approvals::consume(&app.db.c(), &id, "run_other", now_ms()).unwrap());

    // Another machine of the project's doesn't get them; its own, once.
    let other = Machine::new(&t, &b.id);
    assert!(matches!(ask(&app, &other, &run.id).await, SecretsGiven::Refused { .. }));
    match ask(&app, &m, &run.id).await {
        SecretsGiven::Given { secrets } => {
            assert_eq!(
                secrets.iter().map(|s| (s.name.as_str(), s.value.as_str())).collect::<Vec<_>>(),
                [("APP_TOKEN", SECRET)]
            )
        }
        r => panic!("{r:?}"),
    }
    assert_eq!(
        ask(&app, &m, &run.id).await,
        SecretsGiven::Refused {
            why: "its secrets were given already (once per run), or it isn't applying its deploy now".into()
        }
    );

    // It fails part way: the deploy fails, and a is paged with its log.
    let run = app.db.run(&run.id).unwrap().unwrap();
    let r = work::report(&app, &m.m, &m.report(&run, RunState::Running, RunState::Failed, now_ms(), r#"{"exit":3}"#))
        .await
        .unwrap();
    assert_eq!(r, Reported::Taken);
    assert_eq!(state(&app, &id), State::Failed);
    let failed = app.db.deploy_history(&id).unwrap().pop().unwrap();
    assert_eq!((failed.kind.as_str(), failed.actor.clone()), ("failed", format!("run {}", run.id)));
    assert_eq!(failed.detail_json().unwrap()["reason"], json!("the apply exited 3"));
    let page = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(p) = hub.pushes().into_iter().find(|(_, m, _)| m["tag"] == json!(format!("deploy-failed-{id}")))
            {
                return p;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the owners are paged");
    assert_eq!(page.0, "a");
    assert_eq!(page.1["log"], json!(format!("/p/{t}/past/{}", run.id)));
    assert!(!page.1.to_string().contains(SECRET), "never the secret");
    assert!(app.db.verify_deploy_log(&env).unwrap().is_ok());
    settle(&app).await;
    assert_eq!(
        hub.pushes().iter().filter(|(_, m, _)| m["tag"] == json!(format!("deploy-failed-{id}"))).count(),
        1,
        "once"
    );
    // Its box goes once its run ended.
    deleted(&app, &fake, &id).await;
}

/// A run that succeeds, and the driver's check passes: the deploy
/// succeeded, nobody is paged.
#[tokio::test]
async fn a_deploy_whose_run_succeeds_succeeded() {
    let (hub, app, base, t, fake) = world().await;
    let env = production(&base, &t).await;
    let (id, _) = approved(&base, &hub, &t, &env, "good", 0).await;
    let m = boot(&app, &hub, &id).await;
    let run = app.db.run(&app.db.deploy_run(&id).unwrap().unwrap().run).unwrap().unwrap();
    work::report(&app, &m.m, &m.report(&run, RunState::Approved, RunState::Running, now_ms(), "{}")).await.unwrap();
    let run = app.db.run(&run.id).unwrap().unwrap();
    work::report(&app, &m.m, &m.report(&run, RunState::Running, RunState::Succeeded, now_ms(), r#"{"exit":0}"#))
        .await
        .unwrap();
    assert_eq!(state(&app, &id), State::Succeeded);
    settle(&app).await;
    assert!(hub.pushes().iter().all(|(_, m, _)| m["tag"] != json!(format!("deploy-failed-{id}"))));
    deleted(&app, &fake, &id).await;
}

/// The approval's `expires` holds at the start, by the run's own clock:
/// a run that starts after it is failed, the box told so, and the deploy
/// expired. Its secrets never go.
#[tokio::test]
async fn a_start_after_the_approval_expires_is_refused() {
    let (hub, app, base, t, _fake) = world().await;
    let env = production(&base, &t).await;
    set_secret(&base, "a", &env, "APP_TOKEN").await;
    let expires = now_ms() + 60_000;
    let (id, _) = approved(&base, &hub, &t, &env, "late", expires).await;
    let m = boot(&app, &hub, &id).await;
    let run = app.db.run(&app.db.deploy_run(&id).unwrap().unwrap().run).unwrap().unwrap();
    let r = work::report(&app, &m.m, &m.report(&run, RunState::Approved, RunState::Running, expires + 1, "{}"))
        .await
        .unwrap();
    let Reported::Refused { why, now, .. } = r else { panic!("{r:?}") };
    assert!(why.contains("expired"), "{why}");
    assert_eq!(now.as_deref(), Some("failed"));
    assert_eq!(app.db.run(&run.id).unwrap().unwrap().state, RunState::Failed);
    assert_eq!(state(&app, &id), State::Expired);
    assert!(matches!(ask(&app, &m, &run.id).await, SecretsGiven::Refused { .. }));
}

/// At apply time the approver must still be an owner: one made an editor
/// since is refused, and no run is made.
#[tokio::test]
async fn an_approver_who_isnt_an_owner_now_isnt_applied() {
    let (hub, app, base, t, fake) = world().await;
    let env = production(&base, &t).await;
    let (id, _) = approved(&base, &hub, &t, &env, "demoted", 0).await;
    hub.team("t", &[("a", TeamRole::Editor), ("b", TeamRole::Owner), ("x", TeamRole::Viewer)]);
    settle(&app).await;
    assert_eq!(state(&app, &id), State::Canceled);
    assert!(app.db.deploy_run(&id).unwrap().is_none(), "no run");
    let why = app.db.deploy_history(&id).unwrap().pop().unwrap().detail_json().unwrap();
    assert!(why["reason"].as_str().unwrap().contains("doesn't hold now"), "{why}");
    assert!(box_of(&app, &id).is_none() && fake.boxes().is_empty(), "no box was made");
}

/// A box that never comes up (it never enrolls): the deploy is canceled,
/// saying so, and the box is deleted all the same.
#[tokio::test]
async fn a_box_that_never_comes_up_is_deleted_and_its_deploy_canceled() {
    let (hub, app, base, t, fake) = world().await;
    let env = production(&base, &t).await;
    let (id, _) = approved(&base, &hub, &t, &env, "nowhere", 0).await;
    settle(&app).await;
    drive(&app, now_ms()).await;
    assert_eq!(fake.boxes().len(), 1, "made at the provider");
    assert_eq!(its_box(&app, &id).state, "enrolling");
    settle_at(&app, now_ms() + BOX_UP_MS + 60_000).await;
    assert_eq!(state(&app, &id), State::Canceled);
    let why = app.db.deploy_history(&id).unwrap().pop().unwrap().detail_json().unwrap();
    assert!(why["reason"].as_str().unwrap().contains("didn't come up"), "{why}");
    deleted(&app, &fake, &id).await;
}

/// A crash mid-way: the machine holding the deploy's job dies (its lease
/// is never handed back) after the box was made and the run started.
/// Another takes the job up from its checkpoint when the lease runs out,
/// follows the run to its end, and deletes the box.
#[tokio::test]
async fn a_crash_mid_way_still_deletes_the_box() {
    let (hub, app, base, t, fake) = world().await;
    let env = production(&base, &t).await;
    let (id, _) = approved(&base, &hub, &t, &env, "crash", 0).await;
    let m = boot(&app, &hub, &id).await;
    let run = app.db.run(&app.db.deploy_run(&id).unwrap().unwrap().run).unwrap().unwrap();
    work::report(&app, &m.m, &m.report(&run, RunState::Approved, RunState::Running, now_ms(), "{}")).await.unwrap();
    // As if the machine stepping it died mid-step, holding the lease.
    app.db
        .c()
        .execute(
            "UPDATE platform_jobs SET lease = 'dead', lease_until = ?2 WHERE id = ?1",
            params![id, now_ms() + 120_000],
        )
        .unwrap();
    let run = app.db.run(&run.id).unwrap().unwrap();
    work::report(&app, &m.m, &m.report(&run, RunState::Running, RunState::Succeeded, now_ms(), r#"{"exit":0}"#))
        .await
        .unwrap();
    settle(&app).await;
    assert_eq!(its_box(&app, &id).state, "ready", "nobody holds the job yet");
    // The lease runs out; another machine takes it up.
    settle_at(&app, now_ms() + 200_000).await;
    for _ in 0..3 {
        settle_at(&app, now_ms() + 210_000).await;
        drive(&app, now_ms() + 215_000).await;
    }
    assert_eq!(state(&app, &id), State::Succeeded);
    assert_eq!(its_box(&app, &id).state, "deleted");
    assert!(fake.boxes().is_empty());
}

/// An approved run its box never started by the approval's `expires`
/// isn't offered again, and the deploy expired.
#[tokio::test]
async fn a_run_not_started_before_its_approval_expires_isnt_offered_again() {
    let (hub, app, base, t, _fake) = world().await;
    let env = production(&base, &t).await;
    let expires = now_ms() + 20_000;
    let (id, _) = approved(&base, &hub, &t, &env, "slow", expires).await;
    let m = boot(&app, &hub, &id).await;
    let run = app.db.deploy_run(&id).unwrap().unwrap().run;
    assert_eq!(pull(&app, &m, &[TAKES_RUN, TAKES_DEPLOY]).await.items.len(), 1);
    settle_at(&app, expires + 1).await;
    assert_eq!(state(&app, &id), State::Expired);
    assert_eq!(app.db.run(&run).unwrap().unwrap().state, RunState::Expired);
    assert!(pull(&app, &m, &[TAKES_RUN, TAKES_DEPLOY]).await.items.is_empty());
}

/// A box that refuses a deploy's run (its approval didn't verify there)
/// ends the run before it starts: the deploy is canceled, with no
/// `started` in its log, and its owners are paged once.
#[tokio::test]
async fn a_run_the_box_refuses_cancels_its_deploy_and_pages_its_owners() {
    let (hub, app, base, t, fake) = world().await;
    let env = production(&base, &t).await;
    let (id, _) = approved(&base, &hub, &t, &env, "refused", 0).await;
    let m = boot(&app, &hub, &id).await;
    let run = app.db.run(&app.db.deploy_run(&id).unwrap().unwrap().run).unwrap().unwrap();
    let detail = r#"{"reason":"approval_device: not one of the owner's browsers"}"#;
    let r = work::report(&app, &m.m, &m.report(&run, RunState::Approved, RunState::Failed, now_ms(), detail))
        .await
        .unwrap();
    assert_eq!(r, Reported::Taken);
    assert_eq!(state(&app, &id), State::Canceled);
    let kinds: Vec<String> = app.db.deploy_history(&id).unwrap().into_iter().map(|e| e.kind).collect();
    assert_eq!(kinds, ["planned", "requested", "approved", "canceled"]);
    settle(&app).await;
    // Something forged or stale got past the platform: the owners hear.
    let page = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(p) =
                hub.pushes().into_iter().find(|(_, m, _)| m["tag"] == json!(format!("deploy-refused-{id}")))
            {
                return p;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the owners are paged");
    assert_eq!(page.0, "a");
    assert_eq!(
        page.1["body"],
        json!(format!(
            "The deploy box refused the approval for {id}: approval_device: not one of the owner's browsers"
        ))
    );
    settle(&app).await;
    assert_eq!(
        hub.pushes().iter().filter(|(_, m, _)| m["deploy"] == json!(id)).count(),
        2,
        "the ask's, and once for this"
    );
    deleted(&app, &fake, &id).await;
}

/// The platform's own clock holds the approval's `expires` too (with a
/// minute's slack): a start the box signed in time, reported after that,
/// is refused, and the deploy expired.
#[tokio::test]
async fn a_start_the_platform_hears_too_late_is_refused() {
    let (hub, app, base, t, _fake) = world().await;
    let env = production(&base, &t).await;
    let (id, _) = approved(&base, &hub, &t, &env, "slow-net", 0).await;
    let m = boot(&app, &hub, &id).await;
    let run = app.db.run(&app.db.deploy_run(&id).unwrap().unwrap().run).unwrap().unwrap();
    // As if the approval's time ran out more than a minute ago by the
    // platform's clock (the log's copy still says it's good).
    let past = now_ms() - START_SLACK_MS - 1_000;
    app.db.c().execute("UPDATE deploy_approvals SET expires = ?2 WHERE deploy_id = ?1", params![id, past]).unwrap();
    let r =
        work::report(&app, &m.m, &m.report(&run, RunState::Approved, RunState::Running, now_ms(), "{}")).await.unwrap();
    assert!(matches!(&r, Reported::Refused { now: Some(n), .. } if n == "failed"), "{r:?}");
    assert_eq!(state(&app, &id), State::Expired);
}

/// Secret names a driver's shell uses are refused, and only a browser's
/// session sets a secret: never a CLI's login or an agent's machine.
#[tokio::test]
async fn reserved_names_are_refused_and_only_a_browser_sets_a_secret() {
    let (_hub, app, base, t, _fake) = world().await;
    for bad in ["PATH", "LD_PRELOAD", "BASH_ENV", "DEPLOY_ID", "IFS"] {
        let (st, v) = call(
            &base,
            "a",
            "POST",
            &format!("/api/projects/{t}/environments"),
            Some(json!({ "name": format!("env-{}", bad.to_lowercase().replace('_', "-")), "driver": "fake", "secret_names": [bad] })),
        )
        .await;
        assert_eq!(st, 400, "{bad}: {v}");
    }
    let env = production(&base, &t).await;
    let cli = crate::deploys_tests::serve_with_cli(app.clone()).await;
    let post = |header: (&'static str, &'static str)| {
        let (env, cli) = (env.clone(), cli.clone());
        async move {
            reqwest::Client::new()
                .post(format!("{cli}/api/environments/{env}/secrets/APP_TOKEN"))
                .header(header.0, header.1)
                .json(&json!({ "value": "x" }))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }
    };
    assert_eq!(post(("x-test-cli", "a dev_cli")).await, 403, "a CLI's login");
    assert_eq!(post(("x-test-daemon", "a geek")).await, 403, "an agent's machine");
    assert_eq!(set_secret(&base, "a", &env, "APP_TOKEN").await.0, 200, "a browser's session");
}

/// M213: a deploy's box stays up for its run's last log chunk. A run its
/// box reported ended has its whole log kept, and the box goes at once
/// (`a_deploy_whose_run_succeeds_succeeded`); one that control ended while
/// the box ran it keeps its box [`LOG_WAIT_MS`] more, then it goes.
#[tokio::test]
async fn a_deploys_box_waits_for_its_runs_last_log_when_its_box_didnt_say_it_ended() {
    let (hub, app, base, t, fake) = world_with(true).await;
    let env = production(&base, &t).await;
    let (id, _) = approved(&base, &hub, &t, &env, "good", 0).await;
    let m = boot(&app, &hub, &id).await;
    let run = app.db.run(&app.db.deploy_run(&id).unwrap().unwrap().run).unwrap().unwrap();
    work::report(&app, &m.m, &m.report(&run, RunState::Approved, RunState::Running, now_ms(), "{}")).await.unwrap();
    let ended = now_ms();
    let failed = crate::db::runs::Move::Control {
        from: RunState::Running,
        to: RunState::Failed,
        at: ended,
        detail: r#"{"reason":"stopped by control"}"#,
    };
    app.db.move_run(&run.id, &failed).unwrap().unwrap();
    settle_at(&app, ended + LOG_WAIT_MS / 2).await;
    drive(&app, ended + LOG_WAIT_MS / 2 + 5_000).await;
    let b = its_box(&app, &id);
    assert_ne!(b.asked.as_deref(), Some("delete"), "kept for its log");
    assert_ne!(b.state, "deleted");
    settle_at(&app, ended + LOG_WAIT_MS + 1_000).await;
    deleted(&app, &fake, &id).await;
}

/// A deploy whose row went (its project deleted) still has its box
/// deleted by its job; and the reaper deletes a deploy's box past its
/// longest life that nothing else did.
#[tokio::test]
async fn a_box_outlives_neither_its_deploy_nor_its_longest_life() {
    let (hub, app, base, t, fake) = world().await;
    let env = production(&base, &t).await;
    let (id, _) = approved(&base, &hub, &t, &env, "orphan", 0).await;
    boot(&app, &hub, &id).await;
    let b = its_box(&app, &id).id;
    // The deploy's row goes (as with its project), its job still holding
    // its box in its checkpoint.
    app.db.c().execute("DELETE FROM deploy_runs WHERE deploy_id = ?1", params![id]).unwrap();
    app.db.c().execute("DELETE FROM deploys WHERE id = ?1", params![id]).unwrap();
    for i in 1..4 {
        settle_at(&app, now_ms() + i * 40_000).await;
        drive(&app, now_ms() + i * 40_000 + 5_000).await;
    }
    assert_eq!(app.db.deploy_box(&b).unwrap().unwrap().state, "deleted", "its job deleted it");

    // A box nothing deletes: the reaper does, once it's past its life.
    app.db.add_deploy_box("hw_lost", &t, "dep_lost", "a", now_ms()).unwrap();
    assert_eq!(reap(&app, now_ms()).unwrap(), 0, "not yet");
    assert_eq!(reap(&app, now_ms() + BOX_MAX_MS + 1).unwrap(), 1);
    assert_eq!(app.db.deploy_box("hw_lost").unwrap().unwrap().asked.as_deref(), Some("delete"));
    assert_eq!(reap(&app, now_ms() + BOX_MAX_MS + 1).unwrap(), 0, "asked once");
    drive(&app, now_ms() + 5_000).await;
    assert!(fake.boxes().is_empty());
    use crate::jobs::Handler as _;
    assert_eq!(Apply { app: app.clone() }.max_attempts(), u32::MAX, "never given up on");
}

/// What chant→Fly's box reports at its run's end (M220): chant's ledger
/// entry, chant's record and Fly's image and health.
fn chant_fly_end(exit: i32, sha: &str, image: &str, healthy: bool) -> String {
    let sha = crate::deploys_tests::full(sha);
    json!({
        "exit": exit,
        "result": {
            "chant_ledger": format!("https://github.com/acme/app/blob/{}/github/releases.jsonl#L1", sha.replace('0', "f")),
            "chant_record": { "component": "acme", "env": "github", "digest": format!("sha256:{}", &sha[..8]), "gitSha": sha },
            "fly": { "app": "acme-web", "image": image, "healthy": healthy },
        },
    })
    .to_string()
}

/// Plan (as b, `roll_back` or not), ask, and approve (a's phone): its id.
async fn chant_fly_approved(base: &str, hub: &TestHub, t: &str, env: &str, sha: &str, roll_back: bool) -> String {
    let body = json!({
        "environment": env, "source": { "kind": "commit", "ref": crate::deploys_tests::full(sha), "where": "acme/app" },
        "roll_back": roll_back,
    });
    let (st, d) = call(base, "b", "POST", &format!("/api/projects/{t}/deploys"), Some(body)).await;
    assert_eq!(st, 200, "{d}");
    let id = d["id"].as_str().unwrap().to_owned();
    let (st, d) = call(base, "b", "POST", &format!("/api/deploys/{id}/request"), None).await;
    assert_eq!(st, 200, "{d}");
    let op = d["operation"]["hash"].as_str().unwrap().to_owned();
    let a = devices(hub, "a");
    let (st, d) = call(
        base,
        "a",
        "POST",
        &format!("/api/deploys/{id}/approve"),
        Some(signed(&a.phone, "a", &id, &op, Decision::Once)),
    )
    .await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "{d}");
    id
}

/// Its box applies it: pulls it (and checks the plan names the settings
/// and roll back it was handed), starts it, and ends it with `end`.
async fn chant_fly_applied(app: &Arc<Platform>, hub: &TestHub, id: &str, end: &str) -> (Run, DeployWork) {
    let m = boot(app, hub, id).await;
    let w = pull(app, &m, &[TAKES_RUN, TAKES_DEPLOY]).await;
    let dw = w.items[0].deploy.clone().unwrap();
    assert!(
        arugula_e2e::drivers::planned(&dw.driver, &dw.operation, &dw.settings, dw.roll_back.as_ref()),
        "the box finds the approved plan names what it was handed"
    );
    let run = app.db.run(&app.db.deploy_run(id).unwrap().unwrap().run).unwrap().unwrap();
    work::report(app, &m.m, &m.report(&run, RunState::Approved, RunState::Running, now_ms(), "{}")).await.unwrap();
    let run = app.db.run(&run.id).unwrap().unwrap();
    let to = if end.contains(r#""exit":0"#) { RunState::Succeeded } else { RunState::Failed };
    assert_eq!(
        work::report(app, &m.m, &m.report(&run, RunState::Running, to, now_ms(), end)).await.unwrap(),
        Reported::Taken
    );
    (app.db.run(&run.id).unwrap().unwrap(), dw)
}

/// chant→Fly end to end on the platform (M220): the box is handed the
/// environment's settings and the approval's id; what it reports is
/// checked (Fly healthy, chant's entry), kept in the log, and the run
/// links chant's ledger entry, a failed apply's too. A roll back goes back
/// to a source the environment ran, with the image its log kept, in a
/// plan of its own that the box checks too.
#[tokio::test]
async fn chant_fly_links_chants_ledger_and_rolls_back_to_what_it_kept() {
    let (hub, app, base, t, _fake) = world().await;
    let body = json!({
        "name": "production", "driver": "chant-fly", "secret_names": ["FLY_API_TOKEN"],
        "settings": { "fly_app": "acme-web", "chant_dir": "ops", "chant_env": "github" },
    });
    let (st, env) = call(&base, "a", "POST", &format!("/api/projects/{t}/environments"), Some(body)).await;
    assert_eq!(st, 200, "{env}");
    assert_eq!(
        env["settings"],
        json!({ "fly_app": "acme-web", "chant_dir": "ops", "chant_env": "github", "chant_op": "release", "chant_digest": "plan" })
    );
    let env = env["id"].as_str().unwrap().to_owned();
    assert_eq!(set_secret(&base, "a", &env, "FLY_API_TOKEN").await.0, 200);

    // A release that goes well.
    let good = chant_fly_approved(&base, &hub, &t, &env, "aaa1", false).await;
    let image1 = &format!("registry.fly.io/acme-web:deployment-1@sha256:{}", "1".repeat(64));
    let (run, dw) = chant_fly_applied(&app, &hub, &good, &chant_fly_end(0, "aaa1", image1, true)).await;
    assert_eq!(dw.settings["fly_app"], "acme-web");
    assert!(dw.roll_back.is_none());
    assert!(dw.approval_id.as_deref().is_some_and(|a| a.starts_with("apv_")), "{dw:?}");
    assert_eq!(state(&app, &good), State::Succeeded);
    assert_eq!(
        run.chant_ledger,
        Some(format!("https://github.com/acme/app/blob/aaa1{}/github/releases.jsonl#L1", "f".repeat(36)))
    );
    let ok = app.db.deploy_history(&good).unwrap().pop().unwrap().detail_json().unwrap();
    assert_eq!(
        (ok["image"].as_str(), ok["chant_digest"].as_str()),
        (Some(image1.as_str()), Some("sha256:aaa10000")),
        "{ok}"
    );

    // One that went out but isn't healthy: failed, paged, its entry linked.
    let bad = chant_fly_approved(&base, &hub, &t, &env, "bbb2", false).await;
    let image2 = &format!("registry.fly.io/acme-web:deployment-2@sha256:{}", "2".repeat(64));
    let (run, _) = chant_fly_applied(&app, &hub, &bad, &chant_fly_end(0, "bbb2", image2, false)).await;
    assert_eq!(state(&app, &bad), State::Failed);
    assert!(run.chant_ledger.as_deref().is_some_and(|l| l.contains("/blob/bbb2f")), "{run:?}");
    let failed = app.db.deploy_history(&bad).unwrap().pop().unwrap().detail_json().unwrap();
    assert_eq!(failed["reason"], json!("its Fly app isn't healthy after the apply"));

    // Back to what ran before: never to a source it didn't run.
    let never = json!({
        "environment": env, "source": { "kind": "commit", "ref": crate::deploys_tests::full("ccc3"), "where": "acme/app" },
        "roll_back": true,
    });
    let (st, v) = call(&base, "b", "POST", &format!("/api/projects/{t}/deploys"), Some(never)).await;
    assert_eq!(st, 409, "{v}");
    let back = chant_fly_approved(&base, &hub, &t, &env, "aaa1", true).await;
    let (_, d) = call(&base, "a", "GET", &format!("/api/deploys/{back}"), None).await;
    assert_eq!(
        (d["roll_back"]["deploy"].as_str(), d["roll_back"]["to"]["image"].as_str()),
        (Some(good.as_str()), Some(image1.as_str()))
    );
    assert!(
        d["plan_text"].as_str().unwrap().contains(&format!("flyctl deploy --app acme-web --image {image1}")),
        "{d}"
    );
    let (run, dw) = chant_fly_applied(&app, &hub, &back, &chant_fly_end(0, "aaa1", image1, true)).await;
    assert_eq!(dw.roll_back.as_ref().map(|r| r.deploy.as_str()), Some(good.as_str()));
    assert_eq!(state(&app, &back), State::Succeeded);
    assert!(run.chant_ledger.is_some());
    assert!(app.db.verify_deploy_log(&env).unwrap().is_ok());
}
