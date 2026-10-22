//! What starts runs (M214, platform step 7): by hand, on a schedule, or on
//! a linked repo's event. Each makes a run `pending` on a project's hosted
//! workspace and queues its `run` job ([`crate::db::runs::dispatch`]); the
//! workspace's machine executes it (M212).
//!
//! - **Run specs** live in the project, as rows edited here
//!   ([`routes`]) and with `arugula runs` (the page's part is M215): a
//!   name, the hosted workspace, and a command or an instruction.
//! - **By hand** ([`routes::start`]): a person, or a CLI device as its
//!   account, starts a spec or a command. A machine's agent doesn't, here:
//!   MCP (M216) does, under the project's standing grant (M211).
//! - **Schedules** ([`schedule`]): the platform schedules, and the machine
//!   executes. That answers `docs/platform-features.md`'s question: a
//!   schedule keeps ticking with every person's machine asleep.
//! - **Events** ([`events`]): rules matched as the App's webhook brings a
//!   linked repo's events.
//!
//! The rows, and how a trigger makes exactly one run, are
//! [`crate::db::triggers`]'s. Schedules' ticks ([`schedule::Ticks`]) run on
//! the platform's one job worker, beside runs' dispatch
//! ([`crate::work::start`]): one machine fires a schedule at a time (the
//! job's lease).

//!
//! **Who's answerable.** A schedule or a rule fires only while the person
//! who made it may still start runs on its project: one of its owners or
//! editors, on the hosted beta's list (`ARUGULA_HOSTED_ACCOUNTS`). Checked
//! each time it fires ([`Gate`]): one whose maker left the team, became a
//! viewer, left the list or deleted their account passes its tick or event
//! with no run, saying why, until someone who may makes it again.
//!
//! **A run's trigger source is untrusted text.** An event's source holds
//! what GitHub's payload said (branches, a check's name, who opened a PR),
//! which anyone who can open a PR or push a branch chooses. It's for
//! showing, quoted. It never goes into a run's environment, its command or
//! an agent's prompt unquoted: what a run does is its spec's operation, and
//! only that.

pub mod events;
pub mod routes;
pub mod schedule;

use std::sync::Arc;

use arugula_e2e::team::TeamRole;
use axum::http::StatusCode;
use futures_util::future::BoxFuture;

use crate::Platform;

/// Whether the maker of a schedule or a rule may still start runs on its
/// project as it fires.
pub trait Gate: Send + Sync {
    /// `None`: they may. `Some(why)`: they may not, and the trigger passes
    /// with no run. An error is the hub's, not an answer: try again.
    fn refuses<'a>(
        &'a self,
        project: &'a str,
        maker: Option<&'a str>,
    ) -> BoxFuture<'a, anyhow::Result<Option<&'static str>>>;
}

/// [`Gate`] as the routes have it: an owner or editor of the project (as
/// the hub says now), on the hosted beta's list.
pub struct Members(pub Arc<Platform>);

impl Gate for Members {
    fn refuses<'a>(
        &'a self,
        project: &'a str,
        maker: Option<&'a str>,
    ) -> BoxFuture<'a, anyhow::Result<Option<&'static str>>> {
        Box::pin(async move {
            let Some(maker) = maker else { return Ok(Some("its maker's account is gone")) };
            match crate::forge::links::membership(&self.0, maker, project, true).await {
                Ok((_, TeamRole::Viewer)) => Ok(Some("its maker can only view the project now")),
                Ok(_) if !self.0.hosted.allows(maker) => Ok(Some("its maker isn't on the hosted beta's list")),
                Ok(_) => Ok(None),
                Err(e) if e.0 == StatusCode::NOT_FOUND => Ok(Some("its maker isn't in the project any more")),
                Err(e) => anyhow::bail!("asking the hub who's in the project: {}", e.1),
            }
        })
    }
}

/// [`Gate`] that lets every maker through (tests).
#[cfg(test)]
pub struct Open;

#[cfg(test)]
impl Gate for Open {
    fn refuses<'a>(&'a self, _: &'a str, _: Option<&'a str>) -> BoxFuture<'a, anyhow::Result<Option<&'static str>>> {
        Box::pin(async { Ok(None) })
    }
}

#[cfg(test)]
mod tests;
