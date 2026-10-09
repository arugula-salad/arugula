//! Agent recipes (M76, #398): Claude Code subagents a machine's owner
//! offers, each with an A2A 1.0 agent card. Behind the `agents` flag; the
//! team's catalog (M77) and tasks over A2A (M78) build on it.
//!
//! - **A recipe is a Claude Code subagent file** (`.claude/agents/NAME.md`
//!   in a project, or `~/.claude/agents/NAME.md`), unchanged: YAML
//!   frontmatter (`name`, `description`, `tools`, `disallowedTools`,
//!   `model`, `permissionMode`, `skills`, `mcpServers`) and the system
//!   prompt as its body. Arugula never writes to it.
//! - **Offering one** is the machine owner's choice, kept by the machine in
//!   `<state>/agent-offers.json` (`{agent, dir}`: the recipe and the project
//!   it works in). Nothing is offered by default.
//! - **Its card** (`GET /api/a2a/agents/NAME/card`, all of them at
//!   `GET /api/a2a/agents`) is built from the recipe each time. Its
//!   interface is `arugula://MACHINE/api/a2a/agents/NAME`, JSON-RPC, with a
//!   *required* extension: it's reached over Arugula's end-to-end channel
//!   only, where the caller is the device on the other end (S34, q2).
//! - **Wearing it** in an agent block ([`wear`]) is M44's wearing of a
//!   Fountain agent, with the recipe as the source.

pub mod block;
pub mod catalog;
pub mod delegate;
pub mod manage;
pub mod tasks;
pub mod wear;

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::warn;

use crate::{acl::Principal, server::App};

/// The A2A version the cards speak.
pub const A2A_VERSION: &str = "1.0";

/// The extension every card requires: reached over Arugula's channel only.
pub const CHANNEL_EXT: &str = "https://arugula.io/a2a/ext/channel/v1";

/// The owner's list of offered recipes, in the state dir.
pub const OFFERS_FILE: &str = "agent-offers.json";

// ---------------------------------------------------------------- recipes

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum StrOrList {
    One(String),
    Many(Vec<String>),
}

impl StrOrList {
    /// Claude Code takes `tools: Read, Grep` as well as a list.
    fn list(self) -> Vec<String> {
        match self {
            StrOrList::One(s) => s.split(',').map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).collect(),
            StrOrList::Many(v) => v.into_iter().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Front {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    tools: Option<StrOrList>,
    #[serde(default, rename = "disallowedTools")]
    disallowed_tools: Option<StrOrList>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "permissionMode")]
    permission_mode: Option<String>,
    #[serde(default)]
    skills: Option<StrOrList>,
    #[serde(default, rename = "mcpServers")]
    mcp_servers: Option<serde_norway::Value>,
}

/// One of a recipe's MCP servers: one already configured for the project
/// or the user, by name, or one written into the recipe.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum RecipeServer {
    Named(String),
    Inline { name: String, config: Value },
}

/// One agent, as its Claude Code subagent file says.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Recipe {
    pub name: String,
    pub description: String,
    /// The body: its system prompt.
    pub prompt: String,
    pub tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    /// `sonnet`, `opus`, `haiku`, a model id, or none (`inherit`).
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    /// Skills, by name (in the project's or the user's `.claude/skills/`).
    pub skills: Vec<String>,
    pub mcp_servers: Vec<RecipeServer>,
    pub path: PathBuf,
}

/// `mcpServers` as Claude Code writes it in a subagent: a list of names and
/// one-key maps (`- github: {type: http, …}`), or a map of name to config.
fn servers_of(v: serde_norway::Value) -> Result<Vec<RecipeServer>, String> {
    let v: Value = serde_json::to_value(v).map_err(|e| format!("mcpServers: {e}"))?;
    let inline = |name: &str, config: &Value| -> Result<RecipeServer, String> {
        if !config.is_object() {
            return Err(format!("mcpServers: {name}'s config isn't a map"));
        }
        Ok(RecipeServer::Inline { name: name.to_owned(), config: config.clone() })
    };
    let mut out = vec![];
    match v {
        Value::Null => {}
        Value::String(s) => out.extend(StrOrList::One(s).list().into_iter().map(RecipeServer::Named)),
        Value::Array(items) => {
            for item in items {
                match item {
                    Value::String(s) => out.push(RecipeServer::Named(s)),
                    Value::Object(m) if m.len() == 1 => {
                        let (k, c) = m.into_iter().next().unwrap();
                        out.push(inline(&k, &c)?);
                    }
                    _ => return Err("mcpServers: each is a name or a one-key map of name to config".into()),
                }
            }
        }
        Value::Object(m) => {
            for (k, c) in m {
                out.push(if c.is_null() { RecipeServer::Named(k) } else { inline(&k, &c)? });
            }
        }
        _ => return Err("mcpServers: a list or a map".into()),
    }
    Ok(out)
}

/// A name a card, a URL and a file can all carry.
pub(crate) fn good_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        && !n.starts_with('.')
}

/// Parse a subagent file.
pub fn parse(text: &str, path: &Path) -> Result<Recipe, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text).replace("\r\n", "\n");
    let rest = text.strip_prefix("---\n").ok_or("no frontmatter (it starts with ---)")?;
    let (front, body) = match rest.find("\n---") {
        Some(end) => (&rest[..end], &rest[end + 4..]),
        None => return Err("the frontmatter never ends (no closing ---)".into()),
    };
    let front: Front = serde_norway::from_str(front).map_err(|e| format!("frontmatter: {e}"))?;
    if !good_name(&front.name) {
        return Err(format!("{:?} isn't a name (letters, digits, - _ .)", front.name));
    }
    let body = body.strip_prefix(|c: char| c == '-').map_or(body, |b| b.trim_start_matches('-')).trim().to_owned();
    Ok(Recipe {
        name: front.name,
        description: front.description.trim().to_owned(),
        prompt: body,
        tools: front.tools.map(StrOrList::list).unwrap_or_default(),
        disallowed_tools: front.disallowed_tools.map(StrOrList::list).unwrap_or_default(),
        model: front.model.map(|m| m.trim().to_owned()).filter(|m| !m.is_empty() && m != "inherit"),
        permission_mode: front.permission_mode.map(|m| m.trim().to_owned()).filter(|m| !m.is_empty()),
        skills: front.skills.map(StrOrList::list).unwrap_or_default(),
        mcp_servers: front.mcp_servers.map(servers_of).transpose()?.unwrap_or_default(),
        path: path.to_owned(),
    })
}

/// Where a project's recipes are, then the user's.
fn agent_dirs(dir: &Path, home: &Path) -> [PathBuf; 2] {
    [dir.join(".claude/agents"), home.join(".claude/agents")]
}

/// Every recipe a project at `dir` has, its own first (a user's recipe of
/// the same name is hidden by the project's, as in Claude Code). Files that
/// don't parse are skipped, and logged.
pub fn all(dir: &Path, home: &Path) -> Vec<Recipe> {
    let mut out: Vec<Recipe> = vec![];
    for d in agent_dirs(dir, home) {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut paths: Vec<PathBuf> =
            rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "md")).collect();
        paths.sort();
        for p in paths {
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            match parse(&text, &p) {
                Ok(r) if !out.iter().any(|o| o.name == r.name) => out.push(r),
                Ok(_) => {}
                Err(e) => warn!(path = %p.display(), error = e, "not a recipe"),
            }
        }
    }
    out
}

/// The recipe named `name` for a project at `dir`.
pub fn find(dir: &Path, home: &Path, name: &str) -> Result<Recipe, String> {
    all(dir, home)
        .into_iter()
        .find(|r| r.name == name)
        .ok_or_else(|| format!("no agent {name} in {}/.claude/agents or ~/.claude/agents", dir.display()))
}

// ---------------------------------------------------------------- offers

/// An offered recipe: its name, and the project it works in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Offer {
    pub agent: String,
    pub dir: String,
}

pub fn offers(state_dir: &Path) -> Vec<Offer> {
    std::fs::read(state_dir.join(OFFERS_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(state_dir: &Path, all: &[Offer]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(all).map_err(|e| e.to_string())?;
    crate::store::write_atomic(&state_dir.join(OFFERS_FILE), &bytes).map_err(|e| format!("can't keep the offers: {e}"))
}

/// Offer `agent` from `dir` (in place of an offer of the same name), or stop
/// offering it (`dir: None`). Answers the list as it now is.
pub fn set_offer(state_dir: &Path, home: &Path, agent: &str, dir: Option<&str>) -> Result<Vec<Offer>, String> {
    let mut list: Vec<Offer> = offers(state_dir);
    let had = list.len();
    list.retain(|o| o.agent != agent);
    match dir {
        Some(dir) => {
            let d = super::fountain::wear::expand(dir, home);
            if !d.is_absolute() {
                return Err(format!("{dir}: the project's whole path"));
            }
            let d = d.canonicalize().map_err(|e| format!("{dir}: {e}"))?;
            find(&d, home, agent)?;
            list.push(Offer { agent: agent.to_owned(), dir: d.display().to_string() });
        }
        None if list.len() == had => return Err(format!("{agent} isn't offered here")),
        None => {}
    }
    save(state_dir, &list)?;
    Ok(list)
}

// ---------------------------------------------------------------- cards

/// A recipe's A2A 1.0 agent card, as `machine` offers it.
pub fn card(machine: &str, r: &Recipe) -> Value {
    let mut tags = vec!["claude-code".to_owned()];
    tags.extend(r.model.iter().cloned());
    json!({
        "name": r.name,
        "description": r.description,
        "version": env!("CARGO_PKG_VERSION"),
        "supportedInterfaces": [{
            "url": format!("arugula://{machine}/api/a2a/agents/{}", r.name),
            "protocolBinding": "JSONRPC",
            "protocolVersion": A2A_VERSION,
        }],
        "provider": { "organization": machine, "url": "https://arugula.io" },
        "capabilities": {
            "streaming": false,
            "pushNotifications": false,
            "extensions": [{
                "uri": CHANNEL_EXT,
                "description": "Reached over Arugula's end-to-end channel (through control or directly): the caller is the device on the channel, and a task waits for this machine's owner to allow it.",
                "required": true,
            }],
        },
        "defaultInputModes": ["text/plain"],
        "defaultOutputModes": ["text/plain", "text/x-diff"],
        "skills": [{
            "id": r.name,
            "name": r.name,
            "description": r.description,
            "tags": tags,
        }],
    })
}

/// What's offered here, as cards, and any offer whose recipe is gone or
/// broken (the owner's to fix; nobody else sees why).
pub fn cards(machine: &str, state_dir: &Path, home: &Path) -> (Vec<Value>, Vec<Value>) {
    let mut cards = vec![];
    let mut broken = vec![];
    for o in offers(state_dir) {
        match find(Path::new(&o.dir), home, &o.agent) {
            Ok(r) => cards.push(card(machine, &r)),
            Err(why) => broken.push(json!({ "agent": o.agent, "dir": o.dir, "why": why })),
        }
    }
    (cards, broken)
}

// ---------------------------------------------------------------- routes

type AppState = State<Arc<App>>;

pub fn routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    r.route("/api/a2a/agents", get(list))
        .route("/api/a2a/agents/{name}/card", get(card_get))
        .route("/api/a2a/agents/{name}", post(rpc))
        .route("/api/a2a/offers", get(offers_get).post(offers_set))
        .route("/api/a2a/recipes", get(recipes_get))
        .route("/api/a2a/catalog", get(catalog_get))
        .route("/api/a2a/waiting", get(waiting_get))
        .route("/api/a2a/tasks/{id}/consent", post(consent_set))
        .route("/api/a2a/grants", get(grants_get).post(grants_revoke))
        .route("/api/a2a/delegate", post(delegate_post))
        .route("/api/a2a/recipes/all", get(recipes_all))
        .route("/api/a2a/recipes/save", post(recipes_save))
        .route("/api/a2a/recipes/delete", post(recipes_delete))
        .route("/api/a2a/recipes/projects", post(recipes_projects))
}

/// Whether the request is this machine's own account's: the owner, and (over
/// a channel) a device of this account, not a team's other owner.
pub fn own_account(
    app: &App,
    who: &Option<axum::Extension<Principal>>,
    dev: &Option<axum::Extension<crate::e2e::Caller>>,
) -> bool {
    let own = app.control.enrolled().map(|e| e.saved.cert.account.clone());
    owner(who) && dev.as_ref().is_none_or(|d| Some(&d.0.account) == own.as_ref())
}

fn refuse(status: StatusCode, why: &str) -> Response {
    (status, Json(json!({ "error": why }))).into_response()
}

/// Off unless the machine has the flag: the routes aren't there.
fn off(app: &App) -> Option<Response> {
    (!super::on(app.control.state_dir(), arugula_proto::flags::AGENTS)).then(|| {
        refuse(StatusCode::NOT_FOUND, "the agent catalog is in Labs: `arugulad flags agents on` turns it on here")
    })
}

fn owner(who: &Option<axum::Extension<Principal>>) -> bool {
    who.as_ref().is_none_or(|w| w.0.is_owner())
}

fn home(_app: &App) -> PathBuf {
    crate::home()
}

/// The agents offered here, as cards: for anyone who reaches this machine.
/// The owner also sees offers that are broken, and why.
async fn list(
    State(app): AppState,
    who: Option<axum::Extension<Principal>>,
    caller: Option<axum::Extension<crate::e2e::Caller>>,
) -> Response {
    if let Some(r) = off(&app) {
        return r;
    }
    if let Some(c) = caller {
        tracing::debug!(account = c.account, device = c.device, name = c.name, "agent cards read");
    }
    let machine = app.hosts.name().to_owned();
    let (cards, broken) = cards(&machine, app.control.state_dir(), &home(&app));
    let mut out = json!({ "machine": machine, "agents": cards });
    if owner(&who) {
        out["broken"] = json!(broken);
    }
    Json(out).into_response()
}

async fn card_get(State(app): AppState, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(r) = off(&app) {
        return r;
    }
    let Some(o) = offers(app.control.state_dir()).into_iter().find(|o| o.agent == name) else {
        return refuse(StatusCode::NOT_FOUND, &format!("{name} isn't offered here"));
    };
    match find(Path::new(&o.dir), &home(&app), &name) {
        Ok(r) => Json(card(app.hosts.name(), &r)).into_response(),
        Err(_) => refuse(StatusCode::NOT_FOUND, &format!("{name} isn't offered here")),
    }
}

/// The card's interface. Tasks are M78's: until then every method is A2A's
/// `UnsupportedOperationError`, as JSON-RPC.
/// The card's interface: A2A 1.0 JSON-RPC (`tasks.rs`).
async fn rpc(
    State(app): AppState,
    UrlPath(name): UrlPath<String>,
    dev: Option<axum::Extension<crate::e2e::Caller>>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
    body: axum::body::Bytes,
) -> Response {
    if let Some(r) = off(&app) {
        return r;
    }
    let req: Value = serde_json::from_slice(&body).unwrap_or_default();
    let id = req["id"].clone();
    let rpc_err = |code: i64, m: &str| {
        Json(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": m } })).into_response()
    };
    // A2A's version: its header (the channel carries it since #400), or
    // the query, as S34 sent it.
    let v = headers
        .get("a2a-version")
        .and_then(|v| v.to_str().ok())
        .or(q.get("a2a-version").map(String::as_str))
        .unwrap_or("0.3");
    if !v.starts_with("1.") {
        return rpc_err(-32009, &format!("A2A {v} isn't supported: this speaks {A2A_VERSION}"));
    }
    let Some(o) = offers(app.control.state_dir()).into_iter().find(|o| o.agent == name) else {
        return rpc_err(-32004, &format!("{name} isn't offered here"));
    };
    if find(Path::new(&o.dir), &home(&app), &name).is_err() {
        return rpc_err(-32004, &format!("{name} is offered but its recipe is gone"));
    }
    let claims = req["params"]["message"]["metadata"]["arugula"].clone();
    let caller = tasks::asker(&app, dev.as_ref().map(|d| &d.0), claims);
    Json(tasks::rpc(app.clone(), &name, &o.dir, caller, req).await).into_response()
}

/// Every recipe on this machine, for the Agents page.
async fn recipes_all(State(app): AppState, who: Option<axum::Extension<Principal>>) -> Response {
    if let Some(r) = off(&app) {
        return r;
    }
    if !owner(&who) {
        return refuse(StatusCode::FORBIDDEN, "the owner's");
    }
    // The repositories this machine's own panes are in.
    let panes = app.mux.api(crate::mux::Api::Panes).await.unwrap_or_default();
    let open: Vec<String> = panes
        .iter()
        .filter(|p| p.info.host.is_none())
        .filter_map(|p| p.info.project.as_ref().map(|x| x.root.clone()))
        .collect();
    Json(manage::every(app.control.state_dir(), &home(&app), &open)).into_response()
}

/// Write a recipe from the page's form: this machine's own account's.
async fn recipes_save(
    State(app): AppState,
    who: Option<axum::Extension<Principal>>,
    dev: Option<axum::Extension<crate::e2e::Caller>>,
    Json(f): Json<manage::Form>,
) -> Response {
    if let Some(r) = off(&app) {
        return r;
    }
    if !own_account(&app, &who, &dev) {
        return refuse(StatusCode::FORBIDDEN, "only this machine's own account writes its recipes");
    }
    match manage::save(&home(&app), &f) {
        Ok(p) => Json(json!({ "path": p })).into_response(),
        Err(e) => refuse(StatusCode::BAD_REQUEST, &e),
    }
}

#[derive(Deserialize)]
struct PathReq {
    path: String,
}

async fn recipes_delete(
    State(app): AppState,
    who: Option<axum::Extension<Principal>>,
    dev: Option<axum::Extension<crate::e2e::Caller>>,
    Json(r): Json<PathReq>,
) -> Response {
    if let Some(x) = off(&app) {
        return x;
    }
    if !own_account(&app, &who, &dev) {
        return refuse(StatusCode::FORBIDDEN, "only this machine's own account removes its recipes");
    }
    match manage::delete(app.control.state_dir(), &home(&app), &r.path) {
        Ok(()) => {
            app.control.poke();
            Json(json!({ "deleted": r.path })).into_response()
        }
        Err(e) => refuse(StatusCode::BAD_REQUEST, &e),
    }
}

#[derive(Deserialize)]
struct ProjectReq {
    dir: String,
    #[serde(default = "yes")]
    keep: bool,
}

fn yes() -> bool {
    true
}

/// Add a project to the page's list, or drop it.
async fn recipes_projects(
    State(app): AppState,
    who: Option<axum::Extension<Principal>>,
    Json(r): Json<ProjectReq>,
) -> Response {
    if let Some(x) = off(&app) {
        return x;
    }
    if !owner(&who) {
        return refuse(StatusCode::FORBIDDEN, "the owner's");
    }
    match manage::set_project(app.control.state_dir(), &home(&app), &r.dir, r.keep) {
        Ok(l) => Json(json!({ "projects": l })).into_response(),
        Err(e) => refuse(StatusCode::BAD_REQUEST, &e),
    }
}

#[derive(Deserialize)]
struct DelegateReq {
    /// send, get, answer or cancel.
    kind: String,
    machine: String,
    agent: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    task: Option<String>,
    /// Seconds to wait for it to finish or ask (default 0).
    #[serde(default)]
    wait: u64,
    /// review, apply: the checkout.
    #[serde(default)]
    dir: Option<String>,
    /// review: the pane to open its diff block beside.
    #[serde(default)]
    beside: Option<arugula_proto::PaneId>,
}

/// A task for another machine's agent, as this machine's owner (the CLI's;
/// agents use MCP's `delegate`).
async fn delegate_post(
    State(app): AppState,
    who: Option<axum::Extension<Principal>>,
    dev: Option<axum::Extension<crate::e2e::Caller>>,
    Json(r): Json<DelegateReq>,
) -> Response {
    if let Some(x) = off(&app) {
        return x;
    }
    if !own_account(&app, &who, &dev) {
        return refuse(StatusCode::FORBIDDEN, "this machine's own account delegates from it");
    }
    // M80: a kept result, reviewed or applied here.
    if r.kind == "review" || r.kind == "apply" {
        let (Some(task), Some(dir)) = (r.task.as_deref(), r.dir.as_deref()) else {
            return refuse(StatusCode::BAD_REQUEST, "review and apply take task and dir");
        };
        let state = app.control.state_dir().to_owned();
        if r.kind == "apply" {
            return match delegate::apply(&state, &delegate::review_root(), task, Path::new(dir)).await {
                Ok(a) => Json(json!({ "applied": a, "summary": a.text(task) })).into_response(),
                Err(e) => refuse(StatusCode::BAD_REQUEST, &e),
            };
        }
        let (wt, a) = match delegate::review(&state, &delegate::review_root(), task, Path::new(dir)).await {
            Ok(x) => x,
            Err(e) => return refuse(StatusCode::BAD_REQUEST, &e),
        };
        let req = arugula_proto::api::OpenRequest {
            kind: arugula_proto::BlockType::Diff,
            config: json!({ "repo": wt }),
            split: r.beside,
            from_pane: r.beside,
            local: true,
            ..Default::default()
        };
        let block = match app.mux.api(|x| crate::mux::Api::Open(req, None, x)).await {
            Some(Ok(b)) => b,
            Some(Err(e)) => return refuse(StatusCode::BAD_REQUEST, &e),
            None => return refuse(StatusCode::SERVICE_UNAVAILABLE, "the daemon is stopping"),
        };
        let summary = format!("Task {task}'s patch is in diff block %{block}, on a copy of {dir}. {}", a.text(task));
        return Json(json!({ "block": block, "worktree": wt, "applied": a, "summary": summary })).into_response();
    }
    let (m, a) = (r.machine.as_str(), r.agent.as_str());
    let claims = json!({ "machine": app.hosts.name() });
    let text = r.text.as_deref().filter(|t| !t.trim().is_empty());
    let task = r.task.as_deref().filter(|t| !t.is_empty());
    let got = match (r.kind.as_str(), text, task) {
        ("send", Some(t), _) => delegate::send(&app, m, a, t, None, claims).await,
        ("answer", Some(t), Some(id)) => delegate::send(&app, m, a, t, Some(id), claims).await,
        ("get", _, Some(id)) => delegate::get(&app, m, a, id).await,
        ("cancel", _, Some(id)) => delegate::cancel(&app, m, a, id).await,
        _ => Err("kind: send {text}, answer {task, text}, get {task} or cancel {task}".into()),
    };
    let got = match got {
        Ok(t) if r.kind != "cancel" && r.wait > 0 && !delegate::settled(&t) => match t["id"].as_str() {
            Some(id) => delegate::wait(&app, m, a, id, std::time::Duration::from_secs(r.wait.min(1800))).await,
            None => Ok(t),
        },
        other => other,
    };
    match got {
        Ok(t) => Json(json!({ "task": t, "summary": delegate::summary(m, a, &t) })).into_response(),
        Err(e) => refuse(StatusCode::BAD_GATEWAY, &e),
    }
}

/// Tasks waiting for this machine's own account.
async fn waiting_get(
    State(app): AppState,
    who: Option<axum::Extension<Principal>>,
    dev: Option<axum::Extension<crate::e2e::Caller>>,
) -> Response {
    if let Some(r) = off(&app) {
        return r;
    }
    if !own_account(&app, &who, &dev) {
        return refuse(StatusCode::FORBIDDEN, "this machine's own account's");
    }
    let v: Vec<Value> = tasks::waiting().iter().map(|t| t.json(None)).collect();
    Json(json!({ "waiting": v })).into_response()
}

#[derive(Deserialize)]
struct ConsentReq {
    /// once, hour or deny.
    answer: String,
}

/// The owner's answer to a waiting task (the CLI's; the page answers the
/// card): only this machine's own account, never a team's other owner.
async fn consent_set(
    State(app): AppState,
    UrlPath(id): UrlPath<String>,
    who: Option<axum::Extension<Principal>>,
    dev: Option<axum::Extension<crate::e2e::Caller>>,
    Json(req): Json<ConsentReq>,
) -> Response {
    if let Some(r) = off(&app) {
        return r;
    }
    if !own_account(&app, &who, &dev) {
        tracing::warn!(task = id, "refused a task's answer from another account");
        return refuse(StatusCode::FORBIDDEN, "only this machine's own account allows tasks for its agents");
    }
    let Some(a) = tasks::Answer::parse(&req.answer) else {
        return refuse(StatusCode::BAD_REQUEST, "answer: once, hour or deny");
    };
    match tasks::answer(&id, a) {
        Ok(()) => {
            block::consent_settled(&app);
            Json(json!({ "task": id, "answer": req.answer })).into_response()
        }
        Err(e) => refuse(StatusCode::NOT_FOUND, &e),
    }
}

async fn grants_get(
    State(app): AppState,
    who: Option<axum::Extension<Principal>>,
    dev: Option<axum::Extension<crate::e2e::Caller>>,
) -> Response {
    if let Some(r) = off(&app) {
        return r;
    }
    if !own_account(&app, &who, &dev) {
        return refuse(StatusCode::FORBIDDEN, "this machine's own account's");
    }
    Json(tasks::grants(app.control.state_dir())).into_response()
}

#[derive(Deserialize)]
struct RevokeReq {
    account: String,
    agent: String,
}

/// Take a standing grant back.
async fn grants_revoke(
    State(app): AppState,
    who: Option<axum::Extension<Principal>>,
    dev: Option<axum::Extension<crate::e2e::Caller>>,
    Json(req): Json<RevokeReq>,
) -> Response {
    if let Some(r) = off(&app) {
        return r;
    }
    if !own_account(&app, &who, &dev) {
        return refuse(StatusCode::FORBIDDEN, "only this machine's own account takes grants back");
    }
    match tasks::revoke(app.control.state_dir(), &req.account, &req.agent) {
        Ok(had) => Json(json!({ "revoked": had })).into_response(),
        Err(e) => refuse(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn offers_get(State(app): AppState, who: Option<axum::Extension<Principal>>) -> Response {
    if let Some(r) = off(&app) {
        return r;
    }
    if !owner(&who) {
        return refuse(StatusCode::FORBIDDEN, "only this machine's owner sees what it offers");
    }
    Json(offers(app.control.state_dir())).into_response()
}

#[derive(Deserialize)]
struct OfferReq {
    agent: String,
    /// The project it works in; none to stop offering it.
    #[serde(default)]
    dir: Option<String>,
}

/// Offer a recipe, or stop: the owner's.
async fn offers_set(
    State(app): AppState,
    who: Option<axum::Extension<Principal>>,
    Json(req): Json<OfferReq>,
) -> Response {
    if let Some(r) = off(&app) {
        return r;
    }
    if !owner(&who) {
        return refuse(StatusCode::FORBIDDEN, "only this machine's owner offers its agents");
    }
    match set_offer(app.control.state_dir(), &home(&app), &req.agent, req.dir.as_deref()) {
        Ok(list) => {
            // Who gets in follows what's offered (#399): tell control now.
            app.control.poke();
            Json(list).into_response()
        }
        Err(e) => refuse(StatusCode::BAD_REQUEST, &e),
    }
}

#[derive(Deserialize)]
struct CatalogQuery {
    /// `1` (or `true`): ask every machine now.
    #[serde(default)]
    fresh: Option<String>,
}

/// The team's catalog (#399), as this machine sees it: the owner's (and
/// their agents', through MCP).
async fn catalog_get(
    State(app): AppState,
    who: Option<axum::Extension<Principal>>,
    axum::extract::Query(q): axum::extract::Query<CatalogQuery>,
) -> Response {
    if let Some(r) = off(&app) {
        return r;
    }
    if !owner(&who) {
        return refuse(StatusCode::FORBIDDEN, "the owner's");
    }
    let fresh = matches!(q.fresh.as_deref(), Some("1" | "true"));
    Json(catalog::get(&app, if fresh { 0 } else { catalog::FRESH_MS }).await).into_response()
}

#[derive(Deserialize)]
struct RecipesQuery {
    dir: String,
}

/// The recipes a project has (and the user's), offered or not: the owner's,
/// to choose what to offer.
async fn recipes_get(
    State(app): AppState,
    who: Option<axum::Extension<Principal>>,
    axum::extract::Query(q): axum::extract::Query<RecipesQuery>,
) -> Response {
    if let Some(r) = off(&app) {
        return r;
    }
    if !owner(&who) {
        return refuse(StatusCode::FORBIDDEN, "the owner's");
    }
    let home = home(&app);
    let dir = super::fountain::wear::expand(&q.dir, &home);
    let offered: BTreeMap<String, String> =
        offers(app.control.state_dir()).into_iter().map(|o| (o.agent, o.dir)).collect();
    let list: Vec<Value> = all(&dir, &home)
        .into_iter()
        .map(|r| {
            let mut v = serde_json::to_value(&r).unwrap_or_default();
            v["offered_from"] = json!(offered.get(&r.name));
            v
        })
        .collect();
    Json(list).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXER: &str = "---\nname: fixer\ndescription: Fixes failing tests\ntools: Read, Edit, Bash\nmodel: haiku\npermissionMode: acceptEdits\nskills:\n  - testing\nmcpServers:\n  - github\n  - docs:\n      type: http\n      url: https://example.com/mcp\n---\n\nYou fix tests.\n";

    #[test]
    fn a_subagent_file_is_a_recipe() {
        let r = parse(FIXER, Path::new("/p/.claude/agents/fixer.md")).unwrap();
        assert_eq!(r.name, "fixer");
        assert_eq!(r.tools, ["Read", "Edit", "Bash"]);
        assert_eq!(r.model.as_deref(), Some("haiku"));
        assert_eq!(r.permission_mode.as_deref(), Some("acceptEdits"));
        assert_eq!(r.skills, ["testing"]);
        assert_eq!(r.mcp_servers[0], RecipeServer::Named("github".into()));
        assert!(matches!(&r.mcp_servers[1], RecipeServer::Inline { name, .. } if name == "docs"));
        assert_eq!(r.prompt, "You fix tests.");
    }

    #[test]
    fn inherit_is_no_model_and_crlf_parses() {
        let r = parse("---\r\nname: a\r\nmodel: inherit\r\n---\r\nBody\r\n", Path::new("a.md")).unwrap();
        assert_eq!(r.model, None);
        assert_eq!(r.prompt, "Body");
    }

    #[test]
    fn what_isnt_a_recipe_says_why() {
        assert!(parse("no front", Path::new("a.md")).unwrap_err().contains("frontmatter"));
        assert!(parse("---\nname: a\n", Path::new("a.md")).unwrap_err().contains("never ends"));
        assert!(parse("---\nname: a/b\n---\n", Path::new("a.md")).unwrap_err().contains("isn't a name"));
        assert!(parse("---\nname: a\nmcpServers: [3]\n---\n", Path::new("a.md")).is_err());
    }

    #[test]
    fn the_project_hides_the_users_recipe_and_offers_keep_dirs() {
        let t = std::env::temp_dir().join(format!("arugula-recipes-{}-{}", std::process::id(), crate::store::now_ms()));
        let (proj, home, state) = (t.join("p"), t.join("h"), t.join("s"));
        for d in [proj.join(".claude/agents"), home.join(".claude/agents"), state.clone()] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(proj.join(".claude/agents/fixer.md"), FIXER).unwrap();
        std::fs::write(home.join(".claude/agents/fixer.md"), FIXER.replace("haiku", "opus")).unwrap();
        std::fs::write(home.join(".claude/agents/mine.md"), "---\nname: mine\n---\nhi").unwrap();
        std::fs::write(home.join(".claude/agents/junk.md"), "nope").unwrap();
        let rs = all(&proj, &home);
        assert_eq!(rs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["fixer", "mine"]);
        assert_eq!(rs[0].model.as_deref(), Some("haiku"));

        assert!(set_offer(&state, &home, "nobody", Some(proj.to_str().unwrap())).is_err());
        assert!(set_offer(&state, &home, "fixer", Some("relative")).is_err());
        let l = set_offer(&state, &home, "fixer", Some(proj.to_str().unwrap())).unwrap();
        assert_eq!(l, [Offer { agent: "fixer".into(), dir: proj.canonicalize().unwrap().display().to_string() }]);
        let (got, broken) = cards("box", &state, &home);
        assert!(broken.is_empty());
        assert_eq!(got[0]["supportedInterfaces"][0]["url"], "arugula://box/api/a2a/agents/fixer");
        assert_eq!(got[0]["capabilities"]["extensions"][0]["required"], true);

        std::fs::remove_file(proj.join(".claude/agents/fixer.md")).unwrap();
        std::fs::remove_file(home.join(".claude/agents/fixer.md")).unwrap();
        let (got, broken) = cards("box", &state, &home);
        assert!(got.is_empty());
        assert_eq!(broken[0]["agent"], "fixer");

        assert!(set_offer(&state, &home, "fixer", None).unwrap().is_empty());
        assert!(set_offer(&state, &home, "fixer", None).is_err());
        let _ = std::fs::remove_dir_all(&t);
    }
}
