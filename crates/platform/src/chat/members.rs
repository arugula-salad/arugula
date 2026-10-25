//! Channel membership (#118) and leaving a DM (#117): the browser of a
//! team's public channels, joining and leaving, and who adds and removes
//! members. Who sees a room is `chat::sight`; this is what changes it. A
//! change tells open pages through `Hub::room`, which moves every stream's
//! rooms on every machine: the rooms a person sees follow the database, not
//! the hub's cache (a roster change reaches them through the change feed as
//! before; membership is the platform's own).

use std::sync::Arc;

use arugula_control_common::err;
use arugula_control_wire::chat::{ChatAddMembers, ChatChannel, ChatChannels, ChatRoom};
use arugula_e2e::team::TeamRole;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use super::{Facts, J, Room, gate, listed, room_for, team_scope};
use crate::{Platform, auth::Person};

/// Whether `account` adds and removes `room`'s members:
/// the creator of a channel, and its team's owners.
pub(super) fn may_manage(facts: &Facts, room: &Room, account: &str) -> bool {
    let Some(team) = room.scope.strip_prefix("team:").filter(|_| room.kind == "channel") else { return false };
    room.created_by == account || facts.role(team, account) == Some(TeamRole::Owner)
}

#[derive(Debug, Deserialize)]
pub struct Browse {
    #[serde(default)]
    q: String,
}

/// `GET /api/chat/teams/{team}/channels?q=`: the team's public channels
/// with their topic and how many of the team are in each, and whether the
/// reader is. Private channels aren't here, not even for their members
/// (those have them in their rooms).
pub async fn channels(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(team): Path<String>,
    Query(q): Query<Browse>,
) -> J<ChatChannels> {
    gate(&app, &s.account).await?;
    let scope = team_scope(&team);
    let rooms = app.db.chat_team_channels(&scope)?;
    let facts = Facts::page(&app, Some(&s.account), &rooms.iter().collect::<Vec<_>>()).await?;
    let t = app.cache.team(&*app.hub, &team).await?;
    if t.as_ref().and_then(|t| t.role(&s.account)).is_none() {
        return Err(err(StatusCode::NOT_FOUND, "no such team"));
    }
    let word = q.q.trim().trim_start_matches('#').to_lowercase();
    let ids: Vec<&str> = rooms.iter().map(|r| r.id.as_str()).collect();
    let mut topics = app.db.chat_topics_of(&ids)?;
    let mut out = Vec::new();
    for r in &rooms {
        let meta = facts.meta(&r.id);
        let topic = topics.remove(&r.id).map(|t| t.topic).unwrap_or_default();
        if meta.private || (!word.is_empty() && !r.name.contains(&word) && !topic.to_lowercase().contains(&word)) {
            continue;
        }
        let members = facts.members(&app, &r.id)?;
        // Of the team now: one who left it isn't counted.
        let members: Vec<&String> = members.iter().filter(|a| facts.role(&team, a).is_some()).collect();
        out.push(ChatChannel {
            id: r.id.clone(),
            name: r.name.clone(),
            topic,
            members: members.len() as u64,
            joined: members.iter().any(|a| **a == s.account),
            archived: r.archived.is_some(),
        });
    }
    Ok(Json(ChatChannels { channels: out }))
}

/// `POST /api/chat/rooms/{id}/join`: into a public channel of a team the
/// reader is in. A private one is 404, as if it weren't there.
pub async fn join(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> J<ChatRoom> {
    gate(&app, &s.account).await?;
    let gone = || err(StatusCode::NOT_FOUND, "no such room");
    let room = app.db.chat_room(&id)?.filter(|r| r.kind == "channel").ok_or_else(gone)?;
    let facts = Facts::of(&app, Some(&s.account), &[&room]).await?;
    let team = room.scope.strip_prefix("team:").ok_or_else(gone)?;
    if facts.role(team, &s.account).is_none() {
        return Err(gone());
    }
    let member = facts.members(&app, &room.id)?.contains(&s.account);
    if facts.meta(&room.id).private && !member {
        return Err(gone());
    }
    if app.db.add_chat_member(&room.id, &s.account, 0)? {
        app.chat.room(&app.db, &room.id);
    }
    Ok(Json(listed(&app, &facts, &s.account, &room, 0)?))
}

/// `POST /api/chat/rooms/{id}/leave`: out of a channel (public or private)
/// or a DM. A public channel stays in the browser; a DM, for the rest. The
/// last one out of a private channel can't leave it, which would strand it
/// where nobody can find it.
pub async fn leave(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
) -> Result<StatusCode, arugula_control_common::ApiError> {
    gate(&app, &s.account).await?;
    let (room, _, facts) = room_for(&app, &s.account, &id).await?;
    if !matches!(room.kind.as_str(), "channel" | "dm") {
        return Err(err(StatusCode::BAD_REQUEST, "only a channel or a DM is left"));
    }
    if room.kind == "channel" && facts.meta(&room.id).private && facts.members(&app, &room.id)?.len() <= 1 {
        return Err(err(
            StatusCode::CONFLICT,
            "you're the last one in this private channel: add someone first, or have an owner archive it",
        ));
    }
    if app.db.remove_chat_member(&room.id, &s.account)? {
        app.chat.room(&app.db, &room.id);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/chat/rooms/{id}/members`: add people of the team to a channel
/// (404 for anyone not in it). A DM's people are fixed: adding is a new
/// DM (#117).
pub async fn add(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(b): Json<ChatAddMembers>,
) -> J<ChatRoom> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    if !may_manage(&facts, &room, &s.account) {
        return Err(err(StatusCode::FORBIDDEN, "a channel's creator and its team's owners add its members"));
    }
    let team = room.scope.strip_prefix("team:").unwrap_or_default();
    if b.accounts.iter().any(|a| facts.role(team, a).is_none()) {
        return Err(err(StatusCode::NOT_FOUND, "nobody by that account in this team"));
    }
    let mut changed = false;
    for a in &b.accounts {
        changed |= app.db.add_chat_member(&room.id, a, 0)?;
    }
    if changed {
        app.chat.room(&app.db, &room.id);
    }
    Ok(Json(listed(&app, &facts, &s.account, &room, from)?))
}

/// `DELETE /api/chat/rooms/{id}/members/{account}`: take someone out of a
/// channel. The same person asking is [`leave`].
pub async fn remove(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, account)): Path<(String, String)>,
) -> J<ChatRoom> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    if !may_manage(&facts, &room, &s.account) {
        return Err(err(StatusCode::FORBIDDEN, "a channel's creator and its team's owners remove its members"));
    }
    if facts.meta(&room.id).private && facts.members(&app, &room.id)?.len() <= 1 {
        return Err(err(StatusCode::CONFLICT, "that's the last one in this private channel"));
    }
    if app.db.remove_chat_member(&room.id, &account)? {
        app.chat.room(&app.db, &room.id);
    }
    Ok(Json(listed(&app, &facts, &s.account, &room, from)?))
}
