//! S34 spike: agents a person offers to other people's agents, over A2A
//! (v1.0, the JSON-RPC binding), carried on illogical's own channel.
//!
//! - **A recipe is a Claude Code subagent file** (`.claude/agents/NAME.md`
//!   in a project, or `~/.claude/agents/NAME.md`): YAML frontmatter
//!   (`name`, `description`, `tools`, `disallowedTools`, `model`, …) and
//!   the system prompt as its body. Nothing here is Fountain's.
//! - **Offering one** is this machine's owner's choice
//!   (`POST /api/a2a/offers`), kept in `<state>/a2a-offers.json`.
//! - **Its agent card** (`GET /api/a2a/agents/NAME/card`) is built from
//!   the recipe. Its interface is `illogical://MACHINE/api/a2a/agents/NAME`:
//!   JSON-RPC over the end-to-end channel, where the caller is the device
//!   on the other end of it (no bearer tokens).
//! - **A task** (`POST /api/a2a/agents/NAME`, `SendMessage`) runs the
//!   recipe as an agent block in a git worktree of the project, after the
//!   owner allows it on a card (a standing grant for an hour skips that).
//!   It ends with the agent's last reply and a patch against the commit it
//!   started from, as artifacts.
//!
//! Spike quality: tasks and grants live in memory, and a restart forgets
//! them.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path as UrlPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::{acl::Principal, mux::Api, server::App, store::now_ms};
use illogical_proto::{BlockType, PaneId, api::OpenRequest};

/// The A2A version spoken (`A2A-Version`).
pub const VERSION: &str = "1.0";
/// How long "Allow for an hour" lasts.
const GRANT: Duration = Duration::from_secs(3600);
/// The extension a card names: reached over illogical's channel only.
const EXT: &str = "https://illogical.widgets.wtf/a2a/ext/channel/v1";

type AppState = State<Arc<App>>;

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/api/a2a/agents", get(list))
        .route("/api/a2a/offers", get(offers_get).post(offers_set))
        .route("/api/a2a/agents/{name}/card", get(card_get))
        .route("/api/a2a/agents/{name}", post(rpc))
        .route("/api/a2a/waiting", get(waiting))
        .route("/api/a2a/tasks/{id}/consent", post(consent_set))
}

// ---------------------------------------------------------------- recipes

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum StrOrList {
    One(String),
    Many(Vec<String>),
}

impl StrOrList {
    fn list(self) -> Vec<String> {
        match self {
            StrOrList::One(s) => s.split(',').map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).collect(),
            StrOrList::Many(v) => v,
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

/// One agent, as its Claude Code subagent file says.
#[derive(Debug, Clone, Serialize)]
pub struct Recipe {
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    /// Named skills (not carried yet: the block runs with no settings).
    pub skills: Vec<String>,
    /// Whether it names MCP servers (not carried yet).
    pub mcp_servers: bool,
    pub path: PathBuf,
}

/// Parse a subagent file.
pub fn parse(text: &str, path: &Path) -> Result<Recipe, String> {
    let rest = text.strip_prefix("---").ok_or("no frontmatter (it starts with ---)")?;
    let end = rest.find("\n---").ok_or("the frontmatter never ends (no closing ---)")?;
    let front: Front = serde_norway::from_str(&rest[..end]).map_err(|e| format!("frontmatter: {e}"))?;
    let body = rest[end + 4..].trim_start_matches(['-']).trim().to_owned();
    if front.name.is_empty() || front.name.contains(':') {
        return Err("a name, without ':'".into());
    }
    Ok(Recipe {
        name: front.name,
        description: front.description,
        prompt: body,
        tools: front.tools.map(StrOrList::list).unwrap_or_default(),
        disallowed_tools: front.disallowed_tools.map(StrOrList::list).unwrap_or_default(),
        model: front.model,
        permission_mode: front.permission_mode,
        skills: front.skills.map(StrOrList::list).unwrap_or_default(),
        mcp_servers: front.mcp_servers.is_some(),
        path: path.to_owned(),
    })
}

/// The recipe named `name` for a project at `dir`: its own
/// `.claude/agents/`, then the user's.
pub fn find(dir: &Path, home: &Path, name: &str) -> Result<Recipe, String> {
    for d in [dir.join(".claude/agents"), home.join(".claude/agents")] {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "md") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            match parse(&text, &p) {
                Ok(r) if r.name == name => return Ok(r),
                Ok(_) => {}
                Err(e) => warn!(path = %p.display(), error = e, "not a recipe"),
            }
        }
    }
    Err(format!("no agent {name} in {}/.claude/agents or ~/.claude/agents", dir.display()))
}

// ---------------------------------------------------------------- offers

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Offer {
    pub agent: String,
    /// The project it works in (a git checkout).
    pub dir: String,
}

fn offers_file(app: &App) -> PathBuf {
    app.control.state_dir().join("a2a-offers.json")
}

fn offers(app: &App) -> Vec<Offer> {
    std::fs::read(offers_file(app)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into())
}

fn offered(app: &App, name: &str) -> Result<(Offer, Recipe), String> {
    let o = offers(app).into_iter().find(|o| o.agent == name).ok_or_else(|| format!("{name} isn't offered here"))?;
    let r = find(Path::new(&o.dir), &home(), name)?;
    Ok((o, r))
}

fn refuse(status: StatusCode, why: &str) -> Response {
    (status, Json(json!({ "error": why }))).into_response()
}

fn owner(who: &Option<axum::Extension<Principal>>) -> bool {
    who.as_ref().is_none_or(|w| w.0.is_owner())
}

async fn offers_get(State(app): AppState, who: Option<axum::Extension<Principal>>) -> Response {
    if !owner(&who) {
        return refuse(StatusCode::FORBIDDEN, "the owner's");
    }
    Json(offers(&app)).into_response()
}

#[derive(Deserialize)]
struct OfferReq {
    agent: String,
    #[serde(default)]
    dir: Option<String>,
    #[serde(default = "yes")]
    offer: bool,
}

fn yes() -> bool {
    true
}

/// Offer an agent (or stop): the owner's.
async fn offers_set(
    State(app): AppState,
    who: Option<axum::Extension<Principal>>,
    Json(req): Json<OfferReq>,
) -> Response {
    if !owner(&who) {
        return refuse(StatusCode::FORBIDDEN, "only this machine's owner offers its agents");
    }
    let mut all: Vec<Offer> = offers(&app).into_iter().filter(|o| o.agent != req.agent).collect();
    if req.offer {
        let Some(dir) = req.dir else { return refuse(StatusCode::BAD_REQUEST, "dir: the project it works in") };
        if let Err(e) = find(Path::new(&dir), &home(), &req.agent) {
            return refuse(StatusCode::BAD_REQUEST, &e);
        }
        all.push(Offer { agent: req.agent, dir });
    }
    if let Err(e) = crate::store::write_atomic(&offers_file(&app), &serde_json::to_vec_pretty(&all).unwrap()) {
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }
    Json(all).into_response()
}

// ---------------------------------------------------------------- cards

pub fn card(machine: &str, r: &Recipe) -> Value {
    json!({
        "name": r.name,
        "description": r.description,
        "version": env!("CARGO_PKG_VERSION"),
        "supportedInterfaces": [{
            "url": format!("illogical://{machine}/api/a2a/agents/{}", r.name),
            "protocolBinding": "JSONRPC",
            "protocolVersion": VERSION,
        }],
        "provider": { "organization": machine, "url": "https://illogical.widgets.wtf" },
        "capabilities": {
            "streaming": false,
            "pushNotifications": false,
            "extensions": [{
                "uri": EXT,
                "description": "Reached over illogical's end-to-end channel (through control or directly); the caller is the device on the channel, and a task waits for this machine's owner to allow it.",
                "required": true,
            }],
        },
        "defaultInputModes": ["text/plain"],
        "defaultOutputModes": ["text/plain", "text/x-diff"],
        "skills": [{
            "id": r.name,
            "name": r.name,
            "description": r.description,
            "tags": ["claude-code"],
        }],
    })
}

/// The agents offered here, as cards (anyone who reaches this machine).
async fn list(State(app): AppState) -> Response {
    let machine = app.hosts.name().to_owned();
    let mut cards = vec![];
    let mut broken = vec![];
    for o in offers(&app) {
        match find(Path::new(&o.dir), &home(), &o.agent) {
            Ok(r) => cards.push(card(&machine, &r)),
            Err(e) => broken.push(json!({ "agent": o.agent, "why": e })),
        }
    }
    Json(json!({ "machine": machine, "agents": cards, "broken": broken })).into_response()
}

async fn card_get(State(app): AppState, UrlPath(name): UrlPath<String>) -> Response {
    match offered(&app, &name) {
        Ok((_, r)) => Json(card(app.hosts.name(), &r)).into_response(),
        Err(e) => refuse(StatusCode::NOT_FOUND, &e),
    }
}

// ---------------------------------------------------------------- tasks

/// Who sent a task: the channel's principal, plus what its message says
/// about where it came from (a claim, shown as one).
#[derive(Debug, Clone, Serialize)]
struct Caller {
    /// `account:…` (or `owner` for this machine's own account).
    id: String,
    name: String,
    /// The device on the channel (its name), proven by its key.
    device: Option<String>,
    /// From the message's `metadata.illogical`: machine and pane.
    claims: Value,
}

#[derive(Debug, Clone)]
struct Task {
    id: String,
    context: String,
    agent: String,
    caller: Caller,
    state: &'static str,
    status: Option<String>,
    block: Option<PaneId>,
    worktree: Option<PathBuf>,
    base: Option<String>,
    artifacts: Vec<Value>,
    history: Vec<Value>,
    at_ms: u64,
}

struct Live {
    task: Task,
    tx: watch::Sender<u64>,
}

static TASKS: LazyLock<Mutex<HashMap<String, Live>>> = LazyLock::new(Default::default);
/// (caller id, agent) → allowed until (ms).
static GRANTS: LazyLock<Mutex<HashMap<(String, String), u64>>> = LazyLock::new(Default::default);

const TERMINAL: [&str; 4] = ["TASK_STATE_COMPLETED", "TASK_STATE_FAILED", "TASK_STATE_CANCELED", "TASK_STATE_REJECTED"];
const INTERRUPTED: [&str; 2] = ["TASK_STATE_INPUT_REQUIRED", "TASK_STATE_AUTH_REQUIRED"];

fn agent_message(text: &str, task: &str, context: &str) -> Value {
    json!({
        "messageId": format!("m{}", now_ms()),
        "role": "ROLE_AGENT",
        "taskId": task,
        "contextId": context,
        "parts": [{ "text": text }],
    })
}

impl Task {
    fn json(&self, history: Option<usize>) -> Value {
        let mut status = json!({ "state": self.state, "timestamp": iso(now_ms()) });
        if let Some(s) = &self.status {
            status["message"] = agent_message(s, &self.id, &self.context);
        }
        let mut t = json!({
            "id": self.id,
            "contextId": self.context,
            "status": status,
            "artifacts": self.artifacts,
            "metadata": { "illogical": {
                "agent": self.agent,
                "block": self.block,
                "base": self.base,
                "caller": self.caller,
                "at_ms": self.at_ms,
            } },
        });
        let h: Vec<Value> = match history {
            Some(0) => vec![],
            Some(n) => self.history.iter().rev().take(n).rev().cloned().collect(),
            None => self.history.clone(),
        };
        if !h.is_empty() {
            t["history"] = json!(h);
        }
        t
    }
}

fn iso(ms: u64) -> String {
    let s = (ms / 1000) as i64;
    time_fmt(s, ms % 1000)
}

/// RFC 3339 in UTC without a date crate.
fn time_fmt(secs: i64, ms: u64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil from days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{ms:03}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn update(id: &str, f: impl FnOnce(&mut Task)) {
    let mut g = TASKS.lock().unwrap();
    if let Some(l) = g.get_mut(id) {
        f(&mut l.task);
        l.tx.send_modify(|v| *v += 1);
    }
}

fn snapshot(id: &str) -> Option<Task> {
    TASKS.lock().unwrap().get(id).map(|l| l.task.clone())
}

// ---------------------------------------------------------------- JSON-RPC

fn rpc_err(id: &Value, code: i64, message: &str) -> Response {
    Json(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })).into_response()
}

fn rpc_ok(id: &Value, result: Value) -> Response {
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
}

/// Who's calling: the account of the device on the channel, not its role
/// here (every owner of a team is `Owner` on its machines, whoever they
/// are). Over the local socket or loopback, it's this machine's own.
fn caller_of(app: &App, dev: &Option<axum::Extension<crate::e2e::Caller>>, claims: Value) -> Caller {
    let own = app.control.enrolled().map(|e| e.saved.cert.account.clone());
    match dev.as_ref().map(|d| &d.0) {
        Some(d) if Some(&d.account) != own.as_ref() => Caller {
            id: format!("account:{}", d.account),
            name: app.control.name_of_account(&d.account).unwrap_or_else(|| d.account.clone()),
            device: Some(format!("{} ({})", d.name, d.device)),
            claims,
        },
        _ => {
            Caller { id: "owner".into(), name: "owner".into(), device: dev.as_ref().map(|d| d.0.name.clone()), claims }
        }
    }
}

async fn rpc(
    State(app): AppState,
    UrlPath(name): UrlPath<String>,
    dev: Option<axum::Extension<crate::e2e::Caller>>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
    Json(req): Json<Value>,
) -> Response {
    let id = req["id"].clone();
    // The end-to-end channel carries a request's content type and no other
    // header, so A2A's service parameters may come in the query instead.
    let v = headers
        .get("a2a-version")
        .and_then(|v| v.to_str().ok())
        .or(q.get("a2a-version").map(String::as_str))
        .unwrap_or("0.3");
    if !v.starts_with("1.") {
        return rpc_err(&id, -32009, &format!("A2A {v} isn't supported; this speaks {VERSION}"));
    }
    let (offer, recipe) = match offered(&app, &name) {
        Ok(x) => x,
        Err(_) => return rpc_err(&id, -32004, &format!("{name} isn't offered here")),
    };
    let p = &req["params"];
    let caller = caller_of(&app, &dev, p["message"]["metadata"]["illogical"].clone());
    match req["method"].as_str().unwrap_or_default() {
        "SendMessage" => send(app, id, offer, recipe, caller, p).await,
        "GetTask" => {
            let Some(t) = p["id"].as_str().and_then(snapshot).filter(|t| may_see(&caller, t)) else {
                return rpc_err(&id, -32001, "no such task");
            };
            rpc_ok(&id, t.json(p["historyLength"].as_u64().map(|n| n as usize)))
        }
        "ListTasks" => {
            let all: Vec<Value> = TASKS
                .lock()
                .unwrap()
                .values()
                .filter(|l| l.task.agent == name && may_see(&caller, &l.task))
                .map(|l| l.task.json(Some(0)))
                .collect();
            rpc_ok(&id, json!({ "tasks": all, "nextPageToken": "" }))
        }
        "CancelTask" => {
            let Some(t) = p["id"].as_str().and_then(snapshot).filter(|t| may_see(&caller, t)) else {
                return rpc_err(&id, -32001, "no such task");
            };
            if TERMINAL.contains(&t.state) {
                return rpc_err(&id, -32002, "it has already ended");
            }
            if let Some(b) = t.block
                && let Some(Some(block)) = app.mux.api(|r| Api::Block(b, r)).await
            {
                let _ = block.call("cancel", json!({})).await;
            }
            update(&t.id, |t| {
                t.state = "TASK_STATE_CANCELED";
                t.status = Some(format!("canceled by {}", caller.name));
            });
            rpc_ok(&id, snapshot(&t.id).map(|t| t.json(Some(0))).unwrap_or_default())
        }
        m => rpc_err(
            &id,
            -32601,
            &format!("{m}: not a method this speaks (SendMessage, GetTask, ListTasks, CancelTask)"),
        ),
    }
}

fn may_see(c: &Caller, t: &Task) -> bool {
    c.id == "owner" || c.id == t.caller.id
}

fn text_of(message: &Value) -> String {
    message["parts"].as_array().into_iter().flatten().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n")
}

async fn send(app: Arc<App>, id: Value, offer: Offer, recipe: Recipe, caller: Caller, p: &Value) -> Response {
    let message = p["message"].clone();
    let text = text_of(&message);
    if text.trim().is_empty() {
        return rpc_err(&id, -32602, "the message has no text part");
    }
    let quick = p["configuration"]["returnImmediately"].as_bool().unwrap_or(false);
    // A follow-up on a task this caller has: the next turn, or the answer
    // it was waiting for.
    let task_id = match message["taskId"].as_str() {
        Some(t) => {
            let Some(task) = snapshot(t).filter(|x| may_see(&caller, x)) else {
                return rpc_err(&id, -32001, "no such task");
            };
            if !matches!(task.state, "TASK_STATE_INPUT_REQUIRED" | "TASK_STATE_COMPLETED") {
                return rpc_err(&id, -32602, &format!("the task is {}; send to it once it waits", task.state));
            }
            update(t, |x| {
                x.history.push(message.clone());
                x.state = "TASK_STATE_WORKING";
                x.status = None;
            });
            let answering = task.state == "TASK_STATE_INPUT_REQUIRED";
            tokio::spawn(turn(app.clone(), t.to_owned(), text, answering));
            t.to_owned()
        }
        None => {
            let t = format!("t{}", now_ms());
            let context = message["contextId"].as_str().map(str::to_owned).unwrap_or_else(|| format!("c{}", now_ms()));
            let (tx, _) = watch::channel(0);
            let task = Task {
                id: t.clone(),
                context,
                agent: recipe.name.clone(),
                caller: caller.clone(),
                state: "TASK_STATE_SUBMITTED",
                status: None,
                block: None,
                worktree: None,
                base: None,
                artifacts: vec![],
                history: vec![message.clone()],
                at_ms: now_ms(),
            };
            TASKS.lock().unwrap().insert(t.clone(), Live { task, tx });
            info!(task = t, agent = recipe.name, caller = caller.name, "A2A task");
            tokio::spawn(start(app.clone(), t.clone(), offer, recipe, text));
            t
        }
    };
    if !quick {
        // Until it ends or waits on someone.
        let mut rx = TASKS.lock().unwrap().get(&task_id).map(|l| l.tx.subscribe());
        while let Some(r) = rx.as_mut() {
            let s = snapshot(&task_id).map(|t| t.state).unwrap_or("TASK_STATE_FAILED");
            if TERMINAL.contains(&s) || INTERRUPTED.contains(&s) {
                break;
            }
            if r.changed().await.is_err() {
                break;
            }
        }
    }
    rpc_ok(&id, json!({ "task": snapshot(&task_id).map(|t| t.json(Some(0))) }))
}

async fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn fail(t: &str, why: String) {
    warn!(task = t, why, "A2A task failed");
    update(t, |x| {
        x.state = "TASK_STATE_FAILED";
        x.status = Some(why);
    });
}

/// A new task: a worktree, an agent block, the owner's OK, then its turn.
async fn start(app: Arc<App>, t: String, offer: Offer, recipe: Recipe, text: String) {
    let dir = PathBuf::from(&offer.dir);
    let base = match git(&dir, &["rev-parse", "HEAD"]).await {
        Ok(b) => b.trim().to_owned(),
        Err(e) => return fail(&t, e),
    };
    let wt = app.control.state_dir().join("a2a").join(&t);
    if let Err(e) = git(&dir, &["worktree", "add", "-q", "--detach", &wt.display().to_string(), &base]).await {
        return fail(&t, e);
    }
    let mut config = json!({
        "agent": "claude",
        "cwd": wt.display().to_string(),
        "recipe": {
            "name": recipe.name,
            "prompt": recipe.prompt,
            "model": recipe.model,
            "tools": recipe.tools,
            "disallowedTools": recipe.disallowed_tools,
        },
    });
    if let Some(m) = &recipe.permission_mode
        && !["bypassPermissions", "dontAsk"].contains(&m.as_str())
    {
        config["permission_mode"] = json!(m);
    }
    let req = OpenRequest {
        kind: BlockType::Agent,
        config,
        session: Some("a2a".into()),
        split: None,
        from_pane: None,
        vm: false,
        image: None,
        host: None,
        local: true,
    };
    let block = match app.mux.api(|r| Api::Open(req, None, r)).await {
        Some(Ok(b)) => b,
        Some(Err(e)) => return fail(&t, format!("opening its block: {e}")),
        None => return fail(&t, "the daemon is shutting down".into()),
    };
    let caller = snapshot(&t).map(|x| x.caller).unwrap();
    update(&t, |x| {
        x.block = Some(block);
        x.worktree = Some(wt.clone());
        x.base = Some(base.clone());
    });
    // The owner's OK, unless it's theirs or a grant stands.
    if caller.id != "owner" {
        let key = (caller.id.clone(), recipe.name.clone());
        let granted = GRANTS.lock().unwrap().get(&key).is_some_and(|until| *until > now_ms());
        if !granted {
            update(&t, |x| {
                x.status = Some(format!("waiting for this machine's owner to allow {}'s task", caller.name))
            });
            info!(task = t, by = caller.name, device = ?caller.device, "A2A task waits for the owner");
            match consent(&t).await {
                Ok(Some(hour)) => {
                    if hour {
                        GRANTS.lock().unwrap().insert(key, now_ms() + GRANT.as_millis() as u64);
                    }
                }
                Ok(None) => {
                    update(&t, |x| {
                        x.state = "TASK_STATE_REJECTED";
                        x.status = Some("this machine's owner said no".into());
                    });
                    return;
                }
                Err(e) => return fail(&t, e),
            }
        }
    }
    update(&t, |x| {
        x.state = "TASK_STATE_WORKING";
        x.status = None;
    });
    turn(app, t, text, false).await;
}

/// Wait for this machine's own account to allow or refuse the task
/// (`POST /api/a2a/tasks/ID/consent`). `Some(true)` is "allow for an hour".
///
/// Not a card on the task's block: the mux refuses asks on an agent block
/// (it asks through its own methods), and a card's answer is recorded by
/// role, where every team owner is `owner`, whichever account they are.
async fn consent(t: &str) -> Result<Option<bool>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    PENDING.lock().unwrap().insert(t.to_owned(), tx);
    rx.await.map_err(|_| "nobody answered".to_owned())
}

static PENDING: LazyLock<Mutex<HashMap<String, tokio::sync::oneshot::Sender<Option<bool>>>>> =
    LazyLock::new(Default::default);

#[derive(Deserialize)]
struct ConsentReq {
    /// allow, hour or deny.
    answer: String,
}

/// The owner's answer to a waiting task: only this machine's own account,
/// not a teammate (an owner of its team among them).
async fn consent_set(
    State(app): AppState,
    UrlPath(id): UrlPath<String>,
    dev: Option<axum::Extension<crate::e2e::Caller>>,
    Json(req): Json<ConsentReq>,
) -> Response {
    let me = caller_of(&app, &dev, Value::Null);
    if me.id != "owner" {
        warn!(task = id, by = me.name, "refused a consent answer from another account");
        return refuse(StatusCode::FORBIDDEN, "only this machine's own account allows tasks for its agents");
    }
    let Some(tx) = PENDING.lock().unwrap().remove(&id) else {
        return refuse(StatusCode::NOT_FOUND, "no task waits for that");
    };
    let answer = match req.answer.as_str() {
        "allow" => Some(false),
        "hour" => Some(true),
        _ => None,
    };
    let _ = tx.send(answer);
    Json(json!({ "task": id, "answer": req.answer })).into_response()
}

/// Tasks waiting for this machine's owner (the owner's).
async fn waiting(State(app): AppState, dev: Option<axum::Extension<crate::e2e::Caller>>) -> Response {
    if caller_of(&app, &dev, Value::Null).id != "owner" {
        return refuse(StatusCode::FORBIDDEN, "the owner's");
    }
    let ids: Vec<String> = PENDING.lock().unwrap().keys().cloned().collect();
    let v: Vec<Value> = ids.iter().filter_map(|i| snapshot(i)).map(|t| t.json(None)).collect();
    Json(json!({ "waiting": v })).into_response()
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_owned() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

/// One turn of the task's agent, followed to where it ends.
///
/// - A permission the agent asks for is about this machine, so it's this
///   machine's owner's to answer, on the block as any other: the task
///   stays working, saying what it waits for.
/// - A question it asks is the caller's: input-required, answered by the
///   next message on the task.
async fn turn(app: Arc<App>, t: String, text: String, answering: bool) {
    use illogical_proto::api::PromptResult;
    let Some(task) = snapshot(&t) else { return };
    let Some(block) = task.block else { return fail(&t, "no block".into()) };
    let by = Some(format!("{} (A2A)", task.caller.name));
    match crate::api::prompt(&app, block, text, answering, Duration::from_secs(120), by).await {
        Ok(PromptResult::Stalled { why, .. }) => return fail(&t, why),
        Err(e) => return fail(&t, e),
        Ok(_) => {}
    }
    loop {
        if snapshot(&t).is_none_or(|x| TERMINAL.contains(&x.state)) {
            return; // canceled meanwhile
        }
        let Some(b) = app.mux.api(|r| Api::Block(block, r)).await.flatten() else {
            return fail(&t, "its block closed".into());
        };
        let s = b.state();
        let pending = s["pending"].as_array().map_or(!s["pending"].is_null(), |p| !p.is_empty());
        let question = s["asks"].as_array().into_iter().flatten().find(|a| a["accepted"] != true).cloned();
        match s["attention"].as_str().unwrap_or_default() {
            "working" => update(&t, |x| {
                x.state = "TASK_STATE_WORKING";
                x.status = None;
            }),
            _ if pending => {
                let what = s["pending"]
                    .pointer("/0/title")
                    .or(s["pending"].pointer("/title"))
                    .and_then(Value::as_str)
                    .unwrap_or("a permission")
                    .to_owned();
                update(&t, |x| {
                    x.state = "TASK_STATE_WORKING";
                    x.status = Some(format!("waiting for this machine's owner to approve: {what}"));
                })
            }
            _ if question.is_some() => {
                let q = question.unwrap();
                let msg = q["message"].as_str().filter(|m| !m.is_empty()).unwrap_or("it asks a question").to_owned();
                update(&t, |x| {
                    x.state = "TASK_STATE_INPUT_REQUIRED";
                    x.status = Some(msg);
                });
                return;
            }
            _ if s["status"] == "exited" => {
                return fail(&t, s["error"].as_str().unwrap_or("the agent exited").to_owned());
            }
            _ => return finish(&app, &t, block).await,
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// The turn ended: its last reply, and the patch against where it started.
async fn finish(app: &App, t: &str, block: PaneId) {
    let Some(task) = snapshot(t) else { return };
    let reply = match app.mux.api(|r| Api::Block(block, r)).await.flatten() {
        Some(b) => b.state()["entries"]
            .as_array()
            .and_then(|e| {
                e.iter().rev().find(|e| e["type"] == "agent").and_then(|e| e["text"].as_str()).map(str::to_owned)
            })
            .unwrap_or_default(),
        None => String::new(),
    };
    let mut artifacts = vec![json!({
        "artifactId": format!("{t}-reply"),
        "name": "reply",
        "parts": [{ "text": reply, "mediaType": "text/markdown" }],
    })];
    if let (Some(wt), Some(base)) = (&task.worktree, &task.base) {
        // New files count: intent-to-add, so the diff shows them. New
        // binary files don't (a build's or a test run's leavings, like
        // `__pycache__`, which made a whole patch fail to apply): they're
        // named instead.
        let _ = git(wt, &["add", "-A", "-N", "."]).await;
        let tracked: std::collections::HashSet<String> = git(wt, &["ls-tree", "-r", "--name-only", base])
            .await
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect();
        let mut left_out = vec![];
        for l in git(wt, &["diff", "--numstat", base]).await.unwrap_or_default().lines() {
            let f: Vec<&str> = l.splitn(3, '\t').collect();
            if let [a, b, path] = f.as_slice()
                && *a == "-"
                && *b == "-"
                && !tracked.contains(*path)
            {
                left_out.push((*path).to_owned());
            }
        }
        let mut args: Vec<String> = vec!["diff".into(), "--binary".into(), base.clone(), "--".into(), ".".into()];
        args.extend(left_out.iter().map(|p| format!(":(exclude,literal){p}")));
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match git(wt, &args).await {
            Ok(p) if !p.is_empty() => {
                let mut stat_args = args.clone();
                stat_args.insert(1, "--stat");
                let files = git(wt, &stat_args).await.unwrap_or_default();
                artifacts.push(json!({
                    "artifactId": format!("{t}-patch"),
                    "name": "patch",
                    "description": format!("Changes against {base}"),
                    "parts": [{ "text": p, "mediaType": "text/x-diff", "filename": format!("{t}.patch") }],
                    "metadata": { "illogical": { "base": base, "stat": files, "left_out": left_out } },
                }));
            }
            Ok(_) => {}
            Err(e) => warn!(task = t, error = e, "no patch"),
        }
    }
    let msg = agent_message(&short(&reply, 2000), t, &task.context);
    update(t, |x| {
        x.artifacts = artifacts;
        x.history.push(msg);
        x.state = "TASK_STATE_COMPLETED";
        x.status = None;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subagent_file_is_a_recipe() {
        let r = parse(
            "---\nname: fixer\ndescription: Fixes tests.\ntools: Read, Edit, Bash\nmodel: haiku\n---\nYou fix bugs.\n",
            Path::new("x.md"),
        )
        .unwrap();
        assert_eq!(r.name, "fixer");
        assert_eq!(r.tools, ["Read", "Edit", "Bash"]);
        assert_eq!(r.model.as_deref(), Some("haiku"));
        assert_eq!(r.prompt, "You fix bugs.");
        let r = parse("---\nname: a\ntools: [Read, Grep]\n---\n", Path::new("x.md")).unwrap();
        assert_eq!(r.tools, ["Read", "Grep"]);
        assert!(parse("no frontmatter", Path::new("x.md")).is_err());
        assert!(parse("---\nname: plug:in\n---\n", Path::new("x.md")).is_err());
    }

    #[test]
    fn its_card_names_the_channel() {
        let r = parse("---\nname: fixer\ndescription: Fixes tests.\n---\nYou fix bugs.\n", Path::new("x.md")).unwrap();
        let c = card("ale-box", &r);
        assert_eq!(c["supportedInterfaces"][0]["url"], "illogical://ale-box/api/a2a/agents/fixer");
        assert_eq!(c["supportedInterfaces"][0]["protocolVersion"], VERSION);
        assert_eq!(c["capabilities"]["extensions"][0]["required"], true);
        assert_eq!(c["skills"][0]["id"], "fixer");
    }

    #[test]
    fn times_are_rfc3339() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_791_269_938_206), "2026-10-06T06:58:58.206Z");
    }
}
