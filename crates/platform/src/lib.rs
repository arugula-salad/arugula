//! Arugula control's platform ("Control is two processes: a hub and a
//! platform", #1150): chat, hosted workspaces and billing, and step 1's projects and
//! grants, which deploy as often as they like without dropping a relayed
//! terminal. The hub (`arugula-control`) keeps identity, devices, rosters,
//! the relay, push, block and doc origins, the jump host, daemons' forge
//! subscriptions and pokes (until M174) and telemetry, and stays the
//! gateway: every platform path stays on control's own site ("Platform
//! paths stay on control.arugula.io forever").
//!
//! The seam:
//! - [`HubPort`] is everything the platform asks the hub. The platform
//!   never reads a hub table: it has its own database handle ([`db::Db`]),
//!   which in process is a second connection to the hub's file.
//! - [`PlatformPort`] is everything the hub asks the platform (deleting an
//!   account, whether one pays, whether chat is on for it).
//! - [`auth`] is who's asking, as the hub found out: a [`Principal`].
//! - [`router()`] is the platform's routes, which the hub mounts in process.
//! - [`page`] is the platform page (M150's build), which the hub serves on
//!   the platform's own host, `app.arugula.io` (M151).
//!
//! Billing moved in first (M139), then Cloud (M137, Managoat-backed until
//! M189 and M190 removed it), then chat
//! ([`chat`], M138), which follows the hub's change feed through [`cache`],
//! then projects and grants ([`projects`], [`grants`]), which follow it
//! too, then the GitHub App's webhook and key ([`forge`], M161). It runs inside the hub's process (`--platform in-process`, the
//! default) or as its own, the `arugula-platform` binary (M140), behind the
//! hub's gateway: then both ports go over the private network ([`remote`]).
//!
//! [`Principal`]: arugula_control_common::Principal

use std::sync::Arc;

use arugula_control_common::{analytics::Analytics, limit::Limits};

#[cfg(test)]
mod agent_issues_tests;
pub mod agent_mcp;
#[cfg(test)]
mod agent_mcp_tests;
#[cfg(test)]
mod agent_threads_tests;
pub mod agent_tokens;
#[cfg(test)]
mod agents_tests;
pub mod auth;
pub mod billing;
pub mod boxes;
#[cfg(test)]
mod boxes_tests;
pub mod cache;
pub mod chat;
#[cfg(test)]
mod chat_tests;
pub mod clients;
#[cfg(test)]
mod clients_tests;
pub mod cloud;
pub mod compute;
#[cfg(feature = "contract")]
pub mod contract;
pub mod db;
pub mod deploys;
#[cfg(test)]
mod deploys_tests;
pub mod drivers;
#[cfg(test)]
mod failure_tests;
pub mod forge;
pub mod grants;
pub mod hosted;
pub mod hub;
#[cfg(test)]
mod hub_calls_tests;
pub mod inbox;
pub mod infisical;
pub mod issues;
pub mod jobs;
pub mod keys;
pub mod links;
#[cfg(test)]
mod object_threads_tests;
pub mod page;
pub mod planter;
mod port;
pub mod projects;
pub mod redone;
pub mod remote;
#[cfg(test)]
mod restart_tests;
mod router;
pub mod run_grant;
#[cfg(test)]
mod run_grant_tests;
pub mod run_log;
pub mod run_logs;
#[cfg(test)]
mod runs_page_tests;
pub mod secrets;
pub mod stuck;
#[cfg(test)]
pub(crate) mod testing;
pub mod timing;
pub mod triggers;
pub mod window;
pub mod work;

pub use hub::HubPort;
pub use port::{Archived, Deleted, PlatformPort, PublicPreview, TurnAnswer};
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
/// in memory (on each of its machines, M144: what's kept here is that
/// machine's alone, as docs/control-ops.md lists). Each feature that moved in brought its own fields, as `App`
/// had them: billing's `stripe`, chat's `chat`, and projects' `feed`.
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
    /// Hosted workspaces' boxes (M176), when this platform has an E2B key:
    /// [`hosted`]'s worker drives them (M179).
    pub compute: Option<Arc<dyn compute::Substrate>>,
    /// Hosted workspaces' template, how their boxes enroll, and who may make
    /// them (M179).
    pub hosted: hosted::Config,
    /// The master key (`ARUGULA_CLOUD_SECRET_KEY`, and the old one while
    /// it's rotated), which wraps each project's data key ([`keys`]).
    pub secret_key: Option<Arc<secrets::SecretCipher>>,
    /// Wakes asked of hosted workspaces' boxes (M182): one at a time per
    /// workspace, and the worker started on them at once.
    pub wakes: hosted::wake::Wakes,
    /// The GitHub App (M40, here since M161), when this control has its id
    /// and key: its webhook, `may()` and installation tokens.
    pub github: Option<forge::GithubApp>,
    /// planter's GitHub App, the deploy App (M233, aru-108), when this
    /// platform has its id and key: it mints the tokens planter's drivers'
    /// runs get, and nothing else.
    pub deploy_app: Option<forge::GithubApp>,
    /// Chat's live streams and recent events (M122, here since M138).
    pub chat: chat::Hub,
    /// The hub's answers chat asks for, following its change feed (M138).
    pub cache: cache::Cache,
    /// Held while [`projects::follow`] applies the hub's change feed to
    /// projects and grants: one reader at a time.
    pub feed: tokio::sync::Mutex<()>,
    /// Background work held for a window (M141): the job worker, hosted
    /// workspaces' worker, billing's usage reports and the projects and
    /// grants feed follower wait while it's set, so
    /// the platform's tables hold still while they're copied. Requests are
    /// held by the hub's maintenance switch, not this.
    pub paused: std::sync::atomic::AtomicBool,
    /// The platform's other machines, on Postgres (M144): their
    /// notifications, and whether this machine is the leader
    /// ([`Platform::is_leader`]). `None` on SQLite: one machine.
    pub listening: Option<db::Listening>,
    /// Set when the machine is stopping (a deploy): the job worker takes no
    /// new job, and [`Platform::drain`] waits for the ones it holds.
    pub stopping: std::sync::atomic::AtomicBool,
    /// Jobs this machine holds a lease on right now.
    pub busy: std::sync::atomic::AtomicUsize,
    /// The linked repos' items whose inbox to work out again (M171), as
    /// this machine's ingest wrote them, and its pushes lately.
    pub inbox: forge::inbox::Queue,
    /// The run-log bucket (M213), when this platform has one: hosted runs'
    /// logs and artifacts go there.
    pub run_logs: Option<run_logs::Bucket>,
    /// Short-lived tokens minted for deploys' runs (M219), revoked when
    /// each run ends: this machine's, in memory only.
    pub deploy_tokens: drivers::Held,
    /// The Infisical projection (M235): off unless the platform binary is
    /// told otherwise (`ARUGULA_INFISICAL_PROJECTION`).
    pub projection: infisical::Projection,
}

impl Platform {
    /// Whether background work is held ([`Platform::paused`]).
    pub fn is_paused(&self) -> bool {
        self.paused.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Whether this machine is the platform's leader (M144): the one that
    /// applies the hub's change feed to projects and grants and reports
    /// billing's usage. On Postgres, the machine whose listening connection
    /// holds the `leader` lock (`pg_try_advisory_lock`); another takes it
    /// within [`db::pg::LEADER_TRY`] of that connection closing. On SQLite,
    /// always: there's one machine.
    pub fn is_leader(&self) -> bool {
        self.listening.as_ref().is_none_or(|l| l.leader.load(std::sync::atomic::Ordering::SeqCst))
    }

    /// Whether the machine is stopping ([`Platform::stopping`]).
    pub fn is_stopping(&self) -> bool {
        self.stopping.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The machine is stopping: take no new job, and wait up to
    /// `within` for the ones held to checkpoint, which hands each back
    /// within seconds instead of at the end of its lease (M144: a rolling
    /// deploy's machine hands its jobs to the other).
    pub async fn drain(&self, within: std::time::Duration) {
        use std::sync::atomic::Ordering::SeqCst;
        self.stopping.store(true, SeqCst);
        let until = std::time::Instant::now() + within;
        while self.busy.load(SeqCst) > 0 && std::time::Instant::now() < until {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let left = self.busy.load(SeqCst);
        if left > 0 {
            tracing::warn!(jobs = left, "stopping with jobs held: each waits out its lease");
        }
    }
}

impl Platform {
    pub fn new(cfg: Config, db: db::Db, hub: Arc<dyn HubPort>, http: reqwest::Client, analytics: Analytics) -> Self {
        let chat = chat::Hub::default();
        chat.start_at(&db);
        let listening = db.listen();
        Platform {
            cfg,
            db,
            // Every hub call a request makes is in its Server-Timing.
            hub: Arc::new(timing::Timed(hub)),
            http,
            limits: Limits::new(None),
            analytics,
            stripe: None,
            compute: None,
            hosted: Default::default(),
            secret_key: None,
            wakes: Default::default(),
            github: None,
            deploy_app: None,
            chat,
            cache: Default::default(),
            feed: Default::default(),
            paused: Default::default(),
            listening,
            stopping: Default::default(),
            busy: Default::default(),
            inbox: Default::default(),
            run_logs: None,
            deploy_tokens: Default::default(),
            projection: Default::default(),
        }
    }
}
