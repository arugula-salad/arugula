//! The launch switches for hosted boxes (#531). The operator sets them on
//! the hub (`arugula-control switches`, docs/control-ops.md), which keeps
//! them and answers each account's standing ([`HubPort::standings`]); the
//! platform counts what they cap and keeps to them:
//!
//! - **Making a box** (a new workspace, or a rebuild's new box) is refused
//!   while new boxes are `off`; while they're `waitlist`ed, unless the
//!   account is admitted (or on `ARUGULA_HOSTED_ACCOUNTS`), and the hub
//!   puts it on the waitlist; for a suspended account; and once the boxes
//!   of the workspaces it made have run its box-hours this UTC month. A
//!   new workspace is also refused once it has made its cap of live ones,
//!   in any project ([`crate::db::hosted::Added::MakerFull`]). When the hub
//!   can't say, nothing is made: try again.
//! - **Waking a box** (an open, or the hub's wake on connect) is refused
//!   when its workspace's maker is suspended or past its hours. `off`
//!   doesn't stop it: what's there keeps going. When the hub can't say, it
//!   wakes.
//! - **What's running** ([`sweep`], every [`SWEEP`] on the leader): a
//!   suspended maker's boxes are paused at once, mid-turn too, and not held
//!   or woken again ([`super::hold::Holds`]); a maker past its hours has its
//!   boxes paused between turns.
//!
//! Box-hours are each box's running time, whatever its size, from the
//! platform's own meter ([`super::usage`]), deleted workspaces' included.
//! Deploy and snapshot boxes aren't counted or stopped: they're the
//! platform's, made for a deploy or a workspace's setup.
//!
//! [`HubPort::standings`]: crate::HubPort::standings

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    time::Duration,
};

use arugula_control_common::{ApiError, refusal};
use arugula_control_common::port::Standing;
use axum::http::StatusCode;
use tracing::{info, warn};

use super::{Ask, HostedWorkspace};
use crate::{
    Platform,
    hosted::broker::prices::{month_start, next_month},
};

/// How often the leader looks at what's running against the switches.
pub const SWEEP: Duration = Duration::from_secs(30);

/// Reasons beside a refusal, for clients that act on which it was.
pub const OFF: &str = "hosted_off";
pub const WAITLIST: &str = "waitlist";
pub const BOX_CAP: &str = "box_cap";
pub const BOX_HOURS: &str = "box_hours";
pub const SUSPENDED: &str = "suspended";

/// What a suspended account is told (the hub's words).
const SUSPENDED_SAY: &str =
    "This Arugula account is suspended. If you think that's a mistake, write to hello@arugula.io.";

const HOUR_MS: u64 = 3_600_000;

/// `1 December`: the day a month starts, as people read it.
fn day(ms: u64) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let m = crate::hosted::broker::prices::month_of(ms);
    let n: usize = m[5..7].parse().unwrap_or(1);
    format!("1 {}", MONTHS[n.saturating_sub(1) % 12])
}

/// The hours `account`'s workspaces' boxes ran this UTC month, and its cap,
/// in ms.
fn hours(app: &Platform, s: &Standing, now: u64) -> anyhow::Result<(u64, u64)> {
    let used = app.db.hosted_ms_made_by(&s.account, month_start(now), now)?;
    Ok((used, s.box_hours.saturating_mul(HOUR_MS)))
}

/// What's refused past the hours: `said`, then when they start again.
fn past_hours(said: &str) -> ApiError {
    refusal(
        StatusCode::TOO_MANY_REQUESTS,
        BOX_HOURS,
        &format!(
            "{said} They start again on {} (UTC); the workspaces stay as they are until then.",
            day(next_month(arugula_e2e::now_ms()))
        ),
    )
}

/// One account's standing, asking the hub; `waiting` puts it on the
/// waitlist if it isn't admitted.
async fn standing(app: &Platform, account: &str, waiting: bool) -> anyhow::Result<Option<Standing>> {
    Ok(app.hub.standings(&[account.to_owned()], waiting).await?.into_iter().find(|s| s.account == account))
}

/// Whether hosted boxes are open to `account`: the shell offers to
/// delegate its projects (M178), and runs and thread sessions may start on
/// its cloud workspaces. On `ARUGULA_HOSTED_ACCOUNTS`, or (by the launch
/// switches) not suspended and admitted or with new boxes open to anyone.
/// `off` doesn't close them: what's there keeps going. When the hub can't
/// say, the list alone.
pub async fn open_to(app: &Platform, account: &str) -> bool {
    if app.hosted.allows(account) {
        return true;
    }
    match standing(app, account, false).await {
        Ok(Some(s)) => !s.suspended && (s.admitted || s.hosted == "open"),
        Ok(None) => false,
        Err(error) => {
            warn!(%account, %error, "the hub didn't say an account's standing: hosted boxes by the list alone");
            false
        }
    }
}

/// Whether `account` may have a box made now: a new workspace (`new`), or
/// a rebuild's. For a new one, the most live workspaces it may have made.
pub async fn may_make(app: &Platform, account: &str, new: bool) -> Result<u64, ApiError> {
    let listed = app.hosted.allows(account);
    let s = match standing(app, account, !listed).await {
        Ok(Some(s)) => s,
        Ok(None) => return Err(refusal(StatusCode::UNAUTHORIZED, SUSPENDED, "sign in again")),
        Err(error) => {
            warn!(%account, %error, "the hub didn't say an account's standing: no box made");
            return Err(err_unavailable());
        }
    };
    if s.suspended {
        return Err(refusal(StatusCode::FORBIDDEN, SUSPENDED, SUSPENDED_SAY));
    }
    match s.hosted.as_str() {
        "open" => {}
        "waitlist" if s.admitted || listed => {}
        "waitlist" => {
            info!(%account, "turned away from a new hosted box: on the waitlist");
            return Err(refusal(
                StatusCode::FORBIDDEN,
                WAITLIST,
                "Cloud workspaces are invite-only during the beta. You're on the waitlist now, and you can make \
                 one as soon as you're let in.",
            ));
        }
        // `off`, or a word from a newer hub: nothing new.
        _ => {
            return Err(refusal(
                StatusCode::SERVICE_UNAVAILABLE,
                OFF,
                if new {
                    "New cloud workspaces are turned off for now. The ones there are keep running and open as usual; \
                     try again later."
                } else {
                    "Rebuilding cloud workspaces is turned off for now, since it makes a new box. The ones there are \
                     keep running and open as usual; try again later."
                },
            ));
        }
    }
    let (used, cap) = hours(app, &s, arugula_e2e::now_ms())?;
    if used >= cap {
        return Err(past_hours(&format!(
            "Your cloud workspaces have used this account's {} box-hours for this month.",
            s.box_hours
        )));
    }
    Ok(s.boxes)
}

fn err_unavailable() -> ApiError {
    arugula_control_common::err(
        StatusCode::SERVICE_UNAVAILABLE,
        "Arugula couldn't check this account's limits just now: try again in a minute.",
    )
}

/// What a new workspace past its maker's cap is told.
pub fn box_cap(made: u64) -> ApiError {
    refusal(
        StatusCode::CONFLICT,
        BOX_CAP,
        &format!(
            "You've made {made} cloud workspace{} that are still there, the most one account may have for now. \
             Delete one to make another.",
            if made == 1 { "" } else { "s" }
        ),
    )
}

/// Whether `w`'s box may be woken: not when its maker is suspended or
/// past its hours. When the hub can't say, it may.
pub async fn may_wake(app: &Platform, w: &HostedWorkspace) -> Result<(), ApiError> {
    if w.purpose != "workspace" {
        return Ok(());
    }
    let Some(maker) = w.created_by.as_deref() else { return Ok(()) };
    let s = match standing(app, maker, false).await {
        Ok(Some(s)) => s,
        Ok(None) => return Ok(()),
        Err(error) => {
            warn!(workspace = %w.id, %error, "the hub didn't say a workspace maker's standing: waking it");
            return Ok(());
        }
    };
    if s.suspended {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            SUSPENDED,
            "This cloud workspace can't start: the account that made it is suspended.",
        ));
    }
    let (used, cap) = hours(app, &s, arugula_e2e::now_ms())?;
    if used >= cap {
        return Err(past_hours(&format!(
            "This cloud workspace can't start: the account that made it has used its {} box-hours for this month.",
            s.box_hours
        )));
    }
    Ok(())
}

/// One look at what's running: pause a suspended maker's boxes at once and
/// a maker's past its hours between turns. The workspaces whose boxes are
/// stopped for a suspension, which nothing holds or wakes.
pub async fn sweep(app: &Platform, now: u64) -> anyhow::Result<HashSet<String>> {
    let running = app.db.hosted_running_makers()?;
    let mut stopped = HashSet::new();
    if running.is_empty() {
        return Ok(stopped);
    }
    let makers: Vec<String> = running.iter().map(|(_, m)| m.clone()).collect::<BTreeSet<_>>().into_iter().collect();
    let standings: HashMap<String, Standing> =
        app.hub.standings(&makers, false).await?.into_iter().map(|s| (s.account.clone(), s)).collect();
    let mut over: HashMap<&str, bool> = HashMap::new();
    for (workspace, maker) in &running {
        let Some(s) = standings.get(maker) else { continue };
        let why = if s.suspended {
            stopped.insert(workspace.clone());
            "its maker is suspended"
        } else {
            let past = match over.get(maker.as_str()) {
                Some(p) => *p,
                None => {
                    let (used, cap) = hours(app, s, now)?;
                    *over.entry(maker).or_insert(used >= cap)
                }
            };
            if !past {
                continue;
            }
            "its maker is past its box-hours"
        };
        if super::ask(app, workspace, Ask::Pause)? {
            info!(%workspace, why, "pausing a hosted box for the launch switches");
            app.wakes.kick();
        }
    }
    Ok(stopped)
}

/// Whether the workspace's maker is suspended: a pause doesn't wait for
/// its turn to end. `false` when the hub can't say.
pub async fn maker_suspended(app: &Platform, w: &HostedWorkspace) -> bool {
    let Some(maker) = w.created_by.as_deref().filter(|_| w.purpose == "workspace") else { return false };
    matches!(standing(app, maker, false).await, Ok(Some(s)) if s.suspended)
}

#[cfg(test)]
#[path = "switches_tests.rs"]
mod tests;
