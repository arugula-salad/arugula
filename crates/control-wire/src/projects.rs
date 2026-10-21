//! Who's reading the platform page, and their projects (M150, "The project is
//! the unit of platform sharing"): their personal project and each team's
//! default one, with the team's members as the platform's projection of the
//! roster keeps them. Read only here: rosters change in the shell, signed by
//! a device ("The platform page is on app.arugula.io, not on the keys'
//! origin").
//!
//! New with M150, so nothing older reads them; the web client's copy is
//! generated (`just proto-ts`, the `ts` feature).

use serde::{Deserialize, Serialize};

/// `GET /api/projects`: who's signed in, and their projects, theirs first.
pub const PROJECTS: &str = "/api/projects";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "PlatformProjects"))]
pub struct Projects {
    pub me: Me,
    pub projects: Vec<Project>,
}

/// The signed-in person, as the hub knows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "PlatformMe"))]
pub struct Me {
    /// Control's id for the account.
    pub account: String,
    /// Its sign-in login (GitHub's), or "".
    pub login: String,
    /// The name it chose, else its login, else "".
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// The daemon's `Project` (a pane's) is another type, so the web client's
// copy is `PlatformProject`.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "PlatformProject"))]
pub struct Project {
    /// `prj_` and 32 hex characters.
    pub id: String,
    pub name: String,
    /// The team it's the default project of; `None` for the person's own.
    pub team: Option<ProjectTeam>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ProjectTeam {
    pub id: String,
    pub name: String,
    /// The reader's role in it: `owner`, `editor` or `viewer`.
    pub role: String,
    pub members: Vec<ProjectMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ProjectMember {
    pub account: String,
    /// What the roster calls them.
    pub name: String,
    pub role: String,
}

/// `GET /api/projects/{project}/repos` (M163): the GitHub repos linked to a
/// project, for its members. `POST` links one ([`LinkRepo`]) and `DELETE
/// /api/projects/{project}/repos/{owner}/{name}` unlinks it. `{project}` is
/// a project's id, or `personal` for the caller's own.
pub const PROJECT_REPOS: &str = "/api/projects/{project}/repos";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LinkedRepos {
    /// The project's id (`personal` asked is answered with the real one).
    pub project: String,
    pub repos: Vec<LinkedRepo>,
}

/// A repo linked to a project, or unlinked and waiting for the purge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LinkedRepo {
    /// GitHub's id for it.
    pub repo_id: u64,
    /// `OWNER/NAME`.
    pub full_name: String,
    pub private: bool,
    /// Who linked it (an account id), when that account is still there.
    pub linked_by: Option<String>,
    pub linked_at: u64,
    /// When it was unlinked: nothing is read from GitHub for it since.
    pub unlinked_at: Option<u64>,
    /// When its rows go, unless it's linked again first.
    pub purge_after: Option<u64>,
    /// The project mirrors its PRs' and issues' bodies and comments (M166).
    #[serde(default)]
    pub mirror_content: bool,
}

/// `PATCH /api/projects/{project}/repos/{owner}/{name}` (M166): a linked
/// repo's settings; the answer is its [`LinkedRepo`]. `mirror_content`
/// on keeps its PRs' and issues' bodies and comments on the platform
/// (search finds their text); off deletes them at once. Turning it on
/// takes what linking takes: a member who maintains the repo on GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RepoSettings {
    pub mirror_content: bool,
}

/// `POST /api/projects/{project}/repos`: link `OWNER/NAME`. A project
/// member who is an admin or maintainer of it on GitHub may. When the
/// GitHub App isn't installed where it lives, the answer is a 409 with
/// `reason: "install_app"` and `install_url`, GitHub's page to install it,
/// which brings them back to the project's settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LinkRepo {
    pub repo: String,
}

/// `GET /api/projects/{project}/items` (M165): a project's PRs and issues
/// from its linked repos, last updated first, each checked against the
/// reader's own GitHub access. A query narrows it: `repo` (`OWNER/NAME`),
/// `kind` (`pr`, `issue`), `state` (`open`, `closed`, `merged`), `author`
/// and `review_requested` (GitHub logins), `q` (text in the title, or in
/// the body, comments and reviews of a repo that mirrors content) and
/// `limit` (at most 200, 100 by default).
pub const PROJECT_ITEMS: &str = "/api/projects/{project}/items";

/// `GET /api/forge/items/{id}`: one item, [`ForgeItem`]; `/checks` its head
/// commit's checks ([`ForgeChecks`]) and `/events` what happened to it
/// ([`ForgeEvents`]). An item the reader may not see is no item at all to
/// them (404).
pub const FORGE_ITEM: &str = "/api/forge/items/{id}";
pub const FORGE_ITEM_CHECKS: &str = "/api/forge/items/{id}/checks";
pub const FORGE_ITEM_EVENTS: &str = "/api/forge/items/{id}/events";
/// `GET /api/forge/items/{id}/content` (M166): its body, comments, reviews'
/// text and review comments ([`ForgeContent`]), through the same check:
/// from GitHub (kept a few minutes in memory), or from the mirror for a
/// repo whose project mirrors content.
pub const FORGE_ITEM_CONTENT: &str = "/api/forge/items/{id}/content";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeItems {
    /// The project's id (`personal` asked is answered with the real one).
    pub project: String,
    pub items: Vec<ForgeItem>,
    /// The project's linked repos whose items the reader may not see, and
    /// why: their names are the project's, which its members see anyway.
    pub withheld: Vec<WithheldRepo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WithheldRepo {
    /// `OWNER/NAME`.
    pub repo: String,
    /// `no_github` (the reader didn't sign in to control with GitHub),
    /// `no_access` (GitHub doesn't give them access), `busy` (too many
    /// lookups lately: it shows again within the hour) or `no_app` (this
    /// control has no GitHub App to ask with).
    pub why: String,
}

/// A PR or an issue, as metadata: never its body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeItem {
    /// The platform's id: `fi_` and 32 hex characters.
    pub id: String,
    /// `OWNER/NAME`.
    pub repo: String,
    pub number: u64,
    /// `pr` or `issue`.
    pub kind: String,
    /// `open`, `closed` or `merged`.
    pub state: String,
    pub title: String,
    pub labels: Vec<String>,
    /// The author's GitHub login.
    pub author: Option<String>,
    pub assignees: Vec<String>,
    /// Requested reviewers' logins.
    pub reviewers: Vec<String>,
    pub draft: bool,
    pub merge_state: Option<String>,
    pub head_ref: Option<String>,
    pub head_sha: Option<String>,
    pub base_ref: Option<String>,
    /// GitHub's times, in milliseconds.
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeChecks {
    pub item: String,
    /// The head commit they're on; `None` for an issue.
    pub head_sha: Option<String>,
    pub checks: Vec<ForgeCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeCheck {
    /// `check_run` or `status`.
    pub kind: String,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub url: Option<String>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeEvents {
    pub item: String,
    pub events: Vec<ForgeEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeEvent {
    pub id: String,
    pub kind: String,
    /// Who did it: a GitHub login.
    pub actor: Option<String>,
    /// A review's state (`approved`, `changes_requested`, `commented`).
    pub review_state: Option<String>,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeContent {
    pub item: String,
    /// `github` (read now, or a few minutes ago) or `mirror`.
    pub from: String,
    pub body: Option<ForgeText>,
    /// Comments, reviews' text and review comments, oldest first. A review
    /// comment's thread is the ones whose `reply_to` is its `id`.
    pub comments: Vec<ForgeText>,
}

/// A body, a comment, a review's text or a review comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeText {
    /// `body`, `comment`, `review` or `review_comment`.
    pub kind: String,
    /// GitHub's id: the PR's or issue's for a body.
    pub id: u64,
    /// A GitHub login.
    pub author: Option<String>,
    /// Markdown, as GitHub has it.
    pub body: String,
    /// A review comment's file and line.
    pub path: Option<String>,
    pub line: Option<u64>,
    /// The review comment it answers.
    pub reply_to: Option<u64>,
    /// GitHub's times, in milliseconds.
    pub created_at: u64,
    pub updated_at: u64,
}
