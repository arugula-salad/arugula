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
//! - On its first boot it reads what to set up from [`SPEC`]
//!   ([`SetupPlan`]), runs that in a pane, and says how it goes at
//!   [`SETUP`] ([`SetupReport`], M183).

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
/// A hosted machine's ports that are public now (M187): `GET`, signed as
/// the machine, answered [`PublicPorts`]. The machine serves a public
/// preview only for a port and id listed here.
pub const HOSTED_PREVIEWS: &str = "/api/hosted/previews";

/// What [`HOSTED_PREVIEWS`] answers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicPorts {
    pub ports: Vec<PublicPort>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicPort {
    pub port: u16,
    /// The preview's id, the label of its host.
    pub preview: String,
}

/// What a hosted machine sets up on its first boot (`GET`, signed as the
/// machine), answered [`SetupPlan`] (M183).
pub const SPEC: &str = "/api/hosted/spec";
/// How its setup goes (`POST`, signed as the machine), [`SetupReport`],
/// answered `{}`.
pub const SETUP: &str = "/api/hosted/setup";

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
    /// What the box is for: [`DEPLOY`] for one deploy's run (M219), which
    /// nobody is routed to; `None` for a people's workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
}

/// A box made for one deploy's run (M219): it takes that run only, lets
/// nobody in, and is deleted after.
pub const DEPLOY: &str = "deploy";

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
    /// What the box is for (M219): [`DEPLOY`], or `None` for a people's
    /// workspace. A box that hears `deploy` stays one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
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

/// What a hosted machine sets up on its first boot, from its workspace's
/// spec (M183): its repos with what to clone each with, its packages, its
/// setup script, and the env its spec takes from its secrets.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct SetupPlan {
    /// The hosted workspace (`hw_…`).
    pub workspace: String,
    /// The spec's hash: what the box is now built from.
    pub spec_hash: String,
    #[serde(default)]
    pub repos: Vec<SetupRepo>,
    #[serde(default)]
    pub packages: Vec<String>,
    /// A shell script, run after both.
    #[serde(default)]
    pub setup: String,
    /// The spec's env names that have a value here, and those values.
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    /// The spec's env names with no value here: not set, or held by the
    /// provider's broker.
    #[serde(default)]
    pub unset: Vec<String>,
}

/// A repo to clone, and how.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct SetupRepo {
    /// An HTTPS clone URL, with no credentials in it.
    pub url: String,
    /// `github_app` (a read-only installation token for this repo alone,
    /// minted for this clone: a GitHub repo linked to the project),
    /// `secret` (the workspace's `GIT_TOKEN`), or `none`.
    pub via: String,
    /// What to clone with, over HTTP basic auth.
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

/// Where a hosted machine's setup is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStatus {
    SettingUp,
    Ready,
    Failed,
}

impl SetupStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SetupStatus::SettingUp => "setting_up",
            SetupStatus::Ready => "ready",
            SetupStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "setting_up" => Some(SetupStatus::SettingUp),
            "ready" => Some(SetupStatus::Ready),
            "failed" => Some(SetupStatus::Failed),
            _ => None,
        }
    }
}

/// `POST /api/hosted/setup`: how the setup goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupReport {
    pub status: SetupStatus,
    /// The step it's on, or the one that failed: `clone app`, `packages`,
    /// `setup script`.
    #[serde(default)]
    pub step: String,
    /// Why a failed setup stopped, as a fixed code ([`SETUP_REASONS`]):
    /// never the pane's text, which stays end to end ("Two planes, two
    /// trust rules"): a client reads it from the box.
    #[serde(default)]
    pub reason: String,
    /// The pane it runs in, on the machine.
    #[serde(default)]
    pub pane: Option<u64>,
    /// The spec's hash it runs.
    #[serde(default)]
    pub spec_hash: String,
}

/// Why a setup failed: `exit` (its step exited non-zero), `pane_closed`
/// (its pane went before it finished), `start` (it couldn't start: no
/// home to clone into, or no pane).
pub const SETUP_REASONS: &[&str] = &["exit", "pane_closed", "start"];

// Never the sandbox id or the ticket: each is a bearer secret while its
// enrollment lasts. Never a clone's password or a secret's value.

impl std::fmt::Debug for SetupPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetupPlan")
            .field("workspace", &self.workspace)
            .field("spec_hash", &self.spec_hash)
            .field("repos", &self.repos)
            .field("packages", &self.packages)
            .field("setup", &self.setup.len())
            .field("env", &self.env.keys().collect::<Vec<_>>())
            .field("unset", &self.unset)
            .finish()
    }
}

impl std::fmt::Debug for SetupRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetupRepo")
            .field("url", &self.url)
            .field("via", &self.via)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "…"))
            .finish()
    }
}

impl std::fmt::Debug for EnrollAsk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnrollAsk")
            .field("project", &self.project)
            .field("owner", &self.owner)
            .field("workspace", &self.workspace)
            .field("name", &self.name)
            .field("sandbox", &self.sandbox.as_ref().map(|_| "…"))
            .field("purpose", &self.purpose)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A report round-trips with its status in snake case, and a plan's
    /// Debug never shows a password or a secret's value.
    #[test]
    fn setup_wire() {
        let r = SetupReport {
            status: SetupStatus::SettingUp,
            step: "clone app".into(),
            reason: String::new(),
            pane: Some(3),
            spec_hash: "h".into(),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["status"], "setting_up");
        assert_eq!(serde_json::from_value::<SetupReport>(v).unwrap(), r);
        for s in [SetupStatus::SettingUp, SetupStatus::Ready, SetupStatus::Failed] {
            assert_eq!(SetupStatus::parse(s.as_str()), Some(s));
        }
        let plan = SetupPlan {
            repos: vec![SetupRepo {
                url: "https://git.example/app".into(),
                via: "secret".into(),
                username: Some("x-access-token".into()),
                password: Some("hunter2".into()),
            }],
            env: [("NPM_TOKEN".to_owned(), "s3cret".to_owned())].into(),
            ..Default::default()
        };
        let d = format!("{plan:?}");
        assert!(d.contains("NPM_TOKEN") && !d.contains("hunter2") && !d.contains("s3cret"), "{d}");
    }
}
