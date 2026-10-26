//! Native issues (M194, platform step 6, #1263): a project's own issues,
//! numbered per project, with a timeline and full-text search. Who may do
//! what is decided by the routes (M195); this is storage. The tables, with
//! notes on their columns, are made by `0009_issues`.
//!
//! - An issue's number is its project's: the platform gives it in the
//!   transaction that moves `projects.last_issue`, as chat numbers a room's
//!   messages, so numbers have no gaps and no duplicates.
//! - What changes an issue writes its events (the timeline) in the same
//!   transaction. Comments aren't events: they're the messages of the
//!   issue's object thread (M196).
//! - An agent's issue or change keeps its owner's account as the author or
//!   actor, and names the agent's machine beside it ([`Actor::agent`]).
//! - Search is FTS5's (`issues_fts`) on SQLite and a `tsvector` column on
//!   Postgres, matching words as chat's does ([`Word`]).
//! - No one deletes a single issue: deleting its project takes it.

use std::collections::HashMap;

use anyhow::{Context as _, Result, bail};
use serde_json::json;

use super::{
    Db,
    chat::{Word, fts5_query, tsquery},
    migrate::Dialect,
    sql::{Optional as _, Row, Sql, Value, list, params},
};
use crate::Deleted;

/// A new issue's id: `iss_` and 32 hex characters, beside `prj_` for
/// projects.
fn new_id() -> String {
    format!("iss_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// Why a closed issue was closed: GitHub's reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    Completed,
    NotPlanned,
    Duplicate,
}

impl CloseReason {
    pub fn as_str(self) -> &'static str {
        match self {
            CloseReason::Completed => "completed",
            CloseReason::NotPlanned => "not_planned",
            CloseReason::Duplicate => "duplicate",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "completed" => CloseReason::Completed,
            "not_planned" => CloseReason::NotPlanned,
            "duplicate" => CloseReason::Duplicate,
            _ => return None,
        })
    }
}

/// Open, or closed for a reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Open,
    Closed(CloseReason),
}

impl State {
    /// `open` or `closed`, as the `state` column holds it.
    pub fn as_str(self) -> &'static str {
        match self {
            State::Open => "open",
            State::Closed(_) => "closed",
        }
    }

    pub fn reason(self) -> Option<CloseReason> {
        match self {
            State::Open => None,
            State::Closed(r) => Some(r),
        }
    }
}

/// Where an issue was exported to on GitHub (M205).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubOrigin {
    /// `owner/name`, as it was when exported.
    pub repo: String,
    pub repo_id: u64,
    pub number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub id: String,
    pub project: String,
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: State,
    /// The account that opened it: a person's, or an agent's owner's.
    /// Empty once that account is deleted.
    pub author: String,
    /// The machine whose agent opened it.
    pub author_agent: Option<String>,
    pub labels: Vec<String>,
    /// Accounts.
    pub assignees: Vec<String>,
    pub github: Option<GithubOrigin>,
    pub created_at: u64,
    pub updated_at: u64,
    pub closed_at: Option<u64>,
}

/// What an event on an issue's timeline is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Opened,
    Edited,
    Closed,
    Reopened,
    Labelled,
    Unlabelled,
    Assigned,
    Unassigned,
    Exported,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Opened => "opened",
            EventKind::Edited => "edited",
            EventKind::Closed => "closed",
            EventKind::Reopened => "reopened",
            EventKind::Labelled => "labelled",
            EventKind::Unlabelled => "unlabelled",
            EventKind::Assigned => "assigned",
            EventKind::Unassigned => "unassigned",
            EventKind::Exported => "exported",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "opened" => EventKind::Opened,
            "edited" => EventKind::Edited,
            "closed" => EventKind::Closed,
            "reopened" => EventKind::Reopened,
            "labelled" => EventKind::Labelled,
            "unlabelled" => EventKind::Unlabelled,
            "assigned" => EventKind::Assigned,
            "unassigned" => EventKind::Unassigned,
            "exported" => EventKind::Exported,
            _ => return None,
        })
    }
}

/// One event on an issue's timeline. `detail` is a small JSON object:
/// - `edited`: `title` (the old title) when the title changed, and
///   `body: true` when the body did;
/// - `closed`: `reason`;
/// - `labelled`, `unlabelled`: `label`;
/// - `assigned`, `unassigned`: `assignee` (an account; empty once it's
///   deleted);
/// - `exported`: `repo` and `number` (M205);
/// - `opened`, `reopened`: nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub id: u64,
    pub issue: String,
    pub kind: EventKind,
    /// The account that did it: a person's, or an agent's owner's. Empty
    /// once that account is deleted.
    pub actor: String,
    /// The machine whose agent did it.
    pub actor_agent: Option<String>,
    pub at: u64,
    pub detail: serde_json::Value,
}

/// Who writes: a person, or (with `agent`, its machine) a person's agent.
#[derive(Debug, Clone, Copy)]
pub struct Actor<'a> {
    pub account: &'a str,
    pub agent: Option<&'a str>,
}

impl<'a> Actor<'a> {
    pub fn person(account: &'a str) -> Self {
        Self { account, agent: None }
    }
}

pub struct NewIssue<'a> {
    pub project: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub labels: &'a [String],
    pub assignees: &'a [String],
}

/// A change to a list of labels or assignees: the whole list, or some
/// added or removed (so two people adding one each both land).
#[derive(Debug, Clone, Copy)]
pub enum ListEdit<'a> {
    Set(&'a [String]),
    Add(&'a [String]),
    Remove(&'a [String]),
}

impl ListEdit<'_> {
    /// `old` after the edit: in order, each once.
    fn apply(self, old: &[String]) -> Vec<String> {
        let mut out = match self {
            ListEdit::Set(v) => v.to_vec(),
            ListEdit::Add(v) => old.iter().chain(v).cloned().collect(),
            ListEdit::Remove(v) => old.iter().filter(|x| !v.contains(x)).cloned().collect(),
        };
        dedupe(&mut out);
        out
    }
}

/// What a patch changes; `None` leaves it.
#[derive(Debug, Default, Clone, Copy)]
pub struct Patch<'a> {
    pub title: Option<&'a str>,
    pub body: Option<&'a str>,
    pub state: Option<State>,
    pub labels: Option<ListEdit<'a>>,
    pub assignees: Option<ListEdit<'a>>,
}

/// Which of a project's issues a list wants. Each that's set narrows it.
#[derive(Debug, Default, Clone, Copy)]
pub struct Filter<'a> {
    /// `Some(true)`: open ones; `Some(false)`: closed ones.
    pub open: Option<bool>,
    /// Issues with every one of these labels.
    pub labels: &'a [String],
    pub assignee: Option<&'a str>,
    pub author: Option<&'a str>,
    /// Issues whose title or body has every word.
    pub words: &'a [Word],
}

/// An issue and its timeline, for an export (M195).
#[derive(Debug, Clone, PartialEq)]
pub struct Exported {
    pub issue: Issue,
    pub events: Vec<Event>,
}

const COLS: &str = "id, project_id, number, title, body, state, close_reason, author, author_agent, labels, assignees, \
                    github_repo, github_repo_id, github_number, created_at, updated_at, closed_at";
const EVENT_COLS: &str = "id, issue_id, kind, actor, actor_agent, at, detail";

fn issue_row(r: &Row) -> Result<Issue> {
    let state = match (r.get::<String>(5)?.as_str(), r.get::<Option<String>>(6)?) {
        ("open", _) => State::Open,
        ("closed", Some(why)) => {
            State::Closed(CloseReason::parse(&why).with_context(|| format!("an issue closed as {why:?}"))?)
        }
        (s, why) => bail!("an issue in state {s:?} ({why:?})"),
    };
    let github = match (r.get::<Option<String>>(11)?, r.get::<Option<u64>>(12)?, r.get::<Option<u64>>(13)?) {
        (Some(repo), Some(repo_id), Some(number)) => Some(GithubOrigin { repo, repo_id, number }),
        _ => None,
    };
    Ok(Issue {
        id: r.get(0)?,
        project: r.get(1)?,
        number: r.get(2)?,
        title: r.get(3)?,
        body: r.get(4)?,
        state,
        author: r.get(7)?,
        author_agent: r.get(8)?,
        labels: serde_json::from_str(&r.get::<String>(9)?)?,
        assignees: serde_json::from_str(&r.get::<String>(10)?)?,
        github,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
        closed_at: r.get(16)?,
    })
}

fn event_row(r: &Row) -> Result<Event> {
    let kind: String = r.get(2)?;
    Ok(Event {
        id: r.get(0)?,
        issue: r.get(1)?,
        kind: EventKind::parse(&kind).with_context(|| format!("an issue event {kind:?}"))?,
        actor: r.get(3)?,
        actor_agent: r.get(4)?,
        at: r.get(5)?,
        detail: serde_json::from_str(&r.get::<String>(6)?)?,
    })
}

fn dedupe(v: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    v.retain(|x| seen.insert(x.clone()));
}

/// `col`, a JSON list in text, has the text in parameter `?n`.
fn json_has(c: &impl Sql, col: &str, n: usize) -> String {
    match c.dialect() {
        Dialect::Sqlite => format!("EXISTS (SELECT 1 FROM json_each({col}) WHERE value = ?{n})"),
        Dialect::Postgres => {
            format!("EXISTS (SELECT 1 FROM jsonb_array_elements_text(CAST({col} AS jsonb)) v WHERE v = ?{n})")
        }
    }
}

/// Write an event on `issue`'s timeline.
fn add_event(
    c: &impl Sql,
    issue: &str,
    kind: EventKind,
    by: Actor,
    at: u64,
    detail: serde_json::Value,
) -> Result<Event> {
    c.query_row(
        &format!(
            "INSERT INTO issue_events (issue_id, kind, actor, actor_agent, at, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING {EVENT_COLS}"
        ),
        params![issue, kind.as_str(), by.account, by.agent, at, detail.to_string()],
        event_row,
    )
}

impl Db {
    /// Open an issue in `project` as `by`: the project gives it the next
    /// number, and its timeline starts with `opened`. `None`: there's no
    /// such project.
    pub fn create_issue(&self, new: &NewIssue, by: Actor, at: u64) -> Result<Option<Issue>> {
        let (mut labels, mut assignees) = (new.labels.to_vec(), new.assignees.to_vec());
        dedupe(&mut labels);
        dedupe(&mut assignees);
        let mut c = self.c();
        let tx = c.transaction()?;
        let Some(number): Option<u64> = tx
            .query_row(
                "UPDATE projects SET last_issue = last_issue + 1 WHERE id = ?1 RETURNING last_issue",
                params![new.project],
                |r| r.get(0),
            )
            .optional()?
        else {
            return Ok(None);
        };
        let issue = tx.query_row(
            &format!(
                "INSERT INTO issues (id, project_id, number, title, body, author, author_agent, labels, assignees,
                     created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10) RETURNING {COLS}"
            ),
            params![
                new_id(),
                new.project,
                number,
                new.title,
                new.body,
                by.account,
                by.agent,
                serde_json::to_string(&labels)?,
                serde_json::to_string(&assignees)?,
                at
            ],
            issue_row,
        )?;
        add_event(&tx, &issue.id, EventKind::Opened, by, at, json!({}))?;
        tx.commit()?;
        Ok(Some(issue))
    }

    pub fn issue(&self, id: &str) -> Result<Option<Issue>> {
        self.c().query_row(&format!("SELECT {COLS} FROM issues WHERE id = ?1"), params![id], issue_row).optional()
    }

    /// A project's issue by its number.
    pub fn issue_by_number(&self, project: &str, number: u64) -> Result<Option<Issue>> {
        self.c()
            .query_row(
                &format!("SELECT {COLS} FROM issues WHERE project_id = ?1 AND number = ?2"),
                params![project, number],
                issue_row,
            )
            .optional()
    }

    /// A page of a project's issues that match `f`, newest (highest
    /// number) first, below the number `before` when it's given, and
    /// whether there are more.
    pub fn issues(&self, project: &str, f: &Filter, before: Option<u64>, limit: u32) -> Result<(Vec<Issue>, bool)> {
        let c = self.c();
        let cols = COLS.split(", ").map(|c| format!("i.{c}")).collect::<Vec<_>>().join(", ");
        let mut sql = format!("SELECT {cols} FROM issues i");
        let mut args: Vec<Value> = vec![Value::Text(project.into())];
        let mut wh = vec!["i.project_id = ?1".to_owned()];
        if !f.words.is_empty() {
            match c.dialect() {
                Dialect::Sqlite => {
                    sql.push_str(" JOIN issues_fts s ON s.rowid = i.search_id");
                    args.push(Value::Text(fts5_query(f.words)));
                    wh.push(format!("issues_fts MATCH ?{}", args.len()));
                }
                Dialect::Postgres => match tsquery(f.words) {
                    Some(q) => {
                        args.push(Value::Text(q));
                        wh.push(format!("i.search @@ to_tsquery('simple', ?{})", args.len()));
                    }
                    None => return Ok((vec![], false)),
                },
            }
        }
        if let Some(open) = f.open {
            wh.push(format!("i.state = '{}'", if open { "open" } else { "closed" }));
        }
        for l in f.labels {
            args.push(Value::Text(l.clone()));
            wh.push(json_has(&c, "i.labels", args.len()));
        }
        if let Some(a) = f.assignee {
            args.push(Value::Text(a.into()));
            wh.push(json_has(&c, "i.assignees", args.len()));
        }
        if let Some(a) = f.author {
            args.push(Value::Text(a.into()));
            wh.push(format!("i.author = ?{}", args.len()));
        }
        if let Some(b) = before {
            args.push(Value::Int(i64::try_from(b)?));
            wh.push(format!("i.number < ?{}", args.len()));
        }
        sql.push_str(&format!(" WHERE {} ORDER BY i.number DESC LIMIT {}", wh.join(" AND "), u64::from(limit) + 1));
        let mut out = c.query_map(&sql, &list(&args), issue_row)?;
        let more = out.len() > limit as usize;
        out.truncate(limit as usize);
        Ok((out, more))
    }

    /// A project's issues whose title or body has every word, newest
    /// first.
    pub fn search_issues(&self, project: &str, words: &[Word], limit: u32) -> Result<Vec<Issue>> {
        Ok(self.issues(project, &Filter { words, ..Default::default() }, None, limit)?.0)
    }

    /// Change an issue as `by`, writing an event for each change in the
    /// same transaction: the issue as it is now, and those events (none
    /// when nothing changed). `None`: there's no such issue.
    pub fn patch_issue(&self, id: &str, p: &Patch, by: Actor, at: u64) -> Result<Option<(Issue, Vec<Event>)>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let Some(old) = tx
            .query_row(&format!("SELECT {COLS} FROM issues WHERE id = ?1{}", tx.for_update()), params![id], issue_row)
            .optional()?
        else {
            return Ok(None);
        };
        let mut new = old.clone();
        let mut events = Vec::new();
        let mut edited = serde_json::Map::new();
        if let Some(t) = p.title.filter(|t| *t != old.title) {
            edited.insert("title".into(), old.title.clone().into());
            new.title = t.to_owned();
        }
        if let Some(b) = p.body.filter(|b| *b != old.body) {
            edited.insert("body".into(), true.into());
            new.body = b.to_owned();
        }
        if !edited.is_empty() {
            events.push((EventKind::Edited, serde_json::Value::Object(edited)));
        }
        if let Some(s) = p.state.filter(|s| *s != old.state) {
            new.state = s;
            match s {
                State::Open => {
                    new.closed_at = None;
                    events.push((EventKind::Reopened, json!({})));
                }
                State::Closed(why) => {
                    // A closed one closed again for another reason keeps its time.
                    new.closed_at = old.closed_at.or(Some(at));
                    events.push((EventKind::Closed, json!({ "reason": why.as_str() })));
                }
            }
        }
        let mut list_events = |edit: Option<ListEdit>, old: &[String], key: &str, on: EventKind, off: EventKind| {
            let Some(edit) = edit else { return old.to_vec() };
            let now = edit.apply(old);
            for x in old.iter().filter(|x| !now.contains(x)) {
                events.push((off, json!({ key: x })));
            }
            for x in now.iter().filter(|x| !old.contains(x)) {
                events.push((on, json!({ key: x })));
            }
            now
        };
        new.labels = list_events(p.labels, &old.labels, "label", EventKind::Labelled, EventKind::Unlabelled);
        new.assignees =
            list_events(p.assignees, &old.assignees, "assignee", EventKind::Assigned, EventKind::Unassigned);
        if new == old {
            return Ok(Some((old, vec![])));
        }
        new.updated_at = at;
        tx.execute(
            "UPDATE issues SET title = ?2, body = ?3, state = ?4, close_reason = ?5, labels = ?6, assignees = ?7,
                 updated_at = ?8, closed_at = ?9
             WHERE id = ?1",
            params![
                id,
                new.title,
                new.body,
                new.state.as_str(),
                new.state.reason().map(CloseReason::as_str),
                serde_json::to_string(&new.labels)?,
                serde_json::to_string(&new.assignees)?,
                new.updated_at,
                new.closed_at
            ],
        )?;
        let events = events
            .into_iter()
            .map(|(kind, detail)| add_event(&tx, id, kind, by, at, detail))
            .collect::<Result<Vec<_>>>()?;
        tx.commit()?;
        Ok(Some((new, events)))
    }

    /// An issue's timeline, oldest first.
    pub fn issue_events(&self, issue: &str) -> Result<Vec<Event>> {
        self.c().query_map(
            &format!("SELECT {EVENT_COLS} FROM issue_events WHERE issue_id = ?1 ORDER BY id"),
            params![issue],
            event_row,
        )
    }

    /// Every issue in a project with its timeline, by number, read in one
    /// transaction (M195's export): one snapshot of both, on Postgres too
    /// (`REPEATABLE READ`; its default reads each statement's own).
    pub fn export_issues(&self, project: &str) -> Result<Vec<Exported>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        if tx.dialect() == Dialect::Postgres {
            tx.execute_batch("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")?;
        }
        let issues = tx.query_map(
            &format!("SELECT {COLS} FROM issues WHERE project_id = ?1 ORDER BY number"),
            params![project],
            issue_row,
        )?;
        let cols = EVENT_COLS.split(", ").map(|c| format!("e.{c}")).collect::<Vec<_>>().join(", ");
        let mut events: HashMap<String, Vec<Event>> = HashMap::new();
        for e in tx.query_map(
            &format!(
                "SELECT {cols} FROM issue_events e JOIN issues i ON i.id = e.issue_id
                 WHERE i.project_id = ?1 ORDER BY e.id"
            ),
            params![project],
            event_row,
        )? {
            events.entry(e.issue.clone()).or_default().push(e);
        }
        tx.commit()?;
        Ok(issues
            .into_iter()
            .map(|issue| {
                let events = events.remove(&issue.id).unwrap_or_default();
                Exported { issue, events }
            })
            .collect())
    }
}

/// A project as issues need it (M195): whose it is, its name, and its
/// issue home as chosen (`None`: the default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueProject {
    pub id: String,
    pub name: String,
    pub account: Option<String>,
    pub team: Option<String>,
    pub home: Option<String>,
}

impl Db {
    pub fn issue_project(&self, id: &str) -> Result<Option<IssueProject>> {
        self.c()
            .query_row(
                "SELECT id, name, account_id, team_id, issue_home FROM projects WHERE id = ?1",
                params![id],
                |r| {
                    Ok(IssueProject {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        account: r.get(2)?,
                        team: r.get(3)?,
                        home: r.get(4)?,
                    })
                },
            )
            .optional()
    }

    /// Choose a project's issue home (`native` or `github`), or `None`
    /// for the default again.
    pub fn set_issue_home(&self, project: &str, home: Option<&str>) -> Result<()> {
        self.c().execute("UPDATE projects SET issue_home = ?2 WHERE id = ?1", params![project, home])?;
        Ok(())
    }

    /// The repos linked to a project now (`owner/name`), by name.
    pub fn linked_repo_names(&self, project: &str) -> Result<Vec<String>> {
        self.c().query_map(
            "SELECT full_name FROM linked_repos WHERE project_id = ?1 AND unlinked_at IS NULL ORDER BY full_name",
            params![project],
            |r| r.get(0),
        )
    }

    /// How many native issues a project has.
    pub fn issue_count(&self, project: &str) -> Result<u64> {
        self.c().query_row("SELECT count(*) FROM issues WHERE project_id = ?1", params![project], |r| r.get(0))
    }

    /// Every label on a project's issues, open or closed, sorted: what a
    /// label menu offers (UI audit #163).
    pub fn issue_labels(&self, project: &str) -> Result<Vec<String>> {
        let lists = self
            .c()
            .query_map("SELECT labels FROM issues WHERE project_id = ?1", params![project], |r| r.get::<String>(0))?;
        let mut all = std::collections::BTreeSet::new();
        for l in lists {
            all.extend(serde_json::from_str::<Vec<String>>(&l)?);
        }
        Ok(all.into_iter().collect())
    }

    /// Who last closed each of these issues, by id: their latest `closed`
    /// event's actor and agent (UI audit #163: a closed row says who).
    pub fn closers(&self, issues: &[&str]) -> Result<HashMap<String, (String, Option<String>)>> {
        if issues.is_empty() {
            return Ok(HashMap::new());
        }
        let marks = (1..=issues.len()).map(|n| format!("?{n}")).collect::<Vec<_>>().join(", ");
        let args: Vec<Value> = issues.iter().map(|i| Value::Text((*i).into())).collect();
        let sql = format!(
            "SELECT {EVENT_COLS} FROM issue_events WHERE kind = 'closed' AND issue_id IN ({marks}) ORDER BY id"
        );
        // Oldest first: a later close overwrites an earlier one.
        Ok(self
            .c()
            .query_map(&sql, &list(&args), event_row)?
            .into_iter()
            .map(|e| (e.issue, (e.actor, e.actor_agent)))
            .collect())
    }
}

/// What deleting an account takes from issues, inside the platform's
/// `account_deleted` transaction, after [`super::projects::erase_account`]:
/// - the issues of the projects that went with it, their timelines and
///   their inbox entries (M202);
/// - elsewhere its issues and events keep their rows, numbers and text,
///   with no author or actor (and no agent);
/// - it's taken off assignees, and out of assignment events.
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> Result<()> {
    let account = &d.account;
    c.execute(
        "DELETE FROM issue_events WHERE issue_id IN
             (SELECT id FROM issues WHERE project_id NOT IN (SELECT id FROM projects))",
        params![],
    )?;
    c.execute(
        "DELETE FROM inbox_items WHERE item IN (SELECT id FROM issues WHERE project_id NOT IN (SELECT id FROM projects))",
        params![],
    )?;
    c.execute("DELETE FROM issues WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    c.execute("UPDATE issues SET author = '', author_agent = NULL WHERE author = ?1", params![account])?;
    c.execute("UPDATE issue_events SET actor = '', actor_agent = NULL WHERE actor = ?1", params![account])?;
    let (assignees, events) = match c.dialect() {
        Dialect::Sqlite => (
            "UPDATE issues SET assignees = (SELECT json_group_array(value) FROM json_each(assignees) WHERE value != ?1)
             WHERE EXISTS (SELECT 1 FROM json_each(assignees) WHERE value = ?1)",
            "UPDATE issue_events SET detail = json_set(detail, '$.assignee', '')
             WHERE kind IN ('assigned', 'unassigned') AND json_extract(detail, '$.assignee') = ?1",
        ),
        Dialect::Postgres => (
            "UPDATE issues SET assignees = (SELECT coalesce(jsonb_agg(v ORDER BY n), '[]'::jsonb)::text
                 FROM jsonb_array_elements_text(assignees::jsonb) WITH ORDINALITY e(v, n) WHERE v != ?1)
             WHERE EXISTS (SELECT 1 FROM jsonb_array_elements_text(assignees::jsonb) v WHERE v = ?1)",
            "UPDATE issue_events SET detail = jsonb_set(detail::jsonb, '{assignee}', '\"\"')::text
             WHERE kind IN ('assigned', 'unassigned') AND detail::jsonb->>'assignee' = ?1",
        ),
    };
    c.execute(assignees, params![account])?;
    c.execute(events, params![account])?;
    // FTS5's old segments still hold the deleted issues' words until
    // they're merged: merge them now, as chat does (deleting an account is
    // rare). Postgres's `search` column goes with its row.
    if c.dialect() == Dialect::Sqlite {
        c.execute("INSERT INTO issues_fts (issues_fts) VALUES ('optimize')", params![])?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "issues_tests.rs"]
mod tests;
