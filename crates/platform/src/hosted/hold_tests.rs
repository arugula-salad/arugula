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

/// A workspace on a real provider (E2B's cloud or loam's E2B-compatible
/// API, `ARUGULA_TEST_E2B_URL` and `ARUGULA_TEST_E2B_KEY`), in ticket
/// mode, with the test hub standing in for the hub and the daemon's busy
/// and idle (as `live-machine`). Its box carries `arugula_test=1`.
struct Live {
    hub: Arc<TestHub>,
    live: Arc<crate::hosted::tests::Tagged>,
    app: Arc<Platform>,
    base: String,
    project: String,
    id: String,
}

fn env_secs(name: &str, default: u64) -> Duration {
    Duration::from_secs(std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(default))
}

impl Live {
    async fn new(name: &str, idle: Duration, cap: Option<Duration>) -> Live {
        let (Ok(url), Ok(key)) = (std::env::var("ARUGULA_TEST_E2B_URL"), std::env::var("ARUGULA_TEST_E2B_KEY")) else {
            panic!("set ARUGULA_TEST_E2B_URL and ARUGULA_TEST_E2B_KEY");
        };
        let template = std::env::var("ARUGULA_TEST_E2B_TEMPLATE").unwrap_or_else(|_| "base".into());
        let (hub, _fake, app, _base, _project) = serve(EnrollBy::Ticket).await;
        let live = Arc::new(crate::hosted::tests::Tagged(compute::E2b::new(&url, key).unwrap()));
        let app = {
            let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
            let mut p =
                Platform::new(cfg, crate::db::Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
            p.compute = Some(live.clone());
            p.hosted = crate::hosted::Config {
                template: Some(template),
                idle_timeout: Some(idle),
                runtime_cap: cap,
                ..app.hosted.clone()
            };
            Arc::new(p)
        };
        let project = app.db.team_project("t1", "t1", 0).unwrap();
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let router = crate::cloud::tests::router(app.clone());
        tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
        hub.delegate(&project);
        let (status, w) = make(&base, "alice", &project, name).await;
        assert_eq!(status, 201, "{w}");
        let id = w["id"].as_str().unwrap().to_owned();
        println!("idle timeout {idle:?}, runtime cap {cap:?}, against {url}");
        Live { hub, live, app, base, project, id }
    }

    fn mine(&self) -> std::collections::BTreeMap<String, String> {
        [("arugula_workspace".to_owned(), self.id.clone())].into()
    }

    async fn state(&self) -> serde_json::Value {
        crate::hosted::tests::show(&self.base, &self.project, &self.id).await["state"].clone()
    }

    /// Make its box and enroll it: the box's id.
    async fn ready(&self) -> String {
        let started = std::time::Instant::now();
        while self.state().await != "enrolling" {
            crate::hosted::worker::tick_at(&self.app, now_ms()).await.unwrap();
            assert!(started.elapsed() < Duration::from_secs(120), "never made");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let sandbox = self.live.list(&self.mine()).await.unwrap().pop().expect("its box").id;
        self.hub.enroll(&self.id, "live-machine");
        while self.state().await != "ready" {
            crate::hosted::worker::tick_at(&self.app, now_ms()).await.unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        sandbox
    }

    async fn status(&self, sandbox: &str) -> compute::Sandbox {
        self.live.status(sandbox).await.unwrap().expect("its box")
    }

    /// Run `body`, then delete every box the workspace has, whatever
    /// happened, and check none is left.
    async fn run(&self, body: impl std::future::Future<Output = ()>) {
        use futures_util::FutureExt as _;
        let outcome = std::panic::AssertUnwindSafe(body).catch_unwind().await;
        for b in self.live.list(&self.mine()).await.unwrap_or_default() {
            let _ = self.live.delete(&b.id).await;
        }
        assert!(self.live.list(&self.mine()).await.unwrap().is_empty(), "a box was left behind");
        if let Err(p) = outcome {
            std::panic::resume_unwind(p);
        }
    }
}

fn ms(t: &str) -> u64 {
    arugula_forge_github::util::time(t).unwrap() as u64
}

/// Holding against a real provider: a box with a short idle timeout is
/// held through a busy stretch longer than it, never pausing, then pauses
/// itself within the idle timeout of going idle. Nothing is left. Ignored
/// unless asked for:
///
/// ```sh
/// ARUGULA_TEST_E2B_URL=https://api.e2b.dev ARUGULA_TEST_E2B_KEY=... \
///   cargo test -p arugula-platform --lib hosted::hold::tests::live_hold -- --ignored --nocapture
/// ```
///
/// `ARUGULA_TEST_IDLE_SECS` (60) is the idle timeout and
/// `ARUGULA_TEST_BUSY_SECS` (150) the busy stretch. M192's half-hour
/// unwatched turn at the real idle timeout is `ARUGULA_TEST_IDLE_SECS=600
/// ARUGULA_TEST_BUSY_SECS=1800` (about 45 minutes; under an hour, so E2B's
/// Hobby cap doesn't end it first).
#[tokio::test]
#[ignore = "makes a real box: set ARUGULA_TEST_E2B_URL and ARUGULA_TEST_E2B_KEY"]
async fn live_hold() {
    use std::time::Instant;
    let idle = env_secs("ARUGULA_TEST_IDLE_SECS", 60);
    let busy_for = env_secs("ARUGULA_TEST_BUSY_SECS", 150);
    let l = Live::new("live-hold", idle, None).await;
    l.run(async {
        let sandbox = l.ready().await;
        // Busy for longer than the idle timeout: held, never paused.
        l.hub.say_busy("live-machine", Some(true));
        let mut holds = Holds::default();
        let (mut held, t0) = (0, Instant::now());
        while t0.elapsed() < busy_for {
            held += tick_at(&l.app, &mut holds, now_ms()).await.unwrap();
            crate::hosted::worker::tick_at(&l.app, now_ms()).await.unwrap();
            assert_eq!(l.status(&sandbox).await.state, State::Running, "paused mid-turn after {:?}", t0.elapsed());
            tokio::time::sleep(TICK).await;
        }
        let s = l.status(&sandbox).await;
        println!("busy {:?}: held {held} times, still running, pauses at {}", t0.elapsed(), s.end_at);
        assert!(held as u64 >= busy_for.as_millis() as u64 / every(idle));

        // Idle: no more holds, and it pauses itself within the idle timeout.
        l.hub.say_busy("live-machine", Some(false));
        let (idle_at, t1) = (now_ms(), Instant::now());
        loop {
            assert_eq!(tick_at(&l.app, &mut holds, now_ms()).await.unwrap(), 0, "held while idle");
            crate::hosted::worker::tick_at(&l.app, now_ms()).await.unwrap();
            let s = l.status(&sandbox).await;
            if s.state == State::Paused {
                let paused_at = ms(&s.end_at);
                println!(
                    "paused {} ms after going idle (seen after {:?})",
                    paused_at.saturating_sub(idle_at),
                    t1.elapsed()
                );
                assert!(paused_at <= idle_at + idle.as_millis() as u64 + 2_000, "paused after its idle timeout");
                break;
            }
            assert!(t1.elapsed() < idle + Duration::from_secs(60), "never paused");
            tokio::time::sleep(TICK).await;
        }
        // The workspace shows it once its state is read again.
        let t2 = Instant::now();
        while l.state().await != "paused" {
            crate::hosted::worker::tick_at(&l.app, now_ms()).await.unwrap();
            assert!(t2.elapsed() < Duration::from_secs(90), "the workspace never showed paused");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
    .await;
}

/// The runtime cap against a real provider. By default the platform's cap
/// is set to 10 minutes (`ARUGULA_TEST_CAP_SECS`), under E2B's own: the
/// box is busy into the last stretch before it and isn't paused, then at
/// its first idle moment the worker pauses and resumes it, and the
/// provider's clock (its `startedAt`) restarts. About 10 minutes.
///
/// With `ARUGULA_TEST_PAST_CAP=1` and the provider's real cap
/// (`ARUGULA_TEST_CAP_SECS=3600` on E2B's Hobby plan, 86400 on Pro), the
/// box stays busy past the cap instead: the provider pauses it there, and
/// the worker wakes it at once and says so on the workspace. About as long
/// as the cap.
///
/// ```sh
/// ARUGULA_TEST_E2B_URL=https://api.e2b.dev ARUGULA_TEST_E2B_KEY=... \
///   cargo test -p arugula-platform --lib hosted::hold::tests::live_cap -- --ignored --nocapture
/// ```
#[tokio::test]
#[ignore = "makes a real box: set ARUGULA_TEST_E2B_URL and ARUGULA_TEST_E2B_KEY"]
async fn live_cap() {
    let cap = env_secs("ARUGULA_TEST_CAP_SECS", 600);
    let past = std::env::var("ARUGULA_TEST_PAST_CAP").is_ok_and(|v| v == "1");
    let idle = env_secs("ARUGULA_TEST_IDLE_SECS", 300);
    let l = Live::new("live-cap", idle, Some(cap)).await;
    l.run(async {
        let sandbox = l.ready().await;
        let started = ms(&l.status(&sandbox).await.started_at);
        let cap_ms = cap.as_millis() as u64;
        let window = cycle_window(cap);
        l.hub.say_busy("live-machine", Some(true));
        let mut holds = Holds::default();
        // Busy until well into the window (or, past the cap, until a minute
        // after it): never paused by the platform.
        let until = if past { started + cap_ms + 60_000 } else { started + cap_ms - window / 3 };
        let mut paused_seen = false;
        while now_ms() < until {
            tick_at(&l.app, &mut holds, now_ms()).await.unwrap();
            crate::hosted::worker::tick_at(&l.app, now_ms()).await.unwrap();
            let s = l.status(&sandbox).await;
            if s.state == State::Paused {
                assert!(past, "paused mid-turn before the cap, at {} ms", now_ms() - started);
                if !paused_seen {
                    println!("the provider paused it {} ms after its start", ms(&s.end_at) - started);
                }
                paused_seen = true;
            }
            tokio::time::sleep(TICK).await;
        }
        let s = l.status(&sandbox).await;
        if past {
            assert_eq!(s.state, State::Running, "woken after the cap");
            assert!(ms(&s.started_at) > started + cap_ms - 60_000, "its clock restarted at the cap: {}", s.started_at);
            let w = crate::hosted::tests::show(&l.base, &l.project, &l.id).await;
            println!("the workspace says: {}", w["error"]);
            assert!(w["error"].as_str().unwrap_or_default().contains("limit"), "{w}");
            return;
        }
        assert_eq!(ms(&s.started_at), started, "busy: never paused and resumed");
        // Idle: paused and resumed within a look or two.
        l.hub.say_busy("live-machine", Some(false));
        let t1 = std::time::Instant::now();
        loop {
            crate::hosted::worker::tick_at(&l.app, now_ms()).await.unwrap();
            let s = l.status(&sandbox).await;
            if s.state == State::Running && ms(&s.started_at) > started {
                println!(
                    "paused and resumed {:?} after going idle, {} ms after its start",
                    t1.elapsed(),
                    ms(&s.started_at) - started
                );
                break;
            }
            assert!(t1.elapsed() < Duration::from_secs(60), "never paused and resumed: {s:?}");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        assert_eq!(l.state().await, "ready");
    })
    .await;
}
