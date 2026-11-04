//! A project's Access view (Arugula #682): who can do what here, read in
//! one place by any member of the project. `GET /api/projects/{id}/grants`
//! lists, with no change to what's enforced:
//!
//! - **project roles:** the team's roster as the platform's cache holds
//!   it (the same read [`crate::grants::agent_may`] makes), or the one
//!   account a personal project belongs to;
//! - **powers:** every live grant whose scope is this project, machines'
//!   and agent grant tokens', with whose each is and whether it reaches
//!   anything now (its owner still reads the project, and the machine is
//!   still the owner's);
//! - **allow:** what a project owner signed: the delegation for hosted
//!   machines (control's) and the standing run grant with its expiry (the
//!   platform's, confirmed with a passkey);
//! - **cloud workspaces:** the project's, and that an owner or editor has
//!   full use of each (the daemon inside takes the roster and checks it
//!   itself).
//!
//! Each part says whose word it is, so nobody reads control's copy as a
//! machine's decision. Who may list is who may read the project at all:
//! a member, by [`membership`], so a stranger gets the 404 every other
//! project route gives them, not a refusal that says the project exists.
//! Never here: a grant over an account (`cloud.drive`, or a power over an
//! owner's DMs), which is its owner's alone to list under `/api/grants`;
//! a token's secret; a workspace's sandbox id; a grant's signature. Only
//! who, what and when.

use std::{collections::HashMap, sync::Arc};

use arugula_control_common::ApiError;
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};

use crate::{Platform, auth::Person, db::projects::subject_kind, forge::links::membership, grants::agent_names};

type R = Result<Json<Value>, ApiError>;

/// What each part of the view says of where it comes from.
pub const ROSTER_SAYS: &str =
    "the team's roster, as control's copy of it stands; each machine checks the roster for itself";
pub const PERSONAL_SAYS: &str = "this account's own project: its owner alone";
pub const POWERS_SAYS: &str = "the platform's grants, asked again on every call an agent makes; a grant reaches nothing past what its owner reads";
pub const ALLOW_SAYS: &str =
    "signed by a project owner: the delegation on control, the standing run grant with a passkey";
pub const WORKSPACES_SAYS: &str =
    "the platform's list; the machine inside each takes the project's roster from control and checks it itself";
/// Who has full use of a cloud workspace (Jake, 2026-10-21: an editor
/// there is as an owner, `crates/daemon/src/control.rs`).
pub const FULL_USE: &str = "owners and editors have full use; viewers watch";

/// `GET /api/projects/{project}/grants`: the project's Access view, for
/// its members.
pub async fn list(State(app): State<Arc<Platform>>, Person { account }: Person, Path(asked): Path<String>) -> R {
    // A stranger gets what every project route gives them: no such
    // project. The roster comes from the cache, as `agent_may` reads it.
    let (project, role) = membership(&app, &account, &asked, false).await?;
    let owner = app
        .db
        .project_owner(&project)?
        .ok_or_else(|| arugula_control_common::err(axum::http::StatusCode::NOT_FOUND, "no such project"))?;
    let now = now_ms();

    // Roles: the roster through the cache, or the one account.
    let team = match &owner.team {
        Some(t) => app.cache.team(&*app.hub, t).await?,
        None => None,
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
    let mut tokens = HashMap::new();
    for id in subjects.iter().filter(|id| subject_kind(id) == "agent") {
        if let Some(t) = app.db.agent_token(id)? {
            tokens.insert(id.clone(), t);
        }
    }

    // What a project owner signed.
    let signer = match (&owner.team, &owner.account) {
        (Some(t), _) => format!("team:{t}"),
        (None, Some(a)) => format!("account:{a}"),
        (None, None) => String::new(),
    };
    let delegation = match app.hub.live_delegation(&project, &signer).await {
        Ok(id) => json!({ "live": id.is_some(), "id": id }),
        Err(e) => {
            tracing::debug!(error = %e, %project, "control couldn't say the project's delegation");
            Value::Null
        }
    };
    let run_grant = app.db.live_run_grant(&project, now)?.map(|g| {
        let by = if g.grant.account.is_empty() { g.posted_by.clone() } else { Some(g.grant.account.clone()) };
        json!({
            "id": g.grant.id,
            "made": g.grant.made,
            "expires": g.grant.expires,
            "account": by,
            "created_at": g.created_at,
        })
    });

    let workspaces = app.db.hosted_workspaces(&project)?;

    // Names for every account named: grant givers, the run grant's signer,
    // workspace makers, and a personal project's owner.
    let mut accounts: Vec<String> = grants.iter().map(|g| g.granted_by.clone()).collect();
    accounts.extend(workspaces.iter().filter_map(|w| w.created_by.clone()));
    accounts.extend(run_grant.as_ref().and_then(|g| g["account"].as_str().map(str::to_owned)));
    accounts.extend(owner.account.clone());
    accounts.sort();
    accounts.dedup();
    let names = app.cache.accounts(&*app.hub, &accounts).await?;
    let name_of = |account: &str| -> Value {
        let n = names.get(account).map(|a| if a.name.is_empty() { a.login.clone() } else { a.name.clone() });
        json!({ "account": account, "name": n })
    };

    let members: Vec<Value> = match (&owner.account, &team) {
        (Some(a), _) => vec![json!({ "account": a, "name": name_of(a)["name"], "role": "owner" })],
        (None, Some(t)) => {
            t.members.iter().map(|m| json!({ "account": m.account, "name": m.name, "role": m.role.as_str() })).collect()
        }
        (None, None) => vec![],
    };

    let powers: Vec<Value> = grants
        .iter()
        .map(|g| {
            let d = named.get(&g.subject_id);
            // A grant counts only while the machine is still its giver's
            // (a token, its minter's and live) and the giver reads the
            // project (`agent_may`).
            let owner_role = role_of(&g.granted_by);
            let still_theirs = match g.subject_kind.as_str() {
                "agent" => {
                    tokens.get(&g.subject_id).is_some_and(|t| t.account == g.granted_by && t.revoked_at.is_none())
                }
                _ => d.is_some_and(|d| d.account == g.granted_by),
            };
            json!({
                "id": g.id,
                "subject": { "kind": g.subject_kind, "id": g.subject_id, "name": d.map(|d| d.name.clone()) },
                "owner": name_of(&g.granted_by),
                "owner_role": owner_role.map(TeamRole::as_str),
                "power": g.power,
                "created_at": g.created_at,
                "reaches": still_theirs && owner_role.is_some(),
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
        "roles": { "says": if owner.team.is_some() { ROSTER_SAYS } else { PERSONAL_SAYS }, "members": members },
        "powers": { "says": POWERS_SAYS, "grants": powers },
        "allow": { "says": ALLOW_SAYS, "delegation": delegation, "run_grant": run_grant },
        "workspaces": { "says": WORKSPACES_SAYS, "full_use": FULL_USE, "list": workspaces },
    })))
}
