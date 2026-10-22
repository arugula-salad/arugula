//! Sending drafts as their person (M168) through the platform's routes and
//! `PlatformPort::send_draft`, in M165's world (`viewer_tests::world`):
//! team t1's project links `acme/app` (77, private: alice, GitHub 1001, and
//! bob, 1002, have access) and `acme/site` (88, public). The fake GitHub
//! (`links_tests`) knows user tokens and takes posts; the test hub plays
//! the GitHub authorization and the machines' `forge.send` answers. The
//! hub's half (the callback, the state, revoking) is control's
//! `forge_send` tests; the machine's is the daemon's `forge_send_op`.

use std::sync::{Arc, Mutex};

use arugula_control_wire::forge::{DaemonOp, OpAnswer};
use serde_json::{Value, json};

use super::{
    links_tests::{Shared, serve},
    send::UserToken,
    viewer_tests::world,
};
use crate::{
    Platform, PlatformPort as _,
    chat_tests::{TestHub, call},
    db::{Sql as _, sql::params},
};

const TOKEN: &str = "ghu_alices_one_use_token";

/// What the platform logged while a test ran, to look for a token in.
#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Logs {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Logs {
    /// Everything logged from now on this thread, at every level.
    fn capture() -> (Self, tracing::subscriber::DefaultGuard) {
        let logs = Logs::default();
        let w = logs.clone();
        let sub = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || w.clone())
            .with_ansi(false)
            .finish();
        (logs, tracing::subscriber::set_default(sub))
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

/// A draft made by `who` (a person's `call`, or a machine's).
async fn draft(base: &str, who: &str, body: Value) -> String {
    let (s, v) = if let Some(m) = who.strip_prefix("machine:") {
        let r = reqwest::Client::new()
            .post(format!("{base}/api/forge/drafts"))
            .header("x-test-daemon", m)
            .json(&body)
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json::<Value>().await.unwrap())
    } else {
        call(base, who, "POST", "/api/forge/drafts", Some(body)).await
    };
    assert_eq!(s, 200, "{v}");
    v["draft"]["id"].as_str().unwrap().to_owned()
}

async fn get(base: &str, who: &str, id: &str) -> Value {
    call(base, who, "GET", &format!("/api/forge/drafts/{id}"), None).await.1
}

async fn send(base: &str, who: &str, id: &str, body: Value) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/forge/drafts/{id}/send"), Some(body)).await
}

/// Every text in the drafts and the token log.
fn rows(app: &Platform) -> String {
    let c = app.db.c();
    let mints: Vec<String> = c
        .query_map(
            "SELECT id || kind || coalesce(permissions, '') || caller || purpose FROM github_token_mints",
            params![],
            |r| r.get(0),
        )
        .unwrap();
    let drafts: Vec<String> = c
        .query_map(
            "SELECT id || body || state || coalesce(sent_url, '') || coalesce(error, '') FROM drafts",
            params![],
            |r| r.get(0),
        )
        .unwrap();
    format!("{mints:?} {drafts:?}")
}

fn user_mints(app: &Platform) -> Vec<(String, Option<u64>, String)> {
    app.db
        .c()
        .query_map(
            "SELECT caller, repo_id, purpose FROM github_token_mints WHERE kind = 'user' ORDER BY at",
            params![],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}

fn posted(gh: &Shared) -> Vec<(String, String, Value)> {
    gh.lock().unwrap().posted.clone()
}

/// Authorize → the hub's code swap → the user token here: it's checked
/// (`GET /user` is alice's GitHub id), its mint logged, the one call made
/// as alice, and the draft sent with GitHub's URL. The token is in no row
/// and no log line.
#[tokio::test]
async fn a_person_sends_as_themselves_with_a_one_use_token() {
    let (logs, _guard) = Logs::capture();
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    world(&hub, &gh, &app).await;
    hub.authorizes(true);
    gh.lock().unwrap().users.insert(TOKEN.into(), 1001);

    let id =
        draft(&base, "alice", json!({ "repo": "acme/app", "number": 1, "kind": "comment", "body": "ship it" })).await;
    // The page asks to send it in the browser: GitHub's page, and nothing
    // sent or held yet.
    let (s, v) = send(&base, "alice", &id, json!({ "via": "browser", "back": "/p/x" })).await;
    assert_eq!(s, 200, "{v}");
    assert!(v["authorize"].as_str().unwrap().contains(&format!("send.alice.{id}&back=/p/x")), "{v}");
    assert_eq!(v["draft"]["state"], "waiting");
    // A path elsewhere isn't a way back.
    let (_, v) = send(&base, "alice", &id, json!({ "via": "browser", "back": "//evil.example" })).await;
    assert!(v["authorize"].as_str().unwrap().ends_with("&back=/"), "{v}");

    // GitHub came back to the hub, which hands the token over.
    let o = app.send_draft("alice", &id, Some(UserToken::new(TOKEN.into()))).await.unwrap();
    let sent = o.sent.expect("sent");
    assert_eq!((sent.via.as_deref(), sent.draft.state.as_str()), (Some("token"), "sent"));
    let url = "https://github.com/repos/acme/app/issues/1/comments#posted";
    assert_eq!(sent.draft.sent_url.as_deref(), Some(url));
    assert!(sent.draft.sent_at.is_some());
    assert_eq!(
        posted(&gh),
        [("/repos/acme/app/issues/1/comments".into(), format!("Bearer {TOKEN}"), json!({ "body": "ship it" }))]
    );
    assert_eq!(user_mints(&app), [("alice".into(), Some(77), "send:comment".into())]);
    let one = get(&base, "alice", &id).await;
    assert_eq!((one["state"].as_str(), one["sent_url"].as_str()), (Some("sent"), Some(url)));

    // Sent is sent: again is refused, and nothing goes to GitHub.
    let o = app.send_draft("alice", &id, Some(UserToken::new(TOKEN.into()))).await.unwrap();
    assert!(o.refused.unwrap().contains("sent already"));
    assert_eq!(posted(&gh).len(), 1);

    // A review and a new issue go to their own endpoints.
    let review = draft(
        &base,
        "alice",
        json!({ "repo": "acme/app", "number": 1, "kind": "review", "event": "APPROVE", "body": "" }),
    )
    .await;
    let issue =
        draft(&base, "alice", json!({ "repo": "acme/app", "kind": "issue", "title": "Bug", "body": "b" })).await;
    for id in [&review, &issue] {
        let o = app.send_draft("alice", id, Some(UserToken::new(TOKEN.into()))).await.unwrap();
        assert!(o.sent.is_some(), "{o:?}");
    }
    let p = posted(&gh);
    assert_eq!(p[1].0, "/repos/acme/app/pulls/1/reviews");
    assert_eq!(p[1].2, json!({ "event": "APPROVE" }));
    assert_eq!(p[2].0, "/repos/acme/app/issues");
    assert_eq!(p[2].2, json!({ "title": "Bug", "body": "b" }));
    assert_eq!(
        user_mints(&app).iter().map(|m| m.2.as_str()).collect::<Vec<_>>(),
        ["send:comment", "send:comment", "send:review", "send:issue"]
    );

    // Never in a row, never in a log line.
    assert!(!rows(&app).contains(TOKEN));
    let logged = logs.text();
    assert!(logged.contains("a draft sent as its person"), "the capture works");
    assert!(!logged.contains(TOKEN), "{logged}");
    assert!(!format!("{:?}", UserToken::new(TOKEN.into())).contains(TOKEN));
}

/// A token whose `GET /user` is another GitHub account than the one alice
/// signed in with is refused before any call; the draft waits, saying so.
#[tokio::test]
async fn a_token_for_another_github_account_is_refused() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    world(&hub, &gh, &app).await;
    gh.lock().unwrap().users.insert("ghu_bobs".into(), 1002);

    let id = draft(&base, "alice", json!({ "repo": "acme/app", "number": 1, "kind": "comment", "body": "x" })).await;
    let o = app.send_draft("alice", &id, Some(UserToken::new("ghu_bobs".into()))).await.unwrap();
    assert!(o.refused.as_deref().unwrap().contains("isn't alice-gh"), "{o:?}");
    // A token GitHub doesn't know is refused too.
    let o = app.send_draft("alice", &id, Some(UserToken::new("ghu_nobody".into()))).await.unwrap();
    assert!(o.refused.is_some(), "{o:?}");
    assert!(posted(&gh).is_empty());
    let d = get(&base, "alice", &id).await;
    assert_eq!(d["state"], "waiting", "{d}");
    assert!(d["error"].as_str().is_some(), "{d}");
}

/// GitHub refusing the call leaves the draft failed, to send again; a
/// token that may not write there (403, the App without write) goes to a
/// machine instead.
#[tokio::test]
async fn github_refusing_it_fails_and_a_token_without_write_goes_to_a_machine() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    world(&hub, &gh, &app).await;
    gh.lock().unwrap().users.insert(TOKEN.into(), 1001);
    hub.daemon("geek", "alice", None);

    let id = draft(&base, "alice", json!({ "repo": "acme/app", "number": 1, "kind": "comment", "body": "x" })).await;
    gh.lock().unwrap().refuse = Some(422);
    let o = app.send_draft("alice", &id, Some(UserToken::new(TOKEN.into()))).await.unwrap();
    assert!(o.refused.as_deref().unwrap().contains("422"), "{o:?}");
    let d = get(&base, "alice", &id).await;
    assert_eq!((d["state"].as_str(), d["sent_at"].as_str()), (Some("failed"), None), "{d}");

    gh.lock().unwrap().refuse = Some(403);
    hub.answers_ops("geek", OpAnswer::Done { url: Some("https://github.com/acme/app/pull/1#c".into()) });
    let o = app.send_draft("alice", &id, Some(UserToken::new(TOKEN.into()))).await.unwrap();
    let sent = o.sent.expect("sent by the machine");
    assert_eq!((sent.via.as_deref(), sent.machine.as_deref()), (Some("machine"), Some("geek-box")));
    assert_eq!(sent.draft.state, "sent");
    assert_eq!(hub.ops_asked().len(), 1);
}

/// A machine can't send, grant or not: not a draft, not anyone's, and no
/// machine is asked. An agent's draft is sent by its person, through a
/// machine of theirs, as them; only its own person sends it.
#[tokio::test]
async fn agents_draft_people_send() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    world(&hub, &gh, &app).await;
    hub.daemon("geek", "alice", None);
    hub.daemon("aaa", "alice", None);
    hub.daemon("bobs", "bob", None);
    hub.answers_ops("aaa", OpAnswer::Unknown);
    hub.answers_ops("geek", OpAnswer::Done { url: Some("https://github.com/acme/app/pull/1#c".into()) });

    let id = draft(
        &base,
        "machine:alice geek",
        json!({ "repo": "acme/app", "number": 1, "kind": "comment", "body": "lgtm", "agent": "mcp:claude-code" }),
    )
    .await;

    // The machine that drafted it can't send it.
    let r = reqwest::Client::new()
        .post(format!("{base}/api/forge/drafts/{id}/send"))
        .header("x-test-daemon", "alice geek")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["reason"], "agents_draft", "{v}");
    assert!(hub.ops_asked().is_empty());

    // Another person who may read it can't send it as themselves; one who
    // may not read it doesn't see it.
    let (s, v) = send(&base, "bob", &id, json!({})).await;
    assert_eq!(s, 403, "{v}");
    let (s, _) = send(&base, "eve", &id, json!({})).await;
    assert_eq!(s, 404);

    // Its person sends it: the machine that drafted it is asked first.
    let (s, v) = send(&base, "alice", &id, json!({})).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!((v["via"].as_str(), v["machine"].as_str()), (Some("machine"), Some("geek-box")));
    assert_eq!(v["draft"]["state"], "sent");
    let asked = hub.ops_asked();
    assert_eq!(asked.len(), 1);
    let DaemonOp::ForgeSend(op) = &asked[0].1 else { panic!("{asked:?}") };
    assert_eq!(asked[0].0, "geek");
    assert_eq!(
        (op.account.as_str(), op.repo.as_str(), op.number, op.kind.as_str(), op.body.as_str()),
        ("alice", "acme/app", Some(1), "comment", "lgtm")
    );

    // A person's own, with no machine that drafted it: one too old to know
    // the op is passed over for the next; bob's machine is never asked.
    let mine = draft(&base, "alice", json!({ "repo": "acme/site", "number": 2, "kind": "comment", "body": "x" })).await;
    let (s, v) = send(&base, "alice", &mine, json!({ "via": "machine" })).await;
    assert_eq!(s, 200, "{v}");
    let asked: Vec<String> = hub.ops_asked().into_iter().map(|(d, _)| d).collect();
    assert_eq!(asked, ["geek", "aaa", "geek"]);
}

/// With no browser hop (no App client at the hub) and no machine awake,
/// the send fails clearly and the draft waits, saying so; a machine that
/// wakes sends it after. One too old to send says so in the refusal.
#[tokio::test]
async fn with_no_way_it_fails_clearly_and_keeps_the_draft() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    world(&hub, &gh, &app).await;
    hub.daemon("old", "alice", None);
    hub.answers_ops("old", OpAnswer::Unknown);

    let id = draft(&base, "alice", json!({ "repo": "acme/app", "number": 1, "kind": "comment", "body": "x" })).await;
    let (s, v) = send(&base, "alice", &id, json!({ "via": "browser", "back": "/p/x" })).await;
    assert_eq!((s, v["reason"].as_str()), (409, Some("no_way")), "{v}");
    let said = v["error"].as_str().unwrap();
    assert!(said.starts_with("Open this on a device to send, or wake a machine."), "{said}");
    assert!(said.contains("old-box: its Arugula is too old"), "{said}");
    let d = get(&base, "alice", &id).await;
    assert_eq!(
        (d["state"].as_str(), d["error"].as_str()),
        (Some("waiting"), Some("Open this on a device to send, or wake a machine.")),
        "{d}"
    );
    // The hub's callback hears the same when the authorization failed.
    let o = app.send_draft("alice", &id, None).await.unwrap();
    assert!(o.refused.unwrap().starts_with("Open this on a device"));

    // A machine wakes: it goes.
    hub.daemon("geek", "alice", None);
    hub.answers_ops("geek", OpAnswer::Done { url: None });
    let (s, v) = send(&base, "alice", &id, json!({})).await;
    assert_eq!((s, v["draft"]["state"].as_str()), (200, Some("sent")), "{v}");

    // GitHub refusing it through a machine leaves it failed, to send again.
    let again = draft(&base, "alice", json!({ "repo": "acme/app", "number": 1, "kind": "comment", "body": "y" })).await;
    hub.answers_ops("geek", OpAnswer::Failed { error: "GitHub said 422".into() });
    let (s, v) = send(&base, "alice", &again, json!({})).await;
    assert_eq!((s, v["reason"].as_str()), (502, Some("github_refused")), "{v}");
    assert_eq!(get(&base, "alice", &again).await["state"], "failed");
}

/// A send holds its draft: a second at once is refused, and it can't be
/// discarded meanwhile. A block's card sends through its machine and says
/// so (`start`, then `sent`); only the draft's own person's machine.
#[tokio::test]
async fn one_send_at_a_time_and_a_blocks_card() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    world(&hub, &gh, &app).await;
    hub.daemon("geek", "alice", None);
    hub.daemon("bobs", "bob", None);
    let id = draft(&base, "alice", json!({ "repo": "acme/app", "number": 1, "kind": "comment", "body": "x" })).await;

    let machine = |who: &'static str, step: Value| {
        let (base, id) = (base.clone(), id.clone());
        async move {
            let r = reqwest::Client::new()
                .post(format!("{base}/api/forge/drafts/{id}/machine"))
                .header("x-test-daemon", who)
                .json(&step)
                .send()
                .await
                .unwrap();
            (r.status().as_u16(), r.json::<Value>().await.unwrap_or_default())
        }
    };
    // Bob's machine can't send alice's.
    assert_eq!(machine("bob bobs", json!({ "step": "start" })).await.0, 403);
    let (s, v) = machine("alice geek", json!({ "step": "start" })).await;
    assert_eq!((s, v["state"].as_str()), (200, Some("sending")), "{v}");
    // Held: no second send, no discard.
    let (s, v) = send(&base, "alice", &id, json!({})).await;
    assert_eq!(s, 409, "{v}");
    assert!(v["error"].as_str().unwrap().contains("being sent"), "{v}");
    let (s, v) = call(&base, "alice", "POST", &format!("/api/forge/drafts/{id}/discard"), None).await;
    assert_eq!((s, v["error"].as_str()), (409, Some("it's being sent")));
    assert_eq!(machine("alice geek", json!({ "step": "start" })).await.0, 409);
    let (s, v) = machine("alice geek", json!({ "step": "sent", "url": "https://github.com/acme/app/pull/1#c" })).await;
    assert_eq!(
        (s, v["state"].as_str(), v["sent_url"].as_str()),
        (200, Some("sent"), Some("https://github.com/acme/app/pull/1#c"))
    );
    assert!(hub.ops_asked().is_empty(), "the platform asked no machine");
    assert_eq!(machine("alice geek", json!({ "step": "sent" })).await.0, 409, "no send holds it now");
    assert!(posted(&gh).is_empty());
}

/// The typed ops (M169) send the same way, as the CLI does: a draft's
/// through a machine, and a comment that isn't a draft yet is made one
/// first, so with no machine awake it waits and the refusal names it.
#[tokio::test]
async fn the_cli_sends_through_the_typed_ops() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    world(&hub, &gh, &app).await;
    let op = |name: &'static str, args: Value| {
        let base = base.clone();
        async move {
            let mut body = json!({ "op": name, "v": 1 });
            body.as_object_mut().unwrap().extend(args.as_object().unwrap().clone());
            call(&base, "alice", "POST", "/api/forge/ops", Some(body)).await
        }
    };
    let (s, v) = op("pr.comment", json!({ "repo": "acme/app", "number": 1, "body": "from the terminal" })).await;
    assert_eq!((s, v["reason"].as_str()), (409, Some("no_way")), "{v}");
    let said = v["error"].as_str().unwrap();
    let id = said.split("draft ").nth(1).unwrap().split(':').next().unwrap().to_owned();
    assert_eq!(get(&base, "alice", &id).await["state"], "waiting");

    hub.daemon("geek", "alice", None);
    hub.answers_ops("geek", OpAnswer::Done { url: Some("https://github.com/acme/app/pull/1#c".into()) });
    let (s, v) = op("draft.send", json!({ "draft": id })).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["result"], json!({ "url": "https://github.com/acme/app/pull/1#c", "draft": id }));
    let (s, v) = op("issue.create", json!({ "repo": "acme/app", "title": "Bug", "body": "b" })).await;
    assert_eq!((s, v["result"]["url"].as_str()), (200, Some("https://github.com/acme/app/pull/1#c")), "{v}");
    let kinds: Vec<String> = hub
        .ops_asked()
        .into_iter()
        .filter_map(|(_, op)| match op {
            DaemonOp::ForgeSend(s) => Some(format!("{} {}", s.kind, s.body)),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, ["comment from the terminal", "issue b"]);
}
