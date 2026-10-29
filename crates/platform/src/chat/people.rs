//! Chat's people: their pictures and who's around.
//!
//! **Pictures.** Control fetches an account's GitHub avatar (by the login
//! the hub knows it by), keeps it a day, and serves it from
//! `GET /api/chat/avatar/{account}`, so a page never loads
//! githubusercontent itself and the CSP needn't open to it. Only the
//! account itself and those who share a team with it get it: a picture
//! isn't a way to learn that an account exists.
//!
//! **Presence.** A page beats every 15 s ([`beat`]) and control keeps
//! only when it last heard from each account, and when from a page that
//! was in use. What shows ([`state_of`]) is that and the clock: a stale
//! beat is offline, a fresh one active if a page was used in the last
//! [`ACTIVE`] and away if not, and what the person chose
//! ([`PresenceMode`]) overrides. It's visible to the person and to those
//! who share a team with them, and to nobody else ([`visible`]); a locked
//! team's non-owners aren't in it, as everywhere in chat. A machine's
//! agents show as the machine's online state from the hub, to those who
//! see its owner or its team.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use arugula_control_common::{ApiError, err};
use arugula_control_wire::chat::{ChatBeat, ChatPresence, ChatPresent, PresenceMode, PresenceState};
use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use super::{J, gate};
use crate::{
    Platform,
    auth::Person,
    db::chat_people::{Avatar, Seen},
};

/// How often an open stream keeps its person seen (the page beats every
/// 15 s on top of it, and says whether it's in use).
pub const STREAM_BEAT: Duration = Duration::from_secs(20);
/// A beat older than this is a page that closed.
const STALE_MS: u64 = 40_000;
/// A page used this recently makes its person active.
const ACTIVE_MS: u64 = 10 * 60_000;
/// Machines a beat may ask about.
const MAX_MACHINES: usize = 64;

/// How long a fetched picture, and a failed fetch, are kept.
const AVATAR_FRESH_MS: u64 = 24 * 3_600_000;
const AVATAR_RETRY_MS: u64 = 3_600_000;
/// The biggest picture kept (GitHub's at 128 px are a few KB).
const AVATAR_MAX: usize = 256 << 10;
const AVATAR_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// What `seen` shows as at `now`.
pub fn state_of(seen: &Seen, now: u64) -> PresenceState {
    if seen.mode == "offline" || seen.seen == 0 || now.saturating_sub(seen.seen) > STALE_MS {
        return PresenceState::Offline;
    }
    if seen.mode == "away" || now.saturating_sub(seen.active) > ACTIVE_MS || seen.active == 0 {
        return PresenceState::Away;
    }
    PresenceState::Active
}

fn mode_of(m: &str) -> PresenceMode {
    match m {
        "away" => PresenceMode::Away,
        "offline" => PresenceMode::Offline,
        _ => PresenceMode::Auto,
    }
}

fn mode_name(m: PresenceMode) -> &'static str {
    match m {
        PresenceMode::Auto => "auto",
        PresenceMode::Away => "away",
        PresenceMode::Offline => "offline",
    }
}

/// Whom `account` sees: itself, and everyone in a team it's in now, and
/// the teams it is in (for machines).
pub(crate) async fn visible(app: &Platform, account: &str) -> anyhow::Result<(HashSet<String>, HashSet<String>)> {
    let hub = &*app.hub;
    let ids = app.cache.teams_of(hub, account).await?;
    let teams = app.cache.teams(hub, &ids).await?;
    let mut people = HashSet::from([account.to_owned()]);
    let mut mine = HashSet::new();
    for t in teams.values().filter(|t| t.role(account).is_some()) {
        mine.insert(t.id.clone());
        people.extend(t.members.iter().filter(|m| t.role(&m.account).is_some()).map(|m| m.account.clone()));
    }
    Ok((people, mine))
}

/// `POST /api/chat/presence`: a page's heartbeat, and who's around.
pub async fn beat(State(app): State<Arc<Platform>>, s: Person, Json(b): Json<ChatBeat>) -> J<ChatPresence> {
    gate(&app, &s.account).await?;
    let now = now_ms();
    app.db.chat_beat(&s.account, now, !b.idle, b.gone)?;
    if let Some(m) = b.mode {
        app.db.set_chat_presence_mode(&s.account, mode_name(m))?;
    }
    Ok(Json(presence(&app, &s.account, &b.machines, now).await?))
}

/// What `account` sees of who's around at `now`, and which of `machines`
/// are online (of those it sees).
pub(crate) async fn presence(
    app: &Platform,
    account: &str,
    machines: &[String],
    now: u64,
) -> anyhow::Result<ChatPresence> {
    let (people, teams) = visible(app, account).await?;
    let mut ids: Vec<String> = people.iter().cloned().collect();
    ids.sort();
    let kept: HashMap<String, Seen> = app.db.chat_seen(&ids)?;
    let mine = kept.get(account).map(|k| mode_of(&k.mode)).unwrap_or_default();
    let people = ids
        .into_iter()
        .map(|a| {
            let state = kept.get(&a).map_or(PresenceState::Offline, |k| state_of(k, now));
            ChatPresent { account: a, state }
        })
        .collect::<Vec<_>>();
    let seen: HashSet<&str> = people.iter().map(|p| p.account.as_str()).collect();
    let asked: Vec<String> = machines.iter().take(MAX_MACHINES).cloned().collect();
    let daemons = app.cache.daemons(&*app.hub, &asked).await?;
    let may: Vec<String> = asked
        .into_iter()
        .filter(|m| {
            daemons.get(m).is_some_and(|d| {
                seen.contains(d.account.as_str()) || d.team.as_ref().is_some_and(|t| teams.contains(t))
            })
        })
        .collect();
    // The hub not saying is offline, not an error: presence is a nicety.
    let machines_online = if may.is_empty() { vec![] } else { app.hub.machines_online(&may).await.unwrap_or_default() };
    Ok(ChatPresence { mode: mine, people, machines_online })
}

/// `GET /api/chat/avatar/{account}`: the account's picture, if the reader
/// is it or shares a team with it, and it has one. Anything else is a 404,
/// for which the page keeps its letters.
pub async fn avatar(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(account): Path<String>,
) -> Result<Response, ApiError> {
    gate(&app, &s.account).await?;
    let gone = || err(StatusCode::NOT_FOUND, "no picture");
    if !visible(&app, &s.account).await?.0.contains(&account) {
        return Err(gone());
    }
    let login = app
        .cache
        .accounts(&*app.hub, std::slice::from_ref(&account))
        .await?
        .remove(&account)
        .map(|a| a.login)
        .unwrap_or_default();
    let Some(login) = clean_login(&login) else { return Err(gone()) };
    let got = picture(&app, &login, now_ms()).await?;
    let Some(Avatar { ctype, body: Some(body), .. }) = got else { return Err(gone()) };
    let mut r = body.into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(&ctype).map_err(|e| anyhow::anyhow!("{e}"))?);
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=3600"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    Ok(r)
}

/// A GitHub login as a lowercase path piece, or `None` when it isn't one
/// (it goes into a URL).
pub(crate) fn clean_login(login: &str) -> Option<String> {
    let ok = !login.is_empty()
        && login.len() <= 39
        && login.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !login.starts_with('-');
    ok.then(|| login.to_ascii_lowercase())
}

/// The picture of `login`, fetched if the one kept is old. A failed fetch
/// serves what was kept (or nothing) and tries again in an hour.
async fn picture(app: &Platform, login: &str, now: u64) -> anyhow::Result<Option<Avatar>> {
    let kept = app.db.chat_avatar(login)?;
    if let Some(k) = &kept {
        let age = now.saturating_sub(k.fetched);
        if age < if k.body.is_some() { AVATAR_FRESH_MS } else { AVATAR_RETRY_MS } {
            return Ok(kept);
        }
    }
    let next = match fetch(app, login).await {
        Ok(Some((ctype, body))) => Avatar { ctype, body: Some(body), fetched: now },
        // GitHub had none, or wasn't reachable: what was kept stays for an
        // hour more (it's dated so), and nothing is asked for again in one.
        Ok(None) | Err(_) => match kept {
            Some(Avatar { ctype, body: Some(body), .. }) => {
                Avatar { ctype, body: Some(body), fetched: now.saturating_sub(AVATAR_FRESH_MS - AVATAR_RETRY_MS) }
            }
            _ => Avatar { ctype: String::new(), body: None, fetched: now },
        },
    };
    app.db.set_chat_avatar(login, &next)?;
    Ok(Some(next))
}

/// GitHub's 128-px picture of `login` (crisp at 3x on a 40-px avatar). The
/// host is fixed; the login was checked ([`clean_login`]).
async fn fetch(app: &Platform, login: &str) -> anyhow::Result<Option<(String, Vec<u8>)>> {
    let r =
        app.http.get(format!("https://github.com/{login}.png?size=128")).timeout(Duration::from_secs(5)).send().await?;
    if !r.status().is_success() {
        return Ok(None);
    }
    let ctype = r
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if !AVATAR_TYPES.contains(&ctype.as_str()) || r.content_length().is_some_and(|n| n as usize > AVATAR_MAX) {
        return Ok(None);
    }
    let body = r.bytes().await?;
    Ok((body.len() <= AVATAR_MAX).then(|| (ctype, body.to_vec())))
}
