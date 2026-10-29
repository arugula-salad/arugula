//! Notification levels and do not disturb (#113, #115) through chat's
//! routes: which pushes the hub is asked for at each level, snoozed or
//! not. The decisions on their own are the base's (`notify.rs`'s tests).

use std::{sync::Arc, time::Duration};

use arugula_control_wire::chat::NotifyLevel as L;
use arugula_e2e::now_ms;
use serde_json::{Value, json};

use crate::{
    Platform,
    chat_tests::{TestHub, call, seed, serve},
};

async fn setup() -> (Arc<TestHub>, Arc<Platform>, String) {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    (hub, app, base)
}

/// Until the hub has been asked for `n` pushes (they go in the background).
async fn pushes(hub: &TestHub, n: usize) -> Vec<(String, Value)> {
    for _ in 0..50 {
        if hub.pushes().len() >= n {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // A moment more, for a push that shouldn't be there.
    tokio::time::sleep(Duration::from_millis(60)).await;
    hub.pushes().into_iter().map(|(a, m, _)| (a, m)).collect()
}

async fn say(base: &str, who: &str, room: &str, text: &str) -> Value {
    let (st, v) =
        call(base, who, "POST", &format!("/api/chat/rooms/{room}/messages"), Some(json!({ "text": text }))).await;
    assert_eq!(st, 200, "{v}");
    v
}

#[tokio::test]
async fn a_channel_on_mentions_pushes_the_mention_and_not_the_rest() {
    let (hub, _, base) = setup().await;
    let v = say(&base, "a", "ch_general", "morning all").await;
    assert_eq!(v["pushed"], json!([]), "{v}");
    let v = say(&base, "a", "ch_general", "@b a word").await;
    assert_eq!(v["pushed"], json!([{ "account": "b", "why": "mention" }]), "{v}");
    assert_eq!(pushes(&hub, 1).await.len(), 1);
}

#[tokio::test]
async fn every_message_pushes_when_the_room_is_on_all() {
    let (hub, _, base) = setup().await;
    let (st, r) = call(&base, "b", "POST", "/api/chat/rooms/ch_general/level", Some(json!({ "level": "all" }))).await;
    assert_eq!((st, &r["level"], &r["level_own"]), (200, &json!("all"), &json!(true)), "{r}");
    let v = say(&base, "a", "ch_general", "morning all").await;
    assert_eq!(v["pushed"], json!([{ "account": "b", "why": "message" }]), "{v}");
    let sent = pushes(&hub, 1).await;
    assert_eq!(sent[0].1["title"], "a in #ch_general");
    // Back to the default: mentions again.
    let (_, r) = call(&base, "b", "POST", "/api/chat/rooms/ch_general/level", Some(json!({ "level": null }))).await;
    assert_eq!((&r["level"], &r["level_own"]), (&json!("mentions"), &json!(false)));
    let v = say(&base, "a", "ch_general", "again").await;
    assert_eq!(v["pushed"], json!([]));
}

#[tokio::test]
async fn the_account_default_decides_for_rooms_with_no_level_of_their_own() {
    let (_, _, base) = setup().await;
    let (st, p) = call(&base, "b", "POST", "/api/notify/prefs", Some(json!({ "channels": "all" }))).await;
    assert_eq!((st, &p["channels"], &p["dms"]), (200, &json!("all"), &json!("all")), "{p}");
    let v = say(&base, "a", "ch_general", "morning").await;
    assert_eq!(v["pushed"], json!([{ "account": "b", "why": "message" }]), "{v}");
    // A room of their own still wins.
    call(&base, "b", "POST", "/api/chat/rooms/ch_general/level", Some(json!({ "level": "mentions" }))).await;
    assert_eq!(say(&base, "a", "ch_general", "again").await["pushed"], json!([]));
    // The page lists the level it gets.
    let (_, rooms) = call(&base, "b", "GET", "/api/chat", None).await;
    let general = rooms["rooms"].as_array().unwrap().iter().find(|r| r["id"] == "ch_general").unwrap();
    assert_eq!((&general["level"], &general["level_own"]), (&json!("mentions"), &json!(true)));
}

#[tokio::test]
async fn nothing_is_what_muting_was() {
    let (_, app, base) = setup().await;
    let (_, r) =
        call(&base, "b", "POST", "/api/chat/rooms/ch_general/level", Some(json!({ "level": "nothing" }))).await;
    assert_eq!((&r["level"], &r["muted"]), (&json!("nothing"), &json!(true)));
    assert_eq!(say(&base, "a", "ch_general", "@b hello?").await["pushed"], json!([]));
    // The old switch reaches the same place, both ways.
    call(&base, "b", "POST", "/api/chat/rooms/ch_general/mute", Some(json!({ "muted": false }))).await;
    assert_eq!(app.db.chat_level("ch_general", "b").unwrap(), None);
    let (_, r) = call(&base, "b", "POST", "/api/chat/rooms/ch_general/mute", Some(json!({ "muted": true }))).await;
    assert_eq!((&r["level"], &r["level_own"]), (&json!("nothing"), &json!(true)));
    assert_eq!(app.db.chat_level("ch_general", "b").unwrap(), Some(L::Nothing));
    assert!(app.db.chat_mark("ch_general", "b").unwrap().1);
}

#[tokio::test]
async fn a_dm_pushes_everything_until_its_level_says_nothing() {
    let (hub, _, base) = setup().await;
    let (st, dm) = call(&base, "a", "POST", "/api/chat/teams/t/dms", Some(json!({ "with": "b" }))).await;
    assert_eq!(st, 200, "{dm}");
    let id = dm["id"].as_str().unwrap().to_owned();
    let v = say(&base, "a", &id, "no mention needed").await;
    assert_eq!(v["pushed"], json!([{ "account": "b", "why": "dm" }]), "{v}");
    assert_eq!(pushes(&hub, 1).await.len(), 1);
    call(&base, "b", "POST", &format!("/api/chat/rooms/{id}/level"), Some(json!({ "level": "nothing" }))).await;
    assert_eq!(say(&base, "a", &id, "still there?").await["pushed"], json!([]));
}

/// "Notify anyway" on a post pushes a DM's paused other person, for that
/// message only (the push gap then holds the next), and does nothing in a
/// channel or for a post that doesn't ask.
#[tokio::test]
async fn notify_anyway_pushes_a_paused_dm_once_and_only_there() {
    let (hub, _, base) = setup().await;
    let (_, dm) = call(&base, "a", "POST", "/api/chat/teams/t/dms", Some(json!({ "with": "b" }))).await;
    let id = dm["id"].as_str().unwrap().to_owned();
    let until = now_ms() + 3600 * 1000;
    call(&base, "b", "POST", "/api/notify/prefs", Some(json!({ "snooze_until": until }))).await;
    let post = |room: String, text: &'static str, anyway: bool| {
        let base = base.clone();
        async move {
            let body = json!({ "text": text, "notify_anyway": anyway });
            call(&base, "a", "POST", &format!("/api/chat/rooms/{room}/messages"), Some(body)).await.1
        }
    };
    assert_eq!(post(id.clone(), "hello?", false).await["pushed"], json!([]));
    let v = post("ch_general".into(), "@b hi", true).await;
    assert_eq!(v["pushed"], json!([]), "a channel has no such thing: {v}");
    assert_eq!(pushes(&hub, 1).await.len(), 0);
    let v = post(id.clone(), "it's urgent", true).await;
    assert_eq!(v["pushed"], json!([{ "account": "b", "why": "dm" }]), "{v}");
    assert_eq!(pushes(&hub, 1).await.len(), 1);
}

/// Snooze an hour: a DM in that hour doesn't push, and once it has ended
/// one does. The others can see the moon meanwhile.
#[tokio::test]
async fn a_snooze_stops_a_dm_pushing_and_shows_a_moon() {
    let (hub, _, base) = setup().await;
    let (_, dm) = call(&base, "a", "POST", "/api/chat/teams/t/dms", Some(json!({ "with": "b" }))).await;
    let id = dm["id"].as_str().unwrap().to_owned();
    assert_eq!(dm["with"]["dnd"], json!(false));
    let until = now_ms() + 3600 * 1000;
    let (st, p) = call(&base, "b", "POST", "/api/notify/prefs", Some(json!({ "snooze_until": until }))).await;
    assert_eq!((st, &p["paused"], &p["snooze_until"]), (200, &json!(true), &json!(until)), "{p}");

    let v = say(&base, "a", &id, "are you there").await;
    assert_eq!(v["pushed"], json!([]), "{v}");
    assert_eq!(pushes(&hub, 1).await.len(), 0);
    // a sees b's moon, in the room and in the list.
    let (_, room) = call(&base, "a", "GET", &format!("/api/chat/rooms/{id}"), None).await;
    assert_eq!(room["room"]["with"]["dnd"], json!(true), "{room}");
    let b = room["people"].as_array().unwrap().iter().find(|p| p["account"] == "b").unwrap();
    assert_eq!(b["dnd"], json!(true));
    // b's unread still counts.
    let (_, rooms) = call(&base, "b", "GET", "/api/chat", None).await;
    let r = rooms["rooms"].as_array().unwrap().iter().find(|r| r["id"] == id).unwrap();
    assert_eq!(r["unread"], json!(1));

    // The hour passes (it ended at 1 ms past the epoch).
    let (_, p) = call(&base, "b", "POST", "/api/notify/prefs", Some(json!({ "snooze_until": 1 }))).await;
    assert_eq!((&p["paused"], &p["snooze_until"]), (&json!(false), &Value::Null), "{p}");
    let v = say(&base, "a", &id, "now?").await;
    assert_eq!(v["pushed"], json!([{ "account": "b", "why": "dm" }]), "{v}");
    assert_eq!(pushes(&hub, 1).await.len(), 1);
    let (_, room) = call(&base, "a", "GET", &format!("/api/chat/rooms/{id}"), None).await;
    assert_eq!(room["room"]["with"]["dnd"], json!(false));
}
