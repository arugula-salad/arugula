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
//! Today it runs inside the hub's process and holds nothing yet: Cloud
//! (M137), chat (M138) and billing (M139) move in next, each into its own
//! module here. M140 runs it as its own process behind the hub.
//!
//! [`Principal`]: arugula_control_common::Principal

use std::sync::Arc;

use arugula_control_common::{analytics::Analytics, limit::Limits};

pub mod auth;
pub mod billing;
pub mod chat;
pub mod cloud;
#[cfg(feature = "contract")]
pub mod contract;
pub mod db;
pub mod hub;
mod port;
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
}

/// The platform: its own database, its view of the hub, and what it keeps
/// in memory. The features moving in add their own fields (`cloud`,
/// `chat`, `stripe`), as `App` has them now.
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
}

impl Platform {
    pub fn new(cfg: Config, db: db::Db, hub: Arc<dyn HubPort>, http: reqwest::Client, analytics: Analytics) -> Self {
        Platform { cfg, db, hub, http, limits: Limits::new(None), analytics }
    }
}
