//! Runs on hosted machines (platform step 7): what the platform asks a
//! hosted machine through the hub, as typed ops ([`crate::forge::DaemonOp`]).
//!
//! - `runs.start` ([`RunStart`], M211): start under the project's standing
//!   grant (`arugula_e2e::project_grant`). The machine checks the grant
//!   against its own enrollment before anything starts, and answers
//!   `done`, or `cant` with the refusal's code. It carries no operation:
//!   one that names a run (M212) has the machine pull its work at once
//!   (`crate::work`), where the run comes with its grant, checked again.
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

// ---- Reading and starting runs (M216)
//
// A project's runs for the CLI (a person, or a CLI device as its account),
// and for a machine's agents through MCP, signed by the machine: those
// need the machine's `runs.read` grant over the project to read,
// `runs.start` to start a run spec and `runs.command` a command or an
// instruction of its own (M197's grants, given by its owner), and never go
// past the owner's role. A project in a path is its id, or `personal`.

/// `GET` [`RunListQuery`]: a project's runs, newest first, [`RunList`].
/// `POST` (M214): start one by hand.
pub const PROJECT_RUNS: &str = "/api/projects/{project}/runs";
/// `GET` [`RunQuery`]: a run with its timeline and artifacts, [`RunView`].
pub const RUN: &str = "/api/runs/{run}";
/// `GET` [`RunLogQuery`]: a page of a run's durable log, [`RunLogPage`].
pub const RUN_LOG: &str = "/api/runs/{run}/log";

/// `GET`: the projects whose runs this machine's agents reach, and with
/// which powers, [`RunReach`].
pub const AGENT_RUN_REACH: &str = "/api/daemon/runs/reach";
/// [`PROJECT_RUNS`] for a machine's agents: `GET` with `runs.read`, `POST`
/// [`AgentRunStart`] with `runs.start` for a run spec, or `runs.command`
/// for a command or an instruction of its own, answered [`RunView`].
pub const AGENT_PROJECT_RUNS: &str = "/api/daemon/projects/{project}/runs";
/// [`RUN`] for a machine's agents (`runs.read`).
pub const AGENT_RUN: &str = "/api/daemon/runs/{run}";
/// [`RUN_LOG`] for a machine's agents (`runs.read`).
pub const AGENT_RUN_LOG: &str = "/api/daemon/runs/{run}/log";

/// The longest a read of a run waits for it to move ([`RunQuery::wait`]),
/// in seconds: well inside a daemon's request timeout.
pub const RUN_WAIT_MAX: u64 = 15;
/// The most bytes a log page holds ([`RunLogQuery::max`]).
pub const RUN_LOG_PAGE_MAX: u64 = 64 * 1024;

/// [`AGENT_PROJECT_RUNS`] for `project`.
pub fn agent_project_runs(project: &str) -> String {
    format!("/api/daemon/projects/{project}/runs")
}

/// [`AGENT_RUN`] for `run`.
pub fn agent_run(run: &str) -> String {
    format!("/api/daemon/runs/{run}")
}

/// [`AGENT_RUN_LOG`] for `run`.
pub fn agent_run_log(run: &str) -> String {
    format!("/api/daemon/runs/{run}/log")
}

/// [`PROJECT_RUNS`]' query.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunListQuery {
    /// Only runs in this state (`pending`, `running`, `succeeded`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// At most this many (default 20, at most 100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// [`RUN`]'s query: a read that waits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunQuery {
    /// The last event seen (`last_seq`): with `wait`, answer once the run
    /// has a later one, or has ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    /// Seconds to wait for that, at most [`RUN_WAIT_MAX`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<u64>,
}

/// [`RUN_LOG`]'s query.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunLogQuery {
    /// A byte offset in the log (a page's `next_offset`); 0 without.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// At most this many bytes, at most [`RUN_LOG_PAGE_MAX`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u64>,
    /// The page that ends at the log's end as it is now, instead of
    /// `offset`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail: Option<bool>,
}

/// [`AGENT_PROJECT_RUNS`]' `POST`: a run spec by its id or name, or a
/// command (`argv`) or an instruction to an agent on one of the project's
/// hosted workspaces (its id or name; the only one when left out), as a
/// person's start by hand takes it (M214).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunStart {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
}

/// A run, as the CLI and a machine's agents read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunView {
    pub id: String,
    pub project: String,
    /// The hosted workspace (`hw_…`) it runs on.
    #[serde(default)]
    pub workspace: Option<String>,
    /// `manual`, `schedule` or `event`.
    pub trigger: String,
    /// Which spec, schedule or event: what an event's payload said is
    /// untrusted text, for showing.
    #[serde(default)]
    pub source: serde_json::Value,
    /// The S37 operation it runs (`arugula op v1`), and its SHA-256.
    #[serde(default)]
    pub op: String,
    pub op_hash: String,
    /// The machine it runs on.
    pub daemon: String,
    /// S37's: `pending`, `approved`, `running`, `succeeded`, `failed`,
    /// `denied`, `canceled` or `expired`.
    pub state: String,
    /// The account that started it (an agent's owner's), or the schedule's
    /// or rule's id.
    #[serde(default)]
    pub started_by: String,
    /// The machine whose agent started it.
    #[serde(default)]
    pub started_by_agent: Option<String>,
    /// The standing grant it runs under; without one it waits for an
    /// approval.
    #[serde(default)]
    pub grant: Option<String>,
    pub last_seq: u64,
    pub created_at: u64,
    #[serde(default)]
    pub started_at: Option<u64>,
    #[serde(default)]
    pub ended_at: Option<u64>,
    #[serde(default)]
    pub exit_code: Option<i64>,
    /// The pane it runs in on its machine (its `running` event says): its
    /// live view, which the project's people watch end to end.
    #[serde(default)]
    pub pane: Option<u64>,
    /// Its timeline, oldest first (left out of a list).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<RunEventView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<RunArtifactView>,
}

/// One change of a run's state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunEventView {
    pub seq: u64,
    #[serde(default)]
    pub from: Option<String>,
    pub to: String,
    pub at: u64,
    /// What its reporter said (an exit code, a reason, the pane).
    #[serde(default)]
    pub detail: serde_json::Value,
    /// The machine that reported it, signed; none when control did.
    #[serde(default)]
    pub daemon: Option<String>,
}

/// Something a run left behind (M213).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunArtifactView {
    pub name: String,
    pub size: u64,
    pub digest: String,
    pub created_at: u64,
}

/// [`PROJECT_RUNS`]' answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunList {
    pub project: String,
    pub runs: Vec<RunView>,
}

/// A page of a run's durable log (M213: hosted workspaces only, kept 30
/// days).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunLogPage {
    pub run: String,
    /// Whether the platform keeps a log for it. Until it does, the run's
    /// pane is where its output is.
    pub kept: bool,
    pub text: String,
    pub offset: u64,
    pub next_offset: u64,
    /// The run has ended and this page reaches the end of its log.
    pub end: bool,
    /// Why there's no log, or what to do for the rest.
    #[serde(default)]
    pub note: Option<String>,
}

/// [`AGENT_RUN_REACH`]'s answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReach {
    pub projects: Vec<RunReachProject>,
}

/// A project a machine's run grants reach, as its owner reads it now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReachProject {
    pub id: String,
    pub name: String,
    /// `runs.read`, `runs.start`, `runs.command`.
    pub powers: Vec<String>,
}
