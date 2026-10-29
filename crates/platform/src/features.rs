//! Feature crates (`arugula-platform-<area>`, #314): what each adds to the
//! platform, registered before it starts. Both binaries register every
//! one (`arugula-platform-all`), so the platform in the hub's process and
//! the `arugula-platform` binary are the same platform. The base calls
//! these hooks where it used to name the feature; a feature that isn't
//! registered adds nothing. A feature keeps what it holds in memory in
//! [`Slots`] on the [`Platform`].

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::{Arc, Mutex},
};

use arugula_control_common::{ApiError, err};
use arugula_control_wire::work::{ArtifactGiven, DeployWork, Reported, SecretsAsk, SecretsGiven};
use axum::{Router, http::StatusCode, routing::MethodRouter};
use futures_util::future::BoxFuture;
use serde_json::Value;

use crate::{
    Platform,
    auth::Machine,
    db::{Db, runs::Run},
    jobs::Worker,
    work::HostedMachine,
};

/// What a feature crate adds.
pub trait Feature: Send + Sync + 'static {
    /// Its routes, beside the base's ([`crate::router`], [`crate::paths`]).
    fn routes(&self, r: Routes) -> Routes {
        r
    }
    /// Its background work, once the platform starts (in the hub's process
    /// or as its own binary).
    fn start(&self, _p: &Arc<Platform>) {}
    /// Its kinds of job, for the one job worker (`work::start`).
    fn jobs(&self, _p: &Arc<Platform>, w: Worker) -> Worker {
        w
    }
}

static ALL: Mutex<Vec<&'static dyn Feature>> = Mutex::new(Vec::new());

/// Puts a feature in this platform: before it's made, by the binary (or a
/// test).
pub fn register(f: Box<dyn Feature>) {
    ALL.lock().unwrap().push(Box::leak(f));
}

pub(crate) fn all() -> Vec<&'static dyn Feature> {
    ALL.lock().unwrap().clone()
}

/// Every feature's background work ([`Feature::start`]).
pub fn start(p: &Arc<Platform>) {
    for f in all() {
        f.start(p);
    }
}

/// Every feature's kinds of job ([`Feature::jobs`]), with the worker's own.
pub fn jobs(p: &Arc<Platform>, w: Worker) -> Worker {
    all().into_iter().fold(w, |w, f| f.jobs(p, w))
}

/// Deploys (`arugula-platform-deploys`), as the rest of the platform asks
/// them: the runs engine for a deploy's run, a deploy box for its secrets
/// and artifact, agents with no machine for a plan. Without the crate, a
/// run is no deploy's and every ask is refused.
pub trait Deploys: Send + Sync + 'static {
    /// A deploy's run whose deploy's job gave up: its owners hear.
    fn page_unpaged<'a>(&'a self, _app: &'a Platform, _run: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async { Ok(()) })
    }
    /// What a deploy's run carries to the box that applies it.
    fn deploy_work(&self, _db: &Db, _run: &str) -> anyhow::Result<Option<DeployWork>> {
        Ok(None)
    }
    /// A deploy's run moved: its deploy moves too, and the box may be told
    /// otherwise.
    fn reported<'a>(&'a self, _app: &'a Platform, _run: &'a Run) -> BoxFuture<'a, anyhow::Result<Option<Reported>>> {
        Box::pin(async { Ok(None) })
    }
    /// A deploy box's secrets (`PlatformPort::hosted_secrets`).
    fn secrets<'a>(
        &'a self,
        _app: &'a Platform,
        _m: &'a HostedMachine,
        _ask: &'a SecretsAsk,
    ) -> BoxFuture<'a, anyhow::Result<SecretsGiven>> {
        Box::pin(async { Ok(SecretsGiven::Refused { why: "no deploys here".into() }) })
    }
    /// A deploy box's artifact (`PlatformPort::hosted_artifact`).
    fn artifact<'a>(
        &'a self,
        _app: &'a Platform,
        _m: &'a HostedMachine,
        _ask: &'a SecretsAsk,
    ) -> BoxFuture<'a, anyhow::Result<ArtifactGiven>> {
        Box::pin(async { Ok(ArtifactGiven::Refused { why: "no deploys here".into() }) })
    }
    /// A machine's agent plans a deploy (`POST /api/daemon/projects/{project}/deploys`).
    fn agent_plan(
        &self,
        _app: Arc<Platform>,
        _me: Machine,
        _project: String,
        _plan: Value,
    ) -> BoxFuture<'static, Result<Value, ApiError>> {
        Box::pin(async { Err(no_deploys()) })
    }
    /// A machine's agent asks for a deploy's approval.
    fn agent_request(
        &self,
        _app: Arc<Platform>,
        _me: Machine,
        _id: String,
    ) -> BoxFuture<'static, Result<Value, ApiError>> {
        Box::pin(async { Err(no_deploys()) })
    }
    /// A deploy, as a machine's agent may read it.
    fn agent_get(&self, _app: Arc<Platform>, _me: Machine, _id: String) -> BoxFuture<'static, Result<Value, ApiError>> {
        Box::pin(async { Err(no_deploys()) })
    }
}

fn no_deploys() -> ApiError {
    err(StatusCode::NOT_FOUND, "no deploys here")
}

struct NoDeploys;

impl Deploys for NoDeploys {}

static DEPLOYS: std::sync::OnceLock<Box<dyn Deploys>> = std::sync::OnceLock::new();

/// Puts deploys in this platform (`arugula-platform-deploys`'s `register`).
pub fn install_deploys(d: Box<dyn Deploys>) {
    let _ = DEPLOYS.set(d);
}

/// Deploys, as registered; refusing everything when they aren't.
pub fn deploys() -> &'static dyn Deploys {
    DEPLOYS.get().map_or(&NoDeploys, |d| d.as_ref())
}

/// A router that notes its paths too: routes.toml's test reads them.
#[derive(Default)]
pub struct Routes {
    pub(crate) router: Router<Arc<Platform>>,
    pub(crate) paths: Vec<&'static str>,
}

impl Routes {
    pub fn route(mut self, path: &'static str, methods: MethodRouter<Arc<Platform>>) -> Self {
        self.paths.push(path);
        self.router = self.router.route(path, methods);
        self
    }
}

/// What feature crates keep on the platform: one value of each type, put
/// while the platform is made and read from then on.
#[derive(Default)]
pub struct Slots(HashMap<TypeId, Box<dyn Any + Send + Sync>>);

impl Slots {
    pub fn put<T: Any + Send + Sync>(&mut self, v: T) {
        self.0.insert(TypeId::of::<T>(), Box::new(v));
    }

    pub fn get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.0.get(&TypeId::of::<T>()).and_then(|v| v.downcast_ref())
    }
}
