//! Enrolment: a daemon asks to join, shows the code, and polls until a
//! signed-in device approves it, or a signed-in person adds it (aru-141);
//! or it joins at once with a join token a signed-in person made.

use arugula_e2e::{Cert, Revocation, Trust};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};

/// `POST /api/join`. The CLI joins the same way, with only `cert` and
/// `proof`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct JoinRequest {
    pub cert: Cert,
    #[serde(default)]
    pub urls: Vec<String>,
    /// A machine that belongs to a team (M19), not a person.
    #[serde(default)]
    pub team: Option<String>,
    /// What the daemon understands, comma-separated (older ones send none).
    #[serde(default)]
    pub features: String,
    /// Its signature with the key it asks with (0.17 and newer).
    #[serde(default)]
    pub proof: Option<JoinProof>,
}

/// What the CLI sends to `/api/join`: a [`JoinRequest`] with only `cert` and
/// `proof`, as the CLI always has (control reads it as a `JoinRequest`, the
/// rest defaulted).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CliJoinRequest {
    pub cert: Cert,
    pub proof: JoinProof,
    /// `join-secret` when its link carries a secret (#760); left out
    /// otherwise, as before.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub features: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct JoinProof {
    pub ms: u64,
    pub sig: String,
}

/// Control's answer to a join: the code to show, and a token to poll with.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct JoinStarted {
    pub code: String,
    pub poll: String,
    /// The CLI took ten minutes when control left it out.
    #[serde(default = "ten_minutes")]
    pub expires_in_secs: u64,
    /// The team `--team` named, by name.
    #[serde(default)]
    pub team_name: Option<String>,
}

fn ten_minutes() -> u64 {
    600
}

/// `GET /api/join/{code}?poll=`: where a join stands. Waiting is just
/// `approved: false`, turned down adds `rejected`, and approved is the rest;
/// each is serialized as control always has (see [`JoinPoll::serialize`]).
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct JoinPoll {
    pub approved: bool,
    /// Turned down, on this device (#100).
    #[serde(default)]
    pub rejected: Option<String>,
    #[serde(default)]
    pub cert: Option<Cert>,
    #[serde(default)]
    pub trust: Option<Trust>,
    #[serde(default)]
    pub team: Option<JoinTeam>,
    #[serde(default)]
    pub certs: Vec<Cert>,
    #[serde(default)]
    pub revocations: Vec<Revocation>,
    /// The approving device's MAC under the join secret (#760,
    /// `arugula_e2e::cert::join_mac`), hex; only when it sent one.
    #[serde(default)]
    pub mac: Option<String>,
    /// Added by a signed-in person with no device certificate (aru-141):
    /// where the machine went, on control's word. Then `cert`, `trust`
    /// and the rest are absent, and the daemon pins no root.
    #[serde(default)]
    pub joined: Option<Joined>,
}

/// The token the daemon polls with: `GET /api/join/{code}?poll=`.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PollQuery {
    pub poll: String,
}

impl JoinPoll {
    /// The path to poll: [`crate::JOIN_POLL`] for `code`, with its token.
    pub fn path(code: &str, poll: &str) -> String {
        format!("{}?poll={poll}", crate::JOIN_POLL.replace("{code}", code))
    }

    /// Nobody has approved or turned it down yet.
    pub fn waiting() -> Self {
        Self {
            approved: false,
            rejected: None,
            cert: None,
            trust: None,
            team: None,
            certs: vec![],
            revocations: vec![],
            mac: None,
            joined: None,
        }
    }

    /// Added by a signed-in person (aru-141): `joined`, and nothing else.
    pub fn added(joined: Joined) -> Self {
        Self { approved: true, joined: Some(joined), ..Self::waiting() }
    }

    /// Turned down on `device`.
    pub fn turned_down(device: String) -> Self {
        Self { rejected: Some(device), ..Self::waiting() }
    }
}

impl Serialize for JoinPoll {
    /// Control has sent `{approved: false}` while waiting, that and
    /// `rejected` once turned down, and everything (a `null` team and trust
    /// included) once approved, with `mac` only when there is one.
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if !self.approved {
            let n = 1 + usize::from(self.rejected.is_some());
            let mut o = s.serialize_struct("JoinPoll", n)?;
            o.serialize_field("approved", &self.approved)?;
            if let Some(on) = &self.rejected {
                o.serialize_field("rejected", on)?;
            }
            return o.end();
        }
        // Added by a person (aru-141): `approved` and `joined` alone.
        if let Some(j) = &self.joined {
            let mut o = s.serialize_struct("JoinPoll", 2)?;
            o.serialize_field("approved", &self.approved)?;
            o.serialize_field("joined", j)?;
            return o.end();
        }
        let mut o = s.serialize_struct("JoinPoll", 6 + usize::from(self.mac.is_some()))?;
        o.serialize_field("approved", &self.approved)?;
        o.serialize_field("cert", &self.cert)?;
        o.serialize_field("trust", &self.trust)?;
        o.serialize_field("certs", &self.certs)?;
        o.serialize_field("revocations", &self.revocations)?;
        o.serialize_field("team", &self.team)?;
        // Only with a secret's MAC (#760): without one, as before.
        if let Some(mac) = &self.mac {
            o.serialize_field("mac", mac)?;
        }
        o.end()
    }
}

/// The team an approved machine joins.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct JoinTeam {
    pub team: String,
    pub founder: String,
    pub founder_root: String,
    #[serde(default)]
    pub name: String,
    /// The approving device's signature over the team pin's join body
    /// (`TeamPin::join_body`).
    #[serde(default)]
    pub sig: Option<String>,
}

/// `POST /api/join-tokens`, by a signed-in person: a join token for their
/// account (`{}`), or for a team (aru-141). The daemon doesn't make them;
/// the CLI, project settings and scripts do.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct JoinTokenRequest {
    /// The team the machine joins; none for the person's own account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
}

/// A join token, made: `ajt_…`, good once, until `expires`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct JoinTokenMade {
    pub token: String,
    /// When it stops working (seconds since the epoch).
    pub expires: u64,
}

/// `POST /api/join/token`, with no session (aru-141): a daemon joins with
/// a join token, at once, answered [`Joined`]. `cert` and `proof` are as
/// for [`JoinRequest`]: the key the machine signs its calls with, and
/// that it holds it. A used, expired or unknown token is a 410 with
/// `{"error"}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TokenJoinRequest {
    pub cert: Cert,
    pub proof: JoinProof,
    pub join_token: String,
    #[serde(default)]
    pub urls: Vec<String>,
    /// What the daemon understands, comma-separated.
    #[serde(default)]
    pub features: String,
}

/// Where a machine went, on control's word (aru-141): a join token's
/// answer, and a person's one-click approval in [`JoinPoll`]. The daemon
/// pins no root: control decides who reaches it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Joined {
    /// The account it's in: its owner.
    pub account: String,
    /// That account's login.
    pub login: String,
    /// The machine's id: its key's.
    pub machine: String,
    /// The team it joined, when it's a team's.
    #[serde(default)]
    pub team: Option<JoinedTeam>,
}

/// The team a [`Joined`] machine is in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct JoinedTeam {
    pub id: String,
    pub name: String,
}
