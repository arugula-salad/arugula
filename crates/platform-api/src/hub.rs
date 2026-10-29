//! `HubPort`: the platform's only view of the hub ("Control is two
//! processes: a hub and a platform", #1153).
//!
//! The platform never reads a hub table. Who someone is, their name, their
//! teams, which machine is whose and who may reach it, Labs flags, billing's
//! meters, push, relaying forge pokes, and what changed since it last
//! looked all come through this trait. In process it's `hub_port::InProcess` in `arugula-control`,
//! over the hub's own state; once the platform is its own process (M140)
//! it's the same calls over the private network, checked by the same
//! contract tests (`contract`, with the `contract` feature).
//!
//! It only grows: a method the platform needs ships in the hub a release
//! before the platform calls it, and nothing here is renamed or removed
//! while a deployed platform might call it. The types cross the network in
//! M140, so they live in `arugula_control_wire::internal` (#219, re-exported
//! here) and keep its two rules: the bytes don't change, and nothing is
//! `deny_unknown_fields`.
//!
//! Lists are batched (one call for a page of rooms, never one per room),
//! answers leave out what isn't there (an account that's gone, a machine
//! that left) rather than fail, and order isn't promised unless a method
//! says so.
//!
//! What isn't here, on purpose:
//! - **Rate limits.** The platform keeps its own `Limits`
//!   (`arugula_control_common::limit`): each limit is counted by one process
//!   only, so neither needs the other's counts.
//! - **Product analytics.** The platform captures its own events
//!   (`arugula_control_common::analytics`). Events are keyed by account, so
//!   PostHog joins them up, and a capture that never waits shouldn't wait on
//!   a call to the hub.
//! - **The VAPID key and push subscriptions.** The hub keeps both (never
//!   regenerate the key: it kills every subscription); the platform asks it
//!   to [`HubPort::push`].

use std::collections::BTreeMap;

use arugula_control_common::Principal;
pub use arugula_control_wire::activity::MachineActivity;
use arugula_control_wire::{
    forge::{DaemonOp, OpAnswer, Poke},
    hosted::{EnrollAsk, EnrollGiven, Enrolled},
};
use futures_util::future::BoxFuture;

use arugula_control_wire::internal::GithubUser;

/// What a hub call answers.
pub type HubResult<T> = anyhow::Result<T>;

/// The platform's view of the hub. Object-safe: the platform holds an
/// `Arc<dyn HubPort>`.
pub trait HubPort: Send + Sync {
    /// Who a principal is now: its account, and the machine for a daemon.
    /// `None` when the account is gone, a CLI device isn't one of its
    /// approved devices or was revoked, or a daemon isn't the account's
    /// machine any more.
    fn resolve<'a>(&'a self, principal: &'a Principal) -> BoxFuture<'a, HubResult<Option<Resolved>>>;

    /// Accounts by id, with their names. Ids with no account are left out.
    fn accounts<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Account>>>;

    /// Whom a word names, as chat's search reads `from:word`: the account
    /// whose login it is (any case), else every account that chose it as
    /// its display name, at most two (so a caller can tell one from many).
    fn accounts_called<'a>(&'a self, word: &'a str) -> BoxFuture<'a, HubResult<Vec<Account>>>;

    /// The teams an account is in, by their latest rosters: ids, sorted.
    fn teams_of<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>>;

    /// Teams by id, each with the members of its latest signed roster that
    /// still have an account. Ids with no team are left out.
    fn teams<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Team>>>;

    /// Machines (daemons) by id: whose each is and which team it's in. Ids
    /// that aren't a joined machine are left out.
    fn daemons<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Daemon>>>;

    /// The machines an account might reach, sorted: its own, its teams',
    /// and those shared with it that control would route it to. A team's
    /// machines are listed even while the team is locked; [`may_reach`]
    /// is the exact check.
    ///
    /// [`may_reach`]: HubPort::may_reach
    fn machines_for<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>>;

    /// Of these machines, the ones control would route `account` to (its
    /// own; its team's while it's in the team, and only owners while the
    /// team is locked; and those shared with it), in the order asked.
    fn may_reach<'a>(&'a self, account: &'a str, machines: &'a [String]) -> BoxFuture<'a, HubResult<Vec<String>>>;

    /// Push a notification to an account's devices: each of its push
    /// subscriptions signed by one of its approved devices that isn't
    /// revoked. `message` is the JSON object the page's service worker gets
    /// (`title`, `body`, `tag` and whatever it opens); the hub encrypts it
    /// for each subscription and signs with its VAPID key. Waits for the
    /// push services' answers.
    fn push<'a>(&'a self, account: &'a str, message: &'a serde_json::Value) -> BoxFuture<'a, HubResult<Pushed>>;

    /// [`push`](HubPort::push), with how long a push service keeps it for
    /// a device that's away, and which subscriptions it goes to (M138).
    /// `push_with(a, m, PushOptions::default())` is `push(a, m)`.
    fn push_with<'a>(
        &'a self,
        account: &'a str,
        message: &'a serde_json::Value,
        options: PushOptions,
    ) -> BoxFuture<'a, HubResult<Pushed>>;

    /// The Labs flags each owner set (`account:ID` or `team:ID`), by owner
    /// and then by name. An owner that set none is left out.
    fn labs_flags<'a>(&'a self, owners: &'a [String]) -> BoxFuture<'a, HubResult<BTreeMap<String, Flags>>>;

    /// What these accounts used, for billing: bytes relayed in the calendar
    /// month (UTC) that `until` (ms) falls in, and hosted sandbox minutes
    /// between `since` and `until` (none since hosted sandboxes went,
    /// M190). One per account asked, in that order.
    fn meters<'a>(&'a self, accounts: &'a [String], since: u64, until: u64) -> BoxFuture<'a, HubResult<Vec<Meter>>>;

    /// Every account that has had a hosted sandbox: whose minutes billing
    /// reports. None since hosted sandboxes went (M190).
    fn metered_accounts(&self) -> BoxFuture<'_, HubResult<Vec<String>>>;

    /// What changed in the hub after `after` (a [`Change::seq`]), oldest
    /// first, at most `limit` of them: the platform's caches follow this.
    fn changes(&self, after: u64, limit: u32) -> BoxFuture<'_, HubResult<ChangePage>>;

    /// The account's personal project (M129), which its Cloud workspaces
    /// and cloud secrets belong to: made if it has none yet, the same one
    /// every time after. `None` when there's no such account, which is how
    /// Cloud tells an account that's gone (M137).
    ///
    /// **Unused by the platform from step 2's projects move on:** projects
    /// are the platform's own (`crate::projects::personal_project`). It
    /// stays because this trait only grows; the in-process hub answers it
    /// from the platform's projects, and no platform from that version on
    /// calls it, so M140's HTTP hub needn't serve it.
    fn personal_project<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<String>>>;

    /// Whether this machine holds a live `cloud.drive` grant over
    /// `account` (M130, "Grants, not impersonation"): what the Cloud
    /// workspace routes check of a daemon (M137).
    ///
    /// **Unused by the platform from step 2's projects move on:** grants
    /// are the platform's own (`crate::grants::can_drive_cloud`), and a
    /// machine's joining reaches them through [`changes`]. It stays because
    /// this trait only grows; the in-process hub answers it from the
    /// platform's grants, and M140's HTTP hub needn't serve it.
    ///
    /// [`changes`]: HubPort::changes
    fn can_drive_cloud<'a>(&'a self, daemon: &'a str, account: &'a str) -> BoxFuture<'a, HubResult<bool>>;

    /// The team's default project (M129), which its chat channels belong
    /// to: made if it has none yet, the same one every time after. `None`
    /// when there's no such team (M138).
    ///
    /// **Unused by the platform from step 2's projects move on**
    /// (`crate::projects::team_project`), as [`personal_project`]: it
    /// stays, the in-process hub answers it from the platform's projects,
    /// and M140's HTTP hub needn't serve it.
    ///
    /// [`personal_project`]: HubPort::personal_project
    fn team_project<'a>(&'a self, team: &'a str) -> BoxFuture<'a, HubResult<Option<String>>>;

    /// Relay a GitHub webhook's pokes (M161) to the daemons watching each
    /// repository over their relay sockets, as `forge.poke` messages, the
    /// same bytes as before the webhook moved: how many messages went. The
    /// hub keeps the subscriptions, and this, until M174 retires the relay.
    /// A hub from before M161 doesn't have it.
    fn forge_pokes<'a>(&'a self, pokes: &'a [Poke]) -> BoxFuture<'a, HubResult<u32>> {
        let _ = pokes;
        Box::pin(async { anyhow::bail!("this hub doesn't relay forge pokes") })
    }

    /// The account's GitHub identity (M163), if it signed in with GitHub:
    /// its numeric id, which is who it is on GitHub, and the login control
    /// last saw. `None` for an account that didn't, or isn't there. A hub
    /// from before M163 doesn't have it.
    fn github_user<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<GithubUser>>> {
        let _ = account;
        Box::pin(async { anyhow::bail!("this hub doesn't say GitHub identities") })
    }

    /// The account that signs in with this numeric GitHub id (M235), if
    /// one does: how an import finds a person from another product's
    /// records (redone's tenants), never by login, which changes hands
    /// ("Accounts link by numeric GitHub id", aru-113). A hub from before
    /// M235 doesn't have it.
    fn github_account(&self, id: u64) -> BoxFuture<'_, HubResult<Option<String>>> {
        let _ = id;
        Box::pin(async { anyhow::bail!("this hub doesn't find accounts by GitHub id") })
    }

    /// GitHub's authorization of the App for a person's send (M168): the
    /// hub keeps what it's for (the account, the draft, and the platform
    /// page's path to come back to) for a few minutes, for one use, and
    /// answers GitHub's page to send the browser to. GitHub comes back to
    /// the hub's `/auth/github/callback`, which sends the draft with the
    /// user token (`PlatformPort::send_draft`). A hub from before M168, or
    /// one without the App's client, can't.
    fn send_authorization<'a>(
        &'a self,
        account: &'a str,
        draft: &'a str,
        back: &'a str,
    ) -> BoxFuture<'a, HubResult<String>> {
        let _ = (account, draft, back);
        Box::pin(async { anyhow::bail!("this hub can't authorize a send with GitHub") })
    }

    /// Ask a machine to do a typed op over its relay socket (M168), and wait
    /// for its answer: `Offline` when it isn't on the relay or doesn't
    /// answer in time, `Unknown` when it never said it knows the op. A hub
    /// from before M168 can't.
    fn daemon_op<'a>(&'a self, daemon: &'a str, op: &'a DaemonOp) -> BoxFuture<'a, HubResult<OpAnswer>> {
        let _ = (daemon, op);
        Box::pin(async { anyhow::bail!("this hub can't ask machines to do ops") })
    }

    /// Record a one-time enrollment for a hosted box the platform is
    /// making (M178), under the project's live delegation: by a ticket the
    /// hub makes (for the box's env) or by the box's sandbox id. The box
    /// enrolls with it once, at the hub's `/api/hosted/enroll`. A hub from
    /// before M178 can't.
    fn hosted_enrollment<'a>(&'a self, ask: &'a EnrollAsk) -> BoxFuture<'a, HubResult<EnrollGiven>> {
        let _ = ask;
        Box::pin(async { anyhow::bail!("this hub doesn't enroll hosted machines") })
    }

    /// Of these hosted workspaces (M178), the ones whose box has enrolled,
    /// and as which machine; and the ones whose enrollment someone tried
    /// again (`taken`: fail closed, the platform deletes the box). A hub
    /// from before M178 can't say.
    fn hosted_enrolled<'a>(&'a self, workspaces: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Enrolled>>> {
        let _ = workspaces;
        Box::pin(async { anyhow::bail!("this hub doesn't enroll hosted machines") })
    }

    /// These hosted workspaces' boxes are gone (deleted, or made again,
    /// M178): the machines they enrolled as leave their projects and are
    /// hung up on, and an enrollment not yet used is dropped. A hub from
    /// before M178 has none to drop.
    fn hosted_gone<'a>(&'a self, workspaces: &'a [String]) -> BoxFuture<'a, HubResult<()>> {
        let _ = workspaces;
        Box::pin(async { Ok(()) })
    }

    /// The id of `project`'s live delegation for hosted machines (M178),
    /// signed for `owner` (`team:<id>` or `account:<id>`) and naming the
    /// hub's hosted key: what a standing grant for runs must name (M211).
    /// `None` when it has none. A hub from before M211 can't say.
    fn live_delegation<'a>(&'a self, project: &'a str, owner: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        let _ = (project, owner);
        Box::pin(async { anyhow::bail!("this hub doesn't say projects' delegations") })
    }

    /// Whether `sig` is `device`'s over `body`, and `device` is a browser
    /// (all the way back to its account's root, as control holds it) of
    /// one of the owners `owner` (`team:<id>` or `account:<id>`) names:
    /// what a project's standing grant and its revokes take (M211). A hub
    /// from before can't say.
    fn owner_browser_signed<'a>(
        &'a self,
        owner: &'a str,
        device: &'a str,
        body: &'a str,
        sig: &'a str,
    ) -> BoxFuture<'a, HubResult<bool>> {
        let _ = (owner, device, body, sig);
        Box::pin(async { anyhow::bail!("this hub doesn't check owners' signatures") })
    }

    /// Whether these machines are working (M180): busy from the start of
    /// an agent turn or a foreground command to its end, as each hosted
    /// daemon last said on its relay socket. Machines that aren't on the
    /// relay, or never said, are left out: treat them as idle. Order isn't
    /// promised. A hub from before M180 can't.
    fn activity<'a>(&'a self, machines: &'a [String]) -> BoxFuture<'a, HubResult<Vec<MachineActivity>>> {
        let _ = machines;
        Box::pin(async { anyhow::bail!("this hub doesn't say whether machines are working") })
    }

    /// Which of these machines are connected to the hub's relay now: the
    /// online dot beside an agent in chat. An error is "can't say" (the
    /// page shows them offline).
    fn machines_online<'a>(&'a self, machines: &'a [String]) -> BoxFuture<'a, HubResult<Vec<String>>> {
        let _ = machines;
        Box::pin(async { anyhow::bail!("this hub doesn't say which machines are online") })
    }

    /// What an account's devices are (M218): its root (the first device it
    /// approved, which its daemons pin), its approved device certificates
    /// and its revocations, as the hub keeps them. The platform decides
    /// from these which devices count (`arugula_e2e::Trust::evaluate`), so
    /// a certificate the hub kept that doesn't chain to the root never
    /// does. `None` when there's no such account, or it has no root yet. A
    /// hub from before M218 doesn't have it.
    fn device_trust<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<DeviceTrust>>> {
        let _ = account;
        Box::pin(async { anyhow::bail!("this hub doesn't say an account's devices") })
    }

    /// Every version of a team's roster the hub keeps, oldest first, as
    /// signed (M218: whose device made an approver an owner). Empty when
    /// there's no such team. A hub from before M218 can't say.
    fn team_rosters<'a>(&'a self, team: &'a str) -> BoxFuture<'a, HubResult<Vec<arugula_e2e::team::Roster>>> {
        let _ = team;
        Box::pin(async { anyhow::bail!("this hub doesn't say a team's rosters") })
    }

    /// Check a passkey's assertion for the platform (M218): the passkey is
    /// registered to `account` (the hub keeps passkeys' public keys: they're
    /// bound to control's own site), the ceremony is a `webauthn.get` from
    /// control's page over exactly `challenge` (base64url, no padding), the
    /// person was verified (UV), and the signature and counter check out.
    /// It signs nobody in. A hub from before M218 can't.
    fn verify_passkey<'a>(
        &'a self,
        account: &'a str,
        challenge: &'a str,
        assertion: &'a PasskeyAssertion,
    ) -> BoxFuture<'a, HubResult<PasskeyVerdict>> {
        let _ = (account, challenge, assertion);
        Box::pin(async { anyhow::bail!("this hub doesn't check passkeys for the platform") })
    }

    /// The products an account let sign it in or is linked to (M229), by
    /// product: its consent, and the product's user linked to it by numeric
    /// GitHub id, if any. Empty for an account with none, or that isn't
    /// there. A hub from before M229 can't say.
    fn product_links<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<ProductLink>>> {
        let _ = account;
        Box::pin(async { anyhow::bail!("this hub doesn't link products' accounts") })
    }

    /// The account an issuer access token (M228, `aat_…`) is for, when it
    /// was issued to `client` and still stands (minutes long, ending with
    /// the hub session it came from): what a client sends to show a person
    /// is present (M236). `None` for anything else. A hub from before M236
    /// can't say.
    fn client_account<'a>(&'a self, token: &'a str, client: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        let _ = (token, client);
        Box::pin(async { anyhow::bail!("this hub doesn't check access tokens for the platform") })
    }

    /// End what `account` has with a product (M229), as the person asked
    /// on the platform page: its link and its consent, audited; the product
    /// sees it on its next call, and the next sign-in through it asks again.
    /// Whether there was either. A hub from before M229 can't.
    fn unlink_product<'a>(&'a self, account: &'a str, client: &'a str) -> BoxFuture<'a, HubResult<bool>> {
        let _ = (account, client);
        Box::pin(async { anyhow::bail!("this hub doesn't link products' accounts") })
    }

    /// The platform account a product's user is linked to (M229), with the
    /// numeric GitHub id it was linked by, or `None`: what planter's import
    /// (M232) asks of each planter, by planter's own user id. A hub from
    /// before M232 can't say.
    fn linked_account<'a>(&'a self, client: &'a str, user: &'a str) -> BoxFuture<'a, HubResult<Option<LinkedAccount>>> {
        let _ = (client, user);
        Box::pin(async { anyhow::bail!("this hub doesn't say products' links") })
    }
}

pub use arugula_control_wire::internal::{
    Account, Change, ChangeKind, ChangePage, Daemon, DeviceTrust, Flags, LinkedAccount, Member, Meter,
    PasskeyAssertion, PasskeyVerdict, ProductLink, PushOptions, Pushed, Resolved, Team,
};
