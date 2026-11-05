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
//! A grant is revoked by its giver, or by the account it's over
//! (`cloud.drive`), or (Arugula #683) by an owner of the project it's
//! over, whoever gave it, a machine's or a token's: the owner's reach is
//! the project's grants and nothing else, never one over an account (a
//! teammate's `cloud.drive`, or their DMs) or over another project
//! ([`revoke`]). The grant keeps who revoked it and when, and the giver is
//! told. A revoke isn't a ban: the grant is the machine owner's (aru-073),
//! so they can give it again; an owner who means it takes them out of the
//! team. It stops the next call [`agent_may`] sees; a run, an agent
//! session or a deploy approval already under way carries on as it was.
//! And a write power that does nothing for a viewer's agents
//! ([`WRITE_POWERS`]) is refused to a viewer's machine at [`give`] (and to
//! a viewer's token at its mint), saying why; one that still does something
//! there is given, and [`list`] says what it does. One from before stays
//! until a role change brings the rest of it back.
//!
//! [`WRITE_POWERS`]: crate::db::projects::WRITE_POWERS
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
//! **Agents and sessions** (Fleet 3, aru-121). Two more, over a project
//! only, given and asked the same way: `sessions.start` (its agents start
//! an agent session as their owner, the session's asker and payer) and
//! `sessions.reply` (they answer a session's question, or prompt it, in its
//! thread). Never to an agent grant token, and never an approval: a
//! session's permission waits on a person.
//!
//! [`cache::DELAY`]: crate::cache::DELAY

use std::sync::Arc;

use arugula_control_common::{ApiError, err, refusal};
use arugula_e2e::{now_ms, team::TeamRole};
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
    db::projects::{
        AGENT_POWERS, BOARD_POWERS, CLOUD_DRIVE, DEPLOYS_PLAN, Grant, ISSUES_WRITE, Limited, REVOKED_BY_ERASED,
        REVOKED_BY_PLATFORM, RUN_POWERS, SECRETS_WRITE, SESSION_POWERS, TRACKERS_WRITE, ViewerUse, refused_for_viewer,
        subject_kind, viewer_use,
    },
    hub::{Daemon, HubResult},
};

type R = Result<Json<Value>, ApiError>;

/// Why an agent was refused ([`agent_may`]): it has no live grant, or its
/// owner doesn't read the scope (any more).
pub const AGENT_UNGRANTED: &str = "agent_ungranted";

/// Why a grant was refused at [`give`] (or a token's mint): a write power
/// that does nothing for a viewer's agents (Arugula #683).
pub const VIEWER_WRITE: &str = "viewer_write";

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
pub async fn owner_reads(app: &Platform, owner: &str, scope: &AgentScope, now: bool) -> HubResult<Option<String>> {
    Ok(owner_role(app, owner, scope, now).await?.map(|(name, _)| name))
}

/// [`owner_reads`], with the owner's role there: an owner of their own
/// account and personal project, else their place in the team's roster.
pub async fn owner_role(
    app: &Platform,
    owner: &str,
    scope: &AgentScope,
    now: bool,
) -> HubResult<Option<(String, TeamRole)>> {
    match scope {
        AgentScope::Account(a) => Ok((a == owner).then(|| ("your DMs".to_owned(), TeamRole::Owner))),
        AgentScope::Project(p) => {
            let Some(o) = app.db.project_owner(p)? else { return Ok(None) };
            if let Some(a) = &o.account {
                return Ok((a == owner).then_some((o.name, TeamRole::Owner)));
            }
            let Some(team) = &o.team else { return Ok(None) };
            let t =
                if now { app.cache.team_now(&*app.hub, team).await? } else { app.cache.team(&*app.hub, team).await? };
            Ok(t.and_then(|t| t.role(owner).map(|role| (t.name, role))))
        }
    }
}

/// What `power` over `scope` (a project, named `scope_name`) does for an
/// agent whose owner is `role` there, when that's less than the power
/// (Arugula #683): a write power on a viewer's. `None` when it works as
/// for anyone.
pub fn limited_for(power: &str, role: TeamRole, scope: &AgentScope, scope_name: &str) -> Option<Limited> {
    if role != TeamRole::Viewer || !matches!(scope, AgentScope::Project(_)) {
        return None;
    }
    match viewer_use(power)? {
        ViewerUse::Nothing => Some(Limited {
            label: "idle".into(),
            why: format!(
                "{power} does nothing here: its giver only views {scope_name}, so every write its agents try there \
                 is refused. An owner making them an editor brings it to life"
            ),
        }),
        ViewerUse::Only(label, does) => Some(Limited {
            label: label.into(),
            why: format!(
                "{power} {does}: its giver only views {scope_name}. An owner making them an editor brings the rest"
            ),
        }),
    }
}

/// Whether giving `power` to a viewer's agents over `scope` is refused:
/// nothing of it would work ([`refused_for_viewer`]).
pub fn viewer_refused(power: &str, role: TeamRole, scope: &AgentScope) -> bool {
    role == TeamRole::Viewer && matches!(scope, AgentScope::Project(_)) && refused_for_viewer(power)
}

/// The refusal (403, [`VIEWER_WRITE`]) for giving `power` over `scope_name`
/// to `whose` ("geek's agents", "an agent token") when the giver only
/// views it: nothing of it would work, so it isn't given.
pub fn viewer_write(power: &str, scope_name: &str, whose: &str) -> ApiError {
    refusal(
        StatusCode::FORBIDDEN,
        VIEWER_WRITE,
        &format!(
            "you only view {scope_name}, so {power} would do nothing for {whose}: every write there would be refused. \
             Ask an owner of {scope_name} for the editor role first"
        ),
    )
}

/// Whether `power` is an agent's (M197's in conversations, M216's over
/// runs, aru-121's over sessions).
pub fn is_agent_power(power: &str) -> bool {
    AGENT_POWERS.contains(&power)
        || RUN_POWERS.contains(&power)
        || BOARD_POWERS.contains(&power)
        || SESSION_POWERS.contains(&power)
        || power == TRACKERS_WRITE
}

/// Whether an agent's `power` is over a project only, never the owner's
/// DMs.
fn project_only(power: &str) -> bool {
    power == ISSUES_WRITE
        || power == DEPLOYS_PLAN
        || power == SECRETS_WRITE
        || RUN_POWERS.contains(&power)
        || BOARD_POWERS.contains(&power)
        || SESSION_POWERS.contains(&power)
        || power == TRACKERS_WRITE
}

/// Whether `power` is one an agent grant token may hold (M230): the agent
/// powers but a session's (aru-121: a machine's agents', whose card names
/// the machine), and `secrets.write`. Never `cloud.drive`, and never an
/// approval: no grant is.
pub fn is_token_power(power: &str) -> bool {
    (is_agent_power(power) && !SESSION_POWERS.contains(&power)) || power == SECRETS_WRITE
}

/// What an agent with no machine is called beside its owner's name, in
/// place of a machine's: "<owner>'s agent on <this>".
pub fn token_label(name: &str) -> String {
    format!("{name} (MCP)")
}

/// Who an agent is beside its owner's name, as the platform page says it
/// (web/src/shared/agent-label.ts): "<owner>'s agent on <machine>", or an
/// agent grant token's (M230), whose name isn't a machine's: "<owner>'s
/// agent token “<name>”" (UI audit #192). `name`: the machine's name, or
/// the token's (bare or as [`token_label`] names it). An agent session
/// (`ses_…`, aru-115) acts as itself, so it is its name alone.
pub fn agent_label(owner: &str, id: &str, name: &str) -> String {
    if id.starts_with("ses_") {
        name.to_owned()
    } else if subject_kind(id) == "agent" {
        let label = token_label("");
        let name = name.strip_suffix(label.as_str()).unwrap_or(name);
        format!("{owner}'s agent token \u{201c}{name}\u{201d}")
    } else {
        format!("{owner}'s agent on {name}")
    }
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
            out.insert(
                id.clone(),
                Daemon { id, account: t.account, team: None, name: token_label(&t.name), aliases: vec![] },
            );
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
    if BOARD_POWERS.contains(&power) {
        return format!(
            "its owner gives it with `arugula grants add {machine_name} {power} {flag}`, or sets {machine_name}'s \
             Boards for {scope_name} under Settings → Agents on the platform page"
        );
    }
    if power == TRACKERS_WRITE {
        return format!(
            "its owner gives it with `arugula grants add {machine_name} {power} {flag}`, or sets {machine_name}'s \
             Plane writes for {scope_name} under Settings → Agents on the platform page"
        );
    }
    let switch = if RUN_POWERS.contains(&power) {
        "Agents and runs"
    } else if SESSION_POWERS.contains(&power) {
        "Agents on your machines (Start sessions, Answer sessions)"
    } else {
        "Agents in conversations"
    };
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
        "revoked_by": revoked_by_wire(g),
        "live": g.revoked_at.is_none(),
        // What a live one does on a viewer's (Arugula #683): a label and
        // why, else null.
        "limited": g.limited,
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
/// keeps it), project names for those over a project, the name of whoever
/// revoked each revoked one, and what each live one does on a viewer's
/// (Arugula #683: a write power whose giver only views the project, read
/// from the cache as [`agent_may`] reads the owner's reach).
async fn named(app: &Platform, mut grants: Vec<Grant>) -> HubResult<Vec<Grant>> {
    let mut ids: Vec<String> = grants.iter().map(|g| g.subject_id.clone()).collect();
    ids.sort();
    ids.dedup();
    let daemons = app.cache.daemons(&*app.hub, &ids).await?;
    name_revokers(app, &mut grants).await?;
    let mut projects = std::collections::HashMap::new();
    let mut roles: std::collections::HashMap<(String, String), Option<TeamRole>> = Default::default();
    for g in &mut grants {
        g.subject_name = daemons.get(&g.subject_id).map(|d| d.name.clone());
        if g.scope_kind == "project" {
            if !projects.contains_key(&g.scope_id) {
                let name = app.db.project_owner(&g.scope_id)?.map(|o| o.name);
                projects.insert(g.scope_id.clone(), name);
            }
            g.scope_name = projects[&g.scope_id].clone();
            if g.revoked_at.is_none() && viewer_use(&g.power).is_some() {
                let scope = AgentScope::Project(g.scope_id.clone());
                let key = (g.granted_by.clone(), g.scope_id.clone());
                let role = match roles.get(&key) {
                    Some(r) => *r,
                    None => {
                        let r = owner_role(app, &g.granted_by, &scope, false).await?.map(|(_, r)| r);
                        roles.insert(key, r);
                        r
                    }
                };
                let name = g.scope_name.clone().unwrap_or_else(|| g.scope_id.clone());
                g.limited = role.and_then(|r| limited_for(&g.power, r, &scope, &name));
            }
        }
    }
    Ok(grants)
}

/// The name of whoever revoked each revoked grant (`revoked_by_name`),
/// from the hub's directory as the cache keeps it: for [`named`], and for
/// a token's grants as `/api/agent-tokens` lists them (Arugula #683).
pub async fn name_revokers(app: &Platform, grants: &mut [Grant]) -> HubResult<()> {
    let mut revokers: Vec<String> = grants
        .iter()
        .filter(|g| g.revoked_at.is_some())
        .map(|g| g.revoked_by.clone().unwrap_or_else(|| g.granted_by.clone()))
        .filter(|by| by != REVOKED_BY_ERASED && by != REVOKED_BY_PLATFORM)
        .collect();
    revokers.sort();
    revokers.dedup();
    if revokers.is_empty() {
        return Ok(());
    }
    let accounts = app.cache.accounts(&*app.hub, &revokers).await?;
    for g in grants.iter_mut().filter(|g| g.revoked_at.is_some()) {
        let by = g.revoked_by.as_ref().unwrap_or(&g.granted_by);
        g.revoked_by_name = accounts.get(by).map(|a| a.name.clone());
    }
    Ok(())
}

/// Who revoked a grant, as the API says it (Arugula #683): the account,
/// and its name as the directory has it; the platform itself, or an
/// account since deleted (no account, a name saying so). One revoked
/// before that was recorded was its giver's doing. `null` on a live one.
pub fn revoked_by_wire(g: &Grant) -> Value {
    g.revoked_at.map_or(Value::Null, |_| match g.revoked_by.as_deref() {
        Some(REVOKED_BY_ERASED) => json!({ "account": null, "name": "an account since deleted" }),
        Some(REVOKED_BY_PLATFORM) => {
            json!({ "account": REVOKED_BY_PLATFORM, "name": "Arugula control, when the giver left the team" })
        }
        by => json!({ "account": by.unwrap_or(&g.granted_by), "name": g.revoked_by_name }),
    })
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
/// refused from its next call; revoking again changes nothing. The
/// caller is the grant's giver or the account it's over, as before, or
/// (Arugula #683) an owner of the project it's over, whoever gave it:
/// [`revoke_as_owner`], and then the giver is told ([`notify_revoked`])
/// before anything else can fail. Only a [`Person`]: never a machine's
/// signature.
pub async fn revoke(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> R {
    let (g, as_owner_now) = match app.db.revoke_grant(&s.account, &id, now_ms())? {
        Some(g) => (g, false),
        None => revoke_as_owner(&app, &s.account, &id).await?,
    };
    if as_owner_now {
        notify_revoked(
            app.clone(),
            Revoked {
                giver: g.granted_by.clone(),
                subject_kind: g.subject_kind.clone(),
                subject_id: g.subject_id.clone(),
                project: g.scope_id.clone(),
                powers: vec![g.power.clone()],
                by: s.account.clone(),
            },
        );
    }
    let g = named(&app, vec![g]).await?.remove(0);
    let mut answer = wire(&g);
    // The project's Access view (Arugula #682) names nobody who has left
    // the project: not a giver, their machine or token, nor whoever
    // revoked the grant. An owner's revoke of a grant over the project
    // answers the same way, now and when it's revoked again. Someone
    // locked out of a locked team is still on its roster, so still named;
    // and the caller is never unnamed to themselves.
    if g.scope_kind == "project" {
        let gone = |a: &str| {
            let a = a.to_owned();
            let (app, project, caller) = (app.clone(), g.scope_id.clone(), s.account.clone());
            async move { Ok::<_, ApiError>(a != caller && !on_roster(&app, &a, &project).await?) }
        };
        if gone(&g.granted_by).await? {
            unname_giver(&mut answer);
        }
        if let Some(by) = answer["revoked_by"]["account"].as_str().map(str::to_owned)
            && by != REVOKED_BY_PLATFORM
            && gone(&by).await?
        {
            answer["revoked_by"] = json!({ "account": null, "name": "a former member" });
        }
    }
    Ok(Json(answer))
}

/// Whether `account` is on the roster of the project's team now (locked
/// out or not), or is the account a personal project belongs to: what
/// tells a member, even one a lock keeps out, from someone who has left.
pub async fn on_roster(app: &Platform, account: &str, project: &str) -> HubResult<bool> {
    let Some(o) = app.db.project_owner(project)? else { return Ok(false) };
    if let Some(a) = &o.account {
        return Ok(a == account);
    }
    let Some(team) = &o.team else { return Ok(false) };
    Ok(app.cache.team(&*app.hub, team).await?.is_some_and(|t| t.members.iter().any(|m| m.account == account)))
}

/// Take the giver, and their machine's or token's id and name, out of a
/// grant's answer: a former member of the project it's over is "a former
/// member", no more.
pub fn unname_giver(g: &mut Value) {
    g["granted_by"] = Value::Null;
    g["subject"]["id"] = Value::Null;
    g["subject"]["name"] = Value::Null;
    g["former_giver"] = json!(true);
}

/// What a project's owner revoked (Arugula #683), for its giver to be
/// told: ids as the revoke knew them, never names, so the notice doesn't
/// depend on the hub answering.
pub struct Revoked {
    /// The grant's giver: a machine's owner, or a token's minter.
    pub giver: String,
    /// `daemon` or `agent`.
    pub subject_kind: String,
    pub subject_id: String,
    /// The project the grant was over.
    pub project: String,
    /// The powers revoked there: one for a machine's grant, a token's
    /// whole reach there for `agent_tokens::revoke_over`.
    pub powers: Vec<String>,
    /// The owner who revoked them.
    pub by: String,
}

/// Push the giver's devices a notice that a project's owner revoked their
/// grant (Arugula #683): what, who, and how to give it again. Spawned as
/// soon as the revoke is committed, and nothing in it fails the notice:
/// each name is looked up here, and an id stands in for one the hub
/// doesn't answer. The machine is still the giver's to grant from; a
/// token's powers were set when it was minted, so that giver mints a new
/// one. A giver who doesn't read the project now (they left the team, or
/// it's locked to its owners) learns only what of theirs was revoked:
/// not the project's name, nor the owner's.
pub fn notify_revoked(app: Arc<Platform>, r: Revoked) {
    tokio::spawn(async move {
        let msg = revoked_notice(&app, &r).await;
        if let Err(e) = app.hub.push(&r.giver, &msg).await {
            tracing::warn!(account = %r.giver, subject = %r.subject_id, error = %e, "the revoke's notice didn't go");
        }
    });
}

async fn revoked_notice(app: &Platform, r: &Revoked) -> Value {
    let token = r.subject_kind == "agent";
    let subject = if token {
        app.db.agent_token(&r.subject_id).ok().flatten().map(|t| t.name)
    } else {
        let ids = std::slice::from_ref(&r.subject_id);
        app.cache.daemons(&*app.hub, ids).await.ok().and_then(|mut d| d.remove(&r.subject_id)).map(|d| d.name)
    };
    let subject = subject.unwrap_or_else(|| r.subject_id.clone());
    let powers = r.powers.join(", ");
    let scope = AgentScope::Project(r.project.clone());
    let body = match owner_role(app, &r.giver, &scope, false).await.ok().flatten() {
        None => format!(
            "{subject}'s {powers} over a project you no longer read was revoked by one of its owners.{}",
            if token { " The token keeps its reach elsewhere." } else { "" }
        ),
        Some((scope, _)) => {
            let ids = std::slice::from_ref(&r.by);
            let owner =
                app.cache.accounts(&*app.hub, ids).await.ok().and_then(|a| a.get(&r.by).map(|a| a.name.clone()));
            let owner = owner.map_or_else(|| "An owner".to_owned(), |name| format!("{name}, an owner"));
            if token {
                format!(
                    "{owner} of {scope}, revoked {subject}'s {powers} there. The token keeps its reach elsewhere; its \
                     powers were set when it was minted, so for {scope} again, mint a new token under Settings, \
                     Agents with no machine, on the platform page."
                )
            } else {
                format!(
                    "{owner} of {scope}, revoked {subject}'s {powers} there. The machine is still yours: you can give \
                     it again under Settings, Agents, on the platform page, or with `arugula grants add`."
                )
            }
        }
    };
    json!({
        "title": "A grant was revoked",
        "body": body,
        "tag": format!("grant-revoked-{}-{}", r.subject_id, r.project),
        "control": true,
    })
}

/// A project's owner revokes a grant over the project that they didn't
/// give (Arugula #683), as they may a token's reach over it
/// (`agent_tokens::revoke_over`). Only a grant over a project: one over an
/// account (a teammate's `cloud.drive`, their DMs) isn't there for anyone
/// but its own (404). The caller's role is asked of the hub, not the cache,
/// as before any change: someone not in the project doesn't learn the
/// grant exists (404); a member who isn't an owner is refused (403).
/// Whether this call revoked it: a revoke of one revoked already changes
/// nothing, and tells nobody again.
async fn revoke_as_owner(app: &Platform, account: &str, id: &str) -> Result<(Grant, bool), ApiError> {
    let none = || err(StatusCode::NOT_FOUND, "no such grant");
    let project = app.db.grant_project(id)?.ok_or_else(none)?;
    let p = app.db.issue_project(&project)?.ok_or_else(none)?;
    let role = crate::project_roles::role(app, account, &p, true).await?.ok_or_else(none)?;
    if role != TeamRole::Owner {
        return Err(err(StatusCode::FORBIDDEN, "only the project's owners revoke a grant someone else gave over it"));
    }
    let (g, now) = app.db.revoke_grant_over(&project, id, account, now_ms())?.ok_or_else(none)?;
    if now {
        tracing::info!(%account, %project, grant = %id, giver = %g.granted_by, "a project owner revoked a grant over the project");
    }
    Ok((g, now))
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
/// (threads only); `runs.read` and `runs.start` (M216), and
/// `sessions.start` and `sessions.reply` (aru-121), a project.
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
                 runs.read, runs.start, runs.command, boards.read, boards.write, trackers.write, sessions.start and sessions.reply",
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
            let Some((scope_name, role)) = owner_role(&app, &s.account, &scope, true).await? else {
                return Err(err(StatusCode::FORBIDDEN, "you can only give your machine what you can read yourself"));
            };
            // And never a write power that does nothing for a viewer's
            // machine (Arugula #683).
            if viewer_refused(&b.power, role, &scope) {
                let name = app
                    .cache
                    .daemons(&*app.hub, std::slice::from_ref(&b.daemon))
                    .await?
                    .remove(&b.daemon)
                    .map_or_else(|| b.daemon.clone(), |d| d.name);
                return Err(viewer_write(&b.power, &scope_name, &format!("{name}'s agents")));
            }
            // A grant that lets agents into a room says so there (#122).
            let notice = crate::features::chat().grant_notice(&app, &b.power, scope.row()).await;
            let g = app.db.give_grant(&b.daemon, &b.power, scope.row(), &s.account, now_ms())?;
            if let Some(notice) = notice {
                crate::features::chat().announce(&app, notice).await;
            }
            g
        }
    };
    let g = named(&app, vec![g]).await?.remove(0);
    Ok(Json(wire(&g)))
}
