//! The platform's routes. Each is listed in `crates/control/routes.toml`
//! with `process = "platform"`, and its test reads the paths from
//! [`paths`]. The paths stay what they are on control's site: in process
//! the hub mounts this router behind its `hub_port::gate`, and out of
//! process (M140) it forwards each platform path here.

use std::sync::Arc;

use axum::{
    Router,
    routing::{MethodRouter, get, post, put},
};

use arugula_control_wire::{issues, runs as wire_runs};

use crate::{
    Platform, agent_mcp, agent_tokens, billing, chat, clients, deploys, forge, grants, hosted, links, projects,
    run_grant, run_logs,
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
/// (M214); reading runs, and a machine's agents' runs (M216); runs'
/// artifacts (M213); hosted agents' archived transcripts (M184); agents
/// with no machine, their grant tokens and MCP endpoint (M230); the
/// platform page's linked products (M229); and studio as a client (M236).
fn routes() -> Routes {
    Routes::default()
        .route("/api/billing", get(billing::status))
        .route("/api/billing/checkout", post(billing::checkout))
        .route("/api/billing/report", post(billing::report_now))
        .route("/api/stripe/webhook", post(billing::webhook))
        .route("/api/chat", get(chat::rooms))
        .route("/api/chat/live", get(chat::live))
        .route("/api/chat/live/open", post(chat::open))
        .route("/api/chat/search", get(chat::search))
        .route("/api/chat/presence", post(chat::people::beat))
        .route("/api/chat/avatar/{account}", get(chat::people::avatar))
        .route("/api/chat/sidebar", get(chat::sidebar::get).put(chat::sidebar::put))
        .route("/api/chat/targets", post(chat::open_target))
        .route("/api/chat/teams/{team}/channels", get(chat::members::channels).post(chat::new_channel))
        .route("/api/chat/teams/{team}/dms", post(chat::new_dm))
        .route("/api/chat/rooms/{id}", get(chat::room).patch(chat::change_room))
        .route("/api/chat/rooms/{id}/messages", get(chat::messages).post(chat::post))
        .route("/api/chat/rooms/{id}/messages/{seq}", axum::routing::patch(chat::edit).delete(chat::delete))
        .route(
            "/api/chat/rooms/{id}/files",
            post(chat::files::upload)
                .layer(axum::extract::DefaultBodyLimit::max(arugula_control_wire::chat::MAX_FILE_BYTES + 1)),
        )
        .route("/api/chat/rooms/{id}/files/{file}", get(chat::files::get))
        .route("/api/chat/rooms/{id}/previews", get(chat::previews::previews))
        .route("/api/chat/rooms/{id}/messages/{seq}/previews", post(chat::previews::hide))
        .route("/api/chat/previews/image", get(chat::previews::image))
        .route("/api/chat/teams/{team}/previews", get(chat::previews::team_get).put(chat::previews::team_put))
        .route(
            "/api/chat/rooms/{id}/messages/{seq}/reactions",
            post(chat::reactions::add).delete(chat::reactions::remove),
        )
        .route("/api/chat/rooms/{id}/topic", axum::routing::put(chat::pins::set_topic))
        .route("/api/chat/rooms/{id}/pins", get(chat::pins::list))
        .route("/api/chat/rooms/{id}/messages/{seq}/pin", axum::routing::put(chat::pins::pin).delete(chat::pins::unpin))
        .route("/api/chat/rooms/{id}/join", post(chat::members::join))
        .route("/api/chat/rooms/{id}/leave", post(chat::members::leave))
        .route("/api/chat/rooms/{id}/members", post(chat::members::add))
        .route("/api/chat/rooms/{id}/members/{account}", axum::routing::delete(chat::members::remove))
        .route("/api/chat/rooms/{id}/read", post(chat::read))
        .route("/api/chat/threads", get(chat::threads::followed))
        .route("/api/chat/rooms/{id}/threads/{seq}/read", post(chat::threads::read))
        .route("/api/chat/rooms/{id}/threads/{seq}/follow", post(chat::threads::follow))
        .route("/api/chat/rooms/{id}/mute", post(chat::mute))
        .route("/api/chat/rooms/{id}/unread", post(chat::saved::unread))
        .route("/api/chat/saved", get(chat::saved::list))
        .route(
            "/api/chat/rooms/{id}/messages/{seq}/saved",
            axum::routing::put(chat::saved::save).patch(chat::saved::done).delete(chat::saved::unsave),
        )
        .route("/api/chat/rooms/{id}/typing", post(chat::typing::typing))
        .route("/api/chat/keywords", get(chat::alerts::keywords).put(chat::alerts::set_keywords))
        .route("/api/chat/teams/{team}/settings", get(chat::alerts::team_settings).put(chat::alerts::set_team_settings))
        .route("/api/chat/rooms/{id}/level", post(chat::level))
        .route("/api/notify/prefs", get(crate::notify::prefs).post(crate::notify::set_prefs))
        .route(arugula_control_wire::chat::CHAT_READERS, post(chat::daemon_readers))
        .route(arugula_control_wire::chat::CHAT_IMPORT, post(chat::daemon_import))
        .route(arugula_control_wire::chat::AGENT_ROOMS, get(chat::agent_threads::rooms))
        .route(
            "/api/daemon/chat/rooms/{id}/messages",
            get(chat::agent_threads::messages).post(chat::agent_threads::post),
        )
        .route(
            "/api/daemon/chat/rooms/{id}/files",
            post(chat::agent_threads::upload)
                .layer(axum::extract::DefaultBodyLimit::max(arugula_control_wire::chat::MAX_FILE_BYTES + 1)),
        )
        .route("/api/daemon/chat/rooms/{id}/files/{file}", get(chat::agent_threads::download))
        .route(
            "/api/daemon/chat/rooms/{id}/messages/{seq}/reactions",
            post(chat::agent_threads::react).delete(chat::agent_threads::unreact),
        )
        .route(
            "/api/daemon/chat/rooms/{id}/messages/{seq}/pin",
            axum::routing::put(chat::agent_threads::pin).delete(chat::agent_threads::unpin),
        )
        .route(arugula_control_wire::chat::AGENT_SEARCH, get(chat::agent_threads::search))
        .route("/api/grants", get(grants::list).post(grants::give))
        .route("/api/grants/{id}/revoke", post(grants::revoke))
        // Agents with no machine (M230): their grant tokens, and the MCP
        // endpoint only those tokens open.
        .route(agent_tokens::AGENT_TOKENS, get(agent_tokens::list).post(agent_tokens::mint))
        .route(agent_tokens::AGENT_TOKEN_REVOKE, post(agent_tokens::revoke))
        .route(agent_tokens::AGENT_TOKEN_CALLS, get(agent_tokens::calls))
        .route(
            agent_mcp::MCP,
            post(agent_mcp::endpoint).layer(axum::extract::DefaultBodyLimit::max(agent_mcp::MAX_BODY)),
        )
        // A source to ship, as redone's agents upload one (M235).
        .route(
            agent_mcp::UPLOADS,
            put(agent_mcp::upload)
                .post(agent_mcp::upload)
                .layer(axum::extract::DefaultBodyLimit::max(arugula_e2e::deploy::planter::MAX_TARBALL + 1)),
        )
        .route("/api/projects/{project}/agent-tokens", get(agent_tokens::over))
        .route("/api/projects/{project}/agent-tokens/{id}/revoke", post(agent_tokens::revoke_over))
        .route("/api/projects/{project}/secrets", get(agent_tokens::secrets))
        // Linked products (M229): the hub keeps the links.
        .route(links::LINKED_PRODUCTS, get(links::list))
        .route(links::UNLINK_PRODUCT, post(links::unlink))
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
        // studio as a client (M236): its apps by project, the projects of
        // the people who let it, and the secrets project owners let it read.
        .route(clients::PROJECTS, get(clients::projects))
        .route(clients::APPS, get(clients::apps))
        .route(clients::APP, get(clients::show).put(clients::put).delete(clients::remove))
        .route(clients::APP_SECRET, get(clients::secret).put(clients::ask_secret))
        .route(clients::CONSENT, axum::routing::put(clients::set_consent).delete(clients::set_consent))
        .route(clients::PROJECT_CLIENTS, get(clients::project_clients))
        .route(clients::PROJECT_GRANT, axum::routing::put(clients::set_grant).delete(clients::set_grant))
        .route(
            clients::PROJECT_SECRET,
            axum::routing::put(clients::set_project_secret).delete(clients::set_project_secret),
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
        // An agent session's card and page (M257).
        .route(arugula_control_wire::sessions::SESSION, get(crate::session_live::show))
        .route(arugula_control_wire::sessions::SESSION_STOP, post(crate::session_live::stop))
        .route(arugula_control_wire::sessions::SESSION_ACTIONS, post(crate::session_live::act))
        // Its pull request, which its asker drafts and sends (M259).
        .route(
            arugula_control_wire::sessions::SESSION_PR,
            get(crate::sessions::pr::show).put(crate::sessions::pr::save),
        )
        .route(arugula_control_wire::sessions::SESSION_PR_SEND, post(crate::sessions::pr::route))
        // Try 3 and its pick (M264).
        .route(arugula_control_wire::sessions::SESSION_TRY, post(crate::sessions::tries::start))
        .route(arugula_control_wire::sessions::SESSION_PICK, post(crate::sessions::tries::pick))
        .route(arugula_control_wire::sessions::PROJECT_SESSION_STATS, get(crate::sessions::merged::stats))
        .route(arugula_control_wire::sessions::ISSUE_AGENT, post(crate::chat::sessions::issue_agent))
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
