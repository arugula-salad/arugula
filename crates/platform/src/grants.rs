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

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
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
    auth::Person,
    db::projects::{CLOUD_DRIVE, Grant},
    hub::HubResult,
};

type R = Result<Json<Value>, ApiError>;

fn wire(g: &Grant) -> Value {
    json!({
        "id": g.id,
        "subject": { "kind": g.subject_kind, "id": g.subject_id, "name": g.subject_name },
        "power": g.power,
        "scope": { "kind": g.scope_kind, "id": g.scope_id },
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
/// keeps it).
async fn named(app: &Platform, mut grants: Vec<Grant>) -> HubResult<Vec<Grant>> {
    let mut ids: Vec<String> = grants.iter().map(|g| g.subject_id.clone()).collect();
    ids.sort();
    ids.dedup();
    let daemons = app.cache.daemons(&*app.hub, &ids).await?;
    for g in &mut grants {
        g.subject_name = daemons.get(&g.subject_id).map(|d| d.name.clone());
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
    let mut ungranted = vec![];
    for d in app.cache.daemons(&*app.hub, &machines).await?.into_values() {
        if d.account == s.account && app.db.cloud_drive(&d.id, &d.account)?.is_none() {
            ungranted.push(d.id);
        }
    }
    if !ungranted.is_empty() {
        for d in app.cache.daemons_now(&*app.hub, &ungranted).await?.into_values() {
            if d.account == s.account {
                app.db.grant_on_join(&d.id, &d.account, now_ms())?;
            }
        }
    }
    let mut grants = named(&app, app.db.grants(&s.account)?).await?;
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
}

/// `POST /api/grants {daemon, power}`: give one of the account's machines
/// `cloud.drive` again, as a new grant. One it holds already is the answer.
pub async fn give(State(app): State<Arc<Platform>>, s: Person, Json(b): Json<Give>) -> R {
    if b.power != CLOUD_DRIVE {
        return Err(err(StatusCode::BAD_REQUEST, "the one power there is to give is cloud.drive"));
    }
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
    let g = app.db.give_cloud_drive(&s.account, &b.daemon, &s.account, now_ms())?;
    let g = named(&app, vec![g]).await?.remove(0);
    Ok(Json(wire(&g)))
}
