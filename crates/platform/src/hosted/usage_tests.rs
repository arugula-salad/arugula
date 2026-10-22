//! Usage (M188): the interval accounting on its own (lost events,
//! overlaps, pause and resume cycles), the cost, and the rows through the
//! worker's checkpoint and the reconcile, on SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use super::*;
use crate::{
    db::{Db, hosted::Added},
    hosted::{Ask, Spec},
};

const SMALL: Class = Class { cpus: 2, memory_mb: 2048 };

fn obs(sandbox: Option<&str>, running: bool, at: u64) -> Observed {
    Observed { sandbox: sandbox.map(str::to_owned), running: Some(running), seen: Seen::default(), at }
}

/// Apply observations in turn to one workspace's intervals, as `meter`
/// does: its closed intervals and its open one.
fn run(observations: &[Observed]) -> (Vec<(u64, u64)>, Option<Open>) {
    let (mut closed, mut open): (Vec<(u64, u64)>, Option<Open>) = (Vec::new(), None);
    for o in observations {
        let last_end = closed.iter().map(|c| c.1).max().unwrap_or(0);
        for c in changes(open.as_ref(), last_end, o) {
            match c {
                Change::Close { started_at, ended_at } => {
                    assert_eq!(open.take().map(|o| o.started_at), Some(started_at));
                    closed.push((started_at, ended_at));
                }
                Change::Drop { started_at } => assert_eq!(open.take().map(|o| o.started_at), Some(started_at)),
                Change::Open { sandbox, started_at, class } => {
                    assert!(open.is_none(), "one open interval");
                    open = Some(Open { sandbox, started_at, class });
                }
                Change::Size { class, .. } => open.as_mut().unwrap().class = Some(class),
            }
        }
    }
    // Never overlapping, never backwards.
    let mut all = closed.clone();
    all.extend(open.iter().map(|o| (o.started_at, u64::MAX)));
    all.sort();
    for w in all.windows(2) {
        assert!(w[0].1 <= w[1].0, "{all:?} overlap");
    }
    assert!(closed.iter().all(|(s, e)| s < e), "{closed:?}");
    (closed, open)
}

fn total(closed: &[(u64, u64)]) -> u64 {
    closed.iter().map(|(s, e)| e - s).sum()
}

/// Made, paused, woken, paused, woken, deleted: three intervals, and the
/// same observation twice changes nothing.
#[test]
fn pause_and_resume_cycles() {
    let (closed, open) = run(&[
        obs(Some("sbx"), true, 1_000),
        obs(Some("sbx"), true, 61_000),
        obs(Some("sbx"), false, 100_000),
        obs(Some("sbx"), false, 700_000),
        obs(Some("sbx"), true, 800_000),
        obs(Some("sbx"), false, 900_000),
        obs(Some("sbx"), true, 1_000_000),
        obs(None, false, 1_050_000),
    ]);
    assert_eq!(closed, [(1_000, 100_000), (800_000, 900_000), (1_000_000, 1_050_000)]);
    assert!(open.is_none());
    assert_eq!(total(&closed), 249_000);
}

/// The box paused itself at its timeout while the worker was away (or the
/// pause's checkpoint was lost): the next status read closes it at the
/// time the provider says it paused, not when it was seen.
#[test]
fn a_lost_pause_closes_at_the_providers_time() {
    let mut late = obs(Some("sbx"), false, 5_000_000);
    late.seen.paused_at = Some(601_000);
    let (closed, _) = run(&[obs(Some("sbx"), true, 1_000), late]);
    assert_eq!(closed, [(1_000, 601_000)]);

    // A pause time after it was seen (a clock ahead) is taken as seen, and
    // one before it started as nothing having run.
    let mut ahead = obs(Some("sbx"), false, 50_000);
    ahead.seen.paused_at = Some(90_000);
    assert_eq!(run(&[obs(Some("sbx"), true, 1_000), ahead]).0, [(1_000, 50_000)]);
    let mut before = obs(Some("sbx"), false, 50_000);
    before.seen.paused_at = Some(10);
    assert_eq!(run(&[obs(Some("sbx"), true, 1_000), before]), (vec![], None));
}

/// A wake whose checkpoint was lost: the next read finds it running and
/// opens at the provider's start, but never before the last interval
/// ended, so nothing is counted twice.
#[test]
fn a_lost_wake_opens_without_overlap() {
    let mut found = obs(Some("sbx"), true, 900_000);
    found.seen.started_at = Some(700_000);
    let (closed, open) = run(&[obs(Some("sbx"), true, 0), obs(Some("sbx"), false, 100_000), found]);
    assert_eq!(closed, [(0, 100_000)]);
    assert_eq!(open.unwrap().started_at, 700_000);

    // A provider that reports the box's first start: from the last end.
    let mut first = obs(Some("sbx"), true, 900_000);
    first.seen.started_at = Some(0);
    let (_, open) = run(&[obs(Some("sbx"), true, 0), obs(Some("sbx"), false, 100_000), first]);
    assert_eq!(open.unwrap().started_at, 100_000);
}

/// Overlaps can't happen: another box in the same workspace (a lost
/// create's, found by its metadata) closes the first at once; a start time
/// before the last end is moved up to it; a step that says nothing (a
/// delete asked but not done) changes nothing.
#[test]
fn intervals_never_overlap() {
    let (closed, open) = run(&[obs(Some("a"), true, 0), obs(Some("b"), true, 10_000)]);
    assert_eq!(closed, [(0, 10_000)]);
    assert_eq!(open.as_ref().map(|o| (o.sandbox.as_str(), o.started_at)), Some(("b", 10_000)));

    let mut early = obs(Some("a"), true, 50_000);
    early.seen.started_at = Some(5_000);
    let (closed, open) = run(&[obs(Some("a"), true, 0), obs(Some("a"), false, 20_000), early]);
    assert_eq!((closed, open.unwrap().started_at), (vec![(0, 20_000)], 20_000));

    let quiet = Observed { running: None, ..obs(None, false, 30_000) };
    let (closed, open) = run(&[obs(Some("a"), true, 0), quiet]);
    assert!(closed.is_empty() && open.is_some());
}

/// A box's size is taken once the provider says it.
#[test]
fn the_size_is_filled_in() {
    let mut sized = obs(Some("sbx"), true, 60_000);
    sized.seen.class = Some(SMALL);
    let (_, open) = run(&[obs(Some("sbx"), true, 0), sized]);
    assert_eq!(open.unwrap().class, Some(SMALL));
}

/// An interval's part of a window, an open one running to its end.
#[test]
fn active_time_in_a_window() {
    assert_eq!(active_ms(0, Some(100), 50, 200), 50);
    assert_eq!(active_ms(60, None, 50, 200), 140);
    assert_eq!(active_ms(0, Some(40), 50, 200), 0);
    assert_eq!(active_ms(300, None, 50, 200), 0);
}

/// E2B's prices: 2 vCPU and 2 GiB is 3,600 × (2 × 0.000014 + 2 ×
/// 0.0000045) = $0.1332 an hour; the blended rate weighs each size by its
/// hours, and a box of no known size costs as E2B's default.
#[test]
fn cost_per_active_hour() {
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(close(PRICES.per_hour(SMALL), 0.1332), "{}", PRICES.per_hour(SMALL));
    assert!(close(PRICES.per_hour(Class { cpus: 2, memory_mb: 4096 }), 0.1656));
    assert!(close(PRICES.cost(SMALL, 1_800_000), 0.0666));
    let big = Class { cpus: 8, memory_mb: 8192 };
    let intervals = [
        Interval {
            workspace: "hw_1".into(),
            project: "prj_a".into(),
            class: Some(SMALL),
            started_at: 0,
            ended_at: Some(3_600_000),
            purpose: "workspace".into(),
        },
        Interval {
            workspace: "hw_2".into(),
            project: "prj_b".into(),
            class: Some(big),
            started_at: 0,
            ended_at: None,
            purpose: "workspace".into(),
        },
        Interval {
            workspace: "hw_3".into(),
            project: "prj_b".into(),
            class: None,
            started_at: 0,
            ended_at: Some(3_600_000),
            purpose: "deploy".into(),
        },
    ];
    let r = report(&intervals, vec![], 0, 3_600_000);
    // 0.1332 + 3,600 × (8 × 0.000014 + 8 × 0.0000045) + 0.1656
    assert!(close(r.all.cost(), 0.1332 + 0.5328 + 0.1656), "{}", r.all.cost());
    assert!(close(r.all.per_active_hour().unwrap(), (0.1332 + 0.5328 + 0.1656) / 3.0));
    assert_eq!(r.projects["prj_b"].workspaces.len(), 2);
    // M219: a deploy's box is counted, and counted apart too.
    assert_eq!(r.deploys.workspaces.iter().collect::<Vec<_>>(), ["hw_3"]);
    assert!(close(r.deploys.cost(), 0.1656), "{}", r.deploys.cost());
    assert_eq!(r.projects["prj_a"].total_ms(), 3_600_000);
    assert_eq!(Totals::default().per_active_hour(), None);
    let text = r.render("platform.db", 7);
    assert!(text.contains("3 workspaces ran 3.00 hours: $0.83, $0.2772 per active hour"), "{text}");
    assert!(text.contains("size not seen"), "{text}");
}

fn workspace(db: &Db, id: &str, project: &str) {
    let spec = Spec::default();
    assert!(matches!(db.add_hosted_workspace(id, project, id, &spec, "alice", 10, 0).unwrap(), Added::Made(_)));
}

/// One worker step's checkpoint: the box in `state`, metered.
fn step(db: &Db, id: &str, state: &str, sandbox: Option<&str>, seen: Seen, at: u64) {
    {
        use crate::db::sql::{Sql as _, params};
        let c = db.c();
        c.execute(
            "UPDATE hosted_workspaces SET lease_until = CASE WHEN id = ?1 THEN 0 ELSE 1000000000000000 END",
            params![id],
        )
        .unwrap();
    }
    let mut job = db.claim_hosted(1).unwrap().expect("the workspace to step");
    assert_eq!(job.w.id, id);
    job.w.state = state.into();
    job.sandbox = sandbox.map(str::to_owned);
    job.stage = if sandbox.is_some() { "made".into() } else { String::new() };
    if state == "deleted" {
        job.deleted_at = Some(at);
    }
    job.seen = seen;
    job.usage = Some(observed(&job, at));
    db.checkpoint_hosted(&job, None::<Ask>, 0, at).unwrap();
}

fn rows(db: &Db, id: &str) -> Vec<(u64, Option<u64>, Option<u32>)> {
    use crate::db::sql::{Sql as _, params};
    db.c()
        .query_map(
            "SELECT started_at, ended_at, cpus FROM hosted_usage WHERE workspace_id = ?1 ORDER BY started_at",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}

/// Through the checkpoint: made, enrolled, its size seen, paused by its
/// timeout behind the worker's back, woken, deleted. A deleted workspace's
/// usage stays (it's what was billed), and the report and the provider's
/// record read it back.
#[test]
fn checkpoints_meter_the_box() {
    let db = Db::test();
    workspace(&db, "hw_1", "prj_a");
    workspace(&db, "hw_2", "prj_b");
    let h = 3_600_000;
    step(&db, "hw_1", "enrolling", Some("sbx1"), Seen::default(), h);
    step(&db, "hw_1", "ready", Some("sbx1"), Seen { class: Some(SMALL), ..Default::default() }, h + 60_000);
    assert_eq!(rows(&db, "hw_1"), [(h, None, Some(2))]);
    step(&db, "hw_1", "paused", Some("sbx1"), Seen { paused_at: Some(2 * h), ..Default::default() }, 5 * h);
    step(&db, "hw_1", "paused", Some("sbx1"), Seen::default(), 6 * h);
    step(&db, "hw_1", "ready", Some("sbx1"), Seen::default(), 7 * h);
    step(&db, "hw_1", "deleted", None, Seen::default(), 8 * h);
    assert_eq!(rows(&db, "hw_1"), [(h, Some(2 * h), Some(2)), (7 * h, Some(8 * h), None)]);
    // A delete asked and not yet done says nothing.
    step(&db, "hw_2", "ready", Some("sbx2"), Seen::default(), h);
    step(&db, "hw_2", "deleting", Some("sbx2"), Seen::default(), 2 * h);
    assert_eq!(rows(&db, "hw_2"), [(h, None, None)]);

    let r = report(&db.hosted_usage(0, 3 * h).unwrap(), db.hosted_usage_checked(0, 3 * h).unwrap(), 0, 3 * h);
    assert_eq!(r.projects["prj_a"].total_ms(), h);
    assert_eq!(r.projects["prj_b"].total_ms(), 2 * h, "open: to the window's end");

    // The provider's record: only boxes that stopped a while ago, and
    // again only after they run and stop again.
    let due = db.hosted_usage_due(8 * h + SETTLE_MS, SETTLE_MS, 10).unwrap();
    assert_eq!(due, [("sbx1".to_owned(), "hw_1".to_owned(), "prj_a".to_owned())]);
    assert!(db.hosted_usage_due(8 * h + 1, SETTLE_MS, 10).unwrap().is_empty(), "not settled");
    let runs = [
        Run { started_at: h, ms: h + 2_000, cpus: 2, memory_mb: 2048 },
        Run { started_at: 7 * h, ms: h, cpus: 2, memory_mb: 2048 },
    ];
    db.set_hosted_usage_provider("sbx1", "hw_1", "prj_a", Some(&runs), 9 * h).unwrap();
    assert!(db.hosted_usage_due(10 * h, SETTLE_MS, 10).unwrap().is_empty(), "read since");
    let checked = db.hosted_usage_checked(0, 3 * h).unwrap();
    assert_eq!(
        checked,
        [Checked {
            workspace: "hw_1".into(),
            provider_ms: 2 * h + 2_000,
            provider_cpu_ms: 2 * (2 * h + 2_000),
            provider_mib_ms: 2048 * (2 * h + 2_000),
            metered_ms: 2 * h,
        }]
    );
    let text = report(&db.hosted_usage(0, 3 * h).unwrap(), checked, 0, 3 * h).render("x", 7);
    assert!(text.contains("1 boxes checked against E2B's record"), "{text}");
    assert!(text.contains("(-0.0%)"), "{text}");

    // No record at the provider (loam): kept as none, not asked again.
    db.set_hosted_usage_provider("sbx1", "hw_1", "prj_a", None, 9 * h).unwrap();
    assert!(db.hosted_usage_checked(0, 3 * h).unwrap().is_empty());
    assert!(db.hosted_usage_due(10 * h, SETTLE_MS, 10).unwrap().is_empty());
}

/// `arugula-platform usage --db`: read-only, from a file or Postgres (the
/// test's own schema, when there's one).
#[test]
fn the_ops_command_reads_either_database() {
    use crate::window::portable::{Place, usage};
    let db = Db::test();
    workspace(&db, "hw_1", "prj_a");
    step(&db, "hw_1", "ready", Some("sbx1"), Seen { class: Some(SMALL), ..Default::default() }, 0);
    step(&db, "hw_1", "paused", Some("sbx1"), Seen::default(), 1_800_000);
    let place = match db.test_url() {
        Some(url) => Place::Postgres(url),
        None => {
            // A file of its own: the in-memory one isn't reachable by path.
            let path =
                std::env::temp_dir().join(format!("arugula-usage-{}.db", hex::encode(arugula_e2e::random::<8>())));
            let file = Db::open(&path).unwrap();
            workspace(&file, "hw_1", "prj_a");
            step(&file, "hw_1", "ready", Some("sbx1"), Seen { class: Some(SMALL), ..Default::default() }, 0);
            step(&file, "hw_1", "paused", Some("sbx1"), Seen::default(), 1_800_000);
            drop(file);
            Place::File(path)
        }
    };
    let r = usage(&place, 0, 3_600_000).unwrap();
    assert_eq!(r.all.total_ms(), 1_800_000);
    assert!(r.render("db", 7).contains("$0.07, $0.1332 per active hour"), "{}", r.render("db", 7));
    if let Place::File(path) = place {
        let _ = std::fs::remove_file(&path);
    }
}
