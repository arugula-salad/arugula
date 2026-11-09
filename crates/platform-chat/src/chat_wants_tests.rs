//! What wants a reader in an object's thread ([`wire::ChatRoom::wants`]):
//! its mentions, and on an open native issue assigned to them what isn't
//! from their own agents. Everything else stays `unread`, for a dot.

use std::sync::Arc;

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{
    Platform,
    chat_tests::{TestHub, call, serve},
    db::{
        chat::NewMessage,
        sql::{Sql as _, params},
    },
};

/// Team t's project, with a and b in it, and an issue assigned to a and one
/// assigned to nobody, each with its thread open: the two rooms' ids.
async fn seed(app: &Platform, hub: &TestHub, base: &str) -> (String, String, String, String) {
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor)]);
    hub.flag("team:t", true);
    let project = app.db.team_project("t", "t", 1).unwrap();
    let issue = async |assignees: Value| {
        let path = format!("/api/projects/{project}/issues");
        let (st, i) = call(base, "b", "POST", &path, Some(json!({ "title": "x", "assignees": assignees }))).await;
        assert_eq!(st, 200, "{i}");
        let id = i["id"].as_str().unwrap().to_owned();
        let body = json!({ "kind": "object", "target": id });
        let (st, r) = call(base, "b", "POST", "/api/chat/targets", Some(body)).await;
        assert_eq!(st, 200, "{r}");
        (id, r["id"].as_str().unwrap().to_owned())
    };
    let (mine, mine_room) = issue(json!(["a"])).await;
    let (_, other_room) = issue(json!([])).await;
    (project, mine, mine_room, other_room)
}

fn post(app: &Platform, room: &str, author: &str, agent: Option<&str>, via: Option<&str>, mentions: &[String]) {
    app.db
        .add_chat_message(
            room,
            &NewMessage {
                author,
                at: arugula_e2e::now_ms(),
                text: "words",
                quote: None,
                ref_machine: None,
                ref_pane: None,
                mentions,
                agent,
                via,
                parent: None,
                also: false,
                agent_session: None,
            },
        )
        .unwrap();
}

/// (unread, wants) in `room` as `who` lists it.
async fn counts(base: &str, who: &str, room: &str) -> (u64, u64) {
    let (st, v) = call(base, who, "GET", "/api/chat", None).await;
    assert_eq!(st, 200, "{v}");
    let r = v["rooms"].as_array().unwrap().iter().find(|r| r["id"] == room).unwrap();
    (r["unread"].as_u64().unwrap(), r["wants"].as_u64().unwrap())
}

#[tokio::test]
async fn a_thread_wants_its_reader_only_for_a_mention_or_their_own_issue() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (_, mine, mine_room, other_room) = seed(&app, &hub, &base).await;
    let a = ["a".to_owned()];

    // On an issue that's nobody's: all of it is unread, only a mention wants a.
    post(&app, &other_room, "b", None, None, &[]);
    post(&app, &other_room, "a", Some("geek"), None, &[]);
    assert_eq!(counts(&base, "a", &other_room).await, (2, 0));
    post(&app, &other_room, "b", Some("laptop"), None, &a);
    assert_eq!(counts(&base, "a", &other_room).await, (3, 1));

    // On the issue assigned to a, what others write wants them.
    post(&app, &mine_room, "b", None, None, &[]);
    post(&app, &mine_room, "b", Some("laptop"), None, &[]);
    assert_eq!(counts(&base, "a", &mine_room).await, (2, 2));
    // Their own agent reporting doesn't, unless it mentions them.
    post(&app, &mine_room, "a", Some("geek"), None, &[]);
    assert_eq!(counts(&base, "a", &mine_room).await, (3, 2));
    post(&app, &mine_room, "a", Some("geek"), None, &a);
    assert_eq!(counts(&base, "a", &mine_room).await, (4, 3));
    // Nor does a session's card, or what a session they asked for said;
    // what someone else's session said does.
    let insert = |id: &str, parent: i64, asker: &str| {
        app.db
            .c()
            .execute(
                "INSERT INTO agent_sessions (id, project_id, room_id, parent_seq, asker, workspace_id, branch, created_at, updated_at)
                 VALUES (?1, (SELECT project_id FROM issues WHERE id = ?2), ?3, ?5, ?4, 'hw_x', 'b', 1, 1)",
                params![id, mine, mine_room, asker, parent],
            )
            .unwrap();
    };
    insert("ses_a", 100, "a");
    insert("ses_b", 101, "b");
    post(&app, &mine_room, "", None, Some("session:ses_a"), &[]);
    post(&app, &mine_room, "", None, Some("said:ses_a"), &[]);
    assert_eq!(counts(&base, "a", &mine_room).await, (6, 3));
    post(&app, &mine_room, "", None, Some("said:ses_b"), &[]);
    assert_eq!(counts(&base, "a", &mine_room).await, (7, 4));
    // It's a's issue, not b's: nothing here mentions b.
    assert_eq!(counts(&base, "b", &mine_room).await.1, 0);

    // Closed, it wants them only for its mention.
    let (st, v) = call(&base, "b", "PATCH", &format!("/api/issues/{mine}"), Some(json!({ "state": "closed" }))).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(counts(&base, "a", &mine_room).await, (7, 1));
}
