//! The platform in the hub's process (`--platform in-process`, the default
//! and the rollback), as the hub sees it (#314). The `arugula-control`
//! binary hands the hub a [`Platforms`]: the platform's flags, which it adds
//! to the hub's, and how to make the platform from them. The hub then holds
//! an [`InProcess`]: a [`PlatformPort`] it calls directly, the routes it
//! mounts behind its gate, and the background work it starts. Neither names
//! the platform's own types, so the hub's library never builds them.

use std::{any::Any, path::PathBuf, sync::Arc, time::Duration};

use arugula_control_common::analytics::Analytics;
use futures_util::future::BoxFuture;

use crate::{Config, HubPort, PlatformPort, hub::HubResult};

/// The platform as the hub runs it in process.
pub trait InProcess: PlatformPort + Any + Send + Sync {
    /// Its routes, with its own state applied: the hub mounts them behind
    /// its gate.
    fn router(self: Arc<Self>) -> axum::Router;
    /// Its background work (the job worker, hosted workspaces' worker, the
    /// change-feed followers, every feature crate's).
    fn start(self: Arc<Self>);
    /// The maintenance switch's pause (M141): background work holds while
    /// it's set.
    fn set_paused(&self, paused: bool);
    /// Stopping: no new job, and up to `within` for the held ones to
    /// checkpoint (M144).
    fn drain(&self, within: Duration) -> BoxFuture<'_, ()>;
    /// An account's personal project, for the `HubPort` calls the platform
    /// no longer asks (the hub answers them from the platform's own).
    fn personal_project<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<String>>>;
    /// A team's project, for the same.
    fn team_project<'a>(&'a self, team: &'a str) -> BoxFuture<'a, HubResult<Option<String>>>;
    /// The platform itself, for tests that reach into it.
    fn as_any(&self) -> &dyn Any;
    /// The same, to change before the hub starts.
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// The same, shared.
    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
}

/// What the hub gives the platform it makes in process.
pub struct Inputs {
    pub cfg: Config,
    /// The hub's `--db`: the platform's file, unless its flags name another
    /// database (`--platform-database-url`).
    pub db: PathBuf,
    /// The hub, as the platform reaches it.
    pub hub: Arc<dyn HubPort>,
    pub http: reqwest::Client,
    pub analytics: Analytics,
}

/// The platform's side of the hub's command line, from the binary.
pub trait Platforms: Send + Sync {
    /// The hub's command with the platform's flags added (the GitHub App's,
    /// the master key's, hosted workspaces', billing's, ...), with the names
    /// they always had.
    fn augment(&self, cmd: clap::Command) -> clap::Command;
    /// Those flags, as given.
    fn args(&self, m: &clap::ArgMatches) -> anyhow::Result<Box<dyn PlatformArgs>>;
    /// `arugula-control platform …`: a window's tools on the platform's
    /// tables (M141).
    fn window(&self, prog: &str, argv: Vec<String>) -> anyhow::Result<()>;
}

/// The platform's flags, as the hub reads the few it needs itself and makes
/// the platform from the rest.
pub trait PlatformArgs: Send {
    /// The GitHub App's id, when there's one.
    fn github_app_id(&self) -> Option<&str>;
    /// The GitHub App's slug.
    fn github_app_slug(&self) -> &str;
    /// Whether the App's private key or webhook secret is set (the
    /// platform's, M161: unused by a hub whose platform is its own).
    fn github_app_has_secrets(&self) -> bool;
    /// Whether billing is on: a Stripe key is set (and good).
    fn billing(&self) -> anyhow::Result<bool>;
    /// The public previews' domain flag (`ARUGULA_PREVIEW_DOMAIN`), as given.
    fn preview_domain(&self) -> Option<&str>;
    /// The platform in this process.
    fn make(self: Box<Self>, inputs: Inputs) -> anyhow::Result<Arc<dyn InProcess>>;
}
