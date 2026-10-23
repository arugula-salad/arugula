//! The job worker (M208), on two machines sharing one database: a crash
//! mid-step resumes from the checkpoint, a long step keeps its job with
//! heartbeats, a step whose lease is lost is dropped, a stalled machine
//! writes nothing once another took over, a panic or an error backs off,
//! a cancel reaches a running step, and each kind goes to its handler. On
//! SQLite and, with `ARUGULA_TEST_POSTGRES_URL`, on Postgres.
use super::*;
use crate::db::jobs::{RETRY_MS, tests::two_machines};
use serde_json::json;
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering::SeqCst},
};
use tokio::sync::Notify;

/// A test's clock, moved by hand.
fn clock(at: u64) -> (Arc<AtomicU64>, Clock) {
    let t = Arc::new(AtomicU64::new(at));
    let read = t.clone();
    (t, Clock::new(move || read.load(SeqCst)))
}

fn worker(c: &Clock) -> Worker {
    Worker::new().clock(c.clone()).heartbeat(Duration::from_millis(10))
}

/// A kind whose step does an effect once (as a create at a provider would
/// be), winning the state change before it and checkpointing after, then
/// finishes.
struct Make {
    kind: JobKind,
    effects: Arc<Mutex<Vec<String>>>,
    /// The machine dies right after the effect, holding the job.
    die_after_effect: bool,
}

impl Handler for Make {
    fn kind(&self) -> JobKind {
        self.kind
    }

    fn step<'a>(&'a self, db: &'a Db, job: &'a mut LeasedJob, clock: &'a Clock) -> BoxFuture<'a, Result<Step>> {
        Box::pin(async move {
            if job.cancelled() {
                return Ok(Step::Cancelled);
            }
            let stage = job.checkpoint.as_ref().and_then(|c| c["stage"].as_str()).unwrap_or("").to_string();
            match stage.as_str() {
                "" => {
                    job.checkpoint = Some(json!({"stage": "making"}));
                    db.checkpoint_job(job, clock.now())?;
                    self.effects.lock().unwrap().push(format!("made {}", job.id));
                    job.checkpoint = Some(json!({"stage": "made"}));
                    db.checkpoint_job(job, clock.now())?;
                    if self.die_after_effect {
                        std::future::pending::<()>().await;
                    }
                    Ok(Step::Again(clock.now()))
                }
                "made" => {
                    self.effects.lock().unwrap().push(format!("finished {}", job.id));
                    Ok(Step::Done)
                }
                other => anyhow::bail!("unexpected stage {other}"),
            }
        })
    }
}

fn make(kind: JobKind, effects: &Arc<Mutex<Vec<String>>>, die_after_effect: bool) -> Make {
    Make { kind, effects: effects.clone(), die_after_effect }
}

/// For any kind: a machine that dies mid-step leaves the job held until its lease runs
/// out, then the other machine carries on from the checkpoint without
/// repeating the effect.
#[tokio::test]
async fn a_machine_that_dies_mid_step_is_resumed_from_its_checkpoint() {
    let (a, b, _file) = two_machines();
    let effects = Arc::new(Mutex::new(Vec::new()));
    let (t, c) = clock(0);
    let dying = worker(&c).heartbeat(Duration::from_secs(3600)).with(make(JobKind::Run, &effects, true));
    let other = worker(&c).with(make(JobKind::Run, &effects, false));
    assert!(a.enqueue_job("run_1", JobKind::Run, &json!({}), 0, 0).unwrap());
    let died = tokio::time::timeout(Duration::from_millis(200), dying.tick(&a)).await;
    assert!(died.is_err(), "the machine died mid-step");
    t.store(LEASE_MS - 1, SeqCst);
    assert!(!other.tick(&b).await.unwrap(), "held until the lease runs out");
    t.store(LEASE_MS, SeqCst);
    assert!(other.tick(&b).await.unwrap());
    let row = b.platform_job("run_1").unwrap().unwrap();
    assert_eq!((row.state.as_str(), row.checkpoint), ("done", Some(json!({"stage": "made"}))));
    assert_eq!(*effects.lock().unwrap(), ["made run_1", "finished run_1"], "the effect happened once");
}

/// A kind that waits to be let go mid-step, then makes its effect and
/// writes its machine's name. It returns `Cancelled` if it hears of a
/// cancel while it waits.
struct Stall {
    machine: &'static str,
    claimed: Arc<Notify>,
    go: Arc<Notify>,
    effects: Arc<Mutex<Vec<String>>>,
}

impl Handler for Stall {
    fn kind(&self) -> JobKind {
        JobKind::Deploy
    }

    fn step<'a>(&'a self, _: &'a Db, job: &'a mut LeasedJob, clock: &'a Clock) -> BoxFuture<'a, Result<Step>> {
        Box::pin(async move {
            self.claimed.notify_one();
            loop {
                tokio::select! {
                    _ = self.go.notified() => break,
                    _ = tokio::time::sleep(Duration::from_millis(5)) => if job.cancelled() {
                        return Ok(Step::Cancelled);
                    },
                }
            }
            self.effects.lock().unwrap().push(format!("deployed by {}", self.machine));
            job.checkpoint = Some(json!({"by": self.machine}));
            Ok(if self.machine == "b" { Step::Done } else { Step::Again(clock.now() + 1) })
        })
    }
}

fn stall(machine: &'static str, effects: &Arc<Mutex<Vec<String>>>) -> (Stall, Arc<Notify>, Arc<Notify>) {
    let (claimed, go) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    (Stall { machine, claimed: claimed.clone(), go: go.clone(), effects: effects.clone() }, claimed, go)
}

/// `stale_worker_cannot_overwrite_reclaimed_job`'s, through the worker: a
/// machine whose heartbeats stopped (its clock jumped past the lease, as a
/// stall does) while the other took the job over and finished it drops its
/// step: it makes no effect and writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_step_whose_lease_is_lost_is_dropped() {
    let (a, b, _file) = two_machines();
    let effects = Arc::new(Mutex::new(Vec::new()));
    let (t, c) = clock(0);
    let (on_a, claimed, _go) = stall("a", &effects);
    let stalled = worker(&c).with(on_a);
    let (on_b, _, b_go) = stall("b", &effects);
    let other = worker(&c).with(on_b);
    assert!(a.enqueue_job("dep_1", JobKind::Deploy, &json!({}), 0, 0).unwrap());
    let a = Arc::new(a);
    let first = tokio::spawn({
        let a = a.clone();
        async move { stalled.tick(&a).await }
    });
    claimed.notified().await;
    t.store(LEASE_MS, SeqCst);
    b_go.notify_one();
    assert!(other.tick(&b).await.unwrap(), "the lease ran out: b takes it over");
    let dropped = tokio::time::timeout(Duration::from_secs(5), first).await;
    assert!(dropped.expect("a's heartbeat found the lease lost").unwrap().unwrap());
    let row = b.platform_job("dep_1").unwrap().unwrap();
    assert_eq!((row.state.as_str(), row.checkpoint), ("done", Some(json!({"by": "b"}))));
    assert_eq!(*effects.lock().unwrap(), ["deployed by b"], "a never deployed");
}

/// A step longer than the lease keeps its job: its heartbeats hold it, so
/// the other machine can't take it over and deploy again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_long_step_keeps_its_job_with_heartbeats() {
    let (a, b, _file) = two_machines();
    let effects = Arc::new(Mutex::new(Vec::new()));
    let (t, c) = clock(0);
    let (on_a, claimed, go) = stall("a", &effects);
    let long = worker(&c).with(on_a);
    assert!(a.enqueue_job("dep_1", JobKind::Deploy, &json!({}), 0, 0).unwrap());
    let a = Arc::new(a);
    let first = tokio::spawn({
        let a = a.clone();
        async move { long.tick(&a).await }
    });
    claimed.notified().await;
    // Three minutes, a minute at a time, each with time for heartbeats.
    for minute in 1..=3 {
        t.store(minute * 60_000, SeqCst);
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(b.claim_job(&JobKind::ALL, minute * 60_000).unwrap().is_none(), "still held at {minute} min");
    }
    go.notify_one();
    assert!(first.await.unwrap().unwrap());
    let row = b.platform_job("dep_1").unwrap().unwrap();
    assert_eq!((row.checkpoint, row.next_at), (Some(json!({"by": "a"})), 180_001));
    assert_eq!(*effects.lock().unwrap(), ["deployed by a"]);
}

/// A cancel asked for while a step runs reaches it by the next heartbeat.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancel_reaches_a_running_step() {
    let db = Arc::new(Db::test());
    let effects = Arc::new(Mutex::new(Vec::new()));
    let (_, c) = clock(0);
    let (h, claimed, _go) = stall("a", &effects);
    let w = worker(&c).with(h);
    db.enqueue_job("dep_1", JobKind::Deploy, &json!({}), 0, 0).unwrap();
    let running = tokio::spawn({
        let db = db.clone();
        async move { w.tick(&db).await }
    });
    claimed.notified().await;
    assert!(db.cancel_job("dep_1", 1).unwrap());
    let done = tokio::time::timeout(Duration::from_secs(5), running).await;
    assert!(done.expect("the step heard of the cancel").unwrap().unwrap());
    assert_eq!(db.platform_job("dep_1").unwrap().unwrap().state, "cancelled");
    assert!(effects.lock().unwrap().is_empty());
}

/// A kind whose step fails: an error, or a panic.
struct Broken {
    panics: bool,
}

impl Handler for Broken {
    fn kind(&self) -> JobKind {
        JobKind::Schedule
    }

    fn max_attempts(&self) -> u32 {
        2
    }

    fn step<'a>(&'a self, _: &'a Db, job: &'a mut LeasedJob, _: &'a Clock) -> BoxFuture<'a, Result<Step>> {
        Box::pin(async move {
            // Half changed, and not to be written: only an error's job is.
            job.checkpoint = Some(json!({"half": true}));
            if self.panics {
                panic!("the provider said no");
            }
            anyhow::bail!("the provider said no")
        })
    }
}

/// A failing step, by error or by panic, is retried after a back-off, then
/// the job ends `failed` with its error; the worker carries on.
#[tokio::test]
async fn a_failing_or_panicking_step_backs_off_then_fails_the_job() {
    for panics in [false, true] {
        let db = Db::test();
        let (t, c) = clock(0);
        let w = worker(&c).with(Broken { panics });
        db.enqueue_job("sch_1", JobKind::Schedule, &json!({}), 0, 0).unwrap();
        assert!(w.tick(&db).await.unwrap(), "the worker carries on");
        let row = db.platform_job("sch_1").unwrap().unwrap();
        let error = if panics { "the step panicked: the provider said no" } else { "the provider said no" };
        assert_eq!((row.state.as_str(), row.error.as_deref(), row.next_at), ("pending", Some(error), RETRY_MS));
        assert_eq!(row.checkpoint, (!panics).then(|| json!({"half": true})), "a panicked step's job isn't written");
        t.store(RETRY_MS - 1, SeqCst);
        assert!(!w.tick(&db).await.unwrap(), "backing off");
        t.store(RETRY_MS, SeqCst);
        assert!(w.tick(&db).await.unwrap());
        assert_eq!(db.platform_job("sch_1").unwrap().unwrap().state, "failed");
        t.store(u64::MAX / 4, SeqCst);
        assert!(!w.tick(&db).await.unwrap());
    }
}

/// Each job goes to its kind's handler, a kind with none here is left for
/// a machine that has one, and a cancel reaches the handler.
#[tokio::test]
async fn jobs_go_to_their_kinds_handler() {
    let db = Db::test();
    let effects = Arc::new(Mutex::new(Vec::new()));
    let (t, c) = clock(1);
    let w = worker(&c).with(make(JobKind::Run, &effects, false)).with(make(JobKind::Workspace, &effects, false));
    assert_eq!(w.kinds(), [JobKind::Workspace, JobKind::Run]);
    let jobs =
        [("run_1", JobKind::Run), ("ws_1", JobKind::Workspace), ("dep_1", JobKind::Deploy), ("run_2", JobKind::Run)];
    for (created, (id, kind)) in (0..).zip(jobs) {
        db.enqueue_job(id, kind, &json!({}), 0, created).unwrap();
    }
    assert!(db.cancel_job("run_2", 1).unwrap());
    while w.tick(&db).await.unwrap() {
        t.fetch_add(1, SeqCst);
    }
    assert_eq!(*effects.lock().unwrap(), ["made run_1", "made ws_1", "finished run_1", "finished ws_1"]);
    let state = |id| db.platform_job(id).unwrap().unwrap().state;
    assert_eq!(
        [state("run_1"), state("ws_1"), state("run_2"), state("dep_1")],
        ["done", "done", "cancelled", "pending"]
    );
}

/// A kind whose step hangs while its heartbeats go on.
struct Hang;

impl Handler for Hang {
    fn kind(&self) -> JobKind {
        JobKind::Run
    }

    fn max_attempts(&self) -> u32 {
        2
    }

    fn deadline(&self) -> u64 {
        60_000
    }

    fn step<'a>(&'a self, _: &'a Db, job: &'a mut LeasedJob, _: &'a Clock) -> BoxFuture<'a, Result<Step>> {
        Box::pin(async move {
            job.checkpoint = Some(json!({"half": true}));
            std::future::pending().await
        })
    }
}

/// A step that hangs past its kind's deadline is dropped and counts as a
/// failed attempt: it backs off, keeping what it last wrote, until its
/// last attempt ends the job. Before the deadline its heartbeats hold it.
#[tokio::test]
async fn a_step_past_its_deadline_is_dropped_and_fails_the_attempt() {
    let (a, b, _file) = two_machines();
    let (t, c) = clock(0);
    let w = worker(&c).with(Hang);
    a.enqueue_job("run_1", JobKind::Run, &json!({}), 0, 0).unwrap();
    let mut started = 0;
    for attempt in 1..=2 {
        let move_clock = async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            t.store(started + 59_999, SeqCst);
            tokio::time::sleep(Duration::from_millis(30)).await;
            assert!(b.claim_job(&JobKind::ALL, started + 59_999).unwrap().is_none(), "held until the deadline");
            t.store(started + 60_000, SeqCst);
        };
        let (ticked, ()) = tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(w.tick(&a), move_clock) })
            .await
            .expect("the hung step was dropped");
        assert!(ticked.unwrap());
        let row = a.platform_job("run_1").unwrap().unwrap();
        assert_eq!(row.error.as_deref(), Some("step passed its deadline of 1 min"));
        assert_eq!(row.checkpoint, None, "the hung step's half-changed job isn't written");
        if attempt == 1 {
            assert_eq!((row.state.as_str(), row.next_at), ("pending", 60_000 + RETRY_MS));
            started = 60_000 + RETRY_MS;
            t.store(started, SeqCst);
        } else {
            assert_eq!(row.state, "failed", "its last attempt ends it");
        }
    }
    assert_eq!(super::span(30 * 60_000), "30 min");
    assert_eq!(super::span(90_000), "90 s");
}
