//! Runs on hosted machines (platform step 7): what the platform asks a
//! hosted machine through the hub, as typed ops ([`crate::forge::DaemonOp`]).
//!
//! - `runs.start` ([`RunStart`], M211): start under the project's standing
//!   grant (`arugula_e2e::project_grant`). The machine checks the grant
//!   against its own enrollment before anything starts, and answers
//!   `done`, or `cant` with the refusal's code. Until M212 a start carries
//!   no operation: the machine checks the grant and answers, and M212 adds
//!   the run it starts.
//! - `runs.revoke` ([`RunRevoke`], M211): an owner's device revoked the
//!   project's grants made by then. The machine checks the revoke once and
//!   keeps it (so the platform can't take it back by leaving it out later)
//!   and refuses those grants from then on.

use arugula_e2e::project_grant::{GrantRevoke, ProjectGrant};
use serde::{Deserialize, Serialize};

/// Start on this hosted machine under the project's standing grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStart {
    pub project: String,
    /// The hosted workspace (`hw_…`) the machine is.
    pub workspace: String,
    /// The run (`run_…`, M210), once there is one (M212).
    #[serde(default)]
    pub run: Option<String>,
    /// What it's asked to do under the grant: `runs.start` or
    /// `runs.instruct`.
    pub cap: String,
    pub grant: ProjectGrant,
    /// The project's revokes the platform holds.
    #[serde(default)]
    pub revokes: Vec<GrantRevoke>,
}

/// A revoke of the project's grants, for every hosted machine of its
/// project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRevoke {
    pub revoke: GrantRevoke,
}
