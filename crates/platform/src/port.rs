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

use arugula_control_wire::forge::Standing;

use crate::{
    Platform, billing, db,
    forge::{
        self, GithubUser, TokenAnswer,
        send::{Outcome, UserToken},
    },
};

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

    /// A team's roster changed (the hub just took a new version): what the
    /// platform kept of the team goes, and billing's seats follow it. Waits
    /// for Stripe, as the roster's route always has.
    fn roster_changed<'a>(&'a self, team: &'a str) -> BoxFuture<'a, anyhow::Result<()>>;

    /// Where each repository a daemon watches stands (M161: the App's
    /// `may()` is the platform's): live, so its pokes come, or why not.
    /// `github` is the account's GitHub identity, if it signed in with
    /// GitHub; `fresh`, that a daemon just asked, not the heartbeat. One
    /// per repository, in the order asked.
    fn forge_standing<'a>(
        &'a self,
        account: &'a str,
        github: Option<&'a GithubUser>,
        repos: &'a [String],
        fresh: bool,
    ) -> BoxFuture<'a, anyhow::Result<Vec<Standing>>>;

    /// A read-only installation token for one repository, for a hosted
    /// box's `POST /api/daemon/github/token` (M161: the App's key is the
    /// platform's), or why not.
    fn github_token<'a>(
        &'a self,
        account: &'a str,
        github: Option<&'a GithubUser>,
        repo: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<TokenAnswer>>;

    /// Send a person's draft as them (M168), for the hub's GitHub callback:
    /// with the one-use user token GitHub just gave it, or, with `None` (the
    /// authorization failed), through one of their machines. The hub
    /// revokes the token once this answers. A platform from before M168
    /// doesn't have it.
    fn send_draft<'a>(
        &'a self,
        account: &'a str,
        draft: &'a str,
        token: Option<UserToken>,
    ) -> BoxFuture<'a, anyhow::Result<Outcome>> {
        let _ = (account, draft, token);
        Box::pin(async { anyhow::bail!("this platform doesn't send drafts") })
    }
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

/// In process. Billing (M139) answers `active_plans`, `roster_changed`
/// and its part of `account_deleted`; Cloud (M137) `delete_blockers` and
/// its part of `account_deleted`; chat (M138) `chat_on` and its part of
/// `account_deleted`; projects and grants, the forge mirror (M162) and
/// native issues (M194), their part of `account_deleted`; the GitHub App
/// (M161) `forge_standing` and `github_token`; sends (M168) `send_draft`.
impl PlatformPort for Platform {
    fn delete_blockers<'a>(&'a self, account: &'a str, start: bool) -> BoxFuture<'a, anyhow::Result<Vec<String>>> {
        Box::pin(async move {
            let blockers = crate::cloud::account::delete_blockers(self, account, start)?;
            Ok(blockers)
        })
    }

    fn account_deleted<'a>(&'a self, deleted: &'a Deleted) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            let mut c = self.db.c();
            let tx = c.transaction()?;
            db::billing::erase_account(&tx, deleted)?;
            // Before the projects go: their hosted workspaces are deleted.
            db::hosted::erase_account(&tx, deleted, arugula_e2e::now_ms())?;
            db::projects::erase_account(&tx, deleted)?;
            db::forge::erase_account(&tx, deleted, arugula_e2e::now_ms())?;
            db::issues::erase_account(&tx, deleted)?;
            let rooms = crate::chat::erase_account(&tx, deleted)?;
            tx.commit()?;
            drop(c);
            crate::chat::account_gone(self, &rooms);
            crate::cloud::account::account_deleted(self, &deleted.account)?;
            Ok(())
        })
    }

    fn active_plans<'a>(&'a self, owners: &'a [String]) -> BoxFuture<'a, anyhow::Result<Vec<String>>> {
        Box::pin(async move { self.db.active_plans(owners) })
    }

    fn chat_on<'a>(&'a self, account: &'a str) -> BoxFuture<'a, anyhow::Result<bool>> {
        Box::pin(crate::chat::chat_on(self, account))
    }

    fn roster_changed<'a>(&'a self, team: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            // What's kept of the team goes now, not at the next feed tick:
            // the roster's route answers after this, so its caller reads
            // the new roster.
            if self.cache.team_changed(team) {
                self.chat.changed();
            }
            billing::sync_seats(self, team).await;
            Ok(())
        })
    }

    fn forge_standing<'a>(
        &'a self,
        account: &'a str,
        github: Option<&'a GithubUser>,
        repos: &'a [String],
        fresh: bool,
    ) -> BoxFuture<'a, anyhow::Result<Vec<Standing>>> {
        Box::pin(async move { Ok(forge::standing(self, account, github, repos, fresh).await) })
    }

    fn github_token<'a>(
        &'a self,
        account: &'a str,
        github: Option<&'a GithubUser>,
        repo: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<TokenAnswer>> {
        Box::pin(forge::token(self, account, github, repo))
    }

    fn send_draft<'a>(
        &'a self,
        account: &'a str,
        draft: &'a str,
        token: Option<UserToken>,
    ) -> BoxFuture<'a, anyhow::Result<Outcome>> {
        Box::pin(async move {
            let how = match token {
                Some(t) => forge::send::How::Token(t),
                None => forge::send::How::Machine,
            };
            Ok(forge::send::send(self, account, draft, how).await.into())
        })
    }
}
