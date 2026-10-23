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
//! M140, so they keep `arugula-control-wire`'s two rules: the bytes don't
//! change, and nothing is `deny_unknown_fields`.
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
use arugula_e2e::team::TeamRole;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::forge::GithubUser;

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

    /// The products an account is linked to (M229): each product's user
    /// that a product linked to it by numeric GitHub id, by product. Empty
    /// for an account with none, or that isn't there. A hub from before
    /// M229 can't say.
    fn product_links<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<ProductLink>>> {
        let _ = account;
        Box::pin(async { anyhow::bail!("this hub doesn't link products' accounts") })
    }

    /// End one of `account`'s product links (M229), as the person asked on
    /// the platform page: audited, and the product sees it on its next
    /// call. Whether there was such a link of this account's. A hub from
    /// before M229 can't.
    fn unlink_product<'a>(
        &'a self,
        account: &'a str,
        client: &'a str,
        user: &'a str,
    ) -> BoxFuture<'a, HubResult<bool>> {
        let _ = (account, client, user);
        Box::pin(async { anyhow::bail!("this hub doesn't link products' accounts") })
    }
}

/// A product's user linked to an account (M229, [`HubPort::product_links`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductLink {
    /// The product: its issuer client id.
    pub client: String,
    /// The product's name, as its client is registered.
    pub product: String,
    /// The product's own id for its user.
    pub user: String,
    /// How the product names its user, when it said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// When it was linked (ms).
    pub linked: u64,
}

/// A passkey's assertion, as the page got it from `navigator.credentials.get`
/// ([`HubPort::verify_passkey`]): each base64url, no padding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasskeyAssertion {
    /// The credential's id (`rawId`).
    pub id: String,
    #[serde(rename = "clientDataJSON")]
    pub client_data_json: String,
    #[serde(rename = "authenticatorData")]
    pub authenticator_data: String,
    pub signature: String,
}

/// What the hub found of a passkey's assertion: good, or why not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasskeyVerdict {
    pub ok: bool,
    #[serde(default)]
    pub why: String,
}

/// An account's devices ([`HubPort::device_trust`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceTrust {
    /// The root device's id.
    pub root: String,
    /// Its approved certificates.
    pub certs: Vec<arugula_e2e::Cert>,
    pub revocations: Vec<arugula_e2e::Revocation>,
    /// Its browsers and phones that a grandfathered app window approved:
    /// both rows stored and approved by the hub before the app had a kind
    /// of its own (by the hub's own record of when), removed since or not,
    /// as a removed device's past approvals stand on any chain. The window
    /// counts as the link just above one of them, nowhere else
    /// (`arugula_e2e::cert::legacy_app_link`); the device that signs must
    /// still be trusted now. Empty from a hub that
    /// doesn't send it, which only refuses more.
    #[serde(default)]
    pub legacy_app_children: Vec<String>,
}

/// How a push goes ([`HubPort::push_with`]). The default is what
/// [`HubPort::push`] does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushOptions {
    /// How long a push service keeps it for a device that's away (its
    /// `TTL`, at most a day): `None` is the hub's own, an hour. Cloud's
    /// approvals keep five minutes.
    #[serde(default, with = "secs")]
    pub ttl: Option<std::time::Duration>,
    /// Every push subscription the account has, as control's own notices
    /// go, rather than only those signed by one of its approved devices
    /// that isn't revoked. Chat's pushes go this way, as they always have
    /// (M122).
    #[serde(default)]
    pub any_subscription: bool,
}

/// A TTL on the wire: whole seconds.
mod secs {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(d: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_some(&d.as_secs()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
        Ok(Option::<u64>::deserialize(d)?.map(Duration::from_secs))
    }
}

/// An account, with its names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    /// Its sign-in login (GitHub's), or "".
    pub login: String,
    /// The name it chose, else its GitHub login, else "".
    pub name: String,
}

/// A principal the hub still knows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    pub principal: Principal,
    pub account: Account,
    /// The machine, for a daemon.
    pub daemon: Option<Daemon>,
}

/// A team, as its latest signed roster says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    /// The account that made it.
    pub founder: String,
    /// While it's locked, members other than owners are as good as out.
    pub locked: bool,
    /// Its latest roster's version.
    pub version: u64,
    pub members: Vec<Member>,
}

impl Team {
    /// An account's role, if it's in the team now: while the team is
    /// locked, only owners are.
    pub fn role(&self, account: &str) -> Option<TeamRole> {
        let role = self.members.iter().find(|m| m.account == account).map(|m| m.role)?;
        (!self.locked || role == TeamRole::Owner).then_some(role)
    }
}

/// A member of a team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub account: String,
    pub role: TeamRole,
    /// What the roster calls them.
    pub name: String,
}

/// A joined machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Daemon {
    pub id: String,
    /// The account it's joined to: its owner.
    pub account: String,
    /// The team it's in, if it's a team's.
    pub team: Option<String>,
    pub name: String,
}

/// Labs flags by name: on or off. One not here is unset.
pub type Flags = BTreeMap<String, bool>;

/// What a push did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pushed {
    /// Subscriptions a push service took it for.
    pub sent: u32,
    /// Subscriptions whose push service said they're gone: the hub forgot
    /// them.
    pub gone: u32,
    /// Subscriptions it didn't reach (a push service failed or refused).
    pub failed: u32,
}

/// What an account used, for billing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meter {
    pub account: String,
    pub relay_bytes: u64,
    pub sandbox_minutes: u64,
}

/// A page of the hub's change feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangePage {
    pub changes: Vec<Change>,
    /// The newest change the hub has written, whether or not it's in this
    /// page: a reader with no cache starts here.
    pub last: u64,
    /// Changes after the one asked from were pruned before anyone read
    /// them: drop every cache, and go on from this page.
    pub gap: bool,
}

/// One thing that changed in the hub. Written in the same transaction as
/// the change itself, so a reader never sees a change the hub didn't make.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    /// Increasing, never reused.
    pub seq: u64,
    /// When (ms).
    pub at: u64,
    pub kind: ChangeKind,
    /// What changed: see each kind.
    pub subject: String,
}

/// What kind of thing changed. The hub only adds kinds after the platform
/// knows them (it deploys first); one it doesn't know reads as `Unknown`,
/// which a reader treats as "drop every cache".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChangeKind {
    /// An account's name (subject: the account), its display name or the
    /// login it signs in with, or a new account (whose name and login are
    /// new).
    #[serde(rename = "account.name")]
    AccountName,
    /// An account was deleted. The subject is "": nothing of a deleted
    /// account is kept (#173), and the rows naming it, its devices, its
    /// machines and the teams it disbanded go too. Drop every cache.
    #[serde(rename = "account.deleted")]
    AccountDeleted,
    /// A device or machine was revoked (subject: its id).
    #[serde(rename = "device.revoked")]
    DeviceRevoked,
    /// A team was made, or its roster changed (subject: the team).
    #[serde(rename = "team.roster")]
    TeamRoster,
    /// A team was locked or unlocked (subject: the team).
    #[serde(rename = "team.locked")]
    TeamLocked,
    /// A machine joined, or its name changed (subject: the machine).
    #[serde(rename = "daemon.joined")]
    DaemonJoined,
    /// A machine moved to a team or out of one (subject: the machine).
    #[serde(rename = "daemon.moved")]
    DaemonMoved,
    /// A machine left, or was removed (subject: the machine).
    #[serde(rename = "daemon.left")]
    DaemonLeft,
    /// Whom a machine is shared with changed: whom it lets in, or someone's
    /// answer to its offer (subject: the machine).
    #[serde(rename = "daemon.access")]
    DaemonAccess,
    /// A Labs flag was set or unset (subject: its owner, `account:ID` or
    /// `team:ID`).
    #[serde(rename = "labs.flag")]
    LabsFlag,
    #[serde(other)]
    Unknown,
}

impl ChangeKind {
    /// Every kind the hub writes.
    pub const ALL: [ChangeKind; 10] = [
        ChangeKind::AccountName,
        ChangeKind::AccountDeleted,
        ChangeKind::DeviceRevoked,
        ChangeKind::TeamRoster,
        ChangeKind::TeamLocked,
        ChangeKind::DaemonJoined,
        ChangeKind::DaemonMoved,
        ChangeKind::DaemonLeft,
        ChangeKind::DaemonAccess,
        ChangeKind::LabsFlag,
    ];

    /// As the outbox keeps it, and as it goes on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::AccountName => "account.name",
            ChangeKind::AccountDeleted => "account.deleted",
            ChangeKind::DeviceRevoked => "device.revoked",
            ChangeKind::TeamRoster => "team.roster",
            ChangeKind::TeamLocked => "team.locked",
            ChangeKind::DaemonJoined => "daemon.joined",
            ChangeKind::DaemonMoved => "daemon.moved",
            ChangeKind::DaemonLeft => "daemon.left",
            ChangeKind::DaemonAccess => "daemon.access",
            ChangeKind::LabsFlag => "labs.flag",
            ChangeKind::Unknown => "unknown",
        }
    }

    /// From the outbox: `Unknown` for a kind this build doesn't know.
    pub fn parse(s: &str) -> Self {
        Self::ALL.into_iter().find(|k| k.as_str() == s).unwrap_or(ChangeKind::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_on_the_wire() {
        for k in ChangeKind::ALL {
            assert_eq!(serde_json::to_value(k).unwrap(), serde_json::json!(k.as_str()));
            assert_eq!(ChangeKind::parse(k.as_str()), k);
        }
        assert_eq!(serde_json::from_str::<ChangeKind>("\"team.renamed\"").unwrap(), ChangeKind::Unknown);
        assert_eq!(ChangeKind::parse("team.renamed"), ChangeKind::Unknown);
    }

    #[test]
    fn push_options_on_the_wire() {
        let o = PushOptions { ttl: Some(std::time::Duration::from_secs(300)), any_subscription: true };
        assert_eq!(serde_json::to_value(o).unwrap(), serde_json::json!({ "ttl": 300, "any_subscription": true }));
        assert_eq!(serde_json::from_str::<PushOptions>("{}").unwrap(), PushOptions::default());
        assert_eq!(serde_json::from_value::<PushOptions>(serde_json::to_value(o).unwrap()).unwrap(), o);
    }

    #[test]
    fn a_locked_team_has_only_owners() {
        let m = |account: &str, role| Member { account: account.into(), role, name: account.into() };
        let mut t = Team {
            id: "t".into(),
            name: "t".into(),
            founder: "a".into(),
            locked: false,
            version: 1,
            members: vec![m("a", TeamRole::Owner), m("b", TeamRole::Editor)],
        };
        assert_eq!((t.role("a"), t.role("b"), t.role("c")), (Some(TeamRole::Owner), Some(TeamRole::Editor), None));
        t.locked = true;
        assert_eq!((t.role("a"), t.role("b")), (Some(TeamRole::Owner), None));
    }
}
