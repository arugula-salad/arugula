//! Channel members, private channels and the browser (#118), and group DMs
//! (#117): who sees what (404 for the rest, in the list, search and the
//! live stream), joining and leaving, who adds and removes, a roster change,
//! and what an agent reaches.

use std::time::Duration;

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{
    agents_tests::{geek, give, seeded},
    chat::agent_room,
    chat_tests::{call, hears, open_live},
    db::projects::THREADS_READ,
    grants::AGENT_UNGRANTED,
};

const CHANNELS: &str = "/api/chat/teams/t/channels";
const DMS: &str = "/api/chat/teams/t/dms";

async fn make(base: &str, who: &str, name: &str, private: bool) -> String {
    let (st, v) = call(base, who, "POST", CHANNELS, Some(json!({ "name": name, "private": private }))).await;
    assert_eq!(st, 200, "{v}");
    v["id"].as_str().unwrap().to_owned()
}

async fn rooms(base: &str, who: &str) -> Vec<String> {
    let (st, v) = call(base, who, "GET", "/api/chat", None).await;
    assert_eq!(st, 200, "{v}");
    v["rooms"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap().to_owned()).collect()
}

async fn status(base: &str, who: &str, method: &str, path: &str, body: Option<Value>) -> u16 {
    call(base, who, method, path, body).await.0
}

async fn say(base: &str, who: &str, room: &str, text: &str) -> Value {
    let (st, v) =
        call(base, who, "POST", &format!("/api/chat/rooms/{room}/messages"), Some(json!({ "text": text }))).await;
    assert_eq!(st, 200, "{v}");
    v
}

async fn found(base: &str, who: &str, q: &str) -> Vec<String> {
    let (st, v) = call(base, who, "GET", &format!("/api/chat/search?q={q}"), None).await;
    assert_eq!(st, 200, "{v}");
    v["found"].as_array().unwrap().iter().map(|h| h["room"].as_str().unwrap().to_owned()).collect()
}

async fn browser(base: &str, who: &str) -> Vec<(String, u64, bool)> {
    let (st, v) = call(base, who, "GET", CHANNELS, None).await;
    assert_eq!(st, 200, "{v}");
    v["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (c["name"].as_str().unwrap().to_owned(), c["members"].as_u64().unwrap(), c["joined"].as_bool().unwrap())
        })
        .collect()
}

/// A private channel with one person invited: a third member of the team
/// doesn't see it (404 everywhere, nor in search, the browser or the live
/// stream), can't join it, and the invited one does see it.
#[tokio::test]
async fn a_private_channel_is_its_members_alone() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let id = make(&base, "a", "plans", true).await;
    let room = format!("/api/chat/rooms/{id}");
    assert_eq!(status(&base, "a", "POST", &format!("{room}/members"), Some(json!({ "accounts": ["b"] }))).await, 200);
    let x_live = open_live(&base, "x", None).await;
    hears(&x_live, "\"type\":\"hello\"", Duration::from_secs(5)).await;
    let b_live = open_live(&base, "b", None).await;
    hears(&b_live, "\"type\":\"hello\"", Duration::from_secs(5)).await;

    say(&base, "a", &id, "the needle is in the plans").await;
    for who in ["a", "b"] {
        assert_eq!(status(&base, who, "GET", &room, None).await, 200, "{who}");
        assert!(rooms(&base, who).await.contains(&id), "{who}");
        assert_eq!(found(&base, who, "needle").await, std::slice::from_ref(&id), "{who}");
    }
    assert_eq!(status(&base, "x", "GET", &room, None).await, 404);
    assert_eq!(status(&base, "x", "GET", &format!("{room}/messages"), None).await, 404);
    assert_eq!(status(&base, "x", "POST", &format!("{room}/messages"), Some(json!({ "text": "hi" }))).await, 404);
    assert_eq!(status(&base, "x", "POST", &format!("{room}/join"), None).await, 404, "invite-only");
    assert_eq!(status(&base, "x", "POST", &format!("{room}/leave"), None).await, 404);
    assert!(!rooms(&base, "x").await.contains(&id));
    assert!(found(&base, "x", "needle").await.is_empty());
    assert!(browser(&base, "x").await.iter().all(|c| c.0 != "plans"), "hidden from the browser");
    assert!(browser(&base, "b").await.iter().all(|c| c.0 != "plans"), "even from a member's");

    // The stream: b hears the private room's message, x doesn't (but does
    // hear #general's, sent after it).
    let seq = say(&base, "a", "ch_general", "everyone").await["seq"].as_i64().unwrap();
    hears(&x_live, &format!("\"room\":\"ch_general\",\"seq\":{seq}"), Duration::from_secs(5)).await;
    hears(&b_live, &format!("\"room\":\"{id}\""), Duration::from_secs(5)).await;
    assert!(!x_live.lock().unwrap().contains(&id), "x heard of the private room: {}", x_live.lock().unwrap());
}

/// Leave #general: it leaves the sidebar, stays in the browser (and can be
/// joined again), and a roster change that takes someone out of the team
/// hides it from them through the feed.
#[tokio::test]
async fn leaving_a_public_channel_keeps_it_in_the_browser() {
    let (hub, app, base, _t, _u) = seeded().await;
    // Chat stays on for x out of t.
    hub.flag("account:x", true);
    assert_eq!(browser(&base, "x").await, [("ch_general".to_owned(), 3, true)]);
    assert_eq!(status(&base, "x", "POST", "/api/chat/rooms/ch_general/leave", None).await, 204);
    assert!(!rooms(&base, "x").await.contains(&"ch_general".to_owned()));
    assert_eq!(status(&base, "x", "GET", "/api/chat/rooms/ch_general", None).await, 404);
    assert_eq!(browser(&base, "x").await, [("ch_general".to_owned(), 2, false)]);
    assert_eq!(browser(&base, "x").await.len(), 1);
    // Searching the browser by name or topic.
    let (_, v) = call(&base, "x", "GET", &format!("{CHANNELS}?q=nope"), None).await;
    assert_eq!(v["channels"], json!([]));
    // Back in.
    let (st, v) = call(&base, "x", "POST", "/api/chat/rooms/ch_general/join", None).await;
    assert_eq!((st, v["id"].as_str()), (200, Some("ch_general")), "{v}");
    assert!(rooms(&base, "x").await.contains(&"ch_general".to_owned()));
    // Twice is no harm.
    assert_eq!(status(&base, "x", "POST", "/api/chat/rooms/ch_general/join", None).await, 200);

    // Not in the team: not in the browser, not joined, not seen.
    hub.flag("team:u", true);
    assert_eq!(status(&base, "c", "GET", CHANNELS, None).await, 404);
    assert_eq!(status(&base, "c", "POST", "/api/chat/rooms/ch_general/join", None).await, 404);
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor)]);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(status(&base, "x", "GET", "/api/chat/rooms/ch_general", None).await, 404, "x left the team");
    assert_eq!(browser(&base, "a").await, [("ch_general".to_owned(), 2, true)], "and isn't counted");
    // And back in the team: still a member of what they were in.
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(status(&base, "x", "GET", "/api/chat/rooms/ch_general", None).await, 200);
}

/// A new teammate joins the channels set to take them in, and nothing
/// else; the migration's #general is one (its test is in `db::migrate`).
#[tokio::test]
async fn a_new_teammate_joins_the_auto_join_channels() {
    let (hub, app, base, _t, _u) = seeded().await;
    // The roster the platform holds, before there are channels to join.
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let (st, v) = call(&base, "a", "POST", CHANNELS, Some(json!({ "name": "news", "auto_join": true }))).await;
    assert_eq!(st, 200, "{v}");
    let quiet = make(&base, "a", "quiet", false).await;
    let hidden = call(&base, "a", "POST", CHANNELS, Some(json!({ "name": "hid", "private": true, "auto_join": true })))
        .await
        .1["id"]
        .as_str()
        .unwrap()
        .to_owned();
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer), ("y", TeamRole::Editor)]);
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let ys = rooms(&base, "y").await;
    assert!(ys.contains(&v["id"].as_str().unwrap().to_owned()), "{ys:?}");
    assert!(!ys.contains(&quiet) && !ys.contains(&hidden) && !ys.contains(&"ch_general".to_owned()), "{ys:?}");
}

/// The creator and the team's owners add and remove members; nobody else
/// does; only people of the team are added; the last one out of a private
/// channel stays.
#[tokio::test]
async fn who_adds_and_removes_members() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    // b, an editor, makes a public channel; a (owner) and x join.
    let id = make(&base, "b", "build", false).await;
    let members = format!("/api/chat/rooms/{id}/members");
    for who in ["a", "x"] {
        assert_eq!(status(&base, who, "POST", &format!("/api/chat/rooms/{id}/join"), None).await, 200);
    }
    let (st, v) = call(&base, "b", "GET", &format!("/api/chat/rooms/{id}"), None).await;
    assert_eq!((st, v["can_manage"].clone(), v["people"].as_array().unwrap().len()), (200, json!(true), 3), "{v}");
    let (_, v) = call(&base, "x", "GET", &format!("/api/chat/rooms/{id}"), None).await;
    assert_eq!(v["can_manage"], json!(false));
    let (_, v) = call(&base, "a", "GET", &format!("/api/chat/rooms/{id}"), None).await;
    assert_eq!(v["can_manage"], json!(true), "an owner");

    assert_eq!(status(&base, "x", "POST", &members, Some(json!({ "accounts": ["x"] }))).await, 403);
    assert_eq!(status(&base, "x", "DELETE", &format!("{members}/b"), None).await, 403);
    assert_eq!(status(&base, "b", "POST", &members, Some(json!({ "accounts": ["c"] }))).await, 404, "c isn't in t");
    assert_eq!(status(&base, "a", "DELETE", &format!("{members}/x"), None).await, 200);
    assert_eq!(status(&base, "x", "GET", &format!("/api/chat/rooms/{id}"), None).await, 404, "removed");
    assert_eq!(status(&base, "b", "POST", &members, Some(json!({ "accounts": ["x"] }))).await, 200);
    assert_eq!(status(&base, "x", "GET", &format!("/api/chat/rooms/{id}"), None).await, 200, "added");

    // The last one out of a private channel can't leave or be removed.
    let p = make(&base, "b", "solo", true).await;
    assert_eq!(status(&base, "b", "POST", &format!("/api/chat/rooms/{p}/leave"), None).await, 409);
    assert_eq!(status(&base, "b", "DELETE", &format!("/api/chat/rooms/{p}/members/b"), None).await, 409);
    // A topic is #119's: the browser finds the channel by it.
    let topic = format!("/api/chat/rooms/{id}/topic");
    assert_eq!(status(&base, "b", "PUT", &topic, Some(json!({ "topic": "builds and such" }))).await, 200);
    assert_eq!(browser(&base, "x").await.iter().filter(|c| c.0 == "build").count(), 1);
    let (_, v) = call(&base, "x", "GET", &format!("{CHANNELS}?q=such"), None).await;
    assert_eq!(v["channels"][0]["topic"], "builds and such");
}

/// A mention in a private channel reaches its members and nobody else,
/// even by name: the others aren't in its audience.
#[tokio::test]
async fn a_mention_in_a_private_channel_does_not_reach_outsiders() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let id = make(&base, "a", "plans", true).await;
    let v = say(&base, "a", &id, "@x are you there").await;
    assert_eq!(v["mentions"], json!([]));
    assert_eq!(v["unreached"], json!(["x"]));
    assert_eq!(v["pushed"], json!([]));
}

/// An agent reaches the channels its owner is in, and a group DM: a
/// private channel its owner isn't in is not there for it (404).
#[tokio::test]
async fn an_agent_reaches_only_what_its_owner_is_in() {
    let (_hub, app, base, t, _u) = seeded().await;
    let (st, v) =
        call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{v}");
    let (st, v) =
        call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, json!({ "kind": "account" }))).await;
    assert_eq!(st, 200, "{v}");

    let id = make(&base, "b", "backroom", true).await;
    let e = agent_room(&app, &geek(), THREADS_READ, &id).await.unwrap_err();
    assert_eq!(e.0, axum::http::StatusCode::NOT_FOUND, "a isn't in it");
    assert_eq!(
        status(&base, "b", "POST", &format!("/api/chat/rooms/{id}/members"), Some(json!({ "accounts": ["a"] }))).await,
        200
    );
    assert!(agent_room(&app, &geek(), THREADS_READ, &id).await.is_ok());
    // ... and out again.
    assert_eq!(status(&base, "a", "POST", &format!("/api/chat/rooms/{id}/leave"), None).await, 204);
    assert!(agent_room(&app, &geek(), THREADS_READ, &id).await.is_err());
    // Left #general: gone for its agents too.
    assert!(agent_room(&app, &geek(), THREADS_READ, "ch_general").await.is_ok());
    assert_eq!(status(&base, "a", "POST", "/api/chat/rooms/ch_general/leave", None).await, 204);
    assert!(agent_room(&app, &geek(), THREADS_READ, "ch_general").await.is_err());

    // A group DM a is in is a DM for the grant over their DMs.
    let (st, dm) = call(&base, "a", "POST", DMS, Some(json!({ "with": "b", "and": ["x"] }))).await;
    assert_eq!(st, 200, "{dm}");
    let dm = dm["id"].as_str().unwrap();
    assert!(agent_room(&app, &geek(), THREADS_READ, dm).await.is_ok());
    let _ = AGENT_UNGRANTED;
}

/// A three-person DM: made once for the same set whoever asks, named after
/// its people, renamable by any of them; adding someone is a new
/// conversation, a non-member gets 404, and a leaver doesn't see it while
/// the rest do.
#[tokio::test]
async fn a_group_dm_is_its_people_alone() {
    let (hub, app, base, _t, _u) = seeded().await;
    hub.flag("team:u", true);
    let (st, dm) = call(&base, "a", "POST", DMS, Some(json!({ "with": "b", "and": ["x"] }))).await;
    assert_eq!(st, 200, "{dm}");
    let id = dm["id"].as_str().unwrap().to_owned();
    let others: Vec<&str> = dm["others"].as_array().unwrap().iter().map(|p| p["account"].as_str().unwrap()).collect();
    assert_eq!((others, dm["name"].clone(), dm["writable"].clone()), (vec!["b", "x"], json!(""), json!(true)));
    // The same people again, from anyone, in any order.
    let (_, again) = call(&base, "x", "POST", DMS, Some(json!({ "with": "a", "and": ["b"] }))).await;
    assert_eq!(again["id"].as_str(), Some(id.as_str()));
    let (_, again) = call(&base, "b", "POST", DMS, Some(json!({ "with": "x", "and": ["a", "a"] }))).await;
    assert_eq!(again["id"].as_str(), Some(id.as_str()));
    // The pair is its own conversation, and a's and b's from before stays.
    let (_, pair) = call(&base, "a", "POST", DMS, Some(json!({ "with": "b" }))).await;
    assert_eq!(pair["id"], "dm_ab");

    // Not in it: 404 (c is in another team; y is in this one).
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer), ("y", TeamRole::Editor)]);
    crate::cache::catch_up(&app).await.unwrap();
    let room = format!("/api/chat/rooms/{id}");
    for who in ["y", "c"] {
        assert_eq!(status(&base, who, "GET", &room, None).await, 404, "{who}");
        assert!(!rooms(&base, who).await.contains(&id), "{who}");
    }
    // Adding someone is a new conversation, empty of what was said.
    let v = say(&base, "a", &id, "secret needle").await;
    assert_eq!(v["pushed"].as_array().unwrap().len(), 2, "pushes follow the DM default: {v}");
    let (st, four) = call(&base, "a", "POST", DMS, Some(json!({ "with": "b", "and": ["x", "y"] }))).await;
    assert_eq!(st, 200, "{four}");
    assert_ne!(four["id"], id.as_str());
    let (_, ms) =
        call(&base, "y", "GET", &format!("/api/chat/rooms/{}/messages", four["id"].as_str().unwrap()), None).await;
    assert_eq!(ms["messages"], json!([]));
    assert_eq!(found(&base, "y", "needle").await, Vec::<String>::new());
    // Too many, or someone who isn't there, or only yourself.
    let nine: Vec<String> = (0..8).map(|i| format!("p{i}")).collect();
    assert_eq!(status(&base, "a", "POST", DMS, Some(json!({ "with": "b", "and": nine }))).await, 400);
    assert_eq!(status(&base, "a", "POST", DMS, Some(json!({ "with": "b", "and": ["c"] }))).await, 404);
    assert_eq!(status(&base, "a", "POST", DMS, Some(json!({ "with": "a" }))).await, 404);

    // Renamed by anyone in it; not by anyone else; the others see it.
    let (st, v) = call(&base, "x", "PATCH", &room, Some(json!({ "name": "launch crew" }))).await;
    assert_eq!((st, v["name"].as_str()), (200, Some("launch crew")), "{v}");
    assert_eq!(status(&base, "y", "PATCH", &room, Some(json!({ "name": "mine" }))).await, 404);
    let (_, v) = call(&base, "b", "GET", &room, None).await;
    assert_eq!(v["room"]["name"], "launch crew");
    assert_eq!(v["people"].as_array().unwrap().len(), 3);
}

/// Back in a DM in the millisecond of the last thing said there (said just
/// before they came back): they read from after it, not from it. Later,
/// from when they came back (Arugula #294).
#[tokio::test]
async fn back_in_a_dm_in_the_millisecond_of_its_last_message_reads_from_after_it() {
    let (hub, app, base, _t, _u) = seeded().await;
    hub.flag("account:x", true);
    let (_, dm) = call(&base, "a", "POST", DMS, Some(json!({ "with": "b", "and": ["x"] }))).await;
    let id = dm["id"].as_str().unwrap().to_owned();
    assert_eq!(status(&base, "x", "POST", &format!("/api/chat/rooms/{id}/leave"), None).await, 204);
    let at = arugula_e2e::now_ms() + 60_000;
    let m = crate::db::chat::NewMessage {
        author: "b",
        at,
        text: "while away",
        quote: None,
        ref_machine: None,
        ref_pane: None,
        mentions: &[],
        agent: None,
        via: None,
        parent: None,
        also: false,
    };
    app.db.add_chat_message(&id, &m).unwrap();
    assert!(app.db.add_chat_member_from_now(&id, "x", at).unwrap());
    assert_eq!(app.db.chat_member_from(&id, "x").unwrap(), Some(at + 1));
    assert!(!app.db.add_chat_member_from_now(&id, "x", at).unwrap(), "a member already");
    assert!(app.db.remove_chat_member(&id, "x").unwrap());
    assert!(app.db.add_chat_member_from_now(&id, "x", at + 5).unwrap());
    assert_eq!(app.db.chat_member_from(&id, "x").unwrap(), Some(at + 5));
}

/// One who leaves a group DM doesn't see it; the rest do, and write while
/// two are left; the same people asking again bring them back, from then.
/// One who leaves the team is the same: the rest still read it.
#[tokio::test]
async fn leaving_a_group_dm_leaves_the_rest() {
    let (hub, app, base, _t, _u) = seeded().await;
    hub.flag("account:x", true);
    let (_, dm) = call(&base, "a", "POST", DMS, Some(json!({ "with": "b", "and": ["x"] }))).await;
    let id = dm["id"].as_str().unwrap().to_owned();
    let room = format!("/api/chat/rooms/{id}");
    say(&base, "a", &id, "before").await;

    assert_eq!(status(&base, "x", "POST", &format!("{room}/leave"), None).await, 204);
    assert_eq!(status(&base, "x", "GET", &room, None).await, 404);
    assert!(!rooms(&base, "x").await.contains(&id));
    for who in ["a", "b"] {
        let (st, v) = call(&base, who, "GET", &room, None).await;
        assert_eq!((st, v["room"]["writable"].clone(), v["people"].as_array().unwrap().len()), (200, json!(true), 2));
    }
    say(&base, "b", &id, "after").await;
    // x comes back by asking for the same people: from now on.
    let (_, back) = call(&base, "x", "POST", DMS, Some(json!({ "with": "a", "and": ["b"] }))).await;
    assert_eq!(back["id"].as_str(), Some(id.as_str()));
    tokio::time::sleep(Duration::from_millis(5)).await;
    say(&base, "a", &id, "later").await;
    let (_, ms) = call(&base, "x", "GET", &format!("{room}/messages"), None).await;
    let texts: Vec<&str> = ms["messages"].as_array().unwrap().iter().map(|m| m["text"].as_str().unwrap()).collect();
    assert_eq!(texts, ["later"], "not what was said while away");

    // One leaves the team: the rest still read it, and write while two are
    // left; with one, it's read-only.
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor)]);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(status(&base, "x", "GET", &room, None).await, 404);
    let (st, v) = call(&base, "a", "GET", &room, None).await;
    assert_eq!((st, v["room"]["writable"].clone()), (200, json!(true)));
    hub.team("t", &[("a", TeamRole::Owner)]);
    crate::cache::catch_up(&app).await.unwrap();
    let (st, v) = call(&base, "a", "GET", &room, None).await;
    assert_eq!((st, v["room"]["writable"].clone()), (200, json!(false)), "{v}");
    assert_eq!(status(&base, "a", "POST", &format!("{room}/messages"), Some(json!({ "text": "alone" }))).await, 409);
}
