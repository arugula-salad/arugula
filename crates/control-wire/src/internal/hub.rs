//! The hub's answers to the platform (`HubPort` in `arugula-platform`):
//! who someone is, their teams and machines, pushes, meters, Labs flags and
//! the hub's change feed.

use std::collections::BTreeMap;

use arugula_e2e::team::TeamRole;
use serde::{Deserialize, Serialize};

use super::Principal;

/// A person by their GitHub identity: the numeric id is who they are, the
/// login what control last saw them called. The hub knows it (they signed
/// in with GitHub) and says it with each call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GithubUser {
    pub id: u64,
    pub login: String,
}

/// A product's user's link (M232, `HubPort::linked_account`). A link is
/// never authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LinkedAccount {
    /// The platform account (the hub's).
    pub account: String,
    /// The numeric GitHub id it was linked by.
    pub github_id: u64,
}

/// A product an account let sign it in, or is linked to (M229,
/// `HubPort::product_links`). A link is never authorization: nothing on
/// the platform grants anything because of one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ProductLink {
    /// The product: its issuer client id.
    pub client: String,
    /// The product's name, as its client is registered.
    pub product: String,
    /// When the person let it sign them in (ms), the consent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granted: Option<u64>,
    /// The product's own id for its user linked to the account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// How the product names that user, when it said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// When it was linked (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked: Option<u64>,
}

/// A passkey's assertion, as the page got it from `navigator.credentials.get`
/// (`HubPort::verify_passkey`): each base64url, no padding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PasskeyVerdict {
    pub ok: bool,
    #[serde(default)]
    pub why: String,
}

/// An account's devices (`HubPort::device_trust`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

/// How a push goes (`HubPort::push_with`). The default is what
/// `HubPort::push` does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PushOptions {
    /// How long a push service keeps it for a device that's away (its
    /// `TTL`, at most a day): `None` is the hub's own, an hour. Cloud's
    /// approvals keep five minutes.
    #[serde(default, with = "secs")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<u64>"))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Account {
    pub id: String,
    /// Its sign-in login (GitHub's), or "".
    pub login: String,
    /// The name it chose, else its GitHub login, else "".
    pub name: String,
}

/// A principal the hub still knows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Resolved {
    pub principal: Principal,
    pub account: Account,
    /// The machine, for a daemon.
    pub daemon: Option<Daemon>,
}

/// A team, as its latest signed roster says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Member {
    pub account: String,
    pub role: TeamRole,
    /// What the roster calls them.
    pub name: String,
}

/// A joined machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Meter {
    pub account: String,
    pub relay_bytes: u64,
    pub sandbox_minutes: u64,
}

/// A page of the hub's change feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(transform = crate::open_enum))]
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
