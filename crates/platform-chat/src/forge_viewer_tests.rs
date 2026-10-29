//! The viewer check (M165) on a mirrored item's thread (M196): the base's
//! world on a fake GitHub (`forge::viewer_tests::world`), read through
//! chat's routes.

use std::sync::Arc;

use serde_json::json;

use crate::{
    chat_tests::{TestHub, call},
    forge::viewer_tests::world,
    testing::forge_serve as serve,
};

/// The thread on a private repo's item (M196) is read through the same
/// check: bob, whom GitHub lets in, opens it, comments and reads it back;
/// carol, a member GitHub doesn't know, is refused, and doesn't see it in
/// her chat list or her search.
#[tokio::test]
async fn leak_object_threads() {
    use crate::chat::objects::PRIVATE_REPO;
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    world(&hub, &gh, &app).await;
    hub.flag("team:t1", true);
    let open = |who: &'static str| {
        let base = base.clone();
        async move {
            call(&base, who, "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": "fi_a1" }))).await
        }
    };

    let (st, room) = open("bob").await;
    assert_eq!(st, 200, "{room}");
    let id = room["id"].as_str().unwrap().to_owned();
    let post = Some(json!({ "text": "the secret canary" }));
    assert_eq!(call(&base, "bob", "POST", &format!("/api/chat/rooms/{id}/messages"), post).await.0, 200);
    let (st, v) = call(&base, "bob", "GET", &format!("/api/chat/rooms/{id}/messages"), None).await;
    assert_eq!((st, v["messages"].as_array().map(Vec::len)), (200, Some(1)), "{v}");

    let (st, v) = open("carol").await;
    assert_eq!((st, &v["reason"]), (403, &json!(PRIVATE_REPO)), "{v}");
    assert_eq!(call(&base, "carol", "GET", &format!("/api/chat/rooms/{id}"), None).await.0, 404);
    assert_eq!(call(&base, "carol", "GET", &format!("/api/chat/rooms/{id}/messages"), None).await.0, 404);
    let (_, v) = call(&base, "carol", "GET", "/api/chat", None).await;
    assert!(v["rooms"].as_array().unwrap().iter().all(|r| r["id"] != json!(id)), "{v}");
    let (_, v) = call(&base, "carol", "GET", "/api/chat/search?q=canary", None).await;
    assert_eq!(v["found"], json!([]), "{v}");
}
