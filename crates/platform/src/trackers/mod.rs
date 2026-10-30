//! Trackers as a source of work (Arugula #356, phase 3): a Plane project
//! connected to an Arugula project has its work items mirrored into
//! `forge_items`, beside GitHub's.
//!
//! - **[`TrackerProvider`]** is what a tracker answers, one object per API
//!   key: the project, its items (paged), one item, and the writes. Plane's
//!   client ([`plane`]) reads for real and answers "not yet" to the writes
//!   (#359); [`fake::FakeTracker`] keeps items in memory for tests.
//! - **[`Trackers`]** makes a provider from a key. The platform holds one
//!   (`Platform::trackers`), so a test swaps in the fake.
//! - **Connecting** ([`connect`]): an owner or editor gives the workspace,
//!   the project and a key; control reads the project with it, and only then
//!   keeps the connection and seals the key. The backfill runs on a lease,
//!   as a linked repo's poller does. Disconnecting stops ingest and the
//!   purge takes the rows after the grace period.
//! - **The mirror.** A connection's rows are `forge_items` with
//!   `repo_id` = the connection's negative `mirror_id` and `number` = the
//!   tracker's sequence id. [`connect::ingest_page`] is the one place a page
//!   of items becomes rows; the reconcile and the webhooks (#358) call it
//!   too.
//! - **The key** is never returned, logged, or put in an error: it lives in
//!   a local `String` passed to the client, and the client's `Debug` hides
//!   it.

use std::{future::Future, pin::Pin, sync::Arc};

#[cfg(test)]
mod boards_tests;
pub mod connect;
#[cfg(any(test, feature = "testing"))]
pub mod connect_tests;
#[cfg(any(test, feature = "testing"))]
pub mod fake;
pub mod plane;
pub mod webhook;
#[cfg(any(test, feature = "testing"))]
pub mod webhook_tests;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Why a tracker didn't answer. The texts are plain sentences for a person
/// and never carry the tracker's response, which may echo a header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackerError {
    /// The tracker refused the key (401 or 403).
    Unauthorized,
    /// There's no such workspace, project or item.
    NotFound,
    /// The tracker couldn't be reached or said something else.
    Unavailable(String),
    /// This provider doesn't do it yet.
    NotYet(&'static str),
}

impl std::fmt::Display for TrackerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrackerError::Unauthorized => f.write_str("the tracker refused that API key"),
            TrackerError::NotFound => f.write_str("the tracker has no such workspace, project or item"),
            TrackerError::Unavailable(why) => write!(f, "couldn't reach the tracker: {why}"),
            TrackerError::NotYet(what) => write!(f, "{what} isn't supported yet"),
        }
    }
}

impl std::error::Error for TrackerError {}

pub type Result<T> = std::result::Result<T, TrackerError>;

/// A tracker's project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    /// The tracker's own id (Plane's uuid).
    pub id: String,
    /// The short key, such as `ARUGU`.
    pub identifier: String,
    pub name: String,
}

/// A work item, as the mirror keeps it: never its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The tracker's own id (Plane's uuid).
    pub id: String,
    /// The number the project counts its items by (`ARUGU-7` is 7).
    pub sequence_id: u64,
    pub title: String,
    /// The item type's name (`Epic`, `Task`), when the project has types.
    pub item_type: Option<String>,
    pub state_name: String,
    /// `backlog`, `unstarted`, `started`, `completed` or `cancelled`.
    pub state_group: String,
    /// The parent's id, for a child of an epic.
    pub parent_id: Option<String>,
    /// Milliseconds since the epoch, the tracker's.
    pub created_at: u64,
    pub updated_at: u64,
}

impl Item {
    /// The mirror's `state`: closed once the tracker's state group is
    /// `completed` or `cancelled`.
    pub fn state(&self) -> &'static str {
        match self.state_group.as_str() {
            "completed" | "cancelled" => "closed",
            _ => "open",
        }
    }
}

/// A page of items, and where the next one starts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Page {
    pub items: Vec<Item>,
    pub next: Option<String>,
}

/// What a new item says (#359).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NewItem {
    pub title: String,
    pub body: Option<String>,
    pub item_type: Option<String>,
    pub parent_id: Option<String>,
}

/// What an edit changes (#359): what's `None` stays.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Edit {
    pub title: Option<String>,
    pub body: Option<String>,
}

/// A tracker's API, for one key. Object-safe and async, as the platform's
/// other providers are. `project_id` is the tracker's own id, as
/// [`ProjectInfo::id`] gives it.
pub trait TrackerProvider: Send + Sync {
    /// The project `reference` names: its id, or its identifier.
    fn project<'a>(&'a self, workspace: &'a str, reference: &'a str) -> BoxFuture<'a, Result<ProjectInfo>>;
    /// A page of the project's items, most recently updated first, from
    /// `cursor` (what a page's `next` said). `since`: stop at items not
    /// updated after it (milliseconds).
    fn list<'a>(
        &'a self,
        workspace: &'a str,
        project_id: &'a str,
        since: Option<u64>,
        cursor: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Page>>;
    fn get<'a>(&'a self, workspace: &'a str, project_id: &'a str, item: &'a str) -> BoxFuture<'a, Result<Item>>;
    fn create<'a>(&'a self, workspace: &'a str, project_id: &'a str, item: &'a NewItem) -> BoxFuture<'a, Result<Item>>;
    fn comment<'a>(
        &'a self,
        workspace: &'a str,
        project_id: &'a str,
        item: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Result<()>>;
    fn set_state<'a>(
        &'a self,
        workspace: &'a str,
        project_id: &'a str,
        item: &'a str,
        state: &'a str,
    ) -> BoxFuture<'a, Result<Item>>;
    fn edit<'a>(
        &'a self,
        workspace: &'a str,
        project_id: &'a str,
        item: &'a str,
        edit: &'a Edit,
    ) -> BoxFuture<'a, Result<Item>>;
    /// The items whose parent is `epic`.
    fn children<'a>(
        &'a self,
        workspace: &'a str,
        project_id: &'a str,
        epic: &'a str,
    ) -> BoxFuture<'a, Result<Vec<Item>>>;
}

/// Makes a provider from a key.
pub trait Trackers: Send + Sync {
    fn plane(&self, key: &str) -> Arc<dyn TrackerProvider>;
}

/// The real Plane, at `base` (`https://api.plane.so`).
pub struct Live {
    http: reqwest::Client,
    base: String,
}

impl Live {
    pub fn new(http: reqwest::Client, base: &str) -> Self {
        Live { http, base: base.trim_end_matches('/').to_owned() }
    }
}

impl Trackers for Live {
    fn plane(&self, key: &str) -> Arc<dyn TrackerProvider> {
        Arc::new(plane::Plane::new(self.http.clone(), &self.base, key))
    }
}

/// Plane's own API.
pub const PLANE_BASE: &str = "https://api.plane.so";

/// Plane's web app, where an item's link goes.
pub const PLANE_APP: &str = "https://app.plane.so";

/// An item's link in Plane's web app.
pub fn item_url(workspace: &str, identifier: &str, sequence: u64) -> String {
    format!("{PLANE_APP}/{workspace}/browse/{identifier}-{sequence}/")
}
