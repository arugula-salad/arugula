//! Native issues (M195, platform step 6): a project's own issues, numbered
//! per project, with a timeline, search and export, and where a project's
//! "new issue" goes (its issue home). The platform keeps them
//! (`crate::db::issues` there); an issue's comments are the messages of its
//! object thread (M196, [`crate::chat`]).
//!
//! A project in a path is its id (`prj_…`), or `personal` for the
//! reader's own. New with M195, so nothing older reads them; the web
//! client's copy is generated (`just proto-ts`, the `ts` feature).

use serde::{Deserialize, Serialize};

/// The agent that wrote something for its owner (M197), labelled
/// "<owner>'s agent on <machine>": chat's.
pub use crate::chat::ChatAgent;

/// `GET`: a project's issues ([`IssueQuery`]), answered [`IssueList`];
/// `POST` [`NewIssue`]: open one, answered [`Issue`].
pub const PROJECT_ISSUES: &str = "/api/projects/{project}/issues";
/// `GET`: one of a project's issues by its number, [`Issue`].
pub const PROJECT_ISSUE: &str = "/api/projects/{project}/issues/{number}";
/// `GET ?format=json|md`: every issue in a project with its timeline and
/// comments, [`IssueExport`] or Markdown.
pub const PROJECT_ISSUES_EXPORT: &str = "/api/projects/{project}/issues/export";
/// `GET`: where a project's "new issue" goes, [`IssueHome`]; `PUT`
/// [`IssueHomeChange`] changes it.
pub const PROJECT_ISSUE_HOME: &str = "/api/projects/{project}/issue-home";
/// `GET`: an issue by its id, [`Issue`]; `PATCH` [`IssuePatch`], answered
/// [`IssuePatched`].
pub const ISSUE: &str = "/api/issues/{id}";
/// `GET`: an issue's timeline, [`IssueEvents`].
pub const ISSUE_EVENTS: &str = "/api/issues/{id}/events";

/// The longest title, in characters, and the longest body, in bytes:
/// GitHub's, so an issue can go there (M205).
pub const TITLE_MAX: usize = 256;
pub const BODY_MAX: usize = 65_536;
/// The longest label, in characters; the most labels and assignees an
/// issue has.
pub const LABEL_MAX: usize = 50;
pub const LABELS_MAX: usize = 50;
pub const ASSIGNEES_MAX: usize = 10;

/// Where an issue was exported to on GitHub (M205).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IssueGithub {
    /// `owner/name`.
    pub repo: String,
    pub number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// The daemon's forge `Issue` (a GitHub issue block's) is another type, so
// the web client's copy is `NativeIssue`.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "NativeIssue"))]
pub struct Issue {
    /// `iss_` and 32 hex characters.
    pub id: String,
    pub project: String,
    /// Its number in the project.
    pub number: u64,
    pub title: String,
    pub body: String,
    /// `open` or `closed`.
    pub state: String,
    /// A closed one's: `completed`, `not_planned` or `duplicate`.
    pub close_reason: Option<String>,
    /// The account that opened it (an agent's owner's), or "" once it's
    /// deleted.
    pub author: String,
    /// The agent that opened it for its owner.
    pub agent: Option<ChatAgent>,
    pub labels: Vec<String>,
    /// Accounts.
    pub assignees: Vec<String>,
    pub github: Option<IssueGithub>,
    pub created_at: u64,
    pub updated_at: u64,
    pub closed_at: Option<u64>,
}

/// `GET /api/projects/{project}/issues`'s query. Each that's given narrows
/// the list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IssueQuery {
    /// `open` (when not given), `closed` or `all`.
    #[serde(default)]
    pub state: Option<String>,
    /// Labels, comma-separated: issues with every one.
    #[serde(default)]
    pub labels: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    /// Search words, in the title or the body: each one (`dep*`: a word
    /// starting with `dep`).
    #[serde(default)]
    pub q: Option<String>,
    /// Issues numbered below this: the next page.
    #[serde(default)]
    pub before: Option<u64>,
    /// At most this many (default 50, at most 200).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IssueList {
    /// The project's id (`personal` resolved).
    pub project: String,
    /// Newest (highest number) first.
    pub issues: Vec<Issue>,
    /// There are more past the last one (`before` its number).
    pub more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "NewNativeIssue"))]
pub struct NewIssue {
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub labels: Vec<String>,
    /// Accounts, each a member of the project.
    #[serde(default)]
    pub assignees: Vec<String>,
}

/// A change to an issue's labels or assignees: the whole list, or some
/// added or taken off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum IssueListEdit {
    Set(Vec<String>),
    Add(Vec<String>),
    Remove(Vec<String>),
}

/// `PATCH /api/issues/{id}`: what's given changes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IssuePatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    /// `open` or `closed`.
    #[serde(default)]
    pub state: Option<String>,
    /// Why it's closed (with `state: closed`): `completed` (when not
    /// given), `not_planned` or `duplicate`.
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub labels: Option<IssueListEdit>,
    #[serde(default)]
    pub assignees: Option<IssueListEdit>,
}

/// An event on an issue's timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IssueEvent {
    /// Orders an issue's events.
    pub id: u64,
    /// `opened`, `edited`, `closed`, `reopened`, `labelled`, `unlabelled`,
    /// `assigned`, `unassigned` or `exported`.
    pub kind: String,
    /// The account that did it (an agent's owner's), or "" once it's
    /// deleted.
    pub actor: String,
    pub agent: Option<ChatAgent>,
    pub at: u64,
    /// `edited`: `title` (the old one) and/or `body: true`; `closed`:
    /// `reason`; `labelled`/`unlabelled`: `label`;
    /// `assigned`/`unassigned`: `assignee`; `exported`: `repo`, `number`.
    #[cfg_attr(feature = "ts", ts(type = "Record<string, unknown>"))]
    pub detail: serde_json::Value,
}

/// What a patch made of the issue, and the events it wrote (none when
/// nothing changed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IssuePatched {
    pub issue: Issue,
    pub events: Vec<IssueEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IssueEvents {
    pub issue: String,
    /// Oldest first.
    pub events: Vec<IssueEvent>,
}

/// Where a project's "new issue" goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IssueHome {
    pub project: String,
    /// `native` (the platform's own issues) or `github` (its linked repo).
    pub home: String,
    /// Someone chose it; otherwise it's the default: `github` while the
    /// project has a linked repo, `native` when it has none.
    pub chosen: bool,
    /// The project's linked repos (`owner/name`).
    pub repos: Vec<String>,
    /// How many native issues it has.
    pub native_issues: u64,
    /// It has issues in both places: native ones and a linked repo. The
    /// page says so.
    pub both: bool,
}

/// `PUT /api/projects/{project}/issue-home`: `native`, `github` (only with
/// a linked repo), or `None` for the default again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IssueHomeChange {
    pub home: Option<String>,
}

/// `GET /api/projects/{project}/issues/export?format=`: `json` (when not
/// given) or `md`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IssueExportQuery {
    #[serde(default)]
    pub format: Option<String>,
}

/// A comment on an issue: a message in its thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IssueComment {
    /// Its number in the thread.
    pub seq: i64,
    /// The account that wrote it, or "" once it's deleted.
    pub author: String,
    pub agent: Option<ChatAgent>,
    pub at: u64,
    pub text: String,
    /// Lines it quoted from a pane.
    pub quote: Option<String>,
    pub edited: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ExportedIssue {
    pub issue: Issue,
    pub events: Vec<IssueEvent>,
    /// Oldest first; deleted ones left out.
    pub comments: Vec<IssueComment>,
}

/// Someone an export names, with what they're called now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ExportedPerson {
    pub account: String,
    pub name: String,
}

/// Every issue in a project, by number, with its timeline and comments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IssueExport {
    pub project: String,
    pub project_name: String,
    pub exported_at: u64,
    pub issues: Vec<ExportedIssue>,
    /// Everyone it names (authors, actors, assignees, commenters).
    pub people: Vec<ExportedPerson>,
}
