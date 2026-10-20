//! What Arugula control's `/api/cloud/…` routes take and answer with
//! (aru-046): Cloud workspaces and their settings, tabs and past tabs, the
//! agents tabs run, the models they may run on, and past cloud runs. The
//! platform (`arugula-platform`) serves them, the page and the daemon's
//! `cloud` tools call them, and the web client's copy is generated from
//! these types (`just proto-ts`, the `ts` feature).
//!
//! Two rules for every type here:
//! - **The bytes on the wire don't change.** These types moved here from
//!   control as they were (M137): a field that was `null` stays `null`, one
//!   that was left out still is.
//! - **Strict where control was strict, and nowhere else.** An agent's
//!   definition and a Cloud workspace's settings refuse a field control
//!   doesn't know (`deny_unknown_fields`): only the platform reads them, and
//!   a setting it would drop silently is worse than a refusal. What clients
//!   read is never made stricter.
//!
//! A field a request may leave out (`#[serde(default)]`) is optional in the
//! TypeScript too, since the page sends these settings as well as reading
//! them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// User-editable agent definition. Model funding and execution credentials are
/// supplied by control, not accepted as user-selected provider configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct AgentDefinition {
    pub name: String,
    pub instructions: String,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "Record<string, unknown>"))]
    pub mcp_servers: BTreeMap<String, Value>,
    #[serde(default)]
    pub skills: Vec<Skill>,
    #[serde(default)]
    pub secret_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct Skill {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CloudAgent {
    pub id: String,
    pub version: u64,
    pub definition: AgentDefinition,
    pub curated: bool,
}

/// Immutable launch snapshot. Changing an agent later cannot change a queued run.
/// Credentials belong in the secret store, never in this record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RunSpec {
    pub agent_id: String,
    pub agent_version: u64,
    pub agent: AgentDefinition,
    pub model: String,
    pub runtime: String,
    pub repository: String,
    pub git_ref: String,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub repository_secret: Option<String>,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CloudRun {
    pub id: String,
    pub spec: RunSpec,
    pub state: String,
    pub created: u64,
    pub deadline: u64,
    #[cfg_attr(feature = "ts", ts(type = "Record<string, unknown> | null"))]
    pub result: Option<Value>,
}

/// What a Cloud workspace's machine is built from, as the user saved it: it
/// becomes the environment control keeps for the Cloud workspace. Secret
/// values stay in the account's secret store; this only names them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct CloudWorkspaceSpec {
    pub name: String,
    #[serde(default)]
    pub repositories: Vec<CloudWorkspaceRepository>,
    #[serde(default)]
    pub packages: CloudWorkspacePackages,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional, type = "string"))]
    pub setup_script: String,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub setup_timeout_seconds: Option<u32>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional, type = "Record<string, string>"))]
    pub env_vars: BTreeMap<String, String>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub networking: Option<CloudWorkspaceNetworking>,
    /// The account's secrets the vault holds besides the repositories' own.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional, type = "Array<string>"))]
    pub secrets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct CloudWorkspaceRepository {
    pub url: String,
    #[serde(default, rename = "ref")]
    #[cfg_attr(feature = "ts", ts(optional, type = "string"))]
    pub git_ref: String,
    /// `/home/sprite/<repository name>` when left out.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional, type = "string"))]
    pub mount: String,
    /// The secret the clone authenticates with, kept in the vault.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub secret: Option<String>,
}

/// Managoat installs `apt` and `npm` packages; other toolchains go in the
/// setup script.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct CloudWorkspacePackages {
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional, type = "Array<string>"))]
    pub apt: Vec<String>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional, type = "Array<string>"))]
    pub npm: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct CloudWorkspaceNetworking {
    /// `unrestricted` or `limited`.
    #[serde(rename = "type")]
    #[cfg_attr(feature = "ts", ts(type = "\"unrestricted\" | \"limited\""))]
    pub kind: String,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional, type = "Array<string>"))]
    pub allowed_hosts: Vec<String>,
}

/// A saved setting the machine wasn't built with: `before` from
/// `built_spec` (null when that isn't known), `after` from `saved_spec`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CloudWorkspaceChange {
    pub field: String,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub before: Value,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub after: Value,
}

/// A Cloud workspace as the account sees it. Its Managoat ids stay in
/// control.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CloudWorkspace {
    pub id: String,
    pub name: String,
    pub saved_spec: CloudWorkspaceSpec,
    pub built_spec: Option<CloudWorkspaceSpec>,
    /// Managoat's `provisioning`, `ready` or `failed`, or `deleting`.
    #[cfg_attr(feature = "ts", ts(type = "\"provisioning\" | \"ready\" | \"failed\" | \"deleting\""))]
    pub status: String,
    #[cfg_attr(feature = "ts", ts(type = "\"pending\" | \"starting\" | \"ready\" | \"suspended\" | null"))]
    pub sandbox_status: Option<String>,
    /// The step its setup is running (`machine`, `packages`,
    /// `repositories`, `setup_script`…) while it's `provisioning`.
    pub setup_step: Option<String>,
    pub error: Option<String>,
    pub revision: Option<String>,
    pub built_from_revision: Option<String>,
    pub stale: bool,
    pub changed_fields: Vec<String>,
    /// What a rebuild would change (`pending_changes`).
    pub pending: Vec<CloudWorkspaceChange>,
    /// False from a settings save until Managoat has them.
    pub settings_applied: bool,
    /// A rebuild or retry waits for Managoat to have a machine free.
    pub rebuild_queued: bool,
    /// Why the last one that waited didn't happen.
    pub rebuild_error: Option<String>,
    pub created: u64,
}

/// `GET /api/cloud/workspaces`: the account's Cloud workspaces that aren't
/// deleted yet, newest first.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CloudWorkspaces {
    pub cloud_workspaces: Vec<CloudWorkspace>,
}

/// What a Cloud workspace's last setup printed, step by step, redacted by
/// Managoat: only the fields Managoat documents, each as Managoat sent it
/// and left out when it sent none. The fields are in the order control
/// always wrote them (by name).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CloudSetupLog {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "string | null"))]
    pub finished_at: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "string | null"))]
    pub started_at: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "string"))]
    pub status: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "string | null"))]
    pub step: Option<Value>,
    pub steps: Vec<CloudSetupStep>,
}

/// One step of a [`CloudSetupLog`].
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CloudSetupStep {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "string | null"))]
    pub finished_at: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub omitted_bytes: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub omitted_lines: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "string"))]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "string | null"))]
    pub started_at: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "string"))]
    pub status: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub step: Option<Value>,
}

/// A tab as the account sees it: one conversation in a Cloud workspace.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CloudTab {
    pub id: String,
    pub cloud_workspace: String,
    pub agent_id: String,
    pub agent_name: String,
    pub runtime: String,
    pub model: String,
    pub cwd: String,
    /// `opening` until its conversation is launched, then the
    /// conversation's (`pending`, `running`, `idle`, `failed`, `terminated`)
    /// or `needs_input`; `closing` once closed; `failed` too when Managoat
    /// refused to open it.
    pub status: String,
    pub error: Option<String>,
    /// Opened with a first prompt while the Cloud workspace sets up: the
    /// prompt waits in Managoat until setup ends.
    pub waiting_for_setup: bool,
    pub created: u64,
}

/// `GET /api/cloud/workspaces/{id}/tabs`: a Cloud workspace's open tabs.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CloudTabs {
    pub cloud_tabs: Vec<CloudTab>,
}

/// A closed tab, read only: its conversation and agent are gone, its
/// transcript is kept. Its Cloud workspace may be deleted too.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PastCloudTab {
    pub id: String,
    pub cloud_workspace: String,
    pub cloud_workspace_name: String,
    /// Its Cloud workspace was deleted (the tab ended with it).
    pub cloud_workspace_deleted: bool,
    pub agent_name: String,
    pub runtime: String,
    pub model: String,
    pub cwd: String,
    pub created: u64,
    pub closed: u64,
}

/// A runtime and model a tab may run on (S40, decision 4).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CloudModel {
    pub runtime: String,
    pub model: String,
}
