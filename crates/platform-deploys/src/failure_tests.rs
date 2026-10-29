//! Runs and deploys when something dies part way (M223, platform step 7,
//! native Arugula #19). Each failure is made here with the fake clock and
//! the fake substrate, and each test fails when its protection goes:
//! - **A machine dies holding a job's lease** after each step of a deploy,
//!   and of a run's dispatch: nobody takes the job until its lease runs out,
//!   then another machine does that step again from the checkpoint. The
//!   deploy still gets one box, one run, one `started` and one end; the run
//!   is started once, and its time to be taken isn't reset.
//! - **The platform restarts between "approved" and "dispatched"**: right
//!   after the approval's transaction, and part way through the step that
//!   makes the deploy's run (made, not yet approved). A new `Platform` on
//!   the same database applies it once.
//! - **A driver fails mid-apply**, by exiting non-zero or by its box going
//!   away: the deploy fails and its owner is paged once, however often the
//!   failure is heard again.
//!
//! The box's half, a daemon that crashes mid-run, is
//! `crates/daemon/tests/integration/hosted_work.rs`; a schedule whose
//! machine dies after its tick fired is `triggers::tests`. On SQLite and,
//! with `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering::SeqCst},
    },
    time::Duration,
};

use anyhow::Result;
use arugula_control_wire::work::{Reported, TAKES_DEPLOY, TAKES_RUN};
use arugula_e2e::{
    now_ms,
    run::{DeployApply, Op, State as RunState},
    team::TeamRole,
};
use futures_util::future::BoxFuture;
use serde_json::json;
use tokio::task::JoinHandle;

use crate::{
    Platform,
    chat_tests::TestHub,
    db::{
        Db,
        deploy_runs::NewDeployRun,
        deploys::State,
        jobs::{JobKind, LEASE_MS, LeasedJob},
        sql::{Sql as _, params},
    },
    deploys::apply::{
        Apply, BOX_LIFE_MS, BOX_MS, FOLLOW_MS, RUNNING_MS,
        tests::{Machine, approved, box_of, deleted, its_box, production, pull, settle_at, state},
    },
    hosted::tests::{Fake, drive},
    jobs::{Clock, Handler, Step, Worker},
    work::{self, Dispatch, TAKE_MS, WAITING_MS},
};

/// `h` on a machine that dies right after each step: what the step wrote
/// stands, but the job is never handed back, so its lease holds until it
/// runs out.
struct Dies<H>(H);

impl<H: Handler> Handler for Dies<H> {
    fn kind(&self) -> JobKind {
        self.0.kind()
    }

    fn max_attempts(&self) -> u32 {
        self.0.max_attempts()
    }

    fn step<'a>(&'a self, db: &'a Db, job: &'a mut LeasedJob, clock: &'a Clock) -> BoxFuture<'a, Result<Step>> {
        Box::pin(async move {
            self.0.step(db, job, clock).await?;
            std::future::pending().await
        })
    }
}

/// A clock moved by hand, from `at`.
fn clock(at: u64) -> (Arc<AtomicU64>, Clock) {
    let t = Arc::new(AtomicU64::new(at));
    let read = t.clone();
    (t, Clock::new(move || read.load(SeqCst)))
}

/// One step of `kind`'s job by `w`, a machine that dies mid-way through it.
async fn dies(w: &Worker, db: &Db) {
    let died = tokio::time::timeout(Duration::from_millis(300), w.tick(db)).await;
    assert!(died.is_err(), "the machine died holding the job");
}

/// One machine dies after the job's next step; nobody takes the job until
/// its lease runs out, at `t + LEASE_MS`; then `taker` does the same step
/// again, from what the first one wrote.
async fn dies_and_is_taken_over(dying: &Worker, taker: &Worker, db: &Db, t: &AtomicU64, job: &str) {
    dies(dying, db).await;
    let held = db.platform_job(job).unwrap().unwrap();
    assert_eq!(held.state, "pending");
    let now = t.load(SeqCst);
    assert_eq!(held.lease_until, now + LEASE_MS, "the dead machine still holds it");
    t.store(now + LEASE_MS - 1, SeqCst);
    assert!(!taker.tick(db).await.unwrap(), "nobody takes it while the lease holds");
    t.store(now + LEASE_MS, SeqCst);
    assert!(taker.tick(db).await.unwrap(), "taken over when the lease ran out");
}

fn count(db: &Db, sql: &str, id: &str) -> i64 {
    db.c().query_row(sql, params![id], |r| r.get(0)).unwrap()
}

fn kinds(app: &Platform, deploy: &str) -> Vec<String> {
    app.db.deploy_history(deploy).unwrap().into_iter().map(|e| e.kind).collect()
}

/// A deploy whose machine dies after every step of its apply, each taken
/// over when the lease runs out: one box is made at the provider, one run
/// on it, approved once; its start and its end are each recorded once; the
/// box is deleted. Nothing about the deploy happens twice.
#[tokio::test]
async fn a_deploy_whose_machine_dies_after_each_step_is_applied_once() {
    let (hub, app, base, t, fake) = crate::deploys::apply::tests::world().await;
    let env = production(&base, &t).await;
    let (id, _) = approved(&base, &hub, &t, &env, "good", 0).await;
    let (time, c) = clock(now_ms() + 1_000);
    let slow = Duration::from_secs(3_600);
    let dying = Worker::new().with(Dies(Apply { app: app.clone() })).clock(c.clone()).heartbeat(slow);
    let taker = Worker::new().with(Apply { app: app.clone() }).clock(c.clone()).heartbeat(slow);
    let runs = Worker::new().with(Dispatch { app: app.clone() }).clock(c.clone()).heartbeat(slow);
    let step = || dies_and_is_taken_over(&dying, &taker, &app.db, &time, &id);
    let later = |ms: u64| time.fetch_add(ms, SeqCst) + ms;

    // Its box: named in the checkpoint, made, and made again by the next
    // machine, which finds it.
    step().await;
    let (b, project) = box_of(&app, &id).expect("its box");
    assert_eq!(count(&app.db, "SELECT COUNT(*) FROM hosted_workspaces WHERE name = ?1", &id), 1, "one box");
    drive(&app, later(BOX_MS)).await;
    assert_eq!(fake.boxes().len(), 1, "one box at the provider");
    let m = Machine::new(&project, &b);
    hub.enroll(&b, &m.m.machine);
    drive(&app, later(BOX_MS)).await;
    assert_eq!(its_box(&app, &id).machine.as_deref(), Some(m.m.machine.as_str()));

    // Its run: made, approved and dispatched by the step that dies, and
    // found (not made again) by the next.
    later(BOX_MS);
    step().await;
    let one_run = "SELECT COUNT(*) FROM deploy_runs WHERE deploy_id = ?1";
    assert_eq!(count(&app.db, one_run, &id), 1);
    assert_eq!(count(&app.db, "SELECT COUNT(*) FROM runs WHERE project_id = ?1", &t), 1, "one run");
    let run = app.db.deploy_run(&id).unwrap().unwrap().run;
    let approvals = "SELECT COUNT(*) FROM run_events WHERE run_id = ?1 AND to_state = 'approved'";
    assert_eq!(count(&app.db, approvals, &run), 1, "approved once");
    assert_eq!(count(&app.db, "SELECT COUNT(*) FROM platform_jobs WHERE id = ?1", &run), 1, "dispatched once");
    later(FOLLOW_MS);
    while runs.tick(&app.db).await.unwrap() {}
    assert_eq!(pull(&app, &m, &[TAKES_RUN, TAKES_DEPLOY]).await.items.len(), 1);

    // Its box starts it, and the machine following it dies too.
    let r = app.db.run(&run).unwrap().unwrap();
    let start = m.report(&r, RunState::Approved, RunState::Running, now_ms(), "{}");
    assert_eq!(work::report(&app, &m.m, &start).await.unwrap(), Reported::Taken);
    assert_eq!(state(&app, &id), State::Running);
    later(FOLLOW_MS);
    step().await;
    let r = app.db.run(&run).unwrap().unwrap();
    let end = m.report(&r, RunState::Running, RunState::Succeeded, now_ms(), r#"{"exit":0}"#);
    assert_eq!(work::report(&app, &m.m, &end).await.unwrap(), Reported::Taken);
    assert_eq!(state(&app, &id), State::Succeeded);

    // The box goes, asked for by a step that dies as well.
    later(RUNNING_MS);
    step().await;
    assert_eq!(its_box(&app, &id).state, "deleting");
    drive(&app, later(BOX_MS)).await;
    assert_eq!(its_box(&app, &id).state, "deleted");
    assert!(fake.boxes().is_empty(), "the provider has no box left");
    assert!(taker.tick(&app.db).await.unwrap());
    assert_eq!(app.db.platform_job(&id).unwrap().unwrap().state, "done");
    assert_eq!(kinds(&app, &id), ["planned", "requested", "approved", "started", "succeeded"]);
    assert!(app.db.verify_deploy_log(&env).unwrap().is_ok());
    assert_eq!(count(&app.db, "SELECT COUNT(*) FROM hosted_workspaces WHERE name = ?1", &id), 1, "still one box");
    assert!(hub.pushes().iter().all(|(_, m, _)| m["tag"] != json!(format!("deploy-failed-{id}"))), "nobody paged");
}

/// A machine that stalls in its step just before it makes the deploy's box
/// (its box named in the checkpoint), for longer than its lease: another
/// machine takes the job over and makes that box. When the stalled one
/// goes on, its make is refused as taken: it finds the box another made
/// and carries on, and never cancels the deploy. One box; the deploy is
/// applied.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_machine_that_stalls_before_making_the_box_doesnt_cancel_its_deploy() {
    use crate::deploys::apply::tests::STALL;
    let (hub, app, base, t, fake) = crate::deploys::apply::tests::world().await;
    let env = production(&base, &t).await;
    let (id, _) = approved(&base, &hub, &t, &env, "stall", 0).await;
    let (time, c) = clock(now_ms() + 1_000);
    let slow = Duration::from_secs(3_600);
    let stalling = Worker::new().with(Apply { app: app.clone() }).clock(c.clone()).heartbeat(slow);
    let taker = Worker::new().with(Apply { app: app.clone() }).clock(c).heartbeat(slow);
    let (stalled, stalled_rx) = std::sync::mpsc::channel();
    let (go, go_rx) = std::sync::mpsc::channel::<()>();
    *STALL.lock().unwrap() = Some((
        id.clone(),
        Box::new(move || {
            stalled.send(()).unwrap();
            go_rx.recv().unwrap();
        }),
    ));
    let first = tokio::spawn({
        let app = app.clone();
        async move { stalling.tick(&app.db).await }
    });
    tokio::task::spawn_blocking(move || stalled_rx.recv().unwrap()).await.unwrap();

    // Its lease runs out; the other takes the job and makes the box.
    time.fetch_add(LEASE_MS, SeqCst);
    assert!(taker.tick(&app.db).await.unwrap());
    let (b, project) = box_of(&app, &id).expect("the other made its box");
    go.send(()).unwrap();
    assert!(first.await.unwrap().unwrap(), "the stalled step ends, writing nothing");
    assert_eq!(state(&app, &id), State::Approved, "not canceled");
    assert_eq!(kinds(&app, &id), ["planned", "requested", "approved"]);
    assert_eq!(count(&app.db, "SELECT COUNT(*) FROM hosted_workspaces WHERE name = ?1", &id), 1, "one box");
    let cp = app.db.platform_job(&id).unwrap().unwrap().checkpoint.unwrap();
    assert_eq!(cp["box"], json!(b));

    // And it's applied, on that box.
    let now = time.load(SeqCst);
    drive(&app, now).await;
    let m = Machine::new(&project, &b);
    hub.enroll(&b, &m.m.machine);
    drive(&app, now + 3_000).await;
    settle_at(&app, now + BOX_MS + 5_000).await;
    runs_to(&app, &m, &id, RunState::Succeeded).await;
    assert_eq!(state(&app, &id), State::Succeeded);
    assert_eq!(fake.boxes().len(), 1);
}

/// A run's dispatch whose machine dies after each step: the run is ready
/// for its machine from the first step, started once however often it's
/// pulled, and followed to its end. A run nobody takes still fails at the
/// time its first step set: a machine that keeps dying doesn't keep it
/// waiting for good.
#[tokio::test]
async fn a_runs_dispatch_whose_machine_dies_starts_it_once() {
    use crate::work::tests::{Machine, World};
    let w = World::new();
    let m = Machine::new(&w.project, "hw_1");
    w.workspace("hw_1", &m, "ready");
    let slow = Duration::from_secs(3_600);
    let c = {
        let read = w.clock.clone();
        Clock::new(move || read.load(SeqCst))
    };
    let dying = Worker::new().with(Dies(Dispatch { app: w.app.clone() })).clock(c.clone()).heartbeat(slow);
    let taker = Worker::new().with(Dispatch { app: w.app.clone() }).clock(c).heartbeat(slow);
    let run = w.run(&m.m.machine, "hw_1", Some("g1"));
    let ignored = w.run(&m.m.machine, "hw_1", Some("g1"));
    crate::db::runs::dispatch(&w.app.db.c(), &run, 1_000).unwrap();

    dies_and_is_taken_over(&dying, &taker, &w.app.db, &w.clock, &run).await;
    assert_eq!(w.ready(&m).await, std::slice::from_ref(&run), "ready from the first step");
    let started = m.report(&run, 2, RunState::Pending, RunState::Running, "{}");
    assert_eq!(work::report(&w.app, &m.m, &started).await.unwrap(), Reported::Taken);
    // The same start again (its answer was lost), and a second start.
    assert_eq!(work::report(&w.app, &m.m, &started).await.unwrap(), Reported::Already);
    let again = m.report(&run, 3, RunState::Pending, RunState::Running, "{}");
    assert!(matches!(work::report(&w.app, &m.m, &again).await.unwrap(), Reported::Refused { .. }));
    assert!(w.ready(&m).await.is_empty(), "started: not offered again");

    // The machine following it dies too; the run ends, and its job with it.
    w.at(w.clock.load(SeqCst) + WAITING_MS);
    dies_and_is_taken_over(&dying, &taker, &w.app.db, &w.clock, &run).await;
    let done = m.report(&run, 3, RunState::Running, RunState::Succeeded, r#"{"exit":0}"#);
    assert_eq!(work::report(&w.app, &m.m, &done).await.unwrap(), Reported::Taken);
    w.at(w.clock.load(SeqCst) + RUNNING_MS);
    w.settle().await;
    assert_eq!(w.job(&run), "done");
    let events: Vec<String> = w.app.db.run_events(&run).unwrap().into_iter().map(|e| e.to.to_string()).collect();
    assert_eq!(events, ["pending", "running", "succeeded"], "started once, ended once");

    // Nobody pulls the other; each machine that steps it dies.
    let t0 = w.clock.load(SeqCst);
    crate::db::runs::dispatch(&w.app.db.c(), &ignored, t0).unwrap();
    dies_and_is_taken_over(&dying, &taker, &w.app.db, &w.clock, &ignored).await;
    let mut at = w.clock.load(SeqCst);
    while w.state(&ignored) == RunState::Pending {
        assert!(at < t0 + TAKE_MS + LEASE_MS, "still waiting at {at}: the dying machines reset its time");
        at += LEASE_MS;
        w.at(at);
        dies(&dying, &w.app.db).await;
    }
    assert_eq!(w.state(&ignored), RunState::Failed);
    let last = w.app.db.run_events(&ignored).unwrap().pop().unwrap();
    assert!(last.detail.contains("not_taken"), "{}", last.detail);
}

/// A running platform: its address, and the task serving it.
struct Running {
    app: Arc<Platform>,
    base: String,
    serving: JoinHandle<()>,
}

/// A platform on `db`, with the hub and the provider that outlive it, as
/// production's do (`deploys::apply::tests::world`'s otherwise).
async fn start(hub: &Arc<TestHub>, fake: &Arc<Fake>, db: Db) -> Running {
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, db, hub.clone(), reqwest::Client::new(), Default::default());
    p.secret_key = Some(Arc::new(crate::secrets::SecretCipher::new(&"03".repeat(32)).unwrap()));
    p.compute = Some(fake.clone());
    p.hosted = crate::hosted::Config { template: Some("tpl".into()), ..Default::default() };
    let app = Arc::new(p);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    let serving = tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    Running { app, base, serving }
}

impl Running {
    /// The process goes: nothing of it is left but what it wrote.
    async fn crash(self) {
        self.serving.abort();
        let _ = self.serving.await;
        assert!(Arc::into_inner(self.app).is_some(), "nothing of the old platform is left");
    }
}

/// The deploy's box enrolls as a new machine of `project`.
async fn enrolled(app: &Arc<Platform>, hub: &TestHub, deploy: &str, now: u64) -> Machine {
    drive(app, now).await;
    let (b, project) = box_of(app, deploy).expect("its box is made");
    let m = Machine::new(&project, &b);
    hub.enroll(&b, &m.m.machine);
    drive(app, now + 3_000).await;
    m
}

/// The box runs the deploy's run to `end`.
async fn runs_to(app: &Platform, m: &Machine, deploy: &str, end: RunState) {
    let items = pull(app, m, &[TAKES_RUN, TAKES_DEPLOY]).await.items;
    assert_eq!(items.len(), 1, "its one run, offered");
    let run = app.db.run(&items[0].run).unwrap().unwrap();
    assert_eq!(Some(&run.id), app.db.deploy_run(deploy).unwrap().map(|d| d.run).as_ref());
    let r = work::report(app, &m.m, &m.report(&run, RunState::Approved, RunState::Running, now_ms(), "{}")).await;
    assert_eq!(r.unwrap(), Reported::Taken);
    let run = app.db.run(&run.id).unwrap().unwrap();
    let exit = if end == RunState::Succeeded { r#"{"exit":0}"# } else { r#"{"exit":3}"# };
    let r = work::report(app, &m.m, &m.report(&run, RunState::Running, end, now_ms(), exit)).await;
    assert_eq!(r.unwrap(), Reported::Taken);
}

/// Two crashes between an approval and its run being dispatched, `open`
/// giving a new handle on the same database each time:
/// - right after the approval's transaction, before any step: the next
///   platform finds its job and applies it;
/// - part way through the step that makes the deploy's run (made and
///   dispatched, not yet approved), holding the job: the next platform,
///   once the lease runs out, approves that run rather than make another.
///
/// Each deploy gets one box and one run, and is applied once.
async fn crashes_between_approved_and_dispatched(open: impl Fn() -> Db) {
    let hub = Arc::new(TestHub::default());
    let fake = Arc::new(Fake::default());
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor)]);
    let one = start(&hub, &fake, open()).await;
    crate::projects::follow(&one.app).await.unwrap();
    crate::cache::catch_up(&one.app).await.unwrap();
    let t = crate::projects::team_project(&one.app, "t").await.unwrap().unwrap();
    hub.delegate(&t);
    let env = production(&one.base, &t).await;

    // Approved, and the platform goes before its apply's first step.
    let (first, _) = approved(&one.base, &hub, &t, &env, "first", 0).await;
    let job = one.app.db.platform_job(&first).unwrap().expect("queued with the approval");
    assert_eq!((job.kind.as_str(), job.state.as_str()), ("deploy", "pending"));
    assert!(box_of(&one.app, &first).is_none() && one.app.db.deploy_run(&first).unwrap().is_none());
    one.crash().await;

    let two = start(&hub, &fake, open()).await;
    let now = now_ms();
    settle_at(&two.app, now).await;
    let m = enrolled(&two.app, &hub, &first, now).await;
    settle_at(&two.app, now + 5_000).await;
    runs_to(&two.app, &m, &first, RunState::Succeeded).await;
    assert_eq!(state(&two.app, &first), State::Succeeded);
    assert_eq!(count(&two.app.db, "SELECT COUNT(*) FROM deploy_runs WHERE deploy_id = ?1", &first), 1);
    deleted(&two.app, &fake, &first).await;

    // The next: its box is up, and the step that makes its run dies part
    // way, holding the job, with the run made and dispatched but not
    // approved.
    let (second, _) = approved(&two.base, &hub, &t, &env, "second", 0).await;
    let now = now_ms();
    settle_at(&two.app, now).await;
    let m = enrolled(&two.app, &hub, &second, now).await;
    let b = its_box(&two.app, &second);
    let a = two.app.db.deploy_approvals(&second).unwrap().pop().unwrap();
    let op = Op {
        deploy: Some(DeployApply { deploy: second.clone(), driver: "fake".into(), op: a.op.clone() }),
        ..Op::default()
    }
    .encode(&m.m.machine);
    let new = NewDeployRun {
        project: &t,
        deploy: &second,
        approval: &a.id,
        account: &a.account,
        workspace: &b.id,
        driver: "fake",
        op: &op,
    };
    let made = two.app.db.make_deploy_run(&new, now).unwrap().unwrap();
    assert_eq!(made.state, RunState::Pending);
    two.app
        .db
        .c()
        .execute(
            "UPDATE platform_jobs SET lease = 'dead', lease_until = ?2 WHERE id = ?1",
            params![second, now + LEASE_MS],
        )
        .unwrap();
    two.crash().await;

    let three = start(&hub, &fake, open()).await;
    settle_at(&three.app, now + LEASE_MS - 1).await;
    assert_eq!(three.app.db.run(&made.id).unwrap().unwrap().state, RunState::Pending, "held by the dead machine");
    settle_at(&three.app, now + LEASE_MS).await;
    assert_eq!(three.app.db.run(&made.id).unwrap().unwrap().state, RunState::Approved, "its run, approved");
    runs_to(&three.app, &m, &second, RunState::Succeeded).await;
    assert_eq!(state(&three.app, &second), State::Succeeded);
    assert_eq!(count(&three.app.db, "SELECT COUNT(*) FROM deploy_runs WHERE deploy_id = ?1", &second), 1);
    assert_eq!(count(&three.app.db, "SELECT COUNT(*) FROM runs WHERE project_id = ?1", &t), 2, "a run each");
    for d in [&first, &second] {
        assert_eq!(kinds(&three.app, d), ["planned", "requested", "approved", "started", "succeeded"]);
        assert_eq!(count(&three.app.db, "SELECT COUNT(*) FROM hosted_workspaces WHERE name = ?1", d), 1);
    }
    deleted_after(&three.app, &fake, &second, now + LEASE_MS).await;
    assert!(three.app.db.verify_deploy_log(&env).unwrap().is_ok());
    three.crash().await;
}

/// A directory of its own, removed when dropped.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn a_crash_between_approved_and_dispatched_applies_once_on_a_sqlite_file() {
    let d = Dir(std::env::temp_dir().join(format!("arugula-m223-{}", hex::encode(arugula_e2e::random::<8>()))));
    std::fs::create_dir_all(&d.0).unwrap();
    let file = d.0.join("platform.db");
    crashes_between_approved_and_dispatched(|| Db::open(&file).unwrap()).await;
}

#[tokio::test]
async fn a_crash_between_approved_and_dispatched_applies_once_on_postgres() {
    let Some(url) = std::env::var("ARUGULA_TEST_POSTGRES_URL").ok().filter(|u| !u.is_empty()) else {
        return eprintln!("skipped: needs ARUGULA_TEST_POSTGRES_URL");
    };
    let schema = Db::test_on(&url);
    crashes_between_approved_and_dispatched(|| schema.second().unwrap()).await;
}

/// Pages about `deploy` failing.
fn pages(hub: &TestHub, deploy: &str) -> Vec<String> {
    let tag = json!(format!("deploy-failed-{deploy}"));
    hub.pushes().into_iter().filter(|(_, m, _)| m["tag"] == tag).map(|(to, _, _)| to).collect()
}

/// A driver that fails mid-apply (its run exits non-zero): the deploy
/// fails and its owner is paged once, though the failure is reported
/// again (its answer lost) and the deploy's job looks at it again after.
#[tokio::test]
async fn a_driver_that_fails_mid_apply_fails_its_deploy_and_pages_once() {
    let (hub, app, base, t, fake) = crate::deploys::apply::tests::world().await;
    let env = production(&base, &t).await;
    let (id, _) = approved(&base, &hub, &t, &env, "fail-1", 0).await;
    let m = crate::deploys::apply::tests::boot(&app, &hub, &id).await;
    let run = app.db.deploy_run(&id).unwrap().unwrap().run;
    let r = app.db.run(&run).unwrap().unwrap();
    work::report(&app, &m.m, &m.report(&r, RunState::Approved, RunState::Running, now_ms(), "{}")).await.unwrap();
    let r = app.db.run(&run).unwrap().unwrap();
    let failed = m.report(&r, RunState::Running, RunState::Failed, now_ms(), r#"{"exit":3}"#);
    assert_eq!(work::report(&app, &m.m, &failed).await.unwrap(), Reported::Taken);
    assert_eq!(state(&app, &id), State::Failed);
    assert_eq!(pages(&hub, &id), ["a"], "its owner, paged");

    // Heard again: the box's retry, and the job's backstop.
    assert_eq!(work::report(&app, &m.m, &failed).await.unwrap(), Reported::Already);
    settle_at(&app, now_ms() + RUNNING_MS).await;
    assert_eq!(pages(&hub, &id), ["a"], "paged once");
    let ended: Vec<String> = kinds(&app, &id).into_iter().filter(|k| k == "failed").collect();
    assert_eq!(ended, ["failed"]);
    deleted(&app, &fake, &id).await;
    assert_eq!(pages(&hub, &id), ["a"]);
}

/// A box that goes away mid-apply (it never reports again): at the box's
/// longest life its run fails, the deploy fails and its owner is paged,
/// once; the box is deleted. A late word from the box changes nothing.
#[tokio::test]
async fn a_box_that_goes_away_mid_apply_fails_its_deploy_and_pages_once() {
    let (hub, app, base, t, fake) = crate::deploys::apply::tests::world().await;
    let env = production(&base, &t).await;
    let (id, _) = approved(&base, &hub, &t, &env, "gone", 0).await;
    let m = crate::deploys::apply::tests::boot(&app, &hub, &id).await;
    let run = app.db.deploy_run(&id).unwrap().unwrap().run;
    let r = app.db.run(&run).unwrap().unwrap();
    work::report(&app, &m.m, &m.report(&r, RunState::Approved, RunState::Running, now_ms(), "{}")).await.unwrap();
    assert_eq!(state(&app, &id), State::Running);

    let late = now_ms() + BOX_LIFE_MS + 60_000;
    settle_at(&app, late).await;
    assert_eq!(app.db.run(&run).unwrap().unwrap().state, RunState::Failed);
    assert_eq!(state(&app, &id), State::Failed);
    let why = app.db.deploy_history(&id).unwrap().pop().unwrap().detail_json().unwrap();
    assert_eq!(why["reason"], json!("its deploy's box ran out of time"));
    assert_eq!(pages(&hub, &id), ["a"]);
    settle_at(&app, late + RUNNING_MS).await;

    let r = app.db.run(&run).unwrap().unwrap();
    let said = m.report(&r, RunState::Running, RunState::Succeeded, now_ms(), r#"{"exit":0}"#);
    assert!(matches!(work::report(&app, &m.m, &said).await.unwrap(), Reported::Refused { .. }));
    assert_eq!(state(&app, &id), State::Failed);
    assert_eq!(pages(&hub, &id), ["a"], "paged once");
    deleted_after(&app, &fake, &id, late).await;
}

/// The deploy's job deletes its box, and the hosted worker does it, after
/// `from`: the provider has no box left.
async fn deleted_after(app: &Arc<Platform>, fake: &Fake, deploy: &str, from: u64) {
    for i in 1..4 {
        settle_at(app, from + i * 40_000).await;
        drive(app, from + i * 40_000 + 5_000).await;
    }
    assert_eq!(its_box(app, deploy).state, "deleted");
    assert!(fake.boxes().is_empty(), "the provider has no box left");
}

/// The operator's view (`arugula-platform stuck`): a deploy whose jobs
/// gave up while its run ran shows as a run and a deploy no job follows,
/// with its box left over. Failing the run, as the operator, ends the
/// deploy the platform's way (failed, its owner paged once) and deletes
/// the box. A deploy not yet running is canceled, and a people's
/// workspace is never deleted from here.
#[tokio::test]
async fn the_operator_sees_whats_stuck_and_unsticks_it() {
    let (hub, app, base, t, fake) = crate::deploys::apply::tests::world().await;
    let env = production(&base, &t).await;
    let (id, _) = approved(&base, &hub, &t, &env, "stuck-1", 0).await;
    let m = crate::deploys::apply::tests::boot(&app, &hub, &id).await;
    let run = app.db.deploy_run(&id).unwrap().unwrap().run;
    let r = app.db.run(&run).unwrap().unwrap();
    work::report(&app, &m.m, &m.report(&r, RunState::Approved, RunState::Running, now_ms(), "{}")).await.unwrap();
    let gave_up = "UPDATE platform_jobs SET state = 'failed', error = 'gave up after 8 attempts' WHERE id = ?1";
    for job in [&id, &run] {
        app.db.c().execute(gave_up, params![job]).unwrap();
    }
    // Another, approved, whose machine died holding its job.
    let (other, _) = approved(&base, &hub, &t, &env, "stuck-2", 0).await;
    let now = now_ms();
    app.db
        .c()
        .execute("UPDATE platform_jobs SET lease = 'dead', lease_until = ?2 WHERE id = ?1", params![other, now - 1])
        .unwrap();

    let s = app.db.stuck(now).unwrap();
    let ids = |jobs: &[crate::db::jobs::JobRow]| jobs.iter().map(|j| j.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&s.lapsed), std::slice::from_ref(&other));
    assert_eq!(ids(&s.failed).len(), 2);
    assert_eq!(
        s.runs.iter().map(|u| (u.id.as_str(), u.job.as_deref())).collect::<Vec<_>>(),
        [(run.as_str(), Some("failed"))]
    );
    assert_eq!(
        s.deploys.iter().map(|u| (u.id.as_str(), u.state.as_str())).collect::<Vec<_>>(),
        [(id.as_str(), "running")]
    );
    let b = &s.boxes[0];
    assert_eq!((s.boxes.len(), b.deploy.as_str(), b.left_over()), (1, id.as_str(), true));
    let shown = s.render();
    assert!(shown.contains("LEFT OVER") && shown.contains(&run), "{shown}");

    // The other is canceled, as the platform; its job ends.
    assert!(app.db.operator_cancel_job(&other, now).unwrap());
    app.db.operator_cancel_deploy(&other, "not today", now).unwrap();
    assert_eq!(state(&app, &other), State::Canceled);
    let why = app.db.deploy_history(&other).unwrap().pop().unwrap();
    assert_eq!(
        (why.actor.as_str(), why.detail_json().unwrap()["reason"].clone()),
        ("platform", json!("operator: not today"))
    );
    settle_at(&app, now + 1_000).await;
    assert_eq!(app.db.platform_job(&other).unwrap().unwrap().state, "done");
    assert!(!app.db.operator_cancel_job(&other, now).unwrap(), "ended");

    // A running deploy ends with its run, never by a cancel.
    assert!(app.db.operator_cancel_deploy(&id, "stuck", now).is_err());
    app.db.operator_fail_run(&run, "its box went away", now).unwrap();
    assert!(app.db.operator_fail_run(&run, "again", now).is_err(), "ended");
    let last = app.db.run_events(&run).unwrap().pop().unwrap();
    assert_eq!((last.daemon, last.detail.as_str()), (None, r#"{"reason":"operator: its box went away"}"#));
    deleted_after(&app, &fake, &id, now).await;
    assert_eq!(state(&app, &id), State::Failed);
    assert_eq!(kinds(&app, &id).last().map(String::as_str), Some("failed"));
    assert_eq!(pages(&hub, &id), ["a"]);

    // People's workspaces aren't the operator's to delete.
    app.db.add_hosted_workspace("hw_people", &t, "people", &Default::default(), "a", 10, 1).unwrap();
    assert!(app.db.operator_delete_box("hw_people", now).is_err());
    let s = app.db.stuck(now + 2_000).unwrap();
    assert_eq!((s.runs.len(), s.deploys.len(), s.boxes.len(), s.lapsed.len()), (0, 0, 0, 0), "{s:?}");
}

/// A run's job that gives up (each claim crashed, up to its most attempts)
/// fails its run in the same transaction, saying so: no run stays pending
/// with no job. One whose job is canceled fails too.
#[tokio::test]
async fn a_run_whose_job_gives_up_fails_with_it() {
    use crate::{
        db::jobs::MAX_ATTEMPTS,
        work::tests::{Machine, World},
    };
    let w = World::new();
    let m = Machine::new(&w.project, "hw_1");
    w.workspace("hw_1", &m, "ready");
    let c = {
        let read = w.clock.clone();
        Clock::new(move || read.load(SeqCst))
    };
    let dying =
        Worker::new().with(Dies(Dispatch { app: w.app.clone() })).clock(c).heartbeat(Duration::from_secs(3_600));
    let run = w.run(&m.m.machine, "hw_1", Some("g1"));
    crate::db::runs::dispatch(&w.app.db.c(), &run, 1_000).unwrap();
    for i in 0..MAX_ATTEMPTS as u64 {
        w.at(1_000 + i * LEASE_MS);
        dies(&dying, &w.app.db).await;
    }
    assert_eq!(w.state(&run), RunState::Pending);
    // The next claim gives up on it, and fails the run with it.
    w.at(1_000 + MAX_ATTEMPTS as u64 * LEASE_MS);
    assert!(!dying.tick(&w.app.db).await.unwrap(), "nothing left to claim");
    let job = w.app.db.platform_job(&run).unwrap().unwrap();
    assert_eq!(job.state, "failed");
    assert_eq!(w.state(&run), RunState::Failed);
    let last = w.app.db.run_events(&run).unwrap().pop().unwrap();
    assert_eq!(last.daemon, None, "control's");
    assert_eq!(last.detail, format!(r#"{{"reason":"its job gave up after {MAX_ATTEMPTS} attempts"}}"#));
    assert!(w.app.db.stuck(w.clock.load(SeqCst)).unwrap().runs.is_empty());

    // A run whose job is canceled.
    let other = w.run(&m.m.machine, "hw_1", Some("g1"));
    crate::db::runs::dispatch(&w.app.db.c(), &other, w.clock.load(SeqCst)).unwrap();
    w.settle().await;
    assert!(w.app.db.operator_cancel_job(&other, w.clock.load(SeqCst)).unwrap());
    w.settle().await;
    assert_eq!(w.job(&other), "cancelled");
    assert_eq!(w.state(&other), RunState::Failed);
    let last = w.app.db.run_events(&other).unwrap().pop().unwrap();
    assert_eq!(last.detail, r#"{"reason":"its job was canceled"}"#);
}

/// A deploy's run whose job gives up while it applies the deploy: the run
/// and the deploy fail in the same transaction, the deploy's job pages its
/// owner once, and deletes its box.
#[tokio::test]
async fn a_deploy_run_whose_job_gives_up_fails_its_deploy_and_pages_once() {
    let (hub, app, base, t, fake) = crate::deploys::apply::tests::world().await;
    let env = production(&base, &t).await;
    let (id, _) = approved(&base, &hub, &t, &env, "gives-up", 0).await;
    let m = crate::deploys::apply::tests::boot(&app, &hub, &id).await;
    let run = app.db.deploy_run(&id).unwrap().unwrap().run;
    let r = app.db.run(&run).unwrap().unwrap();
    work::report(&app, &m.m, &m.report(&r, RunState::Approved, RunState::Running, now_ms(), "{}")).await.unwrap();
    assert_eq!(state(&app, &id), State::Running);
    // Its job crashed at every claim: the next one gives up.
    let now = now_ms() + 10_000;
    app.db
        .c()
        .execute(
            "UPDATE platform_jobs SET attempts = ?2, lease = 'dead', lease_until = ?3, next_at = ?3 WHERE id = ?1",
            params![run, crate::db::jobs::MAX_ATTEMPTS, now],
        )
        .unwrap();
    let runs = Worker::new().with(Dispatch { app: app.clone() }).clock(Clock::new(move || now));
    assert!(!runs.tick(&app.db).await.unwrap());
    assert_eq!(app.db.platform_job(&run).unwrap().unwrap().state, "failed");
    assert_eq!(app.db.run(&run).unwrap().unwrap().state, RunState::Failed);
    assert_eq!(state(&app, &id), State::Failed);
    let failed = app.db.deploy_history(&id).unwrap().pop().unwrap();
    assert_eq!(failed.actor, format!("run {run}"));
    assert!(app.db.verify_deploy_log(&env).unwrap().is_ok());

    deleted_after(&app, &fake, &id, now).await;
    assert_eq!(pages(&hub, &id), ["a"], "paged once");
    settle_at(&app, now + 300_000).await;
    assert_eq!(pages(&hub, &id), ["a"]);
    let s = app.db.stuck(now + 300_000).unwrap();
    assert!(s.runs.is_empty() && s.deploys.is_empty() && s.boxes.is_empty(), "{s:?}");
}
