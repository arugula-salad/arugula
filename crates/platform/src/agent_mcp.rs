//! The platform's MCP endpoint for agents with no machine (M230, platform
//! step 8): `POST /api/mcp` on control's own site, behind the hub's
//! gateway like every platform path, for an agent that runs somewhere
//! with no `arugulad` of its own (redone's, a hosted service's): "A hosted
//! MCP endpoint serves agents with no machine" (aru-111), amending "MCP is
//! served by the daemon" (aru-038), which still serves every machine's
//! agents.
//!
//! - **Streamable HTTP, stateless.** JSON-RPC 2.0 over `POST`: one message
//!   (or a batch) in, its answer as JSON out; a notification gets `202`.
//!   No session and no server stream: nothing here pushes. `initialize`,
//!   `ping`, `tools/list` and `tools/call`.
//! - **Who's asking is a bearer agent grant token** (`Authorization:
//!   Bearer aru_agent_…`, [`crate::agent_tokens`]), which only this route
//!   reads: every other route ignores it, so it opens nothing else. The
//!   hub vouches for nobody here; the platform finds the token by its hash.
//! - **The same checks as a machine's agents.** Each tool is the route a
//!   machine's agents call (`/api/daemon/...`), called here with the token
//!   in the machine's place ([`Machine`] `{ account: owner, daemon: agt_… }`),
//!   so who may do what is checked in one place: the token's live grant
//!   over the project, asked on every call ([`crate::grants::agent_may`]),
//!   the owner's reach and role as the ceiling, and the same rate limits,
//!   counted per token. What it writes is its owner's, labelled
//!   "<owner>'s agent on <name> (MCP)".
//! - **The same tool names as the daemon's MCP** where they overlap:
//!   `capabilities`, `list` (kind `threads`), `read_thread`,
//!   `post_thread`, `search_threads`, `issue_write`, `forge_read` (kinds
//!   `issue` and `native_issues`), `start_run`, `read_run`, `wait` (a run),
//!   `plan_deploy` and `read_deploy`; and `set_secret`, a project's secret
//!   by name, write-only (`secrets.write`): nothing reads a value back.
//! - **Never an approval** (aru-101). There's no tool that approves or
//!   denies anything, a call naming one is refused, and the approval
//!   routes take a person's session and device signature, never this
//!   token. `plan_deploy` plans and asks; the owners get the card.
//! - **Every call is noted** for the token's owner
//!   (`/api/agent-tokens/{id}/calls`): the tool, its project, how it went,
//!   and what it made or why not. Never an argument's value.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use arugula_control_common::ApiError;
use arugula_control_wire::{
    chat::{
        AgentMessagesQuery, ChatPost, ChatReact, ChatReactQuery, ChatReaction, ChatSearchQuery, ChatSystem, pins_line,
        reactions_line, system_text,
    },
    issues::{IssueListEdit, IssuePatch, IssueQuery, NewIssue},
    runs::{AgentRunStart, RUN_WAIT_MAX, RunListQuery, RunLogQuery, RunQuery},
};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    body::Bytes,
    extract::{FromRequestParts, Path, Query, State},
    http::{StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    Platform,
    auth::Machine,
    chat::agent_threads,
    db::{
        agent_tokens::{AgentCall, AgentToken, TOKEN_PREFIX, hash, new_call},
        projects::{DEPLOYS_PLAN, SECRETS_WRITE},
    },
    deploys,
    grants::{AgentScope, agent_may, owner_reads, token_label},
    issues,
    triggers::agent as runs,
};

pub use arugula_control_wire::agent_mcp::MCP;

/// redone's tools, for an agent whose token came from redone (M235).
#[path = "agent_mcp_redone.rs"]
mod redone;

pub use arugula_control_wire::agent_mcp::UPLOADS;

/// The MCP version answered when the client asks for one this doesn't
/// know.
const PROTOCOL: &str = "2025-06-18";
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// Requests a token's agent makes, per hour, whatever they are: each
/// tool's own limit is counted too.
const REQUESTS: (&str, usize) = ("mcp-agent-request", 7_200);
/// The same, across all of an account's tokens.
pub(crate) const ACCOUNT_REQUESTS: (&str, usize) = ("mcp-account-request", 14_400);
/// Deploys planned and asked for by an account's tokens' agents, per hour,
/// all of them together (each token's are counted too).
pub(crate) const ACCOUNT_DEPLOY_ASKS: (&str, usize) = ("mcp-account-deploy", 30);
/// Runs started by an account's tokens' agents, per hour, all together.
pub(crate) const ACCOUNT_RUN_STARTS: (&str, usize) = ("mcp-account-run", 120);
/// The most a request's body holds, in bytes: a ship's source, base64
/// (M233), and the rest of its call.
pub const MAX_BODY: usize = 3 * 1024 * 1024;
/// The most secrets of its own a project keeps.
pub const MAX_PROJECT_SECRETS: usize = 100;
/// Secrets a token's agent sets, per hour.
const SECRET_WRITES: (&str, usize) = ("mcp-agent-secret", 120);
/// The most a secret's value holds, in bytes.
pub(crate) const MAX_SECRET: usize = 16 * 1024;
/// How long `wait` waits by default, and at most, in seconds.
const WAIT_DEFAULT: u64 = 30;
const WAIT_MAX: u64 = 120;
/// The most of a run's log `read_run` reads at once, in bytes.
const LOG_PAGE: u64 = 16_000;
const LOG_PAGE_MAX: u64 = 40_000;

/// What the endpoint says to an agent without a token that's live.
fn unauthorized(why: &str) -> Response {
    let body = Json(json!({ "error": why }));
    (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer realm=\"arugula\"")], body).into_response()
}

/// A live agent grant token, from the request's `Authorization`.
pub struct TokenAgent(pub AgentToken);

impl FromRequestParts<Arc<Platform>> for TokenAgent {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<Platform>) -> Result<Self, Response> {
        let bearer = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
            .map(str::trim);
        let Some(token) = bearer else {
            return Err(unauthorized(
                "an agent grant token, as `Authorization: Bearer aru_agent_…`: its owner mints one on the platform \
                 page (Settings, Agents with no machine) or with `arugula agent-tokens add`",
            ));
        };
        // A redone token (M235) works here once the import carried it
        // over, and only then: it's found by the hash redone kept.
        let redone = token.starts_with(crate::redone::TOKEN_PREFIX);
        if !token.starts_with(TOKEN_PREFIX) && !redone {
            return Err(unauthorized("that isn't an agent grant token"));
        }
        match app.db.agent_token_by_hash(&hash(token)) {
            Ok(Some(t)) if redone && t.imported_from.as_deref() != Some(crate::db::redone::FROM_REDONE) => {
                Err(unauthorized("no such agent grant token"))
            }
            Ok(Some(t)) if t.revoked_at.is_none() => Ok(TokenAgent(t)),
            Ok(Some(_)) => Err(unauthorized("this agent grant token was revoked")),
            Ok(None) => Err(unauthorized("no such agent grant token")),
            Err(e) => Err(ApiError::from(e).into_response()),
        }
    }
}

/// `POST /api/mcp`: one JSON-RPC message. A batch is refused (MCP
/// 2025-06-18 has none), and so is a request with no `id` (a tool call sent
/// as a notification runs nothing). The body is at most [`MAX_BODY`]
/// (the router's limit).
pub async fn endpoint(State(app): State<Arc<Platform>>, TokenAgent(token): TokenAgent, body: Bytes) -> Response {
    let limited = app
        .limits
        .check_daemon(REQUESTS, &token.id)
        .and_then(|()| app.limits.check_account(ACCOUNT_REQUESTS, &token.account));
    if let Err(e) = limited {
        return e.into_response();
    }
    if let Err(e) = app.db.touch_agent_token(&token.id, now_ms()) {
        tracing::warn!(error = format!("{e:#}"), token = %token.id, "couldn't note an agent token's use");
    }
    let bad = |code: i64, why: &str| (StatusCode::BAD_REQUEST, Json(rpc_error(Value::Null, code, why))).into_response();
    let msg: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(-32700, &format!("parse error: {e}")),
    };
    if msg.is_array() {
        return bad(-32600, "one JSON-RPC message per request: batches aren't taken (MCP 2025-06-18 has none)");
    }
    let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
    if msg.get("id").is_none_or(Value::is_null) && !method.starts_with("notifications/") {
        if method.is_empty() {
            // A response the client sent: nothing to answer.
            return StatusCode::ACCEPTED.into_response();
        }
        return bad(-32600, "a request needs an id: only notifications/… are sent without one, and they run nothing");
    }
    // Its owner as people know them, for what the agent is told.
    let owner = crate::chat::name_of(&app, &token.account).await.ok().filter(|n| !n.is_empty());
    let owner = owner.unwrap_or_else(|| token.account.clone());
    let agent = Agent { app, token, owner };
    match agent.handle(msg).await {
        Some(r) => Json(r).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// How a tool call went wrong: refused (no grant, the owner's role or
/// reach, a revoked token) or failed (anything else).
#[derive(Debug)]
enum Failure {
    Refused(String),
    Failed(String),
}

impl From<ApiError> for Failure {
    fn from(e: ApiError) -> Self {
        match e.0 {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Failure::Refused(e.1),
            _ => Failure::Failed(e.1),
        }
    }
}

impl From<anyhow::Error> for Failure {
    fn from(e: anyhow::Error) -> Self {
        ApiError::from(e).into()
    }
}

/// A tool that answered: what to say, the structured answer, the project
/// it acted in, and the line the call's record keeps.
struct Done {
    text: String,
    value: Value,
    project: Option<String>,
    note: String,
}

type Out = Result<Done, Failure>;

fn failed(why: impl Into<String>) -> Failure {
    Failure::Failed(why.into())
}

fn args<T: DeserializeOwned>(a: &Value) -> Result<T, Failure> {
    serde_json::from_value(a.clone()).map_err(|e| failed(format!("bad arguments: {e}")))
}

fn to_value<T: serde::Serialize>(v: T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// A token's agent, answering one request.
struct Agent {
    app: Arc<Platform>,
    token: AgentToken,
    /// Its owner's name (their account's id if the hub has none).
    owner: String,
}

impl Agent {
    /// Whether its token came from redone (M235): its agent gets redone's
    /// tools too.
    fn is_redone(&self) -> bool {
        self.token.imported_from.as_deref() == Some(crate::db::redone::FROM_REDONE)
    }

    /// The tools its agent is offered.
    fn listed(&self) -> Vec<Value> {
        if self.is_redone() { redone::tools() } else { tools() }
    }

    /// Where its agent uploads a source.
    fn upload_url(&self) -> String {
        format!("{}{UPLOADS}", self.app.cfg.public_url)
    }

    /// The token in a machine's place, for the routes a machine's agents
    /// call.
    fn me(&self) -> Machine {
        Machine { account: self.token.account.clone(), daemon: self.token.id.clone() }
    }

    fn state(&self) -> State<Arc<Platform>> {
        State(self.app.clone())
    }

    /// One JSON-RPC message's answer; `None` for a notification (or a
    /// response the client sent).
    async fn handle(&self, msg: Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let Some(method) = msg.get("method").and_then(Value::as_str).map(str::to_owned) else {
            return id.map(|id| rpc_error(id, -32600, "not a JSON-RPC request"));
        };
        if msg.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Some(rpc_error(id.unwrap_or(Value::Null), -32600, "not a JSON-RPC 2.0 request"));
        }
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
        let answer = match method.as_str() {
            "initialize" => Ok(self.initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": self.listed() })),
            "tools/call" => match params.get("name").and_then(Value::as_str) {
                Some(name) => {
                    let a = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                    Ok(self.call(name, &a).await)
                }
                None => Err((-32602, "tools/call needs a tool's name".to_owned())),
            },
            m if m.starts_with("notifications/") => return None,
            m => Err((-32601, format!("there's no method {m} here: initialize, ping, tools/list and tools/call"))),
        };
        let id = id?;
        Some(match answer {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => rpc_error(id, code, &message),
        })
    }

    fn initialize(&self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL);
        let version = if PROTOCOLS.contains(&asked) { asked } else { PROTOCOL };
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "arugula-platform", "version": env!("CARGO_PKG_VERSION") },
            "instructions": format!(
                "Arugula's platform, for an agent with no machine of its own: you act as {}'s agent, labelled \
                 \"{}\", in the projects and with the powers their token gives you (capabilities says which). \
                 Native issues and their threads (issue_write, forge_read, read_thread, post_thread), runs on a \
                 project's hosted boxes (start_run, read_run, wait), deploys (plan_deploy plans one and asks the \
                 project's owners to approve it; read_deploy follows it) and project secrets (set_secret, \
                 write-only). You never approve anything: an owner approves a deploy on their phone or in a \
                 browser.{}",
                self.owner,
                token_label(&self.token.name),
                if self.is_redone() { redone::instructions(&self.upload_url()) } else { String::new() }
            ),
        })
    }

    /// `tools/call`: the tool's answer, noted for the token's owner.
    async fn call(&self, name: &str, a: &Value) -> Value {
        let out = self.dispatch(name, a).await;
        // A name that's no tool is answered, never kept: the record holds
        // only tools there are, and an ask to approve (as `approve`).
        let known = self.listed().iter().any(|t| t["name"] == name);
        let name = match () {
            () if known => name,
            () if never_tool(name) => "approve",
            () => {
                let why = match &out {
                    Err(Failure::Refused(w) | Failure::Failed(w)) => w.clone(),
                    Ok(_) => String::new(),
                };
                return json!({ "content": [{ "type": "text", "text": why }], "isError": true });
            }
        };
        let asked_project = a.get("project").and_then(Value::as_str).map(str::to_owned);
        let (outcome, project, note) = match &out {
            Ok(d) => ("ok", d.project.clone().or(asked_project), d.note.clone()),
            Err(Failure::Refused(why)) => ("refused", asked_project, why.clone()),
            Err(Failure::Failed(why)) => ("failed", asked_project, why.clone()),
        };
        let record = AgentCall {
            id: new_call(),
            token: self.token.id.clone(),
            tool: name.to_owned(),
            project,
            outcome: outcome.to_owned(),
            detail: note.chars().take(300).collect(),
            at: now_ms(),
        };
        if let Err(e) = self.app.db.note_agent_call(&self.token, &record) {
            tracing::warn!(error = format!("{e:#}"), token = %self.token.id, "couldn't note an agent's call");
        }
        tracing::info!(token = %self.token.id, account = %self.token.account, tool = name, outcome, "an agent's MCP call");
        match out {
            Ok(d) => json!({
                "content": [{ "type": "text", "text": d.text }],
                "structuredContent": d.value,
                "isError": false,
            }),
            Err(Failure::Refused(why) | Failure::Failed(why)) => {
                json!({ "content": [{ "type": "text", "text": why }], "isError": true })
            }
        }
    }

    async fn dispatch(&self, name: &str, a: &Value) -> Out {
        if self.is_redone() {
            if let Some(out) = redone::dispatch(self, name, a).await {
                return out;
            }
            // A carried-over token reaches its tenant's apps only (M235).
            if let Some(why) = redone::refuses(self, name, a).await {
                return Err(why);
            }
        }
        match name {
            "capabilities" => self.capabilities().await,
            "list" => self.list(a).await,
            "read_thread" => self.read_thread(args(a)?).await,
            "post_thread" => self.post_thread(args(a)?).await,
            "search_threads" => self.search_threads(args(a)?).await,
            "issue_write" => self.issue_write(a).await,
            "forge_read" => self.forge_read(a).await,
            "start_run" => self.start_run(args(a)?).await,
            "read_run" => self.read_run(args(a)?).await,
            "wait" => self.wait(args(a)?).await,
            "plan_deploy" => self.plan_deploy(args(a)?).await,
            "read_deploy" => self.read_deploy(args(a)?).await,
            // Never serde's words: they can quote the value.
            "set_secret" => self.set_secret(serde_json::from_value(a.clone()).map_err(|_| {
                failed("set_secret takes name and value (strings), and project (a string) if the token has more than one")
            })?)
            .await,
            n if never_tool(n) => Err(Failure::Refused(format!(
                "there's no {n} here, and there never will be: agents never approve or deny anything, whatever \
                 grant they hold. plan_deploy asks the project's owners, and one of them approves on their phone \
                 or in a browser"
            ))),
            n => Err(failed(format!("there's no tool called {n}: tools/list lists them"))),
        }
    }

    /// The token's projects, by id, and the powers over each.
    fn reach(&self) -> Result<Vec<(String, Vec<String>)>, Failure> {
        let mut by: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for g in self.app.db.agent_token_grants(&self.token.id, &self.token.account)? {
            by.entry(g.scope_id).or_default().push(g.power);
        }
        Ok(by.into_iter().collect())
    }

    /// The project a call names (its id, or its name among the token's),
    /// else the token's only project.
    async fn project(&self, asked: Option<&str>) -> Result<String, Failure> {
        let reach = self.reach()?;
        let asked = asked.map(str::trim).filter(|p| !p.is_empty());
        if let Some(p) = asked.filter(|p| p.starts_with("prj_")) {
            return Ok(p.to_owned());
        }
        let mut named = vec![];
        for (id, _) in &reach {
            let name = self.app.db.project_owner(id)?.map(|o| o.name).unwrap_or_default();
            named.push((id.clone(), name));
        }
        match asked {
            Some(want) => {
                let ids: Vec<&String> =
                    named.iter().filter(|(_, n)| n.eq_ignore_ascii_case(want)).map(|(id, _)| id).collect();
                match ids.as_slice() {
                    [id] => Ok((*id).clone()),
                    [] => Err(failed(format!("this token reaches no project called {want}: capabilities lists them"))),
                    _ => Err(failed(format!("more than one project is called {want}: name it by its id (prj_…)"))),
                }
            }
            None => match named.as_slice() {
                [(id, _)] => Ok(id.clone()),
                _ => Err(failed(format!(
                    "name the project (its id or name): one of {}",
                    named.iter().map(|(id, n)| format!("{n} ({id})")).collect::<Vec<_>>().join(", ")
                ))),
            },
        }
    }

    async fn capabilities(&self) -> Out {
        let mut projects = vec![];
        for (id, powers) in self.reach()? {
            let reads = owner_reads(&self.app, &self.token.account, &AgentScope::Project(id.clone()), false).await?;
            projects.push(json!({ "id": id, "name": reads, "powers": powers, "reachable": reads.is_some() }));
        }
        let label = token_label(&self.token.name);
        let text = format!(
            "You're {}'s agent, labelled \"{label}\". {} project(s): {}. You never approve anything.",
            self.owner,
            projects.len(),
            projects
                .iter()
                .map(|p| format!(
                    "{} ({}): {}",
                    p["name"].as_str().unwrap_or("its owner doesn't read it now"),
                    p["id"].as_str().unwrap_or_default(),
                    p["powers"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .collect::<Vec<_>>()
                .join("; ")
        );
        let value = json!({
            "agent": { "token": self.token.id, "name": self.token.name, "label": label, "owner": self.token.account, "owner_name": self.owner },
            "projects": projects,
            "approves": false,
        });
        Ok(Done { text, value, project: None, note: "read its capabilities".into() })
    }

    async fn list(&self, a: &Value) -> Out {
        match a.get("kind").and_then(Value::as_str) {
            Some("threads") => {
                let r = agent_threads::rooms(self.state(), self.me()).await?.0;
                let v = to_value(&r);
                let text = format!("{} room(s) this token's grants reach.\n{v}", r.rooms.len());
                Ok(Done { text, value: v, project: None, note: format!("listed {} room(s)", r.rooms.len()) })
            }
            k => Err(failed(format!("list's kind here is threads, not {}", k.unwrap_or("nothing")))),
        }
    }

    async fn read_thread(&self, a: ReadThread) -> Out {
        let q =
            AgentMessagesQuery { after: a.after_seq, before: a.before_seq, limit: a.limit, mentions: a.mentions_only };
        let r = agent_threads::messages(self.state(), self.me(), Path(a.room.clone()), Query(q)).await?.0;
        let lines: Vec<String> = r
            .messages
            .iter()
            .map(|m| {
                let v = to_value(m);
                let by = v["name"].as_str().filter(|n| !n.is_empty()).unwrap_or("someone").to_owned();
                let by = match v["agent"]["machine_name"].as_str() {
                    Some(machine) => format!("{by}'s agent on {machine}"),
                    None => by,
                };
                if let Ok(s) = serde_json::from_value::<ChatSystem>(v["system"].clone()) {
                    return format!(
                        "[{}] * {by} {}",
                        v["seq"],
                        system_text(&s, v["text"].as_str().unwrap_or_default())
                    );
                }
                let rs: Vec<ChatReaction> = serde_json::from_value(v["reactions"].clone()).unwrap_or_default();
                let reacted =
                    if rs.is_empty() { String::new() } else { format!("  [reactions: {}]", reactions_line(&rs)) };
                let pinned = if v["pinned"] == true { "  [pinned]" } else { "" };
                format!("[{}] {by}: {}{reacted}{pinned}", v["seq"], v["text"].as_str().unwrap_or_default())
            })
            .collect();
        let mut about = String::new();
        if !r.room.topic.is_empty() {
            about += &format!("Topic: {}\n", r.room.topic);
        }
        if !r.room.description.is_empty() {
            about += &format!("Description: {}\n", r.room.description);
        }
        if !r.pins.is_empty() {
            about += &format!("Pinned:\n{}\n", pins_line(&r.pins));
        }
        let text = format!("{about}{} message(s) in {}:\n{}", lines.len(), a.room, lines.join("\n"));
        let project = r.room.project.clone();
        Ok(Done { text, value: to_value(&r), project, note: format!("read {}", a.room) })
    }

    async fn post_thread(&self, a: PostThread) -> Out {
        if let Some(seq) = a.react_to_seq {
            let emoji = a
                .emoji
                .clone()
                .filter(|e| !e.trim().is_empty())
                .ok_or_else(|| failed("react_to_seq needs an emoji"))?;
            let path = Path((a.room.clone(), seq));
            let m = if a.remove {
                let q = Query(ChatReactQuery { emoji: emoji.clone() });
                agent_threads::unreact(self.state(), self.me(), path, q).await?.0
            } else {
                agent_threads::react(self.state(), self.me(), path, Json(ChatReact { emoji: emoji.clone() })).await?.0
            };
            let what = if a.remove { "Took back" } else { "Reacted with" };
            let text = format!(
                "{what} {emoji} on seq {seq} in {} as {}'s agent on {}",
                a.room,
                self.owner,
                token_label(&self.token.name)
            );
            return Ok(Done { text, value: to_value(&m), project: None, note: format!("reacted in {}", a.room) });
        }
        if let Some(seq) = a.pin_seq {
            let path = Path((a.room.clone(), seq));
            let m = if a.remove {
                agent_threads::unpin(self.state(), self.me(), path).await?.0
            } else {
                agent_threads::pin(self.state(), self.me(), path).await?.0
            };
            let what = if a.remove { "Unpinned" } else { "Pinned" };
            let text = format!(
                "{what} seq {seq} in {} as {}'s agent on {}",
                a.room,
                self.owner,
                token_label(&self.token.name)
            );
            return Ok(Done { text, value: to_value(&m), project: None, note: format!("pinned in {}", a.room) });
        }
        let b = ChatPost { text: a.text, ..Default::default() };
        let r = agent_threads::post(self.state(), self.me(), Path(a.room.clone()), Json(b)).await?.0;
        let v = to_value(&r);
        let text = format!("Posted in {} as {}'s agent on {}: {v}", a.room, self.owner, token_label(&self.token.name));
        Ok(Done { text, value: v, project: None, note: format!("posted in {}", a.room) })
    }

    async fn search_threads(&self, a: SearchThreads) -> Out {
        let q = ChatSearchQuery { q: a.q, team: a.team };
        let r = agent_threads::search(self.state(), self.me(), Query(q)).await?.0;
        let v = to_value(&r);
        let n = v["found"].as_array().map_or(0, Vec::len);
        Ok(Done { text: format!("{n} found.\n{v}"), value: v, project: None, note: format!("searched, {n} found") })
    }

    /// An issue by `iss_…`, or `#N` in the call's project.
    async fn issue_id(&self, issue: &str, project: Option<&str>) -> Result<String, Failure> {
        let s = issue.trim();
        if s.starts_with("iss_") {
            return Ok(s.to_owned());
        }
        let n: u64 = s
            .trim_start_matches('#')
            .parse()
            .map_err(|_| failed(format!("issue: #N (in the project) or iss_…, not {s:?}")))?;
        let p = self.project(project).await?;
        Ok(issues::agent::by_number(self.state(), self.me(), Path((p, n))).await?.0.id)
    }

    async fn patch(&self, id: &str, p: IssuePatch) -> Result<Value, Failure> {
        Ok(to_value(issues::agent::patch(self.state(), self.me(), Path(id.to_owned()), Json(p)).await?.0))
    }

    async fn comment(&self, id: &str, text: String) -> Result<Value, Failure> {
        let b = ChatPost { text, ..Default::default() };
        Ok(to_value(agent_threads::post(self.state(), self.me(), Path(id.to_owned()), Json(b)).await?.0))
    }

    /// `issue_write`: `kind` create, comment, edit, label, assign, close or
    /// reopen, as the daemon's takes them.
    async fn issue_write(&self, a: &Value) -> Out {
        let kind = a.get("kind").and_then(Value::as_str).unwrap_or_default().to_owned();
        let w: IssueWrite = args(a)?;
        let project = w.project.as_deref();
        let issue = || w.issue.clone().ok_or_else(|| failed(format!("issue_write kind {kind} needs issue")));
        let (v, id, what) = match kind.as_str() {
            "create" => {
                let p = self.project(project).await?;
                let new = NewIssue {
                    title: w.title.clone().ok_or_else(|| failed("issue_write kind create needs a title"))?,
                    body: w.body.clone().unwrap_or_default(),
                    labels: w.labels.clone(),
                    assignees: w.assignees.clone(),
                };
                let i = issues::agent::create(self.state(), self.me(), Path(p), Json(new)).await?.0;
                let v = to_value(&i);
                let id = i.id.clone();
                (v, id.clone(), format!("filed #{} ({id})", i.number))
            }
            "comment" => {
                let id = self.issue_id(&issue()?, project).await?;
                let text = w.text.clone().ok_or_else(|| failed("issue_write kind comment needs text"))?;
                (self.comment(&id, text).await?, id.clone(), format!("commented on {id}"))
            }
            "edit" => {
                let id = self.issue_id(&issue()?, project).await?;
                let p = IssuePatch { title: w.title.clone(), body: w.body.clone(), ..Default::default() };
                (self.patch(&id, p).await?, id.clone(), format!("edited {id}"))
            }
            "label" | "assign" => {
                let id = self.issue_id(&issue()?, project).await?;
                if w.add.is_empty() == w.remove.is_empty() {
                    return Err(failed(format!("issue_write kind {kind}: add or remove (one of them)")));
                }
                let edit = if w.add.is_empty() {
                    IssueListEdit::Remove(w.remove.clone())
                } else {
                    IssueListEdit::Add(w.add.clone())
                };
                let p = if kind == "label" {
                    IssuePatch { labels: Some(edit), ..Default::default() }
                } else {
                    IssuePatch { assignees: Some(edit), ..Default::default() }
                };
                (self.patch(&id, p).await?, id.clone(), format!("{kind}ed {id}"))
            }
            "close" => {
                let id = self.issue_id(&issue()?, project).await?;
                let p = IssuePatch { state: Some("closed".into()), reason: w.reason.clone(), ..Default::default() };
                let mut v = self.patch(&id, p).await?;
                if let Some(c) = w.comment.clone() {
                    v["comment"] = self.comment(&id, c).await?;
                }
                (v, id.clone(), format!("closed {id}"))
            }
            "reopen" => {
                let id = self.issue_id(&issue()?, project).await?;
                let p = IssuePatch { state: Some("open".into()), ..Default::default() };
                (self.patch(&id, p).await?, id.clone(), format!("reopened {id}"))
            }
            k => {
                return Err(failed(format!(
                    "issue_write's kind is create, comment, edit, label, assign, close or reopen, not {k:?}"
                )));
            }
        };
        let project = v["project"].as_str().or(v["issue"]["project"].as_str()).map(str::to_owned);
        let text =
            format!("Done, at once, as {}'s agent on {}: {what}.\n{v}", self.owner, token_label(&self.token.name));
        let _ = id;
        Ok(Done { text, value: v, project, note: what })
    }

    /// `forge_read`, for native issues: kind `issue` (one, with its
    /// timeline) or `native_issues` (a project's).
    async fn forge_read(&self, a: &Value) -> Out {
        match a.get("kind").and_then(Value::as_str) {
            Some("issue") => {
                let w: IssueWrite = args(a)?;
                let id = self.issue_id(w.issue.as_deref().unwrap_or_default(), w.project.as_deref()).await?;
                let i = issues::agent::get(self.state(), self.me(), Path(id.clone())).await?.0;
                let events = issues::agent::events(self.state(), self.me(), Path(id.clone())).await?.0;
                let v = json!({ "issue": i, "events": events.events });
                let project = v["issue"]["project"].as_str().map(str::to_owned);
                Ok(Done { text: format!("{v}"), value: v, project, note: format!("read {id}") })
            }
            Some("native_issues") => {
                let l: ListIssues = args(a)?;
                let p = self.project(l.project.as_deref()).await?;
                let q = IssueQuery {
                    state: l.state,
                    labels: (!l.labels.is_empty()).then(|| l.labels.join(",")),
                    assignee: l.assignee,
                    author: l.author,
                    q: l.q,
                    before: l.before,
                    limit: l.limit,
                };
                let r = issues::agent::list(self.state(), self.me(), Path(p.clone()), Query(q)).await?.0;
                let v = to_value(&r);
                Ok(Done { text: format!("{v}"), value: v, project: Some(p), note: "listed issues".into() })
            }
            k => Err(failed(format!(
                "forge_read's kinds here are issue and native_issues (Arugula's own issues), not {}",
                k.unwrap_or("nothing")
            ))),
        }
    }

    async fn start_run(&self, a: StartRun) -> Out {
        let given =
            usize::from(a.spec.is_some()) + usize::from(!a.command.is_empty()) + usize::from(a.instruction.is_some());
        if given != 1 {
            return Err(failed("say what to run: a spec, a command, or an instruction (one of them)"));
        }
        let p = self.project(a.project.as_deref()).await?;
        self.app.limits.check_account(ACCOUNT_RUN_STARTS, &self.token.account)?;
        let b = AgentRunStart {
            spec: a.spec,
            workspace: a.on_box,
            argv: a.command,
            instruction: a.instruction,
            agent: a.recipe,
            directory: a.directory,
        };
        let (_, r) = runs::start(self.state(), self.me(), Path(p.clone()), Json(b)).await?;
        let r = r.0;
        let waits = if r.grant.is_none() {
            " The project has no standing grant, so it waits for an owner to approve it."
        } else {
            ""
        };
        let text = format!(
            "Started {} ({}), as {}'s agent on {}.{waits} wait with run {} follows it; read_run reads its log.",
            r.id,
            r.state,
            self.owner,
            token_label(&self.token.name),
            r.id
        );
        let note = format!("started {}", r.id);
        Ok(Done { text, value: json!({ "run": r }), project: Some(p), note })
    }

    async fn read_run(&self, a: ReadRun) -> Out {
        let Some(run) = a.run else {
            let p = self.project(a.project.as_deref()).await?;
            let q = RunListQuery { state: a.state, ..Default::default() };
            let r = runs::list(self.state(), self.me(), Path(p.clone()), Query(q)).await?.0;
            let v = to_value(&r);
            return Ok(Done { text: format!("{v}"), value: v, project: Some(p), note: "listed runs".into() });
        };
        let r = runs::show(self.state(), self.me(), Path(run.clone()), Query(RunQuery::default())).await?.0;
        let q = RunLogQuery {
            offset: a.offset,
            max: Some(a.max_chars.unwrap_or(LOG_PAGE).clamp(1, LOG_PAGE_MAX)),
            tail: None,
        };
        let log = runs::log(self.state(), self.me(), Path(run.clone()), Query(q)).await?.0;
        let v = json!({ "run": r, "log": log });
        let project = v["run"]["project"].as_str().map(str::to_owned);
        Ok(Done { text: format!("{v}"), value: v, project, note: format!("read {run}") })
    }

    /// `wait` with `run`: until it ends, or `timeout` seconds.
    async fn wait(&self, a: Wait) -> Out {
        let run = a.run.ok_or_else(|| failed("wait here waits for a run: give run (run_…)"))?;
        let limit = Duration::from_secs(a.timeout.unwrap_or(WAIT_DEFAULT).clamp(1, WAIT_MAX));
        let started = Instant::now();
        let mut since = None::<u64>;
        loop {
            let q = match since {
                None => RunQuery::default(),
                Some(s) => RunQuery { since: Some(s), wait: Some(RUN_WAIT_MAX.min(limit.as_secs())) },
            };
            let r = match runs::show(self.state(), self.me(), Path(run.clone()), Query(q)).await {
                Ok(r) => r.0,
                Err(e) if e.0 == StatusCode::TOO_MANY_REQUESTS && started.elapsed() < limit => {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            let ended = arugula_e2e::run::State::parse(&r.state).is_some_and(|s| s.terminal());
            if ended || started.elapsed() >= limit {
                let state = if ended { r.state.clone() } else { "still running".into() };
                let text = if ended {
                    format!(
                        "{} ended: {}{}",
                        r.id,
                        r.state,
                        r.exit_code.map(|c| format!(", exit {c}")).unwrap_or_default()
                    )
                } else {
                    format!("{} is {} after {}s: call wait again", r.id, r.state, started.elapsed().as_secs())
                };
                let project = Some(r.project.clone());
                let note = format!("waited for {run}: {state}");
                return Ok(Done { text, value: json!({ "state": state, "run": r }), project, note });
            }
            since = Some(r.last_seq);
        }
    }

    async fn plan_deploy(&self, a: PlanDeploy) -> Out {
        // planter's ship (M233): the source itself, to a planter-ship
        // environment, kept as the deploy's artifact.
        if a.files.is_some() {
            return self.ship(a).await;
        }
        let commit = a.commit.trim().to_ascii_lowercase();
        if commit.len() != 40 || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(failed(format!("commit: its full 40-hex id, not {:?}", a.commit)));
        }
        let repo = a.repo.trim();
        let part = |s: &str| {
            (1..=100).contains(&s.len())
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
                && !s.starts_with('.')
        };
        if !repo.split_once('/').is_some_and(|(o, n)| part(o) && part(n)) {
            return Err(failed(format!("repo: OWNER/NAME on GitHub, not {repo:?}")));
        }
        let p = self.project(a.project.as_deref()).await?;
        self.app.limits.check_account(ACCOUNT_DEPLOY_ASKS, &self.token.account)?;
        let source = json!({ "kind": "commit", "ref": commit, "where": repo });
        self.plan_and_ask(p, a.environment.trim(), source, a.roll_back, None).await
    }

    /// `plan_deploy` with `files` (M233): planter's ship API's shape, its
    /// `{ name, files }` as an environment and a gzipped tar, base64. The
    /// environment is a planter-ship one (its settings say the app's name,
    /// port, health path and secrets: the owner's, so the plan names them);
    /// the tar is kept as the project's artifact, by its digest, which the
    /// deploy's source names. Planned and asked, as any deploy: an owner
    /// approves it on their device.
    async fn ship(&self, a: PlanDeploy) -> Out {
        use arugula_e2e::deploy::planter;
        use base64::Engine as _;
        if !a.commit.trim().is_empty() || !a.repo.trim().is_empty() || a.roll_back {
            return Err(failed("files ship a source: leave out commit, repo and roll_back"));
        }
        let p = self.project(a.project.as_deref()).await?;
        self.app.limits.check_account(ACCOUNT_DEPLOY_ASKS, &self.token.account)?;
        agent_may(&self.app, &self.me(), DEPLOYS_PLAN, &AgentScope::Project(p.clone())).await?;
        let wanted = a.environment.trim();
        let env = self.app.db.environments(&p)?.into_iter().find(|e| e.id == wanted || e.name == wanted);
        let Some(env) = env.filter(|e| e.driver == planter::SHIP) else {
            return Err(failed(format!(
                "files ship to a {} environment, and the project has none called {wanted:?}: its owner makes one \
                 (its app's name, port and health path are its settings)",
                planter::SHIP
            )));
        };
        let text: String = a.files.unwrap_or_default().split_whitespace().collect();
        let bytes = base64::engine::general_purpose::STANDARD.decode(text.as_bytes()).map_err(|_| {
            failed("files: the app's source as a gzipped tar, base64 (tar -czf - -C DIR . | base64 -w0)")
        })?;
        if bytes.len() > planter::MAX_TARBALL {
            return Err(failed(format!(
                "files is {} bytes: a ship carries at most {} (leave out node_modules and builds)",
                bytes.len(),
                planter::MAX_TARBALL
            )));
        }
        if !bytes.starts_with(&[0x1f, 0x8b]) {
            return Err(failed("files isn't gzipped: tar -czf"));
        }
        // The project's quota, before anything's planned.
        let kept = self.app.db.deploy_artifacts_size(&p)?;
        if kept + bytes.len() as u64 > planter::MAX_PROJECT_ARTIFACTS {
            return Err(failed(format!(
                "the project keeps {} MiB of shipped sources waiting for their deploys, and at most {} MiB: wait for \
                 those to be approved or denied",
                kept >> 20,
                planter::MAX_PROJECT_ARTIFACTS >> 20
            )));
        }
        let digest = format!("sha256:{}", hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&bytes)));
        let source = json!({ "kind": "artifact", "ref": digest, "where": planter::ARTIFACT_WHERE });
        self.plan_and_ask(p, &env.id, source, false, Some(&bytes)).await
    }

    /// Plan a deploy of `source` to `environment` in project `p`, and ask
    /// its owners: what `plan_deploy` answers.
    /// With `keep` (a ship's source): it's kept as the project's artifact
    /// once the deploy is planned, before anyone is asked.
    async fn plan_and_ask(
        &self,
        p: String,
        environment: &str,
        source: Value,
        roll_back: bool,
        keep: Option<&[u8]>,
    ) -> Out {
        let plan: deploys::Plan = args(&json!({
            "environment": environment,
            "source": source,
            "roll_back": roll_back,
        }))?;
        let d = deploys::agent::plan(self.state(), self.me(), Path(p.clone()), Json(plan)).await?.0;
        let id = d["id"].as_str().unwrap_or_default().to_owned();
        if let (Some(bytes), Some(digest)) = (keep, source["ref"].as_str()) {
            let quota = arugula_e2e::deploy::planter::MAX_PROJECT_ARTIFACTS;
            if let Err(e) = self.app.db.put_deploy_artifact(&p, digest, bytes, &self.token.account, now_ms(), quota) {
                return Err(failed(format!("planned {id}, but its source wasn't kept ({e:#}), so it isn't asked")));
            }
        }
        let d = match deploys::agent::request(self.state(), self.me(), Path(id.clone())).await {
            Ok(d) => d.0,
            Err(e) => {
                return Err(failed(format!(
                    "planned {id}, but asking for its approval was refused: {}. read_deploy reads it",
                    e.1
                )));
            }
        };
        let text = format!(
            "Planned and asked, as {}'s agent on {}: {id} is {}. The project's owners got a card; one of them \
             approves it on their phone or in a browser, and you can't. read_deploy {id} follows it.\n{}",
            self.owner,
            token_label(&self.token.name),
            d["state"].as_str().unwrap_or("?"),
            d["plan_text"].as_str().unwrap_or_default()
        );
        Ok(Done { text, value: json!({ "deploy": d }), project: Some(p), note: format!("planned {id} and asked") })
    }

    async fn read_deploy(&self, a: ReadDeploy) -> Out {
        let d = deploys::agent::get(self.state(), self.me(), Path(a.id.trim().to_owned())).await?.0;
        let project = d["project"].as_str().or(d["environment"]["project"].as_str()).map(str::to_owned);
        Ok(Done { text: format!("{d}"), value: json!({ "deploy": d }), project, note: format!("read {}", a.id.trim()) })
    }

    /// `set_secret`: a project's secret by name (`secrets.write`), sealed
    /// with the project's key and never read back here (only a product a
    /// project owner lets read it, by name, gets its value: M236). Never past the
    /// owner's role: an owner or editor of the project.
    async fn set_secret(&self, a: SetSecret) -> Out {
        let p = self.project(a.project.as_deref()).await?;
        agent_may(&self.app, &self.me(), SECRETS_WRITE, &AgentScope::Project(p.clone())).await?;
        let name = a.name.trim();
        if !crate::secrets::valid_name(name) {
            return Err(failed(
                "a secret's name: capital letters, digits and '_', not starting with ARUGULA_, ANTHROPIC_, OPENAI_ \
                 or another reserved prefix",
            ));
        }
        self.seal(&p, &project_secret(name), &a.value).await?;
        tracing::info!(token = %self.token.id, account = %self.token.account, project = %p, name, "an agent set a project's secret");
        let v = json!({ "project": p, "name": name, "set": true });
        let text = format!("Set {name} in {p}. It's sealed, and never shown: you can't read it back.");
        Ok(Done { text, value: v, project: Some(p), note: format!("set secret {name}") })
    }

    /// Seal `value` as project `p`'s secret `stored` (its stored name), as
    /// the token's agent: never past the owner's role (an owner or editor
    /// of the project), counted against the token, and at most
    /// [`MAX_PROJECT_SECRETS`] of the project's own. The caller has asked
    /// for `secrets.write`.
    async fn seal(&self, p: &str, stored: &str, value: &str) -> Result<(), Failure> {
        self.app.limits.check_daemon(SECRET_WRITES, &self.token.id)?;
        let me = self.me();
        let (_, role) = deploys::project_as(&self.app, &me.account, p, true).await.map_err(|e| match e.0 {
            StatusCode::FORBIDDEN => {
                Failure::Refused("a viewer doesn't set a project's secrets: its owner is a viewer there".into())
            }
            _ => e.into(),
        })?;
        debug_assert!(role != TeamRole::Viewer);
        if value.is_empty() || value.len() > MAX_SECRET || value.contains('\0') {
            return Err(failed("a secret's value is 1 to 16384 bytes, with no NUL"));
        }
        let master = self
            .app
            .secret_key
            .as_deref()
            .ok_or_else(|| failed("this platform keeps no secrets (it has no master key)"))?;
        if !self.app.db.has_project_secret(p, stored)? && self.app.db.project_secrets(p)?.len() >= MAX_PROJECT_SECRETS {
            return Err(failed(format!("the project keeps {MAX_PROJECT_SECRETS} secrets of its own already")));
        }
        crate::keys::seal(&self.app.db, master, p, stored, value)?;
        self.app.db.note_secret_writer(p, stored, &self.token.account, Some(&self.token.id))?;
        Ok(())
    }
}

/// Where a project's own secret (not an environment's) is kept by name in
/// `project_secrets`: `project/<NAME>`.
pub fn project_secret(name: &str) -> String {
    format!("{}{name}", crate::db::agent_tokens::PROJECT_SECRET_PREFIX)
}

/// A tool name that would approve or deny something: refused as such.
fn never_tool(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["approv", "deny", "sign_grant", "grant_sign"].iter().any(|w| n.contains(w))
}

#[derive(Deserialize)]
struct ReadThread {
    room: String,
    #[serde(default)]
    after_seq: Option<i64>,
    #[serde(default)]
    before_seq: Option<i64>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    mentions_only: bool,
}

#[derive(Deserialize)]
struct PostThread {
    room: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    react_to_seq: Option<i64>,
    #[serde(default)]
    emoji: Option<String>,
    #[serde(default)]
    pin_seq: Option<i64>,
    #[serde(default)]
    remove: bool,
}

#[derive(Deserialize)]
struct SearchThreads {
    q: String,
    #[serde(default)]
    team: Option<String>,
}

/// `issue_write`'s arguments, every kind's together (as the daemon's
/// `Kind`s take them), and `forge_read` kind `issue`'s.
#[derive(Deserialize)]
struct IssueWrite {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    issue: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    assignees: Vec<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    add: Vec<String>,
    #[serde(default)]
    remove: Vec<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    comment: Option<String>,
}

#[derive(Deserialize)]
struct ListIssues {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    before: Option<u64>,
}

#[derive(Deserialize)]
struct StartRun {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    spec: Option<String>,
    #[serde(default)]
    command: Vec<String>,
    #[serde(default)]
    instruction: Option<String>,
    #[serde(default)]
    recipe: Option<String>,
    #[serde(default)]
    directory: Option<String>,
    #[serde(default, rename = "box")]
    on_box: Option<String>,
}

#[derive(Deserialize)]
struct ReadRun {
    #[serde(default)]
    run: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    offset: Option<u64>,
    #[serde(default)]
    max_chars: Option<u64>,
}

#[derive(Deserialize)]
struct Wait {
    #[serde(default)]
    run: Option<String>,
    /// Seconds.
    #[serde(default)]
    timeout: Option<u64>,
}

#[derive(Deserialize)]
struct PlanDeploy {
    #[serde(default)]
    project: Option<String>,
    environment: String,
    #[serde(default)]
    commit: String,
    #[serde(default)]
    repo: String,
    #[serde(default)]
    roll_back: bool,
    /// planter's ship (M233): the source, a gzipped tar, base64.
    #[serde(default)]
    files: Option<String>,
}

#[derive(Deserialize)]
struct ReadDeploy {
    id: String,
}

#[derive(Deserialize)]
struct SetSecret {
    #[serde(default)]
    project: Option<String>,
    name: String,
    value: String,
}

/// A JSON schema's property.
fn prop(kind: &str, description: &str) -> Value {
    json!({ "type": kind, "description": description })
}

fn strings(description: &str) -> Value {
    json!({ "type": "array", "items": { "type": "string" }, "description": description })
}

const PROJECT: &str = "The project: its id (prj_…) or name. Default: the token's only project.";

fn tool(name: &str, description: &str, properties: Value, required: &[&str], read_only: bool) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
        "annotations": { "readOnlyHint": read_only, "destructiveHint": false, "openWorldHint": false },
    })
}

/// What `tools/list` answers: the daemon's names where they overlap.
pub fn tools() -> Vec<Value> {
    vec![
        tool(
            "capabilities",
            "What this token lets you do: whose agent you are, how you're labelled, and the projects and powers \
             its grants give you. Never an approval.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "list",
            "kind threads: the conversations this token's grants reach (team channels and the threads on issues \
             and PRs), with what you may do in each.",
            json!({ "kind": prop("string", "threads") }),
            &["kind"],
            true,
        ),
        tool(
            "read_thread",
            "Read a conversation this token reaches (threads.read): a room's id, or an issue's or PR's id for its \
             thread. Paged by seq.",
            json!({
                "room": prop("string", "The room's id, or an issue's or PR's id (iss_…, fi_…)."),
                "after_seq": prop("integer", "Messages after this seq, oldest first."),
                "before_seq": prop("integer", "The page before this seq."),
                "limit": prop("integer", "At most this many (default 50, at most 200)."),
                "mentions_only": prop("boolean", "Only the messages that mention this agent."),
            }),
            &["room"],
            true,
        ),
        tool(
            "post_thread",
            "Post in a conversation where this token has threads.post: a room, or an issue's or PR's thread by its \
             id. Everyone there sees it as written by the owner's agent, never as the owner.",
            json!({
                "room": prop("string", "The room's id, or an issue's or PR's id."),
                "text": prop("string", "The message, in markdown. Not needed to react."),
                "react_to_seq": prop("integer", "React to this message (its seq) with emoji instead of posting."),
                "emoji": prop("string", "The emoji to react with, for react_to_seq."),
                "pin_seq": prop("integer", "Pin this message (its seq) instead of posting; the room gets a note."),
                "remove": prop("boolean", "Take your reaction back (or unpin) instead of adding it."),
            }),
            &["room"],
            false,
        ),
        tool(
            "search_threads",
            "Search the conversations this token may read (threads.read): words (mea* a prefix), in:#channel, \
             from:@login, has:quote.",
            json!({ "q": prop("string", "What to look for."), "team": prop("string", "Only one team's rooms.") }),
            &["q"],
            true,
        ),
        tool(
            "issue_write",
            "Write to Arugula's own (native) issues, at once, labelled as your owner's agent: kind create \
             (issues.write; title, body, labels, assignees), comment (threads.post; issue, text), edit (title, \
             body), label or assign (add or remove), close (reason, comment) or reopen. An issue is #N in the \
             project, or iss_….",
            json!({
                "kind": prop("string", "create, comment, edit, label, assign, close or reopen."),
                "project": prop("string", PROJECT),
                "issue": prop("string", "#N in the project, or iss_…."),
                "title": prop("string", "create, edit."),
                "body": prop("string", "create, edit: markdown."),
                "labels": strings("create."),
                "assignees": strings("create: accounts in the project."),
                "text": prop("string", "comment: markdown."),
                "add": strings("label, assign."),
                "remove": strings("label, assign."),
                "reason": prop("string", "close: completed (the default), not_planned or duplicate."),
                "comment": prop("string", "close: a comment to leave once it's closed."),
            }),
            &["kind"],
            false,
        ),
        tool(
            "forge_read",
            "Read Arugula's own (native) issues (threads.read): kind issue (one, #N or iss_…, with its timeline) or \
             native_issues (a project's, newest first; state, labels, assignee, author and q narrow it).",
            json!({
                "kind": prop("string", "issue or native_issues."),
                "project": prop("string", PROJECT),
                "issue": prop("string", "kind issue: #N or iss_…."),
                "state": prop("string", "open (the default), closed or all."),
                "labels": strings("Issues with each of these labels."),
                "assignee": prop("string", "An account."),
                "author": prop("string", "An account."),
                "q": prop("string", "Words in the title or body."),
                "limit": prop("integer", "At most this many (default 50, at most 200)."),
                "before": prop("integer", "Issues numbered below this: the next page."),
            }),
            &["kind"],
            true,
        ),
        tool(
            "start_run",
            "Start a run on one of a project's hosted boxes: a run spec by name (runs.start), or a command or an \
             instruction to an agent of your own (runs.command). It's recorded as your owner's, started by their \
             agent, under the project's standing grant (without one it waits for an owner). Returns the run \
             (run_…), pending.",
            json!({
                "project": prop("string", PROJECT),
                "spec": prop("string", "A run spec of the project, by its name or id."),
                "command": strings("Or a command: the program and its arguments, not through a shell."),
                "instruction": prop("string", "Or an instruction to an agent (Claude Code, headless)."),
                "recipe": prop("string", "With instruction: the agent recipe to run it as."),
                "directory": prop("string", "An absolute directory on the box (default its home)."),
                "box": prop("string", "Which of the project's hosted boxes: its name or id (hw_…)."),
            }),
            &[],
            false,
        ),
        tool(
            "read_run",
            "Read a run (runs.read): its state, exit code, timeline, and a page of its log by byte offset. Without \
             run: a project's latest runs.",
            json!({
                "run": prop("string", "The run (run_…)."),
                "project": prop("string", "Without run: the project to list."),
                "state": prop("string", "Without run: only runs in this state."),
                "offset": prop("integer", "The log from this byte offset (a previous read's next_offset)."),
                "max_chars": prop("integer", "At most this much log (default 16000, at most 40000)."),
            }),
            &[],
            true,
        ),
        tool(
            "wait",
            "Wait for a run to end (runs.read), up to timeout seconds (default 30, at most 120): answers \"still \
             running\" if it hasn't, to call again.",
            json!({ "run": prop("string", "The run (run_…)."), "timeout": prop("integer", "Seconds.") }),
            &["run"],
            true,
        ),
        tool(
            "plan_deploy",
            "Plan a deploy of a commit to one of a project's environments and ask the project's owners to approve \
             it (deploys.plan). Or ship an app's source (planter's ship): files, to a planter-ship environment, \
             which says the app's name, port and health path; it runs at https://<app>.run.inevitable.fyi once \
             approved. Agents plan and ask; they never approve: an owner approves (or denies) it on their phone or \
             in a browser, with a passkey. Returns the deploy (dep_…), waiting for approval.",
            json!({
                "project": prop("string", PROJECT),
                "environment": prop("string", "Its name in the project (production), or its id (env_…)."),
                "commit": prop("string", "The commit's full 40-hex id (not with files)."),
                "repo": prop("string", "OWNER/NAME on GitHub (not with files)."),
                "roll_back": prop("boolean", "Deploy a commit this environment ran before, with what it ran then."),
                "files": prop(
                    "string",
                    "To ship: the app's source, a directory with its Dockerfile at its root, as a gzipped tar, base64 \
                     (tar -czf - -C DIR . | base64 -w0), at most 2 MiB before base64.",
                ),
            }),
            &["environment"],
            false,
        ),
        tool(
            "read_deploy",
            "Read a deploy (deploys.plan over its project): where it goes, its plan, its state and its log. Reading \
             never approves.",
            json!({ "id": prop("string", "The deploy (dep_…).") }),
            &["id"],
            true,
        ),
        tool(
            "set_secret",
            "Set one of a project's secrets by name (secrets.write; your owner an owner or editor there). It's \
             sealed with the project's key and never shown, you included.",
            json!({
                "project": prop("string", PROJECT),
                "name": prop("string", "Capital letters, digits and '_'."),
                "value": prop("string", "1 to 16384 bytes."),
            }),
            &["name", "value"],
            false,
        ),
    ]
}
#[derive(Deserialize)]
pub struct UploadQuery {
    #[serde(default)]
    project: Option<String>,
}

/// `PUT /api/mcp/uploads[?project=]`: a source to ship (M235), a gzipped
/// tar as the body, by itself, as redone's agents upload one (so it never
/// passes through the agent's context). Kept as the project's artifact
/// (M233's `deploy_artifacts`), by its digest, which is the answer's
/// `upload_id`: `plan_deploy {app, upload_id}` ships it. Only an agent
/// grant token with `deploys.plan` over the project, its owner an owner or
/// editor there. At most [`planter::MAX_TARBALL`].
pub async fn upload(
    State(app): State<Arc<Platform>>,
    TokenAgent(token): TokenAgent,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> Response {
    use arugula_e2e::deploy::planter;
    let owner = token.account.clone();
    let agent = Agent { app: app.clone(), token, owner };
    let out: Result<Value, Failure> = async {
        app.limits.check_daemon(REQUESTS, &agent.token.id)?;
        app.limits.check_account(ACCOUNT_DEPLOY_ASKS, &agent.token.account)?;
        let p = agent.project(q.project.as_deref()).await?;
        agent_may(&app, &agent.me(), DEPLOYS_PLAN, &AgentScope::Project(p.clone())).await?;
        let (_, role) = deploys::project_as(&app, &agent.token.account, &p, true).await?;
        if role == TeamRole::Viewer {
            return Err(Failure::Refused("a viewer doesn't ship: its owner is a viewer there".into()));
        }
        if body.len() > planter::MAX_TARBALL {
            return Err(failed(format!("the upload is {} bytes: a ship carries at most {}", body.len(), planter::MAX_TARBALL)));
        }
        if !body.starts_with(&[0x1f, 0x8b]) {
            return Err(failed("the upload isn't gzipped: tar -czf - -C <dir> ."));
        }
        let digest = format!("sha256:{}", hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&body)));
        // Kept against the project's quota (M233's); it goes again once no
        // deploy of it waits, so an upload is planned soon after.
        let quota = arugula_e2e::deploy::planter::MAX_PROJECT_ARTIFACTS;
        if let Err(e) = app.db.put_deploy_artifact(&p, &digest, &body, &agent.token.account, now_ms(), quota) {
            return Err(match e.downcast_ref::<crate::db::deploys::OverQuota>() {
                Some(q) => failed(format!("{q}")),
                None => e.into(),
            });
        }
        Ok(json!({ "upload_id": digest, "project": p, "bytes": body.len(), "next": "plan_deploy with app and this upload_id" }))
    }
    .await;
    match out {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(Failure::Refused(why)) => (StatusCode::FORBIDDEN, Json(json!({ "error": why }))).into_response(),
        Err(Failure::Failed(why)) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": why }))).into_response(),
    }
}
