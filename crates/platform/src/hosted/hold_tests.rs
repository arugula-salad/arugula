//! Pausing only between turns (M181): the hold's cadence, and the leader
//! and the worker against a fake substrate and the test hub's activity:
//! busy keeps a box held, idle and silent let it go, a pause is never
//! made mid-turn, and the provider's runtime cap is met between turns (or
//! said, when a turn runs to it). Runs on SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::{collections::HashSet, sync::Arc, time::Duration};

use arugula_e2e::now_ms;

use super::*;
use crate::{
    chat_tests::TestHub,
    hosted::{
        Ask, EnrollBy,
        tests::{Fake, drive, make, serve},
    },
};

const HOUR: u64 = 3_600_000;

/// E2B's time for `ms`.
fn iso(ms: u64) -> String {
    let s = ms / 1000;
    let (days, rem) = ((s / 86_400) as i64, s % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, ms % 1000)
}

fn running(w: &str, m: &str) -> Running {
    Running { workspace: w.into(), machine: m.into(), sandbox: format!("sbx-{w}") }
}

fn names(r: &[Running]) -> Vec<&str> {
    r.iter().map(|b| b.workspace.as_str()).collect()
}

/// A busy box is held at once, then every third of the idle timeout (so
/// two holds can fail before it would pause); an idle one is forgotten, and
/// held at once when it's busy again.
#[test]
fn busy_boxes_are_held_on_a_cadence_that_cant_lapse() {
    let idle = Duration::from_secs(600);
    let every = every(idle);
    assert_eq!(every, 200_000);
    assert!(every * 3 <= idle.as_millis() as u64);
    assert_eq!(super::every(Duration::from_secs(6)), TICK.as_millis() as u64, "never more often than a tick");

    let mut h = Holds::default();
    let boxes = [running("w1", "m1"), running("w2", "m2")];
    let busy = HashSet::from(["m1".to_owned()]);
    assert_eq!(names(&h.due(&boxes, &busy, every, 0)), ["w1"]);
    h.held("w1", 0);
    assert!(h.due(&boxes, &busy, every, 5_000).is_empty());
    assert!(h.due(&boxes, &busy, every, 199_999).is_empty());
    assert_eq!(names(&h.due(&boxes, &busy, every, 200_000)), ["w1"]);
    h.held("w1", 200_000);
    // Idle: let go, and held at once when it's busy again.
    assert!(h.due(&boxes, &HashSet::new(), every, 210_000).is_empty());
    assert_eq!(names(&h.due(&boxes, &busy, every, 215_000)), ["w1"]);
    h.held("w1", 215_000);
    // A box that's no longer running is forgotten too.
    assert!(h.due(&boxes[1..], &busy, every, 216_000).is_empty());
    assert_eq!(names(&h.due(&boxes, &busy, every, 217_000)), ["w1"]);
}

/// E2B Pro's cap, the window before it in which a box is paused and
/// resumed between turns, and what counts as a pause at the cap.
#[test]
fn the_runtime_cap() {
    assert_eq!(E2B_PRO_RUNTIME, Duration::from_secs(24 * 3600));
    assert_eq!(cycle_window(E2B_PRO_RUNTIME), 2 * HOUR);
    assert_eq!(cycle_window(Duration::from_secs(3600)), 5 * 60_000, "Hobby's");
    assert!(ran_to_cap(0, 24 * HOUR, E2B_PRO_RUNTIME));
    assert!(ran_to_cap(0, 24 * HOUR - 30_000, E2B_PRO_RUNTIME));
    assert!(!ran_to_cap(0, 23 * HOUR, E2B_PRO_RUNTIME));
}

/// A ready workspace whose box `sbx0` enrolled as `m1`: the hub, the
/// provider, the platform, the project and the workspace, and a time after
/// which nothing else is due.
async fn ready() -> (Arc<TestHub>, Arc<Fake>, Arc<Platform>, String, String, u64) {
    let (hub, fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    hub.delegate(&project);
    let (status, w) = make(&base, "bob", &project, "dev").await;
    assert_eq!(status, 201, "{w}");
    let id = w["id"].as_str().unwrap().to_owned();
    let now = now_ms();
    drive(&app, now).await;
    hub.enroll(&id, "m1");
    drive(&app, now + 3_000).await;
    assert_eq!(app.db.hosted_workspace(&project, &id).unwrap().unwrap().state, "ready");
    fake.calls();
    (hub, fake, app, project, id, now + 100_000)
}

fn set_box(fake: &Fake, state: State, started_at: u64, end_at: u64) {
    let mut boxes = fake.boxes.lock().unwrap();
    let b = boxes.get_mut("sbx0").unwrap();
    b.state = state;
    b.started_at = iso(started_at);
    b.end_at = iso(end_at);
}

/// The leader holds the box while its daemon says busy, and not while
/// it's idle, silent, or the hub can't say. A hold that finds the box just
/// paused under a busy daemon (the race at its timeout) wakes it; one that
/// paused a while ago, or at the runtime cap (the worker's), isn't woken.
#[tokio::test]
async fn holds_while_busy_and_lets_go_when_idle_or_silent() {
    let (hub, fake, app, _project, _id, t) = ready().await;
    let mut holds = Holds::default();
    assert_eq!(tick_at(&app, &mut holds, t).await.unwrap(), 0, "nobody said: silent is idle");
    assert!(fake.calls().is_empty());

    hub.say_busy("m1", Some(true));
    assert_eq!(tick_at(&app, &mut holds, t).await.unwrap(), 1);
    assert_eq!(fake.calls(), ["hold sbx0 600s"]);
    assert_eq!(tick_at(&app, &mut holds, t + 5_000).await.unwrap(), 0);
    assert_eq!(tick_at(&app, &mut holds, t + 200_000).await.unwrap(), 1, "a third of the idle timeout on");
    assert_eq!(fake.calls(), ["hold sbx0 600s"]);

    hub.say_busy("m1", Some(false));
    assert_eq!(tick_at(&app, &mut holds, t + 400_000).await.unwrap(), 0, "idle: let it pause");
    hub.say_busy("m1", Some(true));
    assert_eq!(tick_at(&app, &mut holds, t + 405_000).await.unwrap(), 1, "busy again: held at once");
    hub.say_busy("m1", None);
    assert_eq!(tick_at(&app, &mut holds, t + 800_000).await.unwrap(), 0, "off the relay: silent is idle");
    hub.say_busy("m1", Some(true));
    hub.activity_down();
    assert_eq!(tick_at(&app, &mut holds, t + 805_000).await.unwrap(), 0, "the hub can't say: nothing held");
    assert_eq!(fake.calls(), ["hold sbx0 600s"]);

    // It paused itself as a turn started: woken.
    hub.say_busy("m1", Some(true));
    let t = t + HOUR;
    set_box(&fake, State::Paused, t - 600_000, t - 2_000);
    tick_at(&app, &mut holds, t).await.unwrap();
    assert_eq!(fake.calls(), ["hold sbx0 600s", "resume sbx0"]);
    assert_eq!(fake.boxes()[0].state, State::Running);

    // It paused a while ago (what the hub has is stale): left paused.
    let mut holds = Holds::default();
    set_box(&fake, State::Paused, t - 900_000, t - 300_000);
    tick_at(&app, &mut holds, t).await.unwrap();
    assert_eq!(fake.calls(), ["hold sbx0 600s"]);
    assert_eq!(fake.boxes()[0].state, State::Paused);

    // It ran to the runtime cap: the worker wakes it, and says so.
    let mut holds = Holds::default();
    set_box(&fake, State::Paused, t - 24 * HOUR, t - 2_000);
    tick_at(&app, &mut holds, t).await.unwrap();
    assert_eq!(fake.calls(), ["hold sbx0 600s"]);
}

/// A pause asked while the daemon says busy, or while the hub can't say,
/// isn't made: the ask is done, and the box pauses itself after the turn.
/// One asked while it's idle is.
#[tokio::test]
async fn never_paused_mid_turn() {
    let (hub, fake, app, project, id, t) = ready().await;
    hub.say_busy("m1", Some(true));
    assert!(super::super::ask(&app, &id, Ask::Pause).unwrap());
    drive(&app, t).await;
    assert!(fake.calls().is_empty(), "busy: not paused");
    let w = app.db.hosted_workspace(&project, &id).unwrap().unwrap();
    assert_eq!((w.state.as_str(), w.asked.as_deref()), ("ready", None));

    hub.activity_down();
    assert!(super::super::ask(&app, &id, Ask::Pause).unwrap());
    drive(&app, t).await;
    assert!(fake.calls().is_empty(), "the hub can't say: not paused");

    hub.say_busy("m1", Some(false));
    assert!(super::super::ask(&app, &id, Ask::Pause).unwrap());
    drive(&app, t).await;
    assert_eq!(fake.calls(), ["pause sbx0"]);
    assert_eq!(app.db.hosted_workspace(&project, &id).unwrap().unwrap().state, "paused");
}

/// Before the window, nothing; in the last stretch before the cap, a busy
/// box is looked at often and left running, and at its first idle moment
/// it's paused and resumed (which restarts the provider's clock).
#[tokio::test]
async fn paused_and_resumed_between_turns_before_the_cap() {
    let (hub, fake, app, project, id, t) = ready().await;
    set_box(&fake, State::Running, t - 21 * HOUR, t + 3 * HOUR);
    hub.say_busy("m1", Some(true));
    drive(&app, t).await;
    assert!(fake.calls().is_empty(), "three hours to go: nothing");

    set_box(&fake, State::Running, t - 23 * HOUR, t + HOUR);
    drive(&app, t + 60_000).await;
    assert!(fake.calls().is_empty(), "busy: never paused");
    drive(&app, t + 70_000).await;
    assert!(fake.calls().is_empty(), "not due yet");

    hub.say_busy("m1", Some(false));
    drive(&app, t + 75_000).await;
    assert_eq!(fake.calls(), ["pause sbx0", "resume sbx0"], "looked at again within 15 s, idle: cycled");
    let w = app.db.hosted_workspace(&project, &id).unwrap().unwrap();
    assert_eq!((w.state.as_str(), w.error.as_deref()), ("ready", None));
    assert_eq!(fake.boxes()[0].state, State::Running);
}

/// A box that paused at the cap with its daemon not saying idle ran a
/// single turn to it: woken, and said on the workspace. One whose daemon
/// said idle, or that paused at the cap long ago, stays paused.
#[tokio::test]
async fn a_turn_past_the_cap_is_woken_and_said() {
    let (hub, fake, app, project, id, t) = ready().await;
    hub.say_busy("m1", Some(false));
    set_box(&fake, State::Paused, t - 24 * HOUR, t - 5_000);
    drive(&app, t).await;
    assert!(fake.calls().is_empty(), "idle at the cap: left paused");
    assert_eq!(app.db.hosted_workspace(&project, &id).unwrap().unwrap().state, "paused");

    // Its socket dropped as it paused: silent, and it was held to the cap.
    let (hub, fake, app, project, id, t) = ready().await;
    hub.say_busy("m1", None);
    set_box(&fake, State::Paused, t - 24 * HOUR, t - 5_000);
    drive(&app, t).await;
    assert_eq!(fake.calls(), ["resume sbx0"]);
    let w = app.db.hosted_workspace(&project, &id).unwrap().unwrap();
    assert_eq!(w.state, "ready");
    let said = w.error.unwrap_or_default();
    assert!(said.contains("24-hour limit") && said.contains("woken"), "{said}");

    // Long ago: left alone.
    let (_hub, fake, app, _project, _id, t) = ready().await;
    set_box(&fake, State::Paused, t - 26 * HOUR, t - 2 * HOUR);
    drive(&app, t).await;
    assert!(fake.calls().is_empty());
}
