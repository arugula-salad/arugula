//! The platform's routes. Each is listed in `crates/control/routes.toml`
//! with `process = "platform"`, and its test reads the paths from
//! [`paths`]. The paths stay what they are on control's site: in process
//! the hub mounts this router behind its `hub_port::gate`, and out of
//! process (M140) it forwards each platform path here.

use std::sync::Arc;

use axum::{
    Router,
    routing::{MethodRouter, get, post},
};

use crate::{Platform, billing};

/// Every route the platform answers. Each moved here from `routes()` in
/// `crates/control/src/main.rs` with the same path and methods: billing's
/// (M139), then Cloud's and chat's (M137, M138).
fn routes() -> Routes {
    Routes::default()
        .route("/api/billing", get(billing::status))
        .route("/api/billing/checkout", post(billing::checkout))
        .route("/api/billing/report", post(billing::report_now))
        .route("/api/stripe/webhook", post(billing::webhook))
}

/// The platform's router, for any state the hub's router has (the
/// platform's own is applied here).
pub fn router<S: Clone + Send + Sync + 'static>(platform: Arc<Platform>) -> Router<S> {
    routes().router.with_state(platform)
}

/// The paths [`router`] registers, for routes.toml's test.
pub fn paths() -> Vec<&'static str> {
    routes().paths
}

/// A router that notes its paths too.
#[derive(Default)]
struct Routes {
    router: Router<Arc<Platform>>,
    paths: Vec<&'static str>,
}

impl Routes {
    fn route(mut self, path: &'static str, methods: MethodRouter<Arc<Platform>>) -> Self {
        self.paths.push(path);
        self.router = self.router.route(path, methods);
        self
    }
}
