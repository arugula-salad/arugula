//! Labs flags per account and per team (#347): control keeps them and
//! sends them to joined machines with their certificates
//! (`wire::TrustAnswer::flags`), nudging those machines when one changes.
//!
//! - **Who sets them.** An account sets its own, for every machine it
//!   owns. A team's owners set the team's, for the team's machines (those
//!   added to it, `daemons.team`), not for members' other machines.
//! - **Who wins.** The machine: its own `flags.json` beats anything here.
//!   Then the account's, then the team's (`arugula_proto::flags` resolves
//!   it; control only sends both).
//! - **Some are the machine's alone.** `guest-ssh` and `agents`
//!   (`wire::LOCAL_ONLY_FLAGS`) are refused here; a daemon ignores them
//!   from control anyway.
//! - **Who sees them.** The page shows them only to someone with a machine
//!   that has Developer settings (`wire::DEV_SETTINGS` in its features):
//!   strangers never see Labs.
//! - **What a flag is.** Control doesn't know the list (it doesn't depend
//!   on `arugula-proto`): a name is any short lowercase word, and a daemon
//!   ignores one it doesn't have. The web offers the list it was built
//!   with.

use std::{collections::BTreeMap, sync::Arc};

use arugula_control_wire as wire;
use arugula_e2e::team::TeamRole;
use axum::{Json, extract::State, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{ApiError, App, auth::Session, err};

type R = Result<Json<Value>, ApiError>;

/// The most flags one account or team keeps: far more than there are.
const MAX: usize = 64;

pub fn account_owner(account: &str) -> String {
    format!("account:{account}")
}

pub fn team_owner(team: &str) -> String {
    format!("team:{team}")
}

/// What a machine is sent: its account's flags, and its team's if it's
/// in one.
pub fn for_daemon(app: &App, account: &str, device: &str) -> anyhow::Result<wire::Flags> {
    let team = match app.db.daemon_team(device)? {
        Some(t) => app.db.labs_flags(&team_owner(&t))?,
        None => BTreeMap::new(),
    };
    Ok(wire::Flags { account: app.db.labs_flags(&account_owner(account))?, team })
}

/// `GET /api/flags`: the signed-in account's flags and those of each team
/// it's in, with whether it may set them (owners), and whether it has a
/// machine with Developer settings (`dev`), which the page shows them for.
pub async fn list(State(app): State<Arc<App>>, s: Session) -> R {
    let mut teams = serde_json::Map::new();
    for body in app.db.teams_of(&s.account)? {
        let r = crate::teams::parse(&body)?;
        let owner = crate::teams::role_in(&r, &s.account) == Some(TeamRole::Owner);
        let flags = app.db.labs_flags(&team_owner(&r.team))?;
        teams.insert(r.team.clone(), json!({ "flags": flags, "owner": owner }));
    }
    // Labs isn't for strangers: the page offers these only to someone with
    // a machine whose owner asked for Developer settings (it says so).
    let mut dev = false;
    for d in app.db.daemons(&s.account)? {
        if crate::teams::has_feature(&app.db.daemon_features(&d.id)?, wire::DEV_SETTINGS) {
            dev = true;
            break;
        }
    }
    Ok(Json(json!({ "account": app.db.labs_flags(&account_owner(&s.account))?, "teams": teams, "dev": dev })))
}

#[derive(Deserialize)]
pub struct Set {
    name: String,
    /// `null` unsets it.
    on: Option<bool>,
    /// The team's flag, not the account's.
    #[serde(default)]
    team: Option<String>,
}

/// A flag's name as daemons spell them: a short lowercase word.
fn valid(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && name != "labs"
}

/// `POST /api/flags`: set or unset one of the account's flags, or (an
/// owner) one of a team's; the machines it reaches look again now.
pub async fn set(State(app): State<Arc<App>>, s: Session, Json(b): Json<Set>) -> R {
    if !valid(&b.name) {
        return Err(err(StatusCode::BAD_REQUEST, "that isn't a flag's name"));
    }
    if wire::LOCAL_ONLY_FLAGS.contains(&b.name.as_str()) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "only a machine turns that flag on or off, in its own Developer settings",
        ));
    }
    let (owner, daemons) = match &b.team {
        Some(team) => {
            if crate::teams::role_in(&crate::teams::latest(&app, team)?, &s.account) != Some(TeamRole::Owner) {
                return Err(err(StatusCode::FORBIDDEN, "a team's owners set its flags"));
            }
            (team_owner(team), app.db.team_daemons(team)?)
        }
        None => (account_owner(&s.account), app.db.daemons(&s.account)?.into_iter().map(|d| d.id).collect()),
    };
    let now = app.db.labs_flags(&owner)?;
    if b.on.is_some() && !now.contains_key(&b.name) && now.len() >= MAX {
        return Err(err(StatusCode::BAD_REQUEST, "too many flags"));
    }
    if app.db.set_labs_flag(&owner, &b.name, b.on)? {
        app.relay.nudge(&daemons);
        // Control's own chat is behind its `chat` flag (M122).
        app.chat.changed();
    }
    let props = json!({ "flag": b.name, "on": b.on });
    match &b.team {
        Some(t) => app.analytics.capture_team(&s.account, t, "labs flag set", props),
        None => app.analytics.capture(&s.account, "labs flag set", props),
    }
    Ok(Json(json!({ "flags": app.db.labs_flags(&owner)? })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_a_short_lowercase_word() {
        for ok in ["chat", "guest-ssh", "swarm-themes", "v2"] {
            assert!(valid(ok), "{ok}");
        }
        for bad in ["", "Chat", "a b", "labs", "x/y", &"a".repeat(33)] {
            assert!(!valid(bad), "{bad}");
        }
    }
}
