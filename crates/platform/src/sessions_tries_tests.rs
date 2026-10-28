//! Try 3 and `@codex review` (M264) through the platform's own routes and
//! the box's signed reports, in M256's team (`sessions_hydrate_tests`) with
//! the App against a fake GitHub that links `acme/app` to t's project.
//!
//! - Try 3 makes two siblings that re-run the session's first prompt, as
//!   its asker, on the harnesses asked for; viewers and people off the
//!   hosted beta's list can't; the box's cap leaves the extra one queued;
//!   the parent's card lists all three, live; a pick stops the others, "not
//!   picked", and the kept sibling is the thread's session, with its card.
//! - `@codex review` reviews what the thread's session pushed, with the
//!   diff from GitHub and the thread fenced in its prompt; it runs
//!   read-only, its reply is posted in the thread as the review session's
//!   own (Arugula #215), saying when Claude Code stood in for Codex, and
//!   it ends.
//! - Every other turn's answer is posted in the thread, once, as the
//!   session's own, cut with the session's link when long, and it wakes
//!   nobody, prompts nothing and isn't its asker's words.
//! - A turn typed in the session's block gets one plain line in the
//!   thread, saying who asked what (Arugula #215), once.

use std::sync::Arc;

use arugula_control_wire::work::{Reported, SessionReport};
use arugula_e2e::{DeviceKeys, now_ms, team::TeamRole};
use serde_json::{Value, json};

use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::{
        Db,
        chat::Room,
        sessions::{Kind, NOT_PICKED, PromptState, Role, Session, State},
        sql::{Sql as _, params},
    },
    forge::{
        GithubApp,
        links_tests::{SECRET, Shared, fake_github, throwaway_key},
    },
    sessions::review,
    sessions_hydrate_tests::{add_box, post_in},
    work::HostedMachine,
};

struct World {
    hub: Arc<TestHub>,
    app: Arc<Platform>,
    base: String,
    gh: Shared,
    project: String,
    keys: DeviceKeys,
}

/// Team t: a owns it, b and y edit, x views; a, b and x are on the hosted
/// beta's list. #general (`ch_general`), the hosted workspace `dev`
/// (`hw_dev`) cloning acme/app, linked (77) through the App's installation
/// 9.
async fn world() -> World {
    let hub = Arc::new(TestHub::default());
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer), ("y", TeamRole::Editor)]);
    hub.flag("team:t", true);
    let gh: Shared = Arc::default();
    {
        let mut g = gh.lock().unwrap();
        g.installed.insert("acme/app".into(), (9, 500, "acme".into()));
        g.repos.insert("acme/app".into(), (77, true));
    }
    let api = fake_github(gh.clone()).await;
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    p.hosted = crate::hosted::Config { accounts: vec!["a".into(), "b".into(), "x".into()], ..Default::default() };
    p.github = Some(GithubApp::new("1".into(), "arugula-test".into(), SECRET.into(), &api, throwaway_key()));
    let app = Arc::new(p);
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let project = crate::projects::team_project(&app, "t").await.unwrap().unwrap();
    let general = Room {
        id: "ch_general".into(),
        scope: "team:t".into(),
        kind: "channel".into(),
        name: "general".into(),
        machine: None,
        target: None,
        created_by: "a".into(),
        created: 1,
        archived: None,
        last_seq: 0,
    };
    app.db.add_chat_room(&general, &["a", "b", "x", "y"], Some(&project)).unwrap();
    app.db
        .c()
        .execute(
            "INSERT INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_by, linked_at)
             VALUES (77, 'acme/app', TRUE, ?1, 9, 'a', 1)",
            params![project],
        )
        .unwrap();
    let keys = DeviceKeys::generate();
    add_box(&app, &project, "hw_dev", "dev", &["https://github.com/acme/app"], Some(&keys.id()));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    World { hub, app, base, gh, project, keys }
}

impl World {
    /// The box's next signed report on `ses`, through the hub's route.
    async fn says(&self, ses: &str, kind: Kind, to: Option<State>, detail: Value) -> Reported {
        let r = self.report(ses, kind, to, detail);
        self.send(&r).await
    }

    /// The box's next signed report on `ses`.
    fn report(&self, ses: &str, kind: Kind, to: Option<State>, detail: Value) -> SessionReport {
        let s = self.app.db.session(ses).unwrap().unwrap();
        let from = s.seen_as();
        let to = to.unwrap_or(from);
        let detail = detail.to_string();
        let id = self.keys.id();
        let ev = arugula_e2e::session::Event {
            session: ses,
            n: s.daemon_n + 1,
            kind,
            from,
            to,
            at: now_ms(),
            detail: &detail,
            daemon: &id,
        };
        SessionReport {
            session: ses.into(),
            n: ev.n,
            kind: kind.as_str().into(),
            from: from.as_str().into(),
            to: to.as_str().into(),
            at: ev.at,
            sig: ev.sign(&self.keys),
            detail: detail.clone(),
        }
    }

    /// Report `r` through the hub's route, as the box.
    async fn send(&self, r: &SessionReport) -> Reported {
        let m = HostedMachine {
            project: self.project.clone(),
            workspace: "hw_dev".into(),
            machine: self.keys.id(),
            sign: hex::encode(self.keys.sign_public()),
        };
        crate::session_live::report(&self.app, &m, r).await.unwrap()
    }

    /// The box takes `ses`, saying it runs it on `harness`.
    async fn take(&self, ses: &str, harness: &str) {
        let prompts: Vec<String> = queued(&self.app, ses);
        let d = json!({ "prompts": prompts, "block": "7", "harness": harness });
        assert_eq!(self.says(ses, Kind::State, Some(State::Working), d).await, Reported::Taken);
    }

    /// A turn of `ses` that pushed (or not) its branch, then done.
    async fn turn(&self, ses: &str, pushed: bool, diffstat: &str) {
        let branch = self.app.db.session(ses).unwrap().unwrap().branch;
        let d = json!({ "branch": branch, "pushed": pushed, "commits": ["abc1234 fix it"], "diffstat": diffstat });
        assert_eq!(self.says(ses, Kind::Turn, None, d).await, Reported::Taken);
        assert_eq!(self.says(ses, Kind::State, Some(State::Done), json!({})).await, Reported::Taken);
    }

    fn session(&self, id: &str) -> Session {
        self.app.db.session(id).unwrap().unwrap()
    }

    /// The thread's messages, as `a` reads them: (author, via, text).
    async fn thread(&self, p: i64) -> Vec<(String, String, String)> {
        let (st, v) =
            call(&self.base, "a", "GET", &format!("/api/chat/rooms/ch_general/messages?thread={p}"), None).await;
        assert_eq!(st, 200, "{v}");
        v["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                let s = |k: &str| m[k].as_str().unwrap_or_default().to_owned();
                (s("author"), s("via"), s("text"))
            })
            .collect()
    }
}

fn queued(app: &Platform, ses: &str) -> Vec<String> {
    app.db.session_prompts(ses).unwrap().into_iter().filter(|p| p.state == PromptState::Queued).map(|p| p.id).collect()
}

/// How many times pages were told session `ses` changed.
fn told(db: &Db, ses: &str) -> usize {
    let events = db.chat_events_after(0, 10_000).unwrap();
    let want = format!(r#""ses":"{ses}""#);
    events
        .iter()
        .filter(|e| e.kind == "live" && e.body.contains(r#""type":"session""#) && e.body.contains(&want))
        .count()
}

/// `a` asks `@arugula` in #general: the thread's number and its session.
async fn asked(w: &World, text: &str) -> (i64, Session) {
    let m = post_in(&w.base, "ch_general", "a", text, None).await;
    let p = m["seq"].as_i64().unwrap();
    let s = w.app.db.session_on_thread("ch_general", p as u64).unwrap().expect("a session");
    (p, s)
}

#[tokio::test]
async fn try_3_runs_three_and_a_pick_keeps_one() {
    let w = world().await;
    let (p, s) = asked(&w, "@arugula fix the canary").await;
    assert_eq!((s.role, s.harness.as_str(), s.parent_session.as_deref()), (Role::Thread, "claude", None));
    w.take(&s.id, "claude").await;
    w.turn(&s.id, true, " a.rs | 1 +").await;
    let path = format!("/api/sessions/{}/try", s.id);

    // A viewer can't, nor an editor off the hosted beta's list.
    let (st, v) = call(&w.base, "x", "POST", &path, Some(json!({}))).await;
    assert_eq!(st, 403, "{v}");
    let (st, v) = call(&w.base, "y", "POST", &path, Some(json!({}))).await;
    assert_eq!(st, 403, "{v}");
    assert!(v["error"].as_str().unwrap().contains("limited beta"), "{v}");
    let (st, v) = call(&w.base, "b", "POST", &path, Some(json!({ "harnesses": ["claude", "gpt"] }))).await;
    assert_eq!(st, 400, "{v}");

    // b tries it three ways: two siblings, claude and codex, re-running its
    // first prompt as its asker (whose turns they are).
    let (st, v) = call(&w.base, "b", "POST", &path, Some(json!({}))).await;
    assert_eq!(st, 200, "{v}");
    let tries = v["card"]["tries"].as_array().unwrap().clone();
    let ids: Vec<String> = tries.iter().map(|t| t["session"].as_str().unwrap().to_owned()).collect();
    assert_eq!(ids.len(), 3, "{v}");
    assert_eq!(ids[0], s.id, "the session itself first");
    assert_eq!(tries.iter().map(|t| t["harness"].as_str().unwrap()).collect::<Vec<_>>(), ["claude", "claude", "codex"]);
    assert_eq!(tries.iter().map(|t| t["state"].as_str().unwrap()).collect::<Vec<_>>(), ["done", "queued", "queued"]);
    let first = w.app.db.session_prompts(&s.id).unwrap().remove(0);
    for sib in &ids[1..] {
        let t = w.session(sib);
        assert_eq!((t.role, t.parent_session.as_deref(), t.asker.as_str()), (Role::Try, Some(s.id.as_str()), "a"));
        assert_eq!((t.room.as_str(), t.parent_seq, t.repo.as_deref()), ("ch_general", p as u64, Some("acme/app")));
        assert!(t.branch.ends_with("-fix-the-canary") && t.branch != s.branch, "{}", t.branch);
        let prompts = w.app.db.session_prompts(sib).unwrap();
        assert_eq!((prompts.len(), prompts[0].text.as_str()), (1, first.text.as_str()));
    }
    let (st, v) = call(&w.base, "b", "POST", &path, Some(json!({}))).await;
    assert_eq!(st, 409, "once: {v}");
    let (st, _) = call(&w.base, "b", "POST", &format!("/api/sessions/{}/try", ids[1]), Some(json!({}))).await;
    assert_eq!(st, 409, "not of a sibling");
    // Replies still prompt the thread's own session.
    assert_eq!(w.app.db.session_on_thread("ch_general", p as u64).unwrap().unwrap().id, s.id);

    // The box's cap: s, done, holds a slot, and so does another thread's
    // session working there; one sibling waits for a slot.
    let (_, other) = asked(&w, "@arugula something else").await;
    w.take(&other.id, "claude").await;
    let work = w.app.db.session_work("hw_dev").unwrap();
    let listed: Vec<&str> = work.iter().map(|x| x.session.as_str()).collect();
    assert_eq!(listed, [s.id.as_str(), ids[1].as_str(), other.id.as_str()], "the third sibling waits");
    let (_, v) = call(&w.base, "a", "GET", &format!("/api/sessions/{}", s.id), None).await;
    let waits: Vec<bool> = v["card"]["tries"].as_array().unwrap().iter().map(|t| t["no_slot"] == true).collect();
    assert_eq!(waits, [false, false, true], "its card says which waits for a slot: {v}");
    assert_eq!(work[1].harness.as_deref(), Some("claude"));
    // The other one ends: the codex sibling's turn, on a box without
    // Codex, runs on Claude Code, and its parent's card hears it.
    let before = told(&w.app.db, &s.id);
    let stop = format!("/api/sessions/{}/stop", other.id);
    assert_eq!(call(&w.base, "a", "POST", &stop, None).await.0, 200);
    let work = w.app.db.session_work("hw_dev").unwrap();
    assert_eq!(work.iter().find(|x| x.session == ids[2]).unwrap().harness.as_deref(), Some("codex"));
    w.take(&ids[1], "claude").await;
    w.take(&ids[2], "claude").await;
    assert!(told(&w.app.db, &s.id) >= before + 2, "the parent's card is told of its siblings");
    w.turn(&ids[1], true, " b.rs | 2 ++").await;
    w.turn(&ids[2], true, " c.rs | 3 +++").await;
    let (_, v) = call(&w.base, "b", "GET", &format!("/api/sessions/{}", s.id), None).await;
    let tries = v["card"]["tries"].as_array().unwrap();
    assert_eq!(tries[2]["ran"], "claude", "{v}");
    assert_eq!(tries[2]["turn"]["diffstat"], " c.rs | 3 +++");
    assert!(tries.iter().all(|t| t["state"] == "done" && t["pushed"] == true), "{v}");
    assert_eq!(v["card"]["picked"], Value::Null);

    // A viewer can't pick. b picks the codex sibling from its own card: the
    // other two stop, not picked, and it's the thread's session now, with
    // its card in the thread.
    let pick = format!("/api/sessions/{}/pick", ids[2]);
    assert_eq!(call(&w.base, "x", "POST", &pick, Some(json!({ "session": ids[2] }))).await.0, 403);
    let (st, v) = call(&w.base, "b", "POST", &pick, Some(json!({ "session": "ses_nope" }))).await;
    assert_eq!(st, 409, "{v}");
    let (st, v) = call(&w.base, "b", "POST", &pick, Some(json!({ "session": ids[2] }))).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["card"]["picked"], ids[2].as_str());
    for gone in [&ids[0], &ids[1]] {
        assert_eq!(w.session(gone).state, State::Stopped);
        let ev = w.app.db.session_events(gone).unwrap().pop().unwrap();
        assert!(ev.detail.contains(NOT_PICKED), "{}", ev.detail);
    }
    assert_eq!(w.session(&s.id).picked_by.as_deref(), Some("b"));
    let (_, v) = call(&w.base, "a", "GET", &format!("/api/sessions/{}", s.id), None).await;
    let last = v["events"].as_array().unwrap().last().unwrap()["text"].clone();
    assert_eq!(last, "Stopped: another try was kept");
    assert_eq!(w.app.db.session_on_thread("ch_general", p as u64).unwrap().unwrap().id, ids[2]);
    let vias: Vec<String> = w.thread(p).await.into_iter().map(|(_, via, _)| via).collect();
    assert!(vias.contains(&format!("session:{}", ids[2])), "{vias:?}");
    let (st, _) = call(&w.base, "b", "POST", &pick, Some(json!({ "session": ids[2] }))).await;
    assert_eq!(st, 409, "picked once");
    // A reply prompts the kept one.
    post_in(&w.base, "ch_general", "a", "now the docs", Some(p)).await;
    let texts: Vec<String> = w.app.db.session_prompts(&ids[2]).unwrap().into_iter().map(|p| p.text).collect();
    assert_eq!(texts.last().map(String::as_str), Some("now the docs"));

    // M265: the kept one's PR merges, and the thread is done, though the
    // other tries are newer than the session itself; the project's
    // numbers count the ask once, merged.
    assert!(w.app.db.done_threads(&["ch_general"]).unwrap().is_empty());
    let kept = w.session(&ids[2]);
    let (marked, _) = w.app.db.session_merged(77, &kept.branch, 5, now_ms()).unwrap();
    assert_eq!(marked, [ids[2].clone()]);
    assert!(w.app.db.done_threads(&["ch_general"]).unwrap().contains(&("ch_general".to_owned(), p as u64)));
    let counted = w.app.db.session_numbers(&w.project, 0).unwrap();
    let asks: Vec<&str> = counted.iter().map(|c| c.session.as_str()).collect();
    assert_eq!(asks, [s.id.as_str(), other.id.as_str()], "{counted:?}");
    assert!(counted[0].merged_at.is_some() && counted[1].merged_at.is_none(), "{counted:?}");
    let _ = &w.hub;
}

#[tokio::test]
async fn codex_review_reviews_what_was_pushed() {
    let w = world().await;
    let (p, s) = asked(&w, "@arugula fix the canary").await;
    w.take(&s.id, "claude").await;
    let say = |who: &'static str, text: &'static str| {
        let base = w.base.clone();
        async move { post_in(&base, "ch_general", who, text, Some(p)).await }
    };
    let lines = || async {
        w.thread(p).await.into_iter().filter(|(_, via, _)| via == "system").map(|(_, _, t)| t).collect::<Vec<_>>()
    };

    // `@codex` alone says what it does; a viewer can't ask; nothing's
    // pushed yet.
    say("a", "@codex hello").await;
    say("x", "@codex review").await;
    say("a", "@codex review").await;
    let said = lines().await;
    assert_eq!(said[0], review::WHAT);
    assert!(said[1].contains("@codex review needs an owner or editor"), "{said:?}");
    assert_eq!(said[2], review::NOTHING_PUSHED);
    assert!(w.app.db.session_siblings(&s.id).unwrap().is_empty());
    assert_eq!(w.app.db.session_prompts(&s.id).unwrap().len(), 1, "none of it was a prompt to the session");

    // Pushed, but GitHub has no diff for it: a line.
    w.turn(&s.id, true, " a.rs | 1 +").await;
    say("a", "@codex review").await;
    assert!(lines().await[3].contains("can't read what the session pushed"), "{:?}", lines().await);

    // With the diff: a review session under it, read-only, on codex, its
    // prompt the diff and the thread, fenced; its card in the thread only.
    let spec = format!("main...{}", s.branch);
    let diff = "diff --git a/a.rs b/a.rs\n+fn main() {}\n";
    w.gh.lock().unwrap().files.insert(format!("acme/app:compare:{spec}"), diff.into());
    say("b", "@codex review please").await;
    let r = w
        .app
        .db
        .project_sessions(&w.project, 10)
        .unwrap()
        .into_iter()
        .find(|x| x.role == Role::Review)
        .expect("a review session");
    assert_eq!(
        (r.parent_session.as_deref(), r.harness.as_str(), r.asker.as_str()),
        (Some(s.id.as_str()), "codex", "b")
    );
    let text = w.app.db.session_prompts(&r.id).unwrap().remove(0).text;
    assert!(text.contains(&format!("```diff\n{}\n```", diff.trim_end())), "{text}");
    assert!(text.contains("acme/app main...arugula/") && text.contains("a: @arugula fix the canary"), "{text}");
    assert!(text.contains("read-only"), "{text}");
    assert_eq!(w.app.db.session_on_thread("ch_general", p as u64).unwrap().unwrap().id, s.id, "not the thread's");
    let card = format!("session:{}", r.id);
    assert!(w.thread(p).await.iter().any(|(_, via, _)| *via == card), "its card is in the thread");
    let (_, v) = call(&w.base, "a", "GET", "/api/chat/rooms/ch_general/messages", None).await;
    let timeline: Vec<&str> = v["messages"].as_array().unwrap().iter().filter_map(|m| m["via"].as_str()).collect();
    assert!(!timeline.contains(&card.as_str()), "and not in the timeline: {timeline:?}");
    let work = w.app.db.session_work("hw_dev").unwrap();
    let rw = work.iter().find(|x| x.session == r.id).unwrap();
    assert!(rw.review && rw.harness.as_deref() == Some("codex"));
    let m = HostedMachine {
        project: w.project.clone(),
        workspace: "hw_dev".into(),
        machine: w.keys.id(),
        sign: hex::encode(w.keys.sign_public()),
    };
    w.take(&r.id, "claude").await;
    let ask = arugula_control_wire::work::PushAsk { session: r.id.clone(), branch: r.branch.clone(), clone: false };
    let given = crate::sessions::push::token(&w.app, &m, &ask).await.unwrap();
    assert!(format!("{given:?}").contains("a review pushes nothing"), "{given:?}");

    // A review pushes nothing; its reply is posted as the review session's
    // own (Arugula #215: no author, so not b's words), saying Claude Code
    // stood in, and then it ends. It prompts nothing and wakes nobody.
    let pushed = json!({ "branch": r.branch, "pushed": true });
    assert!(matches!(w.says(&r.id, Kind::Turn, None, pushed).await, Reported::Refused { .. }));
    let pushes = crate::chat_threads_tests::pushed(&w.hub).await.len();
    let d = json!({ "branch": r.branch, "pushed": false, "reply": "a.rs:1 main does nothing. @b @codex review it" });
    assert_eq!(w.says(&r.id, Kind::Turn, None, d).await, Reported::Taken);
    assert_eq!(w.says(&r.id, Kind::State, Some(State::Done), json!({})).await, Reported::Taken);
    let said = format!("said:{}", r.id);
    let posted: Vec<_> = w.thread(p).await.into_iter().filter(|(_, via, _)| *via == said).collect();
    assert_eq!(posted.len(), 1, "posted once, as the review and not an answer too: {posted:?}");
    let posted = &posted[0];
    assert_eq!(posted.0, "", "the review session's, not its asker's: {posted:?}");
    assert!(posted.2.contains("Codex isn't on this box, so Claude Code reviewed it."), "{}", posted.2);
    assert!(posted.2.ends_with("a.rs:1 main does nothing. @b @codex review it"), "{}", posted.2);
    assert!(!w.thread(p).await.iter().any(|(_, via, _)| via.ends_with(" review")), "no longer its asker's via");
    let (_, v) = call(&w.base, "a", "GET", &format!("/api/chat/rooms/ch_general/messages?thread={p}"), None).await;
    let m = v["messages"].as_array().unwrap().iter().find(|m| m["via"] == said.as_str()).unwrap().clone();
    assert_eq!(m["mentions"], json!([]), "it names nobody: {m}");
    assert_eq!(crate::chat_threads_tests::pushed(&w.hub).await.len(), pushes, "and pushes nobody");
    assert_eq!(w.app.db.session_prompts(&s.id).unwrap().len(), 1, "it isn't a prompt to the session");
    assert_eq!(w.app.db.session_prompts(&r.id).unwrap().len(), 1, "nor to the review");
    let r = w.session(&r.id);
    assert_eq!(r.state, State::Stopped, "it ends once posted, so its slot frees");
    let reviews = w.thread(p).await.into_iter().filter(|(_, via, _)| *via == said).count();
    assert_eq!(reviews, 1, "posted once");

    // M265: the session's PR merges and its thread is done, though the
    // review on it is newer; the numbers count one ask, not the review.
    w.app.db.session_merged(77, &s.branch, 5, now_ms()).unwrap();
    assert!(w.app.db.done_threads(&["ch_general"]).unwrap().contains(&("ch_general".to_owned(), p as u64)));
    let counted = w.app.db.session_numbers(&w.project, 0).unwrap();
    assert_eq!(counted.iter().map(|c| c.session.as_str()).collect::<Vec<_>>(), [s.id.as_str()], "{counted:?}");
}

/// Each turn's last message is posted in the session's thread (only), as
/// the session's own (Arugula #215: no author, `via` `said:<ses>`), once
/// however often the box sends the report; a long one keeps its end, after
/// the session's link; a pushed one ends saying how many commits went to
/// which branch; an empty one, or a turn from a box that sends none, posts
/// nothing. What the agent wrote is text: its `@`s name nobody, it pushes
/// nobody, it neither prompts the session nor starts one, and it isn't its
/// asker's words (the next prompt says it's the agent's).
#[tokio::test]
async fn each_turns_answer_is_posted_in_its_thread() {
    let w = world().await;
    let (p, s) = asked(&w, "@arugula is the readme up to date?").await;
    w.take(&s.id, "claude").await;
    let (card, via) = (format!("session:{}", s.id), format!("said:{}", s.id));
    let answers = || async {
        w.thread(p)
            .await
            .into_iter()
            .filter(|(_, v, _)| *v == via)
            .map(|(author, _, text)| (author, text))
            .collect::<Vec<_>>()
    };
    let next = || async {
        let d = json!({ "prompts": queued(&w.app, &s.id) });
        assert_eq!(w.says(&s.id, Kind::State, Some(State::Working), d).await, Reported::Taken);
    };
    let turn = |reply: Option<&str>| {
        let mut d = json!({ "branch": s.branch, "pushed": false });
        if let Some(r) = reply {
            d["reply"] = json!(r);
        }
        d
    };

    // Unpushed, with a reply that @-names people and @arugula: posted, as
    // the session's, in the thread only; sent again, it's Already, and
    // nothing more.
    let pushes = crate::chat_threads_tests::pushed(&w.hub).await.len();
    let said = "Mostly: @b the install steps are stale. @arugula fix them? @channel";
    let r = w.report(&s.id, Kind::Turn, None, turn(Some(said)));
    assert_eq!(w.send(&r).await, Reported::Taken);
    assert_eq!(w.send(&r).await, Reported::Already);
    assert_eq!(answers().await, [(String::new(), said.to_owned())], "nobody's: the session's");
    assert_eq!(w.says(&s.id, Kind::State, Some(State::Done), json!({})).await, Reported::Taken);
    let (_, v) = call(&w.base, "a", "GET", "/api/chat/rooms/ch_general/messages", None).await;
    let timeline: Vec<&str> = v["messages"].as_array().unwrap().iter().filter_map(|m| m["via"].as_str()).collect();
    assert_eq!(timeline, [card.as_str()], "the card only, not the answer: {timeline:?}");
    let (_, v) = call(&w.base, "b", "GET", &format!("/api/chat/rooms/ch_general/messages?thread={p}"), None).await;
    let posted = v["messages"].as_array().unwrap().iter().find(|m| m["text"] == said).unwrap().clone();
    assert_eq!(posted["mentions"], json!([]), "it names nobody: {posted}");
    assert_eq!(crate::chat_threads_tests::pushed(&w.hub).await.len(), pushes, "it pushes nobody");
    assert_eq!(w.app.db.session_prompts(&s.id).unwrap().len(), 1, "it isn't a prompt to the session");
    assert_eq!(crate::sessions_hydrate_tests::sessions(&w.app, &w.project).len(), 1, "and starts none");
    // Read back as the thread's record (a next session's first prompt),
    // it's the agent's, not a's own words.
    let room = w.app.db.chat_room("ch_general").unwrap().unwrap();
    let record = crate::sessions::hydrate::first_prompt(&w.app, &room, p as u64, &w.project, "a", None, None, "x")
        .await
        .unwrap();
    let short = &s.id[4..12];
    assert!(record.contains(&format!("agent session {short} (agent): {said}")), "{record}");
    assert_eq!(record.matches(said).count(), 1, "{record}");

    // A person's reply prompts it; the next turn's answer is long and
    // pushed: its end (where the caveats are) after the session's link,
    // then what it pushed (Arugula #215).
    post_in(&w.base, "ch_general", "b", "and the changelog?", Some(p)).await;
    next().await;
    let long = format!("THE START {}Caveat: THE END", "words ".repeat(1000));
    let mut d = turn(Some(&long));
    d["pushed"] = json!(true);
    d["commits"] = json!(["abc1234 the changelog", "def5678 the readme"]);
    assert_eq!(w.says(&s.id, Kind::Turn, None, d).await, Reported::Taken);
    let got = answers().await;
    assert_eq!(got.len(), 2, "{got:?}");
    let (head, rest) = got[1].1.split_once("\n\n").unwrap();
    let link = format!("https://control.example/p/{}/sessions/{}", w.project, s.id);
    assert_eq!(head, format!("{} {link}", crate::sessions::answer::THE_START));
    let (kept, line) = rest.rsplit_once("\n\n").unwrap();
    assert!(kept.chars().count() <= crate::sessions::answer::ANSWER_MAX + 1, "{}", kept.chars().count());
    assert!(kept.starts_with("…words") && kept.ends_with("Caveat: THE END") && !kept.contains("THE START"), "{kept}");
    assert_eq!(line, format!("Pushed 2 commits to `{}`.", s.branch));
    assert_eq!(w.says(&s.id, Kind::State, Some(State::Done), json!({})).await, Reported::Taken);

    // An empty reply, and an older box's turn with none: nothing.
    post_in(&w.base, "ch_general", "a", "thanks", Some(p)).await;
    next().await;
    assert_eq!(w.says(&s.id, Kind::Turn, None, turn(Some("  \n"))).await, Reported::Taken);
    assert_eq!(w.says(&s.id, Kind::Turn, None, turn(None)).await, Reported::Taken);
    assert_eq!(answers().await.len(), 2);
}

/// A turn someone typed in the session's block (Arugula #215): the box's
/// report of its start says who and how it starts, and the thread gets
/// one plain line of it, the platform's, once however often the report
/// comes; it names nobody, pushes nobody and prompts nothing. A turn the
/// agent woke itself for says nothing, and only a turn started in the
/// block may say what was typed.
#[tokio::test]
async fn a_turn_typed_in_the_block_is_said_in_its_thread() {
    let w = world().await;
    let (p, s) = asked(&w, "@arugula is the readme up to date?").await;
    w.take(&s.id, "claude").await;
    w.turn(&s.id, false, "").await;
    let lines = || async {
        w.thread(p)
            .await
            .into_iter()
            .filter(|(author, via, _)| author.is_empty() && via == "system")
            .map(|(_, _, text)| text)
            .collect::<Vec<_>>()
    };
    let before = lines().await.len();
    let pushes = crate::chat_threads_tests::pushed(&w.hub).await.len();

    // Typed in the block by ann: one line, on one line, sent again or not.
    let typed = json!({ "in_block": true, "by": "ann", "typed": "@b check the\nchangelog  too" });
    let r = w.report(&s.id, Kind::State, Some(State::Working), typed);
    assert_eq!(w.send(&r).await, Reported::Taken);
    assert_eq!(w.send(&r).await, Reported::Already);
    let said = lines().await[before..].to_vec();
    assert_eq!(said, ["Asked in the session by ann: “@b check the changelog too”"]);
    let (_, v) = call(&w.base, "b", "GET", &format!("/api/chat/rooms/ch_general/messages?thread={p}"), None).await;
    let m = v["messages"].as_array().unwrap().iter().find(|m| m["text"] == said[0].as_str()).unwrap().clone();
    assert_eq!(m["mentions"], json!([]), "it names nobody: {m}");
    assert_eq!(crate::chat_threads_tests::pushed(&w.hub).await.len(), pushes, "it pushes nobody");
    assert_eq!(w.app.db.session_prompts(&s.id).unwrap().len(), 1, "it isn't a prompt to the session");
    w.turn(&s.id, false, "").await;

    // The agent woke itself: nothing typed, nothing said.
    assert_eq!(w.says(&s.id, Kind::State, Some(State::Working), json!({ "in_block": true })).await, Reported::Taken);
    assert_eq!(lines().await.len(), before + 1);

    // Only a turn started in the block says what was typed.
    let not = w.says(&s.id, Kind::State, Some(State::Done), json!({ "typed": "x" })).await;
    assert!(matches!(not, Reported::Refused { .. }), "{not:?}");
    assert_eq!(w.says(&s.id, Kind::State, Some(State::Done), json!({})).await, Reported::Taken);
    assert_eq!(lines().await.len(), before + 1);
}
