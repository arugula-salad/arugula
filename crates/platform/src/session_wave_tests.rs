//! A wave of agent sessions (Fleet 5), through the platform's routes: the
//! list its members read, narrowed by label, tracker and asker, with what
//! waits on each reader; and for a machine's agents, with a grant to follow
//! the project, the same list, a session's last answer whole, and a wait
//! that answers once a turn finishes.

use std::time::{Duration, Instant};

use arugula_control_wire::{
    sessions::{AGENT_SESSIONS_WAIT, agent_project_sessions, agent_session},
    work::SessionReport,
};
use arugula_e2e::DeviceKeys;
use serde_json::{Value, json};

use crate::{
    Platform,
    agents_tests::give,
    chat_tests::{TestHub, call},
    db::{
        issues::{Actor, NewIssue},
        projects::{SESSIONS_START, THREADS_READ},
        sessions::{Kind, State},
    },
    grants::AGENT_UNGRANTED,
    sessions::answer::{ANSWER_MAX, THE_START},
    sessions_hydrate_tests::{platform, take},
    sessions_pool_tests::report,
    work::HostedMachine,
};

/// A call signed by a's machine geek ("geek-box").
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

async fn grant(base: &str, t: &str, power: &str) {
    let (st, g) =
        call(base, "a", "POST", "/api/grants", give("geek", power, json!({ "kind": "project", "id": t }))).await;
    assert_eq!(st, 200, "{g}");
}

async fn joined(hub: &TestHub, app: &Platform) {
    hub.daemon("geek", "a", Some("t"));
    crate::projects::follow(app).await.unwrap();
    crate::cache::catch_up(app).await.unwrap();
}

/// An issue in `t`, by b: its id and number.
fn issue(app: &Platform, t: &str, title: &str, body: &str, labels: &[&str]) -> (String, u64) {
    let labels: Vec<String> = labels.iter().map(|l| (*l).to_owned()).collect();
    let i = app
        .db
        .create_issue(&NewIssue { project: t, title, body, labels: &labels, assignees: &[] }, Actor::person("b"), 1)
        .unwrap()
        .unwrap();
    (i.id, i.number)
}

/// `who`'s Agent on this on issue `id`: the session.
async fn agent_on(base: &str, who: &str, id: &str) -> String {
    let (st, v) = call(base, who, "POST", &format!("/api/issues/{id}/agent"), None).await;
    assert_eq!(st, 200, "{v}");
    v["session"].as_str().expect("a session").to_owned()
}

/// The box's turn report on `s` (`kind` turn, the state it's in).
async fn turn(app: &Platform, keys: &DeviceKeys, s: &str, detail: Value) {
    let now = app.db.session(s).unwrap().unwrap();
    let detail = detail.to_string();
    let id = keys.id();
    let at = arugula_e2e::now_ms();
    let ev = arugula_e2e::session::Event {
        session: s,
        n: now.daemon_n + 1,
        kind: Kind::Turn,
        from: now.seen_as(),
        to: now.seen_as(),
        at,
        detail: &detail,
        daemon: &id,
    };
    let r = SessionReport {
        session: s.into(),
        n: ev.n,
        kind: "turn".into(),
        from: ev.from.as_str().into(),
        to: ev.to.as_str().into(),
        at,
        sig: ev.sign(keys),
        detail,
    };
    let m = HostedMachine {
        project: now.project.clone(),
        workspace: now.workspace.clone(),
        machine: id,
        sign: hex::encode(keys.sign_public()),
    };
    crate::session_live::report(app, &m, &r).await.unwrap();
}

fn ids(v: &Value) -> Vec<&str> {
    v["sessions"].as_array().unwrap().iter().map(|r| r["session"].as_str().unwrap()).collect()
}

/// The wave: newest first, narrowed by its issue's label, by a tracker
/// (what its body names, what names it, and itself) and by who asked; a
/// session's question waits on an editor, not a viewer; a pushed one's PR
/// waits on its asker. Who isn't in the project gets nothing.
#[tokio::test]
async fn a_projects_wave_narrows_by_label_tracker_and_asker() {
    let (_hub, app, base, t, _u, keys) = platform(&["https://github.com/o/app"]).await;
    let (one, n1) = issue(&app, &t, "Fleet 4", "", &["fleet"]);
    // The tracker names one; two names the tracker.
    let (_, tn) = issue(&app, &t, "Fleet track", &format!("Waves: #{n1}, and more."), &["track"]);
    let (two, _) = issue(&app, &t, "Fleet 5", &format!("Part of #{tn}."), &[]);
    let (other, _) = issue(&app, &t, "Unrelated", "", &["docs"]);
    let s1 = agent_on(&base, "b", &one).await;
    let s2 = agent_on(&base, "a", &two).await;
    let s3 = agent_on(&base, "b", &other).await;

    let wave = |q: &str| {
        let base = base.clone();
        let path = format!("/api/projects/{t}/sessions{q}");
        async move { call(&base, "b", "GET", &path, None).await }
    };
    let (st, v) = wave("").await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(ids(&v), [s3.as_str(), &s2, &s1], "newest first");
    let r = &v["sessions"][2];
    assert_eq!(
        (&r["issue"]["number"], &r["workspace_name"], &r["state"]),
        (&json!(n1), &json!("dev"), &json!("queued"))
    );
    assert_eq!((&r["settled"], &r["pushed_at"], &r["asker"]), (&json!(false), &Value::Null, &json!("b")), "{r}");
    assert!(r["branch"].as_str().unwrap().contains("fleet-4"), "{r}");
    assert_eq!(v["labels"], json!(["docs", "fleet", "track"]));

    assert_eq!(ids(&wave("?label=fleet").await.1), [s1.as_str()]);
    let (_, v) = wave(&format!("?tracker={tn}")).await;
    assert_eq!(ids(&v), [s2.as_str(), &s1], "named by it, or naming it");
    assert_eq!(v["tracker"]["title"], "Fleet track");
    assert_eq!(ids(&wave("?asker=me").await.1), [s3.as_str(), &s1], "b's own");
    assert_eq!(ids(&wave("?asker=a").await.1), [s2.as_str()]);
    assert_eq!(wave("?tracker=999").await.0, 404);

    // A question waits on an editor (b), not a viewer (x).
    take(&app, &keys, &s1);
    let q = json!({ "ask": "question", "request": "q1", "question": "Which colour?", "options": ["Red"] });
    report(&app, &keys, &s1, State::NeedsInput, q).await;
    let row =
        |v: &Value, s: &str| v["sessions"].as_array().unwrap().iter().find(|r| r["session"] == s).unwrap().clone();
    let r = row(&wave("").await.1, &s1);
    assert_eq!(
        (&r["waiting"], &r["question"], &r["settled"]),
        (&json!("its question"), &json!("Which colour?"), &json!(true))
    );
    let (_, xv) = call(&base, "x", "GET", &format!("/api/projects/{t}/sessions"), None).await;
    assert_eq!(row(&xv, &s1)["waiting"], Value::Null, "a viewer answers nothing");
    // Pushed and done: its PR waits on its asker alone.
    report(&app, &keys, &s1, State::Working, json!({})).await;
    let s = app.db.session(&s1).unwrap().unwrap();
    turn(&app, &keys, &s1, json!({ "branch": s.branch, "pushed": true, "head": "abc1234", "commits": ["abc1234 x"] }))
        .await;
    report(&app, &keys, &s1, State::Done, json!({})).await;
    let r = row(&wave("").await.1, &s1);
    assert_eq!(
        (&r["waiting"], &r["head"], &r["state"]),
        (&json!("its PR to send"), &json!("abc1234"), &json!("done")),
        "{r}"
    );
    assert!(r["pushed_at"].is_u64(), "{r}");
    let (_, av) = call(&base, "a", "GET", &format!("/api/projects/{t}/sessions"), None).await;
    assert_eq!(row(&av, &s1)["waiting"], Value::Null, "a didn't ask for it");
    assert_eq!(call(&base, "c", "GET", &format!("/api/projects/{t}/sessions"), None).await.0, 404);
}

/// A machine's agents follow a wave by grant: without one they're refused,
/// saying how; with threads.read (or sessions.start) they list it, read a
/// session's last answer whole (the thread has only its end), and wait:
/// at once for one settled, else until its turn finishes, woken by it.
#[tokio::test]
async fn an_agent_follows_a_wave_reads_whole_answers_and_waits_for_done() {
    let (hub, app, base, t, _u, keys) = platform(&["https://github.com/o/app"]).await;
    let keys = std::sync::Arc::new(keys);
    joined(&hub, &app).await;
    let (one, _) = issue(&app, &t, "Fleet 5", "", &["fleet"]);
    let (two, _) = issue(&app, &t, "Fleet 6", "", &["fleet"]);
    let s1 = agent_on(&base, "b", &one).await;
    let s2 = agent_on(&base, "b", &two).await;

    let list = agent_project_sessions(&t) + "?label=fleet";
    let (st, v) = geek(&base, "GET", &list, None).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    assert!(v["error"].as_str().unwrap().contains("sessions.start"), "{v}");
    assert_eq!(geek(&base, "GET", &agent_session(&s1), None).await.0, 403);

    grant(&base, &t, THREADS_READ).await;
    let (st, v) = geek(&base, "GET", &list, None).await;
    assert_eq!((st, ids(&v)), (200, vec![s2.as_str(), &s1]), "{v}");
    assert_eq!(
        geek(&base, "GET", &format!("{}?agent=me", agent_project_sessions(&t)), None).await.1["sessions"],
        json!([])
    );

    // A long answer: the thread keeps its end, the read has it whole.
    take(&app, &keys, &s1);
    let long = format!("Plan first.\n{}\nCaveat: one spec skipped.", "word ".repeat(ANSWER_MAX));
    let s = app.db.session(&s1).unwrap().unwrap();
    turn(&app, &keys, &s1, json!({ "branch": s.branch, "pushed": true, "head": "abc1234", "reply": long })).await;
    let (_, ms) = call(&base, "b", "GET", &format!("/api/chat/rooms/{}/messages", s.room), None).await;
    let said = ms["messages"].as_array().unwrap().iter().find(|m| m["via"] == format!("said:{s1}")).unwrap();
    assert!(said["text"].as_str().unwrap().starts_with(THE_START), "{said}");
    let (st, r) = geek(&base, "GET", &agent_session(&s1), None).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!((&r["answer"]["text"], &r["answer"]["cut"], &r["turns"]), (&json!(long), &json!(false), &json!(1)));
    assert_eq!((&r["session"]["head"], &r["session"]["settled"]), (&json!("abc1234"), &json!(false)), "still working");
    assert_eq!(r["turn"]["pushed"], true);

    // Nothing settled and no wait: at once, as they are.
    let both = json!({ "sessions": [s1, s2], "wait": 0 });
    let (st, v) = geek(&base, "POST", AGENT_SESSIONS_WAIT, Some(both)).await;
    assert_eq!((st, &v["settled"]), (200, &json!([])), "{v}");
    // Held until s1's turn finishes, woken by it rather than the wait's end.
    let app2 = app.clone();
    let keys2 = keys.clone();
    let s1b = s1.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        report(&app2, &keys2, &s1b, State::Done, json!({})).await;
    });
    let started = Instant::now();
    let (st, v) = geek(&base, "POST", AGENT_SESSIONS_WAIT, Some(json!({ "sessions": [s2, s1], "wait": 10 }))).await;
    assert_eq!((st, &v["settled"]), (200, &json!([s1])), "{v}");
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    assert_eq!(v["sessions"][1]["state"], "done", "in the order asked");

    // A prompt queued for its next turn: not settled until that's done.
    app.db
        .queue_prompt(
            &s1,
            &crate::db::sessions::NewPrompt {
                author: "b",
                source: crate::db::sessions::Source::Member,
                association: None,
                text: "and the docs",
            },
            arugula_e2e::now_ms(),
        )
        .unwrap()
        .unwrap();
    let (_, v) = geek(&base, "POST", AGENT_SESSIONS_WAIT, Some(json!({ "sessions": [s1], "wait": 0 }))).await;
    assert_eq!(v["settled"], json!([]), "a prompt waits for its turn: {v}");

    // Another project's session isn't there; sessions.start follows too.
    assert_eq!(geek(&base, "GET", &agent_session("ses_00000000000000000000000000000000"), None).await.0, 404);
    let (st, v) = geek(&base, "POST", AGENT_SESSIONS_WAIT, Some(json!({ "sessions": [] }))).await;
    assert_eq!(st, 400, "{v}");
    grant(&base, &t, SESSIONS_START).await;
    assert_eq!(geek(&base, "GET", &agent_session(&s2), None).await.0, 200);
}
