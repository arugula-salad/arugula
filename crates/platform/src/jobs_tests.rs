//! The job worker (M208), on two machines sharing one database: a crash
//! mid-step resumes from the checkpoint, a stalled machine writes nothing
//! once another took over, failures back off, and each kind goes to its
//! handler. On SQLite and, with `ARUGULA_TEST_POSTGRES_URL`, on Postgres.
use super::*;
use crate::db::jobs::{RETRY_MS, tests::two_machines};
use serde_json::json;
use std::sync::Mutex;
use tokio::sync::Notify;

/// A kind whose step does an effect once (as a create at a provider would
/// be), checkpointing before and after it, then finishes.
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

    fn step<'a>(&'a self, db: &'a Db, job: &'a mut LeasedJob, now: u64) -> BoxFuture<'a, Result<Step>> {
        Box::pin(async move {
            if job.cancel {
                return Ok(Step::Cancelled);
            }
            let stage = job.checkpoint.as_ref().and_then(|c| c["stage"].as_str()).unwrap_or("").to_string();
            match stage.as_str() {
                "" => {
                    job.checkpoint = Some(json!({"stage": "making"}));
                    db.checkpoint_job(job, now)?;
                    self.effects.lock().unwrap().push(format!("made {}", job.id));
                    job.checkpoint = Some(json!({"stage": "made"}));
                    db.checkpoint_job(job, now)?;
                    if self.die_after_effect {
                        std::future::pending::<()>().await;
                    }
                    Ok(Step::Again(now))
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

/// `cloud_workspace_delete_survives_a_restart_part_way`'s, for any kind: a
/// machine that dies mid-step leaves the job held until its lease runs
/// out, then the other machine carries on from the checkpoint without
/// repeating the effect.
#[tokio::test]
async fn a_machine_that_dies_mid_step_is_resumed_from_its_checkpoint() {
    let (a, b, _file) = two_machines();
    let effects = Arc::new(Mutex::new(Vec::new()));
    let dying = Worker::new().with(make(JobKind::Run, &effects, true));
    let other = Worker::new().with(make(JobKind::Run, &effects, false));
    assert!(a.enqueue_job("run_1", JobKind::Run, &json!({}), 0, 0).unwrap());
    let died = tokio::time::timeout(std::time::Duration::from_millis(200), dying.tick(&a, 0)).await;
    assert!(died.is_err(), "the machine died mid-step");
    assert!(!other.tick(&b, LEASE_MS - 1).await.unwrap(), "held until the lease runs out");
    assert!(other.tick(&b, LEASE_MS).await.unwrap());
    let row = b.platform_job("run_1").unwrap().unwrap();
    assert_eq!((row.state.as_str(), row.checkpoint), ("done", Some(json!({"stage": "made"}))));
    assert_eq!(*effects.lock().unwrap(), ["made run_1", "finished run_1"], "the effect happened once");
}

/// A kind that waits to be let go mid-step, then writes its machine's name.
struct Stall {
    machine: &'static str,
    claimed: Arc<Notify>,
    go: Arc<Notify>,
}

impl Handler for Stall {
    fn kind(&self) -> JobKind {
        JobKind::Deploy
    }

    fn step<'a>(&'a self, _: &'a Db, job: &'a mut LeasedJob, now: u64) -> BoxFuture<'a, Result<Step>> {
        Box::pin(async move {
            self.claimed.notify_one();
            self.go.notified().await;
            job.checkpoint = Some(json!({"by": self.machine}));
            Ok(if self.machine == "b" { Step::Done } else { Step::Again(now + 1) })
        })
    }
}

/// `stale_worker_cannot_overwrite_reclaimed_job`'s, through the worker: a
/// machine that stalled past its lease while the other took the job over
/// and finished it writes nothing when it wakes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stalled_machine_writes_nothing_once_another_took_over() {
    let (a, b, _file) = two_machines();
    let (claimed, go) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let stalled = Worker::new().with(Stall { machine: "a", claimed: claimed.clone(), go: go.clone() });
    let (b_claimed, b_go) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let other = Worker::new().with(Stall { machine: "b", claimed: b_claimed, go: b_go.clone() });
    assert!(a.enqueue_job("dep_1", JobKind::Deploy, &json!({}), 0, 0).unwrap());
    let a = Arc::new(a);
    let first = tokio::spawn({
        let a = a.clone();
        async move { stalled.tick(&a, 0).await }
    });
    claimed.notified().await;
    b_go.notify_one();
    assert!(other.tick(&b, LEASE_MS).await.unwrap(), "the lease ran out: b takes it over");
    go.notify_one();
    assert!(first.await.unwrap().unwrap(), "a lost lease isn't the worker's error");
    let row = b.platform_job("dep_1").unwrap().unwrap();
    assert_eq!((row.state.as_str(), row.checkpoint), ("done", Some(json!({"by": "b"}))));
}

/// A kind whose step always fails.
struct Broken;

impl Handler for Broken {
    fn kind(&self) -> JobKind {
        JobKind::Schedule
    }

    fn max_attempts(&self) -> u32 {
        2
    }

    fn step<'a>(&'a self, _: &'a Db, _: &'a mut LeasedJob, _: u64) -> BoxFuture<'a, Result<Step>> {
        Box::pin(async { anyhow::bail!("the provider said no") })
    }
}

/// A failing step is retried after a back-off, then the job ends `failed`
/// with its error.
#[tokio::test]
async fn a_failing_step_backs_off_then_fails_the_job() {
    let db = Db::test();
    let worker = Worker::new().with(Broken);
    db.enqueue_job("sch_1", JobKind::Schedule, &json!({}), 0, 0).unwrap();
    assert!(worker.tick(&db, 0).await.unwrap());
    let row = db.platform_job("sch_1").unwrap().unwrap();
    assert_eq!(
        (row.state.as_str(), row.error.as_deref(), row.next_at),
        ("pending", Some("the provider said no"), RETRY_MS)
    );
    assert!(!worker.tick(&db, RETRY_MS - 1).await.unwrap(), "backing off");
    assert!(worker.tick(&db, RETRY_MS).await.unwrap());
    assert_eq!(db.platform_job("sch_1").unwrap().unwrap().state, "failed");
    assert!(!worker.tick(&db, u64::MAX / 2).await.unwrap());
}

/// Each job goes to its kind's handler, a kind with none here is left for
/// a machine that has one, and a cancel reaches the handler.
#[tokio::test]
async fn jobs_go_to_their_kinds_handler() {
    let db = Db::test();
    let effects = Arc::new(Mutex::new(Vec::new()));
    let worker =
        Worker::new().with(make(JobKind::Run, &effects, false)).with(make(JobKind::Workspace, &effects, false));
    assert_eq!(worker.kinds(), [JobKind::Workspace, JobKind::Run]);
    let jobs =
        [("run_1", JobKind::Run), ("ws_1", JobKind::Workspace), ("dep_1", JobKind::Deploy), ("run_2", JobKind::Run)];
    for (created, (id, kind)) in (0..).zip(jobs) {
        db.enqueue_job(id, kind, &json!({}), 0, created).unwrap();
    }
    assert!(db.cancel_job("run_2", 1).unwrap());
    let mut now = 1;
    while worker.tick(&db, now).await.unwrap() {
        now += 1;
    }
    assert_eq!(*effects.lock().unwrap(), ["made run_1", "made ws_1", "finished run_1", "finished ws_1"]);
    let state = |id| db.platform_job(id).unwrap().unwrap().state;
    assert_eq!(
        [state("run_1"), state("ws_1"), state("run_2"), state("dep_1")],
        ["done", "done", "cancelled", "pending"]
    );
}
