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
