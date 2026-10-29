//! A board's thread (boards, step 1): an object thread, as an
//! issue's is, with chat on. The board itself is the base's
//! (`boards_tests`).

use std::sync::Arc;

use arugula_e2e::team::TeamRole;
use serde_json::json;

use crate::chat_tests::{TestHub, call, serve};

/// a owns team t, whose project has the board "Launch"; c is in team u.
/// a opens the board's thread, named for it; c doesn't find it.
#[tokio::test]
async fn a_boards_thread_is_its_projects() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    hub.team("t", &[("a", TeamRole::Owner), ("v", TeamRole::Viewer)]);
    hub.team("u", &[("c", TeamRole::Owner)]);
    for team in ["team:t", "team:u"] {
        hub.flag(team, true);
        hub.flag_named(team, crate::boards::FLAG, true);
    }
    let p = app.db.team_project("t", "t", 1).unwrap();
    let (st, b) =
        call(&base, "a", "POST", &format!("/api/projects/{p}/boards"), Some(json!({ "title": "Launch" }))).await;
    assert_eq!(st, 200, "{b}");
    let id = b["board"]["id"].as_str().unwrap().to_owned();

    let open = |who: &'static str| {
        call(&base, who, "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": id })))
    };
    let (st, room) = open("a").await;
    assert_eq!((st, &room["name"]), (200, &json!("board Launch")), "{room}");
    assert_eq!(open("c").await.0, 404, "a stranger doesn't find it");
}
