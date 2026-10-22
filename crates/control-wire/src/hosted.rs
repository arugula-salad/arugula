//! Hosted machines (M178): how a box made by the platform enrolls into its
//! project under the project's delegation, and what it gets back.
//!
//! - The platform records a one-time enrollment when it creates a box
//!   (`HubPort::hosted_enrollment`, an [`EnrollAsk`]), bound to the
//!   project and the `hw_` workspace, by a ticket the hub makes or by the
//!   box's sandbox id.
//! - The box sends [`ENROLL`] once ([`EnrollRequest`]), with that ticket
//!   or sandbox id and its new public keys, and gets its certificate and
//!   what to pin ([`Enrollment`]).
//! - From then on it signs its requests as any machine does, and refreshes
//!   what it pinned from [`HOSTED_TRUST`] ([`Enrollment`] again).
//! - When someone opens a hosted machine whose daemon isn't connected, the
//!   hub asks the platform to wake its box (M182), answered [`WakeAnswer`].

use arugula_e2e::{
    Cert, Revocation, Trust,
    hosted::{Delegation, DelegationRevoke, HostedCert},
    team::{AccountCerts, Roster, TeamPin},
};
use serde::{Deserialize, Serialize};

/// A box enrolls (`POST`), [`EnrollRequest`], answered [`Enrollment`]. Not
/// signed: the ticket or sandbox id and the key's proof stand in.
pub const ENROLL: &str = "/api/hosted/enroll";
/// A hosted machine's project, as it is now (`GET`, signed as the machine),
/// answered [`Enrollment`].
pub const HOSTED_TRUST: &str = "/api/hosted/trust";
/// An agent turn on a hosted box (M185): `POST` `{"agent": "claude" |
/// "codex"}`, signed as the machine, answered `{"turn", "pays"}` (whose
/// inference key: `project` or `platform`), or refused with a `reason`
/// (`no_key`, `cap`) and a sentence for the person. Its end is
/// `POST {HOSTED_TURNS}/{turn}/end`.
pub const HOSTED_TURNS: &str = "/api/hosted/turns";

/// How long a ticket the hub makes lasts: a box made from a template is
/// up in seconds, and one that isn't enrolled by then is made again.
pub const TICKET_MS: u64 = 15 * 60 * 1000;
/// How long an enrollment by sandbox id lasts: the box enrolls as it
/// first sees its sandbox id, a moment after it's made.
pub const SANDBOX_MS: u64 = 2 * 60 * 1000;

/// What the platform asks of the hub as it creates a box: a one-time
/// enrollment into `project` for workspace `workspace`.
#[derive(Clone, Serialize, Deserialize)]
pub struct EnrollAsk {
    pub project: String,
    /// `team:<id>` or `account:<id>`: who the project belongs to, as the
    /// platform holds it. The hub enrolls only under a delegation signed
    /// for this owner.
    pub owner: String,
    /// The hosted workspace (`hw_…`).
    pub workspace: String,
    /// What the machine is called.
    pub name: String,
    /// The box's sandbox id, when it enrolls by that (a box started from a
    /// running snapshot can't read the env it was made with). `None`: the
    /// hub makes a ticket for the box's env (`ARUGULA_HOSTED_TICKET`).
    #[serde(default)]
    pub sandbox: Option<String>,
}

/// What the hub answers an [`EnrollAsk`].
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnrollGiven {
    /// Recorded: until `expires` (ms), once. `ticket` is the one to put in
    /// the box's env when it doesn't enroll by sandbox id.
    Ready {
        #[serde(default)]
        ticket: Option<String>,
        expires: u64,
    },
    /// No owner's device has delegated this project (or every delegation
    /// was revoked): an owner signs one from a device first.
    NoDelegation,
    /// That sandbox id has an enrollment already: one per box, ever.
    Taken,
}

/// A workspace whose box enrolled, and the hosted machine it is
/// (`HubPort::hosted_enrolled`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enrolled {
    pub workspace: String,
    /// The machine's device id; empty when `taken` removed it.
    pub machine: String,
    /// Someone tried the box's enrollment again: fail closed, the machine
    /// that used it was removed, and the platform deletes the box.
    #[serde(default)]
    pub taken: bool,
}

/// `POST /api/hosted/enroll`, once per box.
#[derive(Clone, Serialize, Deserialize)]
pub struct EnrollRequest {
    /// The ticket from `ARUGULA_HOSTED_TICKET`; or
    #[serde(default)]
    pub ticket: Option<String>,
    /// the box's sandbox id (`/run/e2b/.E2B_SANDBOX_ID`). Exactly one.
    #[serde(default)]
    pub sandbox: Option<String>,
    /// The machine's new public keys, hex.
    pub noise: String,
    pub sign: String,
    /// When it signed `proof` (ms).
    pub at: u64,
    /// Its signature with `sign` over
    /// `arugula_e2e::hosted::enroll_proof_body` (the SHA-256 of the ticket
    /// or sandbox id, its keys, `at`).
    pub proof: String,
}

/// What a hosted machine pins and serves by: its certificate, the
/// delegation it's under, and who's in its project, to check people's
/// devices against as a team machine does.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Enrollment {
    /// Control's URL, to dial.
    pub url: String,
    pub cert: HostedCert,
    pub delegation: Delegation,
    /// Revocations of it: the machine stays, but nothing more enrolls.
    #[serde(default)]
    pub revocations: Vec<DelegationRevoke>,
    /// A team's project: the team to pin, its roster now, and its
    /// members' certificates.
    #[serde(default)]
    pub team: Option<TeamPin>,
    #[serde(default)]
    pub roster: Option<Roster>,
    #[serde(default)]
    pub certs: AccountCerts,
    /// An account's project: its root, certificates and revocations.
    #[serde(default)]
    pub trust: Option<Trust>,
    #[serde(default)]
    pub account_certs: Vec<Cert>,
    #[serde(default)]
    pub account_revocations: Vec<Revocation>,
}

/// What a person's page signs and sends: a new delegation
/// (`POST /api/hosted/delegations`), or a revocation of one
/// (`POST /api/hosted/delegations/revoke`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationPost {
    pub delegation: Delegation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevokePost {
    pub revoke: DelegationRevoke,
}

/// What the platform answers the hub's ask to wake a hosted workspace's
/// box (`PlatformPort::hosted_wake`, M182): someone opened its machine and
/// its daemon isn't connected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WakeAnswer {
    /// Its box runs (it was resumed, or it was running): its daemon dials
    /// in again in a moment.
    Awake,
    /// It didn't wake: why, for the person who opened it.
    Failed { code: WakeCode, why: String },
}

/// Why a box didn't wake. The page offers rebuild for each but
/// `starting`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeCode {
    /// The box is gone at the provider, or the workspace was deleted.
    Gone,
    /// The provider said no.
    Refused,
    /// The provider didn't answer in time; the platform keeps trying.
    Unavailable,
    /// The workspace's box is being made or made again.
    Starting,
    /// The workspace failed earlier and waits for a rebuild.
    Failed,
    /// The box woke, but its daemon didn't dial in (the hub's word).
    Silent,
}

impl WakeCode {
    pub fn as_str(self) -> &'static str {
        match self {
            WakeCode::Gone => "gone",
            WakeCode::Refused => "refused",
            WakeCode::Unavailable => "unavailable",
            WakeCode::Starting => "starting",
            WakeCode::Failed => "failed",
            WakeCode::Silent => "silent",
        }
    }
}

// Never the sandbox id or the ticket: each is a bearer secret while its
// enrollment lasts.

impl std::fmt::Debug for EnrollAsk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnrollAsk")
            .field("project", &self.project)
            .field("owner", &self.owner)
            .field("workspace", &self.workspace)
            .field("name", &self.name)
            .field("sandbox", &self.sandbox.as_ref().map(|_| "…"))
            .finish()
    }
}

impl std::fmt::Debug for EnrollGiven {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnrollGiven::Ready { ticket, expires } => f
                .debug_struct("Ready")
                .field("ticket", &ticket.as_ref().map(|_| "…"))
                .field("expires", expires)
                .finish(),
            EnrollGiven::NoDelegation => f.write_str("NoDelegation"),
            EnrollGiven::Taken => f.write_str("Taken"),
        }
    }
}

impl std::fmt::Debug for EnrollRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnrollRequest")
            .field("ticket", &self.ticket.as_ref().map(|_| "…"))
            .field("sandbox", &self.sandbox.as_ref().map(|_| "…"))
            .field("noise", &self.noise)
            .field("sign", &self.sign)
            .field("at", &self.at)
            .finish()
    }
}
