//! Chat's people: who sees whose presence and whose picture. a and b share
//! team t1, b and c share t2, d is alone in t3; everyone has chat on.

use std::sync::Arc;

use arugula_control_wire::chat::PresenceState;
use arugula_e2e::{now_ms, team::TeamRole};
use serde_json::{Value, json};

use crate::{
    chat::people::state_of,
    chat_tests::{TestHub, call, serve},
    db::chat_people::{Avatar, Seen},
};

async fn rig() -> (Arc<TestHub>, Arc<crate::Platform>, String) {
    let hub = Arc::new(TestHub::default());
    hub.team("t1", &[("a", TeamRole::Owner), ("b", TeamRole::Editor)]);
    hub.team("t2", &[("b", TeamRole::Owner), ("c", TeamRole::Editor)]);
    hub.team("t3", &[("d", TeamRole::Owner)]);
    for who in ["a", "b", "c", "d"] {
        hub.flag(&format!("account:{who}"), true);
    }
    hub.daemon("m1", "a", Some("t1"));
    hub.daemon("m2", "d", Some("t3"));
    let (app, base) = serve(hub.clone()).await;
    (hub, app, base)
}

async fn beat(base: &str, who: &str, body: Value) -> Value {
    let (st, v) = call(base, who, "POST", "/api/chat/presence", Some(body)).await;
    assert_eq!(st, 200, "{v}");
    v
}

fn states(v: &Value) -> Vec<(String, String)> {
    v["people"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["account"].as_str().unwrap().to_owned(), p["state"].as_str().unwrap().to_owned()))
        .collect()
}

fn of(v: &Value, who: &str) -> Option<String> {
    states(v).into_iter().find(|(a, _)| a == who).map(|(_, s)| s)
}

fn accounts(v: &Value) -> Vec<String> {
    states(v).into_iter().map(|(a, _)| a).collect()
}

#[test]
fn what_shows_is_the_beat_and_the_clock_and_the_choice() {
    let now = 10_000_000;
    let seen = |seen, active, mode: &str| Seen { seen, active, mode: mode.into(), live: "off".into() };
    assert_eq!(state_of(&seen(now - 5_000, now - 5_000, "auto"), now), PresenceState::Active);
    // Open but unused for over 10 minutes: away.
    assert_eq!(state_of(&seen(now - 5_000, now - 11 * 60_000, "auto"), now), PresenceState::Away);
    // Never used while open, or never heard from: away, or offline.
    assert_eq!(state_of(&seen(now - 5_000, 0, "auto"), now), PresenceState::Away);
    assert_eq!(state_of(&Seen::default(), now), PresenceState::Offline);
    // A page that stopped beating is gone within the minute.
    assert_eq!(state_of(&seen(now - 41_000, now - 41_000, "auto"), now), PresenceState::Offline);
    // Chosen away shows away while a page is open, and offline when none is.
    assert_eq!(state_of(&seen(now - 1_000, now - 1_000, "away"), now), PresenceState::Away);
    assert_eq!(state_of(&seen(now - 90_000, now - 90_000, "away"), now), PresenceState::Offline);
    // Chosen offline wins over everything.
    assert_eq!(state_of(&seen(now, now, "offline"), now), PresenceState::Offline);
}

#[tokio::test]
async fn presence_is_seen_by_those_who_share_a_team_and_nobody_else() {
    let (_hub, _app, base) = rig().await;
    for who in ["a", "b", "c", "d"] {
        beat(&base, who, json!({})).await;
    }
    let a = beat(&base, "a", json!({})).await;
    assert_eq!(accounts(&a), ["a", "b"], "not c (only b's teammate) nor d: {a}");
    let b = beat(&base, "b", json!({})).await;
    assert_eq!(accounts(&b), ["a", "b", "c"], "{b}");
    let c = beat(&base, "c", json!({})).await;
    assert_eq!(accounts(&c), ["b", "c"], "{c}");
    let d = beat(&base, "d", json!({})).await;
    assert_eq!(accounts(&d), ["d"], "alone: {d}");
    assert_eq!(of(&b, "a").as_deref(), Some("active"));

    // A teammate who never opened a page is there, offline.
    let (_hub2, _app2, base2) = rig().await;
    assert_eq!(of(&beat(&base2, "a", json!({})).await, "b").as_deref(), Some("offline"));

    // Chat off: refused, and nothing recorded.
    let (hub3, app3, base3) = rig().await;
    hub3.flag("account:c", false);
    crate::cache::catch_up(&app3).await.unwrap();
    let (st, v) = call(&base3, "c", "POST", "/api/chat/presence", Some(json!({}))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("chat_off")), "{v}");
    assert!(app3.db.chat_seen(&["c".to_owned()]).unwrap().is_empty());
}

/// Live presence (#341): with its Labs flag off the answer has no `live` and
/// a beat's is ignored; with it on, it's `off` until the person says, kept
/// on the account, so their other pages read the same.
#[tokio::test]
async fn live_presence_is_kept_on_the_account_behind_its_flag() {
    let (hub, app, base) = rig().await;
    let v = beat(&base, "a", json!({ "live": "watching" })).await;
    assert!(v.get("live").is_none(), "no flag, no live: {v}");
    assert_eq!(app.db.chat_seen(&["a".to_owned()]).unwrap()["a"].live, "off", "nothing kept without the flag");

    hub.flag_named("account:a", crate::chat::people::LIVE_FLAG, true);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(beat(&base, "a", json!({})).await["live"], "off", "off until they say");
    assert_eq!(beat(&base, "a", json!({ "live": "watching" })).await["live"], "watching");
    // Another page of theirs (any beat) reads it.
    assert_eq!(beat(&base, "a", json!({ "idle": true })).await["live"], "watching");
    assert_eq!(beat(&base, "a", json!({ "live": "off" })).await["live"], "off");
    // It's theirs alone: a teammate's answer says their own.
    assert!(beat(&base, "b", json!({})).await.get("live").is_none(), "b has no flag");
    let (st, v) = call(&base, "a", "POST", "/api/chat/presence", Some(json!({ "live": "live" }))).await;
    assert_eq!(st, 422, "Live isn't kept: {v}");
}

#[tokio::test]
async fn appearing_offline_and_leaving_show_at_once_to_teammates() {
    let (_hub, _app, base) = rig().await;
    beat(&base, "a", json!({})).await;
    let v = beat(&base, "b", json!({})).await;
    assert_eq!(of(&v, "a").as_deref(), Some("active"));

    // a chooses to appear offline: b sees it, a still sees themselves so.
    let mine = beat(&base, "a", json!({ "mode": "offline" })).await;
    assert_eq!((mine["mode"].as_str(), of(&mine, "a").as_deref()), (Some("offline"), Some("offline")));
    let v = beat(&base, "b", json!({})).await;
    assert_eq!(of(&v, "a").as_deref(), Some("offline"));
    // The choice stays across beats that don't name it, and is undone by one that does.
    assert_eq!(beat(&base, "a", json!({})).await["mode"], "offline");
    beat(&base, "a", json!({ "mode": "auto" })).await;
    assert_eq!(of(&beat(&base, "b", json!({})).await, "a").as_deref(), Some("active"));

    // By hand away; an idle page alone doesn't take back an active mark
    // (another page may be in use) but keeps the person seen.
    beat(&base, "a", json!({ "mode": "away" })).await;
    assert_eq!(of(&beat(&base, "b", json!({})).await, "a").as_deref(), Some("away"));
    beat(&base, "a", json!({ "mode": "auto", "idle": true })).await;
    assert_eq!(of(&beat(&base, "b", json!({})).await, "a").as_deref(), Some("active"));

    // A closing page (the beacon) is offline for b on the next look.
    beat(&base, "a", json!({ "gone": true })).await;
    assert_eq!(of(&beat(&base, "b", json!({})).await, "a").as_deref(), Some("offline"));
    // ...and opening it again is active at once.
    beat(&base, "a", json!({})).await;
    assert_eq!(of(&beat(&base, "b", json!({})).await, "a").as_deref(), Some("active"));
}

#[tokio::test]
async fn a_machines_agents_show_the_machines_dot_to_those_who_see_its_owner() {
    let (hub, _app, base) = rig().await;
    hub.set_online("m1", true);
    hub.set_online("m2", true);
    let ask = json!({ "machines": ["m1", "m2", "nope"] });
    // b sees a's machine (a is a teammate) and not d's, though it's up.
    assert_eq!(beat(&base, "b", ask.clone()).await["machines_online"], json!(["m1"]));
    // c shares no team with a: neither.
    assert_eq!(beat(&base, "c", ask.clone()).await["machines_online"], json!([]));
    // d sees its own.
    assert_eq!(beat(&base, "d", ask.clone()).await["machines_online"], json!(["m2"]));
    // Offline when it's down.
    hub.set_online("m1", false);
    assert_eq!(beat(&base, "b", ask).await["machines_online"], json!([]));
}

#[tokio::test]
async fn pictures_go_to_the_account_and_its_teammates_through_control() {
    let (_hub, app, base) = rig().await;
    // Kept fresh, so nothing is fetched from GitHub here.
    for login in ["a", "b", "c"] {
        let pic = Avatar { ctype: "image/png".into(), body: Some(login.as_bytes().to_vec()), fetched: now_ms() };
        app.db.set_chat_avatar(login, &pic).unwrap();
    }
    let get = |who: &'static str, account: &'static str| {
        let base = base.clone();
        async move {
            let r = reqwest::Client::new()
                .get(format!("{base}/api/chat/avatar/{account}"))
                .header("cookie", format!("ilg_session=session-{who}"))
                .send()
                .await
                .unwrap();
            let st = r.status().as_u16();
            let ct = r.headers().get("content-type").map(|v| v.to_str().unwrap().to_owned());
            (st, ct, r.bytes().await.unwrap().to_vec())
        }
    };
    let (st, ct, body) = get("a", "b").await;
    assert_eq!((st, ct.as_deref(), body.as_slice()), (200, Some("image/png"), &b"b"[..]));
    assert_eq!(get("a", "a").await.0, 200, "their own");
    assert_eq!(get("a", "c").await.0, 404, "c is b's teammate, not a's, though a picture is kept");
    assert_eq!(get("d", "a").await.0, 404);
    assert_eq!(get("a", "nobody").await.0, 404);
}

#[test]
fn only_a_github_login_goes_into_the_url() {
    use crate::chat::people::clean_login;
    assert_eq!(clean_login("JHGaylor").as_deref(), Some("jhgaylor"));
    assert_eq!(clean_login("lex-00").as_deref(), Some("lex-00"));
    for bad in ["", "-x", "a/b", "a?b", "a b", "x%2f", &"a".repeat(40)] {
        assert_eq!(clean_login(bad), None, "{bad}");
    }
}
