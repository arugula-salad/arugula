//! Plane's webhooks and the reconcile (Arugula #358), against `FakeTracker`:
//! a signed delivery changes the row and tells the boards, a forged,
//! unsigned or foreign-secret one is a 401 that changes nothing and burns
//! nothing, a redelivery is a no-op, and a reconcile finds what no webhook
//! told.

#![cfg_attr(not(test), allow(dead_code, unused_imports))]

use std::sync::Arc;

use arugula_e2e::{now_ms, team::TeamRole};
use hmac::{KeyInit, Mac};
use serde_json::{Value, json};

use super::{
    connect::{self, Reconciled},
    connect_tests::{KEY, WEBHOOK, backfilled, connect_as, count, serve},
    fake::{FakeTracker, item},
};
use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::{
        Sql as _,
        chat::Room,
        sql::params,
        trackers::{NewConnection, RECONCILE_MS, webhook_name},
    },
    keys,
};

const OTHER: &str = "another-hook-secret"; // gitleaks:allow

fn sign(secret: &str, body: &[u8]) -> String {
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// Post a delivery: `signature` as given (`None`: the header is left off).
async fn post(
    base: &str,
    conn: &str,
    delivery: &str,
    event: &str,
    signature: Option<&str>,
    body: &Value,
) -> (u16, Value) {
    let bytes = serde_json::to_vec(body).unwrap();
    let mut r = reqwest::Client::new()
        .post(format!("{base}/plane/webhook/{conn}"))
        .header("x-plane-delivery", delivery)
        .header("x-plane-event", event)
        .body(bytes);
    if let Some(s) = signature {
        r = r.header("x-plane-signature", s);
    }
    let r = r.send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or_default())
}

/// A signed delivery, from the right secret.
async fn signed(base: &str, conn: &str, delivery: &str, event: &str, body: &Value) -> (u16, Value) {
    let sig = sign(WEBHOOK, &serde_json::to_vec(body).unwrap());
    post(base, conn, delivery, event, Some(&sig), body).await
}

fn updated(project: &str, id: &str) -> Value {
    json!({ "version": "v2", "event": "workitem.updated", "entity_id": id, "entity_type": "issue",
        "data": { "id": id, "project_id": project, "state": "0f0f0f0f-not-a-group" }, "previous_attributes": {} })
}

/// A row as (title, state, state_group, updated_at).
fn row(app: &Platform, mirror: i64, source: &str) -> Option<(String, String, String, u64)> {
    app.db
        .c()
        .query_map(
            "SELECT title, state, state_group, updated_at FROM forge_items WHERE repo_id = ?1 AND source_item_id = ?2",
            params![mirror, source],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
        .pop()
}

struct World {
    fake: FakeTracker,
    app: Arc<Platform>,
    base: String,
    project: String,
    conn: String,
    mirror: i64,
}

/// Team t1 (boards on), alice connected `ARUGU` and it is backfilled.
async fn world(hub: &Arc<TestHub>) -> World {
    let (fake, app, base) = serve(hub).await;
    hub.team("t1", &[("alice", TeamRole::Owner)]);
    hub.flag_named("team:t1", crate::boards::FLAG, true);
    let project = crate::projects::team_project(&app, "t1").await.unwrap().unwrap();
    let (s, made) = connect_as(&base, "alice", &project, KEY, "ARUGU").await;
    assert_eq!(s, 200, "{made}");
    let conn = made["id"].as_str().unwrap().to_owned();
    backfilled(&app, &project, &conn).await;
    let mirror = app.db.tracker_connection(&project, &conn).unwrap().unwrap().mirror_id;
    World { fake, app, base, project, conn, mirror }
}

/// A board with a row pointing at the mirrored item `source`, and a thread
/// on the board so a change to it is heard: the stream of those changes.
async fn board_on(w: &World, source: &str) -> tokio::sync::broadcast::Receiver<Arc<crate::chat::Out>> {
    let (s, b) =
        call(&w.base, "alice", "POST", &format!("/api/projects/{}/boards", w.project), Some(json!({ "title": "B" })))
            .await;
    assert_eq!(s, 200, "{b}");
    let board = b["board"]["id"].as_str().unwrap().to_owned();
    let (s, z) = call(
        &w.base,
        "alice",
        "PUT",
        &format!("/api/boards/{board}/entries"),
        Some(json!({ "entry": "zone", "title": "Now" })),
    )
    .await;
    assert_eq!(s, 200, "{z}");
    let zone = z["zones"][0]["id"].as_str().unwrap().to_owned();
    let fi: String = w
        .app
        .db
        .c()
        .query_row(
            "SELECT id FROM forge_items WHERE repo_id = ?1 AND source_item_id = ?2",
            params![w.mirror, source],
            |r| r.get(0),
        )
        .unwrap();
    let body = json!({ "entry": "row", "zone": zone, "pointer": { "kind": "forge_item", "id": fi } });
    let (s, r) = call(&w.base, "alice", "PUT", &format!("/api/boards/{board}/entries"), Some(body)).await;
    assert_eq!(s, 200, "{r}");
    let room = Room {
        id: "ob_board".into(),
        scope: format!("project:{}", w.project),
        kind: "object".into(),
        name: "B".into(),
        machine: None,
        target: Some(board),
        created_by: "alice".into(),
        created: 1,
        archived: None,
        last_seq: 0,
    };
    w.app.db.add_chat_room(&room, &[], Some(&w.project)).unwrap();
    w.app.chat.subscribe().1
}

/// Whether the board's thread heard of a change.
fn heard(rx: &mut tokio::sync::broadcast::Receiver<Arc<crate::chat::Out>>) -> bool {
    let mut any = false;
    while let Ok(o) = rx.try_recv() {
        any |= matches!(o.ev, arugula_control_wire::chat::ChatLive::Object { .. });
    }
    any
}

/// The main path: a signed `workitem.updated` re-reads the item and rewrites
/// its row (state from the group, not the payload's id), and the board that
/// points at it hears. Forged, unsigned and other-connection signatures are
/// 401s that change nothing and don't burn the delivery; a redelivery, a
/// payload for another project and an event of no interest do nothing; a
/// delete removes the row and tells the board.
#[tokio::test]
async fn a_signed_webhook_updates_the_row_and_tells_the_boards() {
    let hub = Arc::new(TestHub::default());
    let w = world(&hub).await;
    let mut rx = board_on(&w, "item-1").await;
    let was = row(&w.app, w.mirror, "item-1").unwrap();
    assert_eq!((was.0.as_str(), was.1.as_str(), was.2.as_str()), ("First", "open", "backlog"));
    let mut changed = item(1, "First, changed", "completed", 9_000);
    changed.state_name = "Done".into();
    w.fake.set_items(vec![changed, item(2, "Second", "started", 2_000), item(3, "Third", "completed", 1_000)]);
    let body = updated("pl-proj-1", "item-1");
    let bytes = serde_json::to_vec(&body).unwrap();

    // Another connection, with its own secret: its signature isn't this one's.
    let master = w.app.secret_key.as_deref().unwrap();
    let other = NewConnection {
        id: "tc_other".into(),
        project_id: w.project.clone(),
        workspace_slug: "acme".into(),
        source_project_id: "pl-proj-other".into(),
        identifier: "OTHER".into(),
        name: "Other".into(),
        linked_by: "alice".into(),
    };
    w.app.db.connect_tracker(&other, 10).unwrap().unwrap();
    keys::seal(&w.app.db, master, &w.project, &webhook_name("tc_other"), OTHER).unwrap();

    for (what, sig) in [
        ("forged", Some(sign("not-the-secret", &bytes))),
        ("unsigned", None),
        ("another connection's", Some(sign(OTHER, &bytes))),
        ("a body that isn't the one signed", Some(sign(WEBHOOK, b"{}"))),
        ("not hex", Some("zz".to_owned())),
    ] {
        let (s, v) = post(&w.base, &w.conn, "d1", "workitem.updated", sig.as_deref(), &body).await;
        assert_eq!(s, 401, "{what}: {v}");
        assert_eq!(row(&w.app, w.mirror, "item-1").unwrap(), was, "{what} changed the row");
        assert_eq!(w.app.db.delivery_event("plane:d1").unwrap(), None, "{what} burned the delivery");
    }
    assert!(!heard(&mut rx));

    let (s, v) = signed(&w.base, &w.conn, "d1", "workitem.updated", &body).await;
    assert_eq!((s, v["changed"].as_u64()), (200, Some(1)), "{v}");
    assert_eq!(
        row(&w.app, w.mirror, "item-1").unwrap(),
        ("First, changed".into(), "closed".into(), "completed".into(), 9_000)
    );
    assert!(heard(&mut rx), "the board wasn't told");

    // A redelivery: known, and nothing is read or written again.
    let mut again = item(1, "First, again", "completed", 9_500);
    again.state_name = "Done".into();
    w.fake.set_items(vec![again]);
    let (s, v) = signed(&w.base, &w.conn, "d1", "workitem.updated", &body).await;
    assert_eq!((s, &v["duplicate"]), (200, &json!(true)), "{v}");
    assert_eq!(row(&w.app, w.mirror, "item-1").unwrap().0, "First, changed");
    assert!(!heard(&mut rx));

    // Another project's payload, and an event that isn't a work item's.
    let (s, v) = signed(&w.base, &w.conn, "d2", "workitem.updated", &updated("pl-proj-other", "item-1")).await;
    assert_eq!((s, &v["ignored"]), (200, &json!(true)), "{v}");
    let comment =
        json!({ "event": "workitem.comment.created", "entity_id": "item-1", "data": { "project_id": "pl-proj-1" } });
    let (s, v) = signed(&w.base, &w.conn, "d3", "workitem.comment.created", &comment).await;
    assert_eq!((s, &v["ignored"]), (200, &json!(true)), "{v}");
    assert_eq!(row(&w.app, w.mirror, "item-1").unwrap().0, "First, changed");

    // A delete has no data: the project is in its previous attributes.
    let gone = json!({ "event": "workitem.deleted", "entity_id": "item-1", "data": {},
        "previous_attributes": { "id": "item-1", "project_id": "pl-proj-1" } });
    let (s, v) = signed(&w.base, &w.conn, "d4", "workitem.deleted", &gone).await;
    assert_eq!((s, v["changed"].as_u64()), (200, Some(1)), "{v}");
    assert_eq!(row(&w.app, w.mirror, "item-1"), None);
    assert!(heard(&mut rx), "the board wasn't told of the delete");
    assert_eq!(count(&w.app, "forge_items", "repo_id = ?1", params![w.mirror]), 2);
}

/// A connection that isn't there, is disconnected, kept no secret, or whose
/// boards are off is a 404 whatever the signature; a delivery that can't be
/// applied is forgotten so Plane's retry is taken.
#[tokio::test]
async fn a_webhook_for_no_connection_is_a_404_and_a_failed_one_is_retried() {
    let hub = Arc::new(TestHub::default());
    let w = world(&hub).await;
    let body = updated("pl-proj-1", "item-1");
    let (s, _) = signed(&w.base, "tc_nobody", "d1", "workitem.updated", &body).await;
    assert_eq!(s, 404);

    // The key Plane refuses: 502, the delivery forgotten, then taken.
    let master = w.app.secret_key.as_deref().unwrap();
    keys::seal(&w.app.db, master, &w.project, &super::super::db::trackers::key_name(&w.conn), "some-revoked-key")
        .unwrap();
    w.fake.set_items(vec![item(1, "First, changed", "started", 9_000)]);
    let (s, v) = signed(&w.base, &w.conn, "d1", "workitem.updated", &body).await;
    assert_eq!(s, 502, "{v}");
    assert_eq!(w.app.db.delivery_event("plane:d1").unwrap(), None, "a failed delivery stays known");
    keys::seal(&w.app.db, master, &w.project, &super::super::db::trackers::key_name(&w.conn), KEY).unwrap();
    let (s, v) = signed(&w.base, &w.conn, "d1", "workitem.updated", &body).await;
    assert_eq!((s, v["changed"].as_u64()), (200, Some(1)), "{v}");
    assert_eq!(row(&w.app, w.mirror, "item-1").unwrap().0, "First, changed");

    // Boards off for whoever connected it.
    hub.flag_named("team:t1", crate::boards::FLAG, false);
    w.app.cache.drop_all();
    let (s, _) = signed(&w.base, &w.conn, "d5", "workitem.updated", &body).await;
    assert_eq!(s, 404);
    hub.flag_named("team:t1", crate::boards::FLAG, true);
    w.app.cache.drop_all();

    // No secret kept: the same 404.
    keys::remove(&w.app.db, &w.project, &webhook_name(&w.conn)).unwrap();
    let (s, _) = signed(&w.base, &w.conn, "d6", "workitem.updated", &body).await;
    assert_eq!(s, 404);

    // Disconnected.
    keys::seal(&w.app.db, master, &w.project, &webhook_name(&w.conn), WEBHOOK).unwrap();
    let (s, v) = signed(&w.base, &w.conn, "d7", "workitem.updated", &body).await;
    assert_eq!(s, 200, "{v}");
    w.app.db.disconnect_tracker(&w.project, &w.conn, now_ms()).unwrap().unwrap();
    let (s, _) = signed(&w.base, &w.conn, "d8", "workitem.updated", &body).await;
    assert_eq!(s, 404);
}

/// A change that had no webhook is picked up by the reconcile (from the
/// newest held, less a minute), the board that points at it hears, it isn't
/// due again for five minutes, and a key Plane refuses waits the back-off.
#[tokio::test]
async fn a_reconcile_picks_up_a_change_with_no_webhook() {
    let hub = Arc::new(TestHub::default());
    let w = world(&hub).await;
    let mut rx = board_on(&w, "item-2").await;
    // The newest held is far ahead of the fake's old items, so the minute
    // of overlap matters: item 3 (changed at 8_000) is before it.
    let far = item(9, "Nine", "started", 1_000_000);
    connect::ingest_page(&w.app, &w.conn, w.mirror, None, std::slice::from_ref(&far), 1).unwrap().unwrap();
    heard(&mut rx);
    let mut two = item(2, "Second, changed", "completed", 1_000_500);
    two.state_name = "Done".into();
    w.fake.set_items(vec![
        far,
        two,
        item(3, "Third, changed", "completed", 8_000),
        item(4, "Fourth", "backlog", 1_000_700),
    ]);
    // Not due yet.
    assert_eq!(connect::reconcile(&w.app, now_ms()).await.unwrap(), None);
    let due = now_ms() + RECONCILE_MS + 1;
    assert_eq!(
        connect::reconcile(&w.app, due).await.unwrap(),
        Some(Reconciled::Done { connection: w.conn.clone(), changed: 2 })
    );
    assert_eq!(row(&w.app, w.mirror, "item-2").unwrap().1, "closed");
    assert_eq!(row(&w.app, w.mirror, "item-4").unwrap().0, "Fourth");
    assert_eq!(row(&w.app, w.mirror, "item-3").unwrap().0, "Third", "read from before the overlap");
    assert!(heard(&mut rx), "the board wasn't told");
    assert_eq!(connect::reconcile(&w.app, now_ms()).await.unwrap(), None, "done for five minutes");

    // A key Plane refuses: the turn fails and the connection waits.
    let master = w.app.secret_key.as_deref().unwrap();
    keys::seal(&w.app.db, master, &w.project, &super::super::db::trackers::key_name(&w.conn), "some-revoked-key")
        .unwrap();
    let later = due + RECONCILE_MS * 2;
    assert_eq!(
        connect::reconcile(&w.app, later).await.unwrap(),
        Some(Reconciled::Failed { connection: w.conn.clone() })
    );
    assert!(w.app.db.claim_reconcile(later + connect::BACKOFF_MS - 1).unwrap().is_none());
    assert!(w.app.db.claim_reconcile(later + connect::BACKOFF_MS).unwrap().is_some());
}
