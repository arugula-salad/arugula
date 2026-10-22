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

use arugula_e2e::project_grant::{GrantRevoke, ProjectGrant};
use serde::{Deserialize, Serialize};

/// `GET`, signed as the hosted machine: its work, answered [`Work`].
pub const WORK: &str = "/api/hosted/work";
/// `POST` [`Report`], signed as the hosted machine, answered [`Reported`].
pub const WORK_REPORT: &str = "/api/hosted/work/report";

/// What a machine takes: runs (S37's `kind run`, one command in a new
/// pane).
pub const TAKES_RUN: &str = "run";

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
    }
}
