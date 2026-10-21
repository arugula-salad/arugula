//! Arugula control's platform ("Control is two processes: a hub and a
//! platform", #1150): chat, Cloud and billing, and step 1's projects and
//! grants, which deploy as often as they like without dropping a relayed
//! terminal. The hub (`arugula-control`) keeps identity, devices, rosters,
//! the relay, push, block and doc origins, the jump host, forge and
//! telemetry, and stays the gateway: every platform path stays on control's
//! own site ("Platform paths stay on control.arugula.io forever").
//!
//! The seam:
//! - [`HubPort`] is everything the platform asks the hub. The platform
//!   never reads a hub table: it has its own database handle ([`db::Db`]),
//!   which in process is a second connection to the hub's file.
//! - [`PlatformPort`] is everything the hub asks the platform (deleting an
//!   account, whether one pays, whether chat is on for it).
//! - [`auth`] is who's asking, as the hub found out: a [`Principal`].
//! - [`router()`] is the platform's routes, which the hub mounts in process.
//!
//! Today it runs inside the hub's process. Billing moved in first (M139),
//! then Cloud ([`cloud`], M137), then chat ([`chat`], M138), which follows
//! the hub's change feed through [`cache`], then projects and grants
//! ([`projects`], [`grants`]), which follow it too. M140 runs it as its own
//! process behind the hub.
//!
//! [`Principal`]: arugula_control_common::Principal

use std::sync::Arc;

use arugula_control_common::{analytics::Analytics, limit::Limits};

pub mod auth;
pub mod billing;
pub mod cache;
pub mod chat;
#[cfg(test)]
mod chat_tests;
pub mod cloud;
#[cfg(feature = "contract")]
pub mod contract;
pub mod db;
pub mod grants;
pub mod hub;
mod port;
pub mod projects;
mod router;

pub use hub::HubPort;
pub use port::{Deleted, PlatformPort};
pub use router::{paths, router};

/// What the platform is told when it starts.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// The URL people reach control at (the hub's first site), without a
    /// trailing slash.
    pub public_url: String,
    /// Every URL control answers at, `public_url` first (#507), without a
    /// trailing slash.
    pub sites: Vec<String>,
    /// Free relay traffic per account per month, with billing on.
    pub relay_free_bytes: u64,
}

impl Config {
    /// The site a request came in on, by its Host header; the first when
    /// it's none of them.
    pub fn site(&self, headers: &axum::http::HeaderMap) -> &str {
        let host = headers.get(axum::http::header::HOST).and_then(|h| h.to_str().ok()).map(str::to_ascii_lowercase);
        let authority = |s: &str| {
            let u = url::Url::parse(s).ok()?;
            let host = u.host_str()?.to_ascii_lowercase();
            Some(u.port().map_or(host.clone(), |p| format!("{host}:{p}")))
        };
        host.and_then(|h| self.sites.iter().find(|s| authority(s).as_deref() == Some(h.as_str())))
            .map_or(&self.public_url, |s| s)
    }
}

/// The platform: its own database, its view of the hub, and what it keeps
/// in memory. Each feature that moved in brought its own fields, as `App`
/// had them: billing's `stripe`, Cloud's `cloud`, chat's `chat`, and
/// projects' `feed`.
pub struct Platform {
    pub cfg: Config,
    pub db: db::Db,
    /// The hub, and nothing else of it.
    pub hub: Arc<dyn HubPort>,
    pub http: reqwest::Client,
    /// The platform's own rate limits: none of them is counted by the hub.
    pub limits: Limits,
    /// Product analytics: in process, a handle on the hub's queue.
    pub analytics: Analytics,
    /// Billing (M22), when there's a Stripe key: `billing`.
    pub stripe: Option<billing::Stripe>,
    /// Cloud workspaces (M137), when this control has a Managoat key.
    pub cloud: Option<cloud::Cloud>,
    /// Chat's live streams and recent events (M122, here since M138).
    pub chat: chat::Hub,
    /// The hub's answers chat asks for, following its change feed (M138).
    pub cache: cache::Cache,
    /// Held while [`projects::follow`] applies the hub's change feed to
    /// projects and grants: one reader at a time.
    pub feed: tokio::sync::Mutex<()>,
}

impl Platform {
    pub fn new(cfg: Config, db: db::Db, hub: Arc<dyn HubPort>, http: reqwest::Client, analytics: Analytics) -> Self {
        Platform {
            cfg,
            db,
            hub,
            http,
            limits: Limits::new(None),
            analytics,
            stripe: None,
            cloud: None,
            chat: Default::default(),
            cache: Default::default(),
            feed: Default::default(),
        }
    }
}
