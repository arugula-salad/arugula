//! Hosted inference (M185) through the platform's routes and port: a
//! project's own keys (sealed here, written into its boxes' egress rules),
//! a box's turns and whose key pays for them, the caps on the platform's
//! key, and the rules each box is given. Against the fake
//! provider and the test hub, on SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::sync::Arc;

use arugula_e2e::{now_ms, team::TeamRole};
use serde_json::{Value, json};

use super::{
    Config, EnrollBy,
    inference::{Caps, Provider, Settings, platform_secret},
    tests::{Fake, drive, make},
};
use crate::{
    Platform, PlatformPort as _, TurnAnswer,
    chat_tests::{TestHub, call},
    compute::{self, Substrate as _},
    secrets::SecretCipher,
};

/// alice owns team t1 and isn't on the allowlist; bob edits it and is.
/// The platform's key covers `platform`, with `caps`.
async fn serve(platform: &[&str], caps: Caps) -> (Arc<TestHub>, Arc<Fake>, Arc<Platform>, String, String) {
    let hub = Arc::new(TestHub::default());
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("vic", TeamRole::Viewer)]);
    let fake = Arc::new(Fake::default());
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, crate::db::Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    p.compute = Some(fake.clone());
    let platform: Vec<String> = platform.iter().map(|s| (*s).to_owned()).collect();
    p.hosted = Config {
        template: Some("tpl".into()),
        enroll: EnrollBy::Sandbox,
        accounts: vec!["alice".into(), "bob".into()],
        cipher: Some(Arc::new(SecretCipher::new(&"01".repeat(32)).unwrap())),
        inference: Settings::new(&["bob".into(), "*".into()], &platform, caps),
        ..Default::default()
    };
    let app = Arc::new(p);
    let project = app.db.team_project("t1", "t1", 0).unwrap();
    hub.delegate(&project);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (hub, fake, app, base, project)
}

const CAPS: Caps = Caps { concurrent: 1, daily: 2, minutes: 600 };

/// `who` makes a workspace, and its box enrolls as `machine`: its id.
async fn ready(hub: &TestHub, app: &Platform, base: &str, project: &str, who: &str, machine: &str) -> String {
    let (status, w) = make(base, who, project, machine).await;
    assert_eq!(status, 201, "{w}");
    let id = w["id"].as_str().unwrap().to_owned();
    drive(app, now_ms()).await;
    hub.enroll(&id, machine);
    drive(app, now_ms() + 3_000).await;
    id
}

/// The box of workspace `w`, enrolled as `machine`, asks to start a turn
/// of `agent`, as the hub hands it on: the status and body the hub
/// answers with.
async fn turn(app: &Platform, w: &str, machine: &str, agent: &str) -> (u16, Value) {
    match app.hosted_turn(w, machine, agent).await.unwrap() {
        TurnAnswer::Given { turn, pays, auth } => (200, json!({ "turn": turn, "pays": pays, "auth": auth })),
        TurnAnswer::Refused { status, reason, error } => (status, json!({ "reason": reason, "error": error })),
    }
}

async fn end(app: &Platform, w: &str, machine: &str, turn: &Value) -> bool {
    app.hosted_turn_end(w, machine, turn["turn"].as_str().unwrap()).await.unwrap()
}

async fn key(base: &str, who: &str, project: &str, provider: &str, value: Option<&str>) -> (u16, Value) {
    let path = format!("/api/projects/{project}/inference-keys/{provider}");
    match value {
        Some(v) => call(base, who, "PUT", &path, Some(json!({ "value": v }))).await,
        None => call(base, who, "DELETE", &path, None).await,
    }
}

/// The rules the fake provider holds for the one box.
fn rules(fake: &Fake) -> compute::Rules {
    fake.rules.lock().unwrap().values().next().cloned().unwrap_or_default()
}

/// Test values, not keys.
const SK: &str = "sk-ant-test-0123456789"; // gitleaks:allow: a test value
const SK2: &str = "sk-ant-test-9876543210"; // gitleaks:allow: a test value
const OPENAI: &str = "sk-proj-test-0123456789"; // gitleaks:allow: a test value
const SUBSCRIPTION: &str = "sk-ant-oat01-test-0123456789"; // gitleaks:allow: a test value

/// A workspace whose maker isn't on the allowlist can't run an agent
/// until an owner adds a key, then can. The key is sealed here (nothing
/// here holds it in the clear, the rules' digest included), shown as set
/// and never read back, and written into the running box's egress rules
/// at once; a new one replaces it there. Cleared while the box is paused,
/// it's gone from the box's rules when it wakes, and the next turn is
/// refused.
#[tokio::test]
async fn a_stranger_brings_a_key() {
    let (hub, fake, app, base, project) = serve(&["anthropic"], CAPS).await;
    let id = ready(&hub, &app, &base, &project, "alice", "box-a").await;
    let w = id.as_str();
    assert!(fake.creates()[0].rules.is_empty(), "no key, no rules");
    assert!(fake.creates()[0].env.is_empty(), "never a key in the env");

    let (status, no) = turn(&app, w, "box-a", "claude").await;
    assert_eq!((status, no["reason"].as_str()), (403, Some("no_key")), "{no}");
    let said = no["error"].as_str().unwrap();
    assert!(said.contains(&format!("arugula cloud key set anthropic --project {project}")), "{said}");

    assert_eq!(key(&base, "bob", &project, "anthropic", Some(SK)).await.0, 403, "an editor");
    assert_eq!(key(&base, "alice", &project, "gemini", Some(SK)).await.0, 404);
    assert_eq!(key(&base, "alice", &project, "anthropic", Some("not a key")).await.0, 422);
    let (status, shown) = key(&base, "alice", &project, "anthropic", Some(SK)).await;
    assert_eq!(status, 200, "{shown}");
    assert_eq!(rules(&fake)["api.anthropic.com"]["x-api-key"], SK, "written into the running box's rules");
    let (status, listed) = call(&base, "vic", "GET", &format!("/api/projects/{project}/inference-keys"), None).await;
    assert_eq!(status, 200);
    assert_eq!((listed["keys"][0]["set"].as_bool(), listed["keys"][0]["set_by"].as_str()), (Some(true), Some("alice")));
    assert_eq!(listed["keys"][1]["set"], false);
    assert!(!listed.to_string().contains(SK) && !shown.to_string().contains(SK), "never read back");
    assert!(app.db.mentions(SK).is_empty(), "never kept in the clear: {:?}", app.db.mentions(SK));

    let (status, t) = turn(&app, w, "box-a", "claude").await;
    assert_eq!((status, t["pays"].as_str()), (200, Some("project")), "{t}");
    assert!(end(&app, w, "box-a", &t).await);
    // A project's own key has no cap.
    for _ in 0..3 {
        let (status, t) = turn(&app, w, "box-a", "claude").await;
        assert_eq!(status, 200, "{t}");
    }
    assert_eq!(key(&base, "alice", &project, "anthropic", Some(SK2)).await.0, 200);
    assert_eq!(rules(&fake)["api.anthropic.com"]["x-api-key"], SK2, "a new key replaces it at once");

    // Paused, then the key's cleared: the paused box can't be changed, so
    // it loses the key when it wakes.
    let sandbox = fake.boxes()[0].id.clone();
    fake.pause(&sandbox).await.unwrap();
    drive(&app, now_ms() + 120_000).await;
    assert_eq!(app.db.hosted_workspace(&project, w).unwrap().unwrap().state, "paused");
    assert_eq!(key(&base, "alice", &project, "anthropic", None).await.0, 200);
    assert_eq!(rules(&fake)["api.anthropic.com"]["x-api-key"], SK2, "still in the paused box's rules");
    let (status, _) = call(&base, "alice", "GET", &format!("/api/projects/{project}/workspaces/{w}"), None).await;
    assert_eq!(status, 200);
    drive(&app, now_ms() + 121_000).await;
    assert!(rules(&fake).is_empty(), "gone from its rules once it woke: {:?}", rules(&fake));
    let (status, no) = turn(&app, w, "box-a", "claude").await;
    assert_eq!((status, no["reason"].as_str()), (403, Some("no_key")));
}

/// A project's Anthropic key may be a Claude subscription's token
/// (`claude setup-token`): the box's rules set it as a bearer, never as
/// `x-api-key`, and its turns say so, for the box's Claude Code to hold
/// its placeholder as one. An API key put back is a key again.
#[tokio::test]
async fn a_subscription_token_goes_as_a_bearer() {
    let (hub, fake, app, base, project) = serve(&["anthropic"], CAPS).await;
    let w = &ready(&hub, &app, &base, &project, "bob", "box-b").await;
    let (_, t) = turn(&app, w, "box-b", "claude").await;
    assert_eq!((t["pays"].as_str(), t["auth"].as_str()), (Some("platform"), Some("key")), "{t}");
    assert!(end(&app, w, "box-b", &t).await);

    assert_eq!(key(&base, "alice", &project, "anthropic", Some(SUBSCRIPTION)).await.0, 200);
    let set = &rules(&fake)["api.anthropic.com"];
    assert_eq!(set.get("authorization").map(String::as_str), Some(format!("Bearer {SUBSCRIPTION}").as_str()));
    assert!(!set.contains_key("x-api-key"), "{:?}", set.keys());
    let (status, t) = turn(&app, w, "box-b", "claude").await;
    assert_eq!((status, t["pays"].as_str(), t["auth"].as_str()), (200, Some("project"), Some("subscription")), "{t}");
    assert!(end(&app, w, "box-b", &t).await);
    assert!(app.db.mentions(SUBSCRIPTION).is_empty(), "never kept in the clear");

    assert_eq!(key(&base, "alice", &project, "anthropic", Some(SK)).await.0, 200);
    assert_eq!(rules(&fake)["api.anthropic.com"]["x-api-key"], SK);
    let (_, t) = turn(&app, w, "box-b", "claude").await;
    assert_eq!(t["auth"].as_str(), Some("key"), "{t}");
}

/// With no key for the agent's provider, a turn is told so (`no_key`),
/// even when the box's rules can't be set; with a key, rules that can't
/// be set are a 503, never a turn on a key the box doesn't have.
#[tokio::test]
async fn no_key_is_said_even_when_the_rules_cant_be_set() {
    let (hub, fake, app, base, project) = serve(&["anthropic"], CAPS).await;
    let w = &ready(&hub, &app, &base, &project, "alice", "box-a").await;
    // The box is gone from the provider, and its rules are stale.
    fake.boxes.lock().unwrap().clear();
    app.db.set_hosted_rules(w, "stale").unwrap();
    let (status, no) = turn(&app, w, "box-a", "claude").await;
    assert_eq!((status, no["reason"].as_str()), (403, Some("no_key")), "{no}");
    assert_eq!(key(&base, "alice", &project, "anthropic", Some(SK)).await.0, 200);
    let (status, later) = turn(&app, w, "box-a", "claude").await;
    assert_eq!((status, later["reason"].as_str()), (503, None), "{later}");
}

/// A control with no key to seal them with keeps no inference keys.
#[tokio::test]
async fn no_sealing_key_no_project_keys() {
    let (hub, fake, app, _, _) = serve(&[], CAPS).await;
    let mut p =
        Platform::new(crate::Config::default(), crate::db::Db::test(), hub, reqwest::Client::new(), Default::default());
    p.compute = Some(fake);
    p.hosted = Config { cipher: None, ..app.hosted.clone() };
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(Arc::new(p));
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    assert_eq!(key(&base, "alice", "personal", "anthropic", Some(SK)).await.0, 501);
}

/// A workspace an allowlisted account made runs on the platform's key,
/// for the providers it covers, under that account's caps; a project key
/// takes over where there is one.
#[tokio::test]
async fn the_allowlist_runs_on_the_platform_key_under_caps() {
    let (hub, fake, app, base, project) = serve(&["anthropic"], CAPS).await;
    let w = &ready(&hub, &app, &base, &project, "bob", "box-b").await;
    let made = &fake.creates()[0].rules;
    assert_eq!(made["api.anthropic.com"]["x-api-key"], compute::fill(&platform_secret(Provider::Anthropic)));
    assert!(!made.contains_key("api.openai.com"), "the platform doesn't cover OpenAI");

    let (status, t) = turn(&app, w, "box-b", "claude").await;
    assert_eq!((status, t["pays"].as_str()), (200, Some("platform")), "{t}");
    let (status, cap) = turn(&app, w, "box-b", "claude").await;
    assert_eq!((status, cap["reason"].as_str()), (429, Some("cap")), "{cap}");
    assert!(cap["error"].as_str().unwrap().contains("1 agent turns at once"), "{cap}");
    end(&app, w, "box-b", &t).await;
    let (status, t) = turn(&app, w, "box-b", "claude").await;
    assert_eq!(status, 200, "{t}");
    end(&app, w, "box-b", &t).await;
    let (status, cap) = turn(&app, w, "box-b", "claude").await;
    assert_eq!(status, 429);
    assert!(cap["error"].as_str().unwrap().contains("2 agent turns today (UTC)"), "{cap}");

    let (status, no) = turn(&app, w, "box-b", "codex").await;
    assert_eq!((status, no["reason"].as_str()), (403, Some("no_key")), "{no}");
    assert_eq!(key(&base, "alice", &project, "openai", Some(OPENAI)).await.0, 200);
    let (status, t) = turn(&app, w, "box-b", "codex").await;
    assert_eq!((status, t["pays"].as_str()), (200, Some("project")), "{t}");
    let r = rules(&fake);
    assert_eq!(r["api.openai.com"]["authorization"], format!("Bearer {OPENAI}"));
    assert_eq!(r["api.anthropic.com"]["x-api-key"], compute::fill(&platform_secret(Provider::Anthropic)));
}

/// Only a hosted box starts its turns, as the workspace and machine its
/// certificate names, and ends only its own; an unknown agent is refused.
#[tokio::test]
async fn only_a_hosted_box_asks() {
    let (hub, _fake, app, base, project) = serve(&["anthropic"], CAPS).await;
    let b = &ready(&hub, &app, &base, &project, "bob", "box-b").await;
    let c = &ready(&hub, &app, &base, &project, "bob", "box-c").await;
    assert_eq!(turn(&app, b, "laptop", "claude").await.0, 404, "not a hosted workspace's machine");
    assert_eq!(turn(&app, c, "box-b", "claude").await.0, 404, "another workspace's machine");
    assert_eq!(turn(&app, b, "box-b", "gemini").await.0, 400);
    let (_, t) = turn(&app, b, "box-b", "claude").await;
    assert!(!end(&app, c, "box-c", &t).await, "another box's turn");
    assert!(end(&app, b, "box-b", &t).await);
}

/// When the account whose personal project held a key is deleted, the
/// sweep drops the key.
#[tokio::test]
async fn a_gone_projects_key_is_dropped() {
    let (_hub, _fake, app, base, _) = serve(&[], CAPS).await;
    assert_eq!(key(&base, "alice", "personal", "anthropic", Some(SK)).await.0, 200);
    let d = crate::Deleted { account: "alice".into(), machines: vec![], disbanded: vec![] };
    let mut c = app.db.c();
    let tx = c.transaction().unwrap();
    crate::db::inference::erase_account(&tx, &d).unwrap();
    crate::db::projects::erase_account(&tx, &d).unwrap();
    tx.commit().unwrap();
    drop(c);
    super::inference::sweep(&app).await.unwrap();
    assert_eq!(app.db.drop_orphan_inference_keys().unwrap(), 0, "dropped already");
}
