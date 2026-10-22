//! The live suite (M176): the same calls against E2B's cloud and loam's
//! E2B-compatible API, with only the URL and key changed. Ignored unless
//! asked for, since it makes real boxes:
//!
//! ```sh
//! ARUGULA_TEST_E2B_URL=https://api.e2b.dev ARUGULA_TEST_E2B_KEY=... \
//!   cargo test -p arugula-platform --lib compute::live_tests -- --ignored --nocapture
//! ```
//!
//! loam's E2B API is `https://api.e2b.<loam domain>`, with a loam project
//! key (`https://api.<loam domain>` is loam's own API, which isn't E2B's).
//! `ARUGULA_TEST_E2B_TEMPLATE` picks the template (`base` unless set).
//! Every box carries `arugula_test=1` and the run's id, has a short
//! timeout, and is deleted at the end whatever happened, with a listing
//! that checks none is left. Each check is printed and the suite fails at
//! the end if any did, so one run shows every difference (M191).

use std::{
    collections::BTreeMap,
    panic::AssertUnwindSafe,
    time::{Duration, Instant},
};

use futures_util::FutureExt as _;

use super::*;

struct Run {
    checks: Vec<(String, bool, String)>,
    timings: Vec<(String, Duration)>,
}

impl Run {
    fn check(&mut self, name: &str, ok: bool, detail: impl std::fmt::Debug) {
        let detail = format!("{detail:?}");
        println!("{} {name}: {detail}", if ok { "ok  " } else { "FAIL" });
        self.checks.push((name.into(), ok, detail));
    }

    async fn time<T>(&mut self, name: &str, f: impl Future<Output = T>) -> T {
        let at = Instant::now();
        let out = f.await;
        let took = at.elapsed();
        println!("time {name}: {} ms", took.as_millis());
        self.timings.push((name.into(), took));
        out
    }
}

#[tokio::test]
#[ignore = "makes real boxes: set ARUGULA_TEST_E2B_URL and ARUGULA_TEST_E2B_KEY"]
async fn live_suite() {
    let (Ok(url), Ok(key)) = (std::env::var("ARUGULA_TEST_E2B_URL"), std::env::var("ARUGULA_TEST_E2B_KEY")) else {
        panic!("set ARUGULA_TEST_E2B_URL and ARUGULA_TEST_E2B_KEY");
    };
    let template = std::env::var("ARUGULA_TEST_E2B_TEMPLATE").unwrap_or_else(|_| "base".into());
    let e2b = E2b::new(&url, key).unwrap();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let run_id = format!("m176-{:x}{:x}", nanos, std::process::id());
    let tag: BTreeMap<String, String> =
        [("arugula_test".into(), "1".into()), ("arugula_run".into(), run_id.clone())].into();
    println!("run {run_id} against {url}, template {template}");

    let mut run = Run { checks: vec![], timings: vec![] };
    let made = std::sync::Mutex::new(Vec::<String>::new());
    let body = AssertUnwindSafe(suite(&e2b, &template, &tag, &mut run, &made)).catch_unwind().await;

    // Clean up whatever happened: every box made, then any the run's
    // metadata still finds.
    let mut left = made.lock().unwrap().clone();
    if let Ok(found) = e2b.list(&tag).await {
        left.extend(found.into_iter().map(|b| b.id));
    }
    left.sort();
    left.dedup();
    for id in &left {
        let gone = e2b.delete(id).await;
        println!("cleanup delete {id}: {gone:?}");
    }
    let after = e2b.list(&tag).await;
    println!("cleanup list: {after:?}");
    assert!(after.as_ref().is_ok_and(Vec::is_empty), "boxes left behind: {after:?}");

    println!("\ntimings:");
    for (name, took) in &run.timings {
        println!("  {name}: {} ms", took.as_millis());
    }
    let failed: Vec<_> = run.checks.iter().filter(|c| !c.1).collect();
    println!("{} checks, {} failed", run.checks.len(), failed.len());
    if let Err(panic) = body {
        std::panic::resume_unwind(panic);
    }
    assert!(failed.is_empty(), "failed: {failed:#?}");
}

/// A listing's ids and states.
fn ids(l: &Result<Vec<Sandbox>>) -> Result<Vec<(String, State)>> {
    l.clone().map(|l| l.into_iter().map(|b| (b.id, b.state)).collect())
}

async fn suite(
    e2b: &E2b,
    template: &str,
    tag: &BTreeMap<String, String>,
    run: &mut Run,
    made: &std::sync::Mutex<Vec<String>>,
) {
    let mut metadata = tag.clone();
    metadata.insert("arugula_workspace".into(), "hw_live".into());
    let create = Create {
        template: template.into(),
        env: [("ARUGULA_TEST_ENV".into(), "1".into())].into(),
        metadata: metadata.clone(),
        timeout: Duration::from_secs(120),
    };

    // A second box with a short timeout, to see it pause (not die) when
    // the timeout passes; looked at near the end.
    let mut short = create.clone();
    short.timeout = Duration::from_secs(10);
    short.metadata.insert("arugula_workspace".into(), "hw_short".into());
    let short_at = Instant::now();
    let short = e2b.create(&short).await;
    run.check("create (10 s timeout)", short.is_ok(), &short);
    if let Ok(s) = &short {
        made.lock().unwrap().push(s.id.clone());
    }

    let created = run.time("create", e2b.create(&create)).await;
    run.check("create", created.is_ok(), &created);
    let id = created.expect("create").id;
    made.lock().unwrap().push(id.clone());

    let s = e2b.status(&id).await;
    run.check("status: running", matches!(&s, Ok(Some(b)) if b.state == State::Running), &s);
    run.check("status: pauses on timeout", matches!(&s, Ok(Some(b)) if b.pauses_on_timeout == Some(true)), &s);
    run.check("status: metadata", matches!(&s, Ok(Some(b)) if b.metadata == metadata), &s);
    let end_before = s.ok().flatten().map(|b| b.end_at).unwrap_or_default();

    let mut only: BTreeMap<String, String> = tag.clone();
    only.insert("arugula_workspace".into(), "hw_live".into());
    let listed = run.time("list", e2b.list(&only)).await;
    run.check("list by metadata: just this box", ids(&listed) == Ok(vec![(id.clone(), State::Running)]), ids(&listed));
    let both = e2b.list(tag).await;
    run.check("list by the run's tag: both boxes", both.as_ref().is_ok_and(|l| l.len() == 2), ids(&both));

    // E2B's timestamps are to the second at best: wait so a hold moves it.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let held = run.time("hold", e2b.hold(&id, Duration::from_secs(180))).await;
    run.check("hold", held.is_ok(), &held);
    let s = e2b.status(&id).await;
    let end_after = s.as_ref().ok().and_then(|b| b.as_ref()).map(|b| b.end_at.clone()).unwrap_or_default();
    run.check("hold moves endAt later", end_after > end_before, (&end_before, &end_after));

    let resumed = e2b.resume(&id, Duration::from_secs(120)).await;
    run.check("resume a running box: only extends", resumed == Ok(Resumed::Running), &resumed);

    let paused = run.time("pause", e2b.pause(&id)).await;
    run.check("pause", paused == Ok(Paused::Paused), &paused);
    let again = e2b.pause(&id).await;
    run.check("pause again: already", again == Ok(Paused::Already), &again);

    for n in 1..=2 {
        let s = run.time("status (paused)", e2b.status(&id)).await;
        run.check(
            &format!("status #{n} shows paused, not woken"),
            matches!(&s, Ok(Some(b)) if b.state == State::Paused),
            &s,
        );
    }
    let listed = e2b.list(&only).await;
    run.check("list shows it paused", ids(&listed) == Ok(vec![(id.clone(), State::Paused)]), ids(&listed));
    let held = e2b.hold(&id, Duration::from_secs(60)).await;
    run.check("hold on a paused box: refused, not woken", held.is_err(), &held);
    let s = e2b.status(&id).await;
    run.check("still paused after the hold", matches!(&s, Ok(Some(b)) if b.state == State::Paused), &s);

    let resumed = run.time("resume", e2b.resume(&id, Duration::from_secs(120))).await;
    run.check("resume", resumed == Ok(Resumed::Woke), &resumed);
    let s = e2b.status(&id).await;
    run.check("status: running after resume", matches!(&s, Ok(Some(b)) if b.state == State::Running), &s);
    run.check("status: metadata kept through pause", matches!(&s, Ok(Some(b)) if b.metadata == metadata), &s);

    let paused = run.time("pause (second)", e2b.pause(&id)).await;
    run.check("pause again after resume", paused == Ok(Paused::Paused), &paused);

    let deleted = run.time("delete", e2b.delete(&id)).await;
    run.check("delete (paused box)", deleted.is_ok(), &deleted);
    let s = e2b.status(&id).await;
    run.check("status: gone", s == Ok(None), &s);
    let listed = e2b.list(&only).await;
    run.check("list shows it gone", listed.as_ref().is_ok_and(Vec::is_empty), ids(&listed));
    let deleted = e2b.delete(&id).await;
    run.check("delete again: fine", deleted.is_ok(), &deleted);
    let resumed = e2b.resume(&id, Duration::from_secs(60)).await;
    run.check("resume a deleted box: not found", resumed == Err(Error::NotFound), &resumed);

    if let Ok(short) = short {
        let wait = Duration::from_secs(30).saturating_sub(short_at.elapsed());
        tokio::time::sleep(wait).await;
        let s = e2b.status(&short.id).await;
        run.check("10 s timeout passed: paused, not gone", matches!(&s, Ok(Some(b)) if b.state == State::Paused), &s);
        let resumed = run.time("resume (auto-paused)", e2b.resume(&short.id, Duration::from_secs(60))).await;
        run.check("resume an auto-paused box", resumed == Ok(Resumed::Woke), &resumed);
    }
}

/// M188's reconcile source, live: a box made and paused shows one run in
/// its lifecycle events (E2B's lag them by seconds), with its size; loam
/// keeps none. The box is deleted whatever happened.
#[tokio::test]
#[ignore = "makes a real box: set ARUGULA_TEST_E2B_URL and ARUGULA_TEST_E2B_KEY"]
async fn live_runs() {
    let (Ok(url), Ok(key)) = (std::env::var("ARUGULA_TEST_E2B_URL"), std::env::var("ARUGULA_TEST_E2B_KEY")) else {
        panic!("set ARUGULA_TEST_E2B_URL and ARUGULA_TEST_E2B_KEY");
    };
    let template = std::env::var("ARUGULA_TEST_E2B_TEMPLATE").unwrap_or_else(|_| "base".into());
    let e2b = E2b::new(&url, key).unwrap();
    let run_id = format!("m188-{:x}", arugula_e2e::now_ms());
    let tag: BTreeMap<String, String> = [("arugula_test".into(), "1".into()), ("arugula_run".into(), run_id)].into();
    let made = e2b
        .create(&Create { template, env: BTreeMap::new(), metadata: tag.clone(), timeout: Duration::from_secs(60) })
        .await
        .unwrap();
    let body = AssertUnwindSafe(async {
        let size = e2b.status(&made.id).await.unwrap().unwrap();
        println!("size: {:?} vCPU, {:?} MiB", size.cpus, size.memory_mb);
        tokio::time::sleep(Duration::from_secs(2)).await;
        e2b.pause(&made.id).await.unwrap();
        // E2B answers 404 ("no events") until its first event lands.
        let mut runs = None;
        for _ in 0..30 {
            runs = e2b.runs(&made.id).await.unwrap();
            if runs.as_ref().is_some_and(|r| !r.is_empty()) {
                break;
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        println!("runs: {runs:?}");
        if url.contains("e2b.dev") {
            assert!(runs.is_some(), "E2B keeps lifecycle events");
        }
        if let Some(runs) = runs {
            assert_eq!(runs.len(), 1);
            assert!(runs[0].ms >= 1_000, "{runs:?}");
            assert_eq!((Some(runs[0].cpus), Some(runs[0].memory_mb)), (size.cpus, size.memory_mb));
        }
    })
    .catch_unwind()
    .await;
    e2b.delete(&made.id).await.unwrap();
    assert!(e2b.list(&tag).await.unwrap().is_empty(), "a box left");
    if let Err(p) = body {
        std::panic::resume_unwind(p);
    }
}

/// How long a resume takes (M182's wake on connect): one box, paused and
/// resumed `ARUGULA_TEST_E2B_TRIES` times (20 unless set), each resume
/// timed from the ask to the provider's answer that the box runs. The
/// box's own daemon dials in after that (M180: about 0.14 s), which a
/// `base` box doesn't have. `ARUGULA_TEST_E2B_PAUSED_SECS` keeps it paused
/// that long each time. Prints the median, p95 and worst.
///
/// ```sh
/// ARUGULA_TEST_E2B_URL=https://api.e2b.dev ARUGULA_TEST_E2B_KEY=... \
///   cargo test -p arugula-platform --lib compute::live_tests::resume_latency -- --ignored --nocapture
/// ```
#[tokio::test]
#[ignore = "makes a real box: set ARUGULA_TEST_E2B_URL and ARUGULA_TEST_E2B_KEY"]
async fn resume_latency() {
    let (Ok(url), Ok(key)) = (std::env::var("ARUGULA_TEST_E2B_URL"), std::env::var("ARUGULA_TEST_E2B_KEY")) else {
        panic!("set ARUGULA_TEST_E2B_URL and ARUGULA_TEST_E2B_KEY");
    };
    let template = std::env::var("ARUGULA_TEST_E2B_TEMPLATE").unwrap_or_else(|_| "base".into());
    let tries: usize = std::env::var("ARUGULA_TEST_E2B_TRIES").ok().and_then(|n| n.parse().ok()).unwrap_or(20);
    let asleep: u64 = std::env::var("ARUGULA_TEST_E2B_PAUSED_SECS").ok().and_then(|n| n.parse().ok()).unwrap_or(0);
    let e2b = E2b::new(&url, key).unwrap();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let tag: BTreeMap<String, String> =
        [("arugula_test".into(), "1".into()), ("arugula_run".into(), format!("m182-{nanos:x}"))].into();
    let made = e2b
        .create(&Create { template, env: BTreeMap::new(), metadata: tag.clone(), timeout: Duration::from_secs(120) })
        .await
        .unwrap();
    let body = AssertUnwindSafe(async {
        let (mut resumes, mut pauses) = (vec![], vec![]);
        for i in 0..tries {
            let at = Instant::now();
            assert_eq!(e2b.pause(&made.id).await.unwrap(), Paused::Paused, "try {i}");
            pauses.push(at.elapsed());
            tokio::time::sleep(Duration::from_secs(asleep)).await;
            let at = Instant::now();
            assert_eq!(e2b.resume(&made.id, Duration::from_secs(120)).await.unwrap(), Resumed::Woke, "try {i}");
            resumes.push(at.elapsed());
            println!("try {i}: pause {} ms, resume {} ms", pauses[i].as_millis(), resumes[i].as_millis());
        }
        let line = |name: &str, v: &mut Vec<Duration>| {
            v.sort();
            let at = |q: f64| v[((v.len() as f64 * q).ceil() as usize).clamp(1, v.len()) - 1].as_millis();
            println!("{name} over {}: median {} ms, p95 {} ms, worst {} ms", v.len(), at(0.5), at(0.95), at(1.0));
        };
        line("resume", &mut resumes);
        line("pause", &mut pauses);
    })
    .catch_unwind()
    .await;
    let _ = e2b.delete(&made.id).await;
    for b in e2b.list(&tag).await.unwrap_or_default() {
        let _ = e2b.delete(&b.id).await;
    }
    assert!(e2b.list(&tag).await.unwrap().is_empty(), "a box was left behind");
    if let Err(p) = body {
        std::panic::resume_unwind(p);
    }
}
