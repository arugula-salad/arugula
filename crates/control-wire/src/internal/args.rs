//! Each internal method's arguments: one JSON object per call, named for
//! what it carries. A method whose arguments are a wire type of their own
//! (`hosted_enrollment`'s `EnrollAsk`, `box_post`'s `BoxPost`) has none
//! here. A field added later is `#[serde(default)]`, since the caller may
//! be a release behind.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Deleted, GithubUser, HostedMachine, PasskeyAssertion, Principal, PushOptions, UserToken};
use crate::{
    forge::{DaemonOp, Poke},
    hosted::SetupReport,
    run_logs::Upload,
    transcripts::TranscriptPost,
    work::{PushAsk, Report, SecretsAsk, SessionIssuesAsk, SessionReport},
};

#[derive(Serialize, Deserialize)]
pub struct ResolveArgs {
    pub principal: Principal,
}

#[derive(Serialize, Deserialize)]
pub struct IdsArgs {
    pub ids: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct AccountArgs {
    pub account: String,
}

#[derive(Serialize, Deserialize)]
pub struct WordArgs {
    pub word: String,
}

#[derive(Serialize, Deserialize)]
pub struct ReachArgs {
    pub account: String,
    pub machines: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct PushArgs {
    pub account: String,
    pub message: Value,
    #[serde(default)]
    pub options: PushOptions,
}

#[derive(Serialize, Deserialize)]
pub struct OwnersArgs {
    pub owners: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct MetersArgs {
    pub accounts: Vec<String>,
    pub since: u64,
    pub until: u64,
}

#[derive(Serialize, Deserialize)]
pub struct ChangesArgs {
    pub after: u64,
    pub limit: u32,
}

#[derive(Serialize, Deserialize)]
pub struct DriveArgs {
    pub daemon: String,
    pub account: String,
}

#[derive(Serialize, Deserialize)]
pub struct TeamArgs {
    pub team: String,
}

#[derive(Serialize, Deserialize)]
pub struct BlockersArgs {
    pub account: String,
    pub start: bool,
}

#[derive(Serialize, Deserialize)]
pub struct DeletedArgs {
    pub deleted: Deleted,
}

#[derive(Serialize, Deserialize, Default)]
pub struct NoArgs {}

#[derive(Serialize, Deserialize)]
pub struct PokesArgs {
    pub pokes: Vec<Poke>,
}

#[derive(Serialize, Deserialize)]
pub struct StandingArgs {
    pub account: String,
    pub github: Option<GithubUser>,
    pub repos: Vec<String>,
    pub fresh: bool,
}

#[derive(Serialize, Deserialize)]
pub struct TokenArgs {
    pub account: String,
    pub github: Option<GithubUser>,
    pub repo: String,
}

#[derive(Serialize, Deserialize)]
pub struct GithubIdArgs {
    pub id: u64,
}

#[derive(Serialize, Deserialize)]
pub struct SendAuthArgs {
    pub account: String,
    pub draft: String,
    pub back: String,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspacesArgs {
    pub workspaces: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct DelegationArgs {
    pub project: String,
    pub owner: String,
}

#[derive(Serialize, Deserialize)]
pub struct SignedArgs {
    pub owner: String,
    pub device: String,
    pub body: String,
    pub sig: String,
}

#[derive(Serialize, Deserialize)]
pub struct OpArgs {
    pub daemon: String,
    pub op: DaemonOp,
}

/// The user token crosses inside the signed call's body, never logged.
#[derive(Serialize, Deserialize)]
pub struct SendArgs {
    pub account: String,
    pub draft: String,
    #[serde(default)]
    pub token: Option<UserToken>,
}

/// `PlatformPort::hosted_wake`'s (M182).
#[derive(Serialize, Deserialize)]
pub struct WakeArgs {
    pub project: String,
    pub workspace: String,
}

/// A hosted box's turn (M185): its workspace and machine, as the hub's
/// hosted certificate names them, and the agent or the turn.
#[derive(Serialize, Deserialize)]
pub struct TurnArgs {
    pub workspace: String,
    pub machine: String,
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub turn: String,
    /// The thread session a turn is for (M254), if it's one's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct WorkArgs {
    pub machine: HostedMachine,
    pub takes: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ReportArgs {
    pub machine: HostedMachine,
    pub report: Report,
}

#[derive(Serialize, Deserialize)]
pub struct SessionReportArgs {
    pub machine: HostedMachine,
    pub report: SessionReport,
}

#[derive(Serialize, Deserialize)]
pub struct SessionPushArgs {
    pub machine: HostedMachine,
    pub ask: PushAsk,
}

#[derive(Serialize, Deserialize)]
pub struct SessionIssuesArgs {
    pub machine: HostedMachine,
    pub ask: SessionIssuesAsk,
}

#[derive(Serialize, Deserialize)]
pub struct SecretsArgs {
    pub machine: HostedMachine,
    pub ask: SecretsAsk,
}

#[derive(Serialize, Deserialize)]
pub struct HostedArgs {
    pub project: String,
    pub workspace: String,
    pub machine: String,
    #[serde(default)]
    pub report: Option<SetupReport>,
}

/// A bound hud box's project (M237's `box_members`).
#[derive(Serialize, Deserialize)]
pub struct ProjectArgs {
    pub project: String,
}

/// A machine and the account it acts for (M184's `machine_drives`).
#[derive(Serialize, Deserialize)]
pub struct DrivesArgs {
    pub daemon: String,
    pub account: String,
}

/// A hosted machine's transcript (M184): its workspace and machine, as the
/// hub's hosted certificate names them.
#[derive(Serialize, Deserialize)]
pub struct TranscriptArgs {
    pub workspace: String,
    pub machine: String,
    pub transcript: TranscriptPost,
}

/// Public previews (M187): a preview's id, or a workspace's.
#[derive(Serialize, Deserialize)]
pub struct PreviewArgs {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub workspace: String,
}

#[derive(Serialize, Deserialize)]
pub struct PasskeyArgs {
    pub account: String,
    pub challenge: String,
    pub assertion: PasskeyAssertion,
}

#[derive(Serialize, Deserialize)]
pub struct ProductUserArgs {
    pub client: String,
    pub user: String,
}

#[derive(Serialize, Deserialize)]
pub struct UnlinkArgs {
    pub account: String,
    pub client: String,
}

#[derive(Serialize, Deserialize)]
pub struct AccessArgs {
    pub token: String,
    pub client: String,
}

#[derive(Serialize, Deserialize)]
pub struct UploadArgs {
    pub machine: HostedMachine,
    pub upload: Upload,
}
