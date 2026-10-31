//! Native issues over the platform's routes (M195, platform step 6): a
//! project's own issues ([`db::issues`], M194), who may do what with them,
//! their export, and a project's issue home. An issue's comments are its
//! object thread's messages (M196, `arugula-platform-chat`'s
//! `chat::objects`), posted through chat's routes.
//!
//! **Who may do what,** by membership of the issue's project:
//! - a personal project's account does everything;
//! - in a team's project, owners and editors open, edit, close, reopen,
//!   label and assign; viewers read, search, export and comment;
//! - no one deletes a single issue (deleting the project takes them);
//! - anyone else gets 404, as if the project or issue weren't there.
//!
//! Native issues come with chat, so they're behind control's chat setting
//! (`chat_on`), as their threads are.
//!
//! **Who acts** is an [`Actor`]: a person (the routes here, from a browser
//! or a CLI device), or a person's agent on a machine (M197; [`agent`]'s
//! routes, M200). The functions that take one ([`list`], [`create`],
//! [`read`], [`change`], [`timeline`]) are what both call; [`place`] is
//! the one place an agent's grant is checked.
//!
//! **Issue home:** where a project's "new issue" goes, `native` or
//! `github`. Unless someone chose, it's `github` while the project has a
//! linked repo and `native` otherwise, so a project linked to GitHub keeps
//! GitHub. The CLI, MCP and the page read it ([`home`]).
//!
//! A machine's agents reach them through [`agent`]'s routes (M200).
//!
//! **The inbox** (M202): an assignee added, or someone an issue's body
//! newly mentions, gets an entry and a push ([`crate::inbox`]).
//!
//! [`db::issues`]: crate::db::issues

pub mod agent;

use std::{collections::BTreeSet, sync::Arc};

use arugula_control_common::{ApiError, err, refusal};
use arugula_control_wire::{chat::ChatAgent, issues as wire};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::{
    Platform,
    auth::{Machine, Person},
    chat::{CHAT_OFF, chat_on},
    db::{
        chat::{Page, Word},
        issues::{self as db, Actor, CloseReason, Filter, ListEdit, State as IssueState},
        projects::{ISSUES_WRITE, THREADS_READ},
    },
    grants::{AgentScope, agent_may},
    hub::Team,
};

/// A project in a path that means the reader's own.
pub const PERSONAL: &str = "personal";

/// Searches per account, as chat's.
pub(crate) const ISSUE_SEARCHES: (&str, usize) = ("issue-search", 600);
/// Exports per account: each reads a whole project.
const ISSUE_EXPORTS: (&str, usize) = ("issue-export", 60);
/// A list's page, unless it asks for fewer; never more than [`PAGE_MAX`].
const PAGE: u32 = 50;
const PAGE_MAX: u32 = 200;

type J<T> = Result<Json<T>, ApiError>;

/// A project as someone acting on its issues sees it: theirs or their
/// team's, and their role there (a personal project's account is its
/// owner).
pub struct Place {
    pub project: db::IssueProject,
    pub role: TeamRole,
    /// The team's roster, for a team's project.
    pub team: Option<Team>,
}

impl Place {
    fn may_write(&self) -> bool {
        self.role != TeamRole::Viewer
    }

    /// Whether `account` is in the project: who may be assigned.
    fn has(&self, account: &str) -> bool {
        match &self.team {
            Some(t) => t.role(account).is_some(),
            None => self.project.account.as_deref() == Some(account),
        }
    }
}

fn no_project() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such project")
}

fn no_issue() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such issue")
}

/// The project `asked` names (its id, or [`PERSONAL`]) as `by` sees it,
/// or 404 when they aren't a member. `write`: they mean to change
/// something, so the team's roster is asked of the hub, not the cache, and
/// a viewer is refused.
///
/// The one place who acts is checked. An agent (M197) acts for its owner:
/// the owner's role is what it may do, and it needs its machine's grant
/// over the project too (`threads.read` to read, `issues.write` to
/// write), so it never reaches more than its owner. [`agent`]'s routes
/// (M200) call these functions with one.
pub async fn place(app: &Platform, by: Actor<'_>, asked: &str, write: bool) -> Result<Place, ApiError> {
    if !chat_on(app, by.account).await? {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            CHAT_OFF,
            "native issues come with chat, which isn't on for you: a team's owners turn it on in control (Teams)",
        ));
    }
    let id = if asked == PERSONAL {
        crate::projects::personal_project(app, by.account).await?.ok_or_else(no_project)?
    } else {
        asked.to_owned()
    };
    let project = app.db.issue_project(&id)?.ok_or_else(no_project)?;
    let (role, team) = match (&project.account, &project.team) {
        (Some(a), _) if a == by.account => (TeamRole::Owner, None),
        (None, Some(t)) => {
            let t = if write { app.cache.team_now(&*app.hub, t).await? } else { app.cache.team(&*app.hub, t).await? };
            let t = t.ok_or_else(no_project)?;
            (t.role(by.account).ok_or_else(no_project)?, Some(t))
        }
        _ => return Err(no_project()),
    };
    let place = Place { project, role, team };
    if write && !place.may_write() {
        return Err(err(StatusCode::FORBIDDEN, "a viewer reads and comments on issues, and doesn't change them"));
    }
    if let Some(machine) = by.agent {
        let m = Machine { account: by.account.to_owned(), daemon: machine.to_owned() };
        let power = if write { ISSUES_WRITE } else { THREADS_READ };
        agent_may(app, &m, power, &AgentScope::Project(place.project.id.clone())).await?;
    }
    Ok(place)
}

/// An issue by its id, and its project as `by` sees it: 404 unless they
/// may read it.
async fn issue_place(app: &Platform, by: Actor<'_>, id: &str, write: bool) -> Result<(db::Issue, Place), ApiError> {
    let issue = app.db.issue(id)?.ok_or_else(no_issue)?;
    let place = match place(app, by, &issue.project, write).await {
        Err(e) if e.0 == StatusCode::NOT_FOUND => return Err(no_issue()),
        p => p?,
    };
    Ok((issue, place))
}

/// Machines' names for the agents among these, by id. An agent session
/// (`ses_…`, aru-115) is named here, `Agent session <its first 8 hex>`, as
/// chat names what one said.
async fn machine_names(
    app: &Platform,
    agents: impl Iterator<Item = &String>,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let (sessions, ids): (Vec<String>, Vec<String>) =
        agents.cloned().collect::<BTreeSet<_>>().into_iter().partition(|id| id.starts_with(SESSION));
    let mut out: Names = sessions.iter().map(|id| (id.clone(), session_name(id))).collect();
    if !ids.is_empty() {
        out.extend(crate::grants::agent_names(app, &ids).await?.into_iter().map(|(id, d)| (id, d.name)));
    }
    Ok(out)
}

/// What an agent session's id starts with.
const SESSION: &str = "ses_";

pub(crate) fn session_name(id: &str) -> String {
    let hex = id.strip_prefix(SESSION).unwrap_or(id);
    format!("Agent session {}", &hex[..hex.len().min(crate::db::sessions::BRANCH_HEX)])
}

type Names = std::collections::HashMap<String, String>;

fn agent(names: &Names, machine: &Option<String>) -> Option<ChatAgent> {
    machine.as_ref().map(|m| ChatAgent {
        machine: m.clone(),
        machine_name: names.get(m).cloned().unwrap_or_default(),
        session: None,
    })
}

fn issue_wire(i: &db::Issue, names: &Names) -> wire::Issue {
    wire::Issue {
        id: i.id.clone(),
        project: i.project.clone(),
        number: i.number,
        title: i.title.clone(),
        body: i.body.clone(),
        state: i.state.as_str().into(),
        close_reason: i.state.reason().map(|r| r.as_str().into()),
        author: i.author.clone(),
        agent: agent(names, &i.author_agent),
        labels: i.labels.clone(),
        assignees: i.assignees.clone(),
        github: i.github.as_ref().map(|g| wire::IssueGithub { repo: g.repo.clone(), number: g.number }),
        created_at: i.created_at,
        updated_at: i.updated_at,
        closed_at: i.closed_at,
        closed_by: None,
        closed_by_agent: None,
    }
}

fn event_wire(e: &db::Event, names: &Names) -> wire::IssueEvent {
    wire::IssueEvent {
        id: e.id,
        kind: e.kind.as_str().into(),
        actor: e.actor.clone(),
        agent: agent(names, &e.actor_agent),
        at: e.at,
        detail: e.detail.clone(),
    }
}

async fn issues_wire(app: &Platform, issues: &[db::Issue]) -> anyhow::Result<Vec<wire::Issue>> {
    // Who closed the closed ones, for their rows (UI audit #163).
    let closed: Vec<&str> = issues.iter().filter(|i| i.state.reason().is_some()).map(|i| i.id.as_str()).collect();
    let closers = app.db.closers(&closed)?;
    let agents =
        issues.iter().filter_map(|i| i.author_agent.as_ref()).chain(closers.values().filter_map(|(_, a)| a.as_ref()));
    let names = machine_names(app, agents).await?;
    Ok(issues
        .iter()
        .map(|i| {
            let mut w = issue_wire(i, &names);
            if let Some((by, by_agent)) = closers.get(&i.id) {
                w.closed_by = Some(by.clone());
                w.closed_by_agent = agent(&names, by_agent);
            }
            w
        })
        .collect())
}

async fn events_wire(app: &Platform, events: &[db::Event]) -> anyhow::Result<Vec<wire::IssueEvent>> {
    let names = machine_names(app, events.iter().filter_map(|e| e.actor_agent.as_ref())).await?;
    Ok(events.iter().map(|e| event_wire(e, &names)).collect())
}

fn bad(msg: &str) -> ApiError {
    err(StatusCode::BAD_REQUEST, msg)
}

fn title_ok(t: &str) -> Result<&str, ApiError> {
    let t = t.trim();
    if t.is_empty() {
        return Err(bad("an issue needs a title"));
    }
    if t.chars().any(char::is_control) {
        return Err(bad("a title is one line, without control characters"));
    }
    if t.chars().count() > wire::TITLE_MAX {
        return Err(bad(&format!("a title is at most {} characters", wire::TITLE_MAX)));
    }
    Ok(t)
}

fn body_ok(b: &str) -> Result<&str, ApiError> {
    if b.len() > wire::BODY_MAX {
        return Err(bad(&format!("a body is at most {} bytes", wire::BODY_MAX)));
    }
    Ok(b)
}

/// Labels as given, trimmed; each a few words at most.
fn labels_ok(v: &[String]) -> Result<Vec<String>, ApiError> {
    let v: Vec<String> = v.iter().map(|l| l.trim().to_owned()).collect();
    if v.iter().any(|l| {
        l.is_empty() || l.chars().count() > wire::LABEL_MAX || l.contains(',') || l.chars().any(char::is_control)
    }) {
        return Err(bad(&format!("a label is 1 to {} characters, one line without a comma", wire::LABEL_MAX)));
    }
    Ok(v)
}

/// A whole list, or one added to, isn't too long; and those assigned are
/// in the project.
fn list_ok(edit: &wire::IssueListEdit, old: &[String], place: &Place, labels: bool) -> Result<(), ApiError> {
    let (max, what) = if labels { (wire::LABELS_MAX, "labels") } else { (wire::ASSIGNEES_MAX, "assignees") };
    let after = match edit {
        wire::IssueListEdit::Set(v) => v.len(),
        wire::IssueListEdit::Add(v) => old.len() + v.len(),
        wire::IssueListEdit::Remove(_) => 0,
    };
    if after > max {
        return Err(bad(&format!("an issue has at most {max} {what}")));
    }
    if !labels
        && let wire::IssueListEdit::Set(v) | wire::IssueListEdit::Add(v) = edit
        && let Some(a) = v.iter().find(|a| !place.has(a))
    {
        return Err(bad(&format!("{a} isn't in this project, so can't be assigned")));
    }
    Ok(())
}

fn edit_of(e: &wire::IssueListEdit) -> ListEdit<'_> {
    match e {
        wire::IssueListEdit::Set(v) => ListEdit::Set(v),
        wire::IssueListEdit::Add(v) => ListEdit::Add(v),
        wire::IssueListEdit::Remove(v) => ListEdit::Remove(v),
    }
}

/// A search box's words: `dep*` is a word starting with `dep`. Only text,
/// never search syntax (as chat's).
pub(crate) fn words(q: &str) -> Vec<Word> {
    q.split_whitespace()
        .filter_map(|tok| {
            let text: String = tok.trim_end_matches('*').chars().filter(|c| *c != '"').collect();
            (!text.is_empty()).then(|| Word { text, prefix: tok.ends_with('*') })
        })
        .collect()
}

// ---------------------------------------------------------------- the seam

/// A page of a project's issues as `by` may read them.
pub async fn list(
    app: &Platform,
    by: Actor<'_>,
    project: &str,
    q: &wire::IssueQuery,
) -> Result<wire::IssueList, ApiError> {
    let place = place(app, by, project, false).await?;
    let open = match q.state.as_deref().unwrap_or("open") {
        "open" => Some(true),
        "closed" => Some(false),
        "all" => None,
        s => return Err(bad(&format!("state is open, closed or all, not {s:?}"))),
    };
    let labels: Vec<String> =
        q.labels.as_deref().unwrap_or("").split(',').map(str::trim).filter(|l| !l.is_empty()).map(Into::into).collect();
    let words = words(q.q.as_deref().unwrap_or(""));
    if !words.is_empty() {
        app.limits.check_account(ISSUE_SEARCHES, by.account)?;
    }
    let f = Filter {
        open,
        labels: &labels,
        assignee: q.assignee.as_deref().filter(|a| !a.is_empty()),
        author: q.author.as_deref().filter(|a| !a.is_empty()),
        words: &words,
    };
    let limit = q.limit.unwrap_or(PAGE).clamp(1, PAGE_MAX);
    let (issues, more) = app.db.issues(&place.project.id, &f, q.before, limit)?;
    Ok(wire::IssueList { project: place.project.id, issues: issues_wire(app, &issues).await?, more })
}

/// Open an issue in `project` as `by`.
pub async fn create(
    app: &Platform,
    by: Actor<'_>,
    project: &str,
    new: &wire::NewIssue,
) -> Result<wire::Issue, ApiError> {
    let place = place(app, by, project, true).await?;
    let title = title_ok(&new.title)?;
    let body = body_ok(&new.body)?;
    let labels = labels_ok(&new.labels)?;
    list_ok(&wire::IssueListEdit::Set(labels.clone()), &[], &place, true)?;
    list_ok(&wire::IssueListEdit::Set(new.assignees.clone()), &[], &place, false)?;
    let n = db::NewIssue { project: &place.project.id, title, body, labels: &labels, assignees: &new.assignees };
    let issue = app.db.create_issue(&n, by, now_ms())?.ok_or_else(no_project)?;
    crate::inbox::issue_opened(app, by, &issue).await;
    Ok(issues_wire(app, &[issue]).await?.remove(0))
}

/// Open an issue in `project` with `asker`'s rights, as agent session
/// `session`'s own (aru-115): no author, the session as its agent, and its
/// labels from those the project's issues have. Nobody is told: its body's
/// mentions are text, and it has no assignees.
pub async fn create_as_session(
    app: &Platform,
    asker: &str,
    session: &str,
    project: &str,
    new: &wire::NewIssue,
) -> Result<wire::Issue, ApiError> {
    let place = place(app, Actor::person(asker), project, true).await?;
    let title = title_ok(&new.title)?;
    let body = body_ok(&new.body)?;
    let labels = labels_ok(&new.labels)?;
    list_ok(&wire::IssueListEdit::Set(labels.clone()), &[], &place, true)?;
    let known = app.db.issue_labels(&place.project.id)?;
    if let Some(l) = labels.iter().find(|l| !known.contains(l)) {
        let have = if known.is_empty() { "none yet".to_owned() } else { known.join(", ") };
        return Err(bad(&format!("{l:?} isn't one of this project's labels ({have})")));
    }
    if !new.assignees.is_empty() {
        return Err(bad("an agent session assigns nobody"));
    }
    let n = db::NewIssue { project: &place.project.id, title, body, labels: &labels, assignees: &[] };
    let by = Actor { account: "", agent: Some(session) };
    let issue = app.db.create_issue(&n, by, now_ms())?.ok_or_else(no_project)?;
    Ok(issues_wire(app, &[issue]).await?.remove(0))
}

/// One issue as `by` may read it: by its id, or by its project and number.
pub async fn read(app: &Platform, by: Actor<'_>, id: &str) -> Result<wire::Issue, ApiError> {
    let (issue, _) = issue_place(app, by, id, false).await?;
    Ok(issues_wire(app, &[issue]).await?.remove(0))
}

pub async fn read_number(app: &Platform, by: Actor<'_>, project: &str, number: u64) -> Result<wire::Issue, ApiError> {
    let place = place(app, by, project, false).await?;
    let issue = app.db.issue_by_number(&place.project.id, number)?.ok_or_else(no_issue)?;
    Ok(issues_wire(app, &[issue]).await?.remove(0))
}

/// Change an issue as `by`: what changed, and its events.
pub async fn change(
    app: &Platform,
    by: Actor<'_>,
    id: &str,
    p: &wire::IssuePatch,
) -> Result<wire::IssuePatched, ApiError> {
    // Read first, so someone who may read it learns it's there and is
    // refused; then as a write, always: the team's role from the hub, not
    // the cache, and an agent's `issues.write`.
    let (issue, _) = issue_place(app, by, id, false).await?;
    let place = self::place(app, by, &issue.project, true).await?;
    let title = p.title.as_deref().map(title_ok).transpose()?;
    let body = p.body.as_deref().map(body_ok).transpose()?;
    let state = match (p.state.as_deref(), p.reason.as_deref()) {
        (None, None) => None,
        (Some("open"), None) => Some(IssueState::Open),
        (Some("closed"), why) => Some(IssueState::Closed(match why {
            None => CloseReason::Completed,
            Some(w) => CloseReason::parse(w).ok_or_else(|| bad("a reason is completed, not_planned or duplicate"))?,
        })),
        (None, Some(_)) | (Some("open"), Some(_)) => return Err(bad("a reason goes with state closed")),
        (Some(s), _) => return Err(bad(&format!("state is open or closed, not {s:?}"))),
    };
    let labels = match &p.labels {
        Some(wire::IssueListEdit::Set(v)) => Some(wire::IssueListEdit::Set(labels_ok(v)?)),
        Some(wire::IssueListEdit::Add(v)) => Some(wire::IssueListEdit::Add(labels_ok(v)?)),
        other => other.clone(),
    };
    if let Some(l) = &labels {
        list_ok(l, &issue.labels, &place, true)?;
    }
    if let Some(a) = &p.assignees {
        list_ok(a, &issue.assignees, &place, false)?;
    }
    let patch = db::Patch {
        title,
        body,
        state,
        labels: labels.as_ref().map(edit_of),
        assignees: p.assignees.as_ref().map(edit_of),
        by_pr: None,
    };
    let old = issue;
    let (issue, events) = app.db.patch_issue(id, &patch, by, now_ms())?.ok_or_else(no_issue)?;
    crate::inbox::issue_changed(app, by, &old, &issue).await;
    // Pages showing it (its thread's readers) read it again (M203).
    if !events.is_empty() {
        app.chat.object_changed(&app.db, &format!("project:{}", issue.project), &issue.id);
        crate::boards::issue_changed(app, &issue.id);
    }
    Ok(wire::IssuePatched {
        issue: issues_wire(app, &[issue]).await?.remove(0),
        events: events_wire(app, &events).await?,
    })
}

/// An issue's timeline as `by` may read it.
pub async fn timeline(app: &Platform, by: Actor<'_>, id: &str) -> Result<wire::IssueEvents, ApiError> {
    let (issue, _) = issue_place(app, by, id, false).await?;
    let events = app.db.issue_events(&issue.id)?;
    Ok(wire::IssueEvents { issue: issue.id, events: events_wire(app, &events).await? })
}

/// Where a project's "new issue" goes, as `by` may read it.
pub async fn home(app: &Platform, by: Actor<'_>, project: &str) -> Result<wire::IssueHome, ApiError> {
    let place = place(app, by, project, false).await?;
    home_of(app, &place.project)
}

fn home_of(app: &Platform, p: &db::IssueProject) -> Result<wire::IssueHome, ApiError> {
    let repos = app.db.linked_repo_names(&p.id)?;
    let native_issues = app.db.issue_count(&p.id)?;
    // GitHub chosen, and its repo unlinked since: native, until someone
    // links one again.
    let home = match p.home.as_deref() {
        Some("github") | None if repos.is_empty() => "native",
        Some(h) => h,
        None => "github",
    };
    Ok(wire::IssueHome {
        project: p.id.clone(),
        name: p.name.clone(),
        home: home.into(),
        chosen: p.home.as_deref() == Some(home),
        both: native_issues > 0 && !repos.is_empty(),
        repos,
        native_issues,
        labels: app.db.issue_labels(&p.id)?,
    })
}

/// Every issue in a project with its timeline and comments, as `by` may
/// read them (M195: custody's one-way door).
pub async fn export(app: &Platform, by: Actor<'_>, project: &str) -> Result<wire::IssueExport, ApiError> {
    let place = place(app, by, project, false).await?;
    app.limits.check_account(ISSUE_EXPORTS, by.account)?;
    let all = app.db.export_issues(&place.project.id)?;
    let mut people = BTreeSet::new();
    let mut agents = BTreeSet::new();
    let mut threads = Vec::with_capacity(all.len());
    for e in &all {
        let mut comments = vec![];
        if let Some(room) = app.db.chat_room_on(&format!("project:{}", e.issue.project), &e.issue.id)? {
            let mut after = i64::MIN;
            loop {
                let (page, more) = app.db.chat_messages(&room.id, Page::After(after), 500)?;
                for m in &page {
                    after = m.seq;
                    if m.deleted.is_some() {
                        continue;
                    }
                    people.insert(m.author.clone());
                    agents.extend(m.agent.clone());
                    comments.push(m.clone());
                }
                if !more || page.is_empty() {
                    break;
                }
            }
        }
        people.insert(e.issue.author.clone());
        people.extend(e.issue.assignees.iter().cloned());
        people.extend(e.events.iter().map(|ev| ev.actor.clone()));
        people.extend(e.events.iter().filter_map(|ev| ev.detail["assignee"].as_str().map(str::to_owned)));
        agents.extend(e.issue.author_agent.clone());
        agents.extend(e.events.iter().filter_map(|ev| ev.actor_agent.clone()));
        threads.push(comments);
    }
    let names = machine_names(app, agents.iter()).await?;
    // Who closed each, as a read of it says (UI audit #163): their agents
    // are among the events' already.
    let closed: Vec<&str> =
        all.iter().filter(|e| e.issue.state.reason().is_some()).map(|e| e.issue.id.as_str()).collect();
    let closers = app.db.closers(&closed)?;
    let issues = all
        .iter()
        .zip(threads)
        .map(|(e, comments)| wire::ExportedIssue {
            issue: {
                let mut w = issue_wire(&e.issue, &names);
                if let Some((by, by_agent)) = closers.get(&e.issue.id) {
                    w.closed_by = Some(by.clone());
                    w.closed_by_agent = agent(&names, by_agent);
                }
                w
            },
            events: e.events.iter().map(|ev| event_wire(ev, &names)).collect(),
            comments: comments
                .into_iter()
                .map(|m| wire::IssueComment {
                    seq: m.seq,
                    agent: agent(&names, &m.agent),
                    author: m.author,
                    at: m.at,
                    text: m.text,
                    quote: m.quote,
                    edited: m.edited,
                })
                .collect(),
        })
        .collect();
    people.remove("");
    let ids: Vec<String> = people.into_iter().collect();
    let found = app.cache.accounts(&*app.hub, &ids).await?;
    let people = ids
        .into_iter()
        .map(|account| {
            let name = found
                .get(&account)
                .map(|a| if a.name.is_empty() { a.login.clone() } else { a.name.clone() })
                .unwrap_or_default();
            wire::ExportedPerson { account, name }
        })
        .collect();
    Ok(wire::IssueExport {
        project: place.project.id.clone(),
        project_name: place.project.name.clone(),
        exported_at: now_ms(),
        issues,
        people,
    })
}

// ---------------------------------------------------------------- routes

/// `GET /api/projects/{project}/issues`: a page of them, newest first.
pub async fn list_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
    Query(q): Query<wire::IssueQuery>,
) -> J<wire::IssueList> {
    Ok(Json(list(&app, Actor::person(&s.account), &project, &q).await?))
}

/// `POST /api/projects/{project}/issues`: open one (owners and editors).
pub async fn create_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
    Json(new): Json<wire::NewIssue>,
) -> J<wire::Issue> {
    Ok(Json(create(&app, Actor::person(&s.account), &project, &new).await?))
}

/// `GET /api/projects/{project}/issues/{number}`.
pub async fn by_number_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((project, number)): Path<(String, u64)>,
) -> J<wire::Issue> {
    Ok(Json(read_number(&app, Actor::person(&s.account), &project, number).await?))
}

/// `GET /api/issues/{id}`.
pub async fn get_route(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> J<wire::Issue> {
    Ok(Json(read(&app, Actor::person(&s.account), &id).await?))
}

/// `PATCH /api/issues/{id}`: title, body, state, labels, assignees
/// (owners and editors).
pub async fn patch_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(p): Json<wire::IssuePatch>,
) -> J<wire::IssuePatched> {
    Ok(Json(change(&app, Actor::person(&s.account), &id, &p).await?))
}

/// `GET /api/issues/{id}/events`.
pub async fn events_route(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> J<wire::IssueEvents> {
    Ok(Json(timeline(&app, Actor::person(&s.account), &id).await?))
}

/// `GET /api/projects/{project}/issue-home`.
pub async fn home_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
) -> J<wire::IssueHome> {
    Ok(Json(home(&app, Actor::person(&s.account), &project).await?))
}

/// `PUT /api/projects/{project}/issue-home`: owners and editors choose
/// (`github` only with a linked repo), or go back to the default.
pub async fn set_home_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
    Json(b): Json<wire::IssueHomeChange>,
) -> J<wire::IssueHome> {
    let place = place(&app, Actor::person(&s.account), &project, true).await?;
    match b.home.as_deref() {
        None | Some("native") => {}
        Some("github") if !app.db.linked_repo_names(&place.project.id)?.is_empty() => {}
        Some("github") => return Err(bad("GitHub is an issue home only for a project with a linked repo")),
        Some(h) => return Err(bad(&format!("an issue home is native or github, not {h:?}"))),
    }
    app.db.set_issue_home(&place.project.id, b.home.as_deref())?;
    let project = app.db.issue_project(&place.project.id)?.ok_or_else(no_project)?;
    Ok(Json(home_of(&app, &project)?))
}

/// `GET /api/projects/{project}/issues/export?format=json|md`.
pub async fn export_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
    Query(q): Query<wire::IssueExportQuery>,
) -> Result<Response, ApiError> {
    let md = match q.format.as_deref().unwrap_or("json") {
        "json" => false,
        "md" | "markdown" => true,
        f => return Err(bad(&format!("format is json or md, not {f:?}"))),
    };
    let x = export(&app, Actor::person(&s.account), &project).await?;
    let (body, kind, ext) = if md {
        (markdown(&x), "text/markdown; charset=utf-8", "md")
    } else {
        (serde_json::to_string_pretty(&x).map_err(anyhow::Error::from)?, "application/json", "json")
    };
    let file = format!("attachment; filename=\"issues-{}.{ext}\"", x.project);
    Ok(([(header::CONTENT_TYPE, kind.to_owned()), (header::CONTENT_DISPOSITION, file)], body).into_response())
}

// ---------------------------------------------------------------- Markdown

/// A time as `2026-10-21 14:03 UTC`.
fn when(ms: u64) -> String {
    let (days, rest) = ((ms / 86_400_000) as i64, ms % 86_400_000);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rest / 3_600_000, rest / 60_000 % 60)
}

/// What people wrote that goes on one line of Markdown (a name, a title,
/// a label): its control characters (line breaks) made spaces, so it can't
/// start a line of its own.
fn line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// What people wrote at length (a body, a comment, a quote), as a code
/// block whose fence is longer than any run of backticks in it: nothing in
/// it ends the block, or reads as a heading or as who wrote it.
fn fenced(s: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in s.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat((longest + 1).max(3));
    format!("{fence}\n{}\n{fence}\n", s.trim_end())
}

/// An export as one Markdown file: a heading per issue, its facts, its
/// body, its timeline and its comments, each by name. What people wrote is
/// one line or in a code block ([`line`], [`fenced`]), so it can't pass
/// for the export's own structure.
pub fn markdown(x: &wire::IssueExport) -> String {
    use std::fmt::Write as _;
    let name = |account: &str, agent: &Option<ChatAgent>| {
        let who = match x.people.iter().find(|p| p.account == account) {
            _ if account.is_empty() => "a deleted account".to_owned(),
            Some(p) if !p.name.is_empty() => p.name.clone(),
            _ => account.to_owned(),
        };
        line(&match agent {
            Some(a) => crate::grants::agent_label(
                &who,
                &a.machine,
                if a.machine_name.is_empty() { &a.machine } else { &a.machine_name },
            ),
            None => who,
        })
    };
    let mut out = format!(
        "# Issues: {}\n\nExported {} from project `{}`.\n",
        line(&x.project_name),
        when(x.exported_at),
        x.project
    );
    for e in &x.issues {
        let i = &e.issue;
        let _ = write!(out, "\n## #{} {}\n\n", i.number, line(&i.title));
        let state = match &i.close_reason {
            Some(r) => format!("closed ({})", r.replace('_', " ")),
            None => i.state.clone(),
        };
        let _ = writeln!(out, "- State: {state}");
        let _ = writeln!(out, "- Opened by {} on {}", name(&i.author, &i.agent), when(i.created_at));
        if !i.labels.is_empty() {
            let _ = writeln!(out, "- Labels: {}", line(&i.labels.join(", ")));
        }
        if !i.assignees.is_empty() {
            let names: Vec<String> = i.assignees.iter().map(|a| name(a, &None)).collect();
            let _ = writeln!(out, "- Assignees: {}", names.join(", "));
        }
        if let Some(g) = &i.github {
            let _ = writeln!(out, "- On GitHub: {}#{}", line(&g.repo), g.number);
        }
        if !i.body.is_empty() {
            let _ = write!(out, "\n{}", fenced(&i.body));
        }
        out.push_str("\n### Timeline\n\n");
        for ev in &e.events {
            let d = &ev.detail;
            let what = match ev.kind.as_str() {
                "opened" => "opened it".to_owned(),
                "edited" => match d["title"].as_str() {
                    Some(old) => format!("changed the title from \"{old}\""),
                    None => "edited the body".to_owned(),
                },
                "closed" => {
                    let why = d["reason"].as_str().unwrap_or("completed").replace('_', " ");
                    match d["pr"].as_str() {
                        Some(pr) => format!("closed it ({why}) with {pr}"),
                        None => format!("closed it ({why})"),
                    }
                }
                "reopened" => "reopened it".to_owned(),
                "labelled" => format!("added the label {}", d["label"].as_str().unwrap_or("")),
                "unlabelled" => format!("removed the label {}", d["label"].as_str().unwrap_or("")),
                "assigned" => format!("assigned {}", name(d["assignee"].as_str().unwrap_or(""), &None)),
                "unassigned" => format!("unassigned {}", name(d["assignee"].as_str().unwrap_or(""), &None)),
                "exported" => {
                    format!("exported it to {}#{}", d["repo"].as_str().unwrap_or(""), d["number"].as_u64().unwrap_or(0))
                }
                k => k.to_owned(),
            };
            let _ = writeln!(out, "- {}: {} {}", when(ev.at), name(&ev.actor, &ev.agent), line(&what));
        }
        if !e.comments.is_empty() {
            out.push_str("\n### Comments\n");
            for c in &e.comments {
                let _ = write!(out, "\n**{}**, {}:\n\n", name(&c.author, &c.agent), when(c.at));
                if let Some(q) = c.quote.as_deref().filter(|q| !q.is_empty()) {
                    out.push_str(&fenced(q));
                    out.push('\n');
                }
                out.push_str(&fenced(&c.text));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_as_utc() {
        assert_eq!(when(0), "1970-01-01 00:00 UTC");
        assert_eq!(when(1_792_591_380_000), "2026-10-21 14:03 UTC");
    }
}
