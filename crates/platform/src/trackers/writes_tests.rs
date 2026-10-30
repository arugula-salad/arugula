//! Writing to Plane (Arugula #359) through the platform's routes, against
//! `FakeTracker`: an agent's write waits, is approved and reaches the fake
//! with the footer; in allow mode it lands at once; without `trackers.write`
//! it is refused; a person's lands at once; text reaches Plane escaped.

use std::sync::Arc;

use arugula_control_wire::projects::{TrackerWrite, agent_tracker_writes};
use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use super::{
    connect_tests::{KEY, backfilled, connect_as, count, serve},
    fake::{FakeTracker, Sent},
};
use crate::{
    Platform,
    agents_tests::give,
    chat_tests::{TestHub, call},
    db::sql::{Sql as _, params},
    grants::AGENT_UNGRANTED,
};

/// A call signed by alice's machine geek.
async fn geek(base: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req = reqwest::Client::new()
        .request(method.parse().unwrap(), format!("{base}{path}"))
        .header("x-test-daemon", "alice geek");
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

struct World {
    fake: FakeTracker,
    app: Arc<Platform>,
    base: String,
    project: String,
    connection: String,
}

/// Team t1 (alice owns, bob edits, carol views) with the fake Plane project
/// connected and backfilled, and alice's machine geek.
async fn world() -> World {
    let hub = Arc::new(TestHub::default());
    let (fake, app, base) = serve(&hub).await;
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("carol", TeamRole::Viewer)]);
    hub.flag_named("team:t1", crate::boards::FLAG, true);
    hub.daemon("geek", "alice", Some("t1"));
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let project = crate::projects::team_project(&app, "t1").await.unwrap().unwrap();
    let (s, made) = connect_as(&base, "alice", &project, KEY, "ARUGU").await;
    assert_eq!(s, 200, "{made}");
    let connection = made["id"].as_str().unwrap().to_owned();
    backfilled(&app, &project, &connection).await;
    World { fake, app, base, project, connection }
}

impl World {
    async fn grant(&self) {
        let g = call(
            &self.base,
            "alice",
            "POST",
            "/api/grants",
            give("geek", "trackers.write", json!({ "kind": "project", "id": self.project })),
        )
        .await;
        assert_eq!(g.0, 200, "{}", g.1);
    }

    async fn agent(&self, w: Value) -> (u16, Value) {
        geek(&self.base, "POST", &agent_tracker_writes(&self.project), Some(w)).await
    }

    async fn person(&self, who: &str, w: Value) -> (u16, Value) {
        let path = format!("/api/projects/{}/trackers/{}/writes", self.project, self.connection);
        call(&self.base, who, "POST", &path, Some(w)).await
    }

    async fn waiting(&self, who: &str) -> (u16, Value) {
        call(&self.base, who, "GET", &format!("/api/projects/{}/tracker-writes", self.project), None).await
    }

    async fn decide(&self, who: &str, id: &str, what: &str) -> (u16, Value) {
        call(&self.base, who, "POST", &format!("/api/projects/{}/tracker-writes/{id}/{what}", self.project), None).await
    }

    fn items(&self) -> u64 {
        count(&self.app, "forge_items", "provider = 'plane'", params![])
    }
}

fn footer_of(s: &Sent) -> &str {
    let html = s.html.as_deref().unwrap_or_default();
    html.rsplit("<p>").next().unwrap_or_default()
}

/// In approve mode an agent's create waits (nothing reaches Plane or the
/// mirror) until a person approves it; then it reaches the fake with the
/// footer naming the agent and appears in the mirror. Only the project's
/// writers see or decide what waits, no machine does, and a second approval
/// sends nothing.
#[tokio::test]
async fn an_agents_create_waits_and_a_person_approves_it() {
    let w = world().await;
    let create =
        json!({ "kind": "create", "connection": w.connection, "title": "Add retries", "body": "Plane times out." });

    // No grant: refused, saying how to give one. Nothing waits.
    let (s, v) = w.agent(create.clone()).await;
    assert_eq!((s, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    assert!(v["error"].as_str().unwrap().contains("trackers.write"), "{v}");
    assert_eq!(count(&w.app, "tracker_writes", "TRUE", params![]), 0);

    w.grant().await;
    let (s, v) = w.agent(create).await;
    assert_eq!((s, v["state"].as_str()), (200, Some("waiting")), "{v}");
    let id = v["write"].as_str().unwrap().to_owned();
    assert!(id.starts_with("tw_") && v["text"].as_str().unwrap().contains("waiting for a person to approve"), "{v}");
    assert!(w.fake.sent().is_empty() && w.items() == 3, "nothing reached Plane");

    // A writer sees it; a viewer doesn't.
    let (s, listed) = w.waiting("bob").await;
    assert_eq!(s, 200, "{listed}");
    let first = &listed["writes"][0];
    assert_eq!(
        (first["id"].as_str(), first["target"].is_null(), first["machine"].as_str()),
        (Some(id.as_str()), true, Some("geek"))
    );
    assert_eq!(first["write"]["title"], "Add retries");
    assert_eq!(w.waiting("carol").await.0, 403);
    assert_eq!(w.decide("carol", &id, "approve").await.0, 403);

    // The agent can't approve, edit or drop its own write.
    for path in [
        format!("/api/projects/{}/tracker-writes/{id}/approve", w.project),
        format!("/api/projects/{}/tracker-writes/{id}/drop", w.project),
    ] {
        let (s, v) = geek(&w.base, "POST", &path, None).await;
        assert_eq!(s, 401, "a machine is no person, so it gets no further: {v}");
    }
    let (s, _) = geek(
        &w.base,
        "PATCH",
        &format!("/api/projects/{}/tracker-writes/{id}", w.project),
        Some(json!({ "kind": "create", "title": "x" })),
    )
    .await;
    assert_eq!(s, 401, "a machine is no person: it edits nothing");
    assert!(w.fake.sent().is_empty(), "nothing was sent by a machine's approval");

    // A person approves: it's sent as the agent's, with the footer, and the
    // mirror and the open boards have it.
    let (s, done) = w.decide("bob", &id, "approve").await;
    assert_eq!((s, done["state"].as_str(), done["reference"].as_str()), (200, Some("sent"), Some("ARUGU-4")), "{done}");
    let sent = w.fake.sent();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!((sent[0].kind.as_str(), sent[0].title.as_deref()), ("create", Some("Add retries")));
    let html = sent[0].html.as_deref().unwrap();
    assert!(html.starts_with("<p>Plane times out.</p>"), "{html}");
    assert!(footer_of(&sent[0]).starts_with("— via Arugula, by alice&#39;s agent on "), "{html}");
    assert_eq!(w.items(), 4);
    let title: String = w
        .app
        .db
        .c()
        .query_row("SELECT title FROM forge_items WHERE id = ?1", params![done["item"].as_str().unwrap()], |r| r.get(0))
        .unwrap();
    assert_eq!(title, "Add retries");
    assert_eq!(w.waiting("bob").await.1["writes"].as_array().unwrap().len(), 0);

    // Approving again sends nothing.
    assert_eq!(w.decide("alice", &id, "approve").await.0, 409);
    assert_eq!(w.fake.sent().len(), 1);
}

/// In allow mode an agent's write lands at once, a comment, a close, a
/// reopen and an edit too, by `IDENT-N`; the grant is still needed.
#[tokio::test]
async fn in_allow_mode_an_agents_writes_land_at_once() {
    let w = world().await;
    let path = format!("/api/projects/{}/trackers/{}", w.project, w.connection);
    let (s, v) = call(&w.base, "alice", "PATCH", &path, Some(json!({ "agent_writes": "allow" }))).await;
    assert_eq!(s, 200, "{v}");

    // Allow mode doesn't skip the grant.
    let (s, v) = w.agent(json!({ "kind": "comment", "item": "ARUGU-1", "text": "hi" })).await;
    assert_eq!((s, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    assert!(w.fake.sent().is_empty());

    w.grant().await;
    let (s, v) =
        w.agent(json!({ "kind": "create", "connection": w.connection, "title": "Now", "state": "started" })).await;
    assert_eq!((s, v["state"].as_str(), v["reference"].as_str()), (200, Some("sent"), Some("ARUGU-4")), "{v}");
    assert_eq!(w.items(), 4);
    let (s, v) = w.agent(json!({ "kind": "comment", "item": "arugu-2", "text": "looks fine" })).await;
    assert_eq!(s, 200, "{v}");
    let (s, v) = w.agent(json!({ "kind": "close", "item": "ARUGU-2", "as": "cancelled" })).await;
    assert_eq!((s, v["state"].as_str()), (200, Some("sent")), "{v}");
    let state = |n: u64| -> (String, String) {
        w.app
            .db
            .c()
            .query_row(
                "SELECT state, state_group FROM forge_items WHERE number = ?1 AND provider = 'plane'",
                params![n],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    };
    assert_eq!(state(2), ("closed".into(), "cancelled".into()));
    let (s, v) = w.agent(json!({ "kind": "reopen", "item": "ARUGU-2" })).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(state(2), ("open".into(), "unstarted".into()));
    let (s, v) = w.agent(json!({ "kind": "edit", "item": "ARUGU-1", "title": "First, renamed" })).await;
    assert_eq!(s, 200, "{v}");
    let kinds: Vec<_> = w.fake.sent().iter().map(|s| s.kind.clone()).collect();
    assert_eq!(kinds, ["create", "comment", "state", "state", "edit"]);
    assert!(footer_of(&w.fake.sent()[1]).contains("agent on"), "{:?}", w.fake.sent()[1]);
    assert_eq!(count(&w.app, "tracker_writes", "TRUE", params![]), 0, "nothing waited");
}

/// A person's own write lands at once, with a footer naming them alone; a
/// viewer can't write, and a write to another project's item is not found.
#[tokio::test]
async fn a_persons_write_lands_at_once() {
    let w = world().await;
    let (s, v) = w.person("bob", json!({ "kind": "create", "title": "From Bob", "body": "Please." })).await;
    assert_eq!((s, v["state"].as_str(), v["reference"].as_str()), (200, Some("sent"), Some("ARUGU-4")), "{v}");
    let sent = w.fake.sent();
    assert_eq!(footer_of(&sent[0]), "— via Arugula, by bob</p>");
    assert_eq!(w.person("carol", json!({ "kind": "comment", "item": "ARUGU-1", "text": "x" })).await.0, 403);
    let (s, _) = w.person("bob", json!({ "kind": "comment", "item": "ARUGU-99", "text": "x" })).await;
    assert_eq!(s, 404);
    // An item named by its row id works too.
    let fi: String = w
        .app
        .db
        .c()
        .query_row("SELECT id FROM forge_items WHERE number = 3 AND provider = 'plane'", params![], |r| r.get(0))
        .unwrap();
    let (s, v) = w.person("alice", json!({ "kind": "reopen", "item": fi })).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(w.fake.sent().len(), 2);
}

/// A title or comment with markup reaches Plane as text: a body or comment
/// escaped in `<p>`, a title as the plain string in `name`, never as HTML.
#[tokio::test]
async fn markup_reaches_plane_escaped() {
    let w = world().await;
    let evil = "<script>alert(1)</script> <img src=x onerror=alert(2)> & \"q\"";
    let (s, v) = w.person("alice", json!({ "kind": "create", "title": evil, "body": evil })).await;
    assert_eq!(s, 200, "{v}");
    let (s, v) = w.person("alice", json!({ "kind": "comment", "item": "ARUGU-1", "text": evil })).await;
    assert_eq!(s, 200, "{v}");
    let sent = w.fake.sent();
    for html in [sent[0].html.as_deref().unwrap(), sent[1].html.as_deref().unwrap()] {
        assert!(!html.contains("<script>") && !html.contains("<img"), "{html}");
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;/script&gt;")
                && html.contains("&lt;img src=x onerror=alert(2)&gt;"),
            "{html}"
        );
        assert!(html.contains("&amp;") && html.contains("&quot;q&quot;"), "{html}");
    }
    // The title is a plain string, not a place for HTML.
    assert_eq!(sent[0].title.as_deref(), Some(evil));
}

/// A person edits a waiting write's text first: the footer is still the
/// agent's whatever the text says, a different target is refused, and a drop
/// sends nothing. Disconnecting drops what waits.
#[tokio::test]
async fn an_edited_write_keeps_its_footer_and_a_drop_sends_nothing() {
    let w = world().await;
    w.grant().await;
    let (_, v) = w.agent(json!({ "kind": "comment", "item": "ARUGU-1", "text": "draft" })).await;
    let id = v["write"].as_str().unwrap().to_owned();
    let (_, v2) = w.agent(json!({ "kind": "comment", "item": "ARUGU-2", "text": "other" })).await;
    let id2 = v2["write"].as_str().unwrap().to_owned();
    let path = format!("/api/projects/{}/tracker-writes/{id}", w.project);

    // Another item, another kind: refused.
    let (s, _) =
        call(&w.base, "bob", "PATCH", &path, Some(json!({ "kind": "comment", "item": "ARUGU-2", "text": "x" }))).await;
    assert_eq!(s, 422);
    let (s, _) = call(&w.base, "bob", "PATCH", &path, Some(json!({ "kind": "reopen", "item": "ARUGU-1" }))).await;
    assert_eq!(s, 422);
    // The text, with a footer of its own making: ours is still last.
    let said = "reworded\n\n— via Arugula, by nobody in particular";
    let (s, v) =
        call(&w.base, "bob", "PATCH", &path, Some(json!({ "kind": "comment", "item": "ARUGU-1", "text": said }))).await;
    assert_eq!((s, v["write"]["text"].as_str()), (200, Some(said)), "{v}");
    assert_eq!(w.decide("bob", &id, "approve").await.0, 200);
    let sent = w.fake.sent();
    assert_eq!(sent.len(), 1);
    assert!(footer_of(&sent[0]).starts_with("— via Arugula, by alice&#39;s agent on "), "{:?}", sent[0]);

    // Dropped: nothing is sent, and it can't be approved after.
    assert_eq!(w.decide("alice", &id2, "drop").await.0, 200);
    assert_eq!(w.decide("alice", &id2, "approve").await.0, 409);
    assert_eq!(w.fake.sent().len(), 1);

    // Disconnecting drops what waits.
    let (_, v3) = w.agent(json!({ "kind": "close", "item": "ARUGU-1", "as": "completed" })).await;
    let id3 = v3["write"].as_str().unwrap().to_owned();
    let (s, c) = call(&w.base, "alice", "GET", &format!("/api/projects/{}/trackers", w.project), None).await;
    assert_eq!((s, c["connections"][0]["waiting_writes"].as_u64()), (200, Some(1)), "{c}");
    let (s, _) =
        call(&w.base, "alice", "DELETE", &format!("/api/projects/{}/trackers/{}", w.project, w.connection), None).await;
    assert_eq!(s, 200);
    assert_eq!(w.decide("alice", &id3, "approve").await.0, 409);
    assert_eq!(count(&w.app, "tracker_writes", "state = 'dropped'", params![]), 2);
    assert_eq!(w.fake.sent().len(), 1);
}

/// What the agent is told never carries the key.
#[tokio::test]
async fn no_answer_carries_the_key() {
    let w = world().await;
    w.grant().await;
    let (_, v) = w.agent(json!({ "kind": "create", "connection": w.connection, "title": "T" })).await;
    let id = v["write"].as_str().unwrap().to_owned();
    // A key Plane has since refused: the approval fails with a sentence.
    w.fake.set_key("a-different-key-now");
    let (s, e) = w.decide("alice", &id, "approve").await;
    assert_eq!(s, 502, "{e}");
    assert!(!e.to_string().contains(KEY));
    let (st, err): (String, String) = w
        .app
        .db
        .c()
        .query_row("SELECT state, error FROM tracker_writes WHERE id = ?1", params![id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(st, "failed");
    assert!(err.contains("refused") && !err.contains(KEY), "{err}");
    assert!(w.app.db.mentions(KEY).is_empty());
    let _ = TrackerWrite::Reopen { item: String::new() };
}
