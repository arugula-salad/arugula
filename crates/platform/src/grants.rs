//! Grants (M130, "Grants, not impersonation"): what a machine may do with
//! its owner's platform objects. For now that's one power, `cloud.drive`:
//! the agents on a machine drive its owner's Cloud workspaces (aru-047).
//! Every machine that joins an account gets it; the owner lists, revokes
//! and gives it back here (`arugula grants` in the CLI), and
//! [`CloudDriver`](crate::cloud::CloudDriver) refuses a machine without a
//! live one ([`can_drive_cloud`]).
//!
//! The platform's since step 2 (#1150): the grants are in its own tables
//! ([`db::projects`](crate::db::projects)), and the hub's directory (whose
//! a machine is, its name) comes through `HubPort::daemons`. A machine gets
//! its grant when the hub's change feed says it joined
//! ([`crate::projects::follow`]), or the first time anyone asks about it,
//! whichever is first: so a machine that just joined is never refused for a
//! grant the feed hasn't brought yet.
//!
//! Only a person (a signed-in browser, or a CLI device) manages grants: a
//! machine's signature isn't a [`Person`], so it can't give itself one.
//!
//! **Agents in conversations (M197, platform step 6).** Three more powers,
//! `threads.read`, `threads.post` and `issues.write`, over a project (a
//! team's channels are its default project's; an object's thread and a
//! native issue are their project's) or the owner's own account (their
//! DMs). Unlike `cloud.drive` they're never given on join: the machine's
//! owner gives each one, over a scope they read themselves ([`give`]). The
//! routes an agent calls ask [`agent_may`] every time, which reads the grant
//! and checks again that the owner still reads its scope, so an agent never
//! reaches more than its owner: a revoked grant refuses the next call, and
//! one whose owner left the team stops within [`cache::DELAY`].
//!
//! **Agents and runs (M216, platform step 7).** Three more, over a project
//! only, given and asked the same way: `runs.read` (its agents read the
//! project's runs), `runs.start` (they start its run specs) and
//! `runs.command` (they start a command or an instruction of their own),
//! each start as its owner may (an owner or editor on the hosted beta's
//! list). None is ever an approval.
//!
//! [`cache::DELAY`]: crate::cache::DELAY

use std::sync::Arc;

use arugula_control_common::{ApiError, err, refusal};
use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    Platform,
    auth::{Machine, Person},
    chat,
    db::projects::{
        AGENT_POWERS, CLOUD_DRIVE, DEPLOYS_PLAN, Grant, ISSUES_WRITE, RUN_POWERS, SECRETS_WRITE, THREADS_POST,
        THREADS_READ, subject_kind,
    },
    hub::{Daemon, HubResult},
};

type R = Result<Json<Value>, ApiError>;

/// Why an agent was refused ([`agent_may`]): it has no live grant, or its
/// owner doesn't read the scope (any more).
pub const AGENT_UNGRANTED: &str = "agent_ungranted";

/// Where an agent's grant reaches: a project, or its owner's account (their
/// DMs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentScope {
    Project(String),
    Account(String),
}

impl AgentScope {
    fn row(&self) -> (&'static str, &str) {
        match self {
            AgentScope::Project(id) => ("project", id),
            AgentScope::Account(id) => ("account", id),
        }
    }
}

/// An agent let act ([`agent_may`]): its machine, the machine's name now,
/// and its owner, whose account is the author of what it writes (with the
/// machine beside it, labelled "<owner>'s agent on <machine>").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPrincipal {
    pub machine: String,
    pub machine_name: String,
    pub owner: String,
}

/// What a scope is called, if `owner` reads it now: their own account
/// ("your DMs"), their personal project, or a project of a team they're in
/// (a locked team's owners only). `now`: ask the hub, not the cache (to
/// write a grant on the hub's word).
pub(crate) async fn owner_reads(
    app: &Platform,
    owner: &str,
    scope: &AgentScope,
    now: bool,
) -> HubResult<Option<String>> {
    match scope {
        AgentScope::Account(a) => Ok((a == owner).then(|| "your DMs".to_owned())),
        AgentScope::Project(p) => {
            let Some(o) = app.db.project_owner(p)? else { return Ok(None) };
            if let Some(a) = &o.account {
                return Ok((a == owner).then_some(o.name));
            }
            let Some(team) = &o.team else { return Ok(None) };
            let t =
                if now { app.cache.team_now(&*app.hub, team).await? } else { app.cache.team(&*app.hub, team).await? };
            Ok(t.filter(|t| t.role(owner).is_some()).map(|t| t.name))
        }
    }
}

/// Whether `power` is an agent's (M197's in conversations, M216's over
/// runs).
pub fn is_agent_power(power: &str) -> bool {
    AGENT_POWERS.contains(&power) || RUN_POWERS.contains(&power)
}

/// Whether an agent's `power` is over a project only, never the owner's
/// DMs.
fn project_only(power: &str) -> bool {
    power == ISSUES_WRITE || power == DEPLOYS_PLAN || power == SECRETS_WRITE || RUN_POWERS.contains(&power)
}

/// Whether `power` is one an agent grant token may hold (M230): the agent
/// powers, and `secrets.write`. Never `cloud.drive`, and never an approval:
/// no grant is.
pub fn is_token_power(power: &str) -> bool {
    is_agent_power(power) || power == SECRETS_WRITE
}

/// What an agent with no machine is called beside its owner's name, in
/// place of a machine's: "<owner>'s agent on <this>".
pub fn token_label(name: &str) -> String {
    format!("{name} (MCP)")
}

/// The machines these ids name, as [`Cache::daemons`] has them, and the
/// agent grant tokens among them (M230, `agt_…`) as if they were machines
/// of their owners named [`token_label`]: for the label on what an agent
/// wrote, wherever an agent's machine is named. Never for whether a
/// machine is someone's: only the hub says that.
///
/// [`Cache::daemons`]: crate::cache::Cache::daemons
pub async fn agent_names(app: &Platform, ids: &[String]) -> HubResult<std::collections::HashMap<String, Daemon>> {
    let (tokens, machines): (Vec<String>, Vec<String>) =
        ids.iter().cloned().partition(|id| subject_kind(id) == "agent");
    let mut out =
        if machines.is_empty() { Default::default() } else { app.cache.daemons(&*app.hub, &machines).await? };
    for id in tokens {
        if let Some(t) = app.db.agent_token(&id)? {
            out.insert(id.clone(), Daemon { id, account: t.account, team: None, name: token_label(&t.name) });
        }
    }
    Ok(out)
}

/// How the owner gives this grant, for a refusal.
fn how_to_grant(machine_name: &str, power: &str, scope: &AgentScope, scope_name: &str) -> String {
    let flag = match scope {
        AgentScope::Project(p) => format!("--project {p}"),
        AgentScope::Account(_) => "--dms".to_owned(),
    };
    let switch = if RUN_POWERS.contains(&power) { "Agents and runs" } else { "Agents in conversations" };
    format!(
        "its owner gives it with `arugula grants add {machine_name} {power} {flag}`, or turns on {machine_name} for \
         {scope_name} under Settings, {switch}, on the platform page"
    )
}

/// Whether the agents on `machine` may use `power` over `scope`: a live
/// grant its owner gave, over a scope the owner reads now. Asked on every
/// call an agent makes: the grant is read from the database each time (a
/// revoked one refuses the very next call), and the owner's reach comes
/// from the cache, which hears of a roster change within
/// [`cache::DELAY`](crate::cache::DELAY). A refusal (403,
/// [`AGENT_UNGRANTED`]) names the machine and says how to grant it.
pub async fn agent_may(
    app: &Platform,
    machine: &Machine,
    power: &str,
    scope: &AgentScope,
) -> Result<AgentPrincipal, ApiError> {
    let token = subject_kind(&machine.daemon) == "agent";
    let known = if token { is_token_power(power) } else { is_agent_power(power) };
    if !known || (project_only(power) && matches!(scope, AgentScope::Account(_))) {
        return Err(err(StatusCode::BAD_REQUEST, &format!("{power} isn't a power over that")));
    }
    let owner = &machine.account;
    let refused = |why: String| refusal(StatusCode::FORBIDDEN, AGENT_UNGRANTED, &why);
    let machine_name = if token {
        // An agent grant token (M230): its owner's, and live, asked on
        // every call as a grant is.
        match app.db.agent_token(&machine.daemon)? {
            Some(t) if t.account == *owner && t.revoked_at.is_none() => token_label(&t.name),
            _ => return Err(refused("this agent grant token was revoked".to_owned())),
        }
    } else {
        app.cache
            .daemons(&*app.hub, std::slice::from_ref(&machine.daemon))
            .await?
            .remove(&machine.daemon)
            .map_or_else(|| machine.daemon.clone(), |d| d.name)
    };
    let Some(scope_name) = owner_reads(app, owner, scope, false).await? else {
        return Err(refused(format!(
            "{machine_name}'s agents can't reach that: its owner doesn't read it, and an agent never reaches more \
             than its owner"
        )));
    };
    if app.db.live_grant(&machine.daemon, power, scope.row(), owner)?.is_none() {
        let how = if token {
            format!(
                "a token's powers are given when it's minted, so its owner mints one with {power} over {scope_name} \
                 (`arugula agent-tokens add`, or Settings, Agents with no machine, on the platform page)"
            )
        } else {
            how_to_grant(&machine_name, power, scope, &scope_name)
        };
        return Err(refused(format!("{machine_name} has no {power} grant over {scope_name}: {how}")));
    }
    Ok(AgentPrincipal { machine: machine.daemon.clone(), machine_name, owner: owner.clone() })
}

fn wire(g: &Grant) -> Value {
    json!({
        "id": g.id,
        "subject": { "kind": g.subject_kind, "id": g.subject_id, "name": g.subject_name },
        "power": g.power,
        "scope": { "kind": g.scope_kind, "id": g.scope_id, "name": g.scope_name },
        "granted_by": g.granted_by,
        "created_at": g.created_at,
        "revoked_at": g.revoked_at,
        "live": g.revoked_at.is_none(),
    })
}

/// Whether this machine may drive `account`'s Cloud workspaces: a live
/// `cloud.drive` grant over it. One that never had a grant there gets it
/// now if the hub says it's the account's machine, which is how a machine
/// that just joined drives at once, before the feed says it joined. One
/// whose grant was revoked is asked about again once the feed is applied to
/// its end: if it left and joined again since, its old grant went with it.
pub async fn can_drive_cloud(app: &Platform, daemon: &str, account: &str) -> HubResult<bool> {
    let mut caught_up = false;
    loop {
        match app.db.cloud_drive(daemon, account)? {
            Some(true) => return Ok(true),
            None => {
                // A grant is written on the hub's word, never on a kept
                // answer. Once per machine: after this it has a grant row.
                let joined = app.cache.daemons_now(&*app.hub, &[daemon.to_owned()]).await?;
                if !joined.get(daemon).is_some_and(|d| d.account == account) {
                    return Ok(false);
                }
                app.db.grant_on_join(daemon, account, now_ms())?;
            }
            Some(false) if caught_up => return Ok(false),
            Some(false) => {
                crate::projects::follow(app).await?;
                caught_up = true;
            }
        }
    }
}

/// Machine names for these grants, from the hub's directory (as the cache
/// keeps it), and project names for those over a project.
async fn named(app: &Platform, mut grants: Vec<Grant>) -> HubResult<Vec<Grant>> {
    let mut ids: Vec<String> = grants.iter().map(|g| g.subject_id.clone()).collect();
    ids.sort();
    ids.dedup();
    let daemons = app.cache.daemons(&*app.hub, &ids).await?;
    let mut projects = std::collections::HashMap::new();
    for g in &mut grants {
        g.subject_name = daemons.get(&g.subject_id).map(|d| d.name.clone());
        if g.scope_kind == "project" {
            if !projects.contains_key(&g.scope_id) {
                let name = app.db.project_owner(&g.scope_id)?.map(|o| o.name);
                projects.insert(g.scope_id.clone(), name);
            }
            g.scope_name = projects[&g.scope_id].clone();
        }
    }
    Ok(grants)
}

/// `GET /api/grants`: the grants over the signed-in account, live and
/// revoked, by machine. Each of the account's machines that has never had
/// one gets it first, as it would from the feed. Which machines are the
/// account's comes from the cache; only one with no grant row at all (a
/// machine that just joined, or one that left and lost its grant) is asked
/// about again before it's given one, so the steady state asks the hub
/// nothing.
pub async fn list(State(app): State<Arc<Platform>>, s: Person) -> R {
    let machines = app.cache.machines_for(&*app.hub, &s.account).await?;
    // One read of the account's grants answers both what to list and which
    // machines never had one (`Db::cloud_drive`'s `None`), so the steady
    // state is one statement, not one per machine (a round trip each on
    // Postgres).
    let mut rows = app.db.grants(&s.account)?;
    let had: std::collections::HashSet<&str> = rows
        .iter()
        .filter(|g| g.subject_kind == "daemon" && g.power == CLOUD_DRIVE)
        .map(|g| g.subject_id.as_str())
        .collect();
    let mut ungranted = vec![];
    for d in app.cache.daemons(&*app.hub, &machines).await?.into_values() {
        if d.account == s.account && !had.contains(d.id.as_str()) {
            ungranted.push(d.id);
        }
    }
    if !ungranted.is_empty() {
        let mut gave = false;
        for d in app.cache.daemons_now(&*app.hub, &ungranted).await?.into_values() {
            if d.account == s.account {
                gave |= app.db.grant_on_join(&d.id, &d.account, now_ms())?;
            }
        }
        if gave {
            rows = app.db.grants(&s.account)?;
        }
    }
    // Agent grant tokens' grants are listed with their tokens
    // (`/api/agent-tokens`), not by machine.
    rows.retain(|g| g.subject_kind == "daemon");
    let mut grants = named(&app, rows).await?;
    grants.sort_by(|a, b| {
        let name = |g: &Grant| g.subject_name.clone().unwrap_or_else(|| g.subject_id.clone());
        (name(a), &a.subject_id, std::cmp::Reverse(a.created_at), &a.id).cmp(&(
            name(b),
            &b.subject_id,
            std::cmp::Reverse(b.created_at),
            &b.id,
        ))
    });
    Ok(Json(json!({ "grants": grants.iter().map(wire).collect::<Vec<_>>() })))
}

/// `POST /api/grants/{id}/revoke`: take a grant back. The machine is
/// refused from its next call; revoking again changes nothing.
pub async fn revoke(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> R {
    let g =
        app.db.revoke_grant(&s.account, &id, now_ms())?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such grant"))?;
    let g = named(&app, vec![g]).await?.remove(0);
    Ok(Json(wire(&g)))
}

#[derive(Deserialize)]
pub struct Give {
    /// The machine (its id).
    daemon: String,
    power: String,
    /// An agent power's scope: a project, or `account` (the giver's DMs).
    #[serde(default)]
    scope: Option<GiveScope>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum GiveScope {
    Project { id: String },
    Account,
}

/// `POST /api/grants {daemon, power, scope?}`: give one of the account's
/// machines a power, as a new grant. One it holds already is the answer.
/// `cloud.drive` is over the account (no scope); `threads.read`,
/// `threads.post` and `issues.write` take a scope, `{kind: "project", id}`
/// for a project the giver reads, or `{kind: "account"}` for their DMs
/// (threads only); `runs.read` and `runs.start` (M216) a project.
pub async fn give(State(app): State<Arc<Platform>>, s: Person, Json(b): Json<Give>) -> R {
    let scope = match (b.power.as_str(), b.scope) {
        (CLOUD_DRIVE, None) => None,
        (CLOUD_DRIVE, Some(_)) => {
            return Err(err(StatusCode::BAD_REQUEST, "cloud.drive is over your account: no scope"));
        }
        (p, Some(scope)) if is_agent_power(p) => Some(match scope {
            GiveScope::Project { id } => AgentScope::Project(id),
            GiveScope::Account if project_only(p) => {
                return Err(err(StatusCode::BAD_REQUEST, &format!("{p} is over a project, not your DMs")));
            }
            GiveScope::Account => AgentScope::Account(s.account.clone()),
        }),
        (p, None) if is_agent_power(p) => {
            return Err(err(StatusCode::BAD_REQUEST, "an agent's power is over a project or your DMs: say which"));
        }
        _ => {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "the powers there are to give are cloud.drive, threads.read, threads.post, issues.write, deploys.plan, \
                 runs.read, runs.start and runs.command",
            ));
        }
    };
    // A grant is written on the hub's word, never on a kept answer.
    let mine = app
        .cache
        .daemons_now(&*app.hub, std::slice::from_ref(&b.daemon))
        .await?
        .get(&b.daemon)
        .is_some_and(|d| d.account == s.account);
    if !mine {
        return Err(err(StatusCode::NOT_FOUND, "not a machine on this account"));
    }
    let g = match scope {
        None => app.db.give_cloud_drive(&s.account, &b.daemon, &s.account, now_ms())?,
        Some(scope) => {
            // Only over what the giver reads, on the hub's word.
            if owner_reads(&app, &s.account, &scope, true).await?.is_none() {
                return Err(err(StatusCode::FORBIDDEN, "you can only give your machine what you can read yourself"));
            }
            // A grant that lets agents into a room says so there (#122).
            let rooms = if b.power == THREADS_READ || b.power == THREADS_POST {
                chat::agent_notice::rooms(&app, scope.row()).unwrap_or_default()
            } else {
                vec![]
            };
            let before = chat::agent_notice::reach(&app, &rooms).await.ok();
            let g = app.db.give_grant(&b.daemon, &b.power, scope.row(), &s.account, now_ms())?;
            if let Some(before) = before {
                chat::agent_notice::announce(&app, &rooms, &before).await;
            }
            g
        }
    };
    let g = named(&app, vec![g]).await?.remove(0);
    Ok(Json(wire(&g)))
}
