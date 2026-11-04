//! A project's Access view (Arugula #682): who can do what here, read in
//! one place by any member of the project. `GET /api/projects/{id}/grants`
//! lists, with no change to what's enforced:
//!
//! - **project roles:** the team's roster as the platform's cache holds
//!   it (the same read [`crate::grants::agent_may`] makes), or the one
//!   account a personal project belongs to;
//! - **cloud.drive:** one row per member, since every machine joined to an
//!   account holds `cloud.drive` over it from its join
//!   ([`crate::grants::can_drive_cloud`]) and drives the project's cloud
//!   workspaces as its owner's role allows ([`crate::cloud::CloudDriver`]):
//!   how many of their machines drive, and that nobody revokes it per
//!   project (its owner does, per machine). The view never says "no
//!   power here" while this is true;
//! - **powers:** every live grant whose scope is this project, machines'
//!   and agent grant tokens', with whose each is and, when it does nothing
//!   now, why: a write power on a viewer's (the one definition,
//!   [`crate::grants::idle_for`]), a giver who left the project, a machine
//!   that isn't its giver's any more;
//! - **allow:** what a project owner signed: the delegation for hosted
//!   machines (control's) and the standing run grant with its expiry (the
//!   platform's, confirmed with a passkey), stale when it names an earlier
//!   delegation ([`crate::run_grant::is_stale`]);
//! - **cloud workspaces:** the project's, and that an owner or editor has
//!   full use of each (the daemon inside takes the roster and checks it
//!   itself).
//!
//! Each part says whose word it is, so nobody reads control's copy as a
//! machine's decision. Who may list is who may read the project at all:
//! a member, by [`membership`], so a stranger gets the 404 every other
//! project route gives them, not a refusal that says the project exists.
//! Every member sees that a token holds powers here, its name, who minted
//! it and what it may do (Jake, 2026-11-04); a token's id and a grant's
//! id, which only a revoke needs, are in the answer for the project's
//! owners alone, since only they revoke what they didn't give. A giver who
//! left the project is "a former member", with no name or account. Never
//! here: another account's grant over their DMs, a token, a workspace's
//! sandbox id, a grant's signature. Only who, what and when. 600 reads an
//! hour per account ([`ACCESS_READS`]).

use std::{collections::HashMap, sync::Arc};

use arugula_control_common::{ApiError, err};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde_json::{Value, json};

use crate::{
    Platform,
    auth::Person,
    db::projects::{CLOUD_DRIVE, subject_kind},
    forge::links::membership,
    grants::{AgentScope, agent_names, idle_for},
};

type R = Result<Json<Value>, ApiError>;

/// The most reads of a project's Access view an account makes in an hour
/// (each one asks control for the project's delegation).
pub const ACCESS_READS: (&str, usize) = ("project-access", 600);

/// What each part of the view says of where it comes from.
pub const ROSTER_SAYS: &str =
    "the team's roster, as control's copy of it stands; each machine checks the roster for itself";
pub const PERSONAL_SAYS: &str = "this account's own project: its owner alone";
pub const CLOUD_DRIVE_SAYS: &str = "the platform's cloud.drive grants, one per machine over its owner's account, given when the \
                                    machine joins: its agents drive this project's cloud workspaces as its owner's role \
                                    allows. Its owner revokes it per machine (arugula grants revoke); nobody revokes it per \
                                    project";
pub const POWERS_SAYS: &str = "the platform's grants, asked again on every call an agent makes; a grant reaches nothing past what its owner reads";
pub const ALLOW_SAYS: &str =
    "signed by a project owner: the delegation on control, the standing run grant with a passkey";
pub const WORKSPACES_SAYS: &str =
    "the platform's list; the machine inside each takes the project's roster from control and checks it itself";
/// Who has full use of a cloud workspace (Jake, 2026-10-21: an editor
/// there is as an owner, `crates/daemon/src/control.rs`).
pub const FULL_USE: &str = "owners and editors have full use; viewers watch";
/// What a grant's row says of a giver who isn't in the project now.
pub const FORMER_MEMBER: &str = "a former member";

/// Why a grant does nothing now, beyond [`idle_for`]'s: its giver isn't
/// in the project, or the machine isn't the giver's any more.
pub const IDLE_LEFT: &str =
    "does nothing: its giver doesn't read this project now, and an agent never reaches more than its owner";
pub const IDLE_NOT_THEIRS: &str = "does nothing: the machine isn't its giver's now";
pub const IDLE_TOKEN_GONE: &str = "does nothing: the token was revoked, or isn't its minter's";

/// `GET /api/projects/{project}/grants`: the project's Access view, for
/// its members.
pub async fn list(State(app): State<Arc<Platform>>, Person { account }: Person, Path(asked): Path<String>) -> R {
    app.limits.check_account(ACCESS_READS, &account)?;
    // A stranger gets what every project route gives them: no such
    // project. The roster comes from the cache, as `agent_may` reads it.
    let (project, role) = membership(&app, &account, &asked, false).await?;
    let owner = app.db.project_owner(&project)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such project"))?;
    let is_owner = role == TeamRole::Owner;
    let now = now_ms();

    // Roles: the roster through the cache, or the one account.
    let team = match &owner.team {
        Some(t) => app.cache.team(&*app.hub, t).await?,
        None => None,
    };
    let members: Vec<(String, Option<String>, TeamRole)> = match (&owner.account, &team) {
        (Some(a), _) => vec![(a.clone(), None, TeamRole::Owner)],
        // The roster as the cache holds it: while the team is locked,
        // only its owners are in it (`Team::role`).
        (None, Some(t)) => t
            .members
            .iter()
            .filter_map(|m| t.role(&m.account).map(|r| (m.account.clone(), Some(m.name.clone()), r)))
            .collect(),
        (None, None) => vec![],
    };
    let role_of = |account: &str| -> Option<TeamRole> {
        match (&owner.account, &team) {
            (Some(a), _) => (a == account).then_some(TeamRole::Owner),
            (None, Some(t)) => t.role(account),
            (None, None) => None,
        }
    };

    // Powers: every live grant over the project, named.
    let grants = app.db.grants_over_project(&project)?;
    let mut subjects: Vec<String> = grants.iter().map(|g| g.subject_id.clone()).collect();
    subjects.sort();
    subjects.dedup();
    let named = agent_names(&app, &subjects).await?;
    let token_ids: Vec<String> = subjects.iter().filter(|id| subject_kind(id) == "agent").cloned().collect();
    let tokens: HashMap<String, _> =
        app.db.agent_tokens_by_id(&token_ids)?.into_iter().map(|t| (t.id.clone(), t)).collect();

    // cloud.drive: each member's own machines, and which of them drive.
    let mut drives = Vec::with_capacity(members.len());
    for (account, _, role) in &members {
        let ids = app.cache.machines_for(&*app.hub, account).await?;
        let mine: Vec<String> = app
            .cache
            .daemons(&*app.hub, &ids)
            .await?
            .into_values()
            .filter(|d| d.account == *account)
            .map(|d| d.id)
            .collect();
        let revoked = app.db.revoked_cloud_drive(account)?;
        let driving = mine.iter().filter(|m| !revoked.contains(m)).count();
        drives.push((account.clone(), *role, mine.len(), driving));
    }

    // What a project owner signed.
    let signer = match (&owner.team, &owner.account) {
        (Some(t), _) => format!("team:{t}"),
        (None, Some(a)) => format!("account:{a}"),
        (None, None) => String::new(),
    };
    let delegation = match app.hub.live_delegation(&project, &signer).await {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::debug!(error = %e, %project, "control couldn't say the project's delegation");
            None
        }
    };
    let live = app.db.live_run_grant(&project, now)?;
    let stale = crate::run_grant::is_stale(live.as_ref(), delegation.as_ref().and_then(|d| d.as_deref()));
    let run_grant = live.map(|g| {
        let by = if g.grant.account.is_empty() { g.posted_by.clone() } else { Some(g.grant.account.clone()) };
        json!({
            "id": g.grant.id,
            "made": g.grant.made,
            "expires": g.grant.expires,
            "account": by,
            "created_at": g.created_at,
            // It names an earlier delegation (`run_grant::show`'s
            // `stale`): machines enrolled since refuse it, so it's as
            // good as none until an owner signs it again.
            "stale": stale,
        })
    });

    let workspaces = app.db.hosted_workspaces(&project)?;

    // Names for every account named and in the project now: grant givers,
    // the run grant's signer, workspace makers, a personal project's
    // owner. One who left isn't named.
    let mut accounts: Vec<String> = grants.iter().map(|g| g.granted_by.clone()).collect();
    accounts.extend(workspaces.iter().filter_map(|w| w.created_by.clone()));
    accounts.extend(run_grant.as_ref().and_then(|g| g["account"].as_str().map(str::to_owned)));
    accounts.extend(members.iter().map(|(a, _, _)| a.clone()));
    accounts.retain(|a| role_of(a).is_some());
    accounts.sort();
    accounts.dedup();
    let names = app.cache.accounts(&*app.hub, &accounts).await?;
    let name_of = |account: &str| -> Value {
        if role_of(account).is_none() {
            return json!({ "account": Value::Null, "name": Value::Null, "former": true });
        }
        let n = names.get(account).map(|a| if a.name.is_empty() { a.login.clone() } else { a.name.clone() });
        json!({ "account": account, "name": n, "former": false })
    };

    let roles: Vec<Value> = members
        .iter()
        .map(|(a, name, role)| {
            let name = name.clone().or_else(|| name_of(a)["name"].as_str().map(str::to_owned));
            json!({ "account": a, "name": name, "role": role.as_str() })
        })
        .collect();

    let scope = AgentScope::Project(project.clone());
    let powers: Vec<Value> = grants
        .iter()
        .map(|g| {
            let d = named.get(&g.subject_id);
            let owner_role = role_of(&g.granted_by);
            // Why it does nothing now, if it doesn't: the giver left; the
            // machine isn't the giver's (a token isn't live, or its
            // minter's); or the one definition, a write on a viewer's.
            let token = g.subject_kind == "agent";
            let theirs = if token {
                tokens.get(&g.subject_id).is_some_and(|t| t.account == g.granted_by && t.revoked_at.is_none())
            } else {
                d.is_some_and(|d| d.account == g.granted_by)
            };
            let idle = match owner_role {
                None => Some(IDLE_LEFT.to_owned()),
                Some(_) if !theirs && token => Some(IDLE_TOKEN_GONE.to_owned()),
                Some(_) if !theirs => Some(IDLE_NOT_THEIRS.to_owned()),
                Some(role) => idle_for(&g.power, role, &scope, &owner.name),
            };
            json!({
                // Ids are what a revoke needs: the owners', who may.
                "id": is_owner.then_some(&g.id),
                "subject": {
                    "kind": g.subject_kind,
                    "id": (is_owner || !token).then_some(&g.subject_id),
                    "name": d.map(|d| d.name.clone()),
                },
                "owner": name_of(&g.granted_by),
                "owner_role": owner_role.map(TeamRole::as_str),
                "power": g.power,
                "created_at": g.created_at,
                "idle": idle,
            })
        })
        .collect();

    let cloud_drive: Vec<Value> = drives
        .iter()
        .map(|(account, role, machines, driving)| {
            json!({
                "owner": name_of(account),
                "role": role.as_str(),
                "power": CLOUD_DRIVE,
                "machines": machines,
                "driving": driving,
            })
        })
        .collect();

    let workspaces: Vec<Value> = workspaces
        .iter()
        .map(|w| {
            json!({
                "id": w.id,
                "name": w.name,
                "state": w.state,
                "created_by": w.created_by.as_deref().map(name_of),
                "created_at": w.created_at,
                "full_use": FULL_USE,
            })
        })
        .collect();

    Ok(Json(json!({
        "project": {
            "id": project,
            "name": owner.name.clone(),
            "owner": owner.team.as_ref().map_or_else(
                || json!({ "kind": "account", "id": owner.account }),
                |t| json!({ "kind": "team", "id": t, "name": team.as_ref().map(|t| t.name.clone()), "locked": team.as_ref().is_some_and(|t| t.locked) }),
            ),
        },
        "role": role.as_str(),
        "roles": { "says": if owner.team.is_some() { ROSTER_SAYS } else { PERSONAL_SAYS }, "members": roles },
        "cloud_drive": { "says": CLOUD_DRIVE_SAYS, "members": cloud_drive },
        "powers": { "says": POWERS_SAYS, "grants": powers },
        "allow": {
            "says": ALLOW_SAYS,
            // `null`: control couldn't say.
            "delegation": delegation.map(|id| json!({ "live": id.is_some(), "id": id })),
            "run_grant": run_grant,
        },
        "workspaces": { "says": WORKSPACES_SAYS, "full_use": FULL_USE, "list": workspaces },
    })))
}
