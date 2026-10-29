//! The platform's agents (#314, `arugula-platform-agents`): agents with no
//! machine of their own, which reach a project through its MCP endpoint
//! ([`agent_mcp`], M230) with an agent grant token ([`agent_tokens`]);
//! redone's tenants, tokens and apps, imported ([`redone`], M235), with the
//! Infisical projection of their secrets ([`infisical`]); and registered
//! clients such as studio ([`clients`], M236) and the platform page's
//! linked products ([`links`], M229).
//!
//! It sits on the platform's base (`arugula-platform-base`, crates/platform),
//! whose modules it names as the platform did before the split
//! (`crate::db`, `crate::grants`, ...: the glob below), and adds its routes
//! and background work through [`features`].

use std::sync::Arc;

use axum::routing::{get, post, put};

#[allow(unused_imports)]
use arugula_platform::*;

pub mod agent_mcp;
#[cfg(test)]
mod agent_mcp_tests;
pub mod agent_tokens;
pub mod clients;
#[cfg(test)]
mod clients_tests;
pub mod infisical;
pub mod links;
pub mod redone;

struct Agents;

impl features::Feature for Agents {
    fn routes(&self, r: features::Routes) -> features::Routes {
        r
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
    }

    fn start(&self, p: &Arc<Platform>) {
        // Clients' secret events past their days (M236).
        clients::start(p);
    }
}

/// Puts the agents in this platform: the binaries do, before it's made
/// (`arugula-platform-all`). Once.
pub fn register() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| features::register(Box::new(Agents)));
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
