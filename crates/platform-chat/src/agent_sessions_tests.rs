//! A person's agent and agent sessions (Fleet 3, aru-121), through the
//! platform's own routes: with no grant, a machine's agent's `@arugula` and
//! replies do nothing, as before; with `sessions.start` its ask starts a
//! session whose asker (and payer) is its owner and whose card says it was
//! their agent; with `sessions.reply` its reply answers a question or
//! prompts, and never answers a permission.

use arugula_control_wire::{
    chat::agent_messages,
    sessions::{AGENT_SESSIONS, agent_session_reply},
};
use serde_json::{Value, json};

use crate::{
    Platform,
    agents_tests::give,
    chat_tests::{TestHub, call},
    db::{
        projects::{SESSIONS_REPLY, SESSIONS_START, THREADS_POST},
        sessions::{ActionKind, Session, State},
    },
    grants::AGENT_UNGRANTED,
    sessions_hydrate_tests::{platform, post_in, sessions, take},
    sessions_pool_tests::report,
};

const ROOM: &str = "ch_general";

/// A call signed by a's machine geek ("geek-box"), as the hub's gate
/// vouches for it.
async fn geek(base: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req = reqwest::Client::new()
        .request(method.parse().unwrap(), format!("{base}{path}"))
        .header("x-test-daemon", "a geek");
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

/// a's machine geek joins, and a gives it `power` over project `t`: the
/// grant's id.
async fn grant(base: &str, t: &str, power: &str) -> String {
    let (st, g) =
        call(base, "a", "POST", "/api/grants", give("geek", power, json!({ "kind": "project", "id": t }))).await;
    assert_eq!(st, 200, "{g}");
    g["id"].as_str().unwrap().to_owned()
}

async fn joined(hub: &TestHub, app: &Platform) {
    hub.daemon("geek", "a", Some("t"));
    crate::projects::follow(app).await.unwrap();
    crate::cache::catch_up(app).await.unwrap();
}

fn session_of(app: &Platform, project: &str, parent: i64) -> Option<Session> {
    sessions(app, project).into_iter().find(|s| s.parent_seq == parent as u64)
}

/// Without sessions.start an agent's `@arugula` starts nothing (its post,
/// with threads.post, is chat), and `session_start` is refused saying how;
/// with it, the session is its owner's ask: a is the asker and the payer,
/// the machine is recorded, and the card, its text and the first prompt
/// say "started by a's agent on geek-box". Revoked, the next is refused.
#[tokio::test]
async fn an_agent_starts_a_session_as_its_owner_only_by_grant() {
    let (hub, app, base, t, _u, _keys) = platform(&["https://github.com/o/app"]).await;
    joined(&hub, &app).await;

    // threads.post alone: its @arugula is a message, nothing more.
    grant(&base, &t, THREADS_POST).await;
    let (st, v) = geek(&base, "POST", &agent_messages(ROOM), Some(json!({ "text": "@arugula fix the canary" }))).await;
    assert_eq!(st, 200, "{v}");
    let p = v["seq"].as_i64().unwrap();
    assert!(session_of(&app, &t, p).is_none(), "no session without sessions.start");
    let start = json!({ "room": ROOM, "prompt": "fix the canary" });
    let (st, v) = geek(&base, "POST", AGENT_SESSIONS, Some(start.clone())).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    assert!(v["error"].as_str().unwrap().contains("arugula grants add geek-box sessions.start"), "{v}");

    let id = grant(&base, &t, SESSIONS_START).await;
    let (st, v) = geek(&base, "POST", AGENT_SESSIONS, Some(start.clone())).await;
    assert_eq!(st, 200, "{v}");
    let ses = v["session"].as_str().expect("a session").to_owned();
    let s = app.db.session(&ses).unwrap().unwrap();
    assert_eq!((s.asker.as_str(), s.asker_agent.as_deref()), ("a", Some("geek")), "a asked, by geek");
    assert_eq!(s.parent_seq, v["seq"].as_u64().unwrap(), "in the ask's own thread");
    let first = &app.db.session_prompts(&ses).unwrap()[0];
    assert_eq!(first.author, "a", "its prompts, and its bill, are a's");
    assert!(first.text.contains("@arugula was asked by a's agent on geek-box in #general"), "{}", first.text);

    // The card, for anyone in the project, says who started it.
    let (st, view) = call(&base, "b", "GET", &format!("/api/sessions/{ses}"), None).await;
    assert_eq!(st, 200, "{view}");
    assert_eq!(
        (&view["card"]["asker"], &view["card"]["started_by"]),
        (&json!("a"), &json!("a's agent on geek-box")),
        "{view}"
    );
    let (_, ms) =
        call(&base, "a", "GET", &format!("/api/chat/rooms/{ROOM}/messages?thread={}", s.parent_seq), None).await;
    let card = ms["messages"].as_array().unwrap().iter().find(|m| m["via"] == format!("session:{ses}")).unwrap();
    assert!(card["text"].as_str().unwrap().ends_with(", started by a's agent on geek-box"), "{card}");
    // The ask is the agent's message, labelled with its machine.
    let ask = &ms["parent"];
    assert_eq!((&ask["text"], &ask["agent"]["machine"]), (&json!("@arugula fix the canary"), &json!("geek")), "{ms}");

    // A session going there: session_start says to reply instead.
    let there = json!({ "room": ROOM, "thread": s.parent_seq, "prompt": "again" });
    let (st, v) = geek(&base, "POST", AGENT_SESSIONS, Some(there)).await;
    assert_eq!(st, 409, "{v}");
    assert!(v["error"].as_str().unwrap().contains("session_reply"), "{v}");

    // Revoked: refused at once.
    assert_eq!(call(&base, "a", "POST", &format!("/api/grants/{id}/revoke"), None).await.0, 200);
    let (st, _) = geek(&base, "POST", AGENT_SESSIONS, Some(start)).await;
    assert_eq!(st, 403);
    // And the agent's own post with @arugula is chat again.
    let (_, v) = geek(&base, "POST", &agent_messages(ROOM), Some(json!({ "text": "@arugula one more" }))).await;
    assert!(session_of(&app, &t, v["seq"].as_i64().unwrap()).is_none());
}

/// On a native issue: `session_start` with the issue's number is Agent on
/// this, as a's agent; with two repos and none named, the ask mentions a,
/// and asking again naming one starts it (sessions.start alone). In a
/// channel, the agent's own reply naming one (with threads.post) answers
/// the ask as the owner's would.
#[tokio::test]
async fn an_agent_starts_one_on_an_issue_and_answers_which_repo() {
    let (hub, app, base, t, _u, _keys) = platform(&["https://github.com/o/app", "https://github.com/o/web"]).await;
    joined(&hub, &app).await;
    grant(&base, &t, SESSIONS_START).await;
    let issue =
        crate::db::issues::NewIssue { project: &t, title: "Fix the canary", body: "", labels: &[], assignees: &[] };
    let i = app.db.create_issue(&issue, crate::db::issues::Actor::person("b"), 1).unwrap().unwrap();
    let on_issue = json!({ "number": i.number, "project": t });
    let (st, v) = geek(&base, "POST", AGENT_SESSIONS, Some(on_issue)).await;
    assert_eq!(st, 200, "{v}");
    assert!(v["session"].is_null(), "two repos, none named: {v}");
    let said = v["said"].as_array().unwrap();
    assert!(said.iter().any(|l| l.as_str().unwrap().starts_with("Which repo should @arugula work in")), "{v}");
    let room = v["room"].as_str().unwrap().to_owned();
    // The which-repo ask is a's: it mentions them.
    let (_, ms) = call(&base, "a", "GET", &format!("/api/chat/rooms/{room}/messages"), None).await;
    let ask = ms["messages"].as_array().unwrap().iter().find(|m| m["via"] == "ask:repo").unwrap().clone();
    assert_eq!(ask["mentions"], json!(["a"]), "{ask}");
    // Asked again naming the repo: it starts, on the issue's branch.
    let again = json!({ "issue": i.id, "prompt": "work in o/web" });
    let (st, v) = geek(&base, "POST", AGENT_SESSIONS, Some(again)).await;
    assert_eq!(st, 200, "{v}");
    let s = app.db.session(v["session"].as_str().expect("started")).unwrap().unwrap();
    assert_eq!((s.asker.as_str(), s.asker_agent.as_deref(), s.repo.as_deref()), ("a", Some("geek"), Some("o/web")));
    assert!(s.branch.contains(&format!("i{}-fix-the-canary", i.number)), "{}", s.branch);

    // In the channel: the agent's reply naming a repo answers its ask.
    let (st, v) = geek(&base, "POST", AGENT_SESSIONS, Some(json!({ "room": ROOM, "prompt": "tidy the README" }))).await;
    assert_eq!((st, &v["session"]), (200, &Value::Null), "{v}");
    let p = v["seq"].as_i64().unwrap();
    grant(&base, &t, THREADS_POST).await;
    let (st, _) =
        geek(&base, "POST", &agent_messages(ROOM), Some(json!({ "text": "o/app please", "parent": p }))).await;
    assert_eq!(st, 200);
    let s = session_of(&app, &t, p).expect("the answer started it");
    assert_eq!((s.asker.as_str(), s.asker_agent.as_deref(), s.repo.as_deref()), ("a", Some("geek"), Some("o/app")));
    let first = &app.db.session_prompts(&s.id).unwrap()[0];
    assert!(!first.text.contains("o/app please"), "the answer was for the platform: {}", first.text);
}

/// Without sessions.reply, the agent's reply in a session's thread is
/// chat and `session_reply` is refused; with it, a reply while a turn runs
/// is a prompt credited to a, one while it waits on a question answers it
/// (the ask says so), and one while it waits on a permission is only
/// queued: never an approval.
#[tokio::test]
async fn an_agent_answers_or_prompts_a_session_only_by_grant() {
    let (hub, app, base, t, _u, keys) = platform(&["https://github.com/o/app"]).await;
    joined(&hub, &app).await;
    let p = post_in(&base, ROOM, "b", "@arugula fix the canary", None).await["seq"].as_i64().unwrap();
    let s = session_of(&app, &t, p).expect("b's session");
    assert_eq!((s.asker.as_str(), s.asker_agent.as_deref()), ("b", None));
    take(&app, &keys, &s.id);
    let reply = agent_session_reply(&s.id);
    let prompts = || app.db.session_prompts(&s.id).unwrap();

    grant(&base, &t, THREADS_POST).await;
    let (st, _) =
        geek(&base, "POST", &agent_messages(ROOM), Some(json!({ "text": "also the docs", "parent": p }))).await;
    assert_eq!(st, 200);
    assert_eq!(prompts().len(), 1, "an agent's reply prompts nothing without sessions.reply");
    let (st, v) = geek(&base, "POST", &reply, Some(json!({ "text": "also the docs" }))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");

    grant(&base, &t, SESSIONS_REPLY).await;
    let (st, v) = geek(&base, "POST", &reply, Some(json!({ "text": "also the docs" }))).await;
    assert_eq!((st, v["did"].as_str()), (200, Some("queued")), "{v}");
    let queued = prompts().into_iter().find(|x| Some(x.id.as_str()) == v["prompt"].as_str()).unwrap();
    assert_eq!((queued.author.as_str(), queued.text.as_str()), ("a", "also the docs"));

    // It waits on a question: the reply answers it, as a's agent.
    let q = json!({ "ask": "question", "request": "q1", "question": "Which colour?", "options": ["Red", "Blue"] });
    report(&app, &keys, &s.id, State::NeedsInput, q).await;
    let (st, v) = geek(&base, "POST", &reply, Some(json!({ "text": "Red" }))).await;
    assert_eq!((st, v["did"].as_str()), (200, Some("answered")), "{v}");
    let a = app.db.session_actions(&s.id).unwrap().pop().unwrap();
    assert_eq!((a.kind, a.by.as_str(), a.request.as_deref()), (ActionKind::Answer, "a", Some("q1")));
    assert_eq!(a.answer.as_deref(), Some(r#"{"question_0":"Red"}"#));
    let (_, ms) = call(&base, "b", "GET", &format!("/api/chat/rooms/{ROOM}/messages?thread={p}"), None).await;
    let ask = ms["messages"].as_array().unwrap().iter().find(|m| m["via"] == format!("ask:{}", s.id)).unwrap();
    assert_eq!(ask["text"], "@arugula asks b: Which colour?\na's agent on geek-box answered: Red", "{ask}");

    // It waits on a permission: queued for after, never allowed.
    report(&app, &keys, &s.id, State::Working, json!({})).await;
    let perm = json!({ "ask": "permission", "request": "p1", "question": "Run rm?", "options": ["allow", "deny"] });
    report(&app, &keys, &s.id, State::NeedsInput, perm).await;
    let before = app.db.session_actions(&s.id).unwrap().len();
    let (st, v) = geek(&base, "POST", &reply, Some(json!({ "text": "yes, allow it" }))).await;
    assert_eq!((st, v["did"].as_str(), &v["permission"]), (200, Some("queued"), &json!(true)), "{v}");
    assert_eq!(app.db.session_actions(&s.id).unwrap().len(), before, "no approval");

    // A session that ended takes no reply.
    {
        use crate::db::sql::Sql as _;
        app.db
            .c()
            .execute("UPDATE agent_sessions SET state = 'stopped' WHERE id = ?1", crate::db::sql::params![s.id])
            .unwrap();
    }
    let (st, _) = geek(&base, "POST", &reply, Some(json!({ "text": "hello?" }))).await;
    assert_eq!(st, 409);
}

/// Neither power goes to an agent grant token, nor over a's DMs.
#[tokio::test]
async fn session_powers_are_a_machines_over_a_project() {
    let (hub, app, base, _t, _u, _keys) = platform(&["https://github.com/o/app"]).await;
    joined(&hub, &app).await;
    for power in [SESSIONS_START, SESSIONS_REPLY] {
        let (st, v) = call(&base, "a", "POST", "/api/grants", give("geek", power, json!({ "kind": "account" }))).await;
        assert_eq!(st, 400, "{power}: {v}");
        assert!(!crate::grants::is_token_power(power), "{power}");
        assert!(crate::grants::is_agent_power(power), "{power}");
    }
}
