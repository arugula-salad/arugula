//! A mention of a machine whose agents can't read the room (#578). A room
//! grant stays the privacy boundary: the mention reaches nobody. The sender
//! is told why (#572), and the machine's owner is asked.
//!
//! - **The card.** Control tells the machine's daemon (`agents.grant_ask`),
//!   which raises a card in its owner's swarm: grant, not now, or never for
//!   the room. The mention waits as `grant_pending`, which the sender reads
//!   as "not granted".
//! - **The grant is the owner's act.** The card's Grant is the owner's own
//!   `POST /api/grants`, over their session, for the room's project. Nothing
//!   here grants, and no daemon-authenticated route does.
//! - **Delivery.** [`granted`] runs after any grant: each pending mention
//!   whose machine now hears its room is claimed (`sending`) and told to the
//!   daemon, once. One older than a week is dropped (`not_granted`).
//! - **Never.** [`never`] is the owner's, over their session too: what
//!   waited is `not_granted` for good, and later mentions there raise no
//!   card. A grant over the room clears it.

use std::sync::Arc;

use arugula_control_common::err;
use arugula_control_wire::{
    chat::ChatGrantNever,
    forge::{AgentGrantAsk, DaemonOp},
};
use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use super::{
    Facts, J, Person, Platform, Room, Told, agent_scope, announce_status, ask_daemon, gate, hears, room_agents,
    room_for, send_claimed, team_machines,
};
use crate::{
    db::chat_mention_status::{GRANT_PENDING, GRANT_PENDING_MS, MentionStatus, NOT_GRANTED},
    grants::AgentScope,
    hub::Daemon,
};

/// The longest first line a card quotes, in characters.
const LINE: usize = 120;

/// The most (room, machine) pairs [`granted`] looks at.
const PAIRS_MAX: usize = 200;

/// The machines of the team a post named (`words`, lowercased, as
/// [`unreached_why`](super::unreached_why) reads them) whose agents can't
/// read the room, each once. Not stored as mentioned: the status row is the
/// mention, and its message's text is what the grant delivers. Nothing for a
/// room with no team.
pub(super) async fn unheard(app: &Platform, facts: &Facts, room: &Room, words: &[String]) -> Vec<Daemon> {
    if words.is_empty() {
        return vec![];
    }
    let machines = match team_machines(app, facts, room).await {
        Ok(Some((_, m))) => m,
        Ok(None) => return vec![],
        Err(e) => {
            tracing::warn!(error = %e, "chat: whom to ask for a grant");
            return vec![];
        }
    };
    let mut out: Vec<Daemon> = vec![];
    for w in words {
        let Some(d) = super::machines_named(&machines, w).first().copied() else { continue };
        if !out.iter().any(|o| o.id == d.id) {
            out.push(d.clone());
        }
    }
    out
}

/// The post is stored (`told`): each of `machines` ([`unheard`]) has the
/// mention kept as `grant_pending` and its daemon asked to raise the card,
/// or, if its owner said never, `not_granted` and no card. A failure is
/// logged: the post stands.
pub(super) async fn ask(app: &Platform, told: &Told<'_>, machines: &[Daemon]) {
    for d in machines {
        if let Err(e) = ask_one(app, told, d).await {
            tracing::warn!(error = %e, machine = %d.id, "chat: asking for a grant");
        }
    }
}

async fn ask_one(app: &Platform, told: &Told<'_>, d: &Daemon) -> anyhow::Result<()> {
    let room = told.room;
    let scope = agent_scope(app, room, &d.account)?;
    let never = app.db.chat_grant_never(&room.id, &d.id)?;
    // No scope to grant over: nothing an owner could say yes to.
    let state = if never || scope.is_none() { NOT_GRANTED } else { GRANT_PENDING };
    let status = MentionStatus {
        room: room.id.clone(),
        seq: told.seq,
        machine: d.id.clone(),
        machine_name: d.name.clone(),
        root: told.parent.unwrap_or(told.seq),
        state: state.into(),
        session: None,
        asked: None,
        names: vec![],
        reply: None,
        at: now_ms(),
    };
    app.db.set_chat_mention_status(&status)?;
    announce_status(app, &status);
    let Some(scope) = scope.filter(|_| !never) else { return Ok(()) };
    // What the grant is over, as its owner reads it: the team's name, or
    // their DMs. The grant is `threads.read` over all of it, not just this
    // room, and the card says so (#578).
    let scope_name = crate::grants::owner_reads(app, &d.account, &scope, false).await?.unwrap_or_default();
    let (scope_kind, scope_id) = match scope {
        AgentScope::Project(id) => ("project", id),
        AgentScope::Account(id) => ("account", id),
    };
    let line = told.text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let line = if line.chars().count() > LINE {
        format!("{}…", line.chars().take(LINE).collect::<String>().trim_end())
    } else {
        line.to_owned()
    };
    let op = DaemonOp::AgentGrantAsk(AgentGrantAsk {
        room: room.id.clone(),
        room_name: room.name.clone(),
        seq: told.seq,
        author: told.author.clone(),
        line,
        machine: d.id.clone(),
        machine_name: d.name.clone(),
        scope_kind: scope_kind.into(),
        scope_id,
        scope_name,
    });
    // Like a mention: told after the post returns (#419), and best effort. A
    // machine that is offline or too old shows no card; the mention waits.
    let machine = d.id.clone();
    match app.arc() {
        Some(app) => {
            tokio::spawn(async move {
                ask_daemon(&app, &machine, &op).await;
            });
        }
        None => {
            ask_daemon(app, &machine, &op).await;
        }
    }
    Ok(())
}

/// A grant was given (the platform's `announce`, after any grant): each
/// mention waiting for a machine that now hears its room is delivered, once.
/// Runs in the background, so the grant's answer doesn't wait on a daemon.
pub(crate) async fn granted(app: &Platform) {
    match app.arc() {
        Some(app) => {
            tokio::spawn(async move { deliver_granted(&app).await });
        }
        None => deliver_granted(app).await,
    }
}

async fn deliver_granted(app: &Platform) {
    let pairs = match app.db.pending_grant_pairs(PAIRS_MAX) {
        Ok(p) => p,
        Err(e) => return tracing::warn!(error = %e, "chat: mentions waiting for a grant"),
    };
    for (room_id, machine) in pairs {
        if let Err(e) = deliver_pair(app, &room_id, &machine).await {
            tracing::warn!(error = %e, room = room_id, machine, "chat: delivering mentions a grant let in");
        }
    }
}

async fn deliver_pair(app: &Platform, room_id: &str, machine: &str) -> anyhow::Result<()> {
    let Some(room) = app.db.chat_room(room_id)?.filter(|r| r.archived.is_none()) else { return Ok(()) };
    let facts = Facts::of(app, None, &[&room]).await?;
    if !room_agents(app, &facts, &room).await?.iter().any(|a| a.machine == machine && hears(a)) {
        return Ok(());
    }
    // It reads the room now: a "never" is undone.
    app.db.clear_chat_grant_never(room_id, machine)?;
    let now = now_ms();
    let (claimed, dropped) =
        app.db.claim_granted_mentions(room_id, machine, now.saturating_sub(GRANT_PENDING_MS), now)?;
    for s in &dropped {
        announce_status(app, s);
    }
    send_claimed(app, claimed, true).await;
    Ok(())
}

/// `POST /api/chat/rooms/{id}/grant-never {daemon}`: the owner of a machine
/// says its agents are never to be asked about this room (#578). Over the
/// owner's session only: a machine can't say it of itself. What waited for
/// the machine in the room is `not_granted` for good.
pub async fn never(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(b): Json<ChatGrantNever>,
) -> J<serde_json::Value> {
    gate(&app, &s.account).await?;
    let (room, ..) = room_for(&app, &s.account, &id).await?;
    // On the hub's word, as a grant is: the owner's own machine.
    let mine = app
        .cache
        .daemons_now(&*app.hub, std::slice::from_ref(&b.daemon))
        .await?
        .get(&b.daemon)
        .is_some_and(|d| d.account == s.account);
    if !mine {
        return Err(err(StatusCode::NOT_FOUND, "not a machine on this account"));
    }
    app.db.set_chat_grant_never(&room.id, &b.daemon, now_ms())?;
    let refused = app.db.refuse_pending_mentions(&room.id, &b.daemon, now_ms())?;
    for r in &refused {
        announce_status(&app, r);
    }
    Ok(Json(serde_json::json!({ "refused": refused.len() })))
}
