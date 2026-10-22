//! The standing project grant (M211, "One standing grant per project starts
//! runs", aru-099): an owner's device signs, once per project, that the
//! platform may start and instruct runs on the project's hosted workspaces
//! (`arugula_e2e::project_grant`). The platform keeps it and relays it; it
//! can't make or widen one, and hosted `arugulad` checks each one it's
//! sent against its own enrollment before anything starts.
//!
//! | Route | Who |
//! |---|---|
//! | `GET /api/projects/{project}/runs/grant` | members |
//! | `POST /api/projects/{project}/runs/grant` (an owner's device's signed grant) | owners, as people or their CLI |
//! | `POST /api/projects/{project}/runs/grant/revoke` (any owner's device's signed revoke) | owners, as people or their CLI |
//! | `POST /api/projects/{project}/runs/start` | owners and editors |
//!
//! - **What the platform checks** when a grant arrives: it's well formed,
//!   for this project and its owner, freshly made, and the project has no
//!   live one (revoke it first). It can't check the signature (it holds
//!   no device keys, aru-079), so a grant only an outsider signed is kept
//!   and refused by every machine. The machines are what's trusted here.
//! - **Offered** ([`GrantView::offer`]) once the project has a hosted
//!   workspace and no live grant: the shell (control's page, the phone)
//!   signs it, where the device keys are.
//! - **A revoke** stops new starts here at once, and goes to every hosted
//!   machine of the project (`runs.revoke`), which keeps it; it goes again
//!   with every start, for a machine that was asleep. `running: stop`
//!   asks for runs going to stop as well: M212's, once there are runs.
//! - **A start** (`runs.start`) sends the live grant and the project's
//!   revokes to the workspace's machine, which answers. Until M212 it
//!   carries no operation: it's the check M212's runs go through, and the
//!   route M212 makes start one.
//!
//! No agent signs or revokes a grant: these take a person (a session or
//! their CLI device, which signs with its own key), never a machine's
//! `cloud.drive`.

use std::sync::Arc;

use arugula_control_common::{ApiError, err, refusal};
use arugula_control_wire::{
    forge::{DaemonOp, OpAnswer},
    runs::{RunRevoke, RunStart},
};
use arugula_e2e::{
    now_ms,
    project_grant::{CAPS, GrantRefusal, GrantRevoke, ProjectGrant, Running},
    team::TeamRole,
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::info;

use crate::{
    Platform,
    auth::Person,
    db::run_grants::{Added, StoredGrant},
    forge::links::membership,
};

/// How far a grant's `made` may be from the platform's clock.
const SKEW_MS: u64 = 10 * 60 * 1000;
/// How many of a project's past grants the read lists.
const PAST: u64 = 5;

/// A grant as the routes answer it: what was signed, the bytes, and what
/// became of it.
#[derive(Debug, Serialize)]
pub struct Shown {
    pub grant: ProjectGrant,
    /// The signed bytes, exactly.
    pub body: String,
    pub posted_by: Option<String>,
    pub created_at: u64,
    pub revoke: Option<GrantRevoke>,
    pub revoked_at: Option<u64>,
}

impl From<StoredGrant> for Shown {
    fn from(s: StoredGrant) -> Self {
        Shown {
            body: s.grant.body(),
            grant: s.grant,
            posted_by: s.posted_by,
            created_at: s.created_at,
            revoke: s.revoke,
            revoked_at: s.revoked_at,
        }
    }
}

/// `GET /api/projects/{project}/runs/grant`.
#[derive(Debug, Serialize)]
pub struct GrantView {
    pub project: String,
    /// `team:<id>` or `account:<id>`: who signs for it.
    pub owner: String,
    /// The caller's role in it.
    pub role: TeamRole,
    /// The live grant.
    pub grant: Option<Shown>,
    /// Revoked ones, newest first.
    pub past: Vec<Shown>,
    /// Its live hosted workspaces.
    pub workspaces: u64,
    /// The project has a hosted workspace and no live grant: an owner is
    /// offered to sign one.
    pub offer: bool,
}

/// `POST /api/projects/{project}/runs/grant`.
#[derive(Debug, Deserialize)]
pub struct Post {
    pub grant: ProjectGrant,
}

/// `POST /api/projects/{project}/runs/grant/revoke`.
#[derive(Debug, Deserialize)]
pub struct RevokePost {
    pub revoke: GrantRevoke,
}

/// `POST /api/projects/{project}/runs/start`.
#[derive(Debug, Deserialize)]
pub struct StartPost {
    /// A hosted workspace's id or name.
    pub workspace: String,
    /// `runs.start` (the default) or `runs.instruct`.
    #[serde(default)]
    pub cap: Option<String>,
}

/// Who signs for `project`: its owner as a grant names it.
fn owner_of(app: &Platform, project: &str) -> Result<String, ApiError> {
    match app.db.project_holder(project)? {
        Some((_, Some(team))) => Ok(format!("team:{team}")),
        Some((Some(account), None)) => Ok(format!("account:{account}")),
        _ => Err(err(StatusCode::NOT_FOUND, "no such project")),
    }
}

async fn owner_only(app: &Platform, account: &str, project: &str) -> Result<String, ApiError> {
    let (project, role) = membership(app, account, project, true).await?;
    if role != TeamRole::Owner {
        return Err(err(StatusCode::FORBIDDEN, "only the project's owners sign or revoke its standing grant"));
    }
    Ok(project)
}

pub async fn show(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
) -> Result<Json<GrantView>, ApiError> {
    let (project, role) = membership(&app, &account, &project, false).await?;
    let owner = owner_of(&app, &project)?;
    let (live, past): (Vec<StoredGrant>, Vec<StoredGrant>) =
        app.db.run_grants(&project, PAST + 1)?.into_iter().partition(|g| g.revoked_at.is_none());
    let workspaces = app.db.hosted_workspaces(&project)?.len() as u64;
    let grant = live.into_iter().next().map(Shown::from);
    Ok(Json(GrantView {
        offer: grant.is_none() && workspaces > 0,
        project,
        owner,
        role,
        grant,
        past: past.into_iter().take(PAST as usize).map(Shown::from).collect(),
        workspaces,
    }))
}

pub async fn sign(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
    Json(b): Json<Post>,
) -> Result<(StatusCode, Json<Shown>), ApiError> {
    let project = owner_only(&app, &account, &project).await?;
    let g = b.grant;
    let now = now_ms();
    if !g.well_formed() || g.sig.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "that isn't a well-formed standing grant"));
    }
    if g.project != project || g.owner != owner_of(&app, &project)? {
        return Err(err(StatusCode::BAD_REQUEST, "that grant is for another project"));
    }
    if g.caps.len() != CAPS.len() {
        return Err(err(StatusCode::BAD_REQUEST, "a project's standing grant gives runs.start and runs.instruct"));
    }
    if g.made.abs_diff(now) > SKEW_MS {
        return Err(err(StatusCode::BAD_REQUEST, "that grant's time is too far from control's clock"));
    }
    if g.expires.is_some_and(|e| e <= now) {
        return Err(err(StatusCode::BAD_REQUEST, "that grant has expired"));
    }
    match app.db.add_run_grant(&g, &account, now)? {
        Added::Kept => {}
        Added::LiveAlready => {
            return Err(err(
                StatusCode::CONFLICT,
                "this project has a standing grant: revoke it before signing another",
            ));
        }
        Added::Seen => return Err(err(StatusCode::CONFLICT, "that grant is here already")),
    }
    info!(%account, %project, grant = g.id, by = g.by, "kept a project's standing grant");
    app.analytics.capture(&account, "run grant signed", json!({ "expires": g.expires.is_some() }));
    let kept = app.db.run_grant(&project, &g.id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such grant"))?;
    Ok((StatusCode::CREATED, Json(kept.into())))
}

pub async fn revoke(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
    Json(b): Json<RevokePost>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let project = owner_only(&app, &account, &project).await?;
    let r = b.revoke;
    if r.project != project || r.sig.is_empty() || GrantRevoke::parse(&r.body(), &r.sig).as_ref() != Some(&r) {
        return Err(err(StatusCode::BAD_REQUEST, "that isn't a revoke of this project's grant"));
    }
    let Some(g) = app.db.run_grant(&project, &r.grant)? else {
        return Err(err(StatusCode::NOT_FOUND, "no such grant"));
    };
    if g.revoked_at.is_some() {
        return Err(err(StatusCode::CONFLICT, "that grant is revoked already"));
    }
    if !app.db.revoke_run_grant(&r, now_ms())? {
        return Err(err(StatusCode::CONFLICT, "that grant is revoked already"));
    }
    info!(%account, %project, grant = r.grant, stop = r.running == Running::Stop, "revoked a project's standing grant");
    app.analytics.capture(&account, "run grant revoked", json!({ "stop": r.running == Running::Stop }));
    // Every hosted machine of the project hears it now; one that's asleep
    // hears it with its next start.
    let op = DaemonOp::RunRevoke(RunRevoke { grant: g.grant, revoke: r });
    let machines: Vec<String> = app.db.hosted_workspaces(&project)?.into_iter().filter_map(|w| w.machine).collect();
    let answers = join_all(machines.iter().map(|m| app.hub.daemon_op(m, &op))).await;
    let told = answers.iter().filter(|a| matches!(a, Ok(OpAnswer::Done { .. }))).count();
    Ok(Json(json!({ "revoked": true, "machines": machines.len(), "told": told })))
}

/// The refusal codes a machine answers, as `&'static str` for the reply.
fn code_of(why: &str) -> (&'static str, &str) {
    let (code, says) = why.split_once(": ").unwrap_or(("", why));
    let known = [
        GrantRefusal::Form,
        GrantRefusal::WrongProject,
        GrantRefusal::WrongDelegation,
        GrantRefusal::Unsigned,
        GrantRefusal::NotCovered,
        GrantRefusal::Expired,
        GrantRefusal::Revoked,
    ];
    let code = known.iter().map(|r| r.code()).chain(["not_hosted", "wrong_workspace"]).find(|c| *c == code);
    (code.unwrap_or("grant_refused"), says)
}

pub async fn start(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
    Json(b): Json<StartPost>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (project, role) = membership(&app, &account, &project, true).await?;
    if role == TeamRole::Viewer {
        return Err(err(StatusCode::FORBIDDEN, "a viewer can't start runs"));
    }
    let cap = b.cap.unwrap_or_else(|| "runs.start".into());
    if !CAPS.contains(&cap.as_str()) {
        return Err(err(StatusCode::BAD_REQUEST, "cap: runs.start or runs.instruct"));
    }
    let Some(g) = app.db.live_run_grant(&project)? else {
        return Err(refusal(
            StatusCode::CONFLICT,
            "no_grant",
            "this project has no standing grant for runs: an owner signs one from a device",
        ));
    };
    if g.grant.expires.is_some_and(|e| e <= now_ms()) {
        return Err(refusal(StatusCode::CONFLICT, GrantRefusal::Expired.code(), GrantRefusal::Expired.says()));
    }
    let w = app
        .db
        .hosted_workspace(&project, &b.workspace)?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such hosted workspace"))?;
    let Some(machine) = w.machine.clone() else {
        return Err(err(StatusCode::CONFLICT, "that hosted workspace's box hasn't enrolled yet"));
    };
    let op = DaemonOp::RunStart(RunStart {
        project: project.clone(),
        workspace: w.id.clone(),
        run: None,
        cap: cap.clone(),
        grant: g.grant.clone(),
        revokes: app.db.run_grant_revokes(&project)?,
    });
    let answer = app.hub.daemon_op(&machine, &op).await.map_err(|e| err(StatusCode::BAD_GATEWAY, &format!("{e:#}")))?;
    match answer {
        OpAnswer::Done { .. } => {
            info!(%account, %project, workspace = w.id, grant = g.grant.id, %cap, "a start under the standing grant");
            Ok(Json(json!({ "started": true, "workspace": w.id, "machine": machine, "grant": g.grant.id })))
        }
        OpAnswer::Cant { why } => {
            let (code, says) = code_of(&why);
            info!(%account, %project, workspace = w.id, grant = g.grant.id, code, "a hosted machine refused a start");
            Err(refusal(StatusCode::CONFLICT, code, &format!("the workspace refused it: {says}")))
        }
        OpAnswer::Offline => Err(err(
            StatusCode::SERVICE_UNAVAILABLE,
            "that hosted workspace's machine isn't connected: open the workspace to wake it",
        )),
        OpAnswer::Unknown => {
            Err(err(StatusCode::CONFLICT, "that hosted workspace's arugulad is older than runs: rebuild it"))
        }
        OpAnswer::Failed { error } => Err(err(StatusCode::BAD_GATEWAY, &error)),
    }
}
