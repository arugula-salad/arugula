//! Open on GitHub, per issue (M205, aru-130), in the base's world on a fake
//! GitHub (`forge::viewer_tests::world`): team t1's project links
//! `acme/app` (77, private: alice and bob read it on GitHub, carol doesn't)
//! and `acme/site` (88). A native issue is a draft its person sends as
//! themselves; once it's on GitHub its origin and `exported` say so, and
//! comments cross both ways through the mirror, each once and never back.
//! Runs on SQLite and, with `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_control_wire::forge::{DaemonOp, OpAnswer};
use arugula_platform::{PlatformPort as _, forge::send::UserToken};
use serde_json::{Value, json};

use crate::{
    Platform,
    chat_tests::{TestHub, call, hears, open_live},
    db::{Sql as _, sql::params},
    forge::{
        export::{self, AGENT_WAITS},
        links_tests::Shared,
        viewer_tests::world,
    },
    testing::forge_serve as serve,
};

const TOKEN: &str = "ghu_alices_one_use_token";

/// The world, chat on for t1, GitHub answering as itself: (gh, app, base,
/// team).
async fn start() -> (Arc<TestHub>, Shared, Arc<Platform>, String, String) {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    hub.flag("team:t1", true);
    gh.lock().unwrap().github_urls = true;
    (hub, gh, app, base, team)
}

async fn new_issue(base: &str, who: &str, team: &str, title: &str, body: &str) -> String {
    let (s, v) =
        call(base, who, "POST", &format!("/api/projects/{team}/issues"), Some(json!({ "title": title, "body": body })))
            .await;
    assert_eq!(s, 200, "{v}");
    v["id"].as_str().unwrap().to_owned()
}

async fn export_draft(base: &str, who: &str, issue: &str, repo: &str) -> (u16, Value) {
    let b = json!({ "repo": repo, "kind": "issue", "native_issue": issue });
    call(base, who, "POST", "/api/forge/drafts", Some(b)).await
}

async fn issue(base: &str, who: &str, id: &str) -> Value {
    let (s, v) = call(base, who, "GET", &format!("/api/issues/{id}"), None).await;
    assert_eq!(s, 200, "{v}");
    v
}

async fn thread(base: &str, who: &str, issue: &str) -> String {
    let (s, room) =
        call(base, who, "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": issue }))).await;
    assert_eq!(s, 200, "{room}");
    room["id"].as_str().unwrap().to_owned()
}

async fn messages(base: &str, room: &str) -> Vec<Value> {
    let (s, v) = call(base, "alice", "GET", &format!("/api/chat/rooms/{room}/messages"), None).await;
    assert_eq!(s, 200, "{v}");
    v["messages"].as_array().unwrap().clone()
}

async fn post(base: &str, who: &str, room: &str, body: Value) -> Value {
    let (s, v) = call(base, who, "POST", &format!("/api/chat/rooms/{room}/messages"), Some(body)).await;
    assert_eq!(s, 200, "{v}");
    v
}

/// GitHub's `issue_comment` on acme/site's (public) issue `number`.
fn comment(number: u64, id: u64, login: &str, body: &str) -> Value {
    comment_on((88, "acme/site"), number, id, login, body)
}

fn comment_on(repo: (u64, &str), number: u64, id: u64, login: &str, body: &str) -> Value {
    json!({
        "action": "created",
        "repository": { "id": repo.0, "full_name": repo.1 },
        "issue": { "number": number },
        "comment": { "id": id, "body": body, "user": { "login": login, "id": id + 1 } },
    })
}

/// A GitHub comment a test lands by webhook: its id, login and text.
type Landing = (u64, &'static str, String);

/// A person's live stream, once it's said hello.
async fn live(base: &str, who: &str) -> Arc<Mutex<String>> {
    let heard = open_live(base, who, None).await;
    hears(&heard, "\"type\":\"hello\"", Duration::from_secs(5)).await;
    heard
}

/// How the send of message `seq`'s comment ended (Arugula #584): the
/// `github` event on its author's live stream `heard`.
async fn send_ended(heard: &Mutex<String>, room: &str, seq: i64) -> Value {
    let what = format!(r#""type":"github","room":"{room}","seq":{seq},"#);
    hears(heard, &what, Duration::from_secs(10)).await;
    let text = heard.lock().unwrap().clone();
    let line = text.lines().filter_map(|l| l.strip_prefix("data: ")).find(|l| l.contains(&what)).unwrap();
    serde_json::from_str::<Value>(line).unwrap()["github"].clone()
}

/// A post of `text` by `who` whose comment goes to GitHub: the post's
/// answer (`sending`), then how the send ended on `heard`. Its seq too.
async fn post_out(base: &str, who: &str, room: &str, heard: &Mutex<String>, text: &str) -> (i64, Value) {
    let v = post(base, who, room, json!({ "text": text })).await;
    assert_eq!((v["github"]["sending"].as_bool(), v["github"]["sent"].as_bool()), (Some(true), Some(false)), "{v}");
    let seq = v["seq"].as_i64().unwrap();
    let g = send_ended(heard, room, seq).await;
    assert_eq!(g["draft"], v["github"]["draft"], "{g}");
    assert_eq!(g["sending"], false, "{g}");
    (seq, g)
}

/// Alice opens a new issue on acme/site (public) as its #7, through her
/// machine `geek`: the issue and its thread.
async fn exported(hub: &TestHub, base: &str, team: &str, title: &str) -> (String, String) {
    let id = new_issue(base, "alice", team, title, "").await;
    hub.daemon("geek", "alice", None);
    hub.answers_ops("geek", OpAnswer::Done { url: Some("https://github.com/acme/site/issues/7".into()), routed: None });
    let (_, v) = export_draft(base, "alice", &id, "acme/site").await;
    let d = v["draft"]["id"].as_str().unwrap().to_owned();
    let (s, v) = call(base, "alice", "POST", &format!("/api/forge/drafts/{d}/send"), Some(json!({}))).await;
    assert_eq!(s, 200, "{v}");
    let room = thread(base, "bob", &id).await;
    (id, room)
}

/// The thread's messages as (who, text): an author, or `github:<login>`.
async fn said(base: &str, room: &str) -> Vec<(String, String)> {
    messages(base, room)
        .await
        .iter()
        .map(|m| {
            let who = m["via"].as_str().filter(|v| v.starts_with("github:")).or(m["author"].as_str()).unwrap();
            (who.to_owned(), m["text"].as_str().unwrap().to_owned())
        })
        .collect()
}

fn crossed(app: &Platform) -> Vec<(u64, String, Option<i64>)> {
    app.db
        .c()
        .query_map("SELECT github_id, way, seq FROM issue_github_comments ORDER BY github_id", params![], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap()
}

/// An owner or editor drafts it to a linked repo (the project's one, or the
/// one named), once; it goes as its person with a one-use token, with its
/// title and body as they are when it's sent. Then it's on GitHub: its
/// origin and an `exported` event say where, and it isn't sent twice. It
/// stays native: open, numbered, with its thread.
#[tokio::test]
async fn a_person_opens_an_issue_on_github_as_themselves() {
    let (hub, gh, app, base, team) = start().await;
    hub.authorizes(true);
    gh.lock().unwrap().users.insert(TOKEN.into(), 1001);
    let id = new_issue(&base, "alice", &team, "Crash on start", "steps").await;

    // A viewer doesn't; with two linked repos it asks which; a repo of
    // another project isn't one.
    let (s, v) = export_draft(&base, "carol", &id, "acme/app").await;
    assert_eq!(s, 403, "{v}");
    let (s, v) = export_draft(&base, "alice", &id, "").await;
    assert_eq!(s, 400, "{v}");
    assert!(v["error"].as_str().unwrap().contains("acme/app, acme/site"), "{v}");
    let (s, v) = export_draft(&base, "alice", &id, "eve-gh/x").await;
    assert_eq!(s, 404, "{v}");

    let (s, v) = export_draft(&base, "alice", &id, "acme/app").await;
    assert_eq!(s, 200, "{v}");
    let d = v["draft"].clone();
    assert_eq!(
        (d["kind"].as_str(), d["title"].as_str(), d["body"].as_str()),
        (Some("issue"), Some("Crash on start"), Some("steps"))
    );
    assert_eq!((d["native_issue"].as_str(), v["created"].as_bool()), (Some(id.as_str()), Some(true)));
    let draft = d["id"].as_str().unwrap().to_owned();
    // Again is the same draft; another person's is refused while it waits.
    let (_, v) = export_draft(&base, "alice", &id, "acme/app").await;
    assert_eq!((v["draft"]["id"].as_str(), v["created"].as_bool()), (Some(draft.as_str()), Some(false)));
    let (s, v) = export_draft(&base, "bob", &id, "acme/app").await;
    assert_eq!(s, 409, "{v}");

    // Edited before it's sent: what goes is the issue now.
    let (s, _) =
        call(&base, "alice", "PATCH", &format!("/api/issues/{id}"), Some(json!({ "title": "Crash on start-up" })))
            .await;
    assert_eq!(s, 200);
    let (s, v) =
        call(&base, "alice", "POST", &format!("/api/forge/drafts/{draft}/send"), Some(json!({ "via": "browser" })))
            .await;
    assert_eq!(s, 200, "{v}");
    assert!(v["authorize"].as_str().is_some(), "{v}");
    let o = app.send_draft("alice", &draft, Some(UserToken::new(TOKEN.into()))).await.unwrap();
    let sent = o.sent.clone().unwrap_or_else(|| panic!("{o:?}"));
    assert_eq!(sent.draft.sent_url.as_deref(), Some("https://github.com/acme/app/issues/41"));
    let posted = gh.lock().unwrap().posted.clone();
    assert_eq!(posted.len(), 1);
    assert_eq!(posted[0].0, "/repos/acme/app/issues");
    assert_eq!(posted[0].1, format!("Bearer {TOKEN}"));
    assert_eq!(posted[0].2, json!({ "title": "Crash on start-up", "body": "steps" }));

    let i = issue(&base, "bob", &id).await;
    assert_eq!(i["github"], json!({ "repo": "acme/app", "number": 41 }));
    assert_eq!(i["state"], "open", "it stays native, as it was");
    let (_, ev) = call(&base, "bob", "GET", &format!("/api/issues/{id}/events"), None).await;
    let last = ev["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!((last["kind"].as_str(), last["actor"].as_str()), (Some("exported"), Some("alice")));
    assert_eq!(last["detail"], json!({ "repo": "acme/app", "number": 41 }));

    // Once on GitHub: no second draft, and no second send.
    let (s, v) = export_draft(&base, "alice", &id, "acme/site").await;
    assert_eq!(s, 409, "{v}");
    assert!(v["error"].as_str().unwrap().contains("acme/app#41"), "{v}");
    let o = app.send_draft("alice", &draft, Some(UserToken::new(TOKEN.into()))).await.unwrap();
    assert!(o.refused.is_some());
    assert_eq!(gh.lock().unwrap().posted.len(), 1);
    // Its thread is still its own.
    let room = thread(&base, "bob", &id).await;
    post(&base, "carol", &room, json!({ "text": "still here" })).await;
    assert_eq!(messages(&base, &room).await.len(), 1);
}

/// An agent drafts the export with its machine's grants, and waits for its
/// person: no machine is asked, and its person sends it through one.
#[tokio::test]
async fn an_agent_only_drafts_the_export() {
    let (hub, _gh, app, base, team) = start().await;
    hub.daemon("geek", "alice", None);
    let id = new_issue(&base, "alice", &team, "Agent found it", "").await;
    let draft = |who: &'static str| {
        let (base, id) = (base.clone(), id.clone());
        async move {
            let r = reqwest::Client::new()
                .post(format!("{base}/api/forge/drafts"))
                .header("x-test-daemon", who)
                .json(&json!({ "repo": "acme/app", "kind": "issue", "native_issue": id, "agent": "mcp:claude-code" }))
                .send()
                .await
                .unwrap();
            (r.status().as_u16(), r.json::<Value>().await.unwrap())
        }
    };
    // It reads the issue, and changes it, with its machine's grants over
    // the project: without issues.write it's refused.
    let give = |power: &str| json!({ "daemon": "geek", "power": power, "scope": { "kind": "project", "id": team } });
    let (s, g) = call(&base, "alice", "POST", "/api/grants", Some(give("threads.read"))).await;
    assert_eq!(s, 200, "{g}");
    let (s, v) = draft("alice geek").await;
    assert_eq!(s, 403, "{v}");
    assert!(v["error"].as_str().unwrap().contains("issues.write"), "{v}");
    let (s, g) = call(&base, "alice", "POST", "/api/grants", Some(give("issues.write"))).await;
    assert_eq!(s, 200, "{g}");
    let (s, v) = draft("alice geek").await;
    assert_eq!(s, 200, "{v}");
    assert_eq!((v["draft"]["state"].as_str(), v["draft"]["machine"].as_str()), (Some("waiting"), Some("geek")));
    assert!(hub.ops_asked().is_empty());
    assert!(issue(&base, "alice", &id).await["github"].is_null());

    // Its person sends it, through their machine.
    hub.answers_ops("geek", OpAnswer::Done { url: Some("https://github.com/acme/app/issues/7".into()), routed: None });
    let d = v["draft"]["id"].as_str().unwrap();
    let (s, v) = call(&base, "alice", "POST", &format!("/api/forge/drafts/{d}/send"), Some(json!({}))).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(issue(&base, "alice", &id).await["github"], json!({ "repo": "acme/app", "number": 7 }));
    let _ = app;
}

/// On an exported issue, a viewer's own comment goes to GitHub as them
/// (a draft their device sends), and their agent's stays a native comment,
/// saying why: a viewer's machine makes no drafts.
#[tokio::test]
async fn a_viewers_comment_goes_out_and_their_agents_stays() {
    let (hub, _gh, app, base, team) = start().await;
    let (id, room) = exported(&hub, &base, &team, "Seen by a viewer").await;
    hub.daemon("cbox", "carol", None);
    let v = post(&base, "carol", &room, json!({ "text": "from carol" })).await;
    // Hers goes out as her, through one of her machines.
    assert_eq!(v["github"]["sending"], true, "{v}");
    assert!(v["github"]["draft"].as_str().unwrap().starts_with("dft_"), "her own draft: {v}");
    let got = export::posted(&app, &id, &room, "carol", Some("cbox"), 43, "the agent's note").await.unwrap();
    assert!(got.draft.is_empty(), "no draft: {got:?}");
    assert!(got.why.as_deref().unwrap_or_default().contains("your owner only views"), "{got:?}");
    let (_, ds) = call(&base, "carol", "GET", "/api/forge/drafts?state=all", None).await;
    let texts: Vec<&str> = ds["drafts"].as_array().unwrap().iter().filter_map(|d| d["body"].as_str()).collect();
    assert_eq!(texts, ["from carol"], "only hers: {ds}");
}

/// Once it's on a public repo: a person's comment goes there as them (and
/// GitHub's copy of it, even one that comes before the send's answer, isn't
/// brought back); a GitHub comment comes into the thread as GitHub's, once,
/// by webhook or by the poller; an agent's waits as a draft; a reply kept
/// in a sub-thread stays here; with no machine awake it waits, saying so.
#[tokio::test]
async fn comments_cross_both_ways_once() {
    let (hub, gh, app, base, team) = start().await;
    let (id, room) = exported(&hub, &base, &team, "Flaky login").await;
    let (bob_hears, alice_hears) = (live(&base, "bob").await, live(&base, "alice").await);

    // Bob's machine sends his comment; GitHub's webhook for it lands while
    // the send is still out.
    hub.daemon("bobs", "bob", None);
    let echo = app.clone();
    hub.on_ops(Arc::new(move |daemon, op| {
        let app = echo.clone();
        Box::pin(async move {
            let DaemonOp::ForgeSend(s) = op else { return OpAnswer::Unknown };
            if daemon != "bobs" || s.kind != "comment" {
                return OpAnswer::Offline;
            }
            export::webhook(&app, "issue_comment", &comment(7, 501, "bob-gh", &format!("{}\r\n", s.body))).await;
            OpAnswer::Done { url: Some("https://github.com/acme/site/issues/7#issuecomment-501".into()), routed: None }
        })
    }));
    let (bobs, g) = post_out(&base, "bob", &room, &bob_hears, "On it").await;
    assert_eq!(g["sent"], true, "{g}");
    assert_eq!(g["url"], "https://github.com/acme/site/issues/7#issuecomment-501");
    assert_eq!(messages(&base, &room).await.len(), 1, "its copy isn't brought back");
    assert_eq!(crossed(&app), [(501, "out".into(), Some(bobs))]);
    // A redelivery of it later: still not.
    export::webhook(&app, "issue_comment", &comment(7, 501, "bob-gh", "On it")).await;
    assert_eq!(messages(&base, &room).await.len(), 1);

    // Someone on GitHub: into the thread as theirs on GitHub, once.
    let dana = comment(7, 777, "dana-gh", "Seen on 1.2 too");
    export::webhook(&app, "issue_comment", &dana).await;
    export::webhook(&app, "issue_comment", &dana).await;
    let ms = messages(&base, &room).await;
    assert_eq!(ms.len(), 2, "{ms:?}");
    let m = &ms[1];
    assert_eq!(
        (m["author"].as_str(), m["via"].as_str(), m["text"].as_str()),
        (Some(""), Some("github:dana-gh"), Some("Seen on 1.2 too"))
    );
    // ... and never sent back: the only sends were alice's export and bob's.
    let sends = hub.ops_asked().len();
    assert_eq!(sends, 2);
    // A PR's comment, another issue's and an edit come to nothing.
    let mut pr = comment(7, 778, "x", "pr");
    pr["issue"]["pull_request"] = json!({});
    export::webhook(&app, "issue_comment", &pr).await;
    export::webhook(&app, "issue_comment", &comment(8, 779, "x", "other")).await;
    let mut edit = comment(7, 777, "dana-gh", "edited");
    edit["action"] = json!("edited");
    export::webhook(&app, "issue_comment", &edit).await;
    assert_eq!(messages(&base, &room).await.len(), 2);

    // A reply kept in a sub-thread stays here.
    let v = post(&base, "bob", &room, json!({ "text": "a side note", "parent": bobs })).await;
    assert!(v["github"].is_null(), "{v}");
    assert_eq!(hub.ops_asked().len(), sends);

    // An agent's comment is a draft for its person, and nothing is asked.
    let got = export::posted(&app, &id, &room, "alice", Some("geek"), 42, "the agent's note").await.unwrap();
    assert_eq!((got.sent, got.why.as_deref()), (false, Some(AGENT_WAITS)));
    assert_eq!(hub.ops_asked().len(), sends);
    let (_, ds) = call(&base, "alice", "GET", "/api/forge/drafts", None).await;
    let agents: Vec<&Value> = ds["drafts"].as_array().unwrap().iter().filter(|d| d["id"] == json!(got.draft)).collect();
    assert_eq!((agents[0]["state"].as_str(), agents[0]["native_issue"].as_str()), (Some("waiting"), Some(id.as_str())));

    // Alice's machine is away: her comment waits, saying so.
    let (_, g) = post_out(&base, "alice", &room, &alice_hears, "looking").await;
    assert_eq!(g["sent"], false, "{g}");
    assert!(g["why"].as_str().unwrap().contains("Open this on a device"), "{g}");
    // Each send's end was told to its author only.
    assert_eq!(bob_hears.lock().unwrap().matches(r#""type":"github""#).count(), 1);
    assert_eq!(alice_hears.lock().unwrap().matches(r#""type":"github""#).count(), 1);

    // A webhook that never came: the poller's turn over the issue finds it,
    // and only it (ours and dana's crossed already).
    let list = |id: u64, login: &str, body: &str| json!({ "id": id, "body": body, "user": { "login": login } });
    gh.lock().unwrap().comments.insert(
        (88, 7),
        vec![list(501, "bob-gh", "On it"), list(777, "dana-gh", "Seen on 1.2 too"), list(780, "erin-gh", "late one")],
    );
    export::polled(&app, (88, 9, "acme/site"), &[7, 8]).await;
    export::polled(&app, (88, 9, "acme/site"), &[7]).await;
    let ms = messages(&base, &room).await;
    let theirs: Vec<(&str, &str)> = ms
        .iter()
        .filter(|m| m["via"].as_str().is_some_and(|v| v.starts_with("github:")))
        .map(|m| (m["via"].as_str().unwrap(), m["text"].as_str().unwrap()))
        .collect();
    assert_eq!(theirs, [("github:dana-gh", "Seen on 1.2 too"), ("github:erin-gh", "late one")]);
    let ways: Vec<(u64, String)> = crossed(&app).into_iter().map(|(g, w, _)| (g, w)).collect();
    assert_eq!(ways, [(501, "out".into()), (777, "in".into()), (780, "in".into())]);
}

/// On a private repo, a person's comment still goes out as them, but GitHub's
/// stay on GitHub: the thread is the whole project's, and a private repo's
/// text reaches only those GitHub lets read it (aru-086). The comment of
/// someone GitHub won't let read the repo stays here.
#[tokio::test]
async fn a_private_repos_comments_stay_on_github() {
    let (hub, _gh, app, base, team) = start().await;
    let id = new_issue(&base, "alice", &team, "Secret roadmap", "").await;
    hub.daemon("geek", "alice", None);
    hub.daemon("bobs", "bob", None);
    hub.answers_ops("geek", OpAnswer::Done { url: Some("https://github.com/acme/app/issues/3".into()), routed: None });
    let (_, v) = export_draft(&base, "alice", &id, "acme/app").await;
    let d = v["draft"]["id"].as_str().unwrap().to_owned();
    let (s, v) = call(&base, "alice", "POST", &format!("/api/forge/drafts/{d}/send"), Some(json!({}))).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(issue(&base, "carol", &id).await["github"], json!({ "repo": "acme/app", "number": 3 }));
    let room = thread(&base, "bob", &id).await;
    let bob_hears = live(&base, "bob").await;

    hub.answers_ops(
        "bobs",
        OpAnswer::Done { url: Some("https://github.com/acme/app/issues/3#issuecomment-61".into()), routed: None },
    );
    let (_, g) = post_out(&base, "bob", &room, &bob_hears, "noted").await;
    assert_eq!(g["sent"], true, "{g}");
    let v = post(&base, "carol", &room, json!({ "text": "me too" })).await;
    assert_eq!(v["github"]["sent"], false, "{v}");
    assert!(v["github"]["draft"].as_str().unwrap().is_empty(), "no draft: {v}");
    assert!(v["github"]["why"].as_str().unwrap().contains("doesn't let you read acme/app"), "{v}");

    export::webhook(&app, "issue_comment", &comment_on((77, "acme/app"), 3, 62, "dana-gh", "private words")).await;
    let ms = messages(&base, &room).await;
    assert_eq!(ms.len(), 2, "{ms:?}");
    assert!(ms.iter().all(|m| m["text"] != "private words"));
    assert_eq!(crossed(&app), [(61, "out".into(), Some(ms[0]["seq"].as_i64().unwrap()))]);
}

/// A person's comment on an exported issue answers once it's stored: its
/// send through their machine runs after the post (Arugula #584), here held
/// up until the test lets it go, and how it ended reaches its author, and
/// only them, on the live stream.
#[tokio::test]
async fn a_comments_post_answers_before_its_send_ends() {
    let (hub, _gh, _app, base, team) = start().await;
    let (_id, room) = exported(&hub, &base, &team, "Slow machine").await;
    hub.daemon("bobs", "bob", None);
    let (asked, go) = (Arc::new(tokio::sync::Notify::new()), Arc::new(tokio::sync::Notify::new()));
    let (on_asked, on_go) = (asked.clone(), go.clone());
    hub.on_ops(Arc::new(move |daemon, op| {
        let (asked, go) = (on_asked.clone(), on_go.clone());
        Box::pin(async move {
            let DaemonOp::ForgeSend(s) = op else { return OpAnswer::Unknown };
            if daemon != "bobs" || s.kind != "comment" {
                return OpAnswer::Offline;
            }
            asked.notify_one();
            go.notified().await;
            OpAnswer::Done { url: Some("https://github.com/acme/site/issues/7#issuecomment-90".into()), routed: None }
        })
    }));
    let (bob_hears, alice_hears) = (live(&base, "bob").await, live(&base, "alice").await);

    let v = tokio::time::timeout(Duration::from_secs(5), post(&base, "bob", &room, json!({ "text": "On it" })))
        .await
        .expect("the post answers while its send waits on the machine");
    assert_eq!((v["github"]["sending"].as_bool(), v["github"]["sent"].as_bool()), (Some(true), Some(false)), "{v}");
    assert!(v["github"]["url"].is_null(), "{v}");
    let seq = v["seq"].as_i64().unwrap();
    tokio::time::timeout(Duration::from_secs(5), asked.notified()).await.expect("its machine is asked");
    // Stored and shown already; nothing said of the send yet.
    assert_eq!(said(&base, &room).await, [("bob".to_owned(), "On it".to_owned())]);
    assert!(!bob_hears.lock().unwrap().contains(r#""type":"github""#));

    go.notify_one();
    let g = send_ended(&bob_hears, &room, seq).await;
    assert_eq!(
        g,
        json!({
            "draft": v["github"]["draft"],
            "sending": false,
            "sent": true,
            "url": "https://github.com/acme/site/issues/7#issuecomment-90",
            "why": null,
        })
    );
    // Alice sees the room, and isn't told of Bob's send.
    hears(&alice_hears, r#""type":"message""#, Duration::from_secs(5)).await;
    assert!(!alice_hears.lock().unwrap().contains(r#""type":"github""#));
}

/// Someone on GitHub writes the same words as a comment of ours while it's
/// being sent (Arugula #585): the send's answer names ours, which is kept
/// out, and theirs comes in, each once, whichever webhook comes first, and
/// one that comes after the answer. Only when the answer named no comment
/// is the first with its text taken for ours, and only in the send's window.
#[tokio::test]
async fn the_same_words_on_github_mid_send_both_end_up_right() {
    let (hub, gh, app, base, team) = start().await;
    let (id, room) = exported(&hub, &base, &team, "Echoes").await;
    hub.daemon("bobs", "bob", None);
    let bob_hears = live(&base, "bob").await;
    // Per text: GitHub's comments that land while Bob's is being sent, in
    // order (ours has GitHub's line end), and the id the answer names.
    let ours = |n: u64, text: &str| (n, "bob-gh", format!("{text}\r\n"));
    let plan: Vec<(&str, Vec<Landing>, Option<u64>)> = vec![
        ("+1", vec![(601, "dana-gh", "+1".into()), ours(602, "+1")], Some(602)),
        ("thanks", vec![ours(603, "thanks"), (604, "erin-gh", "thanks".into())], Some(603)),
        ("lgtm", vec![(605, "frank-gh", "lgtm".into())], Some(606)),
        ("done", vec![], None),
        ("ok", vec![], None),
    ];
    let echo = app.clone();
    hub.on_ops(Arc::new(move |daemon, op| {
        let (app, plan) = (echo.clone(), plan.clone());
        Box::pin(async move {
            let DaemonOp::ForgeSend(s) = op else { return OpAnswer::Unknown };
            if daemon != "bobs" || s.kind != "comment" {
                return OpAnswer::Offline;
            }
            let (_, land, answer) = plan.iter().find(|(t, ..)| *t == s.body).expect("a planned comment");
            for (n, login, body) in land {
                export::webhook(&app, "issue_comment", &comment(7, *n, login, body)).await;
            }
            let url = match answer {
                Some(n) => format!("https://github.com/acme/site/issues/7#issuecomment-{n}"),
                None => "https://github.com/acme/site/issues/7".into(),
            };
            OpAnswer::Done { url: Some(url), routed: None }
        })
    }));

    let mut seqs = vec![];
    for text in ["+1", "thanks", "lgtm"] {
        let (seq, g) = post_out(&base, "bob", &room, &bob_hears, text).await;
        assert_eq!(g["sent"], true, "{g}");
        seqs.push(seq);
    }
    // Ours for "lgtm", after the answer named it: kept out.
    export::webhook(&app, "issue_comment", &comment(7, 606, "bob-gh", "lgtm")).await;
    // An answer that named no comment: the first "done" after it is ours.
    let (done, _) = post_out(&base, "bob", &room, &bob_hears, "done").await;
    export::webhook(&app, "issue_comment", &comment(7, 607, "bob-gh", "done")).await;
    export::webhook(&app, "issue_comment", &comment(7, 608, "gail-gh", "done")).await;
    // ... and only in the send's window: past it, the same words are someone's.
    let (ok, _) = post_out(&base, "bob", &room, &bob_hears, "ok").await;
    let long_ago = arugula_e2e::now_ms() - crate::db::forge::SENDING_MS - 1_000;
    app.db.c().execute("UPDATE drafts SET updated_at = ?1 WHERE native_seq = ?2", params![long_ago, ok]).unwrap();
    export::webhook(&app, "issue_comment", &comment(7, 609, "hal-gh", "ok")).await;

    let pair = |who: &str, text: &str| (who.to_owned(), text.to_owned());
    let want = [
        pair("bob", "+1"),
        pair("github:dana-gh", "+1"),
        pair("bob", "thanks"),
        pair("github:erin-gh", "thanks"),
        pair("bob", "lgtm"),
        pair("github:frank-gh", "lgtm"),
        pair("bob", "done"),
        pair("github:gail-gh", "done"),
        pair("bob", "ok"),
        pair("github:hal-gh", "ok"),
    ];
    assert_eq!(said(&base, &room).await, want);
    let ways: Vec<(u64, String)> = crossed(&app).into_iter().map(|(g, w, _)| (g, w)).collect();
    let way = |n: u64, w: &str| (n, w.to_owned());
    assert_eq!(
        ways,
        [
            way(601, "in"),
            way(602, "out"),
            way(603, "out"),
            way(604, "in"),
            way(605, "in"),
            way(606, "out"),
            way(607, "out"),
            way(608, "in"),
            way(609, "in"),
        ]
    );
    let outs: Vec<(u64, Option<i64>)> =
        crossed(&app).into_iter().filter(|(_, w, _)| w == "out").map(|(g, _, s)| (g, s)).collect();
    assert_eq!(outs, [(602, Some(seqs[0])), (603, Some(seqs[1])), (606, Some(seqs[2])), (607, Some(done))]);
    let held: i64 = app.db.c().query_row("SELECT count(*) FROM issue_github_held", params![], |r| r.get(0)).unwrap();
    assert_eq!(held, 0, "nothing left held");

    // Again, by the poller, with every one: nothing new.
    let list: Vec<Value> = [
        (601, "dana-gh", "+1"),
        (602, "bob-gh", "+1"),
        (603, "bob-gh", "thanks"),
        (604, "erin-gh", "thanks"),
        (605, "frank-gh", "lgtm"),
        (606, "bob-gh", "lgtm"),
        (607, "bob-gh", "done"),
        (608, "gail-gh", "done"),
        (609, "hal-gh", "ok"),
    ]
    .iter()
    .map(|(n, login, body)| json!({ "id": n, "body": body, "user": { "login": login } }))
    .collect();
    gh.lock().unwrap().comments.insert((88, 7), list);
    export::polled(&app, (88, 9, "acme/site"), &[7]).await;
    assert_eq!(said(&base, &room).await, want);
    let _ = id;
}
