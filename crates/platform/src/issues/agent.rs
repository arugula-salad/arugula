//! Native issues for a machine's agents (M200, platform step 6): the
//! routes MCP's issue tools call, signed by the machine. Each is one of
//! [`super`]'s functions with the agent as the [`Actor`], so who may do
//! what is checked in one place ([`super::place`]): the owner's role in
//! the project is the ceiling, and the machine needs its grant over the
//! project too (M197: `threads.read` to read, `issues.write` to write),
//! asked again on every call. What an agent writes is its owner's,
//! labelled with the machine.
//!
//! A comment is a post to the issue's thread, by its id
//! (`/api/daemon/chat/rooms/{iss_…}/messages`, M198), with
//! `threads.post`.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::issues as wire;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use super::{J, PERSONAL};
use crate::{Platform, auth::Machine, db::issues::Actor};

/// Reads a machine's agents make, per hour (a list, a search, an issue).
const AGENT_ISSUE_READS: (&str, usize) = ("issue-agent-read", 3_600);
/// Writes a machine's agents make, per hour (a new issue, a change). Each
/// lands at once and may push people, so fewer than reads.
pub const AGENT_ISSUE_WRITES: (&str, usize) = ("issue-agent-write", 300);

fn actor(d: &Machine) -> Actor<'_> {
    Actor { account: &d.account, agent: Some(&d.daemon) }
}

/// The team's default project, when `d` is a team's machine (control's
/// kind `team`) and its owner still reads it: where a context with no
/// project of its own means (a team's Docs live there, Arugula #862).
/// `None` for a personal machine, or one whose team or owner's place in it
/// is gone. Asked without the machine's `threads.read`: it names the
/// project only, and what's done there asks for its own power.
async fn team_home(app: &Platform, d: &Machine) -> Result<Option<wire::IssueHome>, ApiError> {
    let machines = app.cache.daemons(&*app.hub, std::slice::from_ref(&d.daemon)).await?;
    let Some(team) = machines.get(&d.daemon).and_then(|m| m.team.as_deref()) else { return Ok(None) };
    let Some(project) = crate::projects::team_project(app, team).await? else { return Ok(None) };
    match crate::project_docs::place(app, &d.account, &project, false).await {
        Ok(place) => {
            let p =
                app.db.issue_project(&place.project)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such project"))?;
            Ok(Some(super::home_of(app, &p)?))
        }
        Err(e) if e.0 == StatusCode::NOT_FOUND => Ok(None),
        Err(e) => Err(e),
    }
}

/// `GET /api/daemon/issue-home?project=&repo=`: the project an agent's
/// `#N` means and where its new issues and Docs go: `project`, else the
/// project `repo` is linked to (one its owner is in), else, on a team's
/// machine, the team's default project, else the owner's personal project.
pub async fn home(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Query(q): Query<wire::AgentIssueHomeQuery>,
) -> J<wire::IssueHome> {
    app.limits.check_daemon(AGENT_ISSUE_READS, &d.daemon)?;
    let project = match (q.project.as_deref().filter(|p| !p.is_empty()), q.repo.as_deref().filter(|r| !r.is_empty())) {
        (Some(p), _) => p.to_owned(),
        (None, Some(repo)) => {
            // Only a project the owner is in says the repo is linked; any
            // other is the same as none.
            let not_linked = || {
                err(
                    StatusCode::NOT_FOUND,
                    &format!("{repo} isn't linked to a project of this machine's owner: name the project"),
                )
            };
            let linked = match app.db.linked_repo_named(repo)? {
                Some(link) => match super::home(&app, actor(&d), &link.project_id).await {
                    Err(e) if e.0 == StatusCode::NOT_FOUND => None,
                    h => Some(h?),
                },
                None => None,
            };
            return match (linked, team_home(&app, &d).await?) {
                (Some(h), _) | (None, Some(h)) => Ok(Json(h)),
                (None, None) => Err(not_linked()),
            };
        }
        (None, None) => {
            if let Some(h) = team_home(&app, &d).await? {
                return Ok(Json(h));
            }
            PERSONAL.to_owned()
        }
    };
    Ok(Json(super::home(&app, actor(&d), &project).await?))
}

/// `GET /api/daemon/projects/{project}/issues`: a page, as the owner may
/// read them.
pub async fn list(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(project): Path<String>,
    Query(q): Query<wire::IssueQuery>,
) -> J<wire::IssueList> {
    app.limits.check_daemon(AGENT_ISSUE_READS, &d.daemon)?;
    Ok(Json(super::list(&app, actor(&d), &project, &q).await?))
}

/// `POST /api/daemon/projects/{project}/issues`: opened at once, as the
/// owner's, labelled with the machine.
pub async fn create(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(project): Path<String>,
    Json(new): Json<wire::NewIssue>,
) -> J<wire::Issue> {
    app.limits.check_daemon(AGENT_ISSUE_WRITES, &d.daemon)?;
    Ok(Json(super::create(&app, actor(&d), &project, &new).await?))
}

/// `GET /api/daemon/projects/{project}/issues/{number}`.
pub async fn by_number(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path((project, number)): Path<(String, u64)>,
) -> J<wire::Issue> {
    app.limits.check_daemon(AGENT_ISSUE_READS, &d.daemon)?;
    Ok(Json(super::read_number(&app, actor(&d), &project, number).await?))
}

/// `GET /api/daemon/issues/{id}`.
pub async fn get(State(app): State<Arc<Platform>>, d: Machine, Path(id): Path<String>) -> J<wire::Issue> {
    app.limits.check_daemon(AGENT_ISSUE_READS, &d.daemon)?;
    Ok(Json(super::read(&app, actor(&d), &id).await?))
}

/// `PATCH /api/daemon/issues/{id}`: edit, label, assign, close or reopen,
/// at once.
pub async fn patch(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(id): Path<String>,
    Json(p): Json<wire::IssuePatch>,
) -> J<wire::IssuePatched> {
    app.limits.check_daemon(AGENT_ISSUE_WRITES, &d.daemon)?;
    Ok(Json(super::change(&app, actor(&d), &id, &p).await?))
}

/// `GET /api/daemon/issues/{id}/events`.
pub async fn events(State(app): State<Arc<Platform>>, d: Machine, Path(id): Path<String>) -> J<wire::IssueEvents> {
    app.limits.check_daemon(AGENT_ISSUE_READS, &d.daemon)?;
    Ok(Json(super::timeline(&app, actor(&d), &id).await?))
}
