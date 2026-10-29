//! Notification levels and do not disturb (#113, #115): the decisions on
//! their own, in and out of a time zone's midnight, then through the
//! server: the prefs' route, and other notices. Which pushes chat asks the
//! hub for at each level is `arugula-platform-chat`'s `notify_tests`.

use std::sync::Arc;

use arugula_control_wire::chat::{ChatPushWhy as Why, NotifyLevel as L, QuietHours};
use arugula_e2e::now_ms;
use jiff::Timestamp;
use serde_json::json;

use super::*;
use crate::{
    chat_tests::{TestHub, call, seed, serve},
    db::notify::Prefs,
};

/// An instant, in ms.
fn at(iso: &str) -> u64 {
    iso.parse::<Timestamp>().unwrap().as_millisecond() as u64
}

fn nights() -> Vec<QuietHours> {
    // Weeknights, 22:00 to 08:00.
    vec![QuietHours { days: vec![1, 2, 3, 4, 5], from: 22 * 60, to: 8 * 60 }]
}

#[test]
fn the_levels_let_through_what_they_say() {
    // A keyword alert (#114) is a mention the person asked for.
    for (level, mention, dm, message) in
        [(L::All, true, true, true), (L::Mentions, true, true, false), (L::Nothing, false, false, false)]
    {
        assert_eq!(
            (lets(level, Why::Mention), lets(level, Why::Keyword), lets(level, Why::Dm), lets(level, Why::Message)),
            (mention, mention, dm, message),
            "{level:?}"
        );
    }
}

#[test]
fn the_defaults_are_all_for_dms_and_mentions_for_channels() {
    let p = Prefs::default();
    assert_eq!(default_level(&p, "dm"), L::All);
    assert_eq!(default_level(&p, "channel"), L::Mentions);
    assert_eq!(default_level(&p, "pane"), L::Mentions);
    let p = Prefs { channels: L::All, dms: L::Nothing, ..Prefs::default() };
    assert_eq!((default_level(&p, "channel"), default_level(&p, "dm")), (L::All, L::Nothing));
}

/// What gets pushed at each level, for each reason, in each kind of room.
#[test]
fn a_push_follows_the_level_for_the_room() {
    let p = Prefs::default();
    let now = at("2026-10-26T12:00:00Z");
    let go = |kind: &str, own, why| chat_push(&p, kind, own, why, now, false);
    // A channel on its default: a mention, not a message.
    assert!(go("channel", None, Why::Mention));
    assert!(!go("channel", None, Why::Message));
    // A DM on its default: everything.
    assert!(go("dm", None, Why::Dm));
    // The room's own level wins over the account's.
    assert!(go("channel", Some(L::All), Why::Message));
    assert!(!go("channel", Some(L::Nothing), Why::Mention));
    assert!(!go("dm", Some(L::Nothing), Why::Dm));
    assert!(go("dm", Some(L::Mentions), Why::Dm), "mentions only still lets a DM through");
    // And an account default of all makes every channel loud.
    let loud = Prefs { channels: L::All, ..Prefs::default() };
    assert!(chat_push(&loud, "channel", None, Why::Message, now, false));
    assert!(!chat_push(&loud, "channel", Some(L::Mentions), Why::Message, now, false));
}

#[test]
fn a_snooze_holds_pushes_back_until_it_ends() {
    let now = at("2026-10-26T12:00:00Z");
    let hour = 3600 * 1000;
    let p = Prefs { snooze_until: Some(now + hour), ..Prefs::default() };
    assert!(paused(&p, now));
    assert!(paused(&p, now + hour - 1));
    assert!(!paused(&p, now + hour), "over at the minute it says");
    assert!(!chat_push(&p, "dm", None, Why::Dm, now + 30 * 60 * 1000, false));
    assert!(chat_push(&p, "dm", None, Why::Dm, now + hour + 1, false));
    assert!(!chat_push(&p, "channel", None, Why::Mention, now, false));
}

#[test]
fn notify_anyway_gets_a_dm_through_but_not_a_muted_room() {
    let now = at("2026-10-26T12:00:00Z");
    let p = Prefs { snooze_until: Some(now + 1000), ..Prefs::default() };
    assert!(chat_push(&p, "dm", None, Why::Dm, now, true));
    assert!(!chat_push(&p, "dm", Some(L::Nothing), Why::Dm, now, true));
    assert!(chat_push(&p, "dm", None, Why::Mention, now, true), "named in a DM is still a DM");
    assert!(!chat_push(&p, "channel", None, Why::Mention, now, true), "only a DM's sender may");
}

/// The schedule is read in the person's zone. Berlin is an hour ahead of
/// UTC once summer time ends (2026-10-25), so Monday 21:30 UTC is already
/// 22:30 there.
#[test]
fn quiet_hours_are_read_in_the_accounts_time_zone() {
    let q = nights();
    let mon_2130_utc = at("2026-10-26T21:30:00Z");
    assert!(!in_quiet_hours(&q, "UTC", mon_2130_utc));
    assert!(in_quiet_hours(&q, "Europe/Berlin", mon_2130_utc));
    // Los Angeles is behind: the same instant is Monday 14:30.
    assert!(!in_quiet_hours(&q, "America/Los_Angeles", mon_2130_utc));
    // The other way: Monday 05:30 UTC is Sunday 21:30 in LA (not a window
    // day) and Monday 06:30 in Berlin (the Sunday-night window isn't set).
    let mon_0530_utc = at("2026-10-26T05:30:00Z");
    assert!(!in_quiet_hours(&q, "America/Los_Angeles", mon_0530_utc));
    assert!(!in_quiet_hours(&q, "Europe/Berlin", mon_0530_utc));
    // Tokyo is nine ahead: 14:30 UTC on Monday is 23:30 there.
    assert!(in_quiet_hours(&q, "Asia/Tokyo", at("2026-10-26T14:30:00Z")));
    // A zone nobody knows is UTC.
    assert!(!in_quiet_hours(&q, "Not/AZone", mon_2130_utc));
    assert!(in_quiet_hours(&q, "Not/AZone", at("2026-10-26T22:30:00Z")));
}

#[test]
fn a_window_past_midnight_belongs_to_the_day_it_starts_on() {
    let q = nights();
    // Tuesday 07:59 is Monday night's; Saturday 07:59 is Friday night's.
    assert!(in_quiet_hours(&q, "UTC", at("2026-10-27T07:59:00Z")));
    assert!(!in_quiet_hours(&q, "UTC", at("2026-10-27T08:00:00Z")));
    assert!(in_quiet_hours(&q, "UTC", at("2026-10-31T07:59:00Z")));
    // Saturday night isn't a window day; Sunday morning is Saturday's.
    assert!(!in_quiet_hours(&q, "UTC", at("2026-10-31T23:00:00Z")));
    assert!(!in_quiet_hours(&q, "UTC", at("2026-11-01T03:00:00Z")));
    // Friday night runs into Saturday morning, and the start is inclusive.
    assert!(in_quiet_hours(&q, "UTC", at("2026-10-30T22:00:00Z")));
    assert!(!in_quiet_hours(&q, "UTC", at("2026-10-30T21:59:00Z")));
    // Monday 07:00 is Sunday night's, not a window day.
    assert!(!in_quiet_hours(&q, "UTC", at("2026-10-26T07:00:00Z")));
}

#[test]
fn a_same_day_window_and_the_clocks_going_back() {
    let lunch = vec![QuietHours { days: vec![3], from: 12 * 60, to: 13 * 60 }];
    // Wednesday 2026-10-28.
    assert!(in_quiet_hours(&lunch, "UTC", at("2026-10-28T12:00:00Z")));
    assert!(!in_quiet_hours(&lunch, "UTC", at("2026-10-28T13:00:00Z")));
    assert!(!in_quiet_hours(&lunch, "UTC", at("2026-10-27T12:30:00Z")));
    // The night the clocks go back in Berlin (Sunday 2026-10-25, 03:00 to
    // 02:00): 00:30 UTC is 02:30 CEST, and 01:30 UTC is 02:30 CET: the
    // same wall time twice, so a 02:00-03:00 window holds both.
    let night = vec![QuietHours { days: vec![7], from: 2 * 60, to: 3 * 60 }];
    assert!(in_quiet_hours(&night, "Europe/Berlin", at("2026-10-25T00:30:00Z")));
    assert!(in_quiet_hours(&night, "Europe/Berlin", at("2026-10-25T01:30:00Z")));
    assert!(!in_quiet_hours(&night, "Europe/Berlin", at("2026-10-25T02:30:00Z")));
}

#[test]
fn a_schedule_pauses_a_push_only_inside_its_windows() {
    let p = Prefs { quiet: nights(), tz: "Europe/Berlin".into(), ..Prefs::default() };
    let (inside, outside) = (at("2026-10-26T21:30:00Z"), at("2026-10-26T12:00:00Z"));
    assert!(!chat_push(&p, "dm", None, Why::Dm, inside, false));
    assert!(chat_push(&p, "dm", None, Why::Dm, outside, false));
}

// ------------------------------------------------------------ the server

async fn setup() -> (Arc<TestHub>, Arc<Platform>, String) {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    (hub, app, base)
}

#[tokio::test]
async fn the_schedule_is_read_in_the_accounts_zone_and_checked() {
    let (_, app, base) = setup().await;
    let all_day = json!([{ "days": [1, 2, 3, 4, 5, 6, 7], "from": 0, "to": 1439 }]);
    let (st, p) =
        call(&base, "b", "POST", "/api/notify/prefs", Some(json!({ "tz": "Europe/Berlin", "quiet": all_day }))).await;
    assert_eq!((st, &p["tz"]), (200, &json!("Europe/Berlin")), "{p}");
    let saved = app.db.notify_prefs("b").unwrap();
    assert_eq!((saved.tz.as_str(), saved.quiet.len()), ("Europe/Berlin", 1));
    // Nonsense is refused, and nothing changes.
    for bad in [
        json!({ "tz": "Mars/Olympus" }),
        json!({ "quiet": [{ "days": [8], "from": 0, "to": 60 }] }),
        json!({ "quiet": [{ "days": [], "from": 0, "to": 60 }] }),
        json!({ "quiet": [{ "days": [1], "from": 600, "to": 600 }] }),
        json!({ "quiet": [{ "days": [1], "from": 0, "to": 1500 }] }),
        json!({ "snooze_until": u64::MAX / 2 }),
    ] {
        let (st, e) = call(&base, "b", "POST", "/api/notify/prefs", Some(bad.clone())).await;
        assert_eq!(st, 400, "{bad}: {e}");
    }
    assert_eq!(app.db.notify_prefs("b").unwrap(), saved);
}

#[tokio::test]
async fn do_not_disturb_covers_other_notices_only_when_asked() {
    let (_, app, _) = setup().await;
    let until = now_ms() + 3600 * 1000;
    let p = Prefs { snooze_until: Some(until), ..Prefs::default() };
    app.db.set_notify_prefs("b", &p).unwrap();
    assert!(notice_goes(&app, "b"), "chat's pause is chat's alone by default");
    app.db.set_notify_prefs("b", &Prefs { all_notices: true, ..p.clone() }).unwrap();
    assert!(!notice_goes(&app, "b"));
    assert!(notice_goes(&app, "a"), "someone who chose nothing");
    app.db.set_notify_prefs("b", &Prefs { all_notices: true, snooze_until: Some(1), ..p }).unwrap();
    assert!(notice_goes(&app, "b"), "once it ends");
}

#[test]
fn the_migration_carries_muted_over_to_nothing() {
    // A database as 0041 left it, with a muted room and an open one.
    let mut c = rusqlite::Connection::open_in_memory().unwrap();
    crate::db::migrate::run_to(&mut c, 41).unwrap();
    c.execute_batch("INSERT INTO chat_reads (room, account, seq, muted) VALUES ('r1', 'x', 3, 1), ('r2', 'x', 0, 0)")
        .unwrap();
    assert_eq!(crate::db::migrate::run_to(&mut c, 42).unwrap().ran, [42]);
    let level = |room: &str| -> Option<String> {
        c.query_row("SELECT level FROM chat_reads WHERE room = ?1", [room], |r| r.get(0)).unwrap()
    };
    assert_eq!((level("r1").as_deref(), level("r2")), (Some("nothing"), None));
}

/// A reply in a followed thread goes by the thread default (#106), apart
/// from the room's level, which only a `nothing` for the room overrides.
#[test]
fn a_followed_threads_reply_goes_by_the_thread_default() {
    let now = at("2026-10-26T12:00:00Z");
    let p = Prefs::default();
    // Channels are on mentions, and a thread's replies still go (all).
    assert!(chat_push(&p, "channel", None, Why::Thread, now, false));
    assert!(chat_push(&p, "channel", Some(L::Mentions), Why::Thread, now, false));
    assert!(!chat_push(&p, "channel", Some(L::Nothing), Why::Thread, now, false), "a muted room stays quiet");
    let quiet = Prefs { threads: L::Mentions, ..Prefs::default() };
    assert!(!chat_push(&quiet, "channel", None, Why::Thread, now, false), "mentions in it come as mentions");
    assert!(chat_push(&quiet, "channel", None, Why::Mention, now, false));
    let off = Prefs { threads: L::Nothing, ..Prefs::default() };
    assert!(!chat_push(&off, "dm", Some(L::All), Why::Thread, now, false));
    // Not while paused.
    let snoozed = Prefs { snooze_until: Some(now + 60_000), ..Prefs::default() };
    assert!(!chat_push(&snoozed, "channel", None, Why::Thread, now, false));
}
