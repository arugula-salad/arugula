//! The pool and asks (Arugula #214) through the platform's own routes:
//! which box `@arugula` picks with several and none chosen, what it asks
//! and when, the ask as a form and a mention of its asker until it's
//! answered, the agent's own question the same way, and Move to….

use std::time::Duration;

use arugula_control_wire::work::SessionReport;
use arugula_e2e::DeviceKeys;
use serde_json::{Value, json};

use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::{
        sessions::{Kind, PromptState, Session, State},
        sql::{Sql as _, params},
    },
    sessions_hydrate_tests::{add_box, box_says, lines, platform, post_in, sessions},
    work::HostedMachine,
};

const ROOM: &str = "ch_general";

/// The asks (`via` `ask:…`) in `thread`, as `who` reads them: their
/// `via`, text and whom they mention.
async fn asks_in(base: &str, who: &str, thread: i64) -> Vec<(String, String, Value)> {
    let (st, v) = call(base, who, "GET", &format!("/api/chat/rooms/{ROOM}/messages?thread={thread}"), None).await;
    assert_eq!(st, 200, "{v}");
    v["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["via"].as_str().is_some_and(|v| v.starts_with("ask:")))
        .map(|m| (m["via"].as_str().unwrap().to_owned(), m["text"].as_str().unwrap().to_owned(), m["mentions"].clone()))
        .collect()
}

/// `who`'s unread mentions in the channel's list, and in the Threads view.
async fn mentions_of(base: &str, who: &str) -> (u64, u64) {
    let (st, v) = call(base, who, "GET", "/api/chat", None).await;
    assert_eq!(st, 200, "{v}");
    let room = v["rooms"].as_array().unwrap().iter().find(|r| r["id"] == ROOM).unwrap().clone();
    (room["mentions"].as_u64().unwrap(), v["threads_mentions"].as_u64().unwrap_or(0))
}

/// The pushes so far, to whom and their titles.
async fn pushed(hub: &TestHub) -> Vec<(String, String)> {
    tokio::time::sleep(Duration::from_millis(150)).await;
    hub.pushes().into_iter().map(|(to, msg, _)| (to, msg["title"].as_str().unwrap_or_default().to_owned())).collect()
}

/// `who` asks `@arugula …` in the channel: the message's number and the
/// session it started, if it did.
async fn ask(app: &Platform, base: &str, project: &str, who: &str, text: &str) -> (i64, Option<Session>) {
    let p = post_in(base, ROOM, who, text, None).await["seq"].as_i64().unwrap();
    let s = sessions(app, project).into_iter().find(|s| s.parent_seq == p as u64);
    (p, s)
}

/// Link `repo` to `project`, as its GitHub App would.
fn link(app: &Platform, project: &str, id: u64, repo: &str) {
    app.db
        .c()
        .execute(
            "INSERT INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_at)
             VALUES (?1, ?2, FALSE, ?3, 1, 1)",
            params![id, repo, project],
        )
        .unwrap();
}

/// With several boxes and none chosen, the session goes to one with the
/// repo the ask names; with that repo on one box only, that one, whatever
/// else is free.
#[tokio::test]
async fn the_pool_picks_a_box_with_the_repo() {
    let (hub, app, base, t, _u, keys) = platform(&["https://github.com/o/app", "https://github.com/o/web"]).await;
    hub.set_online(&keys.id(), true);
    add_box(&app, &t, "hw_site", "site", &["https://github.com/o/site"], Some("m_site"));
    add_box(&app, &t, "hw_api", "api", &["https://github.com/o/app"], Some("m_api"));
    hub.set_online("m_site", true);
    hub.set_online("m_api", true);
    let (_, s) = ask(&app, &base, &t, "b", "@arugula fix the o/site footer").await;
    let s = s.expect("started");
    assert_eq!((s.workspace.as_str(), s.repo.as_deref()), ("hw_site", Some("o/site")));
    // Only dev has web: it, though api is free too.
    let (_, s) = ask(&app, &base, &t, "b", "@arugula fix the web canary").await;
    assert_eq!(s.map(|s| (s.workspace, s.repo)), Some(("hw_dev".into(), Some("o/web".into()))));
    // app is on dev and api: the less busy of the two.
    let (_, s) = ask(&app, &base, &t, "b", "@arugula bump the app's deps").await;
    assert_eq!(s.map(|s| (s.workspace, s.repo)), Some(("hw_api".into(), Some("o/app".into()))));
}

/// Of the boxes with the repo: awake with a free slot first, then the
/// least busy, then by name. A box that's down, or full, waits its turn.
#[tokio::test]
async fn the_pool_prefers_awake_with_a_free_slot_then_least_busy() {
    let (hub, app, base, t, _u, _keys) = platform(&["https://github.com/o/app"]).await;
    // dev is ready but off the relay: not awake.
    add_box(&app, &t, "hw_c", "c-three", &["https://github.com/o/app"], Some("m_c"));
    add_box(&app, &t, "hw_b", "b-two", &["https://github.com/o/app"], Some("m_b"));
    hub.set_online("m_b", true);
    hub.set_online("m_c", true);
    let mut went = Vec::new();
    for i in 0..7 {
        let (_, s) = ask(&app, &base, &t, "b", &format!("@arugula task {i}")).await;
        went.push(s.expect("started").workspace);
    }
    // b and c take turns, by name when even, until both hold three; then
    // dev, down but the least busy.
    assert_eq!(went, ["hw_b", "hw_c", "hw_b", "hw_c", "hw_b", "hw_c", "hw_dev"]);
    // The hub can't say who's online: ready is awake, and dev is free.
    hub.online_down(true);
    let (_, s) = ask(&app, &base, &t, "b", "@arugula task 8").await;
    assert_eq!(s.unwrap().workspace, "hw_dev");
}

/// It asks only when it must: which repo when the boxes have several and
/// the ask names none; it says so when the ask names a linked repo no box
/// has. An explicit "agents run on" wins over the pool.
#[tokio::test]
async fn it_asks_only_when_the_repo_is_missing_or_ambiguous() {
    let (hub, app, base, t, _u, keys) = platform(&["https://github.com/o/app"]).await;
    hub.set_online(&keys.id(), true);
    add_box(&app, &t, "hw_web", "web", &["https://github.com/o/web"], Some("m_web"));
    link(&app, &t, 1, "o/app");
    link(&app, &t, 2, "o/web");
    link(&app, &t, 3, "o/docs");
    let (p, s) = ask(&app, &base, &t, "b", "@arugula fix the canary").await;
    assert!(s.is_none());
    let ask_text = "Which repo should @arugula work in? Reply with one: o/app or o/web.";
    assert_eq!(asks_in(&base, "a", p).await, [("ask:repo".into(), ask_text.into(), json!(["b"]))]);
    assert!(lines(&base, ROOM, Some(p)).await.is_empty(), "an ask, not a system line");
    let (q, s) = ask(&app, &base, &t, "b", "@arugula update o/docs").await;
    assert!(s.is_none());
    assert_eq!(lines(&base, ROOM, Some(q)).await, [crate::chat::sessions::missing("o/docs")]);
    assert!(asks_in(&base, "a", q).await.is_empty());

    // Agents run on web: there, in its repo, though dev has app and is free.
    assert!(app.db.set_agents_run_on(&t, Some("hw_web")).unwrap());
    let (_, s) = ask(&app, &base, &t, "b", "@arugula fix the app").await;
    assert_eq!(s.map(|s| (s.workspace, s.repo)), Some(("hw_web".into(), Some("o/web".into()))));
}

/// The which-repo ask is a form (`/api/chat/rooms/{room}/asks/{seq}`), in
/// the channel too, and its asker's unread mention, pushed once, until
/// they answer it: from the channel, by its button. Only they may.
#[tokio::test]
async fn the_repo_ask_is_a_form_and_a_mention_until_answered() {
    let (hub, app, base, t, _u, _keys) = platform(&["https://github.com/o/app", "https://github.com/o/web"]).await;
    let before = pushed(&hub).await.len();
    let (p, _) = ask(&app, &base, &t, "b", "@arugula fix the canary").await;
    // In the channel's own list, as a reply sent there too.
    let (st, main) = call(&base, "a", "GET", &format!("/api/chat/rooms/{ROOM}/messages"), None).await;
    assert_eq!(st, 200);
    let asked = main["messages"].as_array().unwrap().iter().find(|m| m["via"] == "ask:repo").expect("in the channel");
    assert_eq!((&asked["parent"], &asked["also"]), (&json!(p), &json!(true)), "{asked}");
    let seq = asked["seq"].as_i64().unwrap();
    assert_eq!(mentions_of(&base, "b").await, (1, 1), "the asker's mention, in the channel and the thread");
    assert_eq!(mentions_of(&base, "a").await.0, 0, "nobody else's");
    let to_b: Vec<_> = pushed(&hub).await.into_iter().skip(before).filter(|(to, _)| to == "b").collect();
    assert_eq!(to_b, [("b".to_owned(), "@arugula mentioned you in #general".to_owned())], "pushed once");

    let form = format!("/api/chat/rooms/{ROOM}/asks/{seq}");
    let (st, f) = call(&base, "b", "GET", &form, None).await;
    assert_eq!(st, 200, "{f}");
    assert_eq!(
        (&f["choices"], &f["open"], &f["mine"], &f["asker"]),
        (&json!(["o/app", "o/web"]), &json!(true), &json!(true), &json!("b"))
    );
    let (_, f) = call(&base, "a", "GET", &form, None).await;
    assert_eq!(f["mine"], false);
    let (st, _) = call(&base, "a", "POST", &form, Some(json!({ "choice": "o/web" }))).await;
    assert_eq!(st, 403, "only its asker answers it");
    let (st, _) = call(&base, "b", "POST", &form, Some(json!({ "choice": "o/nope" }))).await;
    assert_eq!(st, 400);
    let (st, f) = call(&base, "b", "POST", &form, Some(json!({ "choice": "o/web" }))).await;
    assert_eq!(st, 200, "{f}");
    let s = sessions(&app, &t).pop().expect("started");
    assert_eq!((s.repo.as_deref(), s.parent_seq), (Some("o/web"), p as u64));
    assert_eq!((&f["open"], &f["session"]), (&json!(false), &json!(s.id)));
    assert_eq!(asks_in(&base, "a", p).await[0].2, json!([]), "it mentions nobody now");
    assert_eq!(mentions_of(&base, "b").await, (0, 0), "answered: no longer a mention, though unread");
    let (st, _) = call(&base, "b", "POST", &form, Some(json!({ "choice": "o/app" }))).await;
    assert_eq!(st, 409, "answered once");
    // Answered in words, the same.
    let (q, _) = ask(&app, &base, &t, "b", "@arugula and the docs").await;
    post_in(&base, ROOM, "b", "app", Some(q)).await;
    assert!(sessions(&app, &t).iter().any(|s| s.parent_seq == q as u64 && s.repo.as_deref() == Some("o/app")));
    assert_eq!(asks_in(&base, "a", q).await[0].2, json!([]));
}

/// `who`'s box's next signed report on `s`, through the hub's route.
async fn report(app: &Platform, keys: &DeviceKeys, s: &str, to: State, detail: Value) -> Value {
    let now = app.db.session(s).unwrap().unwrap();
    let detail = detail.to_string();
    let id = keys.id();
    let ev = arugula_e2e::session::Event {
        session: s,
        n: now.daemon_n + 1,
        kind: Kind::State,
        from: now.seen_as(),
        to,
        at: arugula_e2e::now_ms(),
        detail: &detail,
        daemon: &id,
    };
    let r = SessionReport {
        session: s.into(),
        n: ev.n,
        kind: "state".into(),
        from: ev.from.as_str().into(),
        to: to.as_str().into(),
        at: ev.at,
        sig: ev.sign(keys),
        detail,
    };
    let m = HostedMachine {
        project: now.project.clone(),
        workspace: now.workspace.clone(),
        machine: id,
        sign: hex::encode(keys.sign_public()),
    };
    serde_json::to_value(crate::session_live::report(app, &m, &r).await.unwrap()).unwrap()
}

/// What the agent asks is the asker's too: an ask in the thread and the
/// channel that mentions them, pushed once, its choices the card's
/// buttons; answered from the card, it stops counting.
#[tokio::test]
async fn an_agents_question_is_an_ask_for_its_asker() {
    let (hub, app, base, t, _u, keys) = platform(&["https://github.com/o/app"]).await;
    let (p, s) = ask(&app, &base, &t, "b", "@arugula fix the canary").await;
    let s = s.unwrap();
    let first = app.db.session_prompts(&s.id).unwrap()[0].id.clone();
    report(&app, &keys, &s.id, State::Working, json!({ "prompts": [first], "block": "7" })).await;
    let before = pushed(&hub).await.len();
    let q = json!({ "ask": "question", "request": "q1", "question": "Which colour?", "options": ["Red", "Blue"] });
    report(&app, &keys, &s.id, State::NeedsInput, q).await;
    let via = format!("ask:{}", s.id);
    assert_eq!(asks_in(&base, "a", p).await, [(via.clone(), "@arugula asks b: Which colour?".into(), json!(["b"]))]);
    assert_eq!(mentions_of(&base, "b").await, (1, 1));
    let to_b: Vec<_> = pushed(&hub).await.into_iter().skip(before).filter(|(to, _)| to == "b").collect();
    assert_eq!(to_b.len(), 1, "pushed once: {to_b:?}");
    let (_, v) = call(&base, "a", "GET", &format!("/api/sessions/{}", s.id), None).await;
    assert_eq!(v["card"]["open"]["options"], json!(["Red", "Blue"]), "a button each");
    // a answers it from the card: b's ask is answered.
    let act = json!({ "kind": "answer", "request": "q1", "answer": { "question_0": "Red" } });
    let (st, _) = call(&base, "a", "POST", &format!("/api/sessions/{}/actions", s.id), Some(act)).await;
    assert_eq!(st, 200);
    assert_eq!(mentions_of(&base, "b").await, (0, 0));
    // Arugula #215: the thread says who answered, and what, under the
    // question: the agent's next words aren't it answering itself.
    assert_eq!(
        asks_in(&base, "b", p).await,
        [(via, "@arugula asks b: Which colour?\na answered: Red".into(), json!([]))]
    );
    // Its next question is a new ask; the turn going on answers it too.
    report(&app, &keys, &s.id, State::Working, json!({})).await;
    report(&app, &keys, &s.id, State::NeedsInput, json!({ "ask": "question", "request": "q2", "question": "Sure?" }))
        .await;
    assert_eq!(mentions_of(&base, "b").await.0, 1);
    report(&app, &keys, &s.id, State::Working, json!({})).await;
    assert_eq!(mentions_of(&base, "b").await.0, 0);
    let open: Vec<_> = asks_in(&base, "a", p).await.into_iter().filter(|a| a.2 != json!([])).collect();
    assert!(open.is_empty(), "{open:?}");
}

/// Arugula #215: what an answer said, in words, for its ask's line.
#[test]
fn an_answer_in_words() {
    use crate::sessions::asks::said;
    let a = json!({ "question_1": "Tea", "question_0_custom": "the dark one", "question_0": "Red", "note": "" });
    assert_eq!(said("answer", Some(&a), None), "answered: Red; the dark one; Tea", "in the order asked");
    assert_eq!(said("answer", Some(&json!({ "question_0": ["Red", "Blue"] })), None), "answered: Red; Blue");
    assert_eq!(said("answer", Some(&json!({})), None), "answered, choosing nothing");
    assert_eq!(said("approve", None, None), "allowed it");
    assert_eq!(said("deny", None, Some(" not that file ")), "denied it: not that file");
}

/// Arugula #215: "agents use model …" is the project's setting, for its
/// owners and editors to change; its sessions' work carries it to their
/// box, unless the ask named one; the card says the one asked for and
/// the one the box says it runs on.
#[tokio::test]
async fn a_projects_model_goes_to_its_box_and_its_card() {
    let (_hub, app, base, t, _u, keys) = platform(&["https://github.com/o/app"]).await;
    let path = format!("/api/projects/{t}/sessions/settings");
    let (st, v) = call(&base, "x", "GET", &path, None).await;
    assert_eq!(
        (st, v["model"].clone(), v["models"].clone()),
        (200, Value::Null, json!(["opus", "fable", "sonnet", "haiku"]))
    );
    let (st, _) = call(&base, "x", "PUT", &path, Some(json!({ "model": "sonnet" }))).await;
    assert_eq!(st, 403, "a viewer can't");
    let (st, _) = call(&base, "b", "PUT", &path, Some(json!({ "model": "gpt-9" }))).await;
    assert_eq!(st, 400, "not one of ours");
    let (st, v) = call(&base, "b", "PUT", &path, Some(json!({ "model": "sonnet" }))).await;
    assert_eq!((st, v["model"].clone()), (200, json!("sonnet")));
    assert_eq!(app.db.agents_model(&t).unwrap().as_deref(), Some("sonnet"), "kept");

    let (_, s) = ask(&app, &base, &t, "b", "@arugula fix the canary").await;
    let s = s.unwrap();
    let (_, named) = ask(&app, &base, &t, "b", "@arugula tidy the README, use haiku").await;
    let named = named.unwrap();
    let work = app.db.session_work("hw_dev").unwrap();
    let model_of = |id: &str| work.iter().find(|w| w.session == id).and_then(|w| w.model.clone());
    assert_eq!(model_of(&s.id).as_deref(), Some("sonnet"), "the project's");
    assert_eq!(model_of(&named.id).as_deref(), Some("haiku"), "the ask's own word wins");

    // The box says which it runs on; the card shows it.
    let first = app.db.session_prompts(&named.id).unwrap()[0].id.clone();
    report(&app, &keys, &named.id, State::Working, json!({ "prompts": [first], "block": "7", "model": "Haiku 4.5" }))
        .await;
    let (_, v) = call(&base, "x", "GET", &format!("/api/sessions/{}", named.id), None).await;
    assert_eq!((v["card"]["model_asked"].clone(), v["card"]["model"].clone()), (json!("haiku"), json!("Haiku 4.5")));
    let (_, v) = call(&base, "x", "GET", &format!("/api/sessions/{}", s.id), None).await;
    assert_eq!((v["card"]["model_asked"].clone(), v["card"]["model"].clone()), (json!("sonnet"), Value::Null));
    // Back to the strongest: the box picks it.
    let (st, v) = call(&base, "a", "PUT", &path, Some(json!({ "model": null }))).await;
    assert_eq!((st, v["model"].clone()), (200, Value::Null));
    assert_eq!(app.db.session_work("hw_dev").unwrap().iter().find(|w| w.session == s.id).unwrap().model, None);
}

/// Move to…: a done session goes to another box with its repo, queued
/// there from the thread, its old box asked for that block's transcript
/// first; a queued one just goes. Not mid-turn, and not by a viewer.
#[tokio::test]
async fn move_to_requeues_on_the_chosen_box() {
    use arugula_control_wire::forge::{DaemonOp, OpAnswer};
    let (hub, app, base, t, _u, keys) = platform(&["https://github.com/o/app"]).await;
    add_box(&app, &t, "hw_two", "two", &["https://github.com/o/app"], Some("m_two"));
    add_box(&app, &t, "hw_web", "web", &["https://github.com/o/web"], Some("m_web"));
    assert!(app.db.set_agents_run_on(&t, Some("hw_dev")).unwrap());
    hub.on_ops(std::sync::Arc::new(|_, op| {
        Box::pin(async move {
            match op {
                DaemonOp::HostedArchiveBlocks(b) if b.blocks == [7] => OpAnswer::Done { url: None },
                _ => OpAnswer::Unknown,
            }
        })
    }));
    let (_, s) = ask(&app, &base, &t, "b", "@arugula fix the canary").await;
    let s = s.unwrap();
    let path = format!("/api/sessions/{}/move", s.id);
    let (_, v) = call(&base, "b", "GET", &format!("/api/sessions/{}", s.id), None).await;
    let moves: Vec<&str> = v["card"]["moves"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
    assert_eq!(moves, ["two"], "the others with its repo");
    let (_, v) = call(&base, "x", "GET", &format!("/api/sessions/{}", s.id), None).await;
    assert_eq!(v["card"]["moves"], json!([]), "a viewer has none");
    let (st, _) = call(&base, "x", "POST", &path, Some(json!({ "workspace": "hw_two" }))).await;
    assert_eq!(st, 403, "a viewer can't");
    let (st, _) = call(&base, "b", "POST", &path, Some(json!({ "workspace": "hw_web" }))).await;
    assert_eq!(st, 400, "web hasn't its repo");

    // Mid-turn: refused.
    let first = app.db.session_prompts(&s.id).unwrap()[0].id.clone();
    let took = json!({ "prompts": [first], "block": "7" }).to_string();
    box_says(&app, &keys, &s.id, 1, State::Queued, State::Working, &took);
    let (st, v) = call(&base, "b", "POST", &path, Some(json!({ "workspace": "hw_two" }))).await;
    assert_eq!(st, 409, "{v}");
    // Done: moved, its block's transcript asked for first.
    box_says(&app, &keys, &s.id, 2, State::Working, State::Done, "{}");
    let (st, v) = call(&base, "a", "POST", &path, Some(json!({ "workspace": "hw_two" }))).await;
    assert_eq!(st, 200, "an owner may: {v}");
    let after = app.db.session(&s.id).unwrap().unwrap();
    assert_eq!(
        (after.state, after.workspace.as_str(), after.machine.as_deref(), after.block.as_deref()),
        (State::Queued, "hw_two", None, None)
    );
    let asked: Vec<String> = hub.ops_asked().into_iter().map(|(m, op)| format!("{m} {}", op.name())).collect();
    assert_eq!(asked, [format!("{} hosted.archive_blocks", keys.id())]);
    let prompts = app.db.session_prompts(&s.id).unwrap();
    let again = prompts.iter().find(|p| p.state == PromptState::Queued).expect("its next turn's prompt");
    assert_eq!(again.author, "b", "the asker's");
    assert!(again.text.starts_with("This session moved to another box"), "{}", again.text);
    assert_eq!(v["card"]["workspace_name"], "two");
    let last = v["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["text"], "Moved to two by a, without its conversation: starting again from the thread");
    assert!(app.db.session_work("hw_two").unwrap().iter().any(|w| w.session == s.id), "the new box's work");
    assert!(app.db.session_work("hw_dev").unwrap().iter().all(|w| w.session != s.id), "not the old one's");
    // The old box's reports are refused now.
    let late = crate::sessions_hydrate_tests::box_report(&app, &keys, &s.id, 3, State::Queued, State::Working, "{}");
    assert!(late.is_err());

    // Queued: it just goes back, its prompt with it.
    let (st, v) = call(&base, "b", "POST", &path, Some(json!({ "workspace": "hw_dev" }))).await;
    assert_eq!(st, 200, "{v}");
    let back = app.db.session(&s.id).unwrap().unwrap();
    assert_eq!((back.state, back.workspace.as_str()), (State::Queued, "hw_dev"));
    let queued = app.db.session_prompts(&s.id).unwrap().into_iter().filter(|p| p.state == PromptState::Queued);
    assert_eq!(queued.count(), 1);
}
