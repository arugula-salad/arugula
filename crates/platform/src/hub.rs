//! `HubPort`: the platform's only view of the hub ("Control is two
//! processes: a hub and a platform", #1153).
//!
//! The platform never reads a hub table. Who someone is, their name, their
//! teams, which machine is whose and who may reach it, Labs flags, billing's
//! meters, push, and what changed since it last looked all come through
//! this trait. In process it's `hub_port::InProcess` in `arugula-control`,
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
use arugula_e2e::team::TeamRole;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

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

    /// The Labs flags each owner set (`account:ID` or `team:ID`), by owner
    /// and then by name. An owner that set none is left out.
    fn labs_flags<'a>(&'a self, owners: &'a [String]) -> BoxFuture<'a, HubResult<BTreeMap<String, Flags>>>;

    /// What these accounts used, for billing: bytes relayed in the calendar
    /// month (UTC) that `until` (ms) falls in, and hosted sandbox minutes
    /// between `since` and `until`. One per account asked, in that order.
    fn meters<'a>(&'a self, accounts: &'a [String], since: u64, until: u64) -> BoxFuture<'a, HubResult<Vec<Meter>>>;

    /// Every account that has had a hosted sandbox: whose minutes billing
    /// reports.
    fn metered_accounts(&self) -> BoxFuture<'_, HubResult<Vec<String>>>;

    /// What changed in the hub after `after` (a [`Change::seq`]), oldest
    /// first, at most `limit` of them: the platform's caches follow this.
    fn changes(&self, after: u64, limit: u32) -> BoxFuture<'_, HubResult<ChangePage>>;
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
    /// login it signs in with.
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
