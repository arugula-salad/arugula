//! Who someone is in a project (a native issue's project, `IssueProject`):
//! their role, the project as they see it, and its owners. Deploys asked
//! these first (M217); the agents' grant tokens and hosted workspaces ask
//! them too, so they're the base's (#314).

use arugula_control_common::{ApiError, err};
use arugula_e2e::team::TeamRole;
use axum::http::StatusCode;

use crate::{Platform, db::issues::IssueProject};

fn no_project() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such project")
}

/// `account`'s role in `project`, or `None` when they aren't in it.
/// `fresh`: the team's roster is asked of the hub, not the cache (before a
/// change, and always before an approval).
pub async fn role(
    app: &Platform,
    account: &str,
    project: &IssueProject,
    fresh: bool,
) -> Result<Option<TeamRole>, ApiError> {
    Ok(match (&project.account, &project.team) {
        (Some(a), _) => (a == account).then_some(TeamRole::Owner),
        (None, Some(t)) => {
            let t = if fresh { app.cache.team_now(&*app.hub, t).await? } else { app.cache.team(&*app.hub, t).await? };
            t.and_then(|t| t.role(account))
        }
        (None, None) => None,
    })
}

/// The project as `account` sees it, with their role: 404 unless they're
/// in it, 403 for a viewer when `write`.
pub async fn project_as(
    app: &Platform,
    account: &str,
    project: &str,
    write: bool,
) -> Result<(IssueProject, TeamRole), ApiError> {
    let p = app.db.issue_project(project)?.ok_or_else(no_project)?;
    let role = role(app, account, &p, write).await?.ok_or_else(no_project)?;
    if write && role == TeamRole::Viewer {
        return Err(err(StatusCode::FORBIDDEN, "a viewer reads deploys, and doesn't plan or ask for them"));
    }
    Ok((p, role))
}

/// The project's owners, now.
pub async fn owners(app: &Platform, p: &IssueProject) -> anyhow::Result<Vec<String>> {
    Ok(match (&p.account, &p.team) {
        (Some(a), _) => vec![a.clone()],
        (None, Some(t)) => app
            .cache
            .team_now(&*app.hub, t)
            .await?
            .map(|t| {
                t.members
                    .iter()
                    .filter(|m| t.role(&m.account) == Some(TeamRole::Owner))
                    .map(|m| m.account.clone())
                    .collect()
            })
            .unwrap_or_default(),
        (None, None) => vec![],
    })
}
