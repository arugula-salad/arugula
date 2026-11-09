//! Labs flags per account and per team (#347): control keeps them and
//! sends them to joined machines with their certificates
//! (`wire::TrustAnswer::flags`), nudging those machines when one changes.
//!
//! - **Who sets them.** An account sets its own, for every machine it
//!   owns. A team's owners set the team's, for the team's machines (those
//!   added to it, `daemons.team`), not for members' other machines.
//! - **Chat reaches a member's own machines too, as a word of its own.**
//!   Chat isn't a Labs flag but control's own setting that rides along
//!   with them (native #203), and the platform reads it per account: the
//!   account's own word, else on when any team the account is in turned
//!   it on. A machine joined to the account alone is sent
//!   `wire::CHAT_HEARD` in `team` when a team of its owner's has chat on
//!   (native #673), so its thread readers and threads follow the platform.
//!   The other flags stay the team's machines' alone.
//! - **Doc copies are the account's own word, `wire::DOC_COPIES`** (native
//!   #673): set by a person on the page for their own account, never by a
//!   team, and sent only to the account's personal machines, the one kind
//!   that copies its Docs up. An account that had its own `chat` on when
//!   this came in is taken to have said yes (`db::migrate`), so nobody's copies stop.
//! - **Who wins.** The machine: its own `flags.json` beats anything here.
//!   Then the account's, then the team's (`arugula_proto::flags` resolves
//!   it; control only sends both).
//! - **Some are the machine's alone.** `agent-delegation`, and `guest-ssh` (a
//!   machine's sharing setting now, native #205)
//!   (`wire::LOCAL_ONLY_FLAGS`), are refused here; a daemon ignores them
//!   from control anyway.
//! - **Who sees them.** The page shows them only to someone with a machine
//!   that has Developer settings (`wire::DEV_SETTINGS` in its features):
//!   strangers never see Labs.
//! - **What a flag is.** Control doesn't know the list (it doesn't depend
//!   on `arugula-proto`): a name is any short lowercase word, and a daemon
//!   ignores one it doesn't have. The web offers the list it was built
//!   with.
//! - **Renamed flags (aru-117).** Control knows the renames
//!   (`wire::FLAG_ALIASES`): an old name set from an older page is stored
//!   as the new one (the database's own were moved at startup), and a
//!   machine is sent both, so one from before the rename still hears it.

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

/// Control's own chat setting, the one a personal machine hears from its
/// owner's teams as `wire::CHAT_HEARD`.
const CHAT: &str = "chat";

/// What a machine is sent: its account's flags, and its team's if it's
/// in one; a personal machine's `team` is [`heard_from_teams`] instead.
pub fn for_daemon(app: &App, account: &str, device: &str) -> anyhow::Result<wire::Flags> {
    let on_team = app.db.daemon_team(device)?;
    let team = match &on_team {
        Some(t) => app.db.labs_flags(&team_owner(t))?,
        None => heard_from_teams(app, account)?,
    };
    let mut account = app.db.labs_flags(&account_owner(account))?;
    // A team's machine never copies Docs: not on its enrolling member's word.
    if on_team.is_some() {
        account.remove(wire::DOC_COPIES);
    }
    Ok(wire::Flags { account: with_old_names(account), team: with_old_names(team) })
}

/// `wire::CHAT_HEARD`, when any of `account`'s teams turned chat on, as
/// the platform reads chat for the account's own machines.
fn heard_from_teams(app: &App, account: &str) -> anyhow::Result<BTreeMap<String, bool>> {
    let mut out = BTreeMap::new();
    for body in app.db.teams_of(account)? {
        // A roster that doesn't parse is that team's trouble, not this
        // machine's refresh.
        let team = match crate::teams::parse(&body) {
            Ok(r) => r.team,
            Err(e) => {
                tracing::warn!(account, error = %e, "a team's roster doesn't parse: its chat isn't heard");
                continue;
            }
        };
        let flags = app.db.labs_flags(&team_owner(&team))?;
        if flags.get(CHAT) == Some(&true) {
            out.insert(wire::CHAT_HEARD.to_owned(), true);
            break;
        }
    }
    Ok(out)
}

/// Each renamed flag under its old name too (aru-117), for a daemon from
/// before the rename; a newer one reads the new name first.
fn with_old_names(mut flags: BTreeMap<String, bool>) -> BTreeMap<String, bool> {
    for (old, new) in wire::FLAG_ALIASES {
        if let Some(on) = flags.get(*new).copied() {
            flags.entry(old.to_string()).or_insert(on);
        }
    }
    flags
}

/// A flag's name now: an old one (aru-117) is the flag it was renamed to.
fn canonical(name: String) -> String {
    wire::FLAG_ALIASES.iter().find(|(old, _)| *old == name).map_or(name, |(_, new)| new.to_string())
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
pub async fn set(State(app): State<Arc<App>>, s: Session, Json(mut b): Json<Set>) -> R {
    if !valid(&b.name) {
        return Err(err(StatusCode::BAD_REQUEST, "that isn't a flag's name"));
    }
    b.name = canonical(b.name);
    if b.name == wire::DOC_COPIES && b.team.is_some() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "copying Docs to the platform is each account's own word, not a team's",
        ));
    }
    // Off is kept as off, never unset, so the once-only carry-over can't be
    // mistaken for an answer the account never gave.
    if b.name == wire::DOC_COPIES && b.on.is_none() {
        return Err(err(StatusCode::BAD_REQUEST, "copying Docs is on or off: send true or false"));
    }
    if wire::LOCAL_ONLY_FLAGS.contains(&b.name.as_str()) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "only a machine turns that flag on or off, in its own Developer settings",
        ));
    }
    let (owner, daemons) = match &b.team {
        Some(team) => {
            let roster = crate::teams::latest(&app, team)?;
            if crate::teams::role_in(&roster, &s.account) != Some(TeamRole::Owner) {
                return Err(err(StatusCode::FORBIDDEN, "a team's owners set its flags"));
            }
            let mut daemons = app.db.team_daemons(team)?;
            // The members' own machines hear the team's chat (`for_daemon`).
            if b.name == CHAT {
                for m in &roster.members {
                    daemons.extend(app.db.daemons(&m.account)?.into_iter().map(|d| d.id));
                }
                daemons.sort();
                daemons.dedup();
            }
            (team_owner(team), daemons)
        }
        None => (account_owner(&s.account), app.db.daemons(&s.account)?.into_iter().map(|d| d.id).collect()),
    };
    let now = app.db.labs_flags(&owner)?;
    if b.on.is_some() && !now.contains_key(&b.name) && now.len() >= MAX {
        return Err(err(StatusCode::BAD_REQUEST, "too many flags"));
    }
    if app.db.set_labs_flag(&owner, &b.name, b.on)? {
        // Control's own chat is behind its `chat` flag (M122): the platform
        // hears of it from the change feed.
        app.relay.nudge(&daemons);
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
