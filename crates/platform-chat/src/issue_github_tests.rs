//! Open on GitHub, per issue (M205, aru-130), in the base's world on a fake
//! GitHub (`forge::viewer_tests::world`): team t1's project links
//! `acme/app` (77, private: alice and bob read it on GitHub, carol doesn't)
//! and `acme/site` (88). A native issue is a draft its person sends as
//! themselves; once it's on GitHub its origin and `exported` say so, and
//! comments cross both ways through the mirror, each once and never back.
//! Runs on SQLite and, with `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::sync::Arc;

use arugula_control_wire::forge::{DaemonOp, OpAnswer};
use arugula_platform::{PlatformPort as _, forge::send::UserToken};
use serde_json::{Value, json};

use crate::{
    Platform,
    chat_tests::{TestHub, call},
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

/// Once it's on a public repo: a person's comment goes there as them (and
/// GitHub's copy of it, even one that comes before the send's answer, isn't
/// brought back); a GitHub comment comes into the thread as GitHub's, once,
/// by webhook or by the poller; an agent's waits as a draft; a reply kept
/// in a sub-thread stays here; with no machine awake it waits, saying so.
#[tokio::test]
async fn comments_cross_both_ways_once() {
    let (hub, gh, app, base, team) = start().await;
    let id = new_issue(&base, "alice", &team, "Flaky login", "").await;
    hub.daemon("geek", "alice", None);
    hub.answers_ops("geek", OpAnswer::Done { url: Some("https://github.com/acme/site/issues/7".into()), routed: None });
    let (_, v) = export_draft(&base, "alice", &id, "acme/site").await;
    let d = v["draft"]["id"].as_str().unwrap().to_owned();
    let (s, v) = call(&base, "alice", "POST", &format!("/api/forge/drafts/{d}/send"), Some(json!({}))).await;
    assert_eq!(s, 200, "{v}");
    let room = thread(&base, "bob", &id).await;

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
    let v = post(&base, "bob", &room, json!({ "text": "On it" })).await;
    assert_eq!(v["github"]["sent"], true, "{v}");
    assert_eq!(v["github"]["url"], "https://github.com/acme/site/issues/7#issuecomment-501");
    let bobs = v["seq"].as_i64().unwrap();
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
    let got = export::posted(&app, &id, "alice", Some("geek"), 42, "the agent's note").await.unwrap();
    assert_eq!((got.sent, got.why.as_deref()), (false, Some(AGENT_WAITS)));
    assert_eq!(hub.ops_asked().len(), sends);
    let (_, ds) = call(&base, "alice", "GET", "/api/forge/drafts", None).await;
    let agents: Vec<&Value> = ds["drafts"].as_array().unwrap().iter().filter(|d| d["id"] == json!(got.draft)).collect();
    assert_eq!((agents[0]["state"].as_str(), agents[0]["native_issue"].as_str()), (Some("waiting"), Some(id.as_str())));

    // Alice's machine is away: her comment waits, saying so.
    let v = post(&base, "alice", &room, json!({ "text": "looking" })).await;
    assert_eq!(v["github"]["sent"], false, "{v}");
    assert!(v["github"]["why"].as_str().unwrap().contains("Open this on a device"), "{v}");

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

    hub.answers_ops(
        "bobs",
        OpAnswer::Done { url: Some("https://github.com/acme/app/issues/3#issuecomment-61".into()), routed: None },
    );
    let v = post(&base, "bob", &room, json!({ "text": "noted" })).await;
    assert_eq!(v["github"]["sent"], true, "{v}");
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
