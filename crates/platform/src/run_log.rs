//! A run's durable log (platform step 7): what the CLI's `arugula runs
//! logs` and MCP's `read_run` read once a run has a log the platform keeps.
//!
//! M213 keeps it: the hosted machine uploads the run's log to the run
//! bucket (hosted workspaces only, kept 30 days), and [`page`] reads a page
//! of it there. Until then the platform keeps no log, and [`page`] answers
//! `None`: the run's output is in its pane on its machine, which its
//! `running` event names, and the project's people watch end to end (aru-055:
//! the platform doesn't read panes).
//!
//! The routes (`GET /api/runs/{run}/log` for a person,
//! `GET /api/daemon/runs/{run}/log` for a machine's agents, M216) have
//! checked who may read the run before they ask here.

use anyhow::Result;

use crate::{Platform, db::runs::Run};

/// Where a page starts: a byte offset, or so that it ends at the end of
/// the log as it is now (its tail).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum From {
    Offset(u64),
    Tail,
}

/// A page of a run's kept log: its text (lossy UTF-8 of the bytes read),
/// and whether it reaches the end of the log as it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub text: String,
    /// The offset of the first byte read.
    pub offset: u64,
    /// The offset after the last byte read.
    pub next_offset: u64,
    /// No byte after `next_offset` is kept (yet).
    pub at_end: bool,
}

/// At most `max` bytes of `run`'s kept log from `from`. `None`: the
/// platform keeps no log for it.
///
/// M213 fills this in from the run bucket. Until then there's none.
pub async fn page(app: &Platform, run: &Run, from: From, max: u64) -> Result<Option<Page>> {
    let _ = (app, run, from, max);
    Ok(None)
}
