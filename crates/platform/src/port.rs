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

use arugula_control_wire::{
    forge::Standing,
    work::{Report, Reported, Work},
};

pub use crate::hosted::inference::TurnAnswer;
use crate::{
    Platform, billing, db,
    forge::{
        self, GithubUser, TokenAnswer,
        send::{Outcome, UserToken},
    },
    work::HostedMachine,
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

    /// Wake a hosted workspace's box (M182): someone opened its machine
    /// and its daemon isn't connected. Opens at once share one resume,
    /// which the workspace's worker does. Answers once the box runs (its
    /// daemon dials in a moment later) or didn't wake, and why. A platform
    /// from before M182 doesn't have it.
    fn hosted_wake<'a>(
        &'a self,
        project: &'a str,
        workspace: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<arugula_control_wire::hosted::WakeAnswer>> {
        let _ = (project, workspace);
        Box::pin(async { anyhow::bail!("this platform doesn't wake hosted workspaces") })
    }

    /// A hosted box asks to start an agent turn of `agent` (M185), signed
    /// as its hosted machine, whose certificate names its workspace: whose
    /// inference key pays, or why it can't run. A platform from before
    /// M185 doesn't have it.
    fn hosted_turn<'a>(
        &'a self,
        workspace: &'a str,
        machine: &'a str,
        agent: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<TurnAnswer>> {
        let _ = (workspace, machine, agent);
        Box::pin(async { anyhow::bail!("this platform doesn't run hosted agents") })
    }

    /// The turn ended: whether it was that box's, open.
    fn hosted_turn_end<'a>(
        &'a self,
        workspace: &'a str,
        machine: &'a str,
        turn: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<bool>> {
        let _ = (workspace, machine, turn);
        Box::pin(async { anyhow::bail!("this platform doesn't run hosted agents") })
    }

    /// A hosted machine's work (M212), for the hub's `GET
    /// /api/hosted/work`: the runs ready for it, of the kinds it `takes`.
    /// The hub checked its signature, and says who it is. A platform from
    /// before M212 has none to give.
    fn hosted_work<'a>(
        &'a self,
        machine: &'a HostedMachine,
        takes: &'a [String],
    ) -> BoxFuture<'a, anyhow::Result<Work>> {
        let _ = (machine, takes);
        Box::pin(async { Ok(Work::default()) })
    }

    /// A hosted machine's signed report of a run's state change (M212), for
    /// the hub's `POST /api/hosted/work/report`. A platform from before
    /// M212 can't take it.
    fn hosted_report<'a>(
        &'a self,
        machine: &'a HostedMachine,
        report: &'a Report,
    ) -> BoxFuture<'a, anyhow::Result<Reported>> {
        let _ = (machine, report);
        Box::pin(async { anyhow::bail!("this platform doesn't take runs' reports") })
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
/// (M161) `forge_standing` and `github_token`; sends (M168) `send_draft`;
/// runs (M210), deploys (M217) and what starts runs (M214), their part of
/// `account_deleted`; hosted workspaces (M182) `hosted_wake`; hosted
/// inference (M185) `hosted_turn` and `hosted_turn_end`; hosted machines'
/// runs (M212) `hosted_work` and `hosted_report`.
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
            db::inference::erase_account(&tx, deleted)?;
            db::projects::erase_account(&tx, deleted)?;
            db::forge::erase_account(&tx, deleted, arugula_e2e::now_ms())?;
            db::issues::erase_account(&tx, deleted)?;
            db::runs::erase_account(&tx, deleted)?;
            db::deploys::erase_account(&tx, deleted)?;
            db::triggers::erase_account(&tx, deleted)?;
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

    fn hosted_wake<'a>(
        &'a self,
        project: &'a str,
        workspace: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<arugula_control_wire::hosted::WakeAnswer>> {
        Box::pin(crate::hosted::wake::wake(self, project, workspace))
    }

    fn hosted_turn<'a>(
        &'a self,
        workspace: &'a str,
        machine: &'a str,
        agent: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<TurnAnswer>> {
        Box::pin(crate::hosted::inference::turn(self, workspace, machine, agent))
    }

    fn hosted_turn_end<'a>(
        &'a self,
        workspace: &'a str,
        machine: &'a str,
        turn: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<bool>> {
        Box::pin(crate::hosted::inference::turn_end(self, workspace, machine, turn))
    }

    fn hosted_work<'a>(
        &'a self,
        machine: &'a HostedMachine,
        takes: &'a [String],
    ) -> BoxFuture<'a, anyhow::Result<Work>> {
        Box::pin(crate::work::work(self, machine, takes))
    }

    fn hosted_report<'a>(
        &'a self,
        machine: &'a HostedMachine,
        report: &'a Report,
    ) -> BoxFuture<'a, anyhow::Result<Reported>> {
        Box::pin(crate::work::report(self, machine, report))
    }
}
