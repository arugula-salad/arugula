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
