//! What daemons and Arugula control say to each other to enrol and to
//! route: joining, the account's and team's certificates, who gets in, how
//! the relay is dialled, and what `/control.json` says about control.
//!
//! One type per message, used by both sides, so a field renamed on one side
//! doesn't compile on the other. It isn't `arugula-proto`: that is what
//! browsers and clients speak with a daemon (and generates the web client's
//! types), and control shouldn't depend on it. This crate holds only these
//! messages and the certificates in them (`arugula-e2e`).
//!
//! Two rules for every type here:
//! - **The bytes on the wire don't change.** A field that was `null` stays
//!   `null` (no `skip_serializing_if`); one that was left out when empty
//!   still is. The tests round-trip messages recorded from both sides.
//! - **Be as lenient as either side was.** Deployed daemons and control
//!   don't upgrade together, so a field either one read with
//!   `#[serde(default)]` keeps it, and nothing here is `deny_unknown_fields`.
//!   A field neither side needs to find is not made required.

use serde::{Deserialize, Serialize};

pub mod chat;
pub mod forge;
pub mod join;
pub mod maintenance;
pub mod projects;
pub mod push;
pub mod team;
pub mod telemetry;

pub use join::{CliJoinRequest, JoinPoll, JoinProof, JoinRequest, JoinStarted, JoinTeam, PollQuery};
pub use push::{PushAnswer, PushRequest, PushSubs};
pub use team::{
    AccessList, DEV_SETTINGS, Features, Flags, LOCAL_ONLY_FLAGS, PeerCerts, PeersQuery, SharedTeamAnswer, TeamAnswer,
    TeamNone, TeamQuery, TeamsQuery, TrustAnswer,
};

/// What control says about itself, to browsers, the CLI and daemons.
pub const CONTROL_JSON: &str = "/control.json";
/// A daemon asks to join (`POST`), [`JoinRequest`], answered [`JoinStarted`].
pub const JOIN: &str = "/api/join";
/// The route a daemon polls for its approval (`GET /api/join/{code}`),
/// answered [`JoinPoll`].
pub const JOIN_POLL: &str = "/api/join/{code}";
/// The account's certificates (`GET`), [`TrustAnswer`].
pub const TRUST: &str = "/api/daemon/trust";
/// A daemon leaves (`POST`, no body).
pub const LEAVE: &str = "/api/daemon/leave";
/// A team machine's team (`GET`), [`TeamAnswer`].
pub const TEAM: &str = "/api/daemon/team";
/// People sessions were shared with (`GET`), [`PeerCerts`] by account.
pub const PEERS: &str = "/api/daemon/peers";
/// Teams sessions were shared with (`GET`), [`SharedTeamAnswer`] by team.
pub const TEAMS: &str = "/api/daemon/teams";
/// Who gets in (`POST`), [`AccessList`].
pub const ACCESS: &str = "/api/daemon/access";
/// The subscriptions of the people a daemon serves (`GET`), [`PushSubs`].
pub const PUSH_SUBS: &str = "/api/daemon/push-subs";
/// A daemon has control relay an encrypted notification (`POST`),
/// [`PushRequest`], answered [`PushAnswer`].
pub const PUSH: &str = "/api/daemon/push";
/// A hosted sandbox's last session closed (`POST`, no body), answered
/// [`Ack`].
pub const SANDBOX_DONE: &str = "/api/daemon/sandbox-done";
/// The daemon's WebSocket to the relay (`GET`), [`DialQuery`].
pub const RELAY_DIAL: &str = "/api/relay/dial";

/// `GET /control.json`. Read leniently: a daemon needs only `daemon_auth`
/// and `guest_ssh` of it, and an older control is without some of the rest.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControlInfo {
    #[serde(default)]
    pub control: bool,
    /// The URL this answer was asked at.
    #[serde(default)]
    pub url: String,
    /// Where control is now (#507), when it answers at more than one URL:
    /// a daemon on another of them may move here.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub primary: String,
    /// Every URL control answers at, `primary` first (#507).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<String>,
    #[serde(default)]
    pub github: bool,
    /// Passkeys need a domain name: WebAuthn refuses IP addresses.
    #[serde(default)]
    pub passkeys: bool,
    /// The Web Push application server key.
    #[serde(default)]
    pub vapid: String,
    #[serde(default)]
    pub github_app: Option<String>,
    /// How daemons sign their requests here: 2 takes body hashes and nonces.
    #[serde(default)]
    pub daemon_auth: u64,
    /// The CLI joins with a code and signs its requests (M49).
    #[serde(default)]
    pub cli_join: u64,
    /// Control passes a join secret's MAC from the approving device to the
    /// machine (#760), so a machine's link may carry a secret.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub join_secret: u64,
    /// Control passes an approving device's MAC under a waiting device's
    /// code to it (#761), so a waiting browser shows a code to scan or type
    /// rather than a fingerprint to compare.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub device_secret: u64,
    /// The ssh jump host for guests of daemons behind NAT (M65).
    #[serde(default)]
    pub guest_ssh: Option<GuestJump>,
    /// Blocks get origins of their own under this domain (M50):
    /// `https://b-<site key>.<block_domain>`. `None`: control doesn't serve
    /// blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_domain: Option<String>,
    /// Control keeps encrypted Doc snapshots and serves them to anyone with
    /// the link (#1064): the version of that API. `None`: it doesn't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_links: Option<u8>,
    /// Where those are opened, without a trailing slash: a link is
    /// `<doc_viewer>/<id>#k=<key>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_viewer: Option<String>,
}

/// Just the signature version from `/control.json`, for a daemon deciding
/// how to sign. Read apart from [`ControlInfo`] so that a field it can't
/// take never quietly drops the daemon back to v1 signatures.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct ControlAuth {
    #[serde(default)]
    pub daemon_auth: u64,
}

/// Just whether control takes the CLI as a device, from `/control.json`
/// (M49), read apart from [`ControlInfo`] so a field the CLI never needs
/// can't make it think control is older than it is. `None`: control doesn't
/// say (it is older than the CLI).
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct ControlCliJoin {
    #[serde(default)]
    pub cli_join: Option<u64>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// Just whether control passes a join secret's MAC on (#760), from
/// `/control.json`, read apart from [`ControlInfo`] like the others. A
/// machine or the CLI puts a secret in its link only when it does.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct ControlJoinSecret {
    #[serde(default)]
    pub join_secret: u64,
}

/// Just where control is from `/control.json` (#507), read apart from
/// [`ControlInfo`] like the others.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ControlWhere {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub primary: String,
    #[serde(default)]
    pub urls: Vec<String>,
    /// Control serves block origins (M50): blocks show in its page.
    #[serde(default)]
    pub block_domain: Option<String>,
}

/// Just what shared Docs need from `/control.json` (#1064), read apart from
/// [`ControlInfo`] like the others.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ControlDocs {
    #[serde(default)]
    pub doc_links: Option<u8>,
    #[serde(default)]
    pub doc_viewer: Option<String>,
}

/// Just the jump host from `/control.json`, read apart from
/// [`ControlInfo`] so an unrelated malformed field doesn't lose it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ControlJump {
    #[serde(default)]
    pub guest_ssh: Option<GuestJump>,
}

/// Control's ssh jump host, for daemons making invites.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestJump {
    pub host: String,
    pub port: u16,
    pub known_hosts: String,
    #[serde(default)]
    pub fingerprint: String,
}

/// An answer with nothing in it: `{}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {}

/// Control's 410 Gone (#330): this key was removed from its account.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RemovedAnswer {
    /// Control's words.
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub removed: Option<RemovedAt>,
}

/// When a key was removed and, to the key's own holder only, by which
/// device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemovedAt {
    /// When it was removed (ms since the epoch).
    pub at: u64,
    /// The removing device's name; left out when control isn't telling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

/// `GET /api/relay/dial?urls=`: the daemon's direct URLs, for the directory.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DialQuery {
    /// JSON list of the daemon's direct URLs.
    #[serde(default)]
    pub urls: Option<String>,
}

impl DialQuery {
    /// What a daemon sends for its direct URLs.
    pub fn new(urls: &[String]) -> Self {
        // A list of strings always serializes.
        Self { urls: serde_json::to_string(urls).ok() }
    }

    /// The URLs, if it sent a list control can read.
    pub fn direct_urls(&self) -> Option<Vec<String>> {
        self.urls.as_deref().and_then(|u| serde_json::from_str(u).ok())
    }
}

#[cfg(test)]
mod tests;
