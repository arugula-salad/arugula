//! Work for hosted machines (M212, platform step 7): the runs the platform
//! has for a hosted machine, which it pulls, and the state changes it
//! reports back, each signed. S37's execution contract
//! (`spikes/s37-team-permissions/README.md`, q4) on `arugulad --hosted`.
//!
//! - **Pulling.** A hosted machine asks for its work ([`WORK`], signed as
//!   the machine, saying what it takes: [`WorkQuery`]) when it's enrolled,
//!   and every few seconds after. Only the hub's hosted signature opens it
//!   (`auth::HostedAuth`): a person's machine never gets work. The answer
//!   is every run that's ready for the machine and hasn't started, until it
//!   has: the same run comes again on every pull, so the machine's ledger
//!   (`<state>/work/ID.json`) is what makes it run at most once.
//! - **Reporting.** Each state change the machine makes (`running`,
//!   `succeeded`, `failed`) goes to [`WORK_REPORT`] as a [`Report`]: the
//!   event's fields and the machine's signature over `arugula run event
//!   v1` (`arugula_e2e::run::Event`). The platform keeps the signature with
//!   the event, so anyone can check later which machine said what.

use arugula_e2e::{
    deploy::{Approval, Operation},
    project_grant::{GrantRevoke, ProjectGrant},
};
use serde::{Deserialize, Serialize};

/// `GET`, signed as the hosted machine: its work, answered [`Work`].
pub const WORK: &str = "/api/hosted/work";
/// `POST` [`Report`], signed as the hosted machine, answered [`Reported`].
pub const WORK_REPORT: &str = "/api/hosted/work/report";

/// What a machine takes: runs (S37's `kind run`, one command in a new
/// pane).
pub const TAKES_RUN: &str = "run";
/// What a machine takes: deploys' applies (M219, `kind deploy`). A machine
/// that doesn't say so is never sent one.
pub const TAKES_DEPLOY: &str = "deploy";

/// `POST` [`SecretsAsk`], signed as the hosted machine (M219): the
/// environment's secrets for one of its deploy runs, while it runs, once.
/// Answered [`SecretsGiven`].
pub const WORK_SECRETS: &str = "/api/hosted/work/secrets";

/// [`WORK`]'s query: what the machine takes, comma-separated. A machine
/// that says nothing takes nothing, so an older one never gets work.
pub struct WorkQuery;

impl WorkQuery {
    pub fn path(takes: &[&str]) -> String {
        format!("{WORK}?takes={}", takes.join(","))
    }
}

/// [`WORK`]'s answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Work {
    #[serde(default)]
    pub items: Vec<WorkItem>,
    /// Every revoke of the project's grants the platform holds, on every
    /// pull, ready runs or not: a machine checks them against the grants
    /// its runs going were started under, and stops those a `running stop`
    /// covers.
    #[serde(default)]
    pub revokes: Vec<GrantRevoke>,
}

/// A run that's ready for this machine and hasn't started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkItem {
    /// `run_` and 32 hex characters.
    pub run: String,
    pub project: String,
    /// The hosted workspace (`hw_…`) the machine is.
    pub workspace: String,
    /// The S37 operation's bytes (`arugula op v1`), exactly as stored.
    pub op: String,
    /// Their SHA-256, hex: the machine checks the bytes against it.
    pub op_hash: String,
    /// `pending` (under the project's standing grant) or `approved`.
    pub state: String,
    /// The run's latest event: the machine's first report is the next.
    pub last_seq: u64,
    /// The project's standing grant the run starts under (M211), when it's
    /// `pending` under one, and the project's revokes the platform holds.
    /// The machine checks them against its own enrollment before anything
    /// starts, and keeps the revokes.
    #[serde(default)]
    pub grant: Option<ProjectGrant>,
    #[serde(default)]
    pub revokes: Vec<GrantRevoke>,
    /// A deploy's run (M219): the deploy it applies, with its owner's
    /// approval, which the machine checks itself before it starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deploy: Option<DeployWork>,
}

/// What a deploy's run applies (M219), for its machine to check: the
/// deploy's operation (`arugula deploy op v1`) and the approval an owner's
/// device signed over it, with the passkey assertion the hub checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeployWork {
    /// `dep_` and 32 hex characters.
    pub deploy: String,
    pub operation: Operation,
    pub approval: Approval,
    #[serde(default)]
    pub passkey: Option<PasskeyAssertion>,
    /// The environment's driver.
    pub driver: String,
    /// The names of the secrets its driver gets: their values come only
    /// from [`WORK_SECRETS`], while the run runs.
    #[serde(default)]
    pub secrets: Vec<String>,
    /// The environment's driver settings (M220): the plan names them, so
    /// the machine checks them against the approved plan
    /// (`arugula_e2e::drivers::planned`).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub settings: arugula_e2e::drivers::Settings,
    /// For a roll back (M220): the earlier deploy it goes back to, and what
    /// that ran. The plan names it too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roll_back: Option<arugula_e2e::drivers::RollBack>,
    /// The approval's id (`apv_`), for the driver to note where it records
    /// it (chant's approve note).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
}

/// A passkey's assertion as the hub checked it (M218): each base64url.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasskeyAssertion {
    pub id: String,
    pub client_data_json: String,
    pub authenticator_data: String,
    pub signature: String,
}

/// [`WORK_SECRETS`]'s body: which run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretsAsk {
    pub run: String,
}

/// [`WORK_SECRETS`]'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SecretsGiven {
    /// The environment's secrets and the driver's short-lived tokens, by
    /// name. Given once per run.
    Given { secrets: Vec<Secret> },
    /// Not given, and asking again won't change that.
    Refused { why: String },
}

/// One secret, by its name as an environment variable. Its value is never
/// printed (`Debug` leaves it out).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Secret {
    pub name: String,
    pub value: String,
    /// A short-lived token's revoke URL (`DELETE`, the token as its bearer):
    /// the box revokes it when its driver ends (M219).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoke: Option<String>,
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secret").field("name", &self.name).finish_non_exhaustive()
    }
}

/// One state change, signed by the machine that made it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub run: String,
    /// The event's number in the run.
    pub seq: u64,
    pub from: String,
    pub to: String,
    /// When (ms, the machine's clock).
    pub at: u64,
    /// A JSON object's text, exactly as signed: an exit code (`exit`), a
    /// pane (`pane`, `epoch`), or why it failed (`reason`).
    pub detail: String,
    /// The machine's signature over `arugula run event v1`, hex.
    pub sig: String,
}

/// What became of a [`Report`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reported {
    /// Kept: the run moved.
    Taken,
    /// Kept before: the same event, signed the same (a retry whose answer
    /// was lost).
    Already,
    /// Not kept, and sending it again won't change that. `now` is the run's
    /// state when it moved on without this machine (canceled, say), and
    /// `next` the event number it's at when this one's isn't it.
    Refused {
        why: String,
        #[serde(default)]
        now: Option<String>,
        #[serde(default)]
        next: Option<u64>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_the_wire() {
        assert_eq!(WorkQuery::path(&[TAKES_RUN]), "/api/hosted/work?takes=run");
        let r: Reported = serde_json::from_str(r#"{"kind":"refused","why":"no"}"#).unwrap();
        assert_eq!(r, Reported::Refused { why: "no".into(), now: None, next: None });
        assert_eq!(serde_json::to_string(&Reported::Already).unwrap(), r#"{"kind":"already"}"#);
        let w: Work = serde_json::from_str(r#"{"items":[{"run":"run_1","project":"prj_1","workspace":"hw_1","op":"x","op_hash":"h","state":"pending","last_seq":1,"extra":1}]}"#).unwrap();
        assert_eq!(w.items[0].grant, None);
        assert_eq!(w.items[0].deploy, None);
        assert!(!serde_json::to_string(&w).unwrap().contains("deploy"), "an older machine's answer, as it was");
        let s = Secret { name: "FLY_API_TOKEN".into(), value: "s3cr3t".into(), revoke: None };
        assert!(!format!("{s:?}").contains("s3cr3t"), "a secret's value is never printed");
        let g: SecretsGiven = serde_json::from_str(r#"{"kind":"refused","why":"no"}"#).unwrap();
        assert_eq!(g, SecretsGiven::Refused { why: "no".into() });
    }
}
