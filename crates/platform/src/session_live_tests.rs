//! A session's card through the platform's routes (M257), with chat's
//! test hub: the card's one message in its thread, the card read by the
//! project's members only, every move told through `chat_events`, an
//! answer from a second member (the first wins, viewers can't), a cancel,
//! Stop, and the ticker marking a quiet box's session.

use std::sync::Arc;

use arugula_e2e::{DeviceKeys, team::TeamRole};
use serde_json::{Value, json};

use super::*;
use crate::{
    chat::session_card::post_card,
    chat_tests::{TestHub, call, serve},
    db::{
        Db,
        sessions::{DaemonReport, NewPrompt, NewSession, Source},
        sql::{Sql as _, params},
    },
};

/// Team t: a owns it, b edits, v views; c is in no team of it. A channel
/// with a's message asking for the session, a hosted workspace whose box
/// is `machine`, and the session, its card posted.
async fn seed(app: &Platform, hub: &TestHub, base: &str, machine: &str) -> (Session, String, i64) {
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("v", TeamRole::Viewer)]);
    hub.team("u", &[("c", TeamRole::Owner)]);
    hub.flag("team:t", true);
    let project = app.db.team_project("t", "t", 1).unwrap();
    let (st, ch) = call(base, "a", "POST", "/api/chat/teams/t/channels", Some(json!({ "name": "general" }))).await;
    assert_eq!(st, 200, "{ch}");
    let room = ch["id"].as_str().unwrap().to_owned();
    for who in ["b", "v"] {
        call(base, who, "POST", &format!("/api/chat/rooms/{room}/join"), None).await;
    }
    let (st, m) = call(
        base,
        "a",
        "POST",
        &format!("/api/chat/rooms/{room}/messages"),
        Some(json!({ "text": "@arugula fix the login page" })),
    )
    .await;
    assert_eq!(st, 200, "{m}");
    let parent = m["seq"].as_u64().unwrap();
    let spec = crate::hosted::Spec::default();
    let crate::db::hosted::Added::Made(ws) =
        app.db.add_hosted_workspace("hw_dev", &project, "dev", &spec, "a", 10, 1).unwrap()
    else {
        panic!("no workspace");
    };
    app.db.c().execute("UPDATE hosted_workspaces SET machine = ?2 WHERE id = ?1", params![ws.id, machine]).unwrap();
    let new = NewSession {
        project: &project,
        room: &room,
        parent_seq: parent,
        asker: "a",
        workspace: &ws.id,
        repo: Some("o/r"),
        slug: "fix-login",
        prompt: NewPrompt { author: "a", source: Source::Member, association: None, text: "fix the login page" },
    };
    let (s, _) = app.db.create_session(&new, now_ms()).unwrap().unwrap();
    let seq = post_card(app, &s).unwrap();
    (s, room, seq)
}

/// How many times the card's room was told the session changed.
fn told(db: &Db, ses: &str) -> usize {
    let events = db.chat_events_after(0, 10_000).unwrap();
    events.iter().filter(|e| e.kind == "live" && e.body.contains(r#""type":"session""#) && e.body.contains(ses)).count()
}

struct HostedBox {
    keys: DeviceKeys,
    id: String,
}

impl HostedBox {
    /// Report on `s` as its box, through [`record`].
    fn say(&self, app: &Platform, s: &str, kind: Kind, to: Option<State>, detail: Value) -> Session {
        let now = app.db.session(s).unwrap().unwrap();
        let from = now.seen_as();
        let detail = detail.to_string();
        let ev = arugula_e2e::session::Event {
            session: s,
            n: now.daemon_n + 1,
            kind,
            from,
            to: to.unwrap_or(from),
            at: now_ms(),
            detail: &detail,
            daemon: &self.id,
        };
        let key = hex::encode(self.keys.sign_public());
        let r = DaemonReport::verify(&ev, &key, &ev.sign(&self.keys)).unwrap();
        record(app, s, &Move::Daemon { report: &r, now: now_ms() }).unwrap().unwrap().0
    }
}

#[tokio::test]
async fn a_sessions_card_lives_in_its_thread_and_its_people_answer_it() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let keys = DeviceKeys::generate();
    let b0x = HostedBox { id: keys.id(), keys };
    let (s, room, card) = seed(&app, &hub, &base, &b0x.id).await;
    let path = format!("/api/sessions/{}", s.id);

    // One message, the platform's, in the thread and in the room's list.
    let (_, v) = call(&base, "b", "GET", &format!("/api/chat/rooms/{room}/messages"), None).await;
    let m = v["messages"].as_array().unwrap().iter().find(|m| m["seq"] == json!(card)).cloned().unwrap();
    assert_eq!(
        (&m["author"], &m["via"], &m["parent"]),
        (&json!(""), &json!(format!("session:{}", s.id)), &json!(s.parent_seq))
    );
    assert_eq!(m["also"], json!(true), "{m}");

    // Members read it; who isn't one gets nothing.
    let (st, v) = call(&base, "v", "GET", &path, None).await;
    assert_eq!((st, &v["card"]["state"], &v["may"]), (200, &json!("queued"), &json!(false)), "{v}");
    assert_eq!(call(&base, "c", "GET", &path, None).await.0, 404);
    assert_eq!(call(&base, "b", "GET", &path, None).await.1["may"], json!(true));

    // The box takes it, works and asks: each move is told.
    let before = told(&app.db, &s.id);
    let prompt = app.db.session_prompts(&s.id).unwrap()[0].id.clone();
    b0x.say(&app, &s.id, Kind::State, Some(State::Working), json!({ "prompts": [prompt], "block": "4" }));
    b0x.say(&app, &s.id, Kind::Heartbeat, None, json!({ "tool": "Bash" }));
    b0x.say(&app, &s.id, Kind::Heartbeat, None, json!({ "tool": "Bash" }));
    assert_eq!(told(&app.db, &s.id), before + 2, "a heartbeat that changes nothing isn't told");
    let asks = json!({ "question": "Which page?", "request": "req_1", "ask": "question" });
    b0x.say(&app, &s.id, Kind::State, Some(State::NeedsInput), asks);
    let (_, v) = call(&base, "b", "GET", &path, None).await;
    let c = &v["card"];
    assert_eq!(
        (&c["state"], &c["open"]["question"], &c["open"]["request"]),
        (&json!("needs_input"), &json!("Which page?"), &json!("req_1"))
    );
    assert_eq!((&c["machine"], &c["block"], &c["repo"]), (&json!(b0x.id), &json!("4"), &json!("o/r")), "{c}");

    // A viewer can't answer; an editor can, once, and it's told.
    let answer = json!({ "kind": "answer", "request": "req_1", "answer": { "question_0": "the login page" } });
    assert_eq!(call(&base, "v", "POST", &format!("{path}/actions"), Some(answer.clone())).await.0, 403);
    let before = told(&app.db, &s.id);
    let (st, v) = call(&base, "b", "POST", &format!("{path}/actions"), Some(answer.clone())).await;
    assert_eq!((st, &v["card"]["answered"]["by"]), (200, &json!("b")), "{v}");
    assert_eq!(told(&app.db, &s.id), before + 1);
    let (st, v) = call(&base, "a", "POST", &format!("{path}/actions"), Some(answer)).await;
    assert_eq!((st, v["error"].as_str().unwrap().contains("answered it already")), (409, true), "{v}");
    let work = app.db.session_work("hw_dev").unwrap();
    let action = &work[0].actions[0];
    assert_eq!((action.kind.as_str(), action.by.as_str()), ("answer", "b"));

    // The box takes the answer and finishes the turn.
    b0x.say(&app, &s.id, Kind::State, Some(State::Working), json!({ "actions": [action.id] }));
    let turn =
        json!({ "branch": s.branch, "pushed": true, "commits": ["abc fix login"], "diffstat": "1 file changed" });
    b0x.say(&app, &s.id, Kind::Turn, None, turn);
    b0x.say(&app, &s.id, Kind::State, Some(State::Done), json!({}));
    let (_, v) = call(&base, "a", "GET", &path, None).await;
    let c = &v["card"];
    assert_eq!(
        (&c["state"], &c["answered"], &c["turn"]["diffstat"]),
        (&json!("done"), &Value::Null, &json!("1 file changed")),
        "{c}"
    );
    assert!(c["ended_at"].is_u64(), "{c}");
    let lines: Vec<&str> = v["events"].as_array().unwrap().iter().map(|e| e["text"].as_str().unwrap()).collect();
    assert_eq!(
        lines,
        [
            "Asked for",
            "Its box took it",
            "Asks: Which page?",
            "Answered: working again",
            &format!("Turn pushed {}: 1 commit", s.branch),
            "The turn finished"
        ]
    );

    // Cancel needs a turn under way; Stop is the owners' and editors'.
    let (st, _) = call(&base, "b", "POST", &format!("{path}/actions"), Some(json!({ "kind": "cancel" }))).await;
    assert_eq!(st, 409, "nothing to cancel when done");
    assert_eq!(call(&base, "v", "POST", &format!("{path}/stop"), None).await.0, 403);
    let before = told(&app.db, &s.id);
    let (st, v) = call(&base, "b", "POST", &format!("{path}/stop"), None).await;
    assert_eq!((st, &v["card"]["state"]), (200, &json!("stopped")), "{v}");
    assert_eq!(v["events"].as_array().unwrap().last().unwrap()["text"], json!("Stopped by b"));
    assert_eq!(told(&app.db, &s.id), before + 1);
    assert!(app.db.session_work("hw_dev").unwrap().is_empty(), "a stopped session leaves the box's work");
    assert_eq!(call(&base, "b", "POST", &format!("{path}/stop"), None).await.0, 409);
}

#[tokio::test]
async fn a_quiet_boxs_session_stops_responding_on_the_tick() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let keys = DeviceKeys::generate();
    let b0x = HostedBox { id: keys.id(), keys };
    let (s, _, _) = seed(&app, &hub, &base, &b0x.id).await;
    let prompt = app.db.session_prompts(&s.id).unwrap()[0].id.clone();
    b0x.say(&app, &s.id, Kind::State, Some(State::Working), json!({ "prompts": [prompt], "block": "4" }));
    let path = format!("/api/sessions/{}", s.id);
    // A cancel while it works waits for the box.
    let (st, v) = call(&base, "a", "POST", &format!("{path}/actions"), Some(json!({ "kind": "cancel" }))).await;
    assert_eq!((st, &v["card"]["cancelling"]), (200, &json!(true)), "{v}");
    tick(&app, now_ms()).unwrap();
    assert_eq!(app.db.session(&s.id).unwrap().unwrap().state, State::Working, "not quiet yet");
    let before = told(&app.db, &s.id);
    tick(&app, now_ms() + crate::db::sessions::QUIET_MS + 1).unwrap();
    assert_eq!(told(&app.db, &s.id), before + 1);
    let (_, v) = call(&base, "a", "GET", &path, None).await;
    assert_eq!((&v["card"]["state"], &v["card"]["was"]), (&json!("unresponsive"), &json!("working")), "{v}");
    // Heard from again: back to working, told.
    b0x.say(&app, &s.id, Kind::Heartbeat, None, json!({}));
    assert_eq!(call(&base, "a", "GET", &path, None).await.1["card"]["state"], json!("working"));
}
