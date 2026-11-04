//! Agent sessions under the project's standing grant ("One standing grant
//! per project starts runs", aru-099, and aru-115, as amended), through the
//! platform's own routes: with no grant nothing starts and the thread says
//! how to give one, every way a session starts or goes on; a revoke ends
//! what's live and refuses what follows, on the same routes as runs'; a
//! good grant goes with each session's work.

use arugula_control_wire::{
    sessions::{AGENT_SESSIONS, agent_session_reply},
    work::{TAKES_RUN, TAKES_SESSION},
};
use arugula_e2e::{
    DeviceKeys,
    hosted::{Delegation, HostedKey},
    now_ms,
    passkey::Assertion,
    project_grant::{GrantRevoke, ProjectGrant, Running},
};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::{
        issues::{Actor, NewIssue},
        jobs::JobKind,
        projects::{SESSIONS_REPLY, SESSIONS_START},
        sessions::{PromptState, Session, State},
    },
    jobs::{Clock, Handler as _, Step},
    sessions_hydrate_tests::{add_box, allow, box_says, forbid, lines, platform, post_in, sessions, take},
    work::HostedMachine,
};

const ROOM: &str = "ch_general";
const REPO: &str = "https://github.com/o/app";
/// What every line saying how to give a grant says.
const HOW: &str = "Devices and machines";

fn machine(t: &str, keys: &DeviceKeys) -> HostedMachine {
    HostedMachine {
        project: t.into(),
        workspace: "hw_dev".into(),
        machine: keys.id(),
        sign: hex::encode(keys.sign_public()),
    }
}

/// What the box pulls.
async fn pull(app: &Platform, t: &str, keys: &DeviceKeys) -> arugula_control_wire::work::Work {
    crate::work::work(app, &machine(t, keys), &[TAKES_RUN.into(), TAKES_SESSION.into()]).await.unwrap()
}

/// Session `s`'s job, stepped now.
async fn step(app: &std::sync::Arc<Platform>, s: &str) -> Step {
    let follow = crate::sessions::job::Follow { app: app.clone() };
    let now = now_ms() + 1;
    let mut job = None;
    let mut held = Vec::new();
    while let Some(j) = app.db.claim_job(&[JobKind::Session], now + 60_000).unwrap() {
        if j.id == s {
            job = Some(j);
            break;
        }
        held.push(j);
    }
    for j in &held {
        app.db.release_job(j, now + 60_000, now).unwrap();
    }
    let mut job = job.expect("its job");
    let next = follow.step(&app.db, &mut job, &Clock::new(move || now)).await.unwrap();
    if let Step::Again(_) = next {
        app.db.release_job(&job, now, now).unwrap();
    }
    next
}

/// A session b asked for in #general, the box's and its thread's: `p`.
async fn session(base: &str, app: &Platform, t: &str, ask: &str) -> (Session, i64) {
    let p = post_in(base, ROOM, "b", &format!("@arugula {ask}"), None).await["seq"].as_i64().unwrap();
    let s = sessions(app, t).into_iter().find(|s| s.parent_seq == p as u64).expect("a session");
    (s, p)
}

/// `keys`, as a's passkey, over `challenge`, from control's page.
fn assertion(keys: &DeviceKeys, challenge: &str) -> Assertion {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
    let client =
        json!({ "type": "webauthn.get", "challenge": challenge, "origin": "https://control.example" }).to_string();
    let mut data = Sha256::digest(b"control.example").to_vec();
    data.push(0x05);
    data.extend_from_slice(&0u32.to_be_bytes());
    let mut msg = data.clone();
    msg.extend_from_slice(&Sha256::digest(client.as_bytes()));
    Assertion {
        id: "pk-a".into(),
        client_data_json: B64.encode(client.as_bytes()),
        authenticator_data: B64.encode(&data),
        signature: B64.encode(keys.signature(&msg)),
    }
}

/// a revokes the project's grants through the platform's route, saying
/// `running`: what it answered.
async fn revoke(hub: &TestHub, app: &Platform, base: &str, t: &str, running: Running) -> Value {
    let keys = DeviceKeys::generate();
    hub.passkey("pk-a", "a", keys.sign_public());
    let g = app.db.live_run_grant(t, now_ms()).unwrap().expect("a live grant").grant;
    let mut r = GrantRevoke::v3(t, &g.delegation, running, now_ms(), "a");
    r.passkey = Some(assertion(&keys, &r.passkey_challenge()));
    let path = format!("/api/projects/{t}/runs/grant/revoke");
    let (st, v) = call(base, "a", "POST", &path, Some(json!({ "revoke": r }))).await;
    assert_eq!(st, 200, "{v}");
    v
}

/// a revokes the project's grants under `delegation` through the route,
/// saying `running`: what it answered.
async fn revoke_of(keys: &DeviceKeys, base: &str, t: &str, delegation: &str, running: Running) -> Value {
    let mut r = GrantRevoke::v3(t, delegation, running, now_ms(), "a");
    r.passkey = Some(assertion(keys, &r.passkey_challenge()));
    let path = format!("/api/projects/{t}/runs/grant/revoke");
    let (st, v) = call(base, "a", "POST", &path, Some(json!({ "revoke": r }))).await;
    assert_eq!(st, 200, "{v}");
    v
}

/// With no grant, an editor's `@arugula` starts nothing: no session, no job,
/// nothing for the box, and the thread says an owner gives one, and how.
#[tokio::test]
async fn with_no_grant_a_mention_starts_nothing_and_says_how() {
    let (_hub, app, base, t, _u, keys) = platform(&[REPO]).await;
    forbid(&app, &t);
    let p = post_in(&base, ROOM, "b", "@arugula fix the canary", None).await["seq"].as_i64().unwrap();
    assert!(sessions(&app, &t).is_empty(), "no session");
    assert!(app.db.claim_job(&[JobKind::Session], now_ms() + 1).unwrap().is_none(), "no job");
    assert!(pull(&app, &t, &keys).await.sessions.is_empty());
    let said = lines(&base, ROOM, Some(p)).await;
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("standing grant") && said[0].contains("owner") && said[0].contains(HOW), "{}", said[0]);
    assert!(said[0].contains("https://control.example"), "where: {}", said[0]);

    // Given one, the same ask starts.
    allow(&app, &t);
    post_in(&base, ROOM, "b", "@arugula fix the canary", Some(p)).await;
    assert_eq!(sessions(&app, &t).len(), 1, "a good grant still works");
}

/// A which-repo ask doesn't get past it either: the question isn't asked
/// with no grant, and answering one asked before the grant went starts
/// nothing.
#[tokio::test]
async fn the_which_repo_answer_needs_a_grant_too() {
    let (_hub, app, base, t, _u, _keys) = platform(&[REPO, "https://github.com/o/site"]).await;
    let p = post_in(&base, ROOM, "b", "@arugula fix it", None).await["seq"].as_i64().unwrap();
    assert!(sessions(&app, &t).is_empty(), "it asks which repo first");
    forbid(&app, &t);
    post_in(&base, ROOM, "b", "o/app", Some(p)).await;
    assert!(sessions(&app, &t).is_empty(), "no session");
    assert!(lines(&base, ROOM, Some(p)).await.iter().any(|l| l.contains(HOW)));
}

/// `session_start`, a person's agent's: no session, and what it answers
/// says why. `session_reply` is refused the same way, saying why.
#[tokio::test]
async fn an_agents_session_start_and_reply_say_why_with_no_grant() {
    let (hub, app, base, t, _u, _keys) = platform(&[REPO]).await;
    hub.daemon("geek", "a", Some("t"));
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    for power in [SESSIONS_START, SESSIONS_REPLY] {
        let give = crate::agents_tests::give("geek", power, json!({ "kind": "project", "id": t }));
        let (st, g) = call(&base, "a", "POST", "/api/grants", give).await;
        assert_eq!(st, 200, "{g}");
    }
    let geek = async |path: &str, body: Value| {
        let r = reqwest::Client::new()
            .post(format!("{base}{path}"))
            .header("x-test-daemon", "a geek")
            .json(&body)
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json::<Value>().await.unwrap_or_default())
    };
    let (s, _) = session(&base, &app, &t, "before it went").await;
    forbid(&app, &t);
    let (st, v) = geek(AGENT_SESSIONS, json!({ "room": ROOM, "prompt": "fix the canary" })).await;
    assert_eq!(st, 200, "{v}");
    assert!(v["session"].is_null(), "{v}");
    assert!(v["said"][0].as_str().unwrap().contains(HOW), "{v}");
    let before = app.db.session_prompts(&s.id).unwrap().len();
    let (st, v) = geek(&agent_session_reply(&s.id), json!({ "text": "and this" })).await;
    assert_eq!((st, v["did"].as_str()), (200, Some("refused")), "{v}");
    assert!(v["why"].as_str().unwrap().contains(HOW), "{v}");
    assert_eq!(app.db.session_prompts(&s.id).unwrap().len(), before);
}

/// Agent on this is refused up front, saying why, and posts nothing.
#[tokio::test]
async fn agent_on_this_is_refused_with_no_grant() {
    let (_hub, app, base, t, _u, _keys) = platform(&[REPO]).await;
    forbid(&app, &t);
    let new = NewIssue { project: &t, title: "the canary", body: "", labels: &[], assignees: &[] };
    let issue = app.db.create_issue(&new, Actor::person("a"), 1).unwrap().unwrap();
    let (st, v) = call(&base, "b", "POST", &format!("/api/issues/{}/agent", issue.id), None).await;
    assert_eq!((st, v["reason"].as_str()), (409, Some("no_grant")), "{v}");
    assert!(v["error"].as_str().unwrap().contains(HOW), "{v}");
    assert!(sessions(&app, &t).is_empty());
}

/// Try 3, Move to… and `@codex review` start work: each is refused with no
/// grant, the routes with a reason, the mention with a line.
#[tokio::test]
async fn try_3_a_move_and_a_review_need_a_grant() {
    let (_hub, app, base, t, _u, keys) = platform(&[REPO]).await;
    add_box(&app, &t, "hw_other", "other", &[REPO], Some("other-machine"));
    let (s, p) = session(&base, &app, &t, "one").await;
    take(&app, &keys, &s.id);
    box_says(&app, &keys, &s.id, 2, State::Working, State::Done, "{}");
    forbid(&app, &t);

    let (st, v) = call(&base, "b", "POST", &format!("/api/sessions/{}/try", s.id), Some(json!({}))).await;
    assert_eq!((st, v["reason"].as_str()), (409, Some("no_grant")), "{v}");
    assert!(app.db.session_siblings(&s.id).unwrap().is_empty(), "no siblings");

    let to = json!({ "workspace": "hw_other" });
    let (st, v) = call(&base, "b", "POST", &format!("/api/sessions/{}/move", s.id), Some(to)).await;
    assert_eq!((st, v["reason"].as_str()), (409, Some("no_grant")), "{v}");
    assert_eq!(app.db.session(&s.id).unwrap().unwrap().workspace, "hw_dev");

    post_in(&base, ROOM, "b", "@codex review", Some(p)).await;
    assert_eq!(sessions(&app, &t).len(), 1, "no review");
    let said = lines(&base, ROOM, Some(p)).await;
    assert!(said.iter().any(|l| l.contains("@codex review") && l.contains(HOW)), "{said:?}");
}

/// A follow-up with no grant isn't queued: the thread says so, and the
/// session stays as it was. Given one again, the next reply prompts it.
#[tokio::test]
async fn a_follow_up_with_no_grant_is_refused_and_says_how() {
    let (_hub, app, base, t, _u, keys) = platform(&[REPO]).await;
    let (s, p) = session(&base, &app, &t, "one").await;
    take(&app, &keys, &s.id);
    box_says(&app, &keys, &s.id, 2, State::Working, State::Done, "{}");
    let before = app.db.session_prompts(&s.id).unwrap().len();
    forbid(&app, &t);
    post_in(&base, ROOM, "b", "and also this", Some(p)).await;
    assert_eq!(app.db.session_prompts(&s.id).unwrap().len(), before, "not queued");
    assert_eq!(app.db.session(&s.id).unwrap().unwrap().state, State::Done);
    assert!(lines(&base, ROOM, Some(p)).await.iter().any(|l| l.contains(HOW)));
    allow(&app, &t);
    post_in(&base, ROOM, "b", "and also this", Some(p)).await;
    assert_eq!(app.db.session_prompts(&s.id).unwrap().len(), before + 1);
}

/// A grant with a v1's `runs.start` alone doesn't let a session run, nor
/// one for an earlier delegation than the project's live one.
#[tokio::test]
async fn a_grant_that_cant_instruct_or_is_stale_starts_nothing() {
    let (hub, app, base, t, _u, _keys) = platform(&[REPO]).await;
    forbid(&app, &t);
    let keys = DeviceKeys::generate();
    let mut d = Delegation {
        project: t.clone(),
        owner: "team:t".into(),
        key: HostedKey::from_seed(&[1; 32]).public(),
        at: 1,
        by: String::new(),
        sig: String::new(),
    };
    d.sign_with(&keys);
    let now = now_ms();
    let mut narrow = ProjectGrant::new(&d, now - 1000, Some(now + 86_400_000));
    narrow.caps = vec!["runs.start".into()];
    narrow.sign_with(&keys);
    app.db.add_run_grant(&narrow, "a", now).unwrap();
    let p = post_in(&base, ROOM, "b", "@arugula one", None).await["seq"].as_i64().unwrap();
    assert!(sessions(&app, &t).is_empty(), "runs.start alone");
    assert!(lines(&base, ROOM, Some(p)).await.iter().any(|l| l.contains(HOW)));

    forbid(&app, &t);
    let g = allow(&app, &t);
    hub.set_delegation(&t, Some("another-delegation"));
    let p = post_in(&base, ROOM, "b", "@arugula two", None).await["seq"].as_i64().unwrap();
    assert!(sessions(&app, &t).is_empty(), "an earlier delegation's");
    let said = lines(&base, ROOM, Some(p)).await;
    assert!(said.iter().any(|l| l.contains("earlier delegation") && l.contains(HOW)), "{said:?}");
    hub.set_delegation(&t, Some(&g.delegation));
    post_in(&base, ROOM, "b", "@arugula three", None).await;
    assert_eq!(sessions(&app, &t).len(), 1);
}

/// A revoke saying `running stop`, through the route: every live session
/// stops at once, mid-turn or not, the thread saying so; the box's pull
/// lists none of them; a reply is only chat; and a new `@arugula` is
/// refused. The revoke reaches a pull even from a box that takes only
/// sessions.
#[tokio::test]
async fn a_revoke_saying_stop_ends_live_sessions_and_refuses_what_follows() {
    let (hub, app, base, t, _u, keys) = platform(&[REPO]).await;
    let (working, p) = session(&base, &app, &t, "one").await;
    take(&app, &keys, &working.id);
    let (queued, q) = session(&base, &app, &t, "two").await;
    let v = revoke(&hub, &app, &base, &t, Running::Stop).await;
    assert_eq!(v["sessions"], 2, "{v}");
    for (s, thread) in [(&working, p), (&queued, q)] {
        let after = app.db.session(&s.id).unwrap().unwrap();
        assert_eq!(after.state, State::Stopped, "{}", s.id);
        assert!(lines(&base, ROOM, Some(thread)).await.iter().any(|l| l.contains("revoked") && l.contains(HOW)));
    }
    let w = pull(&app, &t, &keys).await;
    assert!(w.sessions.is_empty(), "the box stops them");
    assert_eq!(w.revokes.len(), 1);
    let only = crate::work::work(&app, &machine(&t, &keys), &[TAKES_SESSION.into()]).await.unwrap();
    assert_eq!(only.revokes.len(), 1, "with sessions alone too");

    let before = app.db.session_prompts(&working.id).unwrap().len();
    post_in(&base, ROOM, "b", "and also this", Some(p)).await;
    assert_eq!(app.db.session_prompts(&working.id).unwrap().len(), before, "a follow-up is chat now");
    let n = post_in(&base, ROOM, "b", "@arugula three", None).await["seq"].as_i64().unwrap();
    assert_eq!(sessions(&app, &t).len(), 2, "no new session");
    assert!(lines(&base, ROOM, Some(n)).await.iter().any(|l| l.contains("revoked") && l.contains(HOW)));
}

/// A revoke saying `running finish`: what isn't mid-turn ends at once; a
/// turn under way finishes, takes no follow-up, and its session ends when
/// the turn does, even if a new grant came meanwhile. A session made under
/// that new grant goes on.
#[tokio::test]
async fn a_revoke_saying_finish_lets_the_turn_end_and_no_more() {
    let (hub, app, base, t, _u, keys) = platform(&[REPO]).await;
    let (idle, _) = session(&base, &app, &t, "idle").await;
    take(&app, &keys, &idle.id);
    box_says(&app, &keys, &idle.id, 2, State::Working, State::Done, "{}");
    let (busy, p) = session(&base, &app, &t, "busy").await;
    take(&app, &keys, &busy.id);
    post_in(&base, ROOM, "b", "queued before the revoke", Some(p)).await;
    let v = revoke(&hub, &app, &base, &t, Running::Finish).await;
    let queued = app.db.session_prompts(&busy.id).unwrap();
    assert!(queued.iter().all(|x| x.state != PromptState::Queued), "what was queued goes: {queued:?}");
    assert_eq!(v["sessions"], 1, "{v}");
    assert_eq!(app.db.session(&idle.id).unwrap().unwrap().state, State::Stopped);
    assert_eq!(app.db.session(&busy.id).unwrap().unwrap().state, State::Working);
    assert!(pull(&app, &t, &keys).await.sessions.iter().any(|w| w.session == busy.id && w.grant.is_none()));

    let before = app.db.session_prompts(&busy.id).unwrap().len();
    post_in(&base, ROOM, "b", "and also this", Some(p)).await;
    assert_eq!(app.db.session_prompts(&busy.id).unwrap().len(), before, "no follow-up");
    assert!(matches!(step(&app, &busy.id).await, Step::Again(_)), "still mid-turn");

    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    allow(&app, &t);
    let (later, _) = session(&base, &app, &t, "later").await;
    post_in(&base, ROOM, "b", "and also this", Some(p)).await;
    assert_eq!(app.db.session_prompts(&busy.id).unwrap().len(), before, "a new grant doesn't bring it back");

    box_says(&app, &keys, &busy.id, 2, State::Working, State::Done, "{}");
    assert_eq!(step(&app, &busy.id).await, Step::Done);
    assert_eq!(app.db.session(&busy.id).unwrap().unwrap().state, State::Stopped);
    assert!(matches!(step(&app, &later.id).await, Step::Again(_)));
    assert_eq!(app.db.session(&later.id).unwrap().unwrap().state, State::Queued);
}

/// A queued session with no grant (requeued by a rebuild, or from before
/// this): its job fails it, saying how, and the box isn't offered it.
#[tokio::test]
async fn a_queued_session_with_no_grant_fails_saying_how() {
    let (_hub, app, base, t, _u, keys) = platform(&[REPO]).await;
    let (s, p) = session(&base, &app, &t, "one").await;
    forbid(&app, &t);
    assert!(pull(&app, &t, &keys).await.sessions.is_empty(), "not offered, even to a box that checks nothing");
    assert_eq!(step(&app, &s.id).await, Step::Done);
    assert_eq!(app.db.session(&s.id).unwrap().unwrap().state, State::Failed);
    assert!(lines(&base, ROOM, Some(p)).await.iter().any(|l| l.contains(HOW)));
}

/// A live session on a project with no grant, as at rollout: it isn't
/// stopped. Its turn finishes; its queued prompts are dropped and said, its
/// thread told once; the box isn't given them. A grant given later lets it
/// go on.
#[tokio::test]
async fn a_live_session_with_no_grant_stays_and_is_told_once() {
    let (_hub, app, base, t, _u, keys) = platform(&[REPO]).await;
    let (s, p) = session(&base, &app, &t, "one").await;
    take(&app, &keys, &s.id);
    post_in(&base, ROOM, "b", "and also this", Some(p)).await;
    let queued = |app: &Platform| {
        app.db.session_prompts(&s.id).unwrap().iter().filter(|x| x.state == PromptState::Queued).count()
    };
    assert_eq!(queued(&app), 1);
    forbid(&app, &t);
    assert!(pull(&app, &t, &keys).await.sessions[0].prompts.is_empty(), "the box isn't given it");
    assert!(matches!(step(&app, &s.id).await, Step::Again(_)));
    assert_eq!(app.db.session(&s.id).unwrap().unwrap().state, State::Working, "its turn goes on");
    assert_eq!(queued(&app), 0, "dropped");
    let told = lines(&base, ROOM, Some(p)).await;
    assert_eq!(told.iter().filter(|l| l.contains(HOW)).count(), 1, "{told:?}");
    assert!(told.iter().any(|l| l.contains("finishes")), "{told:?}");
    box_says(&app, &keys, &s.id, 2, State::Working, State::Done, "{}");
    assert!(matches!(step(&app, &s.id).await, Step::Again(_)));
    assert_eq!(app.db.session(&s.id).unwrap().unwrap().state, State::Done, "not ended");
    assert_eq!(lines(&base, ROOM, Some(p)).await.iter().filter(|l| l.contains(HOW)).count(), 1, "told once");

    allow(&app, &t);
    post_in(&base, ROOM, "b", "and this", Some(p)).await;
    assert_eq!(queued(&app), 1, "a good grant lets it go on");
    let w = pull(&app, &t, &keys).await;
    assert_eq!(w.sessions[0].prompts.len(), 1);
    assert!(w.sessions[0].grant.is_some());
}

/// A pull with a good grant: each session carries it, and a v2's passkey
/// log while one may start a turn (and only then: it asks the hub).
#[tokio::test]
async fn a_good_grant_goes_with_each_sessions_work() {
    let (_hub, app, base, t, _u, keys) = platform(&[REPO]).await;
    let g = app.db.live_run_grant(&t, now_ms()).unwrap().unwrap().grant;
    let (s, _) = session(&base, &app, &t, "one").await;
    let w = pull(&app, &t, &keys).await;
    assert_eq!(w.sessions[0].grant.as_ref(), Some(&g), "queued");
    take(&app, &keys, &s.id);
    let w = pull(&app, &t, &keys).await;
    assert_eq!(w.sessions[0].grant.as_ref(), Some(&g), "mid-turn too");
}

/// A warm ping doesn't wake a box for a project with no grant.
#[tokio::test]
async fn a_warm_ping_wakes_nothing_with_no_grant() {
    let (_hub, app, _base, t, _u, _keys) = platform(&[REPO]).await;
    let room = app.db.chat_room(ROOM).unwrap().unwrap();
    forbid(&app, &t);
    let w = crate::chat::typing::warm_for(&app, &room, "b", now_ms()).await.unwrap();
    assert_eq!(w, crate::chat::typing::Warm::NoGrant);
}

/// Revoking a grant another has replaced (under an earlier delegation, say)
/// ends nothing that runs under the other: not at the route, not by its job,
/// and follow-ups and new sessions go on. Revoking the one in force ends them.
#[tokio::test]
async fn a_revoke_of_a_replaced_grant_ends_nothing_under_the_new_one() {
    let (hub, app, base, t, _u, keys) = platform(&[REPO]).await;
    let old = app.db.live_run_grant(&t, now_ms()).unwrap().unwrap().grant;
    // A new delegation, and a grant under it.
    let device = DeviceKeys::generate();
    let mut d = Delegation {
        project: t.clone(),
        owner: "team:t".into(),
        key: HostedKey::from_seed(&[2; 32]).public(),
        at: 2,
        by: String::new(),
        sig: String::new(),
    };
    d.sign_with(&device);
    assert_ne!(d.id(), old.delegation);
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let now = now_ms();
    let mut new = ProjectGrant::new(&d, now, Some(now + 86_400_000));
    new.sign_with(&device);
    app.db.add_run_grant(&new, "a", now).unwrap();

    let (s, p) = session(&base, &app, &t, "one").await;
    take(&app, &keys, &s.id);
    box_says(&app, &keys, &s.id, 2, State::Working, State::Done, "{}");

    let passkey = DeviceKeys::generate();
    hub.passkey("pk-a", "a", passkey.sign_public());
    let v = revoke_of(&passkey, &base, &t, &old.delegation, Running::Stop).await;
    assert_eq!((&v["revoked"], &v["sessions"]), (&json!(1), &json!(0)), "{v}");
    assert_eq!(app.db.session(&s.id).unwrap().unwrap().state, State::Done);
    assert!(matches!(step(&app, &s.id).await, Step::Again(_)));
    assert_eq!(app.db.session(&s.id).unwrap().unwrap().state, State::Done, "its job leaves it too");
    let before = app.db.session_prompts(&s.id).unwrap().len();
    post_in(&base, ROOM, "b", "and this", Some(p)).await;
    assert_eq!(app.db.session_prompts(&s.id).unwrap().len(), before + 1, "a follow-up goes");
    let (second, _) = session(&base, &app, &t, "two").await;
    assert_eq!(second.state, State::Queued);

    let v = revoke_of(&passkey, &base, &t, &d.id(), Running::Stop).await;
    assert_eq!(v["sessions"], 2, "{v}");
    assert_eq!(app.db.session(&s.id).unwrap().unwrap().state, State::Stopped);
}

/// A narrower grant is the newest for the box to know of, though sessions
/// can't run under it. A run named under a grant that's been replaced goes
/// with the one that replaced it: a box refuses the older once it's seen it.
#[tokio::test]
async fn the_pull_lists_the_grant_in_force() {
    let (_hub, app, base, t, _u, keys) = platform(&[REPO]).await;
    let (s, _) = session(&base, &app, &t, "one").await;
    take(&app, &keys, &s.id);
    forbid(&app, &t);
    let device = DeviceKeys::generate();
    let mut d = Delegation {
        project: t.clone(),
        owner: "team:t".into(),
        key: HostedKey::from_seed(&[1; 32]).public(),
        at: 1,
        by: String::new(),
        sig: String::new(),
    };
    d.sign_with(&device);
    let now = now_ms();
    let mut narrow = ProjectGrant::new(&d, now, Some(now + 86_400_000));
    narrow.caps = vec!["runs.start".into()];
    narrow.sign_with(&device);
    app.db.add_run_grant(&narrow, "a", now).unwrap();
    let w = pull(&app, &t, &keys).await;
    assert_eq!(w.sessions[0].grant.as_ref(), Some(&narrow));
    assert!(w.sessions[0].prompts.is_empty());
}
