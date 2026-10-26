//! Waking on connect (M182), as the hub asks it, against the fake provider
//! and the platform's own worker: opens at once share one resume, a wake
//! that fails says why, and a woken box is `ready` only once its daemon
//! dials in (M258): one whose daemon never does needs a rebuild, and the
//! project's owners and editors are told. Runs on SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::{
    sync::{Arc, atomic::Ordering::SeqCst},
    time::{Duration, Instant},
};

use arugula_control_wire::hosted::{WakeAnswer, WakeCode};
use arugula_e2e::now_ms;

use super::{
    EnrollBy,
    tests::{Fake, drive, make, serve},
    wake::wake,
    worker,
};
use crate::{Platform, compute, compute::State};

/// A workspace whose box enrolled and runs, and its id.
async fn ready() -> (Arc<Fake>, Arc<Platform>, String, String) {
    let (_hub, fake, app, project, id) = ready_with_hub().await;
    (fake, app, project, id)
}

/// [`ready`], with the test hub.
async fn ready_with_hub() -> (Arc<crate::chat_tests::TestHub>, Arc<Fake>, Arc<Platform>, String, String) {
    let (hub, fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    hub.delegate(&project);
    let (_, w) = make(&base, "alice", &project, "dev").await;
    let id = w["id"].as_str().unwrap().to_owned();
    let now = now_ms();
    drive(&app, now).await;
    hub.enroll(&id, "m1");
    drive(&app, now + 3_000).await;
    assert_eq!(app.db.hosted_workspace(&project, &id).unwrap().unwrap().state, "ready");
    (hub, fake, app, project, id)
}

/// The workspace's row once the worker has made it `ready` (it is once its
/// daemon has dialed in, a tick or so after the resume).
async fn once_ready(app: &Platform, project: &str, id: &str) -> crate::hosted::HostedWorkspace {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let w = app.db.hosted_workspace(project, id).unwrap().unwrap();
        if w.state == "ready" {
            return w;
        }
        assert!(Instant::now() < until, "never ready: {w:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn code(a: &WakeAnswer) -> Option<WakeCode> {
    match a {
        WakeAnswer::Awake => None,
        WakeAnswer::Failed { code, .. } => Some(*code),
    }
}

/// The box paused itself at its timeout, and the worker hasn't seen it
/// (the workspace still says `ready`). Five opens at once: one resume,
/// done by the worker, and every open hears the box runs.
#[tokio::test]
async fn opens_at_once_share_one_resume() {
    let (fake, app, project, id) = ready().await;
    fake.pause_all();
    fake.resume_ms.store(200, SeqCst);
    worker::start(app.clone());
    let started = Instant::now();
    let opens: Vec<_> = (0..5)
        .map(|_| {
            let (app, project, id) = (app.clone(), project.clone(), id.clone());
            tokio::spawn(async move { wake(&app, &project, &id).await.unwrap() })
        })
        .collect();
    for o in opens {
        assert_eq!(o.await.unwrap(), WakeAnswer::Awake);
    }
    println!("five opens woke the box in {} ms", started.elapsed().as_millis());
    assert_eq!(fake.resumes.load(SeqCst), 1, "one resume");
    assert_eq!(fake.boxes()[0].state, State::Running);
    let w = once_ready(&app, &project, &id).await;
    assert_eq!((w.state.as_str(), w.asked.as_deref(), w.error.as_deref()), ("ready", None, None));

    // Opened again later: a resume of its own (a running box only has its
    // timeout moved).
    assert_eq!(wake(&app, &project, &id).await.unwrap(), WakeAnswer::Awake);
    assert_eq!(fake.resumes.load(SeqCst), 2);
}

/// A box gone at the provider: "gone", the workspace fails, and the next
/// open says so without asking the provider again.
#[tokio::test]
async fn a_gone_box_says_so() {
    let (fake, app, project, id) = ready().await;
    fake.boxes.lock().unwrap().clear();
    worker::start(app.clone());
    let a = wake(&app, &project, &id).await.unwrap();
    assert_eq!(code(&a), Some(WakeCode::Gone), "{a:?}");
    assert_eq!(app.db.hosted_workspace(&project, &id).unwrap().unwrap().state, "failed");
    let a = wake(&app, &project, &id).await.unwrap();
    assert_eq!(code(&a), Some(WakeCode::Gone), "{a:?}");
    assert_eq!(fake.resumes.load(SeqCst), 1);
}

/// The provider says no: "refused", with what it said; the workspace stays
/// as it was. The next open asks again, and what the last one said goes.
#[tokio::test]
async fn a_refused_wake_says_why_and_the_next_asks_again() {
    let (fake, app, project, id) = ready().await;
    fake.pause_all();
    *fake.resume_error.lock().unwrap() = Some(compute::Error::Refused { status: 409, code: Some("conflict".into()) });
    worker::start(app.clone());
    let a = wake(&app, &project, &id).await.unwrap();
    let WakeAnswer::Failed { code: WakeCode::Refused, why } = &a else { panic!("{a:?}") };
    assert!(why.contains("HTTP 409 conflict"), "{why}");
    assert_ne!(app.db.hosted_workspace(&project, &id).unwrap().unwrap().state, "failed");

    *fake.resume_error.lock().unwrap() = None;
    assert_eq!(wake(&app, &project, &id).await.unwrap(), WakeAnswer::Awake);
    assert_eq!(app.db.hosted_workspace(&project, &id).unwrap().unwrap().error, None);
}

/// The provider doesn't answer: "unavailable" at once, the wake still
/// asked, and the worker wakes the box once it answers.
#[tokio::test]
async fn an_unanswered_wake_is_unavailable_and_tried_again() {
    let (fake, app, project, id) = ready().await;
    fake.pause_all();
    *fake.resume_error.lock().unwrap() =
        Some(compute::Error::Unavailable { status: 503, code: None, retry_after: None });
    worker::start(app.clone());
    let a = wake(&app, &project, &id).await.unwrap();
    assert_eq!(code(&a), Some(WakeCode::Unavailable), "{a:?}");
    assert_eq!(app.db.hosted_workspace(&project, &id).unwrap().unwrap().asked.as_deref(), Some("wake"));

    *fake.resume_error.lock().unwrap() = None;
    let until = Instant::now() + Duration::from_secs(10);
    while fake.boxes()[0].state != State::Running {
        assert!(Instant::now() < until, "never woken");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(wake(&app, &project, &id).await.unwrap(), WakeAnswer::Awake);
}

/// A workspace being made is "starting"; one being deleted, or gone, is
/// "gone". Neither asks the provider.
#[tokio::test]
async fn a_box_being_made_or_deleted_isnt_woken() {
    let (hub, fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    hub.delegate(&project);
    let (_, w) = make(&base, "alice", &project, "dev").await;
    let id = w["id"].as_str().unwrap().to_owned();
    assert_eq!(code(&wake(&app, &project, &id).await.unwrap()), Some(WakeCode::Starting));
    assert!(super::ask(&app, &id, super::Ask::Delete).unwrap());
    assert_eq!(code(&wake(&app, &project, &id).await.unwrap()), Some(WakeCode::Gone));
    assert_eq!(code(&wake(&app, &project, "hw_nothing").await.unwrap()), Some(WakeCode::Gone));
    assert_eq!(fake.resumes.load(SeqCst), 0);
}

/// The pushes about `id`'s box waking without its daemon, by whom to.
fn told(hub: &crate::chat_tests::TestHub, id: &str) -> Vec<String> {
    let tag = serde_json::json!(format!("hosted-no-daemon-{id}"));
    hub.pushes().into_iter().filter(|(_, m, _)| m["tag"] == tag).map(|(to, _, _)| to).collect()
}

/// Arugula #124: the box resumes, but its daemon died before the pause and never
/// dials in. The hub hears the box runs (it waits for the dial itself), and
/// the workspace is `waking`, never `ready`. Past the dial wait it fails as
/// needing a rebuild, its box is paused again, the team's owner and editor
/// (not its viewer) are pushed once, and every open from then on says the
/// box's daemon is silent, without asking the provider again.
#[tokio::test]
async fn a_box_whose_daemon_never_dials_in_needs_a_rebuild() {
    let (hub, fake, app, project, id) = ready_with_hub().await;
    fake.pause_all();
    hub.set_online("m1", false);
    let t = now_ms() + 10_000;
    assert!(super::ask(&app, &id, super::Ask::Wake).unwrap());
    drive(&app, t).await;
    assert_eq!(fake.resumes.load(SeqCst), 1);
    assert_eq!(fake.boxes()[0].state, State::Running);
    let w = app.db.hosted_workspace(&project, &id).unwrap().unwrap();
    assert_eq!((w.state.as_str(), w.asked.as_deref(), w.error.as_deref()), ("waking", None, None));
    assert_eq!(wake(&app, &project, &id).await.unwrap(), WakeAnswer::Awake, "the box runs");

    drive(&app, t + 30_000).await;
    drive(&app, t + 59_000).await;
    assert_eq!(app.db.hosted_workspace(&project, &id).unwrap().unwrap().state, "waking", "within the dial wait");
    assert!(told(&hub, &id).is_empty());

    drive(&app, t + 60_000).await;
    let w = app.db.hosted_workspace(&project, &id).unwrap().unwrap();
    assert_eq!((w.state.as_str(), w.error.as_deref()), ("failed", Some(worker::NO_DAEMON)));
    assert!(w.error.as_deref().unwrap().contains("Rebuild the workspace"), "it says what to do");
    assert_eq!(fake.boxes()[0].state, State::Paused, "not left running unheld");
    let mut to = told(&hub, &id);
    to.sort();
    assert_eq!(to, ["alice", "bob"], "the owner and the editor, not the viewer");
    let m = hub
        .pushes()
        .into_iter()
        .find(|(_, m, _)| m["tag"] == serde_json::json!(format!("hosted-no-daemon-{id}")))
        .unwrap()
        .1;
    assert_eq!(m["hosted"], serde_json::json!({ "project": project, "workspace": id }), "a tap opens the workspace");

    drive(&app, t + 600_000).await;
    assert_eq!(told(&hub, &id).len(), 2, "told once");
    let a = wake(&app, &project, &id).await.unwrap();
    assert_eq!(code(&a), Some(WakeCode::Silent), "{a:?}");
    assert_eq!(fake.resumes.load(SeqCst), 1, "not woken again");

    // A rebuild makes it again, from the start.
    assert!(super::ask(&app, &id, super::Ask::Rebuild).unwrap());
    drive(&app, t + 601_000).await;
    assert_eq!(app.db.hosted_workspace(&project, &id).unwrap().unwrap().state, "enrolling");
}

/// A daemon that dials in late, within the dial wait (its supervisor
/// started it again): `ready` then, and nobody is pushed.
#[tokio::test]
async fn a_daemon_that_dials_in_within_the_wait_is_ready() {
    let (hub, fake, app, project, id) = ready_with_hub().await;
    fake.pause_all();
    hub.set_online("m1", false);
    let t = now_ms() + 10_000;
    assert!(super::ask(&app, &id, super::Ask::Wake).unwrap());
    drive(&app, t).await;
    drive(&app, t + 20_000).await;
    assert_eq!(app.db.hosted_workspace(&project, &id).unwrap().unwrap().state, "waking");
    hub.set_online("m1", true);
    drive(&app, t + 21_000).await;
    let w = app.db.hosted_workspace(&project, &id).unwrap().unwrap();
    assert_eq!((w.state.as_str(), w.error.as_deref()), ("ready", None));
    drive(&app, t + 120_000).await;
    assert_eq!(app.db.hosted_workspace(&project, &id).unwrap().unwrap().state, "ready");
    assert!(told(&hub, &id).is_empty());
}

/// While the hub can't say who's on the relay, a woken box stays `waking`
/// (it's neither said ready nor failed for the hub's silence), and goes
/// `ready` once the hub says its daemon is there.
#[tokio::test]
async fn a_hub_that_cant_say_keeps_it_waking() {
    let (hub, fake, app, project, id) = ready_with_hub().await;
    fake.pause_all();
    hub.set_online("m1", false);
    hub.online_down(true);
    let t = now_ms() + 10_000;
    assert!(super::ask(&app, &id, super::Ask::Wake).unwrap());
    drive(&app, t).await;
    drive(&app, t + 120_000).await;
    assert_eq!(app.db.hosted_workspace(&project, &id).unwrap().unwrap().state, "waking");
    hub.online_down(false);
    hub.set_online("m1", true);
    drive(&app, t + 130_000).await;
    assert_eq!(app.db.hosted_workspace(&project, &id).unwrap().unwrap().state, "ready");
}
