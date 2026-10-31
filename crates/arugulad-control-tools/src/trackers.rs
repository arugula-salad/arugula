//! A project's connected Plane project, from agents (Arugula #359):
//! `tracker_write` creates, comments on, closes, reopens and edits its work
//! items, behind the `boards` Labs flag (listed only when it is on, and
//! refused when it is off).
//!
//! - **Through the owner's grants.** A call is a machine-signed route on the
//!   platform (`/api/daemon/projects/{project}/tracker-writes`,
//!   `crates/platform/src/trackers/writes.rs`), which asks this machine's
//!   `trackers.write` grant over the project every time
//!   (`arugula grants add <machine> trackers.write --project <name>`), and
//!   never goes past its owner's role.
//! - **A write may wait.** On a connection whose agent writes are set to
//!   "Approve each write" (the default) the write is kept for a person, and
//!   this answers "waiting for a person to approve it" with its `tw_` id:
//!   nothing reached Plane yet, and an agent can't approve its own. On
//!   "Allow all writes" it lands at once. Either way what lands ends with
//!   a "via Arugula" line naming the person and this machine, added by the
//!   platform when it is sent.
//! - **Items** are the mirror's (`fi_…`, which `forge_read` lists) or
//!   `IDENT-N` (`ARUGU-7`). Assignees are not here.

use arugula_control_wire::projects::{TrackerClose, TrackerWrite, TrackerWriteDone, agent_tracker_writes};
use reqwest::Method;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::mcp::{
    results::{TrackerReachProject, TrackersReach},
    tools::{Call, Kind, Out, done},
};
use crate::{control::CloudRefused, mux::Api};

pub(crate) const WRITE_TOOL: &str = "tracker_write";

/// The platform's route for what a machine reaches.
const REACH: &str = arugula_control_wire::projects::AGENT_TRACKER_REACH;

pub(crate) const WRITE_DESCRIPTION: &str = "Write to the Plane project connected to a project in Arugula control: create a work item, comment on one, close it (completed or cancelled), reopen it, or edit its title or body. Your person's trackers.write grant over the project says you may, never more than their role. A write may wait: when the connection is set to approve each agent write (the default) it is kept for a person to approve, edit or drop in the project's Triage, and this says \"waiting for a person to approve it\" with its id; nothing has reached Plane yet, and you can't approve it yourself. When the connection allows agent writes it lands at once. What is sent ends with a line saying it was sent via Arugula by your person's agent on this machine. Items are named by their fi_ id (forge_read lists them) or IDENT-N, like ARUGU-7. Assignees are not supported.";

#[derive(Deserialize, JsonSchema)]
pub struct CreateArgs {
    /// The project's id (prj_…) or personal. Default: the current project.
    #[serde(default)]
    pub project: Option<String>,
    /// The Plane connection to create the item in (tc_…, from capabilities'
    /// trackers). Not needed when the project has one connection.
    #[serde(default)]
    pub connection: Option<String>,
    /// Or its identifier, like ARUGU, in any case.
    #[serde(default)]
    pub tracker: Option<String>,
    /// One line, at most 255 characters.
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    /// The state group it starts in: backlog, unstarted or started.
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CommentArgs {
    #[serde(default)]
    pub project: Option<String>,
    /// The work item: its fi_ id, or IDENT-N.
    pub item: String,
    pub text: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloseArgs {
    #[serde(default)]
    pub project: Option<String>,
    pub item: String,
    /// completed or cancelled.
    #[serde(rename = "as")]
    pub close_as: CloseAs,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum CloseAs {
    Completed,
    Cancelled,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReopenArgs {
    #[serde(default)]
    pub project: Option<String>,
    pub item: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct EditArgs {
    #[serde(default)]
    pub project: Option<String>,
    pub item: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
}

pub(crate) const WRITE: &[Kind] = &[
    Kind {
        name: "create",
        description: "Create a work item in a connected Plane project (may wait for a person).",
        schema: schema_for_type::<CreateArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "comment",
        description: "Comment on a work item (may wait for a person).",
        schema: schema_for_type::<CommentArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "close",
        description: "Close a work item as completed or cancelled (may wait for a person).",
        schema: schema_for_type::<CloseArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "reopen",
        description: "Open a closed work item again (may wait for a person).",
        schema: schema_for_type::<ReopenArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "edit",
        description: "Change a work item's title or body (may wait for a person).",
        schema: schema_for_type::<EditArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
];

fn args<T: serde::de::DeserializeOwned>(a: Value) -> Result<T, String> {
    serde_json::from_value(a).map_err(|e| format!("bad arguments: {e}"))
}

/// What a refusal says to an agent.
fn said(e: CloudRefused) -> String {
    match e {
        CloudRefused::NotJoined => "this machine hasn't joined Arugula control, where Plane connections live: ask your person to join it (Getting started on the web page, or `arugulad join`)".into(),
        CloudRefused::OldControl | CloudRefused::Said { status: 405, .. } => {
            "this Arugula control doesn't let agents write to Plane yet".into()
        }
        e => e.to_string(),
    }
}

/// They act as this machine's owner: only the owner's own agents may.
async fn owners_trackers(c: &Call<'_>) -> Result<(), String> {
    crate::lab_flags::flag(c.app().control.state_dir(), arugula_proto::flags::BOARDS)?;
    if let Some(me) = c.me()
        && c.app().mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some()
    {
        return Err(format!("{WRITE_TOOL} acts as this machine's owner: only their own agents may"));
    }
    Ok(())
}

/// The write a kind and its arguments make, and the project it is for.
async fn write_of(c: &Call<'_>, kind: &str, a: Value) -> Result<(String, TrackerWrite), String> {
    let (project, write) = match kind {
        "create" => {
            let a: CreateArgs = args(a)?;
            (
                a.project,
                TrackerWrite::Create {
                    connection: a.connection,
                    tracker: a.tracker,
                    title: a.title,
                    body: a.body,
                    state: a.state,
                },
            )
        }
        "comment" => {
            let a: CommentArgs = args(a)?;
            (a.project, TrackerWrite::Comment { item: a.item, text: a.text })
        }
        "close" => {
            let a: CloseArgs = args(a)?;
            let close_as = match a.close_as {
                CloseAs::Completed => TrackerClose::Completed,
                CloseAs::Cancelled => TrackerClose::Cancelled,
            };
            (a.project, TrackerWrite::Close { item: a.item, close_as })
        }
        "reopen" => {
            let a: ReopenArgs = args(a)?;
            (a.project, TrackerWrite::Reopen { item: a.item })
        }
        "edit" => {
            let a: EditArgs = args(a)?;
            (a.project, TrackerWrite::Edit { item: a.item, title: a.title, body: a.body })
        }
        _ => return Err(format!("{WRITE_TOOL} kind {kind} isn't wired up")),
    };
    Ok((c.current_project(project.as_deref()).await?, write))
}

/// `tracker_write`.
pub(crate) async fn tracker_write(c: &Call<'_>, kind: &str, a: Value) -> Out {
    owners_trackers(c).await?;
    let (project, write) = write_of(c, kind, a).await?;
    let body = serde_json::to_value(&write).map_err(|e| e.to_string())?;
    let v = c.app().control.cloud(Method::POST, &agent_tracker_writes(&project), Some(&body)).await.map_err(said)?;
    let r: TrackerWriteDone =
        serde_json::from_value(v).map_err(|e| format!("Arugula control sent an answer I can't read: {e}"))?;
    let text = match r.state.as_str() {
        "waiting" => format!(
            "{}. Nothing has reached Plane yet; a person approves, edits or drops it in the project's Triage.",
            r.text
        ),
        _ => format!("{}, as your person's agent on this machine", r.text),
    };
    done(text, r)
}

/// Whether `tracker_write` is live here, for `capabilities`: joined, and a
/// live `trackers.write` grant over a project its owner reads.
pub(super) async fn trackers_reach(app: &crate::server::App) -> TrackersReach {
    let asked =
        tokio::time::timeout(std::time::Duration::from_secs(3), app.control.cloud_now(Method::GET, REACH, None));
    let v = match asked.await {
        Ok(Ok(v)) => v,
        Ok(Err(CloudRefused::Said { status: 404, .. })) => {
            return TrackersReach {
                live: false,
                projects: vec![],
                note: "Plane connections are in Labs: turn on boards for your account".into(),
            };
        }
        Ok(Err(e)) => return TrackersReach { live: false, projects: vec![], note: said(e) },
        Err(_) => {
            return TrackersReach {
                live: false,
                projects: vec![],
                note: "Arugula control didn't answer in time".into(),
            };
        }
    };
    let projects: Vec<TrackerReachProject> = serde_json::from_value(v["projects"].clone()).unwrap_or_default();
    let live = !projects.is_empty();
    let note = if live {
        "tracker_write creates, comments on, closes, reopens and edits a Plane project's work items over these projects. A write may wait for a person to approve it, as the connection says; what is sent says it was via Arugula, by your person's agent on this machine. Each project lists its connections (id, identifier, agent_writes): create takes connection or tracker, or neither when there is one. After a daemon update, a session may need to reload its tool list to see tools new in this version.".into()
    } else {
        "This machine's owner hasn't given its agents trackers.write: `arugula grants add <machine> trackers.write --project <name>`, or Settings, Agents, Plane writes, on the platform page.".into()
    };
    TrackersReach { live, projects, note }
}
