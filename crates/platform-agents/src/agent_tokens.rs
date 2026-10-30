//! Agent grant tokens (M230, platform step 8): how an owner lets an agent
//! with no machine of its own (redone's, a hosted service's) act on their
//! projects, through the platform's MCP endpoint ([`crate::agent_mcp`]).
//! "A hosted MCP endpoint serves agents with no machine" (aru-111), with
//! a bearer agent grant token that is a grant ("Grants, not
//! impersonation", aru-073).
//!
//! - **Minted only on the platform page,** from a signed-in browser's
//!   session (a CLI's login is refused, as for environment secrets: its key
//!   is a file an agent on its machine can read), over projects they read, with powers
//!   from the agents' list (`threads.read`, `threads.post`, `issues.write`,
//!   `deploys.plan`, `runs.read`, `runs.start`, `runs.command`) and
//!   `secrets.write`. Never `cloud.drive`, and never an approval: no grant
//!   is (aru-101).
//! - **Shown once.** Only its SHA-256 is kept ([`crate::db::agent_tokens`]),
//!   with the browser it was minted from, and the account's devices are
//!   pushed a notice of every mint.
//! - **Listed and revoked** here and with `arugula agent-tokens`, with the
//!   calls its agent made. A revoke refuses the very next call. A project's
//!   owners see every token over it, anyone's, and revoke its reach there
//!   ([`over`], [`revoke_over`]). A token's grants over a team's project go
//!   when its owner leaves the team (`db::agent_tokens::revoke_departed`).
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
    auth::{BrowserPerson, Person},
    db::{
        agent_tokens::{AgentToken, MAX_LIVE, NewToken, hash, new_token},
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
        "minted_from": t.minted_from,
        "imported_from": t.imported_from,
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

/// Unicode's format characters (general category Cf): bidirectional
/// controls, zero-width joiners and the like, which make a name read as
/// something it isn't.
pub fn is_format_char(c: char) -> bool {
    matches!(c as u32,
        0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x0890..=0x0891 | 0x08E2 | 0x180E
        | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB
        | 0x110BD | 0x110CD | 0x13430..=0x1343F | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0001
        | 0xE0020..=0xE007F)
}

/// A token's name: 1 to 64 characters, no control or format characters,
/// trimmed.
fn name_ok(name: &str) -> Result<String, ApiError> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > MAX_NAME || n.chars().any(|c| c.is_control() || is_format_char(c)) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "a token's name is 1 to 64 characters, on one line, with no invisible or direction characters",
        ));
    }
    Ok(n.to_owned())
}

/// `POST /api/agent-tokens {name, projects, powers}`: mint a token with
/// each power over each project, and answer it once (`token`): it isn't
/// kept, and can't be shown again. Only from the platform page, in a
/// signed-in browser ([`BrowserPerson`]): a CLI's login is refused. The
/// account's devices are pushed a notice.
pub async fn mint(State(app): State<Arc<Platform>>, s: BrowserPerson, headers: HeaderMap, Json(b): Json<Mint>) -> R {
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
                 runs.read, runs.start, runs.command, boards.read, boards.write, trackers.write and secrets.write (none approves anything)"
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
    let from: String = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("a browser")
        .chars()
        .take(200)
        .collect();
    let new = NewToken { id, account: s.account.clone(), name, token_hash: hash(&token), minted_from: from };
    let t = app.db.mint_agent_token(&new, &grants, now_ms())?;
    tracing::info!(account = %s.account, token = %t.id, powers = ?powers, projects = ?projects, "minted an agent token");
    notify_mint(&app, &s.account, &t, &powers, projects.len());
    let view = wire(&app, &t, &app.db.agent_token_grants(&t.id, &s.account)?).await?;
    Ok(Json(json!({ "agent_token": view, "token": token, "mcp_url": mcp_url(&app, &headers) })))
}

/// Push the account's devices a notice of a new token: who it acts for,
/// what it may do, and where to revoke it. In the background: a failure is
/// only logged.
fn notify_mint(app: &Platform, account: &str, t: &AgentToken, powers: &[String], projects: usize) {
    let msg = json!({
        "title": "A new agent token",
        "body": format!(
            "\u{201c}{}\u{201d} can act as your agent over {projects} project(s): {}. Not you? Revoke it in Settings, \
             Agents with no machine.",
            t.name,
            powers.join(", ")
        ),
        "tag": format!("agent-token-{}", t.id),
        "control": true,
    });
    let hub = app.hub.clone();
    let (account, id) = (account.to_owned(), t.id.clone());
    tokio::spawn(async move {
        if let Err(e) = hub.push(&account, &msg).await {
            tracing::warn!(%account, token = %id, error = %e, "the agent token's notice didn't go");
        }
    });
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

/// The project's owner, or 403/404.
async fn owner_of(app: &Platform, account: &str, project: &str) -> Result<(), ApiError> {
    let (_, role) = crate::project_roles::project_as(app, account, project, false).await?;
    if role != arugula_e2e::team::TeamRole::Owner {
        return Err(err(StatusCode::FORBIDDEN, "only the project's owners see and revoke the agent tokens over it"));
    }
    Ok(())
}

/// `GET /api/projects/{project}/agent-tokens`: every live token with a
/// grant over the project, whoever minted it, with its powers there: for
/// the project's owners.
pub async fn over(State(app): State<Arc<Platform>>, s: Person, Path(project): Path<String>) -> R {
    owner_of(&app, &s.account, &project).await?;
    let mut tokens = vec![];
    let rows = app.db.agent_tokens_over(&project)?;
    let ids: Vec<String> = rows.iter().map(|(t, _)| t.account.clone()).collect();
    let names = app.cache.accounts(&*app.hub, &ids).await?;
    for (t, grants) in rows {
        let mut v = wire(&app, &t, &grants).await?;
        v["owner"] = json!({ "account": t.account, "name": names.get(&t.account).map(|a| a.name.clone()) });
        tokens.push(v);
    }
    Ok(Json(json!({ "project": project, "tokens": tokens })))
}

/// `POST /api/projects/{project}/agent-tokens/{id}/revoke`: a project
/// owner takes a token's reach over the project back, whoever minted it.
/// Its grants elsewhere stay its owner's to keep or revoke.
pub async fn revoke_over(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((project, id)): Path<(String, String)>,
) -> R {
    owner_of(&app, &s.account, &project).await?;
    let n = app.db.revoke_agent_token_over(&project, &id, now_ms())?;
    if n == 0 {
        return Err(err(StatusCode::NOT_FOUND, "no live agent token over this project by that id"));
    }
    tracing::info!(account = %s.account, %project, token = %id, "a project owner revoked an agent token's reach");
    Ok(Json(json!({ "project": project, "token": id, "revoked": n })))
}

/// `GET /api/projects/{project}/secrets`: the project's own secrets by
/// name (`set_secret`'s), who last set each and when: never a value. For
/// its owners.
pub async fn secrets(State(app): State<Arc<Platform>>, s: Person, Path(project): Path<String>) -> R {
    owner_of(&app, &s.account, &project).await?;
    let rows = app.db.project_secrets(&project)?;
    let mut ids: Vec<String> = rows.iter().filter_map(|r| r.updated_by.clone()).collect();
    ids.sort();
    ids.dedup();
    let names = app.cache.accounts(&*app.hub, &ids).await?;
    let agents: Vec<String> = rows.iter().filter_map(|r| r.updated_by_agent.clone()).collect();
    let agents = crate::grants::agent_names(&app, &agents).await?;
    let secrets: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "name": r.name,
                "updated_at": r.updated_at,
                "updated_by": r.updated_by.as_ref().map(|a| json!({ "account": a, "name": names.get(a).map(|n| n.name.clone()) })),
                "agent": r.updated_by_agent.as_ref().map(|a| json!({ "id": a, "name": agents.get(a).map(|d| d.name.clone()) })),
            })
        })
        .collect();
    Ok(Json(json!({ "project": project, "secrets": secrets, "max": crate::agent_mcp::MAX_PROJECT_SECRETS })))
}
