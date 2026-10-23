//! Deploys over the platform's routes, and their approval (M218, platform
//! step 7, native Arugula #14). The storage and the stale rule are M217's
//! ([`db::deploys`]); the signed approval's format is
//! [`arugula_e2e::deploy`], S37's work approval (q3) over a deploy's
//! operation: the environment, the plan's digest, the source and the hash
//! of its `planned` event.
//!
//! **Who may do what,** by their role in the deploy's project (a personal
//! project's account is its owner; anyone else gets 404, as if it weren't
//! there):
//! - **read** a deploy, with its operation, approvals and log: any member;
//! - **make an environment:** an owner;
//! - **plan a deploy and ask for its approval:** an owner or an editor, or
//!   their agent ([`agent`]'s routes): the agent's owner must be an owner
//!   or editor of the project, and its machine needs the owner's
//!   `deploys.plan` grant over the project (off until given), asked on
//!   every call;
//! - **approve or deny:** an owner, **with one of their devices' signature**
//!   over the operation and a passkey's assertion over the approval
//!   ([`verify`]). Nothing else counts.
//!
//! **What a device is here.** A browser's (or a phone's, or the desktop
//! app's) device key: the Ed25519 key it holds non-extractable, or derives
//! from a passkey (#764), certified by browsers all the way back to the
//! account's root (a CLI's or the app's certificate anywhere on the way
//! refuses it; recovery codes may be links on the way, and so may an app
//! window from before it had a kind, just above a browser it approved back
//! then, [`DeviceTrust::legacy_app_children`]) and not revoked, which the platform checks itself
//! from what the hub keeps ([`HubPort::device_trust`],
//! `arugula_e2e::Trust::evaluate`). Not a CLI's device: its key is a file
//! on a machine, where an agent can read it. Not a machine's, and not a
//! recovery code. The platform holds no such key, so it can't make an
//! approval: it can only refuse service.
//!
//! **Agents never approve.** No grant, MCP tool, CLI call or thread message
//! counts as an approval, on the owner's own machine too (aru-091, aru-101):
//! - the approve and deny routes take a browser's session only: a machine's
//!   signature is refused as an agent's, and a CLI's login as not a device;
//! - there's no machine route that approves, and no MCP tool or CLI command
//!   that calls one;
//! - an approval is recorded only from a [`VerifiedApproval`] (a device signature
//!   that checked out), as a person ([`db::deploys::EventKind::allows`]
//!   refuses an agent anyway).
//!
//! **The person, not only their browser.** Every approval and deny carries
//! a passkey's assertion over the approval's bytes (challenge: their
//! SHA-256), with user verification, from a passkey registered to the
//! approving account. The hub keeps passkeys' public keys and checks it
//! ([`HubPort::verify_passkey`]); it's stored with the approval and in its
//! event, so the log can be checked again. An account with no passkey
//! can't approve.
//!
//! **Applied by M219** (`deploys::apply`): an approval enqueues the
//! deploy's job, which checks the approval's `expires`, the approving
//! device and the owner's role again, then makes the run that applies it
//! on the project's hosted workspace. That machine checks the approval
//! itself against what it pinned (`arugula_e2e::deploy::check_apply`), and
//! the log takes the deploy's `started` only from that run, before its
//! `expires`. Left (`TODO(passkeys)`): the hub publishes passkeys' public
//! keys, so a stored passkey proof (in the approval and its event) can be
//! checked by anyone, the machine included, not only by the hub.
//!
//! **Asking** records the deploy's `requested` event and pushes each of the
//! project's owners a card (`deploy` in the push: a tap opens the deploy on
//! the platform page, where Approve and Deny go to control's page, which
//! holds the device keys and signs with a passkey).
//!
//! [`db::deploys`]: crate::db::deploys
//! [`db::deploys::EventKind::allows`]: crate::db::deploys::EventKind::allows
//! [`HubPort::device_trust`]: crate::hub::HubPort::device_trust
//! [`DeviceTrust::legacy_app_children`]: crate::hub::DeviceTrust::legacy_app_children
//! [`HubPort::verify_passkey`]: crate::hub::HubPort::verify_passkey

pub mod agent;
pub mod apply;
pub mod catalog;

use std::sync::Arc;

use arugula_control_common::{ApiError, Principal, err, refusal};
use arugula_e2e::{
    Trust,
    deploy::{Approval, Decision, MAX_LIFE_MS},
    now_ms,
    team::TeamRole,
};
use axum::{
    Json,
    extract::{FromRequestParts, Path, State},
    http::{StatusCode, request::Parts},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::warn;

use crate::{
    Platform,
    auth::{Asked, Person},
    db::{
        deploys::{
            Actor, Deploy, Environment, EventKind, NewDeploy, PasskeyProof, Refusal, Source, SourceKind,
            VerifiedApproval,
        },
        issues::IssueProject,
    },
    hub::{PasskeyAssertion, PushOptions},
};

pub const ENVIRONMENTS: &str = "/api/projects/{project}/environments";
pub const PROJECT_DEPLOYS: &str = "/api/projects/{project}/deploys";
pub const WAITING: &str = "/api/deploys/waiting";
pub const DEPLOY: &str = "/api/deploys/{id}";
pub const DEPLOY_REQUEST: &str = "/api/deploys/{id}/request";
pub const DEPLOY_APPROVE: &str = "/api/deploys/{id}/approve";
pub const DEPLOY_DENY: &str = "/api/deploys/{id}/deny";
pub const ENVIRONMENT_SECRET: &str = "/api/environments/{id}/secrets/{name}";

/// Why an approval was refused, for clients to act on.
pub const AGENT: &str = "agent";
pub const NOT_A_DEVICE: &str = "not_a_device";
pub const NOT_AN_OWNER: &str = "not_an_owner";
pub const STALE: &str = "stale";
pub const OTHER_OPERATION: &str = "other_operation";

/// Approvals and denies per account, per hour: each asks the hub for the
/// account's devices.
const APPROVALS: (&str, usize) = ("deploy-approve", 120);
/// Plans and asks per account, per hour (a person's).
const DEPLOY_WRITES: (&str, usize) = ("deploy-write", 600);
/// Asks per project, per hour, by each asker (a person, or an agent's
/// machine): each pushes every owner, so an agent can't bury an owner in
/// approval cards, and one asker's asks don't use up another's. Only asks
/// that were recorded count.
const PROJECT_ASKS: (&str, usize) = ("deploy-ask-project", 30);

/// Who an ask is counted against in a project: the person, or the agent's
/// machine.
fn asker(project: &str, by: &Actor) -> String {
    match by {
        Actor::Agent { machine, .. } => format!("{project}/machine:{machine}"),
        a => format!("{project}/{}", a.account().unwrap_or_default()),
    }
}

type J = Result<Json<Value>, ApiError>;

/// A signed-in person in a browser: the approve and deny routes' caller.
/// A machine's signature (an agent's, on any machine, the owner's own
/// included) and a CLI device's login are refused, each saying why.
pub struct BrowserPerson {
    pub account: String,
}

impl<S: Send + Sync> FromRequestParts<S> for BrowserPerson {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        match parts.extensions.get::<Asked>() {
            Some(Asked::Principal(Principal::Person { account })) => Ok(BrowserPerson { account: account.clone() }),
            Some(Asked::Principal(Principal::Daemon { .. })) => Err(refusal(
                StatusCode::FORBIDDEN,
                AGENT,
                "agents never approve or deny a deploy: they plan one and ask, and an owner approves it on their phone \
                 or in a browser",
            )),
            Some(Asked::Principal(Principal::Cli { .. })) => Err(refusal(
                StatusCode::FORBIDDEN,
                NOT_A_DEVICE,
                "a CLI's login doesn't approve a deploy: approve it on your phone or in a browser, with a passkey",
            )),
            Some(Asked::Refused { status, error, .. }) => Err(err(*status, error)),
            None => Err(err(StatusCode::UNAUTHORIZED, "sign in first")),
        }
    }
}

fn no_project() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such project")
}

fn no_deploy() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such deploy")
}

/// `account`'s role in `project`, or `None` when they aren't in it.
/// `fresh`: the team's roster is asked of the hub, not the cache (before a
/// change, and always before an approval).
pub(crate) async fn role(
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
async fn project_as(
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

/// A deploy, its environment and its project, as `account` sees them.
async fn deploy_as(
    app: &Platform,
    account: &str,
    id: &str,
    write: bool,
) -> Result<(Deploy, Environment, IssueProject, TeamRole), ApiError> {
    let d = app.db.deploy(id)?.ok_or_else(no_deploy)?;
    let env = app.db.environment(&d.environment)?.ok_or_else(no_deploy)?;
    let (p, role) = match project_as(app, account, &env.project, write).await {
        Err(e) if e.0 == StatusCode::NOT_FOUND => return Err(no_deploy()),
        r => r?,
    };
    Ok((d, env, p, role))
}

fn refused(r: Refusal) -> ApiError {
    match r {
        Refusal::Gone => no_deploy(),
        Refusal::Stale { .. } => refusal(StatusCode::CONFLICT, STALE, &r.to_string()),
        Refusal::Agent => refusal(StatusCode::FORBIDDEN, AGENT, &r.to_string()),
        Refusal::Actor | Refusal::Unverified => err(StatusCode::FORBIDDEN, &r.to_string()),
        Refusal::Mismatch(_) => refusal(StatusCode::CONFLICT, OTHER_OPERATION, &r.to_string()),
        Refusal::State(_)
        | Refusal::Busy { .. }
        | Refusal::NotItsRun
        | Refusal::ApprovalExpired
        | Refusal::ApprovalConsumed => err(StatusCode::CONFLICT, &r.to_string()),
    }
}

fn environment_json(e: &Environment) -> Value {
    json!({
        "id": e.id, "project": e.project, "name": e.name, "driver": e.driver, "secret_names": e.secret_names,
        "settings": e.settings,
    })
}

/// A deploy as the routes answer it: with its environment, project,
/// operation (the exact bytes an owner's device signs, and their hash),
/// approvals and log. `may_approve`: whether the reader is an owner and
/// it's waiting for approval with nothing applied since its plan.
pub(crate) async fn deploy_json(
    app: &Platform,
    d: &Deploy,
    env: &Environment,
    project: &IssueProject,
    reader_owns: bool,
) -> Result<Value, ApiError> {
    let (_, op) = app.db.deploy_operation(&d.id)?.ok_or_else(no_deploy)?;
    // The plan's text (M220), made again from what it names: what an owner
    // reads before they approve, when it's still the plan's (its digest).
    let roll_back = app.db.deploy_roll_back(&d.id)?;
    let plan_text = Some(crate::drivers::plan_with(&env.driver, env, &d.source, roll_back.as_ref()))
        .filter(|p| p.digest == d.plan_digest)
        .map(|p| p.text);
    let approvals: Vec<Value> = app
        .db
        .deploy_approvals(&d.id)?
        .iter()
        .map(|a| {
            json!({
                "id": a.id, "decision": a.decision.as_str(), "account": a.account, "device": a.device,
                "device_name": a.device_name, "expires": a.expires, "at": a.at, "seq": a.seq, "op": a.op,
                "sig": a.sig, "body": a.signed().body(),
                "passkey": a.passkey.as_ref().map(|p| json!({
                    "id": p.id, "client_data_json": p.client_data_json,
                    "authenticator_data": p.authenticator_data, "signature": p.signature,
                })),
            })
        })
        .collect();
    let history = app.db.deploy_history(&d.id)?;
    // The `planned` event as stored, so a page can hash it again and check
    // it's the one the operation names (who planned it, from hashed bytes).
    let planned_event = history.iter().find(|e| e.kind == "planned").map(|e| {
        json!({
            "environment": e.environment, "seq": e.seq, "deploy": e.deploy, "kind": e.kind, "actor": e.actor,
            "at": e.at, "detail": e.detail, "prev_hash": e.prev_hash, "hash": e.hash,
        })
    });
    let actors: Vec<Option<Actor>> = history.iter().map(|e| Actor::parse(&e.actor)).collect();
    let stale = match app.db.may_approve(&d.id)? {
        Err(Refusal::Stale { seq, deploy }) => Some(json!({ "seq": seq, "deploy": deploy })),
        _ => None,
    };
    let mut ids = vec![d.planned_by.clone()];
    ids.extend(app.db.deploy_approvals(&d.id)?.into_iter().map(|a| a.account));
    ids.extend(actors.iter().flatten().filter_map(|a| a.account().map(str::to_owned)));
    ids.retain(|a| !a.is_empty());
    ids.sort();
    ids.dedup();
    let names = app.cache.accounts(&*app.hub, &ids).await?;
    let name = |a: &str| names.get(a).map(|a| a.name.clone()).unwrap_or_default();
    let agent = match &d.planned_by_agent {
        Some(m) => {
            let machine_name = app.cache.daemons(&*app.hub, std::slice::from_ref(m)).await?.remove(m).map(|d| d.name);
            json!({ "machine": m, "machine_name": machine_name.unwrap_or_default() })
        }
        None => Value::Null,
    };
    let mut machines: Vec<String> = actors
        .iter()
        .flatten()
        .filter_map(|a| match a {
            Actor::Agent { machine, .. } => Some(machine.clone()),
            _ => None,
        })
        .collect();
    machines.sort();
    machines.dedup();
    let machine_names =
        if machines.is_empty() { Default::default() } else { app.cache.daemons(&*app.hub, &machines).await? };
    // Who each event is by, as people read it.
    let by = |a: &Option<Actor>| match a {
        Some(Actor::Person(p)) => Some(name(p)).filter(|n| !n.is_empty()).unwrap_or_else(|| "a deleted account".into()),
        Some(Actor::Agent { account, machine }) => {
            let m = machine_names.get(machine).map_or(machine.as_str(), |d| d.name.as_str());
            format!(
                "{}'s agent on {m}",
                Some(name(account)).filter(|n| !n.is_empty()).unwrap_or_else(|| "someone".into())
            )
        }
        Some(Actor::Run(r)) => format!("run {r}"),
        Some(Actor::Platform) => "the platform".into(),
        None => "someone".into(),
    };
    let events: Vec<Value> = history
        .iter()
        .zip(&actors)
        .map(|(e, a)| {
            json!({
                "seq": e.seq, "kind": e.kind, "actor": e.actor, "by": by(a), "at": e.at, "hash": e.hash,
                "prev_hash": e.prev_hash,
            })
        })
        .collect();
    let log = match app.db.verify_deploy_log(&d.environment)? {
        Ok(n) => json!({ "verified": n }),
        Err(bad) => json!({ "broken_at": bad.seq, "why": format!("{:?}", bad.why) }),
    };
    Ok(json!({
        "id": d.id,
        "project": { "id": project.id, "name": project.name },
        "environment": environment_json(env),
        "source": { "kind": d.source.kind.as_str(), "ref": d.source.reference, "where": d.source.place },
        "plan": d.plan_digest,
        "plan_text": plan_text,
        "roll_back": roll_back,
        "state": d.state.as_str(),
        "run": d.run,
        "planned_by": d.planned_by,
        "planned_by_name": name(&d.planned_by),
        "agent": agent,
        "created_at": d.created_at,
        "updated_at": d.updated_at,
        "operation": { "body": op.body(), "hash": op.hash() },
        "planned_event": planned_event,
        "stale": stale,
        "may_approve": reader_owns && d.state == crate::db::deploys::State::Pending && stale.is_none(),
        "approvals": approvals.iter().map(|a| {
            let mut a = a.clone();
            a["name"] = name(a["account"].as_str().unwrap_or_default()).into();
            a
        }).collect::<Vec<_>>(),
        "events": events,
        "log": log,
    }))
}

/// `GET /api/projects/{project}/environments`: a project's environments,
/// for its members.
pub async fn environments(State(app): State<Arc<Platform>>, s: Person, Path(project): Path<String>) -> J {
    let (p, _) = project_as(&app, &s.account, &project, false).await?;
    let envs: Vec<Value> = app.db.environments(&p.id)?.iter().map(environment_json).collect();
    Ok(Json(json!({ "environments": envs })))
}

#[derive(Deserialize)]
pub struct NewEnvironment {
    name: String,
    driver: String,
    #[serde(default)]
    secret_names: Vec<String>,
    /// Its driver's settings (M220): chant-fly's `fly_app`, `chant_dir`,
    /// `chant_op` and `fly_config`.
    #[serde(default)]
    settings: arugula_e2e::drivers::Settings,
}

/// `POST /api/projects/{project}/environments {name, driver,
/// secret_names?, settings?}`: make one, as an owner.
pub async fn create_environment(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
    Json(b): Json<NewEnvironment>,
) -> J {
    app.limits.check_account(DEPLOY_WRITES, &s.account)?;
    let (p, role) = project_as(&app, &s.account, &project, true).await?;
    if role != TeamRole::Owner {
        return Err(refusal(StatusCode::FORBIDDEN, NOT_AN_OWNER, "only the project's owners make its environments"));
    }
    // A secret can't take the name of a token the driver mints for its
    // runs (M222: the apps-platform driver's `SOURCE_TOKEN`); one it mints
    // only when the environment holds none of its own may be held.
    if let Some(d) = crate::drivers::driver(&b.driver) {
        // A driver some projects only may use (M221: Arugula's release).
        d.allowed_in(&p.id).map_err(|why| err(StatusCode::FORBIDDEN, &why))?;
        let would = Environment {
            id: String::new(),
            project: p.id.clone(),
            name: b.name.clone(),
            driver: b.driver.clone(),
            secret_names: b.secret_names.clone(),
            settings: b.settings.clone(),
            created_at: 0,
            updated_at: 0,
        };
        if let Some(m) = d.mints(&would).into_iter().find(|m| b.secret_names.iter().any(|n| n == m.name())) {
            return Err(err(
                StatusCode::BAD_REQUEST,
                &format!(
                    "{} is the token the {} driver mints for each run: name the secret otherwise",
                    m.name(),
                    d.name()
                ),
            ));
        }
    }
    // Its driver's settings, defaults filled in (M220): the plan names them.
    let settings = match crate::drivers::driver(&b.driver) {
        Some(d) => d.settings(&b.settings).map_err(|why| err(StatusCode::BAD_REQUEST, &why))?,
        None => b.settings.clone(),
    };
    let env = app.db.create_environment_with(&p.id, &b.name, &b.driver, &b.secret_names, &settings, now_ms());
    let env = env.map_err(|e| match e.downcast_ref::<crate::db::Taken>() {
        Some(_) => err(StatusCode::CONFLICT, &format!("the project has an environment called {}", b.name)),
        None => err(StatusCode::BAD_REQUEST, &format!("{e:#}")),
    })?;
    Ok(Json(environment_json(&env)))
}

#[derive(Deserialize)]
pub struct SecretValue {
    value: String,
}

/// The most a secret's value holds, in bytes.
const MAX_SECRET: usize = 16 * 1024;

/// `POST /api/environments/{id}/secrets/{name} {value}`: set one of its
/// secrets (M219), as an owner, in a browser (a CLI's login or an agent's
/// machine can't overwrite what a deploy is given). Sealed with the project's key
/// (M209), never read back: only a deploy's run gets it, on its machine,
/// once, while it runs.
pub async fn set_secret(
    State(app): State<Arc<Platform>>,
    s: BrowserPerson,
    Path((id, name)): Path<(String, String)>,
    Json(b): Json<SecretValue>,
) -> J {
    app.limits.check_account(DEPLOY_WRITES, &s.account)?;
    let env = app.db.environment(&id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such environment"))?;
    let (p, role) = match project_as(&app, &s.account, &env.project, true).await {
        Err(e) if e.0 == StatusCode::NOT_FOUND => return Err(err(StatusCode::NOT_FOUND, "no such environment")),
        r => r?,
    };
    if role != TeamRole::Owner {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            NOT_AN_OWNER,
            "only the project's owners set its environments' secrets",
        ));
    }
    if !env.secret_names.contains(&name) {
        return Err(err(StatusCode::BAD_REQUEST, "the environment doesn't list a secret by that name"));
    }
    if b.value.is_empty() || b.value.len() > MAX_SECRET || b.value.contains('\0') {
        return Err(err(StatusCode::BAD_REQUEST, "a secret's value is 1 to 16384 bytes, with no NUL"));
    }
    let master = app
        .secret_key
        .as_deref()
        .ok_or_else(|| err(StatusCode::SERVICE_UNAVAILABLE, "this platform keeps no secrets"))?;
    crate::keys::seal(&app.db, master, &p.id, &crate::keys::environment_name(&env.id, &name), &b.value)?;
    Ok(Json(json!({ "environment": env.id, "name": name, "set": true })))
}

#[derive(Deserialize)]
pub struct PlanSource {
    kind: String,
    #[serde(rename = "ref")]
    reference: String,
    #[serde(rename = "where")]
    place: String,
}

#[derive(Deserialize)]
pub struct Plan {
    /// Its id (`env_…`), or its name in the project.
    environment: String,
    source: PlanSource,
    /// The plan's SHA-256, hex: the environment's driver's plan for this
    /// source (M219, `crate::drivers::Driver::plan`), which it's made
    /// from when it's left out, and checked against when it isn't.
    #[serde(default)]
    plan: Option<String>,
    /// A roll back (M220): back to `source`, which the environment ran
    /// before, with what that deploy ran (as its log kept it).
    #[serde(default)]
    roll_back: bool,
}

/// Why a plan was refused: it isn't the environment's driver's.
pub const OTHER_PLAN: &str = "other_plan";

/// Plan a deploy in `project` as `by` (a person, or an agent whose owner
/// may write there): the environment must be the project's.
pub(crate) async fn plan(app: &Platform, by: &Actor, project: &str, b: Plan) -> J {
    let account = by.account().unwrap_or_default();
    let (p, role) = project_as(app, account, project, true).await?;
    // An environment by its id, or (M252) by its name in the project.
    let env = match app.db.environment(&b.environment)?.filter(|e| e.project == p.id) {
        Some(e) => Some(e),
        None => app.db.environments(&p.id)?.into_iter().find(|e| e.name == b.environment),
    };
    let env = env.ok_or_else(|| err(StatusCode::NOT_FOUND, "the project has no such environment"))?;
    let kind = SourceKind::parse(&b.source.kind)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "a deploy's source is a commit or an artifact"))?;
    let source = Source { kind, reference: b.source.reference, place: b.source.place };
    // chant→Fly deploys a commit by its full id only (M220).
    if !arugula_e2e::drivers::source_ok(&env.driver, kind.as_str(), &source.reference) {
        return Err(err(StatusCode::BAD_REQUEST, "this environment's driver deploys a commit, by its full 40-hex id"));
    }
    // The driver's plan (M219): it names the driver, so the approval over
    // its digest binds the driver too, and a box checks it again.
    let driver = crate::drivers::driver(&env.driver)
        .ok_or_else(|| err(StatusCode::CONFLICT, &format!("there's no driver {} here", env.driver)))?;
    // An environment made before its driver named its secrets may hold
    // one it doesn't take: no deploy until it's fixed.
    if let Some(bad) = env.secret_names.iter().find(|n| !arugula_e2e::deploy::driver_takes(&env.driver, n)) {
        return Err(err(
            StatusCode::CONFLICT,
            &format!("the environment's secret {bad} isn't one the {} driver takes: take it out first", env.driver),
        ));
    }
    if let Some(why) = driver.refuses(&source) {
        return Err(err(StatusCode::BAD_REQUEST, &why));
    }
    // A driver some projects only may use (M221: Arugula's release).
    driver.allowed_in(&p.id).map_err(|why| err(StatusCode::FORBIDDEN, &why))?;
    // A token its run mints that writes (M222's list, M220's ledger, M221's
    // release): the App must be able to, or the deploy can't run once it's
    // approved.
    if let Some(why) = crate::drivers::mint::plan_refuses(app, &env, &source).await {
        return Err(err(StatusCode::CONFLICT, &why));
    }
    // Its settings as its driver takes them (M220): an environment made
    // before they were checked is refused until it's made again.
    if driver.settings(&env.settings).ok().as_ref() != Some(&env.settings) {
        let why = driver.settings(&env.settings).err().unwrap_or_else(|| "they're missing a default".into());
        return Err(err(StatusCode::CONFLICT, &format!("the environment's settings aren't its driver's: {why}")));
    }
    let (plan, roll_back) = if b.roll_back {
        let (from, kept) = app.db.last_succeeded_of(&env.id, &source)?.ok_or_else(|| {
            err(StatusCode::CONFLICT, "a roll back goes back to a source this environment ran: it never ran that one")
        })?;
        let to = driver.roll_back_to(&kept).ok_or_else(|| {
            err(StatusCode::CONFLICT, "that deploy's log didn't keep what it ran, so it can't be gone back to")
        })?;
        let r = arugula_e2e::drivers::RollBack { deploy: from, to };
        let p = driver
            .plan_roll_back(&env, &source, &r)
            .ok_or_else(|| err(StatusCode::CONFLICT, "the environment's driver doesn't roll back"))?;
        (p.digest, Some(r))
    } else {
        (driver.plan(&env, &source).digest, None)
    };
    // One spelling of a digest, so the same plan reads the same everywhere.
    if b.plan.is_some_and(|given| given.to_ascii_lowercase() != plan) {
        return Err(refusal(
            StatusCode::BAD_REQUEST,
            OTHER_PLAN,
            "that plan isn't the environment's driver's plan for this source: leave it out, and the platform makes it",
        ));
    }
    let new = NewDeploy { environment: &env.id, source: &source, plan_digest: &plan, roll_back: roll_back.as_ref() };
    let d = app
        .db
        .plan_deploy(&new, by, now_ms())
        .map_err(|e| err(StatusCode::BAD_REQUEST, &format!("{e:#}")))?
        .ok_or_else(no_deploy)?;
    let owns = matches!(by, Actor::Person(_)) && role == TeamRole::Owner;
    Ok(Json(deploy_json(app, &d, &env, &p, owns).await?))
}

/// `POST /api/projects/{project}/deploys {environment, source: {kind, ref,
/// where}, plan?}`: plan a deploy, as an owner or an editor.
pub async fn plan_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
    Json(b): Json<Plan>,
) -> J {
    app.limits.check_account(DEPLOY_WRITES, &s.account)?;
    plan(&app, &Actor::person(&s.account), &project, b).await
}

/// Ask for a deploy's approval as `by`: its `requested` event, and a push
/// to each of the project's owners.
pub(crate) async fn request(app: &Platform, by: &Actor, id: &str) -> J {
    let account = by.account().unwrap_or_default();
    let (_, env, p, role) = deploy_as(app, account, id, true).await?;
    let key = asker(&p.id, by);
    app.limits.check_account(PROJECT_ASKS, &key)?;
    let r = app.db.record_deploy(id, EventKind::Requested, by, now_ms(), json!({}));
    if !matches!(r, Ok(Ok(_))) {
        // Not asked after all: it doesn't count.
        app.limits.uncount_account(PROJECT_ASKS, &key);
    }
    let r = r?.map_err(refused)?;
    push_owners(app, by, &r.deploy, &env, &p).await;
    // An agent is never offered approval, whoever its owner is.
    let owns = matches!(by, Actor::Person(_)) && role == TeamRole::Owner;
    Ok(Json(deploy_json(app, &r.deploy, &env, &p, owns).await?))
}

/// `POST /api/deploys/{id}/request`: ask for its approval, as an owner or
/// an editor.
pub async fn request_route(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> J {
    app.limits.check_account(DEPLOY_WRITES, &s.account)?;
    request(&app, &Actor::person(&s.account), &id).await
}

/// The project's owners, now.
pub(crate) async fn owners(app: &Platform, p: &IssueProject) -> anyhow::Result<Vec<String>> {
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

/// Push each owner a card for the deploy waiting on them: not a person who
/// asked themselves (they're looking at it), but an agent's owner, yes.
/// The push names the environment and the source, never the plan's text;
/// a tap opens the deploy. In the background: a failure is only logged.
async fn push_owners(app: &Platform, by: &Actor, d: &Deploy, env: &Environment, p: &IssueProject) {
    let to = match owners(app, p).await {
        Ok(to) => to,
        Err(e) => return warn!(deploy = %d.id, error = format!("{e:#}"), "deploy: the project's owners"),
    };
    let asked = match by {
        Actor::Agent { machine, .. } => {
            let name = app
                .cache
                .daemons(&*app.hub, std::slice::from_ref(machine))
                .await
                .ok()
                .and_then(|mut m| m.remove(machine));
            format!("an agent on {}", name.map_or_else(|| machine.clone(), |d| d.name))
        }
        _ => "someone".to_owned(),
    };
    let short = d.source.reference.chars().take(12).collect::<String>();
    let msg = json!({
        "deploy": d.id,
        "title": format!("Approve a deploy to {}?", env.name),
        "body": format!("{}: {} {short} from {}, asked by {asked}", p.name, d.source.kind.as_str(), d.source.place),
        "tag": format!("deploy-{}", d.id),
        "control": true,
    });
    let to: Vec<String> = to.into_iter().filter(|a| !matches!(by, Actor::Person(me) if me == a)).collect();
    let hub = app.hub.clone();
    let id = d.id.clone();
    tokio::spawn(async move {
        for account in to {
            if let Err(e) = hub.push_with(&account, &msg, PushOptions::default()).await {
                warn!(deploy = %id, %account, error = %e, "the deploy's push didn't go");
            }
        }
    });
}

/// `GET /api/deploys/{id}`: a deploy with its operation, approvals and log,
/// for its project's members.
pub async fn get(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> J {
    let (d, env, p, role) = deploy_as(&app, &s.account, &id, false).await?;
    Ok(Json(deploy_json(&app, &d, &env, &p, role == TeamRole::Owner).await?))
}

/// `GET /api/deploys/waiting`: the deploys waiting for the reader's
/// approval, in projects they own, oldest first.
pub async fn waiting(State(app): State<Arc<Platform>>, s: Person) -> J {
    let teams = app.cache.teams_of(&*app.hub, &s.account).await?;
    let owned: Vec<String> = app
        .cache
        .teams(&*app.hub, &teams)
        .await?
        .into_values()
        .filter(|t| t.role(&s.account) == Some(TeamRole::Owner))
        .map(|t| t.id)
        .collect();
    let envs = app.db.owned_environments(&s.account, &owned)?;
    let mut out = vec![];
    for d in app.db.pending_deploys(&envs)? {
        let Some(env) = app.db.environment(&d.environment)? else { continue };
        let Some(p) = app.db.issue_project(&env.project)? else { continue };
        out.push(deploy_json(&app, &d, &env, &p, true).await?);
    }
    Ok(Json(json!({ "deploys": out })))
}

/// What an owner's page sends to approve or deny: the signed approval's
/// fields that aren't the route's (the deploy and the decision) or the
/// session's (the account), and the passkey's assertion over the
/// approval's bytes.
#[derive(Deserialize)]
pub struct Signed {
    op: String,
    expires: u64,
    by: String,
    sig: String,
    #[serde(default)]
    passkey: Option<PasskeyAssertion>,
}

/// Why an approval was refused: no passkey, or one the hub didn't accept.
pub const PASSKEY: &str = "passkey";

/// The WebAuthn challenge an approval's passkey signs: SHA-256 of the
/// approval's bytes, base64url without padding.
pub fn challenge_of(a: &Approval) -> String {
    a.passkey_challenge()
}

/// The device of `account` that may sign an approval (a deploy's here; a
/// run's for M211 and M212, over their own bytes): one of its devices that
/// counts, by what the hub keeps of them, evaluated here
/// (`arugula_e2e::Trust::evaluate`: chained to the account's root, not
/// revoked), and a browser's (a browser's, a phone's or the desktop app's
/// key, held non-extractable or derived from a passkey). Never a CLI's,
/// whose key is a file an agent on its machine can read, a machine's or a
/// recovery code. Its certificate, whose `sign` key checks the signature.
pub async fn approving_device(app: &Platform, account: &str, device: &str) -> Result<arugula_e2e::Cert, ApiError> {
    let not_a_device = |why: &str| refusal(StatusCode::FORBIDDEN, NOT_A_DEVICE, why);
    let Some(t) = app.hub.device_trust(account).await? else {
        return Err(not_a_device("your account has no devices yet: sign in on your phone or in a browser first"));
    };
    let trusted = Trust { account: account.to_owned(), root: t.root.clone() }.evaluate(&t.certs, &t.revocations);
    let Some(cert) = trusted.get(device) else {
        return Err(not_a_device(
            "that isn't one of your devices (or it was removed). Approve from a browser or a phone you enrolled by \
             signing in with a passkey",
        ));
    };
    if !browsers_all_the_way(account, &t.root, &t.certs, device, &t.legacy_app_children) {
        return Err(not_a_device(
            "this device can't approve deploys: it, or a device that approved it on the way back to your account's \
             first device, is a CLI's or the desktop app's, whose keys a program on its computer can read. Approve \
             from a browser or a phone you enrolled by signing in with a passkey",
        ));
    }
    Ok(cert.clone())
}

/// Whether `account`'s owner role in `team` was given by a browser: the
/// roster version that made it an owner (the latest run of versions where
/// it is one) is signed, over its bytes, by a device of an owner of the
/// version before (of the founder's, for the first), and that device is a
/// browser certified by browsers to that owner's root as the roster pins it
/// ([`browsers_all_the_way`]). An owner a CLI's or the app's key made can't
/// approve: those keys may be a program's.
pub async fn owner_made_by_a_browser(app: &Platform, team: &str, account: &str) -> Result<(), ApiError> {
    use arugula_e2e::team::TeamRole as R;
    let refuse = |why: &str| refusal(StatusCode::FORBIDDEN, NOT_AN_OWNER, why);
    let rosters = app.hub.team_rosters(team).await?;
    let owner = |r: &arugula_e2e::team::Roster| r.member(account).is_some_and(|m| m.role == R::Owner);
    if !rosters.last().is_some_and(owner) {
        return Err(refuse("only the team's owners approve or deny its deploys"));
    }
    let mut i = rosters.len() - 1;
    while i > 0 && owner(&rosters[i - 1]) {
        i -= 1;
    }
    let made = &rosters[i];
    // Who may have signed it: the owners before it, or (the team's first
    // version) its own owners, the founder among them.
    let before = if i == 0 { made } else { &rosters[i - 1] };
    let body = made.body();
    for m in before.members.iter().filter(|m| m.role == R::Owner) {
        let Some(t) = app.hub.device_trust(&m.account).await? else { continue };
        let trusted = Trust { account: m.account.clone(), root: m.root.clone() }.evaluate(&t.certs, &t.revocations);
        let Some(signer) = trusted.get(&made.by) else { continue };
        if !arugula_e2e::cert::verify_hex(&signer.sign, body.as_bytes(), &made.sig) {
            continue;
        }
        return match browsers_all_the_way(&m.account, &m.root, &t.certs, &made.by, &t.legacy_app_children) {
            true => Ok(()),
            false => Err(refuse(
                "your owner role in this team was given by a CLI's or the app's key, which a program could hold, so it \
                 doesn't approve deploys: ask an owner to give it again from a browser or a phone they enrolled by \
                 signing in with a passkey",
            )),
        };
    }
    Err(refuse("the roster that made you an owner isn't signed by a device of an owner before it"))
}

/// Whether `device` is a browser's or a phone's own (`Cert::is_browser`:
/// not a CLI's, not the desktop app's, new kind or old), and every device
/// that certified it back to the account's root is too, the root included
/// (`Trust::chained` with `browser_link`, as the hub checks who may add an
/// owner). Recovery codes may be links on the way (the person's own, on
/// paper, made only by browsers), never the signing device or the root. A
/// CLI's or the app's keys are files a program on their computer can read,
/// so a device they let in, or that comes after one they let in, doesn't
/// count. Every certificate kept for an id on the way is checked; a link
/// with none, or a chain longer than 64, is refused. An app window from
/// before it had a kind counts just above a browser it approved back then
/// (`children`, [`DeviceTrust::legacy_app_children`]), the root included,
/// and nowhere else.
///
/// [`DeviceTrust::legacy_app_children`]: crate::hub::DeviceTrust::legacy_app_children
fn browsers_all_the_way(
    account: &str,
    root: &str,
    certs: &[arugula_e2e::Cert],
    device: &str,
    children: &[String],
) -> bool {
    arugula_e2e::deploy::approver_chained(account, root, certs, device, children)
}

/// Check an approval as `account`'s: good for now and not too long, signed
/// by their [`approving_device`] over exactly these bytes, and over the
/// deploy's operation as the log has it now. The only place a
/// [`VerifiedApproval`] is made; its device's name, for the record.
pub async fn verify(
    app: &Platform,
    a: &Approval,
    passkey: Option<&PasskeyAssertion>,
    now: u64,
) -> Result<(VerifiedApproval, String), ApiError> {
    if a.expires <= now {
        return Err(err(StatusCode::BAD_REQUEST, "the approval has expired: approve it again"));
    }
    if a.expires > now + MAX_LIFE_MS {
        return Err(err(StatusCode::BAD_REQUEST, "an approval is good for 7 days at most"));
    }
    let cert = approving_device(app, &a.account, &a.by).await?;
    if !a.signed_by(&cert.sign) {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            "bad_signature",
            "the signature isn't that device's over this deploy's operation",
        ));
    }
    let (_, op) = app.db.deploy_operation(&a.deploy)?.ok_or_else(no_deploy)?;
    if op.hash() != a.op {
        return Err(refusal(
            StatusCode::CONFLICT,
            OTHER_OPERATION,
            "the approval signs another operation than this deploy's: look at it again",
        ));
    }
    // The person, not only their browser: a passkey's assertion over this
    // approval's bytes, with user verification, checked by the hub, which
    // keeps passkeys' public keys. A browser's stored key signs without
    // one, so whatever drives the browser (automation, its profile on
    // disk) can't approve alone.
    let no_passkey = |why: &str| refusal(StatusCode::FORBIDDEN, PASSKEY, why);
    let Some(pk) = passkey else {
        return Err(no_passkey("a deploy is approved or denied with a passkey: confirm it with one"));
    };
    // TODO(passkeys): the hub publishes passkeys' public keys, so the proof
    // kept below can be checked again without asking the hub.
    let verdict = app.hub.verify_passkey(&a.account, &challenge_of(a), pk).await?;
    if !verdict.ok {
        return Err(no_passkey(&format!("the passkey didn't confirm this approval: {}", verdict.why)));
    }
    let proof = PasskeyProof {
        id: pk.id.clone(),
        client_data_json: pk.client_data_json.clone(),
        authenticator_data: pk.authenticator_data.clone(),
        signature: pk.signature.clone(),
    };
    Ok((VerifiedApproval::from_checked(a, &op, proof), cert.name))
}

async fn decide(app: &Platform, account: &str, id: &str, decision: Decision, b: Signed) -> J {
    app.limits.check_account(APPROVALS, account)?;
    // The roster now, not a kept one: an owner who was just made an editor
    // doesn't approve.
    let (_, env, p, _) = deploy_as(app, account, id, false).await?;
    if role(app, account, &p, true).await? != Some(TeamRole::Owner) {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            NOT_AN_OWNER,
            "only the project's owners approve or deny its deploys",
        ));
    }
    let a = Approval {
        deploy: id.to_owned(),
        op: b.op,
        account: account.to_owned(),
        decision,
        expires: b.expires,
        by: b.by,
        sig: b.sig,
    };
    let (v, device_name) = verify(app, &a, b.passkey.as_ref(), now_ms()).await?;
    // A personal project's owner is its account. A team's owner must have
    // been made one by a browser: the hub refuses rosters that add owners
    // from a CLI or the app, and this checks it again.
    if let (None, Some(team)) = (&p.account, &p.team) {
        owner_made_by_a_browser(app, team, account).await?;
    }
    let (r, _) = app.db.record_deploy_approval(&v, &device_name, now_ms())?.map_err(refused)?;
    Ok(Json(deploy_json(app, &r.deploy, &env, &p, true).await?))
}

/// `POST /api/deploys/{id}/approve {op, expires, by, sig}`: approve it
/// once, as an owner, with their device's signature over the operation. A
/// browser's session only: never a machine's signature or a CLI's login.
pub async fn approve(
    State(app): State<Arc<Platform>>,
    s: BrowserPerson,
    Path(id): Path<String>,
    Json(b): Json<Signed>,
) -> J {
    decide(&app, &s.account, &id, Decision::Once, b).await
}

/// `POST /api/deploys/{id}/deny {op, expires, by, sig}`: deny it, signed as
/// an approval is.
pub async fn deny(
    State(app): State<Arc<Platform>>,
    s: BrowserPerson,
    Path(id): Path<String>,
    Json(b): Json<Signed>,
) -> J {
    decide(&app, &s.account, &id, Decision::Deny, b).await
}
