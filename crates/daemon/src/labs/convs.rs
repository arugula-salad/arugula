//! S36 (spike, #852): conversations that link panes on several machines,
//! driven by one lead agent. Throwaway: it proves the shape, it isn't the
//! design the milestones build (see `spikes/s36-conversations/README.md`).
//!
//! - **Home machine** (this one, for its conversations): the conversation
//!   is a thread (`ThreadTarget::Conv`) with members, links and a lead
//!   block, kept in `<state>/threads/conv-<id>.json` beside its timeline.
//!   `/api/convs/…` make and change them (the owner's). A watcher per link
//!   follows the pane's attention events (here: the mux's; elsewhere:
//!   `/api/events?follow=1` through the relay) and posts what changed.
//!   `@lead` hands the message, and the events since its last turn, to the
//!   lead block. The lead's MCP token reaches the links, named `L1`, `L2`…
//! - **A linked machine**: `/api/conv-grants` (the owner's) lets one other
//!   daemon drive one pane here, for one conversation. `e2e.rs` lets that
//!   daemon's channel through for that pane's prompt, wait, capture,
//!   events and act, as this machine's owner when it's the same account.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use arugula_proto::{Attention, ConvId, ConvLink, Conversation, Driver, EventKind, PaneId, ThreadTarget};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::{
    acl::Principal,
    api::ApiError,
    mcp::tools::{Call, Out, done},
    mux::{Api, ConvReq},
    server::App,
};

type Res<T> = Result<Json<T>, ApiError>;

fn bad(e: impl ToString) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, e.to_string())
}

pub fn routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    r.route("/api/convs", get(list).post(create))
        .route("/api/convs/{id}", get(one))
        .route("/api/convs/{id}/links", post(link))
        .route("/api/convs/{id}/lead", post(lead))
        .route("/api/convs/{id}/members", post(member))
        .route("/api/convs/{id}/act", post(act))
        .route("/api/conv-grants", get(grants).post(grant))
}

async fn conv(app: &App, req: ConvReq) -> Result<Conversation, ApiError> {
    app.mux.api(|r| Api::Conv(req, r)).await.ok_or_else(|| bad("shutting down"))?.map_err(bad)
}

fn id_of(s: &str) -> Result<ConvId, ApiError> {
    s.parse().map_err(bad)
}

async fn list(State(app): State<Arc<App>>) -> Res<Vec<Conversation>> {
    Ok(Json(app.mux.api(Api::Convs).await.unwrap_or_default()))
}

#[derive(Deserialize)]
struct CreateReq {
    title: String,
}

async fn create(State(app): State<Arc<App>>, Json(r): Json<CreateReq>) -> Res<Conversation> {
    Ok(Json(conv(&app, ConvReq::Create { title: r.title, owner: "owner".into() }).await?))
}

async fn one(State(app): State<Arc<App>>, Path(id): Path<String>) -> Res<Conversation> {
    Ok(Json(conv(&app, ConvReq::Get(id_of(&id)?)).await?))
}

#[derive(Deserialize)]
struct LinkReq {
    /// Another machine (control's name); none: this one.
    #[serde(default)]
    machine: Option<String>,
    pane: PaneId,
    #[serde(default)]
    title: String,
}

/// Link a pane. One here must exist; one elsewhere must answer through the
/// relay (the grant there is what lets it).
async fn link(State(app): State<Arc<App>>, Path(id): Path<String>, Json(r): Json<LinkReq>) -> Res<Conversation> {
    let id = id_of(&id)?;
    match &r.machine {
        None => {
            let panes = app.mux.api(Api::Panes).await.unwrap_or_default();
            if !panes.iter().any(|p| p.info.id == r.pane) {
                return Err(bad(format!("no pane %{} here", r.pane)));
            }
        }
        Some(m) => {
            let (status, body) = remote(&app, m, "GET", &format!("/api/panes/{}/wait?until=idle&timeout=0", r.pane), None, 20)
                .await
                .map_err(bad)?;
            if status >= 400 {
                return Err(bad(format!("{m} refused: {status} {}", String::from_utf8_lossy(&body))));
            }
        }
    }
    let c = conv(&app, ConvReq::Link(id, ConvLink { name: String::new(), machine: r.machine, pane: r.pane, title: r.title }))
        .await?;
    if let Some(l) = c.links.last() {
        watch(app.clone(), c.id, l.clone());
    }
    Ok(Json(c))
}

#[derive(Deserialize)]
struct MemberReq {
    who: String,
}

async fn member(State(app): State<Arc<App>>, Path(id): Path<String>, Json(r): Json<MemberReq>) -> Res<Conversation> {
    Ok(Json(conv(&app, ConvReq::Member(id_of(&id)?, r.who)).await?))
}

#[derive(Deserialize)]
struct LeadReq {
    /// Where it works.
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

/// The tools a lead uses without asking: its conversation's, and driving
/// its links. Answering a link's permission prompt isn't one: people do
/// that, in the conversation.
const LEAD_TOOLS: [&str; 6] = [
    "mcp__arugula__prompt_agent",
    "mcp__arugula__wait",
    "mcp__arugula__read_output",
    "mcp__arugula__read_thread",
    "mcp__arugula__post_thread",
    "mcp__arugula__list",
];

/// Start the lead: an agent block on this machine, in a session of its own.
async fn lead(State(app): State<Arc<App>>, Path(id): Path<String>, Json(r): Json<LeadReq>) -> Res<Conversation> {
    let c = conv(&app, ConvReq::Get(id_of(&id)?)).await?;
    if c.lead.is_some() {
        return Err(bad("it has a lead"));
    }
    let mut config = json!({
        "agent": "claude",
        "title": format!("lead · {}", c.title),
        "prompt": format!(
            "You are the lead of the Arugula conversation \"{}\". People write to you there with @lead; \
             each such message comes to you as your next prompt, with what happened in the linked panes since. \
             Reply only with \"ready\" now.",
            c.title
        ),
        "allow": LEAD_TOOLS.iter().map(|t| json!({ "tool": t })).collect::<Vec<_>>(),
        "brief": BRIEF,
    });
    if let Some(cwd) = r.cwd {
        config["cwd"] = json!(cwd);
    }
    if let Some(m) = r.model {
        config["model"] = json!(m);
    }
    let req: arugula_proto::api::OpenRequest = serde_json::from_value(json!({
        "type": "agent",
        "config": config,
        "session": "conversations",
    }))
    .map_err(bad)?;
    let pane = app.mux.api(|r| Api::Open(req, None, r)).await.ok_or_else(|| bad("shutting down"))?.map_err(bad)?;
    Ok(Json(conv(&app, ConvReq::Lead(c.id, pane)).await?))
}

/// What a lead is told once, in its system prompt.
const BRIEF: &str = "You lead an Arugula conversation: people talk in it, and it links panes (each running an agent, \
often Claude Code) on one or more machines, named L1, L2, ... Drive a link with Arugula's tools, giving its name as \
the pane: prompt_agent (prompt it and wait for its turn), wait (until idle or needs_input), read_output (screen: \
true for what it shows). Prompt several links before waiting on any, so they work at once. When a link asks for a \
permission or a question, don't answer it yourself: the people answer it in the conversation; say what it asks and \
wait. Answer the people with post_thread, giving no pane or session: it posts in your conversation. Keep replies short.";

#[derive(Deserialize)]
struct ActReq {
    link: String,
    #[serde(flatten)]
    rest: serde_json::Map<String, Value>,
}

/// Answer a link's card (allow, deny, answer) from the conversation.
async fn act(
    State(app): State<Arc<App>>,
    who: Option<axum::Extension<Principal>>,
    Path(id): Path<String>,
    Json(r): Json<ActReq>,
) -> Res<Value> {
    let c = conv(&app, ConvReq::Get(id_of(&id)?)).await?;
    let l = c.links.iter().find(|l| l.name.eq_ignore_ascii_case(&r.link)).ok_or_else(|| bad("no such link"))?;
    let mut body = Value::Object(r.rest);
    body["pane"] = json!(l.pane);
    let action = body["action"].as_str().unwrap_or("?").to_owned();
    let out = match &l.machine {
        None => {
            let req: arugula_proto::api::ActRequest = serde_json::from_value(body).map_err(bad)?;
            let by = Driver { who: "owner".into(), name: "owner".into() };
            crate::api::act_as(&app, l.pane, &req, by).await.map_err(bad)?;
            json!({ "ok": true })
        }
        Some(m) => {
            let (status, b) = remote(&app, m, "POST", "/api/attention/act", Some(&body), 20).await.map_err(bad)?;
            let v: Value = serde_json::from_slice(&b).unwrap_or(json!({ "raw": String::from_utf8_lossy(&b) }));
            if status >= 400 {
                return Err(ApiError(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY), v.to_string()));
            }
            v
        }
    };
    let name = who.map(|axum::Extension(w)| w.id().to_owned()).unwrap_or_else(|| "owner".into());
    post_event(&app, c.id, l, &format!("{name} answered {} in the conversation: {action}", l.name)).await;
    Ok(Json(out))
}

// ---------------------------------------------------------------- grants

/// A linked machine's side: this daemon (`device`) may drive `pane` here,
/// for conversation `conv`.
#[derive(Clone, Serialize, Deserialize)]
pub struct Grant {
    pub device: String,
    pub pane: PaneId,
    pub conv: String,
    pub at: u64,
}

fn grants_path(app: &App) -> std::path::PathBuf {
    app.control.state_dir().join("conv-grants.json")
}

fn load_grants(app: &App) -> Vec<Grant> {
    std::fs::read(grants_path(app)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

async fn grants(State(app): State<Arc<App>>) -> Res<Vec<Grant>> {
    Ok(Json(load_grants(&app)))
}

#[derive(Deserialize)]
struct GrantReq {
    device: String,
    pane: PaneId,
    conv: String,
}

async fn grant(State(app): State<Arc<App>>, Json(r): Json<GrantReq>) -> Res<Vec<Grant>> {
    let mut g = load_grants(&app);
    g.retain(|x| !(x.device == r.device && x.pane == r.pane));
    g.push(Grant { device: r.device, pane: r.pane, conv: r.conv, at: crate::store::now_ms() });
    crate::store::write_atomic(&grants_path(&app), &serde_json::to_vec_pretty(&g).unwrap()).map_err(bad)?;
    Ok(Json(g))
}

/// What a daemon's channel may ask here beyond `/api/a2a/` (e2e.rs): a
/// linked pane's prompt, wait, capture, events and act, where a grant
/// names that daemon and pane. `Some(pane)` when it may.
pub fn linked_request(app: &App, device: &str, path: &str, body: &[u8]) -> Option<PaneId> {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let s = crate::authz::segments(path)?;
    let s: Vec<&str> = s.iter().map(String::as_str).collect();
    let pane: PaneId = match s.as_slice() {
        ["api", "panes", id, "prompt" | "wait" | "capture"] => id.parse().ok()?,
        ["api", "events"] => {
            query.split('&').find_map(|kv| kv.strip_prefix("pane=")).and_then(|p| p.parse().ok())?
        }
        ["api", "attention", "act"] => {
            let v: Value = serde_json::from_slice(body).ok()?;
            if v["panes"].as_array().is_some_and(|a| !a.is_empty()) {
                return None;
            }
            v["pane"].as_u64()?.try_into().ok()?
        }
        _ => return None,
    };
    load_grants(app).iter().any(|g| g.device == device && g.pane == pane).then_some(pane)
}

// ---------------------------------------------------------------- remote

/// A request to another machine through the relay, as this daemon.
pub async fn remote(
    app: &App,
    machine: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
    secs: u64,
) -> Result<(u16, Vec<u8>), String> {
    let m = crate::peer::find(&app.control, machine).await.map_err(|e| e.to_string())?;
    crate::peer::request_within(&app.control, &m, method, path, body, Duration::from_secs(secs))
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- watchers

static WATCHING: LazyLock<Mutex<HashSet<(ConvId, String)>>> = LazyLock::new(Default::default);
/// What happened in each conversation's links since its lead's last turn.
static PENDING: LazyLock<Mutex<HashMap<ConvId, Vec<String>>>> = LazyLock::new(Default::default);

/// Start watching every link of every conversation (at start-up).
pub fn resume(app: &Arc<App>) {
    let app = app.clone();
    tokio::spawn(async move {
        // The mux answers once it's up.
        let convs = app.mux.api(Api::Convs).await.unwrap_or_default();
        for c in convs {
            for l in &c.links {
                watch(app.clone(), c.id, l.clone());
            }
        }
    });
}

fn place(l: &ConvLink) -> String {
    match &l.machine {
        Some(m) => format!("{} (%{} on {m})", l.name, l.pane),
        None => format!("{} (%{} here)", l.name, l.pane),
    }
}

async fn post_event(app: &App, id: ConvId, l: &ConvLink, text: &str) {
    let post = crate::mux::ThreadPost {
        target: ThreadTarget::Conv(id),
        who: Principal::Owner,
        as_agent: Some(Driver { who: format!("link:{}", l.name), name: place(l) }),
        text: text.to_owned(),
        quote: None,
    };
    match app.mux.api(|r| Api::ThreadPost(post, r)).await {
        Some(Ok(_)) => {}
        Some(Err(e)) => warn!(error = e.1, "can't post a link's event"),
        None => {}
    }
    PENDING.lock().unwrap().entry(id).or_default().push(format!("{}: {text}", l.name));
}

/// Follow one link's attention, posting each change worth a line: its
/// turn ended, it needs input (what for), it exited. The same state twice
/// in a row is one line.
fn watch(app: Arc<App>, id: ConvId, l: ConvLink) {
    if !WATCHING.lock().unwrap().insert((id, l.name.clone())) {
        return;
    }
    tokio::spawn(async move {
        info!(conv = %id, link = l.name, "watching a link");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<EventKind>();
        match l.machine.clone() {
            None => {
                let mut events = app.mux.events();
                let pane = l.pane;
                tokio::spawn(async move {
                    loop {
                        match events.recv().await {
                            Ok(e) if e.pane == Some(pane) => {
                                if tx.send(e.kind).is_err() {
                                    return;
                                }
                            }
                            Ok(_) => {}
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                            Err(_) => return,
                        }
                    }
                });
            }
            Some(machine) => {
                let app = app.clone();
                let pane = l.pane;
                tokio::spawn(async move {
                    let path = format!("/api/events?pane={pane}&type=attention,exit&follow=1");
                    loop {
                        let r = async {
                            let m = crate::peer::find(&app.control, &machine).await?;
                            let mut buf = Vec::new();
                            crate::peer::follow(&app.control, &m, &path, |part| {
                                buf.extend(part);
                                while let Some(n) = buf.iter().position(|b| *b == b'\n') {
                                    let line: Vec<u8> = buf.drain(..=n).collect();
                                    if let Ok(e) = serde_json::from_slice::<arugula_proto::Event>(&line)
                                        && tx.send(e.kind).is_err()
                                    {
                                        return false;
                                    }
                                }
                                true
                            })
                            .await
                        }
                        .await;
                        if tx.is_closed() {
                            return;
                        }
                        warn!(machine, pane, result = ?r.map_err(|e| e.to_string()), "a link's event stream ended; again in 3s");
                        tokio::time::sleep(Duration::from_secs(3)).await;
                    }
                });
            }
        }
        // Only what a person (or the lead) would act on. The first run
        // posted a "turn ended" when a link was prompted (the prompt sets
        // it idle), and Claude Code's "waiting for your input" a minute
        // after every turn: a turn ends only after it worked, and only a
        // card (an ask) needs input.
        let mut worked = false;
        let mut last: Option<String> = None;
        while let Some(kind) = rx.recv().await {
            let line = match kind {
                EventKind::Attention { state: Attention::Done | Attention::Idle, .. } => {
                    if !std::mem::take(&mut worked) {
                        continue;
                    }
                    "turn ended".to_owned()
                }
                EventKind::Attention { state: Attention::NeedsInput, reason } => {
                    let Some(r) = reason.filter(|r| r.kind == arugula_proto::ReasonKind::Ask) else { continue };
                    format!("needs input: {} (answer here: allow or deny {})", r.headline, l.name)
                }
                EventKind::Attention { state: Attention::Working, .. } => {
                    worked = true;
                    continue;
                }
                EventKind::Exit { code, .. } => format!("exited ({})", code.map_or("?".into(), |c| c.to_string())),
                _ => continue,
            };
            if last.as_deref() == Some(line.as_str()) && !line.starts_with("turn ended") {
                continue;
            }
            last = Some(line.clone());
            post_event(&app, id, &l, &line).await;
            if line.starts_with("turn ended") || line.starts_with("exited") {
                wake_lead(&app, id, &l).await;
            }
        }
    });
}

// ---------------------------------------------------------------- the lead

/// Links each lead prompted and hasn't heard back from: their turn ending
/// is the lead's next prompt, if it's idle by then.
static AWAITING: LazyLock<Mutex<HashSet<(ConvId, String)>>> = LazyLock::new(Default::default);

/// A link the lead was waiting on ended its turn: tell the lead, unless
/// it's in a turn (then it's waiting on it itself, and reads it there).
async fn wake_lead(app: &App, id: ConvId, l: &ConvLink) {
    if !AWAITING.lock().unwrap().remove(&(id, l.name.clone())) {
        return;
    }
    let Some(lead) = app.mux.api(|r| Api::Conv(ConvReq::Get(id), r)).await.and_then(Result::ok).and_then(|c| c.lead)
    else {
        return;
    };
    let Some(b) = app.mux.api(|r| Api::Block(lead, r)).await.flatten() else { return };
    if b.state()["attention"] == "working" {
        return;
    }
    let events = PENDING.lock().unwrap().remove(&id).unwrap_or_default();
    let mut text = format!("{} (you prompted it) ended its turn.\n\nSince your last turn, in the links:\n", l.name);
    for e in events.iter().rev().take(20).rev() {
        text.push_str(&format!("- {e}\n"));
    }
    text.push_str("\nRead what it did (read_output, screen: true) and tell the conversation with post_thread, briefly.");
    if let Err(e) = b.call_by("send", json!({ "text": text }), None).await {
        warn!(error = e, "can't wake the lead");
    }
}

/// The conversation `block` leads, if it leads one.
pub async fn led_by(app: &App, block: PaneId) -> Option<Conversation> {
    app.mux.api(Api::Convs).await.unwrap_or_default().into_iter().find(|c| c.lead == Some(block))
}

/// Whether `block` is a lead and `pane` one of its links on this machine.
pub async fn lead_reaches(app: &App, block: PaneId, pane: PaneId) -> bool {
    led_by(app, block).await.is_some_and(|c| c.links.iter().any(|l| l.machine.is_none() && l.pane == pane))
}

/// Hand an `@lead` message to the conversation's lead, with what its links
/// did since its last turn.
pub async fn tell_lead(app: &App, id: ConvId, msg: &arugula_proto::ThreadMsg) -> Result<bool, String> {
    let c = app.mux.api(|r| Api::Conv(ConvReq::Get(id), r)).await.ok_or("shutting down")??;
    let lead = c.lead.ok_or("this conversation has no lead")?;
    let events = PENDING.lock().unwrap().remove(&id).unwrap_or_default();
    let mut text = format!("{} wrote in the conversation: {}\n", msg.name, msg.text);
    if !events.is_empty() {
        text.push_str("\nSince your last turn, in the links:\n");
        for e in events.iter().rev().take(20).rev() {
            text.push_str(&format!("- {e}\n"));
        }
    }
    // Names only: a pane number here could be another machine's, or the
    // lead's own (the first run's lead drove "%3", its own block).
    text.push_str("\nLinks (give the name as the pane, never a number):\n");
    for l in &c.links {
        let title = if l.title.is_empty() { String::new() } else { format!(": {}", l.title) };
        let at = l.machine.as_deref().map_or("this machine".to_owned(), |m| format!("machine {m}"));
        text.push_str(&format!("- {}{title}, on {at}\n", l.name));
    }
    text.push_str(
        "\n(Prompt the links that should work before waiting on any. Answer with post_thread, no pane or session. \
         read_thread has the whole conversation.)",
    );
    let b = app.mux.api(|r| Api::Block(lead, r)).await.flatten().ok_or("the lead's block is gone")?;
    let name = (msg.who != "owner").then_some(msg.name.as_str());
    b.call_by("send", json!({ "text": text }), name).await.map(|_| true)
}

// ---------------------------------------------------------------- MCP

/// A lead's tool call naming a link (`L2`) as its pane: one on this
/// machine becomes its pane number; one elsewhere is done there, through
/// the relay, and answered here (`Err(out)`).
pub async fn route_link(call: &Call<'_>, tool: &str, mut args: Value) -> Result<Value, Out> {
    if !matches!(tool, "prompt_agent" | "wait" | "read_output" | "agent_respond") {
        return Ok(args);
    }
    let Some(name) = args["pane"].as_str().map(str::trim).filter(|s| {
        s.len() > 1 && s[..1].eq_ignore_ascii_case("l") && s[1..].bytes().all(|b| b.is_ascii_digit())
    }) else {
        return Ok(args);
    };
    let name = name.to_uppercase();
    let Some(me) = call.me() else { return Err(Err(format!("{name}: only a conversation's lead names links"))) };
    let Some(c) = led_by(call.app(), me).await else {
        return Err(Err(format!("{name}: this agent leads no conversation")));
    };
    let Some(l) = c.links.iter().find(|l| l.name == name).cloned() else {
        return Err(Err(format!("no link {name}: the links are {}", c.links.iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join(", "))));
    };
    if tool == "prompt_agent" {
        AWAITING.lock().unwrap().insert((c.id, l.name.clone()));
    }
    let Some(machine) = &l.machine else {
        args["pane"] = json!(l.pane);
        return Ok(args);
    };
    Err(remote_tool(call.app(), tool, machine, &l, &args).await)
}

async fn remote_tool(app: &App, tool: &str, machine: &str, l: &ConvLink, a: &Value) -> Out {
    let pane = l.pane;
    let timeout = a["timeout"].as_f64().unwrap_or(90.0).clamp(1.0, 90.0);
    let json_of = |status: u16, b: Vec<u8>| -> Result<Value, String> {
        let v: Value = serde_json::from_slice(&b).unwrap_or(json!({ "text": String::from_utf8_lossy(&b) }));
        if status >= 400 { Err(format!("{machine} said {status}: {v}")) } else { Ok(v) }
    };
    match tool {
        "prompt_agent" => {
            let body = json!({ "text": a["text"], "answering": a["answering"].as_bool().unwrap_or(false), "timeout": timeout });
            let (s, b) = remote(app, machine, "POST", &format!("/api/panes/{pane}/prompt"), Some(&body), timeout as u64 + 10).await?;
            let r = json_of(s, b)?;
            done(format!("{} on {machine}: {}", l.name, r), json!({ "result": r, "link": l.name }))
        }
        "wait" => {
            let until = match a["until"].as_str().unwrap_or("idle") {
                "needs_input" | "needs-input" => "needs-input",
                "idle" => "idle",
                other => return Err(format!("{}: a link on another machine waits until idle or needs_input, not {other}", l.name)),
            };
            let path = format!("/api/panes/{pane}/wait?until={until}&timeout={timeout}");
            let (s, b) = remote(app, machine, "GET", &path, None, timeout as u64 + 10).await?;
            let r = json_of(s, b)?;
            done(format!("{} on {machine}: {r}", l.name), json!({ "result": r, "link": l.name }))
        }
        "read_output" => {
            let (s, b) = remote(app, machine, "GET", &format!("/api/panes/{pane}/capture"), None, 20).await?;
            if s >= 400 {
                return Err(format!("{machine} said {s}: {}", String::from_utf8_lossy(&b)));
            }
            let text = String::from_utf8_lossy(&b).into_owned();
            let tail: Vec<&str> = text.lines().rev().take(60).collect::<Vec<_>>().into_iter().rev().collect();
            done(format!("{}'s screen on {machine}:\n{}", l.name, tail.join("\n")), json!({ "link": l.name }))
        }
        _ => Err(format!("{tool} on a link on another machine: people answer its prompts in the conversation")),
    }
}
