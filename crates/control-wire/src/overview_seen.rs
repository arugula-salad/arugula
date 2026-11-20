//! Where a person last looked at a project's Overview (Arugula #1277): the
//! event log's head the Overview showed them, kept per person per project
//! on the platform. The briefing line under the Overview's stats ("since
//! 18:04 yesterday: 4 merged · 1 went red") counts the log
//! ([`crate::events::PROJECT_EVENTS`]) from this number to the head, so it
//! says what happened while they were away even while their chief of staff
//! is paused. The page moves it when the person leaves the Overview, or
//! hides its window, and when they come back.
//!
//! It's a number and a time, nothing about what happened: the events stay
//! in the log, each read again with the reader's sight.

use serde::{Deserialize, Serialize};

/// `GET` answers [`OverviewSeen`]; `PUT` takes [`OverviewSeenPut`] and
/// answers what was kept.
pub const OVERVIEW_SEEN: &str = "/api/projects/{project}/overview/seen";

/// The head a person's Overview of a project last showed them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct OverviewSeen {
    /// The log's number they had seen up to: 0 when they never looked.
    pub seq: u64,
    /// When it was kept, in ms since the epoch: 0 when they never looked.
    pub at_ms: i64,
}

/// `PUT`: the head the Overview showed. A number past the log's head is
/// kept as the head, and one below what's kept leaves it as it was, so
/// two tabs never move it back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct OverviewSeenPut {
    pub seq: u64,
}
