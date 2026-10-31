//! Writing to Plane (Arugula #359) through the platform's routes, against
//! `FakeTracker`: an agent's write waits, is approved and reaches the fake
//! with the footer; in allow mode it lands at once; without `trackers.write`
//! it is refused; a person's lands at once; text reaches Plane escaped.

use std::sync::Arc;

use arugula_control_wire::projects::{TrackerClose, TrackerWrite, agent_tracker_writes};
use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use super::{
    connect,
    connect_tests::{KEY, backfilled, connect_as, count, serve},
    fake::{FakeTracker, Sent, default_fields, item},
    writes::changes_state,
};
use crate::{
    Platform,
    agents_tests::give,
    chat_tests::{TestHub, call},
    db::{
        sql::{Sql as _, params},
        trackers::{NewConnection, RECONCILE_MS},
    },
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
    hub: Arc<TestHub>,
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
    World { hub, fake, app, base, project, connection }
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

/// A project with one connection: `create {title}` lands there, and the
/// reach lists the connection (identifier and policy) without a key. With a
/// second, a create naming neither is refused listing both; `tracker` names
/// one in any case; a disconnected one is not listed or chosen.
#[tokio::test]
async fn create_picks_the_only_connection_and_the_reach_lists_them() {
    let w = world().await;
    w.grant().await;

    let (s, v) = geek(&w.base, "GET", "/api/daemon/trackers/reach", None).await;
    assert_eq!(s, 200, "{v}");
    let listed = &v["projects"][0]["connections"];
    assert_eq!(
        (listed[0]["id"].as_str(), listed[0]["identifier"].as_str(), listed[0]["agent_writes"].as_str()),
        (Some(w.connection.as_str()), Some("ARUGU"), Some("approve")),
        "{v}"
    );
    assert!(!v.to_string().contains(KEY), "{v}");

    let (s, v) = w.agent(json!({ "kind": "create", "title": "Only one" })).await;
    assert_eq!((s, v["state"].as_str()), (200, Some("waiting")), "{v}");
    assert_eq!(count(&w.app, "tracker_writes", "connection_id = ?1", params![w.connection.clone()]), 1);

    let second = NewConnection {
        id: "tc_second".into(),
        project_id: w.project.clone(),
        workspace_slug: "acme".into(),
        source_project_id: "pl-proj-2".into(),
        identifier: "OPS".into(),
        name: "Ops".into(),
        linked_by: "alice".into(),
    };
    w.app.db.connect_tracker(&second, 5).unwrap().unwrap();
    let (s, v) = w.agent(json!({ "kind": "create", "title": "Which?" })).await;
    let said = v["error"].as_str().unwrap_or_default();
    assert_eq!(s, 422, "{v}");
    assert!(said.contains(&format!("{} (ARUGU)", w.connection)) && said.contains("tc_second (OPS)"), "{v}");

    let (s, v) = w.agent(json!({ "kind": "create", "tracker": "ops", "title": "By identifier" })).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(count(&w.app, "tracker_writes", "connection_id = 'tc_second'", params![]), 1);
    let (s, v) = w.agent(json!({ "kind": "create", "tracker": "NOPE", "title": "Nowhere" })).await;
    assert_eq!(s, 404, "{v}");
    let (s, v) =
        w.agent(json!({ "kind": "create", "connection": w.connection, "tracker": "OPS", "title": "Clash" })).await;
    assert_eq!(s, 422, "{v}");

    w.app.db.disconnect_tracker(&w.project, "tc_second", 9).unwrap().unwrap();
    let (s, v) = w.agent(json!({ "kind": "create", "title": "Back to one" })).await;
    assert_eq!(s, 200, "{v}");
    let (_, v) = geek(&w.base, "GET", "/api/daemon/trackers/reach", None).await;
    assert_eq!(v["projects"][0]["connections"].as_array().unwrap().len(), 1, "{v}");
}

/// A project with no live connection refuses a create that names none.
#[tokio::test]
async fn create_without_a_connection_is_refused_when_there_is_none() {
    let w = world().await;
    w.grant().await;
    w.app.db.disconnect_tracker(&w.project, &w.connection, 9).unwrap().unwrap();
    let (s, v) = w.agent(json!({ "kind": "create", "title": "Nowhere" })).await;
    assert_eq!(s, 422, "{v}");
    assert!(v["error"].as_str().unwrap().contains("no connected Plane project"), "{v}");
}

// ---- fields (#422)

impl World {
    /// Allow mode, a grant: an agent's writes land at once.
    async fn allow(&self) {
        let path = format!("/api/projects/{}/trackers/{}", self.project, self.connection);
        let (s, v) = call(&self.base, "alice", "PATCH", &path, Some(json!({ "agent_writes": "allow" }))).await;
        assert_eq!(s, 200, "{v}");
        self.grant().await;
    }
}

/// A create names its type, module, labels and state, and all five reach
/// Plane as ids: the fields in the create, the module in a call of its own.
#[tokio::test]
async fn a_create_sets_type_module_labels_and_state_by_name() {
    let w = world().await;
    let (s, v) = w
        .person(
            "alice",
            json!({
                "kind": "create", "title": "Boards epic", "type": "Epic", "module": " boards ",
                "labels": ["Agent", "agent"], "state": "in progress",
            }),
        )
        .await;
    assert_eq!(
        (s, v["state"].as_str(), v["text"].as_str()),
        (200, Some("sent"), Some("created ARUGU-4 in Plane")),
        "{v}"
    );
    let sent = w.fake.sent();
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(
        (sent[0].state.as_deref(), sent[0].item_type.as_deref(), sent[0].labels.as_slice()),
        (Some("st-progress"), Some("ty-epic"), ["lb-agent".to_owned()].as_slice())
    );
    assert_eq!(
        (sent[1].kind.as_str(), sent[1].module.as_deref(), sent[1].item.as_deref()),
        ("module", Some("mod-boards"), Some("item-4"))
    );
    let (state, kind): (String, Option<String>) = w
        .app
        .db
        .c()
        .query_row(
            "SELECT state_name, item_type FROM forge_items WHERE number = 4 AND provider = 'plane'",
            params![],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((state.as_str(), kind.as_deref()), ("In Progress", Some("Epic")));
    // An id works too, and a group is its first state.
    let (s, _) = w.person("alice", json!({ "kind": "create", "title": "B", "state": "st-review" })).await;
    assert_eq!(s, 200);
    let (s, _) = w.person("alice", json!({ "kind": "create", "title": "C", "state": "started" })).await;
    assert_eq!(s, 200);
    let states: Vec<_> = w.fake.sent().iter().filter(|s| s.kind == "create").map(|s| s.state.clone()).collect();
    assert_eq!(states, [Some("st-progress".into()), Some("st-review".into()), Some("st-progress".into())]);
}

/// An exact state name beats a group of the same word.
#[tokio::test]
async fn a_state_named_like_a_group_is_that_state() {
    let w = world().await;
    let mut f = default_fields();
    f.states[3].name = "Started".into();
    w.fake.set_fields(f);
    let (s, _) = w.person("alice", json!({ "kind": "create", "title": "T", "state": "started" })).await;
    assert_eq!(s, 200);
    assert_eq!(w.fake.sent()[0].state.as_deref(), Some("st-review"), "the state called Started, not the group's first");
}

/// An edit with only a state is a state change; labels are added and taken
/// off the item's own; a title with them is one edit.
#[tokio::test]
async fn an_edit_moves_the_state_and_changes_labels() {
    let w = world().await;
    let (s, v) = w.person("alice", json!({ "kind": "edit", "item": "ARUGU-1", "state": "In Review" })).await;
    assert_eq!((s, v["text"].as_str()), (200, Some("edited ARUGU-1 in Plane (now In Review)")), "{v}");
    let sent = w.fake.sent();
    assert_eq!((sent[0].kind.as_str(), sent[0].state.as_deref()), ("state", Some("st-review")));
    let name: String = w
        .app
        .db
        .c()
        .query_row("SELECT state_name FROM forge_items WHERE number = 1 AND provider = 'plane'", params![], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(name, "In Review");

    let (s, v) = w.person("alice", json!({ "kind": "edit", "item": "ARUGU-1", "labels": ["agent", "bug"] })).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(w.fake.labels_of("item-1"), ["lb-agent", "lb-bug"]);
    let (s, v) = w
        .person("alice", json!({ "kind": "edit", "item": "ARUGU-1", "title": "Renamed", "labels_remove": ["Agent"] }))
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(w.fake.labels_of("item-1"), ["lb-bug"]);
    let last = w.fake.sent().pop().unwrap();
    assert_eq!(
        (last.kind.as_str(), last.title.as_deref(), last.labels_remove.as_slice()),
        ("edit", Some("Renamed"), ["lb-agent".to_owned()].as_slice())
    );

    // An edit that says nothing is refused; a module alone writes only the module.
    let (s, _) = w.person("alice", json!({ "kind": "edit", "item": "ARUGU-1" })).await;
    assert_eq!(s, 422);
    let n = w.fake.sent().len();
    let (s, v) = w.person("alice", json!({ "kind": "edit", "item": "ARUGU-2", "module": "Boards" })).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(w.fake.sent().len(), n + 1);
    assert_eq!(w.fake.modules_of("item-2"), ["mod-boards"]);
}

/// A reopen goes to the default state and says which; without a default,
/// to the first unstarted state.
#[tokio::test]
async fn a_reopen_lands_in_the_default_state_and_names_it() {
    let w = world().await;
    let (s, v) = w.person("alice", json!({ "kind": "close", "item": "ARUGU-1", "as": "completed" })).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(w.fake.sent()[0].state.as_deref(), Some("st-done"));
    let (s, v) = w.person("alice", json!({ "kind": "reopen", "item": "ARUGU-1" })).await;
    assert_eq!((s, v["text"].as_str()), (200, Some("reopened ARUGU-1 in Plane (Todo)")), "{v}");

    // The project's default moves to Backlog; the kept fields are aged, as time would.
    let age = |w: &World| {
        w.app.db.set_tracker_fields(&w.connection, &serde_json::to_string(&default_fields()).unwrap(), 1).unwrap()
    };
    let mut f = default_fields();
    f.states[1].default = false;
    f.states[0].default = true;
    w.fake.set_fields(f.clone());
    age(&w);
    let (s, v) = w.person("alice", json!({ "kind": "reopen", "item": "ARUGU-1" })).await;
    assert_eq!((s, v["text"].as_str()), (200, Some("reopened ARUGU-1 in Plane (Backlog)")), "{v}");

    f.states[0].default = false;
    w.fake.set_fields(f);
    age(&w);
    let (s, v) = w.person("alice", json!({ "kind": "reopen", "item": "ARUGU-1" })).await;
    assert_eq!((s, v["text"].as_str()), (200, Some("reopened ARUGU-1 in Plane (Todo)")), "{v}");
}

/// An unknown state is refused, listing the real ones, after one read of
/// Plane; fresh fields are not read again, and a name added in Plane since
/// is found by that one read.
#[tokio::test]
async fn an_unknown_name_is_refused_after_one_refresh_listing_the_real_ones() {
    let w = world().await;
    // Nothing kept yet: the read that fills the cache is the one.
    let (s, v) = w.person("alice", json!({ "kind": "create", "title": "T", "state": "In Reveiw" })).await;
    assert_eq!(s, 422, "{v}");
    assert_eq!(
        v["error"].as_str(),
        Some("no state \"In Reveiw\" in ARUGU; states: Backlog, Todo, In Progress, In Review, Done, Cancelled")
    );
    assert_eq!(w.fake.fields_read(), 1);
    assert!(w.fake.sent().is_empty());

    // Kept and fresh: a good name reads nothing.
    let (s, _) = w.person("alice", json!({ "kind": "create", "title": "T", "state": "Done" })).await;
    assert_eq!(s, 200);
    assert_eq!(w.fake.fields_read(), 1);

    // A bad one reads once more, and the other kinds list their options too.
    let (s, v) = w.person("alice", json!({ "kind": "create", "title": "T", "module": "Nope" })).await;
    assert_eq!((s, v["error"].as_str()), (422, Some("no module \"Nope\" in ARUGU; modules: Boards")), "{v}");
    assert_eq!(w.fake.fields_read(), 2);
    let (_, v) = w.person("alice", json!({ "kind": "create", "title": "T", "type": "Story" })).await;
    assert_eq!(v["error"].as_str(), Some("no type \"Story\" in ARUGU; types: Epic, Task"), "{v}");
    let (_, v) = w.person("alice", json!({ "kind": "edit", "item": "ARUGU-1", "labels_remove": ["x"] })).await;
    assert_eq!(v["error"].as_str(), Some("no label \"x\" in ARUGU; labels: agent, bug"), "{v}");
    assert_eq!(w.fake.sent().len(), 1, "only the good create");

    // A project without types says so.
    let mut f = default_fields();
    f.types.clear();
    w.fake.set_fields(f);
    w.app.db.set_tracker_fields(&w.connection, &serde_json::to_string(&default_fields()).unwrap(), 1).unwrap();
    let (_, v) = w.person("alice", json!({ "kind": "create", "title": "T", "type": "Epic" })).await;
    assert_eq!(v["error"].as_str(), Some("no type \"Epic\" in ARUGU: it has no types"), "{v}");

    // A state added in Plane since is found by the one read.
    let mut f = default_fields();
    f.states.push(super::StateInfo { id: "st-qa".into(), name: "QA".into(), group: "started".into(), default: false });
    w.fake.set_fields(f);
    let before = w.fake.fields_read();
    let (s, v) = w.person("alice", json!({ "kind": "create", "title": "T", "state": "qa" })).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!((w.fake.fields_read(), w.fake.sent().last().unwrap().state.as_deref()), (before + 1, Some("st-qa")));
}

/// Fields kept longer than ten minutes are read again before a write uses
/// them.
#[tokio::test]
async fn stale_fields_are_read_again() {
    let w = world().await;
    let (s, _) = w.person("alice", json!({ "kind": "create", "title": "T", "state": "Done" })).await;
    assert_eq!((s, w.fake.fields_read()), (200, 1));
    let json = serde_json::to_string(&default_fields()).unwrap();
    let old = arugula_e2e::now_ms() - 600_001;
    w.app.db.set_tracker_fields(&w.connection, &json, old).unwrap();
    let (s, _) = w.person("alice", json!({ "kind": "create", "title": "T", "state": "Done" })).await;
    assert_eq!((s, w.fake.fields_read()), (200, 2));
    let (s, _) = w.person("alice", json!({ "kind": "create", "title": "T", "state": "Done" })).await;
    assert_eq!((s, w.fake.fields_read()), (200, 2), "fresh again");
}

/// The reconcile keeps the fields.
#[tokio::test]
async fn the_reconcile_refreshes_the_fields() {
    let w = world().await;
    assert!(w.app.db.tracker_fields(&w.connection).unwrap().is_none());
    let due = arugula_e2e::now_ms() + RECONCILE_MS + 1;
    assert!(connect::reconcile(&w.app, due).await.unwrap().is_some());
    assert_eq!(w.fake.fields_read(), 1);
    let (json, _) = w.app.db.tracker_fields(&w.connection).unwrap().unwrap();
    assert!(json.contains("In Review") && json.contains("Boards"), "{json}");
}

/// A write that waits keeps the names it was given, and meets the project
/// as it is when a person approves it.
#[tokio::test]
async fn a_waiting_write_resolves_its_names_when_approved() {
    let w = world().await;
    w.grant().await;
    let (s, v) = w
        .agent(json!({ "kind": "create", "connection": w.connection, "title": "Ship it", "state": "Shipped", "labels": ["agent"] }))
        .await;
    assert_eq!((s, v["state"].as_str()), (200, Some("waiting")), "a name Plane lacks doesn't stop it queueing: {v}");
    let id = v["write"].as_str().unwrap().to_owned();
    assert_eq!(w.fake.fields_read(), 0, "queueing reads nothing");
    let kept: String =
        w.app.db.c().query_row("SELECT request FROM tracker_writes WHERE id = ?1", params![id], |r| r.get(0)).unwrap();
    assert!(kept.contains("Shipped") && !kept.contains("st-"), "{kept}");

    let mut f = default_fields();
    f.states.push(super::StateInfo {
        id: "st-shipped".into(),
        name: "Shipped".into(),
        group: "completed".into(),
        default: false,
    });
    w.fake.set_fields(f);
    let (s, v) = w.decide("bob", &id, "approve").await;
    assert_eq!((s, v["state"].as_str()), (200, Some("sent")), "{v}");
    assert_eq!(w.fake.sent()[0].state.as_deref(), Some("st-shipped"));

    // One that is still unknown at approval fails there, saying why, and sends nothing.
    let (_, v) = w.agent(json!({ "kind": "create", "connection": w.connection, "title": "X", "state": "Gone" })).await;
    let id = v["write"].as_str().unwrap().to_owned();
    let (s, v) = w.decide("bob", &id, "approve").await;
    assert_eq!(s, 422, "{v}");
    assert!(v["error"].as_str().unwrap().contains("states: Backlog"), "{v}");
    assert_eq!(w.fake.sent().len(), 1);
}

/// The item is made and the module call fails: the answer says so, as a
/// write that landed, so nothing sends it again.
#[tokio::test]
async fn a_failed_module_leaves_the_item_made_and_says_so() {
    let w = world().await;
    w.allow().await;
    w.fake.set_module_down(true);
    let (s, v) =
        w.agent(json!({ "kind": "create", "connection": w.connection, "title": "T", "module": "Boards" })).await;
    assert_eq!((s, v["state"].as_str(), v["reference"].as_str()), (200, Some("sent"), Some("ARUGU-4")), "{v}");
    let text = v["text"].as_str().unwrap();
    assert!(text.starts_with("created ARUGU-4 in Plane; couldn't add it to module Boards: "), "{text}");
    assert_eq!((w.items(), w.fake.sent().iter().filter(|s| s.kind == "create").count()), (4, 1));

    // Through approval too: it is sent, not failed, so it can't be approved again.
    let path = format!("/api/projects/{}/trackers/{}", w.project, w.connection);
    call(&w.base, "alice", "PATCH", &path, Some(json!({ "agent_writes": "approve" }))).await;
    let (_, v) =
        w.agent(json!({ "kind": "create", "connection": w.connection, "title": "U", "module": "Boards" })).await;
    let id = v["write"].as_str().unwrap().to_owned();
    let (s, v) = w.decide("bob", &id, "approve").await;
    assert_eq!((s, v["state"].as_str()), (200, Some("sent")), "{v}");
    assert!(v["text"].as_str().unwrap().contains("couldn't add it to module Boards"), "{v}");
    assert_eq!(w.decide("bob", &id, "approve").await.0, 409);
    assert_eq!(w.fake.sent().iter().filter(|s| s.kind == "create").count(), 2);
}

/// A parent is `IDENT-N` of the same Plane project: the mirror's item, or
/// Plane's own by number when the mirror lacks it.
#[tokio::test]
async fn a_parent_is_found_in_the_mirror_or_in_plane_and_must_be_in_the_connection() {
    let w = world().await;
    let (s, v) = w.person("alice", json!({ "kind": "create", "title": "Child", "parent": "arugu-1" })).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(w.fake.sent()[0].parent.as_deref(), Some("item-1"));

    // Plane has an item the mirror doesn't yet.
    let mut all: Vec<_> = (1..=3).map(|n| item(n, "x", "backlog", 1_000)).collect();
    all.push(item(9, "Not mirrored", "backlog", 1_000));
    w.fake.set_items(all);
    let (s, v) = w.person("alice", json!({ "kind": "edit", "item": "ARUGU-2", "parent": "ARUGU-9" })).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(w.fake.sent().last().unwrap().parent.as_deref(), Some("item-9"));

    for (parent, want) in [
        ("OTHER-1", "parent: OTHER-1 isn't in ARUGU; a parent is in the same Plane project"),
        ("ARUGU-77", "parent: Plane has no work item ARUGU-77 in ARUGU"),
        ("one", "parent: IDENT-N, such as ARUGU-7"),
    ] {
        let n = w.fake.sent().len();
        let (s, v) = w.person("alice", json!({ "kind": "create", "title": "Child", "parent": parent })).await;
        assert_eq!((s, v["error"].as_str()), (422, Some(want)), "{parent}: {v}");
        assert_eq!(w.fake.sent().len(), n);
    }
}

/// The released daemon's create (`state_group`, or `state`) still parses
/// and still lands in that group's first state; a write the build before
/// kept for approval is approved by this one.
#[tokio::test]
async fn an_older_daemons_create_still_works() {
    let old: TrackerWrite =
        serde_json::from_value(json!({ "kind": "create", "title": "T", "state_group": "started" })).unwrap();
    let TrackerWrite::Create { state, labels, module, .. } = &old else { panic!("{old:?}") };
    assert_eq!((state.as_deref(), labels.is_empty(), module.is_none()), (Some("started"), true, true));
    let edit: TrackerWrite =
        serde_json::from_value(json!({ "kind": "edit", "item": "ARUGU-1", "title": "T" })).unwrap();
    assert!(matches!(edit, TrackerWrite::Edit { state: None, .. }));

    let w = world().await;
    w.allow().await;
    let (s, v) = w
        .agent(json!({ "kind": "create", "connection": w.connection, "title": "Old", "state_group": "started" }))
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(w.fake.sent()[0].state.as_deref(), Some("st-progress"));

    let request = json!({ "kind": "edit", "item": "ARUGU-1", "title": "Kept before" }).to_string();
    w.app
        .db
        .add_tracker_write(&crate::db::trackers::PendingWrite {
            id: "tw_old".into(),
            project_id: w.project.clone(),
            connection_id: w.connection.clone(),
            kind: "edit".into(),
            request,
            agent_machine: "geek".into(),
            for_account: "alice".into(),
            state: "waiting".into(),
            error: None,
            item_id: None,
            created_at: 1,
        })
        .unwrap();
    let (s, v) = w.decide("bob", "tw_old", "approve").await;
    assert_eq!(s, 200, "{v}");
}

/// An approval can tell a state change from an edit of the text.
#[test]
fn an_edit_that_names_a_state_changes_state() {
    let edit = |state: Option<&str>| TrackerWrite::Edit {
        item: "ARUGU-1".into(),
        title: Some("T".into()),
        body: None,
        state: state.map(Into::into),
        module: None,
        parent: None,
        item_type: None,
        labels: vec![],
        labels_remove: vec![],
    };
    assert!(changes_state(&edit(Some("Done"))) && !changes_state(&edit(None)));
    assert!(changes_state(&TrackerWrite::Close { item: "x".into(), close_as: TrackerClose::Completed }));
    assert!(changes_state(&TrackerWrite::Reopen { item: "x".into() }));
    assert!(!changes_state(&TrackerWrite::Comment { item: "x".into(), text: "t".into() }));
}

/// `forge_read tracker_fields` is `tracker.fields`: a member reads the
/// project's states, modules, types and labels (kept, not read again);
/// someone outside it, or a key it doesn't hold, doesn't.
#[tokio::test]
async fn the_fields_are_read_by_members_through_the_operation() {
    let w = world().await;
    let ask = |who: &'static str, args: Value| {
        let (base, mut body) = (w.base.clone(), args);
        async move {
            body["op"] = json!("tracker.fields");
            body["v"] = json!(1);
            call(&base, who, "POST", "/api/forge/ops", Some(body)).await
        }
    };
    let (s, v) = ask("bob", json!({ "project": w.project, "tracker": "arugu" })).await;
    assert_eq!(s, 200, "{v}");
    let r = &v["result"];
    assert_eq!((r["identifier"].as_str(), r["connection"].as_str()), (Some("ARUGU"), Some(w.connection.as_str())));
    let names = |k: &str| -> Vec<String> {
        r[k].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(names("states"), ["Backlog", "Todo", "In Progress", "In Review", "Done", "Cancelled"]);
    assert_eq!(
        (names("modules"), names("types"), names("labels")),
        (
            vec!["Boards".to_owned()],
            vec!["Epic".to_owned(), "Task".to_owned()],
            vec!["agent".to_owned(), "bug".to_owned()]
        )
    );
    assert_eq!(r["states"][1]["default"], true);
    assert_eq!(w.fake.fields_read(), 1);
    let (s, _) = ask("bob", json!({ "project": w.project })).await;
    assert_eq!((s, w.fake.fields_read()), (200, 1), "the one connection, and kept");
    assert_eq!(ask("bob", json!({ "project": w.project, "tracker": "NOPE" })).await.0, 404);
    assert_eq!(ask("eve", json!({ "project": w.project })).await.0, 404);
    // Boards are Labs: off, there is nothing to read.
    w.hub.flag_named("team:t1", crate::boards::FLAG, false);
    w.app.cache.drop_all();
    assert_eq!(ask("bob", json!({ "project": w.project })).await.0, 404);
}
