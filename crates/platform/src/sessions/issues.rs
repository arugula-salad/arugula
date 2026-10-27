//! A session's agent and its project's native issues (aru-115, amended):
//! the route the box's session-scoped MCP tools call, through the hub's
//! `POST /api/hosted/work/session/issues` (`WORK_SESSION_ISSUES`), signed
//! as the hosted machine. Read, comment and open, nothing else.
//!
//! A hosted box has no account and no machine grants (aru-091 is for
//! machines of a person), so the session is what authorises it:
//! - the session is on the machine's project and workspace, and is the
//!   machine's own (the one its first report bound it to), and hasn't
//!   ended;
//! - the project is the session's, never the caller's: the request names an
//!   issue by its number in it, so there's no way to name another project's;
//! - every read and write is the **asker's**, by [`issues`](crate::issues)'
//!   one place, so their role in the project is the ceiling, as a person's
//!   own call would be (a viewer reads and comments);
//! - a comment is the session's own, posted as its answers are
//!   ([`super::answer`], Arugula #215): no author, `via` `said:<ses>`, in
//!   the issue's thread with no mentions (nobody is pushed for it). The
//!   words are the agent's, not its asker's, though the asker's role is
//!   what lets it comment; a review session (M264) reads and comments on
//!   nothing;
//! - an issue it opens is the session's own too
//!   ([`issues::create_as_session`]): no author, the session as its agent
//!   (shown as `Agent session <id>`), and no mentions or assignees, so
//!   nobody is pushed. It takes the asker's write rights (a viewer's
//!   session opens nothing), and its labels are from those the project's
//!   issues already have.
//!
//! Every call counts against the box's limit ([`CALLS`]).
//!
//! What this doesn't give: edits, new labels, assigning, closing, other
//! projects, chat rooms other than an issue's thread.

use arugula_control_wire::{
    issues::{self as wire, IssueQuery},
    sessions::said_via,
    work::{SessionIssuesAsk, SessionIssuesGiven, SessionIssuesOp},
};
use axum::http::StatusCode;
use tracing::warn;

use crate::{
    Platform,
    db::{
        chat::Page,
        issues::Actor,
        sessions::{Role, Session, State},
    },
    issues,
    work::HostedMachine,
};

/// Calls a box's sessions make, per hour.
const CALLS: (&str, usize) = ("session-issues", 1_800);
/// Comments the newest page of an issue's thread read.
const COMMENTS: u32 = 30;
/// A comment's longest text, in bytes: what an agent writes is a note, not
/// a document.
pub const COMMENT_MAX: usize = 8_000;

fn refused(m: &HostedMachine, a: &SessionIssuesAsk, why: &str) -> SessionIssuesGiven {
    warn!(session = a.session, machine = m.machine, why, "refused a session's issues call");
    SessionIssuesGiven::Refused { why: why.into() }
}

/// The session `a` names, if `m` may act for it: module docs.
fn session_of(app: &Platform, m: &HostedMachine, a: &SessionIssuesAsk) -> anyhow::Result<Result<Session, String>> {
    let Some(s) = app.db.session(&a.session)? else { return Ok(Err("no such session".into())) };
    if s.project != m.project || s.workspace != m.workspace || s.machine.as_deref() != Some(m.machine.as_str()) {
        return Ok(Err("that session isn't this machine's".into()));
    }
    if matches!(s.state, State::Failed | State::Stopped) {
        return Ok(Err(format!("the session is {}", s.state.as_str())));
    }
    if s.asker.is_empty() {
        return Ok(Err("the session's asker is gone, and it acts as them".into()));
    }
    Ok(Ok(s))
}

/// What an API error says, for the agent to read.
fn said(e: arugula_control_common::ApiError) -> SessionIssuesGiven {
    SessionIssuesGiven::Refused { why: e.1 }
}

/// `POST /api/hosted/work/session/issues` from `m`: module docs.
pub async fn answer(app: &Platform, m: &HostedMachine, a: &SessionIssuesAsk) -> anyhow::Result<SessionIssuesGiven> {
    let s = match session_of(app, m, a)? {
        Ok(s) => s,
        Err(why) => return Ok(refused(m, a, &why)),
    };
    if let Err(e) = app.limits.check_daemon(CALLS, &m.machine) {
        return Ok(said(e));
    }
    let by = Actor::person(&s.asker);
    Ok(match &a.op {
        SessionIssuesOp::List { query } => list(app, by, &s, query).await,
        SessionIssuesOp::Show { number } => show(app, by, &s, *number).await,
        SessionIssuesOp::Comment { number, text } => {
            if s.role == Role::Review {
                return Ok(refused(m, a, "a review session reads and comments on nothing"));
            }
            comment(app, by, &s, *number, text).await
        }
        SessionIssuesOp::Create { title, body, labels } => {
            if s.role == Role::Review {
                return Ok(refused(m, a, "a review session reads and opens nothing"));
            }
            let new =
                wire::NewIssue { title: title.clone(), body: body.clone(), labels: labels.clone(), assignees: vec![] };
            match issues::create_as_session(app, &s.asker, &s.id, &s.project, &new).await {
                Ok(issue) => {
                    let url = format!("{}/issues/{}", app.cfg.page(), issue.id);
                    SessionIssuesGiven::Created { issue, url }
                }
                Err(e) => said(e),
            }
        }
    })
}

async fn list(app: &Platform, by: Actor<'_>, s: &Session, q: &IssueQuery) -> SessionIssuesGiven {
    match issues::list(app, by, &s.project, q).await {
        Ok(list) => SessionIssuesGiven::List { list },
        Err(e) => said(e),
    }
}

async fn show(app: &Platform, by: Actor<'_>, s: &Session, number: u64) -> SessionIssuesGiven {
    let go = async {
        let issue = issues::read_number(app, by, &s.project, number).await?;
        let events = issues::timeline(app, by, &issue.id).await?.events;
        let (comments, more_comments) = match app.db.chat_room_on(&format!("project:{}", issue.project), &issue.id)? {
            Some(room) => {
                let (ms, more) = app.db.chat_messages(&room.id, Page::Newest, COMMENTS)?;
                let ms: Vec<_> = ms.into_iter().filter(|m| m.deleted.is_none()).collect();
                (crate::chat::show_messages(app, by.account, &ms).await?, more)
            }
            None => (vec![], false),
        };
        Ok::<_, arugula_control_common::ApiError>(SessionIssuesGiven::Shown { issue, events, comments, more_comments })
    };
    go.await.unwrap_or_else(said)
}

async fn comment(app: &Platform, by: Actor<'_>, s: &Session, number: u64, text: &str) -> SessionIssuesGiven {
    let text = text.trim();
    if text.is_empty() {
        return SessionIssuesGiven::Refused { why: "say something".into() };
    }
    if text.len() > COMMENT_MAX {
        return SessionIssuesGiven::Refused { why: format!("a comment is at most {COMMENT_MAX} bytes") };
    }
    let go = async {
        // Read first: the asker must see the issue, in this project.
        let issue = issues::read_number(app, by, &s.project, number).await?;
        let room = crate::chat::box_object_room(app, by.account, &s.project, &issue.id).await?;
        let room = app
            .db
            .chat_room(&room)?
            .ok_or_else(|| arugula_control_common::err(StatusCode::NOT_FOUND, "no such issue"))?;
        // Stored as a session's answers are (`review::post`): the session's
        // own (no author, `via` `said:<ses>`), with no mentions, so nobody
        // is pushed for what an agent wrote, and it never starts or prompts
        // a session; live, so people with the issue open see it.
        let seq = super::review::post(app, &room, 0, "", &said_via(&s.id), text)?;
        Ok::<_, arugula_control_common::ApiError>(SessionIssuesGiven::Commented { seq })
    };
    go.await.unwrap_or_else(said)
}
