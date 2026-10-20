//! `PlatformPort`: what the hub asks the platform. The other direction from
//! [`HubPort`](crate::HubPort), and kept apart from it: the hub holds an
//! `Arc<dyn PlatformPort>`. In process it's the [`Platform`] itself, a
//! direct call; once the platform is its own process (M140) it's the same
//! calls over the private network. The hub's gateway, which forwards the
//! platform's routes, is M140's too.
//!
//! Like `HubPort` it only grows, and its types keep
//! `arugula-control-wire`'s two rules.

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::{Platform, billing, db};

pub trait PlatformPort: Send + Sync {
    /// What stands in the way of deleting an account, as sentences for its
    /// person: cloud runs still stopping, Cloud workspaces still being
    /// deleted. With `start`, it also asks for what takes time to go
    /// (Cloud workspaces at Managoat). Empty: go ahead.
    fn delete_blockers<'a>(&'a self, account: &'a str, start: bool) -> BoxFuture<'a, anyhow::Result<Vec<String>>>;

    /// An account is gone from the hub: erase what the platform holds of
    /// it, and tell open pages. Taking it twice changes nothing.
    fn account_deleted<'a>(&'a self, deleted: &'a Deleted) -> BoxFuture<'a, anyhow::Result<()>>;

    /// Of these billing owners (`account:ID`, `team:ID`), the ones with an
    /// active plan: whether someone pays, for the relay's and hosted
    /// sandboxes' limits, and what an account must cancel before it goes.
    fn active_plans<'a>(&'a self, owners: &'a [String]) -> BoxFuture<'a, anyhow::Result<Vec<String>>>;

    /// Whether chat is on for an account, for the page (`/api/me`).
    fn chat_on<'a>(&'a self, account: &'a str) -> BoxFuture<'a, anyhow::Result<bool>>;

    /// A team's roster changed (the hub just took a new version): billing's
    /// seats follow it. Waits for Stripe, as the roster's route always has.
    fn roster_changed<'a>(&'a self, team: &'a str) -> BoxFuture<'a, anyhow::Result<()>>;
}

/// An account the hub deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deleted {
    pub account: String,
    /// Its machines, which left with it.
    pub machines: Vec<String>,
    /// The teams it disbanded.
    pub disbanded: Vec<String>,
}

/// In process. Billing has filled in its parts (M139): `active_plans`,
/// `roster_changed` and its part of `account_deleted`. The rest answer
/// "nothing here" until Cloud fills in `delete_blockers` and its part of
/// `account_deleted` (M137), and chat `chat_on` and its part (M138); until
/// then the hub answers those itself.
impl PlatformPort for Platform {
    fn delete_blockers<'a>(&'a self, _account: &'a str, _start: bool) -> BoxFuture<'a, anyhow::Result<Vec<String>>> {
        Box::pin(async { Ok(vec![]) })
    }

    fn account_deleted<'a>(&'a self, deleted: &'a Deleted) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            let mut c = self.db.c();
            let tx = c.transaction()?;
            db::billing::erase_account(&tx, deleted)?;
            tx.commit()?;
            Ok(())
        })
    }

    fn active_plans<'a>(&'a self, owners: &'a [String]) -> BoxFuture<'a, anyhow::Result<Vec<String>>> {
        Box::pin(async move { self.db.active_plans(owners) })
    }

    fn chat_on<'a>(&'a self, _account: &'a str) -> BoxFuture<'a, anyhow::Result<bool>> {
        Box::pin(async { Ok(false) })
    }

    fn roster_changed<'a>(&'a self, team: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            billing::sync_seats(self, team).await;
            Ok(())
        })
    }
}
