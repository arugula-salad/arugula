//! Agent grant tokens (M230, platform step 8): how an owner lets an agent
//! with no machine of its own (redone's, a hosted service's) act on their
//! projects, through the platform's MCP endpoint ([`crate::agent_mcp`]).
//! "A hosted MCP endpoint serves agents with no machine, using a bearer
//! agent-grant token" (amending aru-038), as grants ("Grants, not
//! impersonation", aru-073).
//!
//! - **Minted by a person** (a signed-in browser, or the CLI's login, as
//!   machines' grants are given), over projects they read, with powers
//!   from the agents' list (`threads.read`, `threads.post`, `issues.write`,
//!   `deploys.plan`, `runs.read`, `runs.start`, `runs.command`) and
//!   `secrets.write`. Never `cloud.drive`, and never an approval: no grant
//!   is (aru-101).
//! - **Shown once.** Only its SHA-256 is kept ([`crate::db::agent_tokens`]).
//! - **Listed and revoked** here and with `arugula agent-tokens`, with the
//!   calls its agent made. A revoke refuses the very next call.
//!
//! The rules over the token's agent are its machine counterpart's
//! ([`crate::grants::agent_may`]): each call asks for its grant again, and
//! it never reaches past what its owner reads, or does more than their
//! role lets them.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    Platform,
    auth::Person,
    db::{
        agent_tokens::{AgentToken, MAX_LIVE, hash, new_token},
        projects::Grant,
    },
    grants::{AgentScope, is_token_power, owner_reads},
};

pub use arugula_control_wire::agent_mcp::{AGENT_TOKEN_CALLS, AGENT_TOKEN_REVOKE, AGENT_TOKENS, MCP};

type R = Result<Json<Value>, ApiError>;

/// Tokens minted per account per hour.
const MINTS: (&str, usize) = ("agent-token-mint", 30);

/// The longest a token's name may be, in characters.
const MAX_NAME: usize = 64;

/// The MCP endpoint's URL on the site the request came in on.
pub fn mcp_url(app: &Platform, headers: &HeaderMap) -> String {
    format!("{}{MCP}", app.cfg.site(headers))
}

/// A token and its grants as the page and the CLI read them: never the
/// token itself.
async fn wire(app: &Platform, t: &AgentToken, grants: &[Grant]) -> Result<Value, ApiError> {
    let mut gs = vec![];
    for g in grants {
        let name = app.db.project_owner(&g.scope_id)?.map(|o| o.name);
        gs.push(json!({
            "id": g.id,
            "power": g.power,
            "project": { "id": g.scope_id, "name": name },
            "live": g.revoked_at.is_none(),
        }));
    }
    Ok(json!({
        "id": t.id,
        "name": t.name,
        "created_at": t.created_at,
        "last_used_at": t.last_used_at,
        "revoked_at": t.revoked_at,
        "live": t.revoked_at.is_none(),
        "grants": gs,
    }))
}

/// `GET /api/agent-tokens`: the account's tokens, live and revoked, newest
/// first, each with its grants, and the endpoint they're for.
pub async fn list(State(app): State<Arc<Platform>>, s: Person, headers: HeaderMap) -> R {
    let mut tokens = vec![];
    for (t, grants) in app.db.agent_tokens(&s.account)? {
        tokens.push(wire(&app, &t, &grants).await?);
    }
    Ok(Json(json!({ "tokens": tokens, "mcp_url": mcp_url(&app, &headers) })))
}

#[derive(Deserialize)]
pub struct Mint {
    /// What its agent is called: "<you>'s agent on <name> (MCP)".
    name: String,
    /// Projects, by id, that the minter reads.
    projects: Vec<String>,
    /// Powers over each of them.
    powers: Vec<String>,
}

/// A token's name: 1 to 64 characters, no control characters, trimmed.
fn name_ok(name: &str) -> Result<String, ApiError> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > MAX_NAME || n.chars().any(char::is_control) {
        return Err(err(StatusCode::BAD_REQUEST, "a token's name is 1 to 64 characters, on one line"));
    }
    Ok(n.to_owned())
}

/// `POST /api/agent-tokens {name, projects, powers}`: mint a token with
/// each power over each project, and answer it once (`token`): it isn't
/// kept, and can't be shown again.
pub async fn mint(State(app): State<Arc<Platform>>, s: Person, headers: HeaderMap, Json(b): Json<Mint>) -> R {
    app.limits.check_account(MINTS, &s.account)?;
    let name = name_ok(&b.name)?;
    let mut powers = b.powers.clone();
    powers.sort();
    powers.dedup();
    let mut projects = b.projects.clone();
    projects.sort();
    projects.dedup();
    if powers.is_empty() || projects.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "a token needs at least one power over at least one project"));
    }
    if let Some(p) = powers.iter().find(|p| !is_token_power(p)) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            &format!(
                "a token can't hold {p}: its powers are threads.read, threads.post, issues.write, deploys.plan, \
                 runs.read, runs.start, runs.command and secrets.write (none approves anything)"
            ),
        ));
    }
    for p in &projects {
        // Only over what the minter reads, on the hub's word.
        if owner_reads(&app, &s.account, &AgentScope::Project(p.clone()), true).await?.is_none() {
            return Err(err(StatusCode::FORBIDDEN, "you can only give an agent what you can read yourself"));
        }
    }
    if app.db.live_agent_tokens(&s.account)? >= MAX_LIVE {
        return Err(err(
            StatusCode::CONFLICT,
            &format!("you hold {MAX_LIVE} live agent tokens already: revoke one you don't use"),
        ));
    }
    let (id, token) = new_token();
    let grants: Vec<(&str, &str)> =
        projects.iter().flat_map(|p| powers.iter().map(move |w| (w.as_str(), p.as_str()))).collect();
    let t = app.db.mint_agent_token(&id, &s.account, &name, &hash(&token), &grants, now_ms())?;
    tracing::info!(account = %s.account, token = %t.id, powers = ?powers, projects = ?projects, "minted an agent token");
    let view = wire(&app, &t, &app.db.agent_token_grants(&t.id, &s.account)?).await?;
    Ok(Json(json!({ "agent_token": view, "token": token, "mcp_url": mcp_url(&app, &headers) })))
}

/// `POST /api/agent-tokens/{id}/revoke`: revoke it and its grants. Its
/// agent is refused from its next call; again changes nothing.
pub async fn revoke(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> R {
    let t = app
        .db
        .revoke_agent_token(&s.account, &id, now_ms())?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such agent token"))?;
    tracing::info!(account = %s.account, token = %t.id, "revoked an agent token");
    let grants = app.db.agent_tokens(&s.account)?.into_iter().find(|(x, _)| x.id == t.id).map(|(_, g)| g);
    Ok(Json(wire(&app, &t, &grants.unwrap_or_default()).await?))
}

#[derive(Deserialize)]
pub struct CallsQuery {
    #[serde(default)]
    limit: Option<u32>,
}

/// `GET /api/agent-tokens/{id}/calls?limit=`: the newest calls its agent
/// made (50, at most 200), newest first.
pub async fn calls(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Query(q): Query<CallsQuery>,
) -> R {
    let t = app.db.agent_token(&id)?.filter(|t| t.account == s.account);
    let t = t.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such agent token"))?;
    let calls = app.db.agent_calls(&s.account, &t.id, q.limit.unwrap_or(50).clamp(1, 200))?;
    Ok(Json(json!({ "token": t.id, "calls": calls })))
}
