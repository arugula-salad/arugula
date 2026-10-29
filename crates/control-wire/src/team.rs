//! What a daemon fetches from control to keep checking who may reach it: the
//! account's certificates, its team's rosters, the people and teams sessions
//! were shared with. And what it tells control: who gets in.

use std::collections::BTreeMap;

use arugula_e2e::{
    Cert, Revocation, Trust,
    team::{AccountCerts, Move, Roster, TeamPin},
};
use serde::{Deserialize, Serialize};

/// `GET /api/daemon/trust`: the account's certificates, to evaluate against
/// the root the daemon pinned.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TrustAnswer {
    /// Not read by daemons: they have the root they pinned.
    #[serde(default)]
    pub trust: Option<Trust>,
    pub certs: Vec<Cert>,
    pub revocations: Vec<Revocation>,
    /// Its last move (#100), for the daemon to check and take.
    #[serde(default)]
    pub moved: Option<Move>,
    /// The Labs flags its account and team set (#347). Left out by a
    /// control from before them; a daemon then keeps what it had.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flags: Option<Flags>,
}

/// Labs flags only a machine sets for itself (#347): control refuses to
/// store them, and a daemon ignores them if sent. The daemon's tests check
/// this is `arugula_proto::flags`'s `local_only` rows, and `guest-ssh`,
/// which is a sharing setting now (native #205) that control never writes:
/// still refused, for daemons from before that.
pub const LOCAL_ONLY_FLAGS: &[&str] = &["guest-ssh", "agent-delegation"];

/// Labs flags that were renamed (aru-117): the old name, then the new.
/// Control stores and pushes the new name, takes the old one from an older
/// page, and pushes it beside the new for a daemon from before the rename.
/// The daemon's tests check this is `arugula_proto::flags::ALIASES`.
pub const FLAG_ALIASES: &[(&str, &str)] = &[
    ("vms", "sandbox-providers"),
    ("workspaces", "chant-workspaces"),
    ("forges", "additional-forges"),
    ("agents", "agent-delegation"),
];

/// What a daemon says it understands (`?features=`) once its owner has
/// asked for Developer settings, or control sends it flags (#347): control's
/// page offers the account's Labs flags only to people with such a machine.
pub const DEV_SETTINGS: &str = "dev-settings";

/// Labs flags control sends a machine (#347): its account's, and its
/// team's if it's a team's machine, by flag name. On or off; a flag
/// neither names follows the machine's own default. The daemon's own
/// `flags.json` wins over both, and account over team.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Flags {
    #[serde(default)]
    pub account: BTreeMap<String, bool>,
    #[serde(default)]
    pub team: BTreeMap<String, bool>,
}

/// `GET /api/daemon/team`: a team machine's team, rosters after the version
/// it has, every member's certificates, and whether it's locked.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TeamAnswer {
    #[serde(default)]
    pub team: Option<TeamPin>,
    pub locked: bool,
    pub rosters: Vec<Roster>,
    pub certs: AccountCerts,
    #[serde(default)]
    pub names: BTreeMap<String, String>,
}

/// What control answers a machine that isn't in a team: `{"team": null}`.
/// A daemon only asks once it has pinned one, and reads this as it always
/// has, as an answer [`TeamAnswer`] can't parse.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TeamNone {
    pub team: Option<TeamPin>,
}

/// `GET /api/daemon/teams`: one of these per team, by id, for each team a
/// session was shared with whose owner is in it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SharedTeamAnswer {
    #[serde(default)]
    pub team: Option<TeamPin>,
    #[serde(default)]
    pub name: String,
    pub locked: bool,
    pub rosters: Vec<Roster>,
    pub certs: AccountCerts,
    #[serde(default)]
    pub names: BTreeMap<String, String>,
}

/// One account's certificates, from `GET /api/daemon/peers` (by account),
/// or as saved in `control.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PeerCerts {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub certs: Vec<Cert>,
    #[serde(default)]
    pub revocations: Vec<Revocation>,
}

/// `POST /api/daemon/access`: which accounts a daemon lets in. The daemon
/// decides for itself; control routes only those it would anyway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AccessList {
    pub accounts: Vec<String>,
    /// Until when (ms) read-only links may reach it through the relay.
    pub links_until: Option<u64>,
    /// #399: accounts whose daemons may reach it for its agents only (a
    /// teammate's machine, for the team's agent catalog). Routed for their
    /// daemons, never for the people: their browsers don't list it. An
    /// older daemon sends none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<String>,
}

/// What a daemon says it understands, comma-separated (older ones send
/// none): `GET /api/daemon/trust?features=`.
#[derive(Debug, Clone, Default, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Features {
    #[serde(default)]
    pub features: String,
}

impl Features {
    pub fn path(features: &str) -> String {
        format!("{}?features={features}", crate::TRUST)
    }
}

/// `GET /api/daemon/team?since=&features=`.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TeamQuery {
    /// The roster version the daemon has.
    #[serde(default)]
    pub since: u64,
    #[serde(default)]
    pub features: String,
}

impl TeamQuery {
    pub fn path(since: u64, features: &str) -> String {
        format!("{}?since={since}&features={features}", crate::TEAM)
    }
}

/// `GET /api/daemon/teams?ids=&features=`.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TeamsQuery {
    /// Team ids, comma-separated.
    pub ids: String,
    #[serde(default)]
    pub features: String,
}

impl TeamsQuery {
    pub fn path(ids: &[&str], features: &str) -> String {
        format!("{}?ids={}&features={features}", crate::TEAMS, ids.join(","))
    }
}

/// `GET /api/daemon/peers?accounts=`.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PeersQuery {
    /// Account ids, comma-separated.
    pub accounts: String,
}

impl PeersQuery {
    pub fn path(accounts: &[String]) -> String {
        format!("{}?accounts={}", crate::PEERS, accounts.join(","))
    }
}
