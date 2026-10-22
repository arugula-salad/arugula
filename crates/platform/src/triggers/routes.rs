//! Run specs, their schedules and rules, and runs started by hand (M214),
//! for a project's members. Only a person, or a CLI device acting as its
//! account: a machine's agent starts runs through MCP (M216) under the
//! project's standing grant (M211), never here.
//!
//! | Route | Who |
//! |---|---|
//! | `GET /api/projects/{project}/run-specs` (each with its schedules and rules) | members |
//! | `POST /api/projects/{project}/run-specs` | owners and editors, on `ARUGULA_HOSTED_ACCOUNTS` |
//! | `GET /api/projects/{project}/run-specs/{spec}` | members |
//! | `PATCH`, `DELETE /api/projects/{project}/run-specs/{spec}` | owners and editors (a change on the list) |
//! | `POST /api/projects/{project}/run-specs/{spec}/schedules` | owners and editors, on the list |
//! | `PATCH`, `DELETE /api/projects/{project}/run-specs/{spec}/schedules/{id}` | owners and editors (turning one on: on the list) |
//! | `POST /api/projects/{project}/run-specs/{spec}/rules` | owners and editors, on the list |
//! | `PATCH`, `DELETE /api/projects/{project}/run-specs/{spec}/rules/{id}` | owners and editors (turning one on: on the list) |
//! | `POST /api/projects/{project}/runs` (a spec, or a command or instruction on a workspace) | owners and editors, on the list |
//!
//! `{project}` is an id, or `personal`; `{spec}` a spec's id or its name
//! in the project. A project someone isn't a member of, and a spec,
//! schedule or rule that isn't the project's, are 404.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::info;

use super::schedule;
use crate::{
    Platform,
    auth::Person,
    db::{
        Taken,
        runs::{Run, TriggerKind},
        triggers::{Operation, RULE_PER_HOUR, Rule, RuleEvent, RunSpec, Schedule, Start, Started},
    },
    forge::links::membership,
};

/// The most specs a project has, and schedules and rules a spec has.
pub const MAX_SPECS: usize = 100;
pub const MAX_SCHEDULES: usize = 10;
pub const MAX_RULES: usize = 20;

/// A spec with its schedules and rules.
#[derive(Debug, Serialize)]
pub struct SpecView {
    #[serde(flatten)]
    pub spec: RunSpec,
    pub schedules: Vec<Schedule>,
    pub rules: Vec<Rule>,
}

/// `GET /api/projects/{project}/run-specs`.
#[derive(Debug, Serialize)]
pub struct Specs {
    pub project: String,
    pub specs: Vec<SpecView>,
}

/// `POST /api/projects/{project}/run-specs`.
#[derive(Debug, Deserialize)]
pub struct NewSpec {
    pub name: String,
    /// The hosted workspace's id or name.
    pub workspace: String,
    #[serde(flatten)]
    pub op: Operation,
}

/// `PATCH /api/projects/{project}/run-specs/{spec}`: what's given changes.
/// Any of the operation's fields replaces the whole operation.
#[derive(Debug, Deserialize)]
pub struct SpecChange {
    pub name: Option<String>,
    pub workspace: Option<String>,
    pub argv: Option<Vec<String>>,
    pub instruction: Option<String>,
    pub agent: Option<String>,
    pub directory: Option<String>,
}

/// `POST …/schedules`.
#[derive(Debug, Deserialize)]
pub struct NewSchedule {
    pub cron: String,
    /// IANA's name; UTC when left out.
    pub timezone: Option<String>,
}

/// `PATCH …/schedules/{id}`.
#[derive(Debug, Deserialize)]
pub struct ScheduleChange {
    pub cron: Option<String>,
    pub timezone: Option<String>,
    pub enabled: Option<bool>,
}

/// `POST …/rules`.
#[derive(Debug, Deserialize)]
pub struct NewRule {
    pub event: RuleEvent,
    /// `owner/name`, a repo linked to the project; any of them when left
    /// out.
    pub repo: Option<String>,
    pub branch: Option<String>,
    /// Also fire on events from forks and from outside the repo: off by
    /// default, and only an owner turns it on.
    #[serde(default)]
    pub outsiders: bool,
    /// The most runs it starts in an hour ([`RULE_PER_HOUR`] when left
    /// out), 1 to [`MAX_PER_HOUR`].
    pub max_per_hour: Option<u32>,
}

/// `PATCH …/rules/{id}`: what's given changes.
#[derive(Debug, Deserialize)]
pub struct RuleChange {
    pub enabled: Option<bool>,
    pub outsiders: Option<bool>,
    pub max_per_hour: Option<u32>,
}

/// The highest hourly cap a rule may have.
pub const MAX_PER_HOUR: u32 = 1000;

fn per_hour(n: Option<u32>) -> Result<Option<u32>, ApiError> {
    match n {
        Some(n) if !(1..=MAX_PER_HOUR).contains(&n) => {
            Err(err(StatusCode::UNPROCESSABLE_ENTITY, &format!("max_per_hour: 1 to {MAX_PER_HOUR}")))
        }
        n => Ok(n),
    }
}

/// Turning `outsiders` on lets anyone who can open a PR start the rule's
/// runs: an owner's call.
async fn outsiders_by_owner(app: &Platform, account: &str, project: &str, on: bool) -> Result<(), ApiError> {
    if on && membership(app, account, project, true).await?.1 != TeamRole::Owner {
        return Err(err(StatusCode::FORBIDDEN, "only an owner lets a rule take events from outside the repo"));
    }
    Ok(())
}

/// `POST /api/projects/{project}/runs`: a spec by its id or name, or an
/// operation on a workspace (its id or name; the project's only one when
/// left out).
#[derive(Debug, Deserialize)]
pub struct StartRun {
    pub spec: Option<String>,
    pub workspace: Option<String>,
    #[serde(flatten)]
    pub op: Operation,
}

fn bad(why: &str) -> ApiError {
    err(StatusCode::BAD_REQUEST, why)
}

/// A member who may change the project's specs and start its runs: not a
/// team's viewer, and (`beta`) on the hosted workspaces' list.
async fn changer(app: &Platform, account: &str, project: &str, beta: bool) -> Result<String, ApiError> {
    let (project, role) = membership(app, account, project, true).await?;
    if role == TeamRole::Viewer {
        return Err(err(StatusCode::FORBIDDEN, "a viewer can't change a project's runs"));
    }
    if beta && !app.hosted.allows(account) {
        return Err(err(StatusCode::FORBIDDEN, "runs on hosted workspaces are in a limited beta"));
    }
    Ok(project)
}

fn spec_of(app: &Platform, project: &str, spec: &str) -> Result<RunSpec, ApiError> {
    app.db.run_spec(project, spec)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such run spec"))
}

/// A hosted workspace of the project, by id or name: its id.
fn workspace_of(app: &Platform, project: &str, workspace: Option<&str>) -> Result<String, ApiError> {
    match workspace {
        Some(w) => Ok(app
            .db
            .hosted_workspace(project, w)?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such hosted workspace in this project"))?
            .id),
        None => {
            let mut all = app.db.hosted_workspaces(project)?;
            match all.len() {
                1 => Ok(all.remove(0).id),
                0 => Err(err(StatusCode::CONFLICT, "this project has no hosted workspace to run on")),
                _ => Err(bad("this project has more than one hosted workspace: say which")),
            }
        }
    }
}

fn check_op(op: &Operation) -> Result<(), ApiError> {
    op.check().map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e))
}

fn view(app: &Platform, spec: RunSpec) -> Result<SpecView, ApiError> {
    Ok(SpecView { schedules: app.db.spec_schedules(&spec.id)?, rules: app.db.spec_rules(&spec.id)?, spec })
}

pub async fn list(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
) -> Result<Json<Specs>, ApiError> {
    let (project, _) = membership(&app, &account, &project, false).await?;
    let specs = app.db.run_specs(&project)?.into_iter().map(|s| view(&app, s)).collect::<Result<_, _>>()?;
    Ok(Json(Specs { project, specs }))
}

pub async fn create(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
    Json(b): Json<NewSpec>,
) -> Result<(StatusCode, Json<SpecView>), ApiError> {
    let project = changer(&app, &account, &project, true).await?;
    let name = b.name.trim();
    if !crate::hosted::good_name(name) {
        return Err(bad("name: 1-64 letters, digits, '-', '_' and '.', starting with a letter or digit"));
    }
    check_op(&b.op)?;
    let workspace = workspace_of(&app, &project, Some(&b.workspace))?;
    if app.db.run_specs(&project)?.len() >= MAX_SPECS {
        return Err(err(StatusCode::CONFLICT, &format!("a project has at most {MAX_SPECS} run specs")));
    }
    let spec = match app.db.add_run_spec(&project, name, &workspace, &b.op, &account, now_ms()) {
        Ok(s) => s,
        Err(e) if e.is::<Taken>() => {
            return Err(err(StatusCode::CONFLICT, &format!("this project has a run spec called {name}")));
        }
        Err(e) => return Err(e.into()),
    };
    info!(%account, %project, spec = %spec.id, "made a run spec");
    Ok((StatusCode::CREATED, Json(view(&app, spec)?)))
}

pub async fn show(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, spec)): Path<(String, String)>,
) -> Result<Json<SpecView>, ApiError> {
    let (project, _) = membership(&app, &account, &project, false).await?;
    Ok(Json(view(&app, spec_of(&app, &project, &spec)?)?))
}

pub async fn update(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, spec)): Path<(String, String)>,
    Json(b): Json<SpecChange>,
) -> Result<Json<SpecView>, ApiError> {
    let project = changer(&app, &account, &project, true).await?;
    let spec = spec_of(&app, &project, &spec)?;
    let name = b.name.as_deref().map(str::trim);
    if name.is_some_and(|n| !crate::hosted::good_name(n)) {
        return Err(bad("name: 1-64 letters, digits, '-', '_' and '.', starting with a letter or digit"));
    }
    let workspace = b.workspace.as_deref().map(|w| workspace_of(&app, &project, Some(w))).transpose()?;
    let changes_op = b.argv.is_some() || b.instruction.is_some() || b.agent.is_some() || b.directory.is_some();
    let op = changes_op.then(|| Operation {
        argv: b.argv.unwrap_or_default(),
        instruction: b.instruction,
        agent: b.agent,
        directory: b.directory,
    });
    if let Some(op) = &op {
        check_op(op)?;
    }
    let changed = match app.db.update_run_spec(&spec.id, name, workspace.as_deref(), op.as_ref(), &account, now_ms()) {
        Ok(s) => s.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such run spec"))?,
        Err(e) if e.is::<Taken>() => {
            return Err(err(StatusCode::CONFLICT, "this project has a run spec by that name"));
        }
        Err(e) => return Err(e.into()),
    };
    info!(%account, %project, spec = %spec.id, "changed a run spec");
    Ok(Json(view(&app, changed)?))
}

pub async fn delete(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, spec)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let project = changer(&app, &account, &project, false).await?;
    let spec = spec_of(&app, &project, &spec)?;
    app.db.delete_run_spec(&spec.id)?;
    info!(%account, %project, spec = %spec.id, "deleted a run spec");
    Ok(StatusCode::NO_CONTENT)
}

/// A zone a person gave, or UTC.
fn timezone(tz: Option<&str>) -> String {
    tz.map(str::trim).filter(|t| !t.is_empty()).unwrap_or("UTC").to_owned()
}

pub async fn add_schedule(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, spec)): Path<(String, String)>,
    Json(b): Json<NewSchedule>,
) -> Result<(StatusCode, Json<Schedule>), ApiError> {
    let project = changer(&app, &account, &project, true).await?;
    let spec = spec_of(&app, &project, &spec)?;
    let (cron, tz, now) = (b.cron.trim(), timezone(b.timezone.as_deref()), now_ms());
    let next = schedule::check(cron, &tz, now).map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e))?;
    if app.db.spec_schedules(&spec.id)?.len() >= MAX_SCHEDULES {
        return Err(err(StatusCode::CONFLICT, &format!("a run spec has at most {MAX_SCHEDULES} schedules")));
    }
    let s = app.db.add_run_schedule(&spec, cron, &tz, next, &account, now)?;
    info!(%account, %project, spec = %spec.id, schedule = %s.id, next, "made a run schedule");
    Ok((StatusCode::CREATED, Json(s)))
}

/// One of `spec`'s schedules.
fn schedule_of(app: &Platform, spec: &RunSpec, id: &str) -> Result<Schedule, ApiError> {
    app.db.run_schedule(id)?.filter(|s| s.spec == spec.id).ok_or_else(|| err(StatusCode::NOT_FOUND, "no such schedule"))
}

pub async fn change_schedule(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, spec, id)): Path<(String, String, String)>,
    Json(b): Json<ScheduleChange>,
) -> Result<Json<Schedule>, ApiError> {
    let on = b.enabled == Some(true);
    let project = changer(&app, &account, &project, on || b.cron.is_some() || b.timezone.is_some()).await?;
    let spec = spec_of(&app, &project, &spec)?;
    let s = schedule_of(&app, &spec, &id)?;
    let cron = b.cron.as_deref().map_or(s.cron.as_str(), str::trim).to_owned();
    let tz = b.timezone.as_deref().map_or(s.timezone.clone(), |t| timezone(Some(t)));
    let enabled = b.enabled.unwrap_or(s.enabled);
    let now = now_ms();
    // Its next tick stays unless its expression or zone changed, or it's
    // passed (a schedule turned on again starts from now).
    let next = if cron != s.cron || tz != s.timezone || s.next_at <= now {
        schedule::check(&cron, &tz, now).map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e))?
    } else {
        s.next_at
    };
    let s = app
        .db
        .set_run_schedule(&s.id, &cron, &tz, enabled, next, now)?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such schedule"))?;
    info!(%account, %project, schedule = %s.id, enabled, next, "changed a run schedule");
    Ok(Json(s))
}

pub async fn delete_schedule(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, spec, id)): Path<(String, String, String)>,
) -> Result<StatusCode, ApiError> {
    let project = changer(&app, &account, &project, false).await?;
    let spec = spec_of(&app, &project, &spec)?;
    let s = schedule_of(&app, &spec, &id)?;
    app.db.delete_run_schedule(&s.id)?;
    info!(%account, %project, schedule = %s.id, "deleted a run schedule");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_rule(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, spec)): Path<(String, String)>,
    Json(b): Json<NewRule>,
) -> Result<(StatusCode, Json<Rule>), ApiError> {
    let project = changer(&app, &account, &project, true).await?;
    let spec = spec_of(&app, &project, &spec)?;
    let repo = b.repo.as_deref().map(str::trim).filter(|r| !r.is_empty());
    if let Some(r) = repo {
        let linked = app.db.linked_repos(&project)?;
        if !linked.iter().any(|l| l.unlinked_at.is_none() && l.full_name.eq_ignore_ascii_case(r)) {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, &format!("{r} isn't linked to this project")));
        }
    }
    let branch = b.branch.as_deref().map(str::trim).filter(|b| !b.is_empty());
    if branch.is_some_and(|b| b.len() > 255 || b.contains(['\0', '\n', '\r', ' '])) {
        return Err(bad("a branch: a name, with `*` for any run of characters"));
    }
    let cap = per_hour(b.max_per_hour)?.unwrap_or(RULE_PER_HOUR);
    outsiders_by_owner(&app, &account, &project, b.outsiders).await?;
    if app.db.spec_rules(&spec.id)?.len() >= MAX_RULES {
        return Err(err(StatusCode::CONFLICT, &format!("a run spec has at most {MAX_RULES} rules")));
    }
    let rule = app.db.add_run_rule(&spec, b.event, repo, branch, b.outsiders, cap, &account, now_ms())?;
    info!(%account, %project, spec = %spec.id, rule = %rule.id, event = b.event.as_str(), "made a run rule");
    Ok((StatusCode::CREATED, Json(rule)))
}

fn rule_of(app: &Platform, spec: &RunSpec, id: &str) -> Result<Rule, ApiError> {
    app.db.run_rule(id)?.filter(|r| r.spec == spec.id).ok_or_else(|| err(StatusCode::NOT_FOUND, "no such rule"))
}

pub async fn change_rule(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, spec, id)): Path<(String, String, String)>,
    Json(b): Json<RuleChange>,
) -> Result<Json<Rule>, ApiError> {
    let widens = b.enabled == Some(true) || b.outsiders == Some(true) || b.max_per_hour.is_some();
    let project = changer(&app, &account, &project, widens).await?;
    let cap = per_hour(b.max_per_hour)?;
    outsiders_by_owner(&app, &account, &project, b.outsiders == Some(true)).await?;
    let spec = spec_of(&app, &project, &spec)?;
    let r = rule_of(&app, &spec, &id)?;
    let r = app
        .db
        .set_run_rule(&r.id, b.enabled, b.outsiders, cap, now_ms())?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such rule"))?;
    info!(%account, %project, rule = %r.id, enabled = r.enabled, outsiders = r.outsiders, "changed a run rule");
    Ok(Json(r))
}

pub async fn delete_rule(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, spec, id)): Path<(String, String, String)>,
) -> Result<StatusCode, ApiError> {
    let project = changer(&app, &account, &project, false).await?;
    let spec = spec_of(&app, &project, &spec)?;
    let r = rule_of(&app, &spec, &id)?;
    app.db.delete_run_rule(&r.id)?;
    info!(%account, %project, rule = %r.id, "deleted a run rule");
    Ok(StatusCode::NO_CONTENT)
}

/// What a start answers of its run (M215 and M216 show runs in full).
pub fn run_json(run: &Run) -> serde_json::Value {
    json!({
        "id": run.id,
        "project": run.project,
        "workspace": run.workspace,
        "trigger": run.trigger.as_str(),
        "source": run.source,
        "op_hash": run.op_hash,
        "daemon": run.daemon,
        "state": run.state.as_str(),
        "started_by": run.started_by,
        "created_at": run.created_at,
    })
}

/// Start a run by hand: a spec, or a command or instruction on one of the
/// project's hosted workspaces. Answered `pending`, its job queued.
pub async fn start(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
    Json(b): Json<StartRun>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let project = changer(&app, &account, &project, true).await?;
    let (spec, op, workspace) = match &b.spec {
        Some(name) => {
            if b.op != Operation::default() || b.workspace.is_some() {
                return Err(bad("a spec, or a command or instruction: not both"));
            }
            let spec = spec_of(&app, &project, name)?;
            let (op, workspace) = (spec.op.clone(), spec.workspace.clone());
            (Some(spec), op, workspace)
        }
        None => {
            check_op(&b.op)?;
            (None, b.op.clone(), workspace_of(&app, &project, b.workspace.as_deref())?)
        }
    };
    let source = match &spec {
        Some(s) => json!({ "spec": s.id, "by": account }),
        None => json!({ "by": account }),
    };
    let start = Start {
        project: &project,
        workspace: &workspace,
        op: &op,
        trigger: TriggerKind::Manual,
        source: &source,
        started_by: &account,
        key: None,
    };
    match app.db.start_run(&start, now_ms())? {
        Started::Made(run) | Started::Already(run) => {
            info!(%account, %project, run = %run.id, spec = ?spec.map(|s| s.id), "started a run by hand");
            app.analytics.capture(&account, "run started", json!({ "trigger": "manual" }));
            Ok((StatusCode::CREATED, Json(run_json(&run))))
        }
        Started::NoMachine => Err(err(
            StatusCode::CONFLICT,
            "the hosted workspace has no machine yet: open it, and try again once it's ready",
        )),
        Started::NoWorkspace => Err(err(StatusCode::NOT_FOUND, "no such hosted workspace in this project")),
        Started::NoProject => Err(err(StatusCode::NOT_FOUND, "no such project")),
        // A trigger's, not a start by hand's.
        other @ (Started::Outsider | Started::Capped | Started::Refused(_)) => {
            Err(err(StatusCode::CONFLICT, other.why_not().unwrap_or_default()))
        }
    }
}
