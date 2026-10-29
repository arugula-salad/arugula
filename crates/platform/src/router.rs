//! The platform's routes. Each is listed in `crates/control/routes.toml`
//! with `process = "platform"`, and its test reads the paths from
//! [`paths`]. The paths stay what they are on control's site: in process
//! the hub mounts this router behind its `hub_port::gate`, and out of
//! process (M140) it forwards each platform path here.

use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post, put},
};

use arugula_control_wire::{boards, issues, project_docs, runs as wire_runs};

use crate::{
    Platform, billing,
    features::{self, Routes},
    forge, grants, hosted, projects, run_grant, run_logs,
    triggers::{agent as run_agents, routes as runs},
};

/// Every route the platform answers. Each moved here from `routes()` in
/// `crates/control/src/main.rs` with the same path and methods: billing's
/// (M139), chat's (M138), then grants'; the platform
/// page's own (M150); the GitHub App's webhook (M161); linked repos
/// (M163); their items, each read checked against the viewer (M165);
/// drafts on them (M170); the inbox (M171); native issues' (M195), for
/// agents too (M200); hosted workspaces (M179) and their inference keys
/// (M185), and the inference broker (M231); the standing project grant for runs (M211); what starts runs
/// (M214); boards (step 1 of the status board); reading runs, and a machine's agents' runs (M216); runs'
/// artifacts (M213); hosted agents' archived transcripts (M184); agents
/// with no machine, their grant tokens and MCP endpoint (M230); the
/// platform page's linked products (M229); and studio as a client (M236).
fn routes() -> Routes {
    Routes::default()
        .route("/api/billing", get(billing::status))
        .route("/api/billing/checkout", post(billing::checkout))
        .route("/api/billing/report", post(billing::report_now))
        .route("/api/stripe/webhook", post(billing::webhook))
        // Notification levels and do not disturb (#113, #115): chat's
        // pushes and the platform's other notices.
        .route("/api/notify/prefs", get(crate::notify::prefs).post(crate::notify::set_prefs))
        .route("/api/grants", get(grants::list).post(grants::give))
        .route("/api/grants/{id}/revoke", post(grants::revoke))
        .route(arugula_control_wire::projects::PROJECTS, get(projects::list))
        .route(arugula_control_wire::projects::PROJECT_REPOS, get(forge::links::list).post(forge::links::link))
        .route(
            "/api/projects/{project}/repos/{owner}/{name}",
            axum::routing::delete(forge::links::unlink).patch(forge::content::settings),
        )
        .route(arugula_control_wire::projects::PROJECT_ITEMS, get(forge::items::list))
        .route(arugula_control_wire::projects::FORGE_ITEM, get(forge::items::get))
        .route(arugula_control_wire::projects::FORGE_ITEM_CHECKS, get(forge::items::checks))
        .route(arugula_control_wire::projects::FORGE_ITEM_EVENTS, get(forge::items::events))
        .route(arugula_control_wire::projects::FORGE_ITEM_CONTENT, get(forge::content::get))
        .route(arugula_control_wire::projects::PROJECT_DRAFTS, get(forge::drafts::project))
        .route(arugula_control_wire::projects::FORGE_ITEM_DRAFTS, get(forge::drafts::item))
        .route(arugula_control_wire::projects::FORGE_DRAFTS, get(forge::drafts::mine).post(forge::drafts::create))
        .route(arugula_control_wire::projects::FORGE_DRAFT, get(forge::drafts::get))
        .route(arugula_control_wire::projects::FORGE_DRAFT_DISCARD, post(forge::drafts::discard))
        .route(arugula_control_wire::projects::FORGE_DRAFT_SEND, post(forge::send::route))
        .route(arugula_control_wire::projects::FORGE_DRAFT_MACHINE, post(forge::send::machine))
        .route(arugula_control_wire::projects::FORGE_REPO, get(forge::drafts::repo))
        .route(arugula_control_wire::forge_ops::FORGE_OPS, post(forge::ops::call))
        .route(arugula_control_wire::projects::INBOX, get(forge::inbox::list))
        .route(arugula_control_wire::projects::INBOX_SHADOW, post(forge::inbox::shadow))
        .route(arugula_control_wire::projects::INBOX_ITEM, post(forge::inbox::mark))
        // Native issues (M195).
        .route(issues::PROJECT_ISSUES, get(crate::issues::list_route).post(crate::issues::create_route))
        .route(issues::PROJECT_ISSUES_EXPORT, get(crate::issues::export_route))
        .route(issues::PROJECT_ISSUE, get(crate::issues::by_number_route))
        .route(issues::PROJECT_ISSUE_HOME, get(crate::issues::home_route).put(crate::issues::set_home_route))
        .route(issues::ISSUE, get(crate::issues::get_route).patch(crate::issues::patch_route))
        .route(issues::ISSUE_EVENTS, get(crate::issues::events_route))
        // A machine's agents on them (M200).
        .route(issues::AGENT_ISSUE_HOME, get(crate::issues::agent::home))
        .route(issues::AGENT_PROJECT_ISSUES, get(crate::issues::agent::list).post(crate::issues::agent::create))
        .route(issues::AGENT_PROJECT_ISSUE, get(crate::issues::agent::by_number))
        .route(issues::AGENT_ISSUE, get(crate::issues::agent::get).patch(crate::issues::agent::patch))
        .route(issues::AGENT_ISSUE_EVENTS, get(crate::issues::agent::events))
        // Boards, behind the `boards` flag.
        .route(boards::PROJECT_BOARDS, get(crate::boards::list_route).post(crate::boards::create_route))
        .route(boards::PROJECT_WORK, get(crate::boards::work::route))
        .route(boards::BOARD, get(crate::boards::get_route).patch(crate::boards::edit_route))
        .route(boards::BOARD_CLOSE, post(crate::boards::close_route))
        .route(boards::BOARD_REOPEN, post(crate::boards::reopen_route))
        .route(boards::BOARD_ENTRIES, put(crate::boards::put_route))
        .route(boards::BOARD_ENTRY, axum::routing::delete(crate::boards::delete_route))
        .route(boards::BOARD_ENTRY_MOVE, post(crate::boards::move_route))
        .route(boards::AGENT_PROJECT_BOARDS, get(crate::boards::agent::list).post(crate::boards::agent::create))
        .route(boards::AGENT_BOARD, get(crate::boards::agent::get).patch(crate::boards::agent::edit))
        .route(boards::AGENT_BOARD_CLOSE, post(crate::boards::agent::close))
        .route(boards::AGENT_BOARD_REOPEN, post(crate::boards::agent::reopen))
        .route(boards::AGENT_BOARD_ENTRIES, put(crate::boards::agent::put))
        .route(boards::AGENT_BOARD_ENTRY, axum::routing::delete(crate::boards::agent::delete))
        .route(boards::AGENT_BOARD_ENTRY_MOVE, post(crate::boards::agent::move_entry))
        .route(boards::AGENT_BLOCK_STATE, put(crate::boards::agent::put_block_state))
        // Project Docs (aru-118), with chat.
        .route(project_docs::PROJECT_DOCS, get(crate::project_docs::list_route).post(crate::project_docs::create_route))
        .route(
            project_docs::PROJECT_DOC,
            get(crate::project_docs::get_route)
                .put(crate::project_docs::put_route)
                .delete(crate::project_docs::delete_route),
        )
        .route("/github/webhook", post(forge::webhook))
        .route("/api/projects/{project}/workspaces", get(hosted::routes::list).post(hosted::routes::create))
        .route("/api/projects/{project}/workspaces/{id}", get(hosted::routes::open).delete(hosted::routes::delete))
        .route("/api/projects/{project}/workspaces/{id}/rebuild", post(hosted::routes::rebuild))
        .route("/api/projects/{project}/workspaces/{id}/ports", get(hosted::previews::list))
        .route(
            "/api/projects/{project}/workspaces/{id}/ports/{port}/public",
            axum::routing::put(hosted::previews::make_public).delete(hosted::previews::make_private),
        )
        // The inference broker (M231), and what its calls cost.
        .route("/api/projects/{project}/inference/anthropic/{*rest}", axum::routing::any(hosted::broker::forward))
        .route("/api/projects/{project}/inference/spend", get(hosted::broker::project_spend))
        .route("/api/inference/spend", get(hosted::broker::my_spend))
        .route("/api/inference/spend/paid", get(hosted::broker::paid_by_me))
        .route("/api/inference/clients", get(hosted::broker::list_clients))
        .route(
            "/api/inference/clients/{client}/consent",
            axum::routing::put(hosted::broker::set_consent).delete(hosted::broker::set_consent),
        )
        .route("/api/projects/{project}/inference-keys", get(hosted::inference::list_keys))
        .route(
            "/api/projects/{project}/inference-keys/{provider}",
            axum::routing::put(hosted::inference::set_key).delete(hosted::inference::clear_key),
        )
        // The standing project grant (M211).
        .route("/api/projects/{project}/runs/grant", get(run_grant::show).post(run_grant::sign))
        .route("/api/projects/{project}/runs/grant/revoke", post(run_grant::revoke))
        .route("/api/projects/{project}/runs/start", post(run_grant::start))
        // What starts runs (M214).
        .route("/api/projects/{project}/run-specs", get(runs::list).post(runs::create))
        .route("/api/projects/{project}/run-specs/{spec}", get(runs::show).patch(runs::update).delete(runs::delete))
        .route("/api/projects/{project}/run-specs/{spec}/schedules", post(runs::add_schedule))
        .route(
            "/api/projects/{project}/run-specs/{spec}/schedules/{id}",
            axum::routing::patch(runs::change_schedule).delete(runs::delete_schedule),
        )
        .route("/api/projects/{project}/run-specs/{spec}/rules", post(runs::add_rule))
        .route(
            "/api/projects/{project}/run-specs/{spec}/rules/{id}",
            axum::routing::patch(runs::change_rule).delete(runs::delete_rule),
        )
        .route("/api/projects/{project}/runs", get(crate::triggers::read::list).post(runs::start))
        // Reading runs, and a machine's agents' runs (M216).
        .route(wire_runs::RUN, get(crate::triggers::read::show))
        .route(wire_runs::RUN_LOG, get(crate::triggers::read::log))
        .route(wire_runs::AGENT_RUN_REACH, get(run_agents::reach))
        .route(wire_runs::AGENT_PROJECT_RUNS, get(run_agents::list).post(run_agents::start))
        .route(wire_runs::AGENT_RUN, get(run_agents::show))
        .route(wire_runs::AGENT_RUN_LOG, get(run_agents::log))
        .route("/api/projects/{project}/workspaces/{id}/spec", axum::routing::put(hosted::routes::set_spec))
        .route("/api/projects/{project}/workspaces/{id}/secrets", get(hosted::routes::secrets))
        .route(
            "/api/projects/{project}/workspaces/{id}/secrets/{name}",
            axum::routing::put(hosted::routes::put_secret).delete(hosted::routes::delete_secret),
        )
        .route("/api/workspaces", get(hosted::routes::all))
        .route("/api/projects/{project}/transcripts", get(hosted::transcripts::list))
        .route("/api/projects/{project}/transcripts/{id}", get(hosted::transcripts::read))
        // Runs' artifacts (M213); their logs are read through RUN_LOG above.
        .route("/api/projects/{project}/runs/{run}/artifacts", get(run_logs::routes::list))
        .route("/api/projects/{project}/runs/{run}/artifacts/{name}", get(run_logs::routes::artifact))
}

/// The base's routes and every registered feature crate's
/// ([`features::Feature::routes`]).
fn all_routes() -> Routes {
    features::all().into_iter().fold(routes(), |r, f| f.routes(r))
}

/// The platform's router, for any state the hub's router has (the
/// platform's own is applied here).
pub fn router<S: Clone + Send + Sync + 'static>(platform: Arc<Platform>) -> Router<S> {
    // A call after a write in the hub reads the feed first (read your
    // writes: `cache::fresh`).
    all_routes()
        .router
        .layer(axum::middleware::from_fn_with_state(platform.clone(), crate::cache::fresh))
        // Outermost: the catch-up above is in the request's Server-Timing.
        .layer(axum::middleware::from_fn(crate::timing::layer))
        .with_state(platform)
}

/// The paths [`router`] registers, for routes.toml's test: the base's and
/// every registered feature crate's.
pub fn paths() -> Vec<&'static str> {
    all_routes().paths
}
