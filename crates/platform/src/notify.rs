//! How loud chat is (#113) and do not disturb (#115): whether a push goes
//! to a person, from their level for the room, their snooze and their
//! quiet hours. The decisions are plain functions of the settings and the
//! time, so each level, and each side of a time zone's midnight, is tested
//! without a server. Storage is `db::notify`.
//!
//! - A room's level is the person's own choice for it, else their default
//!   for its kind: DMs all, channels mentions. Panes', sessions' and
//!   objects' threads are mentions too until following exists (the
//!   `threads` default is for the threads a person follows, and is kept
//!   now so that setting doesn't have to move later).
//! - `mentions` lets through a mention and a DM; `all` every message;
//!   `nothing` none.
//! - Do not disturb holds chat's pushes back, whatever the level (unreads
//!   still count: it changes only what is pushed). With `all_notices` it
//!   holds back the platform's other pushes as well (the inbox, deploy
//!   approvals). A notice about the account itself (a token, a device) is
//!   never held back. A DM's sender may "notify anyway", once, as Slack
//!   allows; a person who chose `nothing` for the room still isn't pushed.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::chat::{ChatPushWhy, NotifyLevel, NotifyPrefs, NotifySet, QuietHours};
use arugula_e2e::now_ms;
use axum::{Json, extract::State, http::StatusCode};
use jiff::{Timestamp, tz::TimeZone};

use crate::{Platform, auth::Person, db::notify::Prefs};

/// The level a person gets in a room of this kind with none of their own.
pub fn default_level(prefs: &Prefs, room_kind: &str) -> NotifyLevel {
    match room_kind {
        "dm" => prefs.dms,
        "channel" => prefs.channels,
        // The hook for followed threads (`prefs.threads`).
        _ => NotifyLevel::Mentions,
    }
}

/// Whether a level lets a push of this reason through.
pub fn lets(level: NotifyLevel, why: ChatPushWhy) -> bool {
    match level {
        NotifyLevel::All => true,
        // A keyword is a mention the person asked for (#114).
        NotifyLevel::Mentions => matches!(why, ChatPushWhy::Mention | ChatPushWhy::Keyword | ChatPushWhy::Dm),
        NotifyLevel::Nothing => false,
    }
}

/// In `tz` (an IANA name; an unknown one is UTC), the ISO weekday (1
/// Monday … 7 Sunday) and the minute of the day at `at` (ms).
fn local(tz: &str, at: u64) -> (u8, u16) {
    let zone = TimeZone::get(tz).unwrap_or(TimeZone::UTC);
    let t = Timestamp::from_millisecond(i64::try_from(at).unwrap_or(i64::MAX)).unwrap_or(Timestamp::UNIX_EPOCH);
    let z = t.to_zoned(zone);
    (z.weekday().to_monday_one_offset() as u8, (z.hour() as u16) * 60 + z.minute() as u16)
}

/// Whether `at` (ms) is inside one of the quiet windows, read in `tz`.
pub fn in_quiet_hours(quiet: &[QuietHours], tz: &str, at: u64) -> bool {
    if quiet.is_empty() {
        return false;
    }
    let (day, minute) = local(tz, at);
    let yesterday = (day + 5) % 7 + 1;
    quiet.iter().any(|q| {
        if q.from < q.to {
            q.days.contains(&day) && (q.from..q.to).contains(&minute)
        } else {
            // Past midnight: the evening is the window's own day's, the
            // morning after belongs to the day before.
            (q.days.contains(&day) && minute >= q.from) || (q.days.contains(&yesterday) && minute < q.to)
        }
    })
}

/// Do not disturb is on at `at` (ms): snoozed, or inside quiet hours.
pub fn paused(prefs: &Prefs, at: u64) -> bool {
    prefs.snooze_until.is_some_and(|t| t > at) || in_quiet_hours(&prefs.quiet, &prefs.tz, at)
}

/// A chat push for `why`, to a person whose own level for the room is
/// `own`, at `at` (ms): does it go? `anyway` is the sender's "notify
/// anyway" on a DM.
pub fn chat_push(
    prefs: &Prefs,
    room_kind: &str,
    own: Option<NotifyLevel>,
    why: ChatPushWhy,
    at: u64,
    anyway: bool,
) -> bool {
    let level = own.unwrap_or_else(|| default_level(prefs, room_kind));
    if !lets(level, why) {
        return false;
    }
    !paused(prefs, at) || (anyway && room_kind == "dm" && why == ChatPushWhy::Dm)
}

/// Whether the platform's other (non-chat) notice goes to `account` now:
/// held back only for someone who asked do not disturb to cover everything.
pub fn notice_goes(app: &Platform, account: &str) -> bool {
    match app.db.notify_prefs(account) {
        Ok(p) => !(p.all_notices && paused(&p, now_ms())),
        // Failing to read a setting is no reason to drop a notice.
        Err(_) => true,
    }
}

fn shown(p: Prefs, at: u64) -> NotifyPrefs {
    NotifyPrefs {
        channels: p.channels,
        dms: p.dms,
        threads: p.threads,
        snooze_until: p.snooze_until.filter(|t| *t > at),
        paused: paused(&p, at),
        tz: p.tz,
        quiet: p.quiet,
        all_notices: p.all_notices,
    }
}

/// `GET /api/notify/prefs`.
pub async fn prefs(State(app): State<Arc<Platform>>, s: Person) -> Result<Json<NotifyPrefs>, ApiError> {
    Ok(Json(shown(app.db.notify_prefs(&s.account)?, now_ms())))
}

/// A window the schedule can hold: real weekdays, minutes in a day.
fn quiet_ok(q: &QuietHours) -> bool {
    !q.days.is_empty() && q.days.iter().all(|d| (1..=7).contains(d)) && q.from < 1440 && q.to <= 1440 && q.from != q.to
}

/// `POST /api/notify/prefs`: what's given changes.
pub async fn set_prefs(
    State(app): State<Arc<Platform>>,
    s: Person,
    Json(b): Json<NotifySet>,
) -> Result<Json<NotifyPrefs>, ApiError> {
    let mut p = app.db.notify_prefs(&s.account)?;
    p.channels = b.channels.unwrap_or(p.channels);
    p.dms = b.dms.unwrap_or(p.dms);
    p.threads = b.threads.unwrap_or(p.threads);
    p.all_notices = b.all_notices.unwrap_or(p.all_notices);
    if let Some(t) = b.snooze_until {
        // A year is longer than any snooze: a mistake, not a choice.
        if t > now_ms() + 366 * 24 * 3600 * 1000 {
            return Err(err(StatusCode::BAD_REQUEST, "snooze for less than a year"));
        }
        p.snooze_until = (t > 0).then_some(t);
    }
    if let Some(tz) = b.tz {
        if TimeZone::get(&tz).is_err() {
            return Err(err(StatusCode::BAD_REQUEST, "tz: an IANA time zone name, like Europe/Berlin"));
        }
        p.tz = tz;
    }
    if let Some(q) = b.quiet {
        if q.len() > 14 || !q.iter().all(quiet_ok) {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "quiet hours: at most 14 windows, on days 1 to 7, in minutes of a day",
            ));
        }
        p.quiet = q;
    }
    app.db.set_notify_prefs(&s.account, &p)?;
    Ok(Json(shown(p, now_ms())))
}

#[cfg(test)]
#[path = "notify_tests.rs"]
mod tests;
