//! The platform in the hub's process (`--platform in-process`, #314): the
//! flags the hub takes for it and how it's made from them
//! ([`HubPlatforms`], which the `arugula-control` binary hands the hub), and
//! [`Platform`] as the hub holds it (`arugula_platform_api::in_process`).
//! The hub's library names none of this: it builds against
//! `arugula-platform-api` alone.

use std::{any::Any, sync::Arc, time::Duration};

use arugula_platform_api::in_process::{InProcess, Inputs, PlatformArgs, Platforms};
use futures_util::future::BoxFuture;

use crate::{Platform, hub::HubResult};

/// The platform's flags on the hub, with the names they always had: the
/// GitHub App's (M161), the master key and the beta allowlist (M137),
/// hosted workspaces' boxes (M176) and the rest of hosted workspaces
/// (M179), the run-log bucket (M213), the platform's own database (M143)
/// and billing (M140).
#[derive(clap::Args, Debug, Clone)]
pub struct HubArgs {
    /// The GitHub App (M40): its flags are the platform's (M161), with the
    /// names they always had. In process the hub hands the App to the
    /// platform; out of process it needs only the id and slug, and the
    /// platform holds the key and webhook secret (docs/control-ops.md).
    #[command(flatten)]
    pub github_app: crate::forge::Args,
    /// The master key and the beta allowlist (M137).
    #[command(flatten)]
    pub cloud: crate::cloud::Args,
    /// Hosted workspaces' boxes (M176), used when it runs in process.
    #[command(flatten)]
    pub compute: crate::compute::Args,
    /// Hosted workspaces (M179), used when it runs in process.
    #[command(flatten)]
    pub hosted: crate::hosted::Args,
    /// The run-log bucket (M213), used when it runs in process.
    #[command(flatten)]
    pub run_logs: crate::run_logs::Args,
    /// The platform's own database (M143): `postgres://` puts it on
    /// Postgres. Without it, a second connection to `--db`, as always.
    #[command(flatten)]
    pub platform_db: crate::db::Args,
    /// Billing (M22): the hub only needs to know it's on.
    #[command(flatten)]
    pub billing: crate::billing::Args,
    /// GitHub's API (tests point it at a fake): the hub's `--github-api`.
    #[arg(skip)]
    pub github_api: String,
}

/// What the `arugula-control` binary hands the hub.
pub struct HubPlatforms;

impl Platforms for HubPlatforms {
    fn augment(&self, cmd: clap::Command) -> clap::Command {
        <HubArgs as clap::Args>::augment_args(cmd)
    }

    fn args(&self, m: &clap::ArgMatches) -> anyhow::Result<Box<dyn PlatformArgs>> {
        let mut a = <HubArgs as clap::FromArgMatches>::from_arg_matches(m)?;
        // The hub's own flag, which the App's calls use too.
        a.github_api = m.get_one::<String>("github_api").cloned().unwrap_or_else(|| "https://api.github.com".into());
        Ok(Box::new(a))
    }

    fn window(&self, prog: &str, argv: Vec<String>) -> anyhow::Result<()> {
        crate::window::run(prog, argv.into_iter())
    }
}

impl PlatformArgs for HubArgs {
    fn github_app_id(&self) -> Option<&str> {
        self.github_app.id()
    }

    fn github_app_slug(&self) -> &str {
        &self.github_app.github_app_slug
    }

    fn github_app_has_secrets(&self) -> bool {
        self.github_app.has_secrets()
    }

    fn billing(&self) -> anyhow::Result<bool> {
        Ok(self.billing.clone().stripe()?.is_some())
    }

    fn preview_domain(&self) -> Option<&str> {
        self.hosted.preview_domain.as_deref()
    }

    fn make(self: Box<Self>, i: Inputs) -> anyhow::Result<Arc<dyn InProcess>> {
        let a = *self;
        let hosted = a.hosted.config(&a.cloud)?;
        let stripe = a.billing.stripe()?;
        let github = (a.github_app.app(&a.github_api)?, a.github_app.deploy_app(&a.github_api)?);
        let compute = a.compute.substrate()?;
        let run_logs = a.run_logs.bucket()?;
        // A second connection to the same file: the platform never reads
        // the hub's tables (#1153).
        let db = a.platform_db.open(&i.db)?;
        let mut p = Platform::new(i.cfg, db, i.hub, i.http, i.analytics);
        p.stripe = stripe;
        // The platform's own App, and planter's, the deploy App (M233).
        (p.github, p.deploy_app) = github;
        p.compute = compute;
        // The master key (M209), which the hosted config was given.
        p.secret_key = hosted.cipher.clone();
        p.hosted = hosted;
        p.run_logs = run_logs;
        Ok(Arc::new(p))
    }
}

/// The platform's background work, in the hub's process or its own: the
/// base's, then every feature crate's.
pub fn start(p: &Arc<Platform>) {
    crate::hosted::start(p);
    crate::keys::start(p);
    // The job worker: runs' dispatch (M212) and schedules (M214).
    crate::work::start(p);
    // Runs' logs past their days (M213).
    crate::run_logs::start(p);
    // Chat follows the hub's change feed (M138), and so do projects and
    // grants: a machine that joins gets its `cloud.drive` grant from it.
    crate::chat::start(p);
    crate::projects::start(p);
    crate::billing::start(p);
    crate::forge::start(p);
    // The feature crates' (#314).
    crate::features::start(p);
}

impl InProcess for Platform {
    fn router(self: Arc<Self>) -> axum::Router {
        crate::router(self)
    }

    fn start(self: Arc<Self>) {
        start(&self);
    }

    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, std::sync::atomic::Ordering::Relaxed);
    }

    fn drain(&self, within: Duration) -> BoxFuture<'_, ()> {
        Box::pin(Platform::drain(self, within))
    }

    fn personal_project<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        Box::pin(crate::projects::personal_project(self, account))
    }

    fn team_project<'a>(&'a self, team: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        Box::pin(crate::projects::team_project(self, team))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}
