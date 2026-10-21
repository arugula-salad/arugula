//! Linking a GitHub repo to a project (M163, #1209), and taking it out
//! again when the App's installation goes.
//!
//! - **Who links.** A member of the project (its account, for a personal
//!   one; a team's owner or editor) who is an admin or maintainer of the
//!   repo on GitHub, by their numeric GitHub id
//!   ([`GithubApp::repo_role`], through a read-only installation token).
//!   Their GitHub identity is the hub's (`HubPort::github_user`): they
//!   signed in with GitHub. A machine links for its owner when it holds a
//!   live `cloud.drive` grant ([`CloudDriver`], as the Cloud routes take
//!   it), so the agents on it can (`arugula` MCP).
//! - **Installing first.** Where the App isn't installed for the repo, the
//!   answer is GitHub's page to install it ([`GithubApp::install_url`]),
//!   with a `state` that brings the person back to the project's settings
//!   (the page's `/installed`).
//! - **Unlinking** sets `unlinked_at` (ingest stops at once: the poller
//!   and the webhook skip it, M164) and `purge_after`. Linking it again to
//!   the same project before then keeps its rows.
//! - **The installation's events** ([`installation_event`]): an
//!   installation deleted or suspended unlinks every repo linked through
//!   it, and a repo taken out of one unlinks that repo.
//! - **The purge** ([`start`]): on every machine, every [`PURGE_EVERY`],
//!   each unlinked repo whose `purge_after` has passed loses its items,
//!   checks, events, content, drafts and inbox rows, then its link
//!   (`Db::purge_one`: one transaction, which another machine's passes
//!   over).
//!
//! Nothing here reads a forge item: the list is project metadata, for the
//! project's members, until M165 checks each viewer's own access.
//!
//! [`GithubApp::repo_role`]: super::GithubApp::repo_role
//! [`GithubApp::install_url`]: super::GithubApp::install_url

use std::{sync::Arc, time::Duration};

use arugula_control_common::{ApiError, err, refusal};
use arugula_control_wire::projects::{LinkRepo, LinkedRepo, LinkedRepos};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use tracing::{info, warn};

use super::{FORGE_LOOKUPS, good_repo};
use crate::{
    Platform,
    auth::Person,
    cloud::CloudDriver,
    db::forge::{self as rows, Installation, Linked},
};

/// How often each machine looks for repos to purge.
pub const PURGE_EVERY: Duration = Duration::from_secs(600);

/// What `{project}` may be instead of an id: the caller's personal one.
const PERSONAL: &str = "personal";

fn wire(r: &rows::LinkedRepo) -> LinkedRepo {
    LinkedRepo {
        repo_id: r.repo_id,
        full_name: r.full_name.clone(),
        private: r.private,
        linked_by: r.linked_by.clone(),
        linked_at: r.linked_at,
        unlinked_at: r.unlinked_at,
        purge_after: r.purge_after,
    }
}

/// The project `asked` names (its id, or `personal`) and the account's
/// role in it: its own is an owner's. One it isn't a member of is no
/// project at all to it. `now`: ask the hub for the team's roster, not the
/// cache, as for a write.
async fn membership(app: &Platform, account: &str, asked: &str, now: bool) -> Result<(String, TeamRole), ApiError> {
    let none = || err(StatusCode::NOT_FOUND, "no such project");
    if asked == PERSONAL {
        let id = crate::projects::personal_project(app, account).await?.ok_or_else(none)?;
        return Ok((id, TeamRole::Owner));
    }
    let (owner, team) = app.db.project_holder(asked)?.ok_or_else(none)?;
    if owner.as_deref() == Some(account) {
        return Ok((asked.to_owned(), TeamRole::Owner));
    }
    let Some(team) = team else { return Err(none()) };
    let t = if now { app.cache.team_now(&*app.hub, &team).await? } else { app.cache.team(&*app.hub, &team).await? };
    let role = t.and_then(|t| t.role(account)).ok_or_else(none)?;
    Ok((asked.to_owned(), role))
}

/// A member who may change what's linked: not a team's viewer.
async fn changer(app: &Platform, account: &str, asked: &str) -> Result<String, ApiError> {
    let (project, role) = membership(app, account, asked, true).await?;
    if role == TeamRole::Viewer {
        return Err(err(StatusCode::FORBIDDEN, "a viewer can't change a project's linked repos"));
    }
    Ok(project)
}

/// `GET /api/projects/{project}/repos`: the project's linked repos, and
/// those waiting for the purge, for its members.
pub async fn list(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path(project): Path<String>,
) -> Result<Json<LinkedRepos>, ApiError> {
    let (project, _) = membership(&app, &account, &project, false).await?;
    let repos = app.db.linked_repos(&project)?.iter().map(wire).collect();
    Ok(Json(LinkedRepos { project, repos }))
}

/// `POST /api/projects/{project}/repos {repo}`: link it (module docs).
pub async fn link(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path(project): Path<String>,
    Json(b): Json<LinkRepo>,
) -> Result<Response, ApiError> {
    let project = changer(&app, &account, &project).await?;
    let Some(gh) = &app.github else { return Err(err(StatusCode::NOT_FOUND, "this control has no GitHub App")) };
    let repo = b.repo.trim();
    if !good_repo(repo) {
        return Err(err(StatusCode::BAD_REQUEST, "repo: OWNER/NAME"));
    }
    let Some(user) = app.hub.github_user(&account).await? else {
        return Err(err(StatusCode::FORBIDDEN, "sign in to control with GitHub to link a GitHub repo"));
    };
    app.limits.check_account(FORGE_LOOKUPS, &account)?;
    let install = match gh.installation_now(&app.http, repo).await {
        Ok(Some(i)) => i,
        Ok(None) => {
            let owner = repo.split('/').next().unwrap_or_default();
            let url = gh.install_url(&format!("{project}:{repo}"));
            let error =
                format!("the GitHub App {} isn't installed for {repo}: install it on {owner} first, at {url}", gh.slug);
            let body = json!({ "error": error, "reason": "install_app", "install_url": url });
            return Ok((StatusCode::CONFLICT, Json(body)).into_response());
        }
        Err(e) => {
            warn!(error = %e, "asking GitHub about the App's installation");
            return Err(err(StatusCode::BAD_GATEWAY, "couldn't ask GitHub about the App's installation"));
        }
    };
    let role = gh
        .repo_role(&super::Caller::of(&app, &account), &install, &user, repo)
        .await
        .map_err(|why| err(StatusCode::FORBIDDEN, &why))?;
    if !role.may_link() {
        let why =
            format!("linking {} takes admin or maintain on GitHub; {} has {}", role.full_name, user.login, role.role);
        return Err(refusal(StatusCode::FORBIDDEN, "github_role", &why));
    }
    let l = rows::LinkedRepo {
        repo_id: role.id,
        full_name: role.full_name.clone(),
        private: role.private,
        project_id: project.clone(),
        installation_id: install.id,
        linked_by: Some(account.clone()),
        linked_at: 0,
        unlinked_at: None,
        purge_after: None,
    };
    let i = Installation {
        id: install.id,
        account_id: install.account_id,
        login: install.account.clone(),
        account_type: install.account_type.clone(),
        permissions: "{}".into(),
    };
    match app.db.link_repo(&l, &i, &account, now_ms())? {
        Linked::Now { repo, again } => {
            info!(%account, %project, repo = repo.repo_id, again, "linked a GitHub repo");
            Ok(Json(wire(&repo)).into_response())
        }
        Linked::Elsewhere => {
            Err(err(StatusCode::CONFLICT, &format!("{} is linked to another project", role.full_name)))
        }
    }
}

/// `DELETE /api/projects/{project}/repos/{owner}/{name}`: unlink it.
/// Nothing is read from GitHub for it from now on, and its rows go at the
/// purge unless it's linked again first.
pub async fn unlink(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, owner, name)): Path<(String, String, String)>,
) -> Result<Json<LinkedRepo>, ApiError> {
    let project = changer(&app, &account, &project).await?;
    let repo = format!("{owner}/{name}");
    let gone = app
        .db
        .unlink_repo(&project, &repo, now_ms())?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, &format!("{repo} isn't linked to this project")))?;
    info!(%account, %project, repo = gone.repo_id, "unlinked a GitHub repo");
    Ok(Json(wire(&gone)))
}

/// An installation as an event says it.
fn installation_of(v: &Value) -> Option<Installation> {
    Some(Installation {
        id: v["id"].as_u64()?,
        account_id: v["account"]["id"].as_u64().unwrap_or(0),
        login: v["account"]["login"].as_str().unwrap_or_default().to_owned(),
        account_type: v["account"]["type"].as_str().unwrap_or_default().to_owned(),
        permissions: v["permissions"].as_object().map_or_else(|| "{}".into(), |p| Value::from(p.clone()).to_string()),
    })
}

/// What an `installation` or `installation_repositories` event changes:
/// the installation kept, and the repos it unlinks. Anything else, or an
/// action this doesn't know, changes nothing.
pub fn installation_event(app: &Platform, event: &str, v: &Value, now: u64) -> anyhow::Result<()> {
    let Some(i) = installation_of(&v["installation"]) else { return Ok(()) };
    let action = v["action"].as_str().unwrap_or_default();
    let unlinked = match (event, action) {
        ("installation", "created" | "new_permissions_accepted" | "unsuspend")
        | ("installation_repositories", "added") => {
            app.db.installation_seen(&i, now)?;
            0
        }
        ("installation", "deleted") => app.db.installation_gone(&i, true, now)?,
        ("installation", "suspend") => app.db.installation_gone(&i, false, now)?,
        ("installation_repositories", "removed") => {
            let ids: Vec<u64> =
                v["repositories_removed"].as_array().into_iter().flatten().filter_map(|r| r["id"].as_u64()).collect();
            app.db.installation_seen(&i, now)?;
            app.db.repos_removed(i.id, &ids, now)?
        }
        _ => return Ok(()),
    };
    info!(event, action, installation = i.id, unlinked, "the GitHub App's installation changed");
    Ok(())
}

/// Purge every unlinked repo that's due: how many went.
pub fn purge(app: &Platform, now: u64) -> anyhow::Result<usize> {
    let mut n = 0;
    while let Some(repo) = app.db.purge_one(now)? {
        info!(repo, "purged an unlinked GitHub repo's rows");
        n += 1;
    }
    Ok(n)
}

/// The purge, every [`PURGE_EVERY`], while background work isn't held.
pub fn start(app: &Arc<Platform>) {
    let app = Arc::downgrade(app);
    tokio::spawn(async move {
        loop {
            let Some(p) = app.upgrade() else { return };
            if !p.is_paused()
                && !p.is_stopping()
                && let Err(e) = purge(&p, now_ms())
            {
                warn!(error = %e, "purging unlinked GitHub repos");
            }
            drop(p);
            tokio::time::sleep(PURGE_EVERY).await;
        }
    });
}
