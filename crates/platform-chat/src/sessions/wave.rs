//! A wave of agent sessions (Fleet 5): a project's sessions in one list,
//! what a lead (a person, or their agent) follows a track's sessions by,
//! instead of each thread and `git ls-remote`.
//!
//! - **The rows** ([`SessionRow`]): each session's state, its box, branch,
//!   HEAD and last push, its issue, who asked (and their agent), and what
//!   waits on the reader: its question or a permission (an owner's or
//!   editor's to answer), CI that failed again, or its PR to send (its
//!   asker's). Newest first; reviews (`@codex review`) aren't listed, being
//!   their session's.
//! - **Narrowed** ([`SessionWaveQuery`]) by its issue's label, by a
//!   tracker (an issue: the issues its body names as `#N`, those whose
//!   body names it, and itself), by who asked, by the machine whose agents
//!   started it, or to the ones going. Among the project's latest
//!   [`LOOKED_AT`].
//! - **For people** ([`PROJECT_SESSIONS`]): the project's members, beside
//!   its runs. **For a machine's agents** ([`AGENT_PROJECT_SESSIONS`],
//!   [`AGENT_SESSION`], [`AGENT_SESSIONS_WAIT`]): with `sessions.start`,
//!   `sessions.reply` or `threads.read` over the project, asked every
//!   call: whoever may start one, answer one or read its thread may follow
//!   it. A read of one session gives its last answer whole, as its box
//!   sent it (the thread keeps only its end, `super::answer`).
//! - **The done signal** ([`AGENT_SESSIONS_WAIT`]): answers once any of
//!   the sessions asked of has settled ([`SessionRow::settled`]: its turn
//!   finished and nothing queued for the next, or it waits on a person, or
//!   it ended), else after [`SESSION_WAIT_MAX`] seconds. It wakes on the
//!   session's live event (`ChatLive::Session`, which every move tells),
//!   and looks again every [`LOOK_EVERY`] besides. MCP's `wait` with
//!   `session` holds it again until its own timeout.
//!
//! [`PROJECT_SESSIONS`]: arugula_control_wire::sessions::PROJECT_SESSIONS
//! [`AGENT_PROJECT_SESSIONS`]: arugula_control_wire::sessions::AGENT_PROJECT_SESSIONS
//! [`AGENT_SESSION`]: arugula_control_wire::sessions::AGENT_SESSION
//! [`AGENT_SESSIONS_WAIT`]: arugula_control_wire::sessions::AGENT_SESSIONS_WAIT

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use arugula_control_common::{ApiError, err};
use arugula_control_wire::sessions::{
    AgentSessionRead, AgentSessionWait, AgentSessionsWaited, SESSION_WAIT_MAX, SessionAnswer, SessionIssue,
    SessionOpen, SessionRow, SessionWave, SessionWaveQuery, WAVE_MAX,
};
use arugula_e2e::team::TeamRole;
use axum::{
    Json,
    extract::{Path, Query, State as AxState},
    http::StatusCode,
};

use crate::{
    Platform,
    auth::{Machine, Person},
    db::{
        issues::Issue,
        projects::{SESSIONS_REPLY, SESSIONS_START, THREADS_READ},
        sessions::{Event, Kind, PromptState, Role, Session, State},
    },
    forge::links::membership,
    grants::{AgentScope, agent_may, owner_reads},
    session_live::{detail, last_turn, reason_of},
};

type J<T> = Result<Json<T>, ApiError>;

/// How many of a project's latest sessions a wave looks among.
pub const LOOKED_AT: u32 = 500;
/// How many a wave lists without `limit`.
const LIST_DEFAULT: u32 = 50;
/// How often a wait looks again, for a move another platform machine made
/// that it hasn't heard of yet.
const LOOK_EVERY: Duration = Duration::from_secs(2);
/// Reads a machine's agents make of sessions, per hour.
const AGENT_READS: (&str, usize) = ("agent-session-read", 3_600);
/// Waits a machine's agents hold, per hour: about
/// [`crate::triggers::read::OPEN_WAITS`] held all hour.
const WAITS: (&str, usize) = ("agent-session-wait", 2_000);

/// What `asker` (and for a machine's agents, `agent`) says for the one
/// asking: the reader, or this machine.
const ME: &str = "me";

/// Any of these over a project lets a machine's agents follow its
/// sessions.
const READS: [&str; 3] = [SESSIONS_START, SESSIONS_REPLY, THREADS_READ];

/// Whether session `s` has settled: its turn finished and no prompt waits
/// for the next one, or it waits on a person, or it ended.
pub fn settled(s: &Session, queued: bool) -> bool {
    match s.seen_as() {
        State::NeedsInput | State::Failed | State::Stopped => true,
        State::Done => !queued,
        State::Queued | State::Working | State::Unresponsive => false,
    }
}

/// The issue numbers `body` names as `#N` (not `owner/repo#N`, nor a
/// word's `x#1`).
pub fn named(body: &str) -> HashSet<u64> {
    let mut out = HashSet::new();
    let b = body.as_bytes();
    for (i, _) in body.match_indices('#') {
        if i > 0 && (b[i - 1].is_ascii_alphanumeric() || matches!(b[i - 1], b'/' | b'_' | b'-' | b'&')) {
            continue;
        }
        let digits: String = body[i + 1..].chars().take_while(char::is_ascii_digit).collect();
        if let Ok(n) = digits.parse() {
            out.insert(n);
        }
    }
    out
}

/// What waits on `reader` (who `may` answer it: an owner or editor) in a
/// session, in a few words.
fn waiting(s: &Session, open: Option<&SessionOpen>, ci: bool, pushed: bool, reader: &str, may: bool) -> Option<String> {
    if s.state.terminal() {
        return None;
    }
    if let Some(o) = open {
        return may
            .then(|| if o.ask.as_deref() == Some("permission") { "a permission" } else { "its question" }.into());
    }
    if ci && may {
        return Some("CI failed again".into());
    }
    (pushed && s.pr.is_none() && s.merged_at.is_none() && !s.asker.is_empty() && s.asker == reader)
        .then(|| "its PR to send".into())
}

fn issue_of(i: &Issue) -> SessionIssue {
    SessionIssue {
        id: i.id.clone(),
        number: i.number,
        title: i.title.clone(),
        labels: i.labels.clone(),
        state: i.state.as_str().into(),
    }
}

/// What a wave reads once and uses for every row.
#[derive(Default)]
struct Seen {
    issues: HashMap<String, Option<Issue>>,
    boxes: HashMap<String, String>,
    names: HashMap<String, String>,
    agents: HashMap<(String, String), String>,
}

impl Seen {
    /// The native issue session `s` was asked on, if it was.
    fn issue(&mut self, app: &Platform, s: &Session) -> anyhow::Result<Option<Issue>> {
        if !self.issues.contains_key(&s.room) {
            let target = app.db.chat_room(&s.room)?.filter(|r| r.kind == "object").and_then(|r| r.target);
            let issue = match target.filter(|t| t.starts_with("iss_")) {
                Some(id) => app.db.issue(&id)?,
                None => None,
            };
            self.issues.insert(s.room.clone(), issue);
        }
        Ok(self.issues[&s.room].clone())
    }

    async fn named(&mut self, app: &Platform, ids: &[String]) -> Result<(), ApiError> {
        let missing: Vec<String> = ids.iter().filter(|a| !self.names.contains_key(*a)).cloned().collect();
        if !missing.is_empty() {
            let got = app.cache.accounts(&*app.hub, &missing).await?;
            for a in missing {
                let name = got.get(&a).map(|x| x.name.clone()).unwrap_or_default();
                self.names.insert(a, name);
            }
        }
        Ok(())
    }

    /// Session `s` as a wave lists it, for `reader`, who `may` answer it.
    async fn row(
        &mut self,
        app: &Platform,
        s: &Session,
        issue: Option<&Issue>,
        events: &[Event],
        reader: &str,
        may: bool,
    ) -> Result<SessionRow, ApiError> {
        self.named(app, std::slice::from_ref(&s.asker)).await?;
        let asker_name = self.names.get(&s.asker).cloned().unwrap_or_default();
        let started_by = match &s.asker_agent {
            Some(m) => {
                let key = (asker_name.clone(), m.clone());
                if !self.agents.contains_key(&key) {
                    let label = crate::chat::sessions::agent_asker(app, &asker_name, m).await?;
                    self.agents.insert(key.clone(), label);
                }
                Some(self.agents[&key].clone())
            }
            None => None,
        };
        if !self.boxes.contains_key(&s.workspace) {
            let name = app.db.hosted_any(&s.project, &s.workspace)?.map(|w| w.name).unwrap_or_default();
            self.boxes.insert(s.workspace.clone(), name);
        }
        let queued = app.db.session_prompts(&s.id)?.iter().any(|p| p.state == PromptState::Queued);
        let open = open_of(s, events);
        let pushed_at =
            events.iter().rev().find(|e| e.kind == Kind::Turn && detail(e).pushed == Some(true)).map(|e| e.at);
        let ci = crate::sessions::feedback::needs_person(app, s)?;
        Ok(SessionRow {
            session: s.id.clone(),
            project: s.project.clone(),
            room: s.room.clone(),
            parent: s.parent_seq,
            state: s.state.as_str().into(),
            settled: settled(s, queued),
            role: s.role.as_str().into(),
            issue: issue.map(issue_of),
            asker: s.asker.clone(),
            asker_name,
            started_by,
            agent: s.asker_agent.clone(),
            workspace: s.workspace.clone(),
            workspace_name: self.boxes[&s.workspace].clone(),
            repo: s.repo.clone(),
            branch: s.branch.clone(),
            head: last_turn(events).and_then(|t| t.head),
            pushed_at,
            waiting: waiting(s, open.as_ref(), ci, pushed_at.is_some(), reader, may),
            question: open.and_then(|o| o.question),
            pr: s.pr.clone(),
            merged_at: s.merged_at,
            reason: reason_of(s, events),
            created_at: s.created_at,
            updated_at: s.updated_at,
        })
    }
}

/// What session `s` asks, while it waits on a person.
fn open_of(s: &Session, events: &[Event]) -> Option<SessionOpen> {
    if s.seen_as() != State::NeedsInput {
        return None;
    }
    let e = events.iter().rev().find(|e| e.kind == Kind::State && e.to == State::NeedsInput)?;
    let d = detail(e);
    Some(SessionOpen { request: d.request, ask: d.ask, question: d.question, options: d.options, at: e.at })
}

/// `project`'s wave, narrowed by `q`, for `reader` (`role` in the project).
async fn wave(app: &Platform, project: &str, q: &SessionWaveQuery, reader: &str, role: TeamRole) -> J<SessionWave> {
    let may = role != TeamRole::Viewer;
    let limit = q.limit.unwrap_or(LIST_DEFAULT).clamp(1, WAVE_MAX) as usize;
    let tracker = match q.tracker {
        Some(n) => Some(
            app.db.issue_by_number(project, n)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such tracker issue"))?,
        ),
        None => None,
    };
    let names = tracker.as_ref().map(|t| named(&format!("{}\n{}", t.title, t.body)));
    let label = q.label.as_deref().map(str::trim).filter(|l| !l.is_empty());
    let mut seen = Seen::default();
    let mut rows = Vec::new();
    let mut more = false;
    for s in app.db.project_sessions(project, LOOKED_AT)? {
        if s.role == Role::Review
            || q.asker.as_deref().is_some_and(|a| a != s.asker)
            || q.agent.as_deref().is_some_and(|m| s.asker_agent.as_deref() != Some(m))
            || (q.going && (s.state.terminal() || s.merged_at.is_some()))
        {
            continue;
        }
        let issue = seen.issue(app, &s)?;
        if let Some(l) = label
            && !issue.as_ref().is_some_and(|i| i.labels.iter().any(|x| x.eq_ignore_ascii_case(l)))
        {
            continue;
        }
        if let (Some(t), Some(names)) = (&tracker, &names) {
            let on = issue
                .as_ref()
                .is_some_and(|i| i.id == t.id || names.contains(&i.number) || named(&i.body).contains(&t.number));
            if !on {
                continue;
            }
        }
        if rows.len() == limit {
            more = true;
            break;
        }
        let events = app.db.session_events(&s.id)?;
        rows.push(seen.row(app, &s, issue.as_ref(), &events, reader, may).await?);
    }
    let mut askers: Vec<(String, String)> =
        rows.iter().filter(|r| !r.asker.is_empty()).map(|r| (r.asker.clone(), r.asker_name.clone())).collect();
    askers.sort_by(|a, b| (&a.1, &a.0).cmp(&(&b.1, &b.0)));
    askers.dedup();
    Ok(Json(SessionWave {
        project: project.to_owned(),
        sessions: rows,
        labels: app.db.issue_labels(project)?,
        askers,
        tracker: tracker.as_ref().map(issue_of),
        more,
    }))
}

/// `GET /api/projects/{project}/sessions?label=&tracker=&asker=&going=`:
/// its wave, for its members.
pub async fn list(
    AxState(app): AxState<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
    Query(q): Query<SessionWaveQuery>,
) -> J<SessionWave> {
    let (project, role) = membership(&app, &account, &project, false).await?;
    let mut q = q;
    if q.asker.as_deref() == Some(ME) {
        q.asker = Some(account.clone());
    }
    wave(&app, &project, &q, &account, role).await
}

/// Whether machine `d`'s agents may follow `project`'s sessions: one of
/// [`READS`] over it. The refusal says how to give the first.
async fn may_follow(app: &Platform, d: &Machine, project: &str) -> Result<TeamRole, ApiError> {
    let scope = AgentScope::Project(project.to_owned());
    let mut refused = None;
    for power in READS {
        match agent_may(app, d, power, &scope).await {
            Ok(_) => {
                return match membership(app, &d.account, project, false).await {
                    Ok((_, role)) => Ok(role),
                    Err(e) => Err(e),
                };
            }
            Err(e) => {
                if refused.as_ref().is_none_or(|r: &ApiError| r.0 != StatusCode::FORBIDDEN) {
                    refused = Some(e);
                }
            }
        }
    }
    Err(refused.unwrap_or_else(|| err(StatusCode::FORBIDDEN, "no grant")))
}

/// Session `id`, for machine `d`'s agents: hidden outside its owner's
/// projects, as for the owner, and refused without a grant to follow it.
async fn followed(app: &Platform, d: &Machine, id: &str) -> Result<(Session, TeamRole), ApiError> {
    let none = || err(StatusCode::NOT_FOUND, &format!("no such session: {id}"));
    let s = app.db.session(id)?.ok_or_else(none)?;
    if owner_reads(app, &d.account, &AgentScope::Project(s.project.clone()), false).await?.is_none() {
        return Err(none());
    }
    let role = may_follow(app, d, &s.project).await?;
    Ok((s, role))
}

/// `GET /api/daemon/projects/{project}/sessions?…`: its wave, for a
/// machine's agents (module docs).
pub async fn agent_list(
    AxState(app): AxState<Arc<Platform>>,
    d: Machine,
    Path(project): Path<String>,
    Query(q): Query<SessionWaveQuery>,
) -> J<SessionWave> {
    app.limits.check_daemon(AGENT_READS, &d.daemon)?;
    let role = may_follow(&app, &d, &project).await?;
    let mut q = q;
    if q.asker.as_deref() == Some(ME) {
        q.asker = Some(d.account.clone());
    }
    if q.agent.as_deref() == Some(ME) {
        q.agent = Some(d.daemon.clone());
    }
    wave(&app, &project, &q, &d.account, role).await
}

/// Session `s`'s read for an agent: its row, its last answer whole, and
/// what its latest turn left.
async fn read(app: &Platform, s: &Session, reader: &str, role: TeamRole) -> Result<AgentSessionRead, ApiError> {
    let events = app.db.session_events(&s.id)?;
    let mut seen = Seen::default();
    let issue = seen.issue(app, s)?;
    let row = seen.row(app, s, issue.as_ref(), &events, reader, role != TeamRole::Viewer).await?;
    let turns: Vec<&Event> = events.iter().filter(|e| e.kind == Kind::Turn).collect();
    let answer = turns.iter().rev().find_map(|e| {
        let text = detail(e).reply?.trim().to_owned();
        (!text.is_empty()).then(|| SessionAnswer { at: e.at, cut: text.starts_with('…'), text })
    });
    Ok(AgentSessionRead {
        session: row,
        answer,
        turn: last_turn(&events),
        open: open_of(s, &events),
        turns: turns.len() as u32,
    })
}

/// `GET /api/daemon/sessions/{ses}`: one session, its last answer whole.
pub async fn agent_read(
    AxState(app): AxState<Arc<Platform>>,
    d: Machine,
    Path(id): Path<String>,
) -> J<AgentSessionRead> {
    app.limits.check_daemon(AGENT_READS, &d.daemon)?;
    let (s, role) = followed(&app, &d, &id).await?;
    Ok(Json(read(&app, &s, &d.account, role).await?))
}

/// `POST /api/daemon/sessions/wait` [`AgentSessionWait`]: the sessions'
/// rows once any has settled, or as they are after the wait (module docs).
pub async fn agent_wait(
    AxState(app): AxState<Arc<Platform>>,
    d: Machine,
    Json(b): Json<AgentSessionWait>,
) -> J<AgentSessionsWaited> {
    let mut ids: Vec<String> = Vec::new();
    for id in b.sessions.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if !ids.iter().any(|x| x == id) {
            ids.push(id.to_owned());
        }
    }
    if ids.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "say which sessions: sessions"));
    }
    if ids.len() > WAVE_MAX as usize {
        return Err(err(StatusCode::BAD_REQUEST, &format!("at most {WAVE_MAX} sessions in a wait")));
    }
    let wait = Duration::from_secs(b.wait.min(SESSION_WAIT_MAX));
    // A wait is one read however long it's held, and at most a few are
    // open at once (with the runs' waits).
    let _held = if wait.is_zero() {
        app.limits.check_daemon(AGENT_READS, &d.daemon)?;
        None
    } else {
        app.limits.check_daemon(WAITS, &d.daemon)?;
        Some(crate::triggers::read::hold(&[format!("account:{}", d.account), format!("daemon:{}", d.daemon)])?)
    };
    // Asked once: each session there, and a grant over each project.
    let mut roles: HashMap<String, TeamRole> = HashMap::new();
    for id in &ids {
        let (s, role) = followed(&app, &d, id).await?;
        roles.insert(s.project, role);
    }
    let mut moves = app.chat.session_moves();
    let until = tokio::time::Instant::now() + wait;
    loop {
        let mut seen = Seen::default();
        let mut rows = Vec::new();
        for id in &ids {
            let s = app.db.session(id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, &format!("no such session: {id}")))?;
            let role = roles.get(&s.project).copied().unwrap_or(TeamRole::Viewer);
            let issue = seen.issue(&app, &s)?;
            let events = app.db.session_events(&s.id)?;
            rows.push(seen.row(&app, &s, issue.as_ref(), &events, &d.account, role != TeamRole::Viewer).await?);
        }
        let settled: Vec<String> = rows.iter().filter(|r| r.settled).map(|r| r.session.clone()).collect();
        if !settled.is_empty() || tokio::time::Instant::now() >= until {
            return Ok(Json(AgentSessionsWaited { sessions: rows, settled }));
        }
        tokio::select! {
            () = moves.changed(&ids) => {}
            () = tokio::time::sleep(LOOK_EVERY) => {}
            () = tokio::time::sleep_until(until) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trackers_body_names_its_issues() {
        let body = "Part of #262. Waves: #263, #264 (#265)\n- [ ] #266\nnot o/r#7, x#8, &#9; but #10.";
        let mut got: Vec<u64> = named(body).into_iter().collect();
        got.sort();
        assert_eq!(got, [10, 262, 263, 264, 265, 266]);
        assert!(named("# heading\n#").is_empty());
    }
}
