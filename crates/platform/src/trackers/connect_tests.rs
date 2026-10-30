//! Connecting a Plane project (Arugula #356) through the platform's own
//! routes, against `FakeTracker`: who may connect, a bad key, the backfill
//! into `forge_items`, that no key comes back out, and the disconnect and
//! its purge. Runs on SQLite and, with `ARUGULA_TEST_POSTGRES_URL`,
//! Postgres.

#![cfg_attr(not(test), allow(dead_code, unused_imports))]

use std::{sync::Arc, time::Duration};

use arugula_control_wire::projects::ConnectTracker;
use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use super::{
    ProjectInfo,
    connect::{self, Turn},
    fake::{FakeTracker, FakeTrackers, item},
};
use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::{
        Sql as _,
        sql::{Param, params},
        trackers::NewConnection,
    },
    secrets::SecretCipher,
};

const KEY: &str = "plane-key-for-the-test"; // gitleaks:allow
const WEBHOOK: &str = "plane-hook-for-the-test"; // gitleaks:allow
const MASTER: &str = "0303030303030303030303030303030303030303030303030303030303030303"; // gitleaks:allow

/// The platform over `hub` with a fake Plane (workspace `acme`, project
/// `ARUGU` with three items, one completed), and its address.
async fn serve(hub: &Arc<TestHub>) -> (FakeTracker, Arc<Platform>, String) {
    let project = ProjectInfo { id: "pl-proj-1".into(), identifier: "ARUGU".into(), name: "Arugula".into() };
    let items = vec![
        item(1, "First", "backlog", 3_000),
        item(2, "Second", "started", 2_000),
        item(3, "Third", "completed", 1_000),
    ];
    let fake = FakeTracker::new(KEY, "acme", project, items);
    let cfg = crate::Config {
        public_url: "https://control.example".into(),
        page_url: "https://app.example".into(),
        ..Default::default()
    };
    let mut p = Platform::new(cfg, crate::db::Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    p.secret_key = Some(Arc::new(SecretCipher::new(MASTER).unwrap()));
    p.trackers = Arc::new(FakeTrackers(fake.clone()));
    let app = Arc::new(p);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (fake, app, base)
}

fn count(app: &Platform, table: &str, filter: &str, p: &[&dyn Param]) -> u64 {
    app.db.c().query_row(&format!("SELECT count(*) FROM {table} WHERE {filter}"), p, |r| r.get(0)).unwrap()
}

fn secrets(app: &Platform) -> u64 {
    count(app, "project_secrets", "name LIKE 'tracker/plane/%'", params![])
}

async fn connect_as(base: &str, who: &str, project: &str, key: &str, what: &str) -> (u16, Value) {
    let body = json!({ "workspace_slug": "acme", "project": what, "api_key": key, "webhook_secret": WEBHOOK });
    call(base, who, "POST", &format!("/api/projects/{project}/trackers"), Some(body)).await
}

/// Until the backfill, which the connect started, has finished.
async fn backfilled(app: &Platform, project: &str, id: &str) {
    for _ in 0..200 {
        if app.db.tracker_connection(project, id).unwrap().is_some_and(|c| c.backfilled_at.is_some()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the backfill didn't finish");
}

/// An editor connects `ARUGU` and its three items are mirrored; a viewer is
/// refused, a bad key or an unknown project is a 422 that keeps nothing,
/// and a Plane project connects to one project. No answer carries the key
/// or the webhook secret, and neither is stored as it is. Disconnecting
/// removes the secrets at once and the purge removes the rows after the
/// grace period.
#[tokio::test]
async fn an_editor_connects_and_the_items_are_mirrored() {
    let hub = Arc::new(TestHub::default());
    let (fake, app, base) = serve(&hub).await;
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("carol", TeamRole::Viewer)]);
    hub.flag_named("team:t1", crate::boards::FLAG, true);
    hub.team("t3", &[("erin", TeamRole::Owner)]);
    hub.flag_named("team:t3", crate::boards::FLAG, true);
    let erin = crate::projects::team_project(&app, "t3").await.unwrap().unwrap();
    let team = crate::projects::team_project(&app, "t1").await.unwrap().unwrap();

    // Boards are Labs, and so is this: on for t1, off for t0, where every
    // route is a 404.
    hub.team("t0", &[("dave", TeamRole::Owner)]);
    let off = crate::projects::team_project(&app, "t0").await.unwrap().unwrap();
    let (s, _) = connect_as(&base, "dave", &off, KEY, "ARUGU").await;
    assert_eq!(s, 404);
    let (s, _) = call(&base, "dave", "GET", &format!("/api/projects/{off}/trackers"), None).await;
    assert_eq!(s, 404);
    assert_eq!(count(&app, "tracker_connections", "TRUE", params![]), 0);

    let (s, v) = connect_as(&base, "carol", &team, KEY, "ARUGU").await;
    assert_eq!(s, 403, "{v}");
    let (s, v) = connect_as(&base, "bob", &team, "plane-some-other-key", "ARUGU").await;
    assert_eq!(s, 422, "{v}");
    assert_eq!(v["error"], "Plane refused that API key");
    assert_eq!(fake.refused(), 1);
    let (s, v) = connect_as(&base, "bob", &team, KEY, "NOPE").await;
    assert_eq!(s, 422, "{v}");
    assert_eq!(count(&app, "tracker_connections", "TRUE", params![]), 0);
    assert_eq!(secrets(&app), 0);

    // By its identifier, as an editor.
    let (s, made) = connect_as(&base, "bob", &team, KEY, "ARUGU").await;
    assert_eq!(s, 200, "{made}");
    let id = made["id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("tc_"), "{made}");
    assert_eq!(
        (made["identifier"].as_str(), made["name"].as_str(), made["agent_writes"].as_str()),
        (Some("ARUGU"), Some("Arugula"), Some("approve"))
    );
    assert_eq!(made["has_webhook_secret"], true);
    assert_eq!(secrets(&app), 2);
    // The same Plane project, again, while it's connected.
    let (s, v) = connect_as(&base, "alice", &team, KEY, "pl-proj-1").await;
    assert_eq!(s, 409, "{v}");
    assert_eq!(secrets(&app), 2, "the refused connect kept nothing");

    backfilled(&app, &team, &id).await;
    let mirror = app.db.tracker_connection(&team, &id).unwrap().unwrap().mirror_id;
    assert!(mirror < 0);
    let rows: Vec<(u64, String, String, String, String)> = app
        .db
        .c()
        .query_map(
            "SELECT number, state, state_group, provider, source_item_id FROM forge_items WHERE repo_id = ?1 ORDER BY number",
            params![mirror],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    let want = |n: u64, state: &str, group: &str| {
        (n, state.to_owned(), group.to_owned(), "plane".to_owned(), format!("item-{n}"))
    };
    assert_eq!(rows, [want(1, "open", "backlog"), want(2, "open", "started"), want(3, "closed", "completed")]);
    // Nothing GitHub's readers join on sees them.
    let joined = "forge_items i JOIN linked_repos l ON l.repo_id = i.repo_id";
    assert_eq!(count(&app, joined, "TRUE", params![]), 0);

    // GitHub's item route doesn't know a tracker's item: not found, not an error.
    let item_id: String = app
        .db
        .c()
        .query_row("SELECT id FROM forge_items WHERE repo_id = ?1 LIMIT 1", params![mirror], |r| r.get(0))
        .unwrap();
    let (s, _) = call(&base, "bob", "GET", &format!("/api/forge/items/{item_id}"), None).await;
    assert_eq!(s, 404);

    // The key never comes back out, to an editor, an owner or a viewer.
    for who in ["bob", "alice", "carol"] {
        let (s, listed) = call(&base, who, "GET", &format!("/api/projects/{team}/trackers"), None).await;
        assert_eq!(s, 200, "{listed}");
        let body = listed.to_string();
        assert!(!body.contains(KEY) && !body.contains(WEBHOOK), "{who} was given a secret: {body}");
        assert_eq!(listed["connections"][0]["items"], 3);
        assert_eq!(listed["connections"][0]["has_webhook_secret"], true);
    }
    assert!(!made.to_string().contains(KEY));
    // Sealed: nowhere in the database as text.
    assert!(app.db.mentions(KEY).is_empty() && app.db.mentions(WEBHOOK).is_empty());

    // Agent writes: owners and editors choose.
    let path = format!("/api/projects/{team}/trackers/{id}");
    let (s, _) = call(&base, "carol", "PATCH", &path, Some(json!({ "agent_writes": "allow" }))).await;
    assert_eq!(s, 403);
    let (s, _) = call(&base, "bob", "PATCH", &path, Some(json!({ "agent_writes": "whenever" }))).await;
    assert_eq!(s, 422);
    let (s, v) = call(&base, "bob", "PATCH", &path, Some(json!({ "agent_writes": "allow" }))).await;
    assert_eq!((s, v["agent_writes"].as_str()), (200, Some("allow")), "{v}");

    // Disconnecting: a viewer can't. The secrets go now, the rows at the purge.
    let (s, _) = call(&base, "carol", "DELETE", &path, None).await;
    assert_eq!(s, 403);
    assert_eq!(secrets(&app), 2);
    let (s, gone) = call(&base, "bob", "DELETE", &path, None).await;
    assert_eq!(s, 200, "{gone}");
    assert!(gone["unlinked_at"].as_u64().is_some() && gone["purge_after"].as_u64().is_some());
    assert_eq!(secrets(&app), 0);
    assert_eq!(count(&app, "forge_items", "repo_id = ?1", params![mirror]), 3);
    let due = gone["purge_after"].as_u64().unwrap();
    assert_eq!(connect::purge(&app, due - 1).unwrap(), 0);
    assert_eq!(count(&app, "forge_items", "repo_id = ?1", params![mirror]), 3);
    assert_eq!(connect::purge(&app, due).unwrap(), 1);
    assert_eq!(count(&app, "forge_items", "repo_id = ?1", params![mirror]), 0);
    assert_eq!(count(&app, "tracker_connections", "TRUE", params![]), 0);
    // Boards turned off after connecting: settings and disconnect are 404 too.
    let (s, again) = connect_as(&base, "erin", &erin, KEY, "ARUGU").await;
    assert_eq!(s, 200, "{again}");
    let path = format!("/api/projects/{erin}/trackers/{}", again["id"].as_str().unwrap());
    hub.flag_named("team:t3", crate::boards::FLAG, false);
    app.cache.drop_all();
    let (s, _) = call(&base, "erin", "PATCH", &path, Some(json!({ "agent_writes": "allow" }))).await;
    assert_eq!(s, 404);
    let (s, _) = call(&base, "erin", "DELETE", &path, None).await;
    assert_eq!(s, 404);
    assert_eq!(count(&app, "tracker_connections", "unlinked_at IS NULL", params![]), 1);
    app.db.disconnect_tracker(&erin, again["id"].as_str().unwrap(), 1).unwrap().unwrap();

    // Connected again, it's a new connection under the next mirror id.
    let (s, again) = connect_as(&base, "bob", &team, KEY, "ARUGU").await;
    assert_eq!(s, 200, "{again}");
}

/// The backfill's claim: one machine at a time, pages carried on from a
/// cursor, and nothing written once the connection is disconnected.
#[tokio::test]
async fn the_backfill_takes_turns_and_stops_at_a_disconnect() {
    let hub = Arc::new(TestHub::default());
    let (_, app, _) = serve(&hub).await;
    let new = |id: &str| NewConnection {
        id: id.into(),
        project_id: "prj_a".into(),
        workspace_slug: "acme".into(),
        source_project_id: "pl-proj-1".into(),
        identifier: "ARUGU".into(),
        name: "Arugula".into(),
        linked_by: "alice".into(),
    };
    let c = app.db.connect_tracker(&new("tc_a"), 10).unwrap().expect("connected");
    assert!(c.mirror_id < 0);
    assert_eq!(app.db.connect_tracker(&new("tc_b"), 11).unwrap(), None);

    let job = app.db.claim_backfill(100).unwrap().unwrap();
    assert!(app.db.claim_backfill(101).unwrap().is_none(), "a lease holds it");
    // Written under the lease; not under another's.
    let items = [item(9, "Nine", "started", 5)];
    assert_eq!(connect::ingest_page(&app, "tc_a", c.mirror_id, Some(&job.lease), &items, 100).unwrap(), Some(1));
    assert_eq!(connect::ingest_page(&app, "tc_a", c.mirror_id, Some("not-the-lease"), &items, 100).unwrap(), None);
    // An older answer doesn't overwrite a newer row.
    let older = [item(9, "Nine, before", "started", 4)];
    assert_eq!(connect::ingest_page(&app, "tc_a", c.mirror_id, None, &older, 101).unwrap(), Some(0));
    let newer = [item(9, "Nine, after", "completed", 6)];
    assert_eq!(connect::ingest_page(&app, "tc_a", c.mirror_id, None, &newer, 102).unwrap(), Some(1));
    assert_eq!(
        count(&app, "forge_items", "repo_id = ?1 AND state = 'closed' AND title = 'Nine, after'", params![c.mirror_id]),
        1
    );

    app.db.disconnect_tracker("prj_a", "tc_a", 200).unwrap().unwrap();
    assert_eq!(connect::ingest_page(&app, "tc_a", c.mirror_id, None, &newer, 201).unwrap(), None);
    assert!(app.db.claim_backfill(300).unwrap().is_none(), "disconnected: not backfilled");
}

/// A turn that can't read (the connection's key isn't kept) fails, and the
/// connection waits its back-off before another machine claims it.
#[tokio::test]
async fn a_failed_turn_waits_its_backoff() {
    let hub = Arc::new(TestHub::default());
    let (_, app, _) = serve(&hub).await;
    let new = NewConnection {
        id: "tc_a".into(),
        project_id: "prj_a".into(),
        workspace_slug: "acme".into(),
        source_project_id: "pl-proj-1".into(),
        identifier: "ARUGU".into(),
        name: "Arugula".into(),
        linked_by: "alice".into(),
    };
    app.db.connect_tracker(&new, 10).unwrap();
    let turn = connect::tick(&app, 1_000).await.unwrap();
    assert_eq!(turn, Some(Turn::Failed { connection: "tc_a".into() }));
    assert!(app.db.claim_backfill(1_000 + connect::BACKOFF_MS - 1).unwrap().is_none());
    assert!(app.db.claim_backfill(1_000 + connect::BACKOFF_MS).unwrap().is_some());
}

/// The wire type's `Debug` hides the key.
#[test]
fn the_connect_body_hides_its_secrets_in_debug() {
    let b = ConnectTracker {
        workspace_slug: "acme".into(),
        project: "ARUGU".into(),
        api_key: KEY.into(),
        webhook_secret: Some(WEBHOOK.into()),
    };
    let shown = format!("{b:?}");
    assert!(!shown.contains(KEY) && !shown.contains(WEBHOOK), "{shown}");
}
