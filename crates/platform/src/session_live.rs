//! An agent session's live side (M257, thread = session, Arugula #129):
//! recording its moves so the pages showing its card hear of them, its
//! box's signed reports, and the routes its card and its page use
//! (`arugula_control_wire::sessions`).
//!
//! - **Record, then tell** ([`record`]). Every move of a session goes
//!   through [`Db::move_session`](crate::db::Db::move_session); [`record`]
//!   is that and the push: `ChatLive::Session` to its card's room through
//!   `chat_events`, so a page on either of the platform's machines reads
//!   the card again. A heartbeat tells only when the tool it names
//!   changed. The box's reports and the session's job move sessions
//!   through it; moves made in a transaction of their own are told with
//!   [`told`].
//! - **The box's reports** ([`report`]): the hub's `POST
//!   /api/hosted/work/session` (`WORK_SESSION_REPORT`), signed by the
//!   hosted machine, checked and kept through [`record`].
//! - **Quiet boxes.** A session's job (`sessions::job`, M256) marks a turn
//!   whose box said nothing for `QUIET_MS` (60 s) `unresponsive`, through
//!   [`record`]. The card also works it out from `heard_at` against the
//!   platform's clock, so it never says "working" of a box that's gone
//!   quiet before its job looks.
//! - **The routes**: reading a session is for its project's members;
//!   stopping it and asking its box to answer, approve, deny or cancel are
//!   for owners and editors (a team's viewers read only). Stop is
//!   control's `stopped` move: the session leaves its box's work list, and
//!   the box's next report is refused with `now: stopped`, so the box stops
//!   the agent. The rest are actions (`db::sessions::Action`) the box
//!   picks up on its next pull.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::sessions::{
    SessionAct, SessionAnswered, SessionCard, SessionFinish, SessionLine, SessionOpen, SessionPort, SessionTry,
    SessionTurn, SessionView,
};
use arugula_control_wire::work::{Reported, SessionReport};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State as AxState},
    http::StatusCode,
};
use tracing::{info, warn};

use crate::{
    Platform,
    auth::Person,
    db::{
        session_merged::MERGED,
        sessions::{
            ActionKind, BOX_REBUILT_LOST, DaemonReport, Event, Kind, Move, NOT_PICKED, NewAction, Refused, Role,
            Session, SessionDetail, State,
        },
    },
    forge::links::membership,
    sessions::review::REVIEWED,
    work::HostedMachine,
};

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

/// Tell the cards of the sessions `ids` that they changed (moved by a
/// transaction of their own: a workspace that went, an asker who left).
pub fn told(app: &Platform, ids: &[String]) -> anyhow::Result<()> {
    for id in ids {
        if let Some(s) = app.db.session(id)? {
            app.chat.session(&app.db, &s);
        }
    }
    Ok(())
}

fn refused_report(why: impl Into<String>) -> Reported {
    Reported::Refused { why: why.into(), now: None, next: None }
}

/// `POST /api/hosted/work/session` from `m`: something its box says of one
/// of its sessions, signed (`arugula session event v1`). Kept through
/// [`record`] when the signature is `m`'s, the session is on `m`'s
/// workspace and the move is one the box may make; the actions it names
/// are done then. A report kept before, sent again after its answer was
/// lost, is [`Reported::Already`].
pub async fn report(app: &Platform, m: &HostedMachine, r: &SessionReport) -> anyhow::Result<Reported> {
    let (Some(kind), Some(from), Some(to)) = (Kind::parse(&r.kind), State::parse(&r.from), State::parse(&r.to)) else {
        return Ok(refused_report("not a session's kind or state"));
    };
    let ev = arugula_e2e::session::Event {
        session: &r.session,
        n: r.n,
        kind,
        from,
        to,
        at: r.at,
        detail: &r.detail,
        daemon: &m.machine,
    };
    let Some(checked) = DaemonReport::verify(&ev, &m.sign, &r.sig) else {
        warn!(session = r.session, machine = m.machine, "a hosted machine's session report isn't signed by it");
        return Ok(refused_report("the report isn't signed by this machine"));
    };
    let Some(s) = app.db.session(&r.session)? else { return Ok(refused_report(Refused::NoSession.to_string())) };
    if s.project != m.project || s.workspace != m.workspace {
        return Ok(refused_report(Refused::OtherDaemon.to_string()));
    }
    match record(app, &r.session, &Move::Daemon { report: &checked, now: now_ms() })? {
        Ok((s, events)) => {
            info!(session = s.id, machine = m.machine, kind = %kind, to = %s.state, "a hosted machine reported on its session");
            // A review's reply goes in its thread (M264), and so does any
            // other turn's answer.
            if let Err(e) = crate::sessions::review::reported(app, &s, &events).await {
                warn!(session = s.id, error = %format!("{e:#}"), "a review's report");
            }
            if let Err(e) = crate::sessions::answer::reported(app, &s, &events) {
                warn!(session = s.id, error = %format!("{e:#}"), "a turn's answer");
            }
            Ok(Reported::Taken)
        }
        Err(why) => {
            let kept = app.db.session_events(&r.session)?.into_iter().find(|e| e.daemon_n == Some(r.n));
            if kept.is_some_and(|e| e.sig.as_deref() == Some(r.sig.as_str())) {
                return Ok(Reported::Already);
            }
            info!(session = r.session, machine = m.machine, refused = %why, "refused a hosted machine's session report");
            let (now, next) = match &why {
                Refused::Moved { now } => (Some(now.as_str().to_owned()), None),
                Refused::Seq { next } => (None, Some(*next)),
                _ => (None, None),
            };
            Ok(Reported::Refused { why: why.to_string(), now, next })
        }
    }
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

pub(crate) async fn actor(app: &Platform, account: &str, id: &str) -> Result<Session, ApiError> {
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
        (Kind::State, Some(_), State::Queued) if d.reason.as_deref() == Some(BOX_REBUILT_LOST) => {
            "Its box was rebuilt, and its conversation was lost with it: starting again from the thread".into()
        }
        (Kind::State, Some(_), State::Queued) => "Its box was rebuilt: starting again from the thread".into(),
        (Kind::State, Some(State::Done), State::Working) => "A new turn started".into(),
        (Kind::State, Some(State::NeedsInput), State::Working) => "Answered: working again".into(),
        (Kind::State, Some(State::Unresponsive), to) => format!("Its box is heard from again ({})", said(to)),
        (Kind::State, _, State::NeedsInput) => match d.question {
            Some(q) => format!("Asks: {q}"),
            None => "Waits for an answer".into(),
        },
        (Kind::State, _, State::Done) => "The turn finished".into(),
        (Kind::State, _, State::Unresponsive) => "Its box stopped responding".into(),
        (Kind::State, _, State::Stopped) if d.reason.as_deref() == Some(MERGED) => {
            "Its pull request merged: done".into()
        }
        (Kind::State, _, State::Stopped) if d.reason.as_deref() == Some(NOT_PICKED) => {
            "Stopped: another try was kept".into()
        }
        (Kind::State, _, State::Stopped) if d.reason.as_deref().is_some_and(|r| r.starts_with(REVIEWED)) => {
            "Its review is posted in the thread".into()
        }
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
            let finish = match &d.finish {
                Some(f) if f.ok => format!(" (finish passed: {})", f.command),
                Some(f) => format!(" (finish failed: {})", f.command),
                None => String::new(),
            };
            if d.pushed == Some(true) {
                format!("Turn pushed {}: {commits}{finish}", d.branch.unwrap_or_default())
            } else {
                format!("Turn left {commits}, not pushed{finish}")
            }
        }
        (Kind::Heartbeat, ..) => "Heard from its box".into(),
    }
}

/// What its latest turn left, from its history.
fn last_turn(events: &[Event]) -> Option<SessionTurn> {
    events.iter().rev().find(|e| e.kind == Kind::Turn).map(|e| {
        let d = detail(e);
        SessionTurn {
            at: e.at,
            pushed: d.pushed.unwrap_or(false),
            head: d.head,
            commits: d.commits,
            diffstat: d.diffstat,
            uncommitted: d.uncommitted,
            finish: d.finish.map(|f| SessionFinish { ok: f.ok, command: f.command, output_tail: f.output_tail }),
        }
    })
}

/// The agent its box ran it on, as its box said on taking it (M264).
fn ran_on(events: &[Event]) -> Option<String> {
    events
        .iter()
        .find(|e| e.kind == Kind::State && e.from == Some(State::Queued) && e.to == State::Working)
        .map(|e| detail(e).harness.unwrap_or_else(|| "claude".into()))
}

fn pushed_in(events: &[Event]) -> bool {
    events.iter().any(|e| e.kind == Kind::Turn && detail(e).pushed == Some(true))
}

/// Why it ended, or why it's queued again (its box was rebuilt).
fn reason_of(s: &Session, events: &[Event]) -> Option<String> {
    let again = s.state == State::Queued && s.last_seq > 1;
    (s.state.terminal() || again)
        .then(|| events.iter().rev().find(|e| e.kind == Kind::State && e.to == s.state))
        .flatten()
        .and_then(|e| detail(e).reason)
}

/// The Try 3 `s` is in (M264): its parent, then the siblings, and the one
/// kept. Nothing for a session in none.
fn tries(app: &Platform, s: &Session) -> anyhow::Result<(Vec<SessionTry>, Option<String>)> {
    let parent = match (s.role, &s.parent_session) {
        (Role::Thread, _) => s.clone(),
        (Role::Try, Some(p)) => match app.db.session(p)? {
            Some(p) => p,
            None => return Ok((vec![], None)),
        },
        _ => return Ok((vec![], None)),
    };
    let siblings = app.db.session_siblings(&parent.id)?;
    if siblings.is_empty() {
        return Ok((vec![], None));
    }
    // What its box may take now: a queued one not listed waits for a slot.
    let listed: Vec<String> = app.db.session_work(&parent.workspace)?.into_iter().map(|w| w.session).collect();
    let mut out = Vec::new();
    for t in std::iter::once(parent.clone()).chain(siblings) {
        let events = app.db.session_events(&t.id)?;
        out.push(SessionTry {
            session: t.id.clone(),
            state: t.state.as_str().into(),
            branch: t.branch.clone(),
            harness: t.harness.clone(),
            ran: ran_on(&events),
            turn: last_turn(&events),
            pushed: pushed_in(&events),
            reason: reason_of(&t, &events),
            updated_at: t.updated_at,
            no_slot: t.state == State::Queued && !listed.contains(&t.id),
        });
    }
    Ok((out, parent.picked))
}

/// Session `s`'s card and history, for `reader`, who `may` act on it.
pub(crate) async fn view(app: &Platform, s: &Session, may: bool, reader: &str) -> Result<SessionView, ApiError> {
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
    let pushed = pushed_in(&events);
    let turn = last_turn(&events);
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
    let reason = reason_of(s, &events);
    let (tries, picked) = tries(app, s)?;
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
        workspace: s.workspace.clone(),
        workspace_name: app.db.hosted_any(&s.project, &s.workspace)?.map(|w| w.name).unwrap_or_default(),
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
        merged_at: s.merged_at,
        pushed,
        ci_rounds: s.ci_rounds,
        ci_max: crate::db::session_feedback::MAX_CI_ROUNDS,
        needs_person: crate::sessions::feedback::needs_person(app, s)?,
        role: s.role.as_str().into(),
        parent_session: s.parent_session.clone(),
        harness: s.harness.clone(),
        ran: ran_on(&events),
        tries,
        picked,
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
    let asked = !s.asker.is_empty() && s.asker == reader;
    Ok(SessionView { card, may, asked, now: now_ms(), events: lines })
}

/// `GET /api/sessions/{ses}`.
pub async fn show(
    AxState(app): AxState<Arc<Platform>>,
    Person { account }: Person,
    Path(id): Path<String>,
) -> Result<Json<SessionView>, ApiError> {
    let (s, may) = readable(&app, &account, &id, false).await?;
    Ok(Json(view(&app, &s, may, &account).await?))
}

pub(crate) fn refused(r: &Refused) -> ApiError {
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
            Ok((after, _)) => return Ok(Json(view(&app, &after, true, &account).await?)),
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
            Ok(Json(view(&app, &after, true, &account).await?))
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
