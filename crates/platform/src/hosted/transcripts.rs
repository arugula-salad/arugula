//! Hosted agent transcripts (M184, #1245). An agent block on a hosted
//! workspace's box keeps its transcript on the box's disk; when the block
//! closes, the box's daemon sends it, signed as its hosted machine, and the
//! hub hands it here ([`archive`], `PlatformPort::hosted_transcript`). It's
//! kept read only under the workspace's project
//! ([`crate::db::hosted_transcripts`]), for the project's members to read
//! after the box is gone ([`list`], [`read`]).
//!
//! - **Only the workspace's own machine** archives into it: the hub says
//!   which machine signed, and its certificate's workspace; a machine that
//!   isn't the one the workspace enrolled as now (a box made again since)
//!   is refused.
//! - **Size.** The daemon cuts the text to `MAX_TEXT` (its start and end
//!   kept); anything longer is refused here. A project keeps its newest
//!   [`MAX_PER_PROJECT`].
//! - **Plaintext, project-plane.** What the agent and its people wrote, as
//!   chat is: `tables.toml` says so, and docs/control-e2e.md.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::transcripts::{MAX_TEXT, TranscriptPost};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::{
    Platform,
    auth::Person,
    cloud::CloudDriver,
    db::hosted_transcripts::{HostedTranscript, NewTranscript},
    db::sql::{Optional as _, Sql as _, params},
    forge::links::membership,
};

/// Transcripts a project keeps: its newest.
pub const MAX_PER_PROJECT: usize = 1000;

/// What the platform answers a box's transcript, through the hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Archived {
    Kept { id: String },
    Refused { status: u16, error: String },
}

fn refused(status: StatusCode, error: &str) -> Archived {
    Archived::Refused { status: status.as_u16(), error: error.to_owned() }
}

/// A new transcript's id: `htx_` and 32 hex characters.
pub fn new_id() -> String {
    format!("htx_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// Keep a closed agent block's transcript from `machine`, the hosted machine
/// whose certificate names `workspace`.
pub async fn archive(app: &Platform, workspace: &str, machine: &str, t: TranscriptPost) -> anyhow::Result<Archived> {
    if t.text.len() > MAX_TEXT {
        return Ok(refused(StatusCode::PAYLOAD_TOO_LARGE, "a transcript is at most 768 KiB: cut it first"));
    }
    let row: Option<(String, Option<String>)> = app
        .db
        .c()
        .query_row("SELECT project_id, machine FROM hosted_workspaces WHERE id = ?1", params![workspace], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let Some((project, enrolled)) = row else {
        return Ok(refused(StatusCode::NOT_FOUND, "no such hosted workspace"));
    };
    if enrolled.as_deref() != Some(machine) {
        return Ok(refused(StatusCode::FORBIDDEN, "this machine isn't its workspace's box any more"));
    }
    let agent = match t.agent.as_str() {
        "claude" | "codex" | "acp" => t.agent.as_str(),
        _ => "agent",
    };
    let title: String = t.title.chars().filter(|c| !c.is_control()).take(200).collect();
    let cwd = t.cwd.as_deref().filter(|c| c.len() <= 4096);
    let now = arugula_e2e::now_ms();
    let new = NewTranscript {
        project: &project,
        workspace,
        machine,
        block: t.block,
        agent,
        title: title.trim(),
        cwd,
        started_at: t.started_at,
        closed_at: if t.closed_at == 0 { now } else { t.closed_at },
        cut: t.cut,
        body: &t.text,
    };
    let id = app.db.add_hosted_transcript(&new_id(), &new, MAX_PER_PROJECT, now)?;
    tracing::info!(%project, %workspace, transcript = %id, bytes = t.text.len(), "archived a hosted agent's transcript");
    Ok(Archived::Kept { id })
}

/// `GET /api/projects/{project}/transcripts`.
#[derive(Debug, Serialize)]
pub struct Transcripts {
    pub project: String,
    pub transcripts: Vec<HostedTranscript>,
}

#[derive(Debug, Deserialize)]
pub struct Which {
    /// Only this workspace's (`hw_…`).
    #[serde(default)]
    pub workspace: Option<String>,
}

/// `GET /api/projects/{project}/transcripts/{id}`.
#[derive(Debug, Serialize)]
pub struct Transcript {
    #[serde(flatten)]
    pub transcript: HostedTranscript,
    pub text: String,
}

/// A project's archived transcripts, newest first (`?workspace=` keeps to
/// one workspace's), for its members.
pub async fn list(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path(project): Path<String>,
    Query(q): Query<Which>,
) -> Result<Json<Transcripts>, ApiError> {
    let (project, _) = membership(&app, &account, &project, false).await?;
    let transcripts = app.db.hosted_transcripts(&project, q.workspace.as_deref())?;
    Ok(Json(Transcripts { project, transcripts }))
}

/// One archived transcript, with its text, for the project's members.
pub async fn read(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id)): Path<(String, String)>,
) -> Result<Json<Transcript>, ApiError> {
    let (project, _) = membership(&app, &account, &project, false).await?;
    let (transcript, text) =
        app.db.hosted_transcript(&project, &id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such transcript"))?;
    Ok(Json(Transcript { transcript, text }))
}
