//! Deploys for a machine's agents (M218): plan one, ask for its approval,
//! and read it, signed by the machine. **Never approve or deny:** there's
//! no route here for it, and the person routes refuse a machine's
//! signature ([`super::BrowserPerson`]).
//!
//! The owner's role in the project is the ceiling (an owner or an editor
//! plans and asks), and the machine needs its owner's `deploys.plan` grant
//! over the project (off until given, like M197's), asked on every call, so an agent never reaches
//! more than its owner. What an agent plans is its owner's, labelled with
//! the machine.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};

use super::{J, Plan};
use crate::{
    Platform,
    auth::Machine,
    db::{deploys::Actor, projects::DEPLOYS_PLAN},
    grants::{AgentScope, agent_may},
};

pub use arugula_control_wire::deploys::{AGENT_DEPLOY, AGENT_DEPLOY_REQUEST, AGENT_PROJECT_DEPLOYS};

/// Plans, asks and reads a machine's agents make, per hour.
const AGENT_DEPLOYS: (&str, usize) = ("deploy-agent", 600);

fn actor(d: &Machine) -> Actor {
    Actor::agent(&d.account, &d.daemon)
}

/// The machine's grant over `project`, asked now.
async fn granted(app: &Platform, d: &Machine, project: &str) -> Result<(), arugula_control_common::ApiError> {
    agent_may(app, d, DEPLOYS_PLAN, &AgentScope::Project(project.to_owned())).await.map(drop)
}

/// The project a deploy is in, for its grant: 404 when there's no such
/// deploy.
fn project_of(app: &Platform, id: &str) -> Result<String, arugula_control_common::ApiError> {
    let gone = || arugula_control_common::err(axum::http::StatusCode::NOT_FOUND, "no such deploy");
    let d = app.db.deploy(id)?.ok_or_else(gone)?;
    Ok(app.db.environment(&d.environment)?.ok_or_else(gone)?.project)
}

/// `POST /api/daemon/projects/{project}/deploys`: plan a deploy, as the
/// machine's owner's agent.
pub async fn plan(State(app): State<Arc<Platform>>, d: Machine, Path(project): Path<String>, Json(b): Json<Plan>) -> J {
    app.limits.check_daemon(AGENT_DEPLOYS, &d.daemon)?;
    granted(&app, &d, &project).await?;
    super::plan(&app, &actor(&d), &project, b).await
}

/// `POST /api/daemon/deploys/{id}/request`: ask the project's owners to
/// approve it. All an agent does past planning.
pub async fn request(State(app): State<Arc<Platform>>, d: Machine, Path(id): Path<String>) -> J {
    app.limits.check_daemon(AGENT_DEPLOYS, &d.daemon)?;
    granted(&app, &d, &project_of(&app, &id)?).await?;
    super::request(&app, &actor(&d), &id).await
}

/// `GET /api/daemon/deploys/{id}`: a deploy, as its owner may read it.
pub async fn get(State(app): State<Arc<Platform>>, d: Machine, Path(id): Path<String>) -> J {
    app.limits.check_daemon(AGENT_DEPLOYS, &d.daemon)?;
    granted(&app, &d, &project_of(&app, &id)?).await?;
    let (dep, env, p, _) = super::deploy_as(&app, &d.account, &id, false).await?;
    // An agent is never offered approval, whoever its owner is.
    Ok(Json(super::deploy_json(&app, &dep, &env, &p, false).await?))
}
