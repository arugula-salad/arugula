//! People blocks' room tokens (arugula.stream): a team's members get one
//! for its room, named as the roster names them, and nobody else does. a
//! and b share t1, c is alone in t2; everyone has chat on.

use std::sync::Arc;

use arugula_e2e::team::TeamRole;
use base64::Engine;
use serde_json::{Value, json};

use crate::{
    Platform, Presence,
    chat_tests::{TestHub, call},
};

async fn rig(presence: bool) -> String {
    let hub = Arc::new(TestHub::default());
    hub.team("t1", &[("a", TeamRole::Owner), ("b", TeamRole::Editor)]);
    hub.team("t2", &[("c", TeamRole::Owner)]);
    for who in ["a", "b", "c"] {
        hub.flag(&format!("account:{who}"), true);
    }
    let presence = presence.then(|| Presence {
        app: "app_test".into(),
        secret: "s3cret".into(),
        relay: "wss://relay.example/socket".into(),
    });
    let cfg = crate::Config { public_url: "https://control.example".into(), presence, ..Default::default() };
    let app = Arc::new(Platform::new(cfg, crate::db::Db::test(), hub, reqwest::Client::new(), Default::default()));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app);
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    base
}

fn claims(token: &str) -> Value {
    let part = token.split('.').nth(1).unwrap();
    serde_json::from_slice(&base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
}

#[tokio::test]
async fn a_teams_members_get_a_token_for_its_room_and_nobody_else() {
    let base = rig(true).await;
    let (st, v) = call(&base, "b", "POST", "/api/chat/presence/token", Some(json!({ "room": "team:t1" }))).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["relay"], "wss://relay.example/socket");
    let c = claims(v["token"].as_str().unwrap());
    assert_eq!((&c["app"], &c["room"], &c["sub"]), (&json!("app_test"), &json!("team:t1"), &json!("b")), "{c}");
    assert!(c["name"].as_str().is_some_and(|n| !n.is_empty()), "{c}");

    // Not c's team, and not a team at all.
    let (st, v) = call(&base, "c", "POST", "/api/chat/presence/token", Some(json!({ "room": "team:t1" }))).await;
    assert_eq!(st, 404, "{v}");
    let (st, v) = call(&base, "a", "POST", "/api/chat/presence/token", Some(json!({ "room": "lobby" }))).await;
    assert_eq!(st, 400, "{v}");
}

#[tokio::test]
async fn without_an_app_control_says_so() {
    let base = rig(false).await;
    let (st, v) = call(&base, "a", "POST", "/api/chat/presence/token", Some(json!({ "room": "team:t1" }))).await;
    assert_eq!(st, 503, "{v}");
}
