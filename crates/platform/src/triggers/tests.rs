//! Each trigger starts a run (M214): by hand through the routes, on a
//! schedule through the job worker on two machines, and on a linked
//! repo's event. Each run is `pending` with its `run` job queued once; a
//! schedule fires a tick once across two machines and a restart, and a
//! window missed while the platform was down fires once. On SQLite and,
//! with `ARUGULA_TEST_POSTGRES_URL`, on Postgres.

use crate::jobs::Worker;
use std::{
    sync::atomic::{AtomicU64, Ordering::SeqCst},
    time::Duration,
};

use arugula_e2e::team::TeamRole;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};

use super::*;
use crate::{
    chat_tests::{TestHub, call},
    db::{
        Db,
        jobs::{JobKind, LEASE_MS, LeasedJob, tests::two_machines},
        runs::{State, TriggerKind},
        sql::{Sql as _, params},
        triggers::{
            Rule, RuleEvent, Started,
            tests::{cmd, link, run_jobs, runs_in, seed},
        },
    },
    jobs::{Clock, Handler, Step},
    triggers::events::{Seen, glob, seen},
};

/// A test's clock, moved by hand.
fn clock(at: u64) -> (Arc<AtomicU64>, Clock) {
    let t = Arc::new(AtomicU64::new(at));
    let read = t.clone();
    (t, Clock::new(move || read.load(SeqCst)))
}

/// Schedules' handler, letting every maker through.
fn ticks() -> schedule::Ticks {
    schedule::Ticks { gate: Arc::new(Open) }
}

/// [`events::fire`], letting every maker through, each start unwrapped.
async fn fire_open(db: &Db, seen: &Seen, delivery: &str, now: u64) -> anyhow::Result<Vec<(Rule, Started)>> {
    Ok(events::fire(db, &Open, seen, delivery, now).await?.into_iter().map(|(r, s)| (r, s.unwrap())).collect())
}

fn worker(c: &Clock) -> Worker {
    Worker::new().clock(c.clone()).heartbeat(Duration::from_millis(10)).with(ticks())
}

/// 2027-01-15T08:00:00Z, on a ten-minute tick.
const T0: u64 = 1_800_000_000_000;
const EVERY_10: &str = "*/10 * * * *";

/// `prj_a`'s spec `ci` on `hw_a`, with a schedule every ten minutes whose
/// first tick is `first`.
fn scheduled(db: &Db, first: u64) -> (String, String) {
    let spec = db.add_run_spec("prj_a", "ci", "hw_a", &cmd(&["just", "test"]), "alice", 1).unwrap();
    let s = db.add_run_schedule(&spec, EVERY_10, "UTC", first, "alice", 1).unwrap();
    (spec.id, s.id)
}

/// The schedule kind, on a machine that dies right after its step fired:
/// the step's writes stand, and the job stays held until its lease runs
/// out.
struct DiesAfterFiring;

impl Handler for DiesAfterFiring {
    fn kind(&self) -> JobKind {
        JobKind::Schedule
    }

    fn step<'a>(&'a self, db: &'a Db, job: &'a mut LeasedJob, clock: &'a Clock) -> BoxFuture<'a, anyhow::Result<Step>> {
        Box::pin(async move {
            ticks().step(db, job, clock).await?;
            std::future::pending().await
        })
    }
}

#[test]
fn cron_and_zones() {
    assert_eq!(schedule::next_tick(EVERY_10, "UTC", T0).unwrap(), T0 + 600_000);
    assert_eq!(schedule::next_tick(EVERY_10, "UTC", T0 - 1).unwrap(), T0);
    // 09:00 on weekdays in Chicago is 15:00 UTC in January (CST).
    let nine = schedule::next_tick("0 9 * * 1-5", "America/Chicago", T0).unwrap();
    assert_eq!(nine, T0 - T0 % 86_400_000 + 15 * 3_600_000);
    assert_eq!(schedule::check("@daily", "UTC", T0).unwrap(), T0 - T0 % 86_400_000 + 86_400_000);
    for (cron, tz) in [
        ("* * * * *", "UTC"),
        ("0,1 * * * *", "UTC"),
        ("0 0 * * * *", "UTC"),
        ("nonsense", "UTC"),
        ("", "UTC"),
        ("0 9 * * *", "Mars/Olympus"),
    ] {
        assert!(schedule::check(cron, tz, T0).is_err(), "{cron} {tz}");
    }
    assert!(schedule::check("*/5 * * * *", "Europe/Berlin", T0).is_ok());
}

/// Two machines at the tick: one fires it, once; the run is `pending` with
/// its job queued, started by the schedule. Before the tick, nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tick_fires_once_on_two_machines() {
    let (a, b, _file) = two_machines();
    seed(&a);
    let (spec, s) = scheduled(&a, T0);
    let (t, c) = clock(T0 - 1);
    let (wa, wb) = (worker(&c), worker(&c));
    assert!(!wa.tick(&a).await.unwrap(), "not due");
    t.store(T0 + 1_000, SeqCst);
    let (ra, rb) = tokio::join!(wa.tick(&a), wb.tick(&b));
    assert_eq!(u8::from(ra.unwrap()) + u8::from(rb.unwrap()), 1, "one machine took it");
    assert!(!wa.tick(&a).await.unwrap() && !wb.tick(&b).await.unwrap());
    let runs = runs_in(&b, "prj_a");
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!((run.state, run.trigger, run.started_by.as_str()), (State::Pending, TriggerKind::Schedule, s.as_str()));
    assert_eq!(run.source, json!({ "schedule": s, "spec": spec, "tick": T0 }));
    assert_eq!(run_jobs(&b, &run.id), 1, "dispatched once");
    let sched = b.run_schedule(&s).unwrap().unwrap();
    assert_eq!((sched.next_at, sched.last_fired_at), (T0 + 600_000, Some(T0)));
    assert_eq!(b.platform_job(&s).unwrap().unwrap().next_at, T0 + 600_000);
}

/// A machine that dies right after firing a tick, holding the job: the
/// other machine (the platform, restarted) takes the job over once the
/// lease runs out and doesn't fire that tick again; it fires the next.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restart_after_a_tick_doesnt_fire_it_again() {
    let (a, b, _file) = two_machines();
    seed(&a);
    let (_, s) = scheduled(&a, T0);
    let (t, c) = clock(T0);
    let dying = Worker::new().clock(c.clone()).heartbeat(Duration::from_secs(3600)).with(DiesAfterFiring);
    let died = tokio::time::timeout(Duration::from_millis(300), dying.tick(&a)).await;
    assert!(died.is_err(), "it died mid-step");
    assert_eq!(runs_in(&b, "prj_a").len(), 1, "it fired before it died");
    // The database stays: `a`'s handle on it is the dead machine's.
    let restarted = worker(&c);
    t.store(T0 + LEASE_MS - 1, SeqCst);
    assert!(!restarted.tick(&b).await.unwrap(), "held until the lease runs out");
    t.store(T0 + LEASE_MS, SeqCst);
    assert!(restarted.tick(&b).await.unwrap(), "taken over");
    assert_eq!(runs_in(&b, "prj_a").len(), 1, "the tick it fired isn't fired again");
    assert_eq!(b.platform_job(&s).unwrap().unwrap().next_at, T0 + 600_000);
    t.store(T0 + 600_000, SeqCst);
    assert!(restarted.tick(&b).await.unwrap());
    let runs = runs_in(&b, "prj_a");
    assert_eq!(runs.len(), 2, "the next tick fires");
    assert_eq!(runs[0].source["tick"], json!(T0 + 600_000));
}

/// The platform down across three ticks: back, it fires once (for the
/// first it missed), and its next tick is the first after now.
#[tokio::test]
async fn a_missed_window_fires_once() {
    let db = Db::test();
    seed(&db);
    let (_, s) = scheduled(&db, T0);
    let (_, c) = clock(T0 + 35 * 60_000);
    let w = worker(&c);
    assert!(w.tick(&db).await.unwrap());
    assert!(!w.tick(&db).await.unwrap(), "nothing else is due");
    let runs = runs_in(&db, "prj_a");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].source["tick"], json!(T0));
    let sched = db.run_schedule(&s).unwrap().unwrap();
    assert_eq!((sched.next_at, sched.last_fired_at), (T0 + 40 * 60_000, Some(T0)));
}

/// A schedule that's off waits and fires nothing; a deleted one's job
/// ends.
#[tokio::test]
async fn off_and_deleted_schedules() {
    let db = Db::test();
    seed(&db);
    let (_, s) = scheduled(&db, T0);
    db.set_run_schedule(&s, EVERY_10, "UTC", false, T0, 1).unwrap();
    let (t, c) = clock(T0);
    let w = worker(&c);
    assert!(w.tick(&db).await.unwrap());
    assert!(runs_in(&db, "prj_a").is_empty());
    assert_eq!(db.platform_job(&s).unwrap().unwrap().next_at, T0 + schedule::IDLE_MS);
    db.c().execute("DELETE FROM run_schedules WHERE id = ?1", params![s]).unwrap();
    db.c().execute("UPDATE platform_jobs SET next_at = 0 WHERE id = ?1", params![s]).unwrap();
    t.store(T0 + 1, SeqCst);
    assert!(w.tick(&db).await.unwrap());
    assert_eq!(db.platform_job(&s).unwrap().unwrap().state, "done");
}

/// Team t: alice owns it, bob edits, vic views, carol edits but isn't on
/// the hosted beta's list; olga is elsewhere. Its project has hosted
/// workspaces `hw_a` (machine `dev_a`) and `hw_b` (not enrolled yet).
async fn serve() -> (Arc<Platform>, String, String) {
    let (app, base, project, _) = serve_hub().await;
    (app, base, project)
}

/// [`serve`], and its hub.
pub(super) async fn serve_hub() -> (Arc<Platform>, String, String, Arc<TestHub>) {
    serve_hub_on(None).await
}

/// [`serve_hub`], with the GitHub App against the (fake) GitHub at `api`
/// when there's one.
async fn serve_hub_on(api: Option<&str>) -> (Arc<Platform>, String, String, Arc<TestHub>) {
    let hub = Arc::new(TestHub::default());
    hub.team(
        "t",
        &[
            ("alice", TeamRole::Owner),
            ("bob", TeamRole::Editor),
            ("vic", TeamRole::Viewer),
            ("carol", TeamRole::Editor),
        ],
    );
    hub.team("u", &[("olga", TeamRole::Owner)]);
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    p.hosted = crate::hosted::Config {
        accounts: vec!["alice".into(), "bob".into(), "vic".into(), "olga".into()],
        ..Default::default()
    };
    if let Some(api) = api {
        use crate::forge::{
            GithubApp,
            links_tests::{SECRET, throwaway_key},
        };
        p.github = Some(GithubApp::new("1".into(), "arugula-test".into(), SECRET.into(), api, throwaway_key()));
    }
    let app = Arc::new(p);
    let project = app.db.team_project("t", "t", 0).unwrap();
    app.db
        .c()
        .execute(
            "INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, machine, created_at, updated_at)
             VALUES ('hw_a', ?1, 'dev', '{}', 'h', 'ready', 'dev_a', 1, 1), ('hw_b', ?1, 'new', '{}', 'h', 'creating', NULL, 1, 1)",
            params![project],
        )
        .unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (app, base, project, hub)
}

/// A call signed by `account`'s machine `machine`.
async fn as_machine(base: &str, account: &str, machine: &str, method: &str, path: &str, body: Value) -> u16 {
    reqwest::Client::new()
        .request(method.parse().unwrap(), format!("{base}{path}"))
        .header("x-test-daemon", format!("{account} {machine}"))
        .json(&body)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

/// By hand: an owner or an editor on the beta's list starts a spec or a
/// command, `pending` with its job queued; a viewer, someone off the list,
/// someone outside and a machine can't; a workspace with no machine is a
/// conflict.
#[tokio::test]
async fn a_person_starts_a_run_by_hand() {
    let (app, base, p) = serve().await;
    let specs = format!("/api/projects/{p}/run-specs");
    let runs = format!("/api/projects/{p}/runs");
    let (st, spec) = call(
        &base,
        "bob",
        "POST",
        &specs,
        Some(json!({ "name": "test", "workspace": "dev", "argv": ["just", "test"], "directory": "/work/app" })),
    )
    .await;
    assert_eq!((st, &spec["workspace"], &spec["kind"]), (201, &json!("hw_a"), &json!("run")), "{spec}");

    let (st, run) = call(&base, "alice", "POST", &runs, Some(json!({ "spec": "test" }))).await;
    assert_eq!(
        (st, &run["state"], &run["trigger"], &run["started_by"]),
        (201, &json!("pending"), &json!("manual"), &json!("alice")),
        "{run}"
    );
    assert_eq!(run["source"], json!({ "spec": spec["id"], "by": "alice" }));
    let id = run["id"].as_str().unwrap();
    assert_eq!(run_jobs(&app.db, id), 1);
    let op = crate::db::triggers::Operation { directory: Some("/work/app".into()), ..cmd(&["just", "test"]) };
    assert_eq!(app.db.run(id).unwrap().unwrap().op, op.encode("dev_a"), "the spec, on the workspace's machine");

    let (st, run) = call(&base, "bob", "POST", &runs, Some(json!({ "argv": ["make"], "workspace": "hw_a" }))).await;
    assert_eq!((st, &run["source"]), (201, &json!({ "by": "bob" })), "{run}");
    let (st, v) = call(&base, "bob", "POST", &runs, Some(json!({ "argv": ["make"] }))).await;
    assert_eq!(st, 400, "two workspaces: say which: {v}");
    let (st, v) = call(&base, "bob", "POST", &runs, Some(json!({ "argv": ["make"], "workspace": "new" }))).await;
    assert_eq!(st, 409, "no machine yet: {v}");
    assert_eq!(call(&base, "bob", "POST", &runs, Some(json!({ "spec": "test", "argv": ["x"] }))).await.0, 400);
    assert_eq!(call(&base, "bob", "POST", &runs, Some(json!({ "spec": "nope" }))).await.0, 404);
    assert_eq!(call(&base, "bob", "POST", &runs, Some(json!({}))).await.0, 422);

    assert_eq!(call(&base, "vic", "POST", &runs, Some(json!({ "spec": "test" }))).await.0, 403, "a viewer");
    assert_eq!(
        call(&base, "carol", "POST", &runs, Some(json!({ "spec": "test" }))).await.0,
        403,
        "off the beta's list"
    );
    assert_eq!(call(&base, "olga", "POST", &runs, Some(json!({ "spec": "test" }))).await.0, 404, "outside");
    let st = as_machine(&base, "alice", "geek", "POST", &runs, json!({ "spec": "test" })).await;
    assert_eq!(st, 401, "a machine's agent doesn't start runs here");
    assert_eq!(runs_in(&app.db, &p).len(), 2);
}

/// Specs, schedules and rules through the routes: who may change them,
/// and what's checked.
#[tokio::test]
async fn specs_schedules_and_rules() {
    let (app, base, p) = serve().await;
    let specs = format!("/api/projects/{p}/run-specs");
    let new = json!({ "name": "triage", "workspace": "hw_a", "instruction": "look at the failure" });
    assert_eq!(call(&base, "vic", "POST", &specs, Some(new.clone())).await.0, 403);
    assert_eq!(call(&base, "olga", "POST", &specs, Some(new.clone())).await.0, 404);
    let (st, spec) = call(&base, "alice", "POST", &specs, Some(new.clone())).await;
    assert_eq!((st, &spec["kind"]), (201, &json!("agent")), "{spec}");
    assert_eq!(call(&base, "alice", "POST", &specs, Some(new)).await.0, 409, "a name once");
    for bad in [
        json!({ "name": "x", "workspace": "nope", "argv": ["ls"] }),
        json!({ "name": "-x", "workspace": "hw_a", "argv": ["ls"] }),
        json!({ "name": "x", "workspace": "hw_a" }),
        json!({ "name": "x", "workspace": "hw_a", "argv": ["ls"], "instruction": "both" }),
    ] {
        assert!(call(&base, "alice", "POST", &specs, Some(bad.clone())).await.0 >= 400, "{bad}");
    }
    let one = format!("{specs}/triage");
    let (st, v) = call(&base, "vic", "GET", &one, None).await;
    assert_eq!((st, &v["schedules"], &v["rules"]), (200, &json!([]), &json!([])), "a viewer reads: {v}");
    assert_eq!(call(&base, "olga", "GET", &one, None).await.0, 404);

    let (st, s) = call(
        &base,
        "bob",
        "POST",
        &format!("{one}/schedules"),
        Some(json!({ "cron": "0 9 * * 1-5", "timezone": "America/Chicago" })),
    )
    .await;
    assert_eq!((st, &s["enabled"], &s["timezone"]), (201, &json!(true), &json!("America/Chicago")), "{s}");
    let sid = s["id"].as_str().unwrap().to_owned();
    assert_eq!(
        app.db.platform_job(&sid).unwrap().unwrap().next_at,
        s["next_at"].as_u64().unwrap(),
        "its job is due at its tick"
    );
    for bad in [
        json!({ "cron": "* * * * *" }),
        json!({ "cron": "0 9 * * *", "timezone": "Nowhere/Here" }),
        json!({ "cron": "x" }),
    ] {
        assert_eq!(call(&base, "bob", "POST", &format!("{one}/schedules"), Some(bad.clone())).await.0, 422, "{bad}");
    }
    let (st, off) =
        call(&base, "bob", "PATCH", &format!("{one}/schedules/{sid}"), Some(json!({ "enabled": false }))).await;
    assert_eq!((st, &off["enabled"]), (200, &json!(false)), "{off}");
    assert_eq!(
        call(&base, "carol", "PATCH", &format!("{one}/schedules/{sid}"), Some(json!({ "enabled": true }))).await.0,
        403,
        "turning one on: on the list"
    );
    assert_eq!(call(&base, "vic", "DELETE", &format!("{one}/schedules/{sid}"), None).await.0, 403);
    assert_eq!(
        call(&base, "bob", "PATCH", &format!("{specs}/nope/schedules/{sid}"), Some(json!({ "enabled": true }))).await.0,
        404
    );

    let (st, v) =
        call(&base, "bob", "POST", &format!("{one}/rules"), Some(json!({ "event": "push", "repo": "acme/app" }))).await;
    assert_eq!(st, 422, "a repo the project hasn't linked: {v}");
    let (st, r) = call(
        &base,
        "bob",
        "POST",
        &format!("{one}/rules"),
        Some(json!({ "event": "check_failed", "branch": "release/*" })),
    )
    .await;
    assert_eq!((st, &r["event"], &r["branch"]), (201, &json!("check_failed"), &json!("release/*")), "{r}");
    assert_eq!(call(&base, "bob", "POST", &format!("{one}/rules"), Some(json!({ "event": "deployed" }))).await.0, 422);
    let rid = r["id"].as_str().unwrap();
    assert_eq!(
        call(&base, "bob", "PATCH", &format!("{one}/rules/{rid}"), Some(json!({ "enabled": false }))).await.1["enabled"],
        json!(false)
    );

    let (_, listed) = call(&base, "vic", "GET", &specs, None).await;
    assert_eq!(listed["specs"][0]["schedules"][0]["id"], json!(sid));
    assert_eq!(listed["specs"][0]["rules"][0]["id"], json!(rid));
    let (st, v) = call(&base, "bob", "PATCH", &one, Some(json!({ "name": "triage2", "argv": ["make"] }))).await;
    assert_eq!(
        (st, &v["name"], &v["kind"], &v["argv"]),
        (200, &json!("triage2"), &json!("run"), &json!(["make"])),
        "{v}"
    );
    assert_eq!(call(&base, "vic", "DELETE", &format!("{specs}/triage2"), None).await.0, 403);
    assert_eq!(call(&base, "bob", "DELETE", &format!("{specs}/triage2"), None).await.0, 204);
    assert!(app.db.run_schedule(&sid).unwrap().is_none());
    assert!(app.db.platform_job(&sid).unwrap().is_none());
}

fn push(branch: &str, sha: &str) -> Value {
    json!({ "ref": format!("refs/heads/{branch}"), "after": sha, "deleted": false, "pusher": { "name": "dev" },
        "repository": { "id": 77, "full_name": "acme/app" } })
}

/// What a webhook brings that a rule can match.
#[test]
fn events_a_rule_can_match() {
    let repo = json!({ "id": 77, "full_name": "acme/app" });
    let pr = json!({ "action": "opened", "repository": repo,
        "pull_request": { "number": 5, "title": "secret", "head": { "sha": "abc", "ref": "fix" }, "base": { "ref": "main" } } });
    let s = seen("pull_request", &pr).unwrap();
    assert_eq!(
        (s.event, s.branches.clone(), s.key.as_str()),
        (RuleEvent::PrOpened, vec!["main".to_owned()], "pr:77:5")
    );
    assert!(!s.detail.to_string().contains("secret"));
    let mut edited = pr.clone();
    edited["action"] = json!("edited");
    assert!(seen("pull_request", &edited).is_none());

    let run = |conclusion: &str| {
        json!({ "action": "completed", "repository": repo, "check_run": { "id": 900, "name": "ci", "head_sha": "abc",
            "conclusion": conclusion, "check_suite": { "head_branch": "main" } } })
    };
    let s = seen("check_run", &run("failure")).unwrap();
    assert_eq!(
        (s.event, s.key.as_str(), &s.detail["check"]),
        (RuleEvent::CheckFailed, "check_run:77:900", &json!("ci"))
    );
    assert!(seen("check_run", &run("timed_out")).is_some());
    assert!(seen("check_run", &run("success")).is_none());
    let status = json!({ "id": 31, "state": "error", "sha": "abc", "context": "ci/legacy", "repository": repo,
        "branches": [{ "name": "main" }, { "name": "release/1" }] });
    let s = seen("status", &status).unwrap();
    assert_eq!((s.key.as_str(), s.branches.len()), ("status:77:31", 2));

    let s = seen("push", &push("main", "def")).unwrap();
    assert_eq!((s.event, s.key.as_str()), (RuleEvent::Push, "push:77:main:def"));
    let mut tag = push("main", "def");
    tag["ref"] = json!("refs/tags/v1");
    assert!(seen("push", &tag).is_none());
    let mut gone = push("main", "def");
    gone["deleted"] = json!(true);
    assert!(seen("push", &gone).is_none());
    assert!(seen("issues", &json!({ "action": "opened", "repository": repo })).is_none());

    assert!(glob("main", "main") && !glob("main", "mainline"));
    assert!(glob("release/*", "release/1.0") && glob("release/*", "release/") && !glob("release/*", "hotfix/1"));
    assert!(glob("*", "") && glob("*-ci", "x-ci") && glob("a*b*c", "aXbYc") && !glob("a*b*c", "aXbY"));
}

/// `prj_a` with `acme/app` (77) linked, and spec `check` on `hw_a`.
fn linked() -> (Db, crate::db::triggers::RunSpec) {
    let db = Db::test();
    seed(&db);
    link(&db, 77, "acme/app", "prj_a");
    let spec = db.add_run_spec("prj_a", "check", "hw_a", &cmd(&["just", "smoke"]), "alice", 1).unwrap();
    (db, spec)
}

/// A linked repo's events start the specs whose rules match them, once
/// per event however often it's delivered; another branch, an unlinked
/// repo and a rule that's off start nothing.
#[tokio::test]
async fn an_event_starts_its_rules_runs_once() {
    let (db, spec) = linked();
    let rule = db.add_run_rule(&spec, RuleEvent::Push, Some("acme/app"), Some("main"), false, 20, "alice", 1).unwrap();
    let seen_main = seen("push", &push("main", "def")).unwrap();
    let fired = fire_open(&db, &seen_main, "d1", 10).await.unwrap();
    let [(r, Started::Made(run))] = &fired[..] else { panic!("{fired:?}") };
    assert_eq!((&r.id, run.trigger, run.started_by.as_str()), (&rule.id, TriggerKind::Event, rule.id.as_str()));
    assert_eq!(run.source["detail"], json!({ "branch": "main", "sha": "def", "pusher": "dev" }));
    assert_eq!(run.source["delivery"], json!("d1"));
    // Redelivered (or seen again under another delivery): no second run.
    let again = fire_open(&db, &seen_main, "d2", 11).await.unwrap();
    assert!(matches!(again[..], [(_, Started::Already(_))]), "{again:?}");
    assert_eq!(run_jobs(&db, &run.id), 1);
    assert!(fire_open(&db, &seen("push", &push("dev", "fed")).unwrap(), "d3", 12).await.unwrap().is_empty());
    let other = Seen { repo_id: 78, ..seen_main.clone() };
    assert!(fire_open(&db, &other, "d4", 12).await.unwrap().is_empty(), "not linked");
    db.set_run_rule(&rule.id, Some(false), None, None, 13).unwrap();
    assert!(fire_open(&db, &seen("push", &push("main", "123")).unwrap(), "d5", 14).await.unwrap().is_empty());
    assert_eq!(runs_in(&db, "prj_a").len(), 1);
}

fn opened(number: u64, association: &str, head_repo: u64) -> Value {
    json!({ "action": "opened", "repository": { "id": 77, "full_name": "acme/app" },
        "pull_request": { "number": number, "author_association": association,
            "head": { "sha": "abc", "ref": "main", "repo": { "id": head_repo, "full_name": "someone/app" } },
            "base": { "ref": "main" } } })
}

/// From outside the repo: a fork's PR, a PR by someone who isn't an owner,
/// member or collaborator, and a check run that doesn't say its head is in
/// the repo. Their branch names never match; inside events' do.
#[test]
fn what_counts_as_outside() {
    let s = seen("pull_request", &opened(5, "MEMBER", 77)).unwrap();
    assert!(!s.outside);
    assert_eq!((&s.detail["author_association"], &s.detail["fork"]), (&json!("MEMBER"), &json!(false)));
    for (assoc, head) in [("MEMBER", 900), ("CONTRIBUTOR", 77), ("NONE", 77), ("FIRST_TIME_CONTRIBUTOR", 900)] {
        assert!(seen("pull_request", &opened(5, assoc, head)).unwrap().outside, "{assoc} {head}");
    }
    let mut gone = opened(5, "OWNER", 77);
    gone["pull_request"]["head"]["repo"] = Value::Null;
    assert!(seen("pull_request", &gone).unwrap().outside, "a deleted fork");

    let check = |suite: Value, prs: Value| {
        json!({ "action": "completed", "repository": { "id": 77, "full_name": "acme/app" },
            "check_run": { "id": 1, "name": "ci", "head_sha": "abc", "conclusion": "failure", "check_suite": suite, "pull_requests": prs } })
    };
    // A fork's: it names its own branch `main`, and no PR.
    let fork = seen("check_run", &check(json!({ "head_branch": "main", "head_repository": { "id": 900 } }), json!([])))
        .unwrap();
    assert!(fork.outside && fork.branches.is_empty(), "{fork:?}");
    let unsaid = seen("check_run", &check(json!({ "head_branch": "main" }), json!([]))).unwrap();
    assert!(unsaid.outside && unsaid.branches.is_empty(), "{unsaid:?}");
    let ours = seen("check_run", &check(json!({ "head_branch": "main", "head_repository": { "id": 77 } }), json!([])))
        .unwrap();
    assert!(!ours.outside && ours.branches == ["main"], "{ours:?}");
    let pr = json!([{ "number": 4, "head": { "ref": "fix", "repo": { "id": 77 } }, "base": { "ref": "release/1", "repo": { "id": 77 } } }]);
    let by_pr = seen("check_run", &check(json!({ "head_branch": "fix" }), pr)).unwrap();
    assert!(!by_pr.outside && by_pr.branches == ["release/1", "fix"], "{by_pr:?}");
}

/// A rule refuses outsiders' events unless it takes them, and starts at
/// most its cap an hour; a redelivery past the cap is still the run it
/// made.
#[tokio::test]
async fn outsiders_and_the_hourly_cap() {
    let (db, spec) = linked();
    let strict = db.add_run_rule(&spec, RuleEvent::PrOpened, None, None, false, 2, "alice", 1).unwrap();
    let fork_pr = seen("pull_request", &opened(9, "NONE", 900)).unwrap();
    let fired = fire_open(&db, &fork_pr, "d0", 10).await.unwrap();
    assert!(matches!(fired[..], [(_, Started::Outsider)]), "{fired:?}");
    assert!(runs_in(&db, "prj_a").is_empty());
    db.set_run_rule(&strict.id, None, Some(true), None, 11).unwrap();
    let fired = fire_open(&db, &fork_pr, "d0", 12).await.unwrap();
    assert!(matches!(fired[..], [(_, Started::Made(_))]), "taken when the rule says so: {fired:?}");

    let t = 1_000_000;
    let first = seen("pull_request", &opened(1, "MEMBER", 77)).unwrap();
    assert!(matches!(fire_open(&db, &first, "d1", t).await.unwrap()[..], [(_, Started::Made(_))]));
    let third = seen("pull_request", &opened(3, "MEMBER", 77)).unwrap();
    assert!(matches!(fire_open(&db, &third, "d3", t + 1).await.unwrap()[..], [(_, Started::Capped)]), "two this hour");
    assert!(matches!(fire_open(&db, &first, "d1", t + 2).await.unwrap()[..], [(_, Started::Already(_))]));
    assert!(
        matches!(fire_open(&db, &third, "d3", t + 3_600_000).await.unwrap()[..], [(_, Started::Made(_))]),
        "an hour on"
    );
    assert_eq!(runs_in(&db, "prj_a").len(), 3);
}

/// A gate that fails for one maker (the hub didn't answer) and lets the
/// rest through.
struct FailsFor(&'static str);

impl Gate for FailsFor {
    fn refuses<'a>(
        &'a self,
        _: &'a str,
        maker: Option<&'a str>,
    ) -> BoxFuture<'a, anyhow::Result<Option<&'static str>>> {
        let fails = maker == Some(self.0);
        Box::pin(async move { if fails { anyhow::bail!("the hub is away") } else { Ok(None) } })
    }
}

/// One rule failing doesn't stop the next, and is said beside it.
#[tokio::test]
async fn a_failing_rule_doesnt_stop_the_rest() {
    let (db, spec) = linked();
    db.add_run_rule(&spec, RuleEvent::Push, None, None, false, 20, "mallory", 1).unwrap();
    db.add_run_rule(&spec, RuleEvent::Push, None, None, false, 20, "alice", 2).unwrap();
    let fired =
        events::fire(&db, &FailsFor("mallory"), &seen("push", &push("main", "a")).unwrap(), "d", 10).await.unwrap();
    assert_eq!(fired.len(), 2);
    assert!(fired[0].1.is_err());
    assert!(matches!(fired[1].1, Ok(Started::Made(_))), "{:?}", fired[1].1);
}

/// A schedule changed while a step holds its job (which read it before):
/// the step's hand-back makes the job due at once, and the next step reads
/// the change, clears the mark and waits for the new tick.
#[tokio::test]
async fn a_change_while_a_step_holds_the_job_isnt_lost() {
    let db = Db::test();
    seed(&db);
    let (_, s) = scheduled(&db, T0);
    db.set_run_schedule(&s, EVERY_10, "UTC", false, T0, 1).unwrap();
    let (_, c) = clock(T0);
    let mut job = db.claim_job(&[JobKind::Schedule], T0).unwrap().unwrap();
    // The step reads it off...
    let step = ticks().step(&db, &mut job, &c).await.unwrap();
    assert_eq!(step, Step::Again(T0 + schedule::IDLE_MS));
    // ...it's turned on as the step ends...
    db.set_run_schedule(&s, EVERY_10, "UTC", true, T0 + 600_000, T0).unwrap();
    // ...and the step hands it back with what it read.
    db.release_job(&job, T0 + schedule::IDLE_MS, T0).unwrap();
    let row = db.platform_job(&s).unwrap().unwrap();
    assert_eq!((row.next_at, row.cancel), (T0, true), "due at once");
    assert!(worker(&c).tick(&db).await.unwrap());
    let row = db.platform_job(&s).unwrap().unwrap();
    assert_eq!((row.next_at, row.cancel), (T0 + 600_000, false), "the new tick, the mark cleared");
    assert!(runs_in(&db, "prj_a").is_empty());
}

/// A schedule whose zone can't be read stops: its job ends `failed`, and
/// the schedule says so; changing it starts it again.
#[tokio::test]
async fn a_schedule_that_cant_be_read_says_it_stopped() {
    let db = Db::test();
    seed(&db);
    let (_, s) = scheduled(&db, T0);
    assert_eq!(ticks().max_attempts(), u32::MAX, "never given up on for failing");
    db.c().execute("UPDATE run_schedules SET timezone = 'Mars/Olympus' WHERE id = ?1", params![s]).unwrap();
    let (_, c) = clock(T0);
    assert!(worker(&c).tick(&db).await.unwrap());
    let read = db.run_schedule(&s).unwrap().unwrap();
    assert!(read.failed && read.enabled, "{read:?}");
    assert!(read.last_error.as_deref().unwrap().starts_with("stopped: no time zone"), "{read:?}");
    let again = db.set_run_schedule(&s, EVERY_10, "UTC", true, T0 + 600_000, T0).unwrap().unwrap();
    assert!(!again.failed && again.last_error.is_none(), "{again:?}");
    assert_eq!(db.platform_job(&s).unwrap().unwrap().state, "pending");
}

/// A schedule fires only while its maker may start runs on the project:
/// one made by someone off the beta's list, or who left the team, passes
/// its tick with no run, saying why.
#[tokio::test]
async fn a_schedule_stops_with_its_maker() {
    let (app, _base, p, hub) = serve_hub().await;
    let spec = app.db.add_run_spec(&p, "ci", "hw_a", &cmd(&["just", "test"]), "bob", 1).unwrap();
    let by_bob = app.db.add_run_schedule(&spec, EVERY_10, "UTC", T0, "bob", 1).unwrap();
    let by_carol = app.db.add_run_schedule(&spec, EVERY_10, "UTC", T0, "carol", 1).unwrap();
    let (t, c) = clock(T0);
    let w = Worker::new().clock(c.clone()).with(schedule::Ticks { gate: Arc::new(Members(app.clone())) });
    while w.tick(&app.db).await.unwrap() {}
    assert_eq!(runs_in(&app.db, &p).len(), 1, "bob's fires");
    let carol = app.db.run_schedule(&by_carol.id).unwrap().unwrap();
    assert_eq!(carol.last_error.as_deref(), Some("its maker isn't on the hosted beta's list"));
    assert_eq!(carol.next_at, T0 + 600_000, "the tick passed");
    hub.team("t", &[("alice", TeamRole::Owner)]);
    t.store(T0 + 600_000, SeqCst);
    while w.tick(&app.db).await.unwrap() {}
    assert_eq!(runs_in(&app.db, &p).len(), 1, "bob left: no run");
    let bob = app.db.run_schedule(&by_bob.id).unwrap().unwrap();
    assert_eq!(bob.last_error.as_deref(), Some("its maker isn't in the project any more"));
}

/// A PATCH that doesn't change the expression or zone keeps the next tick.
#[tokio::test]
async fn a_patch_keeps_the_next_tick() {
    let (_app, base, p) = serve().await;
    let specs = format!("/api/projects/{p}/run-specs");
    call(&base, "bob", "POST", &specs, Some(json!({ "name": "ci", "workspace": "hw_a", "argv": ["ls"] }))).await;
    let (_, s) =
        call(&base, "bob", "POST", &format!("{specs}/ci/schedules"), Some(json!({ "cron": "0 3 * * *" }))).await;
    let path = format!("{specs}/ci/schedules/{}", s["id"].as_str().unwrap());
    let (_, off) = call(&base, "bob", "PATCH", &path, Some(json!({ "enabled": false }))).await;
    let (_, on) = call(&base, "bob", "PATCH", &path, Some(json!({ "enabled": true, "timezone": "UTC" }))).await;
    assert_eq!((&off["next_at"], &on["next_at"]), (&s["next_at"], &s["next_at"]), "{s} {off} {on}");
    let (_, moved) = call(&base, "bob", "PATCH", &path, Some(json!({ "cron": "0 4 * * *" }))).await;
    assert_ne!(moved["next_at"], s["next_at"]);
}

/// Only an owner lets a rule take outsiders' events; a cap is 1 to 1000.
#[tokio::test]
async fn only_an_owner_takes_outsiders() {
    let (app, base, p) = serve().await;
    link(&app.db, 77, "acme/app", &p);
    let specs = format!("/api/projects/{p}/run-specs");
    call(&base, "bob", "POST", &specs, Some(json!({ "name": "ci", "workspace": "hw_a", "argv": ["ls"] }))).await;
    let rules = format!("{specs}/ci/rules");
    let open = json!({ "event": "pr_opened", "outsiders": true });
    assert_eq!(call(&base, "bob", "POST", &rules, Some(open.clone())).await.0, 403, "an editor");
    let (st, r) = call(&base, "alice", "POST", &rules, Some(open)).await;
    assert_eq!((st, &r["outsiders"], &r["max_per_hour"]), (201, &json!(true), &json!(20)), "{r}");
    let (st, strict) = call(&base, "bob", "POST", &rules, Some(json!({ "event": "push", "max_per_hour": 5 }))).await;
    assert_eq!((st, &strict["outsiders"], &strict["max_per_hour"]), (201, &json!(false), &json!(5)), "{strict}");
    let one = format!("{rules}/{}", strict["id"].as_str().unwrap());
    assert_eq!(call(&base, "bob", "PATCH", &one, Some(json!({ "outsiders": true }))).await.0, 403);
    for cap in [0, 1001] {
        assert_eq!(call(&base, "bob", "PATCH", &one, Some(json!({ "max_per_hour": cap }))).await.0, 422, "{cap}");
    }
    assert_eq!(call(&base, "bob", "PATCH", &one, Some(json!({ "max_per_hour": 7 }))).await.1["max_per_hour"], json!(7));
}
