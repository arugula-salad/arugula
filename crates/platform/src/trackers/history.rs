//! What Arugula changed in a Plane project lately (Arugula #441), read from
//! what is already stored, with no table of its own:
//!
//! - **A move to Done** ([`super::done`]) is a `plane_done` job. One that
//!   finished with a checkpoint is `moved`. One that gave up is
//!   `move_failed`, and its `job` is what Retry names: those are in
//!   `failed`, all of them (up to [`FAILED_SHOWN`]), not among the newest
//!   [`SHOWN`] `entries`. A job that ended without a checkpoint left the
//!   item alone, and isn't history.
//! - **An agent's write a person decided** is a `tracker_writes` row:
//!   `sent`, `dropped` or `write_failed`. A write still waiting is shown
//!   where it can be approved, and a write that landed at once under the
//!   connection's policy is never kept, so neither is here.
//! - **Retry** puts a failed move back in the queue with its attempts
//!   cleared, for an owner or editor.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::projects::{TrackerClose, TrackerEvent, TrackerHistory, TrackerWrite};
use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use tracing::info;

use super::{connect::changer, done::Move, writes::person_name};
use crate::{
    Platform,
    auth::Person,
    cloud::CloudDriver,
    db::trackers::{Connection, PendingWrite},
    forge::links::membership,
};

/// Most entries an answer holds.
pub const SHOWN: usize = 20;
/// Most failed moves an answer holds, so each has its Retry.
pub const FAILED_SHOWN: usize = 100;

fn missing() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such connection")
}

/// An account's name, or none when it can't be read: a name missing isn't
/// worth the whole history.
async fn named(app: &Platform, account: Option<&str>) -> Option<String> {
    person_name(app, account?).await.ok()
}

/// The write as the waiting-writes page words it: what it does, and to what.
async fn write_line(app: &Platform, project: &str, p: &PendingWrite) -> String {
    let Ok(w) = serde_json::from_str::<TrackerWrite>(&p.request) else {
        return "A write".to_owned();
    };
    let what = match &w {
        TrackerWrite::Create { .. } => "Create",
        TrackerWrite::Comment { .. } => "Comment",
        TrackerWrite::Close { close_as: TrackerClose::Cancelled, .. } => "Cancel",
        TrackerWrite::Close { .. } => "Close",
        TrackerWrite::Reopen { .. } => "Reopen",
        TrackerWrite::Edit { .. } => "Edit",
    };
    let target = match &w {
        TrackerWrite::Create { .. } => None,
        TrackerWrite::Comment { item, .. }
        | TrackerWrite::Close { item, .. }
        | TrackerWrite::Reopen { item }
        | TrackerWrite::Edit { item, .. } => {
            app.db.tracker_target(project, item).ok().flatten().map(|t| format!("{}-{}", t.identifier, t.number))
        }
    };
    format!("{what} {}", target.as_deref().unwrap_or("new item"))
}

/// `c`'s failed moves, all of them up to [`FAILED_SHOWN`], and its newest
/// [`SHOWN`] other changes, each newest first.
pub async fn read(app: &Platform, project: &str, c: &Connection) -> Result<TrackerHistory, ApiError> {
    let db = &app.db;
    let mut failed = vec![];
    for job in db.plane_moves_failed(&c.id, FAILED_SHOWN)? {
        let Ok(m) = serde_json::from_value::<Move>(job.payload.clone()) else { continue };
        if m.connection != c.id {
            continue;
        }
        failed.push(TrackerEvent {
            at: job.updated,
            kind: "move_failed".into(),
            line: format!("couldn't move {}-{} to Done: merged {}", c.identifier, m.number, m.pr),
            by: None,
            error: job.error.clone().or_else(|| Some("it failed".into())),
            job: Some(job.id.clone()),
        });
    }
    let mut entries = vec![];
    for job in db.plane_moves(&c.id, SHOWN * 4)? {
        let Ok(m) = serde_json::from_value::<Move>(job.payload.clone()) else { continue };
        if m.connection != c.id {
            continue;
        }
        let Some(line) = job.checkpoint.as_ref().and_then(|v| v["moved"].as_str()) else { continue };
        let by = named(app, job.checkpoint.as_ref().and_then(|v| v["by"].as_str())).await;
        entries.push(TrackerEvent {
            at: job.updated,
            kind: "moved".into(),
            line: line.to_owned(),
            by,
            error: None,
            job: None,
        });
    }
    for (p, at, by) in db.tracker_writes_decided(&c.id, SHOWN)? {
        let (kind, tail) = match p.state.as_str() {
            "sent" => ("sent", "approved and sent"),
            "dropped" => ("dropped", "dropped"),
            _ => ("write_failed", "couldn't be sent"),
        };
        let line = format!("{}, {tail}", write_line(app, project, &p).await);
        let error = if kind == "write_failed" { p.error.clone() } else { None };
        entries.push(TrackerEvent {
            at,
            kind: kind.into(),
            line,
            by: named(app, by.as_deref()).await,
            error,
            job: None,
        });
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.at));
    entries.truncate(SHOWN);
    Ok(TrackerHistory { entries, failed })
}

/// `GET /api/projects/{project}/trackers/{connection}/history`.
pub async fn history(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, connection)): Path<(String, String)>,
) -> Result<Json<TrackerHistory>, ApiError> {
    crate::boards::gate(&app, &account).await?;
    let (project, _) = membership(&app, &account, &project, false).await?;
    let c = app.db.tracker_connection(&project, &connection)?.ok_or_else(missing)?;
    Ok(Json(read(&app, &project, &c).await?))
}

/// `POST /api/projects/{project}/trackers/{connection}/moves/{job}/retry`:
/// a failed move goes back in the queue, as if it had just been queued.
pub async fn retry(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, connection, job)): Path<(String, String, String)>,
) -> Result<Json<TrackerHistory>, ApiError> {
    crate::boards::gate(&app, &account).await?;
    let project = changer(&app, &account, &project).await?;
    let c = app.db.tracker_connection(&project, &connection)?.ok_or_else(missing)?;
    if !app.db.retry_plane_move(&c.id, &job, now_ms())? {
        return Err(err(StatusCode::NOT_FOUND, "no failed move by that id"));
    }
    info!(%account, %project, connection = %c.id, %job, "retried a move to Done");
    Ok(Json(read(&app, &project, &c).await?))
}
