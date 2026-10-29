//! Outline presence (S43): who hears whose frames. a and b share team t1,
//! c is alone in t2; everyone has chat on.

use std::{sync::Arc, time::Duration};

use arugula_e2e::team::TeamRole;
use base64::Engine;

use crate::chat_tests::{TestHub, hears, listen, serve};

async fn rig() -> String {
    let hub = Arc::new(TestHub::default());
    hub.team("t1", &[("a", TeamRole::Owner), ("b", TeamRole::Editor)]);
    hub.team("t2", &[("c", TeamRole::Owner)]);
    for who in ["a", "b", "c"] {
        hub.flag(&format!("account:{who}"), true);
    }
    serve(hub).await.1
}

async fn post(base: &str, who: &str, path: &str, body: Vec<u8>) -> u16 {
    reqwest::Client::new()
        .post(format!("{base}{path}"))
        .header("cookie", format!("ilg_session=session-{who}"))
        .header("content-type", "application/octet-stream")
        .body(body)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

async fn open(base: &str, who: &str) -> Arc<std::sync::Mutex<String>> {
    let r = reqwest::Client::new()
        .get(format!("{base}/api/people/outlines"))
        .header("cookie", format!("ilg_session=session-{who}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    listen(r)
}

#[tokio::test]
async fn a_frame_reaches_those_who_share_a_team_and_nobody_else() {
    let base = rig().await;
    let b = open(&base, "b").await;
    let c = open(&base, "c").await;
    hears(&b, ": open", Duration::from_secs(5)).await;
    hears(&c, ": open", Duration::from_secs(5)).await;

    let bytes = vec![1u8, 2, 3, 250];
    assert_eq!(post(&base, "a", "/api/people/outline", bytes.clone()).await, 204);
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    hears(&b, "event: frame", Duration::from_secs(5)).await;
    hears(&b, &format!("\"data\":\"{data}\""), Duration::from_secs(5)).await;
    hears(&b, "\"account\":\"a\"", Duration::from_secs(5)).await;

    // A stream opened later gets the frame kept, the person's own included.
    let a = open(&base, "a").await;
    hears(&a, &format!("\"data\":\"{data}\""), Duration::from_secs(5)).await;

    assert_eq!(post(&base, "a", "/api/people/outline/off", vec![]).await, 204);
    hears(&b, "event: off", Duration::from_secs(5)).await;

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!c.lock().unwrap().contains("event:"), "c shares no team with a: {}", c.lock().unwrap());
}

#[tokio::test]
async fn a_frame_is_at_most_32_kib_and_not_empty() {
    let base = rig().await;
    assert_eq!(post(&base, "a", "/api/people/outline", vec![0; crate::chat::outlines::MAX_FRAME]).await, 204);
    assert_eq!(post(&base, "b", "/api/people/outline", vec![0; crate::chat::outlines::MAX_FRAME + 1]).await, 413);
    assert_eq!(post(&base, "b", "/api/people/outline", vec![]).await, 400);
}
