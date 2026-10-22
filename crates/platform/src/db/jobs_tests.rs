//! `platform_jobs`' lease (M208): `cloud_jobs`' lease tests in `db::cloud`,
//! ported, plus the back-off, heartbeat and cancel. Each runs on SQLite and,
//! with `ARUGULA_TEST_POSTGRES_URL`, on Postgres.
use super::*;
use serde_json::json;

fn queue(db: &Db, id: &str, kind: JobKind, at: u64) {
    assert!(db.enqueue_job(id, kind, &json!({"for": id}), at, at).unwrap());
}

/// Two handles on one database, as two machines have (M144): on Postgres
/// the test's schema twice; on SQLite a file opened twice (removed when
/// the guard drops).
pub(crate) fn two_machines() -> (Db, Db, Option<Removed>) {
    let db = Db::test();
    if let Some(other) = db.second() {
        return (db, other, None);
    }
    let path = std::env::temp_dir().join(format!("arugula-jobs-{}.db", hex::encode(arugula_e2e::random::<8>())));
    (Db::open(&path).unwrap(), Db::open(&path).unwrap(), Some(Removed(path)))
}

pub(crate) struct Removed(std::path::PathBuf);

impl Drop for Removed {
    fn drop(&mut self) {
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", self.0.display()));
        }
    }
}

/// `stale_worker_cannot_overwrite_reclaimed_job`'s: a holder whose lease
/// ran out and was taken over can't write, and the new holder can.
#[test]
fn a_stale_holder_cannot_write_over_the_job_once_reclaimed() {
    let db = Db::test();
    queue(&db, "run_1", JobKind::Run, 0);
    let mut first = db.claim_job(&[JobKind::Run], 0).unwrap().unwrap();
    assert_eq!((first.attempts, first.payload.clone()), (1, json!({"for": "run_1"})));
    assert!(db.claim_job(&[JobKind::Run], 100).unwrap().is_none(), "leased");
    let second = db.claim_job(&[JobKind::Run], LEASE_MS).unwrap().unwrap();
    assert_eq!(first.id, second.id);
    assert_ne!(first.lease, second.lease);
    first.checkpoint = Some(json!({"stage": "stale"}));
    for refused in [
        db.checkpoint_job(&first, 1),
        db.release_job(&first, 0, 1),
        db.extend_job(&first, 1 << 40, 1),
        db.finish_job(&first, JobEnd::Done, None, 1),
        db.fail_job(&first, "x", 5, 1).map(|_| ()),
    ] {
        assert!(refused.unwrap_err().is::<LeaseLost>());
    }
    {
        let mut c = db.c();
        let tx = c.immediate().unwrap();
        assert!(!holds(&tx, &first).unwrap());
        assert!(holds(&tx, &second).unwrap());
    }
    assert_eq!(db.platform_job("run_1").unwrap().unwrap().checkpoint, None, "the stale write didn't land");
    db.release_job(&second, 130_000, 2).unwrap();
    assert!(db.claim_job(&[JobKind::Run], 129_999).unwrap().is_none(), "not due yet");
    assert_eq!(db.claim_job(&[JobKind::Run], 130_000).unwrap().unwrap().id, "run_1");
}

/// A holder whose lease ran out may still write until another claim takes
/// the job, as with `cloud_jobs`: only a claim replaces a lease.
#[test]
fn a_late_holder_writes_until_someone_takes_over() {
    let db = Db::test();
    queue(&db, "dep_1", JobKind::Deploy, 0);
    let mut job = db.claim_job(&[JobKind::Deploy], 0).unwrap().unwrap();
    job.checkpoint = Some(json!({"stage": "late"}));
    db.checkpoint_job(&job, LEASE_MS + 5).unwrap();
    let taken = db.claim_job(&[JobKind::Deploy], LEASE_MS + 10).unwrap().unwrap();
    assert_eq!(taken.checkpoint, Some(json!({"stage": "late"})), "the next holder carries on from it");
    assert!(db.checkpoint_job(&job, LEASE_MS + 11).unwrap_err().is::<LeaseLost>());
}

/// `two_machines_never_claim_the_same_job`'s: two machines' workers, four
/// each, claiming at once take every job exactly once, and none again
/// until its lease runs out.
#[test]
fn two_machines_never_claim_the_same_job() {
    let (a, b, _file) = two_machines();
    let mut want = std::collections::BTreeSet::new();
    for i in 0..12u64 {
        for kind in JobKind::ALL {
            let id = format!("{}_{i}", kind.as_str());
            queue(&a, &id, kind, i);
            want.insert(id);
        }
    }
    let claimed = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for db in [&a, &b] {
            for _ in 0..4 {
                scope.spawn(|| {
                    while let Some(job) = db.claim_job(&JobKind::ALL, 1_000).unwrap() {
                        claimed.lock().unwrap().push(job.id);
                    }
                });
            }
        }
    });
    let claimed = claimed.into_inner().unwrap();
    let unique: std::collections::BTreeSet<String> = claimed.iter().cloned().collect();
    assert_eq!(unique.len(), claimed.len(), "a job was claimed twice");
    assert_eq!(unique, want, "every job, once");
    // Leased: nobody takes them again until the lease runs out.
    assert!(b.claim_job(&JobKind::ALL, 1_000 + LEASE_MS - 1).unwrap().is_none());
    assert!(b.claim_job(&JobKind::ALL, 1_000 + LEASE_MS).unwrap().is_some());
}

/// `a_claim_passes_over_a_job_another_holds`'s: on Postgres a claim passes
/// over a job another machine's claim is holding, rather than waiting for
/// it (`SKIP LOCKED`).
#[test]
fn a_claim_passes_over_a_job_another_holds() {
    let (a, b, _) = two_machines();
    if a.dialect() != Dialect::Postgres {
        return;
    }
    queue(&a, "one", JobKind::Workspace, 0);
    queue(&a, "two", JobKind::Workspace, 1);
    let mut c = a.c();
    let held = c.transaction().unwrap();
    held.execute(&format!("SELECT id FROM platform_jobs WHERE id=?1{}", held.for_update()), params!["one"]).unwrap();
    let b = std::sync::Arc::new(b);
    let (tx, rx) = std::sync::mpsc::channel();
    let claimer = b.clone();
    std::thread::spawn(move || {
        let one = claimer.claim_job(&JobKind::ALL, 10).unwrap().map(|j| j.id);
        let two = claimer.claim_job(&JobKind::ALL, 10).unwrap().map(|j| j.id);
        let _ = tx.send((one, two));
    });
    let claimed = rx.recv_timeout(std::time::Duration::from_secs(5));
    drop(held);
    let (one, two) = claimed.expect("the claim waited for the held job instead of passing over it");
    assert_eq!(one.as_deref(), Some("two"), "the one nobody holds");
    assert_eq!(two, None, "the held one is passed over");
    assert_eq!(b.claim_job(&JobKind::ALL, 10).unwrap().unwrap().id, "one");
}

/// A claim takes only the kinds it asks for, the longest waiting first,
/// and nothing before it's due.
#[test]
fn a_claim_takes_its_kinds_in_order_when_due() {
    let db = Db::test();
    queue(&db, "late", JobKind::Run, 50);
    queue(&db, "early", JobKind::Run, 10);
    queue(&db, "sched", JobKind::Schedule, 0);
    assert!(db.claim_job(&[], 100).unwrap().is_none());
    assert!(db.claim_job(&[JobKind::Run], 9).unwrap().is_none());
    assert_eq!(db.claim_job(&[JobKind::Run], 100).unwrap().unwrap().id, "early");
    assert_eq!(db.claim_job(&[JobKind::Run], 100).unwrap().unwrap().id, "late");
    assert!(db.claim_job(&[JobKind::Run, JobKind::Deploy], 100).unwrap().is_none());
    assert_eq!(db.claim_job(&[JobKind::Schedule], 100).unwrap().unwrap().id, "sched");
    assert!(!db.enqueue_job("early", JobKind::Run, &json!({}), 0, 0).unwrap(), "one job per id");
}

/// Failures back off, doubling, and end the job at its last attempt; a
/// clean hand-back starts the count over.
#[test]
fn failures_back_off_then_end_the_job() {
    assert_eq!([backoff(1), backoff(2), backoff(3)], [RETRY_MS, 2 * RETRY_MS, 4 * RETRY_MS]);
    assert_eq!(backoff(40), RETRY_MAX_MS);
    let db = Db::test();
    queue(&db, "run_1", JobKind::Run, 0);
    let job = db.claim_job(&[JobKind::Run], 0).unwrap().unwrap();
    assert!(!db.fail_job(&job, "first", 3, 0).unwrap());
    let row = db.platform_job("run_1").unwrap().unwrap();
    assert_eq!((row.state.as_str(), row.error.as_deref(), row.next_at), ("pending", Some("first"), RETRY_MS));
    assert!(db.claim_job(&[JobKind::Run], RETRY_MS - 1).unwrap().is_none(), "backing off");
    let job = db.claim_job(&[JobKind::Run], RETRY_MS).unwrap().unwrap();
    assert_eq!(job.attempts, 2);
    db.release_job(&job, RETRY_MS, RETRY_MS).unwrap();
    let row = db.platform_job("run_1").unwrap().unwrap();
    assert_eq!((row.attempts, row.error), (0, None), "a clean hand-back starts over");
    let mut now = RETRY_MS;
    for attempt in 1..=3 {
        let job = db.claim_job(&[JobKind::Run], now).unwrap().unwrap();
        assert_eq!(job.attempts, attempt);
        assert_eq!(db.fail_job(&job, "again", 3, now).unwrap(), attempt == 3);
        now += backoff(attempt);
    }
    let row = db.platform_job("run_1").unwrap().unwrap();
    assert_eq!((row.state.as_str(), row.error.as_deref()), ("failed", Some("again")));
    assert!(db.claim_job(&[JobKind::Run], u64::MAX / 2).unwrap().is_none(), "ended");
}

/// A heartbeat keeps the job past its first lease; a finished job is
/// never claimed or written again.
#[test]
fn a_heartbeat_holds_the_job_and_a_finished_one_stays_finished() {
    let db = Db::test();
    queue(&db, "dep_1", JobKind::Deploy, 0);
    let mut job = db.claim_job(&[JobKind::Deploy], 0).unwrap().unwrap();
    db.extend_job(&job, 3 * LEASE_MS, 1).unwrap();
    assert!(db.claim_job(&[JobKind::Deploy], 2 * LEASE_MS).unwrap().is_none(), "still held");
    job.checkpoint = Some(json!({"applied": true}));
    db.finish_job(&job, JobEnd::Done, None, 2).unwrap();
    let row = db.platform_job("dep_1").unwrap().unwrap();
    assert_eq!((row.state.as_str(), row.checkpoint), ("done", Some(json!({"applied": true}))));
    assert!(db.claim_job(&[JobKind::Deploy], 4 * LEASE_MS).unwrap().is_none());
    assert!(db.checkpoint_job(&job, 3).unwrap_err().is::<LeaseLost>());
    assert!(!db.cancel_job("dep_1", 3).unwrap(), "it has ended");
}

/// A cancel makes the job due at once, and its next claim sees it.
#[test]
fn a_cancel_reaches_the_next_claim() {
    let db = Db::test();
    queue(&db, "sch_1", JobKind::Schedule, 1_000_000);
    assert!(db.claim_job(&[JobKind::Schedule], 10).unwrap().is_none());
    assert!(db.cancel_job("sch_1", 10).unwrap());
    let job = db.claim_job(&[JobKind::Schedule], 10).unwrap().unwrap();
    assert!(job.cancel);
    db.finish_job(&job, JobEnd::Cancelled, None, 11).unwrap();
    assert_eq!(db.platform_job("sch_1").unwrap().unwrap().state, "cancelled");
}
