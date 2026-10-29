//! Deploys (#314, `arugula-platform-deploys`): a project's environments,
//! the deploys planned into them and their approval with a passkey
//! ([`deploys`], M217, M218), and applying an approved one on a box of its
//! own through the runs engine ([`deploys::apply`], M219). The drivers that
//! plan and check each kind of environment are the base's
//! (`arugula_platform::drivers`): agents with no machine and redone's
//! import ask them too.
//!
//! It sits on the platform's base, whose modules it names as the platform
//! did before the split (`crate::db`, `crate::drivers`, ...: the glob
//! below). The rest of the platform reaches it through
//! [`features::Deploys`]: the runs engine for a deploy's run, a deploy box
//! for its secrets and artifact, agents with no machine for a plan.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::work::{ArtifactGiven, DeployWork, Reported, SecretsAsk, SecretsGiven};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use futures_util::future::BoxFuture;
use serde_json::Value;

#[allow(unused_imports)]
use arugula_platform::*;

pub mod deploys;
#[cfg(any(test, feature = "testing"))]
pub mod deploys_tests;
#[cfg(test)]
mod failure_tests;

struct Feature;

impl features::Feature for Feature {
    fn routes(&self, r: features::Routes) -> features::Routes {
        r
            // Deploys and their approval (M218).
            .route(deploys::ENVIRONMENTS, get(deploys::environments).post(deploys::create_environment))
            .route(deploys::PROJECT_DEPLOYS, get(deploys::list).post(deploys::plan_route))
            .route(deploys::WAITING, get(deploys::waiting))
            .route(deploys::DEPLOY, get(deploys::get))
            .route(deploys::DEPLOY_REQUEST, post(deploys::request_route))
            .route(deploys::DEPLOY_APPROVE, post(deploys::approve))
            .route(deploys::DEPLOY_DENY, post(deploys::deny))
            .route(deploys::ENVIRONMENT_SECRET, post(deploys::set_secret))
            .route(deploys::catalog::PROJECT_DRIVERS, get(deploys::catalog::drivers))
            // A machine's agents plan and ask; never approve.
            .route(deploys::agent::AGENT_PROJECT_DEPLOYS, post(deploys::agent::plan))
            .route(deploys::agent::AGENT_DEPLOY, get(deploys::agent::get))
            .route(deploys::agent::AGENT_DEPLOY_REQUEST, post(deploys::agent::request))
    }

    fn start(&self, p: &Arc<Platform>) {
        // Deploy boxes past their life, taken down.
        deploys::apply::start_reaper(p.clone());
    }

    fn jobs(&self, p: &Arc<Platform>, w: jobs::Worker) -> jobs::Worker {
        // Deploys' applies (M219).
        w.with(deploys::apply::Apply { app: p.clone() })
    }
}

struct Hooks;

impl features::Deploys for Hooks {
    fn page_unpaged<'a>(&'a self, app: &'a Platform, run: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(deploys::apply::page_unpaged(app, run))
    }

    fn deploy_work(&self, db: &db::Db, run: &str) -> anyhow::Result<Option<DeployWork>> {
        deploys::apply::deploy_work(db, run)
    }

    fn reported<'a>(
        &'a self,
        app: &'a Platform,
        run: &'a db::runs::Run,
    ) -> BoxFuture<'a, anyhow::Result<Option<Reported>>> {
        Box::pin(deploys::apply::reported(app, run))
    }

    fn secrets<'a>(
        &'a self,
        app: &'a Platform,
        m: &'a work::HostedMachine,
        ask: &'a SecretsAsk,
    ) -> BoxFuture<'a, anyhow::Result<SecretsGiven>> {
        Box::pin(deploys::apply::secrets(app, m, ask))
    }

    fn artifact<'a>(
        &'a self,
        app: &'a Platform,
        m: &'a work::HostedMachine,
        ask: &'a SecretsAsk,
    ) -> BoxFuture<'a, anyhow::Result<ArtifactGiven>> {
        Box::pin(deploys::apply::artifact(app, m, ask))
    }

    fn agent_plan(
        &self,
        app: Arc<Platform>,
        me: auth::Machine,
        project: String,
        plan: Value,
    ) -> BoxFuture<'static, Result<Value, ApiError>> {
        Box::pin(async move {
            let plan = serde_json::from_value(plan).map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
            Ok(deploys::agent::plan(State(app), me, Path(project), Json(plan)).await?.0)
        })
    }

    fn agent_request(
        &self,
        app: Arc<Platform>,
        me: auth::Machine,
        id: String,
    ) -> BoxFuture<'static, Result<Value, ApiError>> {
        Box::pin(async move { Ok(deploys::agent::request(State(app), me, Path(id)).await?.0) })
    }

    fn agent_get(
        &self,
        app: Arc<Platform>,
        me: auth::Machine,
        id: String,
    ) -> BoxFuture<'static, Result<Value, ApiError>> {
        Box::pin(async move { Ok(deploys::agent::get(State(app), me, Path(id)).await?.0) })
    }
}

/// Puts deploys in this platform: the binaries do, before it's made
/// (`arugula-platform-all`). Once.
pub fn register() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        features::register(Box::new(Feature));
        features::install_deploys(Box::new(Hooks));
    });
}

/// The base's agents rig, with this crate registered before its router is
/// made: what the tests here call as `crate::agents_tests`.
#[cfg(test)]
mod agents_tests {
    pub use arugula_platform::agents_tests::*;

    pub async fn seeded()
    -> (std::sync::Arc<crate::chat_tests::TestHub>, std::sync::Arc<crate::Platform>, String, String, String) {
        crate::register();
        arugula_platform::agents_tests::seeded().await
    }
}

/// The base's test router, with this crate registered before it's made:
/// what the tests here call as `crate::testing::router`.
#[cfg(test)]
mod testing {
    pub fn router(app: std::sync::Arc<crate::Platform>) -> axum::Router {
        crate::register();
        arugula_platform::testing::router(app)
    }
}
