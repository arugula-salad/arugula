//! Runs for a machine's agents (M216, platform step 7): the routes MCP's
//! `start_run`, `read_run` and `wait` call, signed by the machine.
//!
//! - **Reach.** The machine's owner gives its agents `runs.read` (read a
//!   project's runs and their logs), `runs.start` (start one of its run
//!   specs) or `runs.command` (start a command or an instruction of their
//!   own there), over one project at a time (M197's grants,
//!   [`crate::grants`]), off until given. A spec is what the project's
//!   people wrote down to run; an agent's own command is anything, so it's
//!   a power of its own.
//!   Each call asks [`agent_may`] again, so a revoked grant refuses the
//!   very next call.
//! - **Never past the owner.** A run an agent starts is its owner's start
//!   by hand ([`super::routes::start_as`]): the owner must be one of the
//!   project's owners or editors, on the hosted beta's list, as for a
//!   person. It's recorded as the owner's, with the machine in
//!   `started_by_agent`, and runs under the project's live standing grant
//!   (M211) like any trigger's, or waits for an approval without one.
//! - **Never an approval.** Nothing here approves a run or a deploy: an
//!   approval is an owner device's signature (aru-101, M218).
//!
//! A run whose project the owner doesn't read is 404, as one that isn't
//! there.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::runs::{
    AgentRunStart, RunList, RunListQuery, RunLogPage, RunLogQuery, RunQuery, RunReach, RunReachProject, RunView,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use super::{
    read,
    routes::{StartRun, changer, start_as},
};
use crate::{
    Platform,
    auth::Machine,
    db::{
        projects::{RUNS_COMMAND, RUNS_READ, RUNS_START},
        runs::Run,
        triggers::Operation,
    },
    forge::links::membership,
    grants::{AgentScope, agent_may, owner_reads},
};

type J<T> = Result<Json<T>, ApiError>;

/// Reads a machine's agents make, per hour: a wait asks every few seconds.
const AGENT_RUN_READS: (&str, usize) = ("run-agent-read", 3_600);
/// Runs a machine's agents start, per hour.
pub(crate) const AGENT_RUN_STARTS: (&str, usize) = ("run-agent-start", 60);

/// The project `asked` names for the machine's owner (its id, or
/// `personal`), if they're in it, and the machine's `power` over it.
async fn project_for(app: &Platform, d: &Machine, asked: &str, power: &str) -> Result<String, ApiError> {
    let (project, _) = membership(app, &d.account, asked, false).await?;
    agent_may(app, d, power, &AgentScope::Project(project.clone())).await?;
    Ok(project)
}

/// A run the machine's agents may read: its project's owner reads, and the
/// machine has `runs.read` over it.
async fn readable(app: &Platform, d: &Machine, id: &str) -> Result<Run, ApiError> {
    let none = || err(StatusCode::NOT_FOUND, "no such run");
    let run = app.db.run(id)?.ok_or_else(none)?;
    let scope = AgentScope::Project(run.project.clone());
    if owner_reads(app, &d.account, &scope, false).await?.is_none() {
        return Err(none());
    }
    agent_may(app, d, RUNS_READ, &scope).await?;
    Ok(run)
}

/// `GET /api/daemon/runs/reach`: the projects the machine's run grants
/// reach, as its owner reads them now (none is a 200: `capabilities` says
/// the tools aren't live).
pub async fn reach(State(app): State<Arc<Platform>>, d: Machine) -> J<RunReach> {
    app.limits.check_daemon(AGENT_RUN_READS, &d.daemon)?;
    let mut by: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for g in app.db.run_grants_of(&d.daemon, &d.account)? {
        by.entry(g.scope_id).or_default().push(g.power);
    }
    let mut projects = vec![];
    for (id, powers) in by {
        if let Some(name) = owner_reads(&app, &d.account, &AgentScope::Project(id.clone()), false).await? {
            projects.push(RunReachProject { id, name, powers });
        }
    }
    projects.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    Ok(Json(RunReach { projects }))
}

/// `GET /api/daemon/projects/{project}/runs` (`runs.read`).
pub async fn list(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(project): Path<String>,
    Query(q): Query<RunListQuery>,
) -> J<RunList> {
    app.limits.check_daemon(AGENT_RUN_READS, &d.daemon)?;
    let project = project_for(&app, &d, &project, RUNS_READ).await?;
    Ok(Json(read::list_of(&app, &project, &q)?))
}

/// `POST /api/daemon/projects/{project}/runs`: a run spec with
/// `runs.start`, a command or an instruction of its own with
/// `runs.command`; started as its owner's, by its agent, `pending` with its
/// job queued.
pub async fn start(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(project): Path<String>,
    Json(b): Json<AgentRunStart>,
) -> Result<(StatusCode, Json<RunView>), ApiError> {
    app.limits.check_daemon(AGENT_RUN_STARTS, &d.daemon)?;
    let power = if b.spec.is_some() { RUNS_START } else { RUNS_COMMAND };
    let project = project_for(&app, &d, &project, power).await?;
    // Never past its owner: one who may start runs there by hand.
    changer(&app, &d.account, &project, true).await?;
    let b = StartRun {
        spec: b.spec,
        workspace: b.workspace,
        op: Operation { argv: b.argv, instruction: b.instruction, agent: b.agent, directory: b.directory },
    };
    let run = start_as(&app, &project, &d.account, Some(&d.daemon), &b)?;
    Ok((StatusCode::CREATED, Json(read::view(&app, &run, true)?)))
}

/// `GET /api/daemon/runs/{run}?since=&wait=` (`runs.read`).
pub async fn show(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(id): Path<String>,
    Query(q): Query<RunQuery>,
) -> J<RunView> {
    // A wait is one read however long it's held, counted apart from the
    // others, and at most a few are open at once.
    let _held = if read::waits(&q) {
        app.limits.check_daemon(read::WAITS, &d.daemon)?;
        Some(read::hold(&[format!("account:{}", d.account), format!("daemon:{}", d.daemon)])?)
    } else {
        app.limits.check_daemon(AGENT_RUN_READS, &d.daemon)?;
        None
    };
    readable(&app, &d, &id).await?;
    let run = read::settled(&app, &id, &q).await?;
    Ok(Json(read::view(&app, &run, true)?))
}

/// `GET /api/daemon/runs/{run}/log?offset=&max=` (`runs.read`).
pub async fn log(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(id): Path<String>,
    Query(q): Query<RunLogQuery>,
) -> J<RunLogPage> {
    app.limits.check_daemon(AGENT_RUN_READS, &d.daemon)?;
    let run = readable(&app, &d, &id).await?;
    Ok(Json(read::log_of(&app, &run, &q).await?))
}
