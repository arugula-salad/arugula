//! A merged PR moves the Plane items it names to Done (Arugula #437). The PR
//! names them as it names a board's rows ([`crate::boards::refs`]: `ARUGU-12`
//! in its title, branch or body; `part of` only links), and
//! [`crate::forge::closes`] queues a job for each, one to a PR and item.
//!
//! - **Sent at once, as nobody's agent.** A person merging the PR is the
//!   decision, so the connection's write policy (which covers what agents
//!   write) isn't asked and nothing waits for approval. It is a close as
//!   completed ([`writes::send`]): the first `completed` state of the Plane
//!   project, a state change alone, so no comment, body or footer. Whoever
//!   merged it is who it is for when they are in the project, else Arugula.
//! - **Left alone** once the item is `completed` or `cancelled`: the mirror
//!   says so, or, when it doesn't, one read of the item from Plane does,
//!   which also catches a mirror a webhook hasn't caught up.
//! - **A failure isn't lost.** Plane refusing or unreachable fails the step,
//!   and the job queue ([`crate::jobs`]) tries again after a back-off, up to
//!   [`ATTEMPTS`] times; then the job ends `failed` with its error in
//!   `platform_jobs`. A retry reads the item first, so one whose move did
//!   land (a crash before the job was marked done) sends nothing.
//! - **Once.** A redelivered webhook or a second pass over the PR queues the
//!   same job id, which is refused; and a moved item is left alone.

use std::sync::Arc;

use anyhow::bail;
use arugula_control_wire::projects::{TrackerClose, TrackerWrite};
use arugula_e2e::now_ms;
use axum::http::StatusCode;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::info;

use super::{TrackerError, connect::ingest_page, writes};
use crate::{
    Platform,
    db::{
        Db,
        jobs::{JobKind, LeasedJob},
    },
    forge::closes::merger,
    jobs::{Clock, Handler, Step},
};

/// Claims a move gets before it ends `failed`: with the queue's back-off,
/// about half a day of a Plane that is down.
pub const ATTEMPTS: u32 = 20;

/// One item of a Plane connection to move for one merged PR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Move {
    pub project: String,
    pub connection: String,
    /// The item's number in its Plane project.
    pub number: u64,
    /// `owner/repo#123`.
    pub pr: String,
    /// GitHub's id of whoever merged it.
    pub merged_by: Option<u64>,
}

/// Queue `to` for the PR with this mirror id. `false` if it is queued, or
/// was done, already.
pub fn queue(db: &Db, pr: &str, to: &Move) -> anyhow::Result<bool> {
    let id = format!("planedone:{pr}:{}:{}", to.connection, to.number);
    db.enqueue_job(&id, JobKind::PlaneDone, &serde_json::to_value(to)?, now_ms(), now_ms())
}

/// Whether Plane's state group is one that is left alone.
fn finished(group: &str) -> bool {
    matches!(group, "completed" | "cancelled")
}

/// The `plane_done` kind's handler.
pub struct Moves {
    pub app: Arc<Platform>,
}

impl Moves {
    async fn step(&self, db: &Db, job: &mut LeasedJob, clock: &Clock) -> anyhow::Result<Step> {
        let m: Move = serde_json::from_value(job.payload.clone())?;
        let app = &*self.app;
        let Some(c) = db.tracker_connection(&m.project, &m.connection)?.filter(|c| c.unlinked_at.is_none()) else {
            return Ok(Step::Done);
        };
        let reference = format!("{}-{}", c.identifier, m.number);
        let Some(t) = db.tracker_target(&m.project, &reference)?.filter(|t| t.connection_id == c.id) else {
            return Ok(Step::Done);
        };
        if db.tracker_item_group(&c.id, &t.source_item_id)?.is_some_and(|g| finished(&g)) {
            return Ok(Step::Done);
        }
        let api = |e: arugula_control_common::ApiError| anyhow::anyhow!("{}", e.1);
        // The mirror may be behind: ask Plane, and mend the mirror if it is.
        let provider = writes::provider(app, &m.project, &c).map_err(api)?;
        match provider.get(&c.workspace_slug, &c.source_project_id, &t.source_item_id).await {
            Ok(live) if finished(&live.state_group) => {
                if let Some(ids) = ingest_page(app, &c.id, c.mirror_id, None, &[live], now_ms())? {
                    crate::boards::items_changed(app, &ids);
                }
                return Ok(Step::Done);
            }
            Ok(_) => {}
            Err(TrackerError::NotFound) => return Ok(Step::Done),
            Err(e) => bail!("couldn't read {reference} in Plane: {e}"),
        }
        let by = match m.merged_by {
            Some(g) => merger(app, &m.project, Some(g)).await,
            None => None,
        };
        let person = match &by {
            Some(account) => writes::person_name(app, account).await.map_err(api)?,
            None => "Arugula".to_owned(),
        };
        let who = writes::Who { person, agent: None };
        let close = TrackerWrite::Close { item: t.id, close_as: TrackerClose::Completed };
        let landed = match writes::send(app, &m.project, &c, &close, &who).await {
            Ok(landed) => landed,
            // The item or the connection went while it was sent.
            Err(e) if e.0 == StatusCode::NOT_FOUND || e.0 == StatusCode::CONFLICT => return Ok(Step::Done),
            // Nothing a retry changes: say so on the job.
            Err(e) if e.0 == StatusCode::UNPROCESSABLE_ENTITY || e.0 == StatusCode::NOT_IMPLEMENTED => {
                return Ok(Step::Failed(e.1));
            }
            Err(e) => bail!("{}", e.1),
        };
        let state = landed.state.as_deref().unwrap_or("Done");
        let line = format!("moved {reference} to {state}: merged {}", m.pr);
        info!(project = %m.project, connection = %c.id, by = by.as_deref().unwrap_or("Arugula"), "{line}");
        job.checkpoint = Some(json!({ "moved": line, "by": by }));
        db.checkpoint_job(job, clock.now())?;
        Ok(Step::Done)
    }
}

impl Handler for Moves {
    fn kind(&self) -> JobKind {
        JobKind::PlaneDone
    }

    fn max_attempts(&self) -> u32 {
        ATTEMPTS
    }

    /// A read, a write, a read back and a question to the hub.
    fn deadline(&self) -> u64 {
        120_000
    }

    fn step<'a>(&'a self, db: &'a Db, job: &'a mut LeasedJob, clock: &'a Clock) -> BoxFuture<'a, anyhow::Result<Step>> {
        Box::pin(self.step(db, job, clock))
    }
}
