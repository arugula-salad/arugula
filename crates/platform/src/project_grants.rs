//! A project's Access view (Arugula #682): who can do what here, read in
//! one place by any member of the project. `GET /api/projects/{id}/grants`
//! lists, with no change to what's enforced:
//!
//! - **project roles:** the team's roster as the platform's cache holds
//!   it (the same read [`crate::grants::agent_may`] makes), or the one
//!   account a personal project belongs to;
//! - **cloud.drive:** one row per member, since every machine joined to an
//!   account holds `cloud.drive` over it from its join
//!   ([`crate::grants::can_drive_cloud`]) and acts as its owner, within the
//!   owner's role, on every route that takes a [`crate::cloud::CloudDriver`]:
//!   the project's cloud workspaces (making, opening, rebuilding and
//!   deleting them, their specs, secrets, ports and public previews, their
//!   archived transcripts), its linked repos (linking, unlinking, content
//!   mirroring) and their pull requests, issues and drafts, its Plane
//!   connections and their history, its inference keys and its Claude
//!   spend: how many of their machines hold it, and that nobody revokes it
//!   per project (its owner does, per machine). The view never says "no
//!   other power" as if nothing reached the project while this is true;
//! - **powers:** every live grant whose scope is this project, machines'
//!   and agent grant tokens', with whose each is and, when it does less
//!   than the power now, what (`limited`: a label and why): a write power
//!   on a viewer's (the one definition the grants list uses,
//!   [`crate::grants::limited_for`]), a giver who left the project or is
//!   locked out of it, a machine that isn't its giver's any more;
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
//! left the project is "a former member", with no name or account, and
//! neither is their machine or token named; a locked-out member is named
//! and said to be locked out. Never
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
    grants::{AgentScope, agent_names, limited_for},
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
                                    machine joins: its agents act as its owner, within the owner's role, on this project's \
                                    cloud workspaces (making, opening, rebuilding, deleting; their specs, secrets, ports, \
                                    previews and transcripts), its linked repos and their pull requests, issues and drafts, \
                                    its Plane connections, its inference keys and its Claude spend. Its owner revokes it per \
                                    machine (arugula grants revoke); nobody revokes it per project";
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

/// Why a grant does nothing now, beyond [`limited_for`]'s: its giver isn't
/// in the project, or is locked out of it, or the machine isn't the
/// giver's any more.
pub const IDLE_LEFT: &str =
    "does nothing: its giver doesn't read this project now, and an agent never reaches more than its owner";
pub const IDLE_LOCKED_OUT: &str = "does nothing while the team is locked: its giver isn't one of its owners, and an agent never reaches more than its owner";
pub const IDLE_NOT_THEIRS: &str = "does nothing: the machine isn't its giver's now";
pub const IDLE_TOKEN_GONE: &str = "does nothing: the token was revoked, or isn't its minter's";

/// Where an account stands in the project now: in it with a role; on the
/// roster but locked out (a locked team's editor or viewer); or not on
/// the roster at all, a former member (or never one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    Member(TeamRole),
    LockedOut(TeamRole),
    Former,
}

impl Standing {
    fn role(self) -> Option<TeamRole> {
        match self {
            Standing::Member(r) => Some(r),
            _ => None,
        }
    }
}

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
    // Who's in the project now, with a role: the roster as the cache holds
    // it, and while the team is locked only its owners (`Team::role`).
    let members: Vec<(String, Option<String>, TeamRole)> = match (&owner.account, &team) {
        (Some(a), _) => vec![(a.clone(), None, TeamRole::Owner)],
        (None, Some(t)) => t
            .members
            .iter()
            .filter_map(|m| t.role(&m.account).map(|r| (m.account.clone(), Some(m.name.clone()), r)))
            .collect(),
        (None, None) => vec![],
    };
    let standing = |account: &str| -> Standing {
        match (&owner.account, &team) {
            (Some(a), _) if a == account => Standing::Member(TeamRole::Owner),
            (None, Some(t)) => match (t.role(account), t.members.iter().find(|m| m.account == account)) {
                (Some(r), _) => Standing::Member(r),
                (None, Some(m)) => Standing::LockedOut(m.role),
                (None, None) => Standing::Former,
            },
            _ => Standing::Former,
        }
    };
    let role_of = |account: &str| standing(account).role();

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
    let signer_of = live
        .as_ref()
        .and_then(|g| if g.grant.account.is_empty() { g.posted_by.clone() } else { Some(g.grant.account.clone()) });
    let run_grant = live.map(|g| {
        json!({
            "id": g.grant.id,
            "made": g.grant.made,
            "expires": g.grant.expires,
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
    accounts.extend(signer_of.clone());
    accounts.extend(members.iter().map(|(a, _, _)| a.clone()));
    accounts.retain(|a| standing(a) != Standing::Former);
    accounts.sort();
    accounts.dedup();
    let names = app.cache.accounts(&*app.hub, &accounts).await?;
    // Who someone is to the project now: a member by name; a locked-out
    // member by name, said to be locked out; a former member unnamed.
    let name_of = |account: &str| -> Value {
        let n = || names.get(account).map(|a| if a.name.is_empty() { a.login.clone() } else { a.name.clone() });
        match standing(account) {
            Standing::Member(_) => json!({ "account": account, "name": n(), "former": false, "locked_out": false }),
            Standing::LockedOut(_) => json!({ "account": account, "name": n(), "former": false, "locked_out": true }),
            Standing::Former => {
                json!({ "account": Value::Null, "name": Value::Null, "former": true, "locked_out": false })
            }
        }
    };
    // The run grant's signer, named the same way (never a raw account).
    let run_grant = run_grant.map(|mut g| {
        g["by"] = signer_of.as_deref().map_or(Value::Null, name_of);
        g
    });

    // The roster's rows: everyone on it, a locked-out member said so.
    let roles: Vec<Value> = match (&owner.account, &team) {
        (Some(a), _) => vec![json!({ "account": a, "name": name_of(a)["name"], "role": "owner", "locked_out": false })],
        (None, Some(t)) => t
            .members
            .iter()
            .map(|m| {
                json!({ "account": m.account, "name": m.name, "role": m.role.as_str(), "locked_out": t.role(&m.account).is_none() })
            })
            .collect(),
        (None, None) => vec![],
    };

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
            let giver = standing(&g.granted_by);
            // What it does now, when less than the power: the label is
            // the word a list shows ("idle"), `why` the sentence. A viewer's
            // is the grants list's own definition (`limited_for`).
            let idle = |why: &str| json!({ "label": "idle", "why": why });
            let limited = match giver {
                Standing::Former => Some(idle(IDLE_LEFT)),
                Standing::LockedOut(_) => Some(idle(IDLE_LOCKED_OUT)),
                Standing::Member(_) if !theirs && token => Some(idle(IDLE_TOKEN_GONE)),
                Standing::Member(_) if !theirs => Some(idle(IDLE_NOT_THEIRS)),
                Standing::Member(role) => limited_for(&g.power, role, &scope, &owner.name).map(|l| json!(l)),
            };
            // A former member's machine or token isn't named either: the
            // row says a former member's agents held the power, no more.
            let named = giver != Standing::Former;
            json!({
                // Ids are what a revoke needs: the owners', who may.
                "id": is_owner.then_some(&g.id),
                "subject": {
                    "kind": g.subject_kind,
                    "id": (named && (is_owner || !token)).then_some(&g.subject_id),
                    "name": if named { d.map(|d| d.name.clone()) } else { None },
                },
                "owner": name_of(&g.granted_by),
                "owner_role": owner_role.map(TeamRole::as_str),
                "power": g.power,
                "created_at": g.created_at,
                "limited": limited,
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
