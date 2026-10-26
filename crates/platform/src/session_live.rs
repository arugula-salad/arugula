//! An agent session's live side (M257, thread = session, Arugula #129):
//! recording its moves so the pages showing its card hear of them, the
//! ticker that marks a quiet box's sessions `unresponsive`, and the
//! routes its card and its page use (`arugula_control_wire::sessions`).
//!
//! - **Record, then tell** ([`record`]). Every move of a session goes
//!   through [`Db::move_session`](crate::db::Db::move_session); [`record`]
//!   is that and the push: `ChatLive::Session` to its card's room through
//!   `chat_events`, so a page on either of the platform's machines reads
//!   the card again. A heartbeat tells only when the tool it names
//!   changed. The report route (the box's half, M255) and dispatch (M256)
//!   move sessions through it.
//! - **Quiet boxes** ([`start`]). The leader looks every [`TICK`] for
//!   sessions whose turn is under way and whose box said nothing for
//!   `QUIET_MS` (60 s), marks them `unresponsive` and tells their cards.
//!   The card also works it out from `heard_at` against the platform's
//!   clock, so it never says "working" of a box that's gone quiet between
//!   ticks.
//! - **The routes**: reading a session is for its project's members;
//!   stopping it and asking its box to answer, approve, deny or cancel are
//!   for owners and editors (a team's viewers read only). Stop is
//!   control's `stopped` move: the session leaves its box's work list, and
//!   the box's next report is refused with `now: stopped`, so the box stops
//!   the agent. The rest are actions (`db::sessions::Action`) the box
//!   picks up on its next pull.

use std::{sync::Arc, time::Duration};

use arugula_control_common::{ApiError, err};
use arugula_control_wire::sessions::{
    SessionAct, SessionAnswered, SessionCard, SessionLine, SessionOpen, SessionPort, SessionTurn, SessionView,
};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State as AxState},
    http::StatusCode,
};
use tracing::warn;

use crate::{
    Platform,
    auth::Person,
    db::sessions::{ActionKind, Event, Kind, Move, NewAction, Refused, Session, SessionDetail, State},
    forge::links::membership,
};

/// How often the leader looks for quiet boxes.
pub const TICK: Duration = Duration::from_secs(15);

/// Move session `id` and tell the pages showing its card. What
/// [`Db::move_session`](crate::db::Db::move_session) answers, as it
/// answers it.
pub fn record(app: &Platform, id: &str, m: &Move) -> anyhow::Result<Result<(Session, Vec<Event>), Refused>> {
    let tool = match m {
        Move::Daemon { .. } => app.db.session(id)?.and_then(|s| s.last_tool),
        Move::Control { .. } => None,
    };
    let moved = app.db.move_session(id, m)?;
    if let Ok((s, events)) = &moved
        && (!events.is_empty() || s.last_tool != tool)
    {
        app.chat.session(&app.db, s);
    }
    Ok(moved)
}

/// Mark the quiet boxes' sessions `unresponsive` and tell their cards.
pub fn tick(app: &Platform, now: u64) -> anyhow::Result<()> {
    for id in app.db.mark_unresponsive(now)? {
        if let Some(s) = app.db.session(&id)? {
            app.chat.session(&app.db, &s);
        }
    }
    Ok(())
}

/// The leader's [`tick`], every [`TICK`].
pub fn start(app: Arc<Platform>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            if app.is_paused() || app.is_stopping() || !app.is_leader() {
                continue;
            }
            if let Err(e) = tick(&app, now_ms()) {
                warn!(error = format!("{e:#}"), "marking quiet sessions unresponsive failed");
            }
        }
    });
}

fn no_session() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such session")
}

/// The session, if `account` is a member of its project, with whether they
/// may act on it (an owner or editor).
async fn readable(app: &Platform, account: &str, id: &str, now: bool) -> Result<(Session, bool), ApiError> {
    let s = app.db.session(id)?.ok_or_else(no_session)?;
    match membership(app, account, &s.project, now).await {
        Ok((_, role)) => Ok((s, role != TeamRole::Viewer)),
        Err(e) if e.0 == StatusCode::NOT_FOUND => Err(no_session()),
        Err(e) => Err(e),
    }
}

async fn actor(app: &Platform, account: &str, id: &str) -> Result<Session, ApiError> {
    let (s, may) = readable(app, account, id, true).await?;
    if !may {
        return Err(err(StatusCode::FORBIDDEN, "a viewer can't stop or answer a session"));
    }
    Ok(s)
}

fn detail(e: &Event) -> SessionDetail {
    serde_json::from_str(&e.detail).unwrap_or_default()
}

/// A state as the card says it.
fn said(s: State) -> &'static str {
    match s {
        State::Queued => "queued",
        State::Working => "working",
        State::NeedsInput => "needs input",
        State::Done => "done",
        State::Failed => "failed",
        State::Unresponsive => "stopped responding",
        State::Stopped => "stopped",
    }
}

/// An event in a line.
fn line(e: &Event, name: &dyn Fn(&str) -> String) -> String {
    let d = detail(e);
    let by: Option<String> = serde_json::from_str::<serde_json::Value>(&e.detail)
        .ok()
        .and_then(|v| v["by"].as_str().map(name))
        .filter(|n| !n.is_empty());
    match (e.kind, e.from, e.to) {
        (Kind::State, None, _) => "Asked for".into(),
        (Kind::State, Some(State::Queued), State::Working) => "Its box took it".into(),
        (Kind::State, Some(State::Done), State::Working) => "A new turn started".into(),
        (Kind::State, Some(State::NeedsInput), State::Working) => "Answered: working again".into(),
        (Kind::State, Some(State::Unresponsive), to) => format!("Its box is heard from again ({})", said(to)),
        (Kind::State, _, State::NeedsInput) => match d.question {
            Some(q) => format!("Asks: {q}"),
            None => "Waits for an answer".into(),
        },
        (Kind::State, _, State::Done) => "The turn finished".into(),
        (Kind::State, _, State::Unresponsive) => "Its box stopped responding".into(),
        (Kind::State, _, State::Stopped) => match by {
            Some(n) => format!("Stopped by {n}"),
            None => format!("Stopped{}", d.reason.map(|r| format!(": {r}")).unwrap_or_default()),
        },
        (Kind::State, _, State::Failed) => format!("Failed: {}", d.reason.unwrap_or_default()),
        (Kind::State, _, to) => format!("Now {}", said(to)),
        (Kind::Error, ..) => format!("Error: {}", d.error.unwrap_or_default()),
        (Kind::Turn, ..) => {
            let n = d.commits.len();
            let commits = format!("{n} commit{}", if n == 1 { "" } else { "s" });
            if d.pushed == Some(true) {
                format!("Turn pushed {}: {commits}", d.branch.unwrap_or_default())
            } else {
                format!("Turn left {commits}, not pushed")
            }
        }
        (Kind::Heartbeat, ..) => "Heard from its box".into(),
    }
}

/// Session `s`'s card and history, for a reader who `may` act on it.
pub(crate) async fn view(app: &Platform, s: &Session, may: bool) -> Result<SessionView, ApiError> {
    let events = app.db.session_events(&s.id)?;
    let actions = app.db.session_actions(&s.id)?;
    let pending: Vec<_> = actions.iter().filter(|a| a.state == crate::db::sessions::ActionState::Pending).collect();
    let mut who: Vec<String> = vec![s.asker.clone()];
    who.extend(pending.iter().map(|a| a.by.clone()));
    who.extend(events.iter().filter_map(|e| {
        serde_json::from_str::<serde_json::Value>(&e.detail).ok().and_then(|v| v["by"].as_str().map(str::to_owned))
    }));
    who.retain(|a| !a.is_empty());
    who.sort();
    who.dedup();
    let accounts = app.cache.accounts(&*app.hub, &who).await?;
    let name = |a: &str| accounts.get(a).map(|x| x.name.clone()).unwrap_or_default();
    let last_state = |to: State| events.iter().rev().find(|e| e.kind == Kind::State && e.to == to);
    let turn = events.iter().rev().find(|e| e.kind == Kind::Turn).map(|e| {
        let d = detail(e);
        SessionTurn {
            at: e.at,
            pushed: d.pushed.unwrap_or(false),
            head: d.head,
            commits: d.commits,
            diffstat: d.diffstat,
            uncommitted: d.uncommitted,
        }
    });
    let open = (s.seen_as() == State::NeedsInput).then(|| last_state(State::NeedsInput)).flatten().map(|e| {
        let d = detail(e);
        SessionOpen { request: d.request, ask: d.ask, question: d.question, options: d.options, at: e.at }
    });
    let answered = pending.iter().find(|a| a.kind.answers()).map(|a| SessionAnswered {
        by: a.by.clone(),
        name: name(&a.by),
        kind: a.kind.as_str().into(),
        at: a.at,
    });
    let ended = matches!(s.state, State::Done | State::Failed | State::Stopped);
    let reason = s.state.terminal().then(|| last_state(s.state)).flatten().and_then(|e| detail(e).reason);
    let ports = crate::hosted::previews::ports(app, &s.workspace, now_ms())?
        .into_iter()
        .map(|p| SessionPort { port: p.port, label: p.label, url: p.url })
        .collect();
    let card = SessionCard {
        session: s.id.clone(),
        project: s.project.clone(),
        room: s.room.clone(),
        parent: s.parent_seq,
        state: s.state.as_str().into(),
        was: s.was.map(|w| w.as_str().into()),
        asker: s.asker.clone(),
        asker_name: name(&s.asker),
        repo: s.repo.clone(),
        branch: s.branch.clone(),
        created_at: s.created_at,
        updated_at: s.updated_at,
        heard_at: s.heard_at,
        ended_at: ended.then(|| last_state(s.state).map_or(s.updated_at, |e| e.at)),
        last_tool: s.last_tool.clone(),
        last_error: s.last_error.clone(),
        reason,
        turn,
        open,
        answered,
        cancelling: pending.iter().any(|a| a.kind == ActionKind::Cancel),
        machine: s.machine.clone(),
        block: s.block.clone(),
        ports,
        pr: s.pr.clone(),
    };
    let lines = events
        .iter()
        .map(|e| SessionLine {
            seq: e.seq,
            kind: e.kind.as_str().into(),
            from: e.from.map(|f| f.as_str().into()),
            to: e.to.as_str().into(),
            at: e.at,
            signed: e.sig.is_some(),
            text: line(e, &name),
        })
        .collect();
    Ok(SessionView { card, may, now: now_ms(), events: lines })
}

/// `GET /api/sessions/{ses}`.
pub async fn show(
    AxState(app): AxState<Arc<Platform>>,
    Person { account }: Person,
    Path(id): Path<String>,
) -> Result<Json<SessionView>, ApiError> {
    let (s, may) = readable(&app, &account, &id, false).await?;
    Ok(Json(view(&app, &s, may).await?))
}

fn refused(r: &Refused) -> ApiError {
    match r {
        Refused::NoSession => no_session(),
        Refused::Moved { now } => err(StatusCode::CONFLICT, &format!("the session is {} now", said(*now))),
        other => err(StatusCode::CONFLICT, &other.to_string()),
    }
}

/// `POST /api/sessions/{ses}/stop`: control's `stopped`, from whatever
/// state it's in (once more if it moved meanwhile).
pub async fn stop(
    AxState(app): AxState<Arc<Platform>>,
    Person { account }: Person,
    Path(id): Path<String>,
) -> Result<Json<SessionView>, ApiError> {
    let mut s = actor(&app, &account, &id).await?;
    let detail = serde_json::json!({ "by": account, "reason": "stopped from its card" }).to_string();
    for _ in 0..2 {
        if s.state.terminal() {
            return Err(refused(&Refused::Moved { now: s.state }));
        }
        let m = Move::Control { from: s.state, to: State::Stopped, at: now_ms(), detail: &detail };
        match record(&app, &id, &m)? {
            Ok((after, _)) => return Ok(Json(view(&app, &after, true).await?)),
            Err(Refused::Moved { .. }) => s = app.db.session(&id)?.ok_or_else(no_session)?,
            Err(r) => return Err(refused(&r)),
        }
    }
    Err(err(StatusCode::CONFLICT, "the session kept moving: try again"))
}

/// `POST /api/sessions/{ses}/actions`: ask its box to answer, approve,
/// deny or cancel. The first answer to what's open wins.
pub async fn act(
    AxState(app): AxState<Arc<Platform>>,
    Person { account }: Person,
    Path(id): Path<String>,
    Json(b): Json<SessionAct>,
) -> Result<Json<SessionView>, ApiError> {
    let s = actor(&app, &account, &id).await?;
    let kind = ActionKind::parse(&b.kind)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "an action is answer, approve, deny or cancel"))?;
    if kind.answers()
        && let Some(first) = app
            .db
            .session_actions(&id)?
            .into_iter()
            .find(|a| a.kind.answers() && a.state == crate::db::sessions::ActionState::Pending)
    {
        let n = app.cache.accounts(&*app.hub, std::slice::from_ref(&first.by)).await?;
        let who = n.get(&first.by).map(|a| a.name.clone()).filter(|n| !n.is_empty()).unwrap_or("someone".into());
        return Err(err(StatusCode::CONFLICT, &format!("{who} answered it already")));
    }
    let reason = b.reason.as_deref().map(str::trim).filter(|r| !r.is_empty());
    let new = NewAction {
        by: &account,
        kind,
        request: b.request.as_deref(),
        answer: b.answer.as_ref(),
        option: b.option.as_deref(),
        reason,
    };
    match app.db.add_session_action(&s.id, &new, now_ms())? {
        Ok(_) => {
            let after = app.db.session(&id)?.ok_or_else(no_session)?;
            app.chat.session(&app.db, &after);
            Ok(Json(view(&app, &after, true).await?))
        }
        Err(Refused::Detail(why)) if why.starts_with("only") || why.starts_with("an answer") => {
            Err(err(StatusCode::BAD_REQUEST, why))
        }
        Err(r) => Err(refused(&r)),
    }
}

#[cfg(test)]
#[path = "session_live_tests.rs"]
mod tests;
