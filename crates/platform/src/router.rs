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

use crate::{Platform, billing, chat, cloud, grants, projects};

/// Every route the platform answers. Each moved here from `routes()` in
/// `crates/control/src/main.rs` with the same path and methods: billing's
/// (M139), Cloud's (M137), chat's (M138), then grants'; and the platform
/// page's own (M150).
fn routes() -> Routes {
    Routes::default()
        .route("/api/billing", get(billing::status))
        .route("/api/billing/checkout", post(billing::checkout))
        .route("/api/billing/report", post(billing::report_now))
        .route("/api/stripe/webhook", post(billing::webhook))
        .route("/api/cloud/secrets", get(cloud::list_secrets))
        .route("/api/cloud/secrets/{name}", axum::routing::put(cloud::put_secret).delete(cloud::delete_secret))
        // Past runs (S40, decision 10): read only. Nothing starts, cancels or
        // answers a run any more.
        .route("/api/cloud/runs", get(cloud::runs))
        .route("/api/cloud/runs/{id}", get(cloud::run))
        .route("/api/cloud/runs/{id}/requests", get(cloud::requests))
        .route("/api/cloud/runs/{id}/events", get(cloud::events))
        .route("/api/cloud/workspaces", get(cloud::workspaces::list).post(cloud::workspaces::create))
        .route(
            "/api/cloud/workspaces/{id}",
            get(cloud::workspaces::show).patch(cloud::workspaces::update).delete(cloud::workspaces::delete),
        )
        .route("/api/cloud/workspaces/{id}/files", get(cloud::files::files))
        .route("/api/cloud/workspaces/{id}/file", get(cloud::files::file))
        .route("/api/cloud/workspaces/{id}/diff", get(cloud::files::diff))
        .route("/api/cloud/workspaces/{id}/git-status", get(cloud::files::git_status))
        .route("/api/cloud/workspaces/{id}/setup-log", get(cloud::files::setup_log))
        .route("/api/cloud/workspaces/{id}/tabs", get(cloud::tabs::list).post(cloud::tabs::open))
        .route("/api/cloud/workspaces/{id}/tabs/{tab}", get(cloud::tabs::show).delete(cloud::tabs::close))
        .route("/api/cloud/workspaces/{id}/tabs/{tab}/prompt", post(cloud::tabs::prompt))
        .route("/api/cloud/workspaces/{id}/tabs/{tab}/interrupt", post(cloud::tabs::interrupt))
        .route("/api/cloud/workspaces/{id}/tabs/{tab}/events", get(cloud::tabs::events))
        .route("/api/cloud/workspaces/{id}/tabs/{tab}/requests", get(cloud::tabs::requests))
        .route("/api/cloud/workspaces/{id}/tabs/{tab}/requests/{request}", post(cloud::tabs::answer))
        .route("/api/cloud/workspaces/{id}/rebuild", post(cloud::rebuild::rebuild).delete(cloud::rebuild::cancel))
        .route("/api/cloud/workspaces/{id}/retry", post(cloud::rebuild::retry))
        .route("/api/cloud/past-tabs", get(cloud::tabs::past))
        .route("/api/cloud/past-tabs/{tab}", get(cloud::tabs::past_show))
        .route("/api/cloud/past-tabs/{tab}/events", get(cloud::tabs::past_events))
        .route("/api/cloud/past-tabs/{tab}/requests", get(cloud::tabs::past_requests))
        .route("/api/cloud/models", get(cloud::models))
        .route("/api/cloud/import", post(cloud::migration::import))
        .route("/api/cloud/agents", get(cloud::catalog).post(cloud::create_agent))
        .route("/api/cloud/agents/{id}", axum::routing::put(cloud::update_agent).delete(cloud::delete_agent))
        .route("/api/chat", get(chat::rooms))
        .route("/api/chat/live", get(chat::live))
        .route("/api/chat/live/open", post(chat::open))
        .route("/api/chat/search", get(chat::search))
        .route("/api/chat/targets", post(chat::open_target))
        .route("/api/chat/teams/{team}/channels", post(chat::new_channel))
        .route("/api/chat/teams/{team}/dms", post(chat::new_dm))
        .route("/api/chat/rooms/{id}", get(chat::room).patch(chat::change_room))
        .route("/api/chat/rooms/{id}/messages", get(chat::messages).post(chat::post))
        .route("/api/chat/rooms/{id}/messages/{seq}", axum::routing::patch(chat::edit).delete(chat::delete))
        .route("/api/chat/rooms/{id}/read", post(chat::read))
        .route("/api/chat/rooms/{id}/mute", post(chat::mute))
        .route(arugula_control_wire::chat::CHAT_READERS, post(chat::daemon_readers))
        .route(arugula_control_wire::chat::CHAT_IMPORT, post(chat::daemon_import))
        .route("/api/grants", get(grants::list).post(grants::give))
        .route("/api/grants/{id}/revoke", post(grants::revoke))
        .route(arugula_control_wire::projects::PROJECTS, get(projects::list))
}

/// The platform's router, for any state the hub's router has (the
/// platform's own is applied here).
pub fn router<S: Clone + Send + Sync + 'static>(platform: Arc<Platform>) -> Router<S> {
    // A call after a write in the hub reads the feed first (read your
    // writes: `cache::fresh`).
    routes()
        .router
        .layer(axum::middleware::from_fn_with_state(platform.clone(), crate::cache::fresh))
        // Outermost: the catch-up above is in the request's Server-Timing.
        .layer(axum::middleware::from_fn(crate::timing::layer))
        .with_state(platform)
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
