//! Agents as principals (M197): a machine's grants, given and revoked by
//! its owner through `/api/grants` and asked about on every use: here, a
//! machine reading its owner's native issue. The rig ([`seeded`]) is also
//! the feature crates' (chat's grants to read and post in its owner's
//! rooms are `arugula-platform-chat`'s `agent_grants_tests`).

// Also the feature crates' test harness (#314, the `testing` feature): its
// helpers are `pub`, and only its own tests use the rest.
#![cfg_attr(not(test), allow(dead_code, unused_imports))]

use std::sync::Arc;

use arugula_control_wire::chat::ChatPost;
use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{
    auth::Machine,
    chat_tests::{TestHub, call, serve},
    db::{
        chat::Room,
        issues::{Actor, NewIssue},
        projects::THREADS_READ,
    },
    grants::AGENT_UNGRANTED,
};

fn room(id: &str, kind: &str) -> Room {
    Room {
        id: id.into(),
        scope: "team:t".into(),
        kind: kind.into(),
        name: id.into(),
        machine: None,
        target: None,
        created_by: "a".into(),
        created: 1,
        archived: None,
        last_seq: 0,
    }
}

pub fn post(text: &str) -> ChatPost {
    ChatPost { text: text.into(), ..Default::default() }
}

pub fn give(machine: &str, power: &str, scope: Value) -> Option<Value> {
    Some(json!({ "daemon": machine, "power": power, "scope": scope }))
}

/// a owns team t (b and x in it too) and the machine geek; c alone is in
/// team u. #general is t's, and there's a DM between a and b and one
/// between b and x. Chat is on for t. t's and u's default projects.
pub async fn seeded() -> (Arc<TestHub>, Arc<crate::Platform>, String, String, String) {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    hub.team("u", &[("c", TeamRole::Owner)]);
    hub.flag("team:t", true);
    hub.daemon("geek", "a", Some("t"));
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let t = crate::projects::team_project(&app, "t").await.unwrap().unwrap();
    let u = crate::projects::team_project(&app, "u").await.unwrap().unwrap();
    app.db.add_chat_room(&room("ch_general", "channel"), &["a", "b", "x"], Some(&t)).unwrap();
    app.db.add_chat_room(&room("dm_ab", "dm"), &["a", "b"], None).unwrap();
    app.db.add_chat_room(&room("dm_bx", "dm"), &["b", "x"], None).unwrap();
    (hub, app, base, t, u)
}

pub fn geek() -> Machine {
    Machine { account: "a".into(), daemon: "geek".into() }
}

/// A call signed by a's machine geek.
async fn as_geek(base: &str, path: &str) -> (u16, Value) {
    let r = reqwest::Client::new().get(format!("{base}{path}")).header("x-test-daemon", "a geek").send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

/// A machine reads a native issue for its owner (M203's pinned issue
/// blocks): refused saying how without `threads.read` over the project,
/// then by id or by number; another team's issue isn't there, and a revoke
/// stops the next read.
#[tokio::test]
async fn a_machine_reads_its_owners_issue_with_a_grant() {
    let (_hub, app, base, t, u) = seeded().await;
    let issue = |project: &str| {
        let n = NewIssue { project, title: "the canary fails", body: "on deploy", labels: &[], assignees: &[] };
        app.db.create_issue(&n, Actor::person("a"), 1).unwrap().unwrap()
    };
    let ours = issue(&t);
    let theirs = issue(&u);
    let by_id = format!("/api/daemon/issues/{}", ours.id);
    let (st, v) = as_geek(&base, &by_id).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    assert!(v["error"].as_str().unwrap().contains("threads.read"), "{v}");
    assert_eq!(as_geek(&base, &format!("/api/daemon/issues/{}", theirs.id)).await.0, 404, "u's isn't a's");

    let (st, g) =
        call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");
    let (st, v) = as_geek(&base, &by_id).await;
    assert_eq!((st, &v["title"], &v["number"]), (200, &json!("the canary fails"), &json!(1)), "{v}");
    let (st, v) = as_geek(&base, &format!("/api/daemon/projects/{t}/issues/1")).await;
    assert_eq!((st, &v["id"]), (200, &json!(ours.id)), "{v}");
    assert_eq!(as_geek(&base, &format!("/api/daemon/projects/{u}/issues/1")).await.0, 404);
    assert_eq!(as_geek(&base, "/api/daemon/issues/iss_00000000000000000000000000000000").await.0, 404);
    // Only as a machine: a person's session isn't one.
    assert_eq!(call(&base, "a", "GET", &by_id, None).await.0, 401);

    let (st, _) = call(&base, "a", "POST", &format!("/api/grants/{}/revoke", g["id"].as_str().unwrap()), None).await;
    assert_eq!(st, 200);
    assert_eq!(as_geek(&base, &by_id).await.0, 403, "revoked: the next read is refused");
}
