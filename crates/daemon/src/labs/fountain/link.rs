//! A Managoat conversation as an agent block's link (#942, F3): the third
//! kind beside a local agent server and one in a VM (`agent/link.rs`). No
//! `fountain acp` runs: the daemon speaks Managoat's REST API and follows
//! the conversation's events itself, and the block keeps speaking ACP.
//!
//! **Outbound** JSON-RPC from the block becomes REST: `initialize` is
//! answered here; `session/new` creates the conversation (`agent_id`, and
//! the block's vault, environment, labels, permission policy and model, by
//! name or id); `session/resume` reopens it with no replay; `session/load`
//! replays what it has stored, then follows; `session/prompt` is `POST
//! /prompts` with its `_meta.clientRequestId`; `session/cancel` is `POST
//! /interrupt`; the block's answer to a permission request is `POST
//! /requests/:id`. The session id is the conversation id, as with `fountain
//! acp`.
//!
//! **Inbound** frames are made from the conversation's events: its `acp`
//! output is the sandbox's own ACP (Managoat is "a proxy, not a
//! translator"), passed on with our session id, and its `stage` events end
//! the turn our prompt opened with its stop reason, or with an error for
//! what ACP has no word for (a sandbox that never started or was reclaimed,
//! a terminated conversation, a refused admission, a failed wake). They
//! also make `_fountain/status` notifications (pending, running, idle and
//! whether the machine is awake or parked) and `$/cancel_request` when a
//! permission request was answered anywhere, so its card goes. This is the
//! Go bridge's mapping (`cli/internal/acp`), ported with its tests.
//!
//! **Where the events come from**: the account stream's fan-out
//! ([`super::stream::Hub`]) when this daemon runs it, after reading what
//! came since the cursor from the conversation's own stream (`wait=false`);
//! else the conversation's stream (`GET /api/conversations/:id/stream`),
//! reconnecting with `Last-Event-ID` on Managoat's idle closes.
//!
//! **The cursor is in the block's log.** Every frame made from an event
//! carries its id (`_arugula.fountain_event`, on the last frame an event
//! makes), and the log keeps every inbound frame, so a restarted daemon
//! knows the last event the block took from its log alone: it follows on
//! from there, with nothing replayed and nothing twice. A block whose log
//! has no cursor (one saved by a daemon that ran `fountain acp`) is offered
//! no `session/resume`, so it reopens with `session/load`, once, and has a
//! cursor from then on.
//!
//! **Turns.** The prompt's `client_request_id` finds the turn it opened
//! (`turn` `started` carries it); that turn's end answers the prompt. A turn
//! nobody here started (another client's, or one Managoat opened itself)
//! shows as `running` and its output goes into the transcript.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use serde_json::{Map, Value, json};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, warn};

use super::{
    account,
    client::{Client, Conversation, Error, NewConversation},
    stream::Parser,
};
use crate::labs::{FountainLink, LinkOut};

/// Where a tool call's sandbox paths go instead of `locations` (as `fountain
/// acp` puts them): they name files in the sandbox, not on this machine.
pub const SANDBOX_LOCATIONS: &str = "fountain.sandboxLocations";

/// The key each frame made from an event carries its id under.
pub const EVENT_KEY: &str = "_arugula";

/// The status notification's method.
pub const STATUS: &str = crate::agent::FOUNTAIN_STATUS;

/// The id of the event a frame was made from, if it was.
pub fn event_of(frame: &Value) -> Option<u64> {
    frame[EVENT_KEY]["fountain_event"].as_u64()
}

/// A JSON-RPC id as a string, as Managoat names a request (`"7"` for 7).
fn key_of(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        v => v.to_string(),
    }
}

// ------------------------------------------------------------------- events

/// One event of the conversation's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ev {
    /// Managoat's id (0 for one it didn't number: the `server` stage).
    pub id: u64,
    /// `output` or `stage`.
    pub kind: String,
    /// output: `acp`, `stdout`, `stderr`.
    pub stream: String,
    /// output: the raw line; stage: JSON-encoded metadata.
    pub data: String,
    pub stage: String,
    pub state: String,
    pub turn_id: Option<String>,
}

impl Ev {
    /// From an SSE payload (`{kind, stream, data, stage, state, turn_id}`).
    pub fn from_payload(id: u64, kind: &str, p: &Value) -> Self {
        let s = |k: &str| p[k].as_str().unwrap_or_default().to_owned();
        Ev {
            id,
            kind: p["kind"].as_str().filter(|k| !k.is_empty()).unwrap_or(kind).to_owned(),
            stream: s("stream"),
            data: match &p["data"] {
                Value::String(d) => d.clone(),
                Value::Null => String::new(),
                v => v.to_string(),
            },
            stage: s("stage"),
            state: s("state"),
            turn_id: p["turn_id"].as_str().map(str::to_owned),
        }
    }

    fn meta(&self) -> Value {
        serde_json::from_str::<Value>(&self.data).ok().filter(Value::is_object).unwrap_or_else(|| json!({}))
    }
}

/// A stage's reason, as the Go bridge says it.
fn reason_text(meta: &Value) -> String {
    for k in ["reason", "error", "exit_code"] {
        match &meta[k] {
            Value::Null => {}
            Value::String(s) => return s.clone(),
            v => return v.to_string(),
        }
    }
    "no reason given".into()
}

// ------------------------------------------------------------------- the core

/// What the core asks the driver to do over REST.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    /// `session/new`: create the conversation, then answer `rpc`.
    Create { rpc: Value },
    /// `session/resume`: check it's there, then answer and follow.
    Resume { rpc: Value, conversation: String },
    /// `session/load`: replay what's stored, then answer and follow.
    Load { rpc: Value, conversation: String },
    /// `session/prompt`.
    Prompt { rpc: Value, text: String, images: Vec<(String, String)>, crid: Option<String> },
    /// `session/cancel`.
    Interrupt,
    /// The block's answer to a permission request.
    Answer { request: String, option: String },
}

/// The machine a conversation runs on, as far as the block need know.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Machine {
    /// None yet, or none any more.
    #[default]
    None,
    /// Being made (`provision`).
    Starting,
    Awake,
    /// Being woken (`reattach`).
    Waking,
    /// Suspended, or reclaimed: the next prompt (or Wake) brings one.
    Parked,
}

impl Machine {
    fn name(self) -> &'static str {
        match self {
            Machine::None => "none",
            Machine::Starting => "starting",
            Machine::Awake => "awake",
            Machine::Waking => "waking",
            Machine::Parked => "parked",
        }
    }

    /// From a sandbox's `status` on Managoat.
    fn of(sandbox: &Value) -> Self {
        match sandbox["status"].as_str() {
            Some("ready" | "running" | "active") => Machine::Awake,
            Some("suspended" | "paused" | "stopped" | "parked") => Machine::Parked,
            Some("pending" | "provisioning" | "starting" | "creating") => Machine::Starting,
            Some(_) | None => Machine::None,
        }
    }
}

/// Our prompt, until its turn ends.
#[derive(Debug, Clone, PartialEq)]
struct Prompt {
    rpc: Value,
    crid: Option<String>,
    /// The turn it opened, once `turn` `started` said so.
    turn: Option<String>,
}

/// The mapping from ACP to Managoat and back, without the I/O: the driver
/// does the REST calls it asks for and hands it the answers and the
/// events. Each method returns the frames to hand the block, in order.
pub struct Core {
    block: arugula_proto::PaneId,
    /// Offer `session/resume`: the block has a cursor (or no session yet).
    resume: bool,
    conversation: Option<String>,
    /// The last event handed on.
    pub cursor: u64,
    prompt: Option<Prompt>,
    /// Open permission requests' options, by request id.
    perms: HashMap<String, Vec<Value>>,
    /// Turns seen starting, and whether each was another client's.
    started: HashMap<String, bool>,
    status: String,
    machine: Machine,
    sandbox_id: Option<String>,
    turn: Option<String>,
    /// The status last told the block.
    shown: Option<Value>,
    /// A replay's prompts, by turn (its `user_message_chunk`s: Managoat's
    /// `acp` stream has the agent's side only).
    pub prompts: HashMap<String, String>,
}

/// A frame's answer to `rpc`.
fn result(rpc: &Value, r: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": rpc, "result": r })
}

fn error(rpc: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": rpc, "error": { "code": code, "message": message.into() } })
}

const INTERNAL: i64 = -32603;
const INVALID_PARAMS: i64 = -32602;
const NOT_FOUND: i64 = -32601;

/// Takes `locations` off a tool call into its `_meta` (only when present).
fn move_locations(call: &mut Value) {
    let Some(o) = call.as_object_mut() else { return };
    match o.get("locations") {
        None | Some(Value::Null) => return,
        Some(_) => {}
    }
    let locs = o.remove("locations").unwrap_or_default();
    if !o.get("_meta").is_some_and(Value::is_object) {
        o.insert("_meta".into(), json!({}));
    }
    o["_meta"][SANDBOX_LOCATIONS] = locs;
}

impl Core {
    pub fn new(block: arugula_proto::PaneId, resume: bool) -> Self {
        Self {
            block,
            resume,
            conversation: None,
            cursor: 0,
            prompt: None,
            perms: HashMap::new(),
            started: HashMap::new(),
            status: String::new(),
            machine: Machine::None,
            sandbox_id: None,
            turn: None,
            shown: None,
            prompts: HashMap::new(),
        }
    }

    /// Carry on where a block left off (a restarted daemon): its
    /// conversation, its cursor, and the prompt still waiting for its turn.
    pub fn adopt(
        &mut self,
        conversation: String,
        cursor: u64,
        prompt: Option<(Value, Option<String>)>,
        turn: Option<String>,
    ) {
        self.conversation = Some(conversation);
        self.cursor = cursor;
        self.prompt = prompt.map(|(rpc, crid)| Prompt { rpc, crid, turn: turn.clone() });
        self.turn = turn;
    }

    /// The prompt still waiting, and the turn it's bound to.
    pub fn waiting(&self) -> Option<(&Value, Option<&str>, Option<&str>)> {
        self.prompt.as_ref().map(|p| (&p.rpc, p.crid.as_deref(), p.turn.as_deref()))
    }

    // ---------------------------------------------------------- outbound

    /// A frame from the block: the frames to answer it with now, and the
    /// calls to make.
    pub fn outbound(&mut self, m: &Value) -> (Vec<Value>, Vec<Call>) {
        let (mut frames, mut calls) = (vec![], vec![]);
        let method = m["method"].as_str();
        let id = m.get("id").filter(|i| !i.is_null());
        match (id, method) {
            (Some(rpc), Some("initialize")) => {
                let mut session = json!({});
                if self.resume {
                    session["resume"] = json!({});
                }
                frames.push(result(
                    rpc,
                    json!({
                        "protocolVersion": 1,
                        "agentCapabilities": {
                            "loadSession": true,
                            "promptCapabilities": { "image": true, "audio": false, "embeddedContext": false },
                            "sessionCapabilities": session,
                        },
                        "agentInfo": { "name": "managoat", "title": "Managoat", "version": env!("CARGO_PKG_VERSION") },
                        "authMethods": [],
                    }),
                ));
            }
            (Some(rpc), Some("session/new")) => calls.push(Call::Create { rpc: rpc.clone() }),
            (Some(rpc), Some(how @ ("session/resume" | "session/load"))) => {
                match m["params"]["sessionId"].as_str().filter(|s| !s.is_empty()) {
                    None => frames.push(error(rpc, INVALID_PARAMS, format!("{how}: no sessionId"))),
                    Some(c) => {
                        let (rpc, conversation) = (rpc.clone(), c.to_owned());
                        calls.push(if how == "session/load" {
                            Call::Load { rpc, conversation }
                        } else {
                            Call::Resume { rpc, conversation }
                        });
                    }
                }
            }
            (Some(rpc), Some("session/prompt")) => match self.prompt_call(rpc, &m["params"]) {
                Ok(c) => calls.push(c),
                Err(f) => frames.push(f),
            },
            // Accepted, not applied: a conversation keeps its agent's
            // options from its launch (as `fountain acp` answers).
            (Some(rpc), Some("session/set_config_option")) => {
                frames.push(result(rpc, json!({ "_meta": { "fountain": { "applied": false } } })))
            }
            (Some(rpc), Some(method)) => {
                frames.push(error(rpc, NOT_FOUND, format!("{method} isn't something Managoat does")))
            }
            (None, Some("session/cancel")) => {
                // Answers nothing: the prompt answers `cancelled` when the
                // turn's interrupted stage comes (or at once, below, when
                // there was no turn to interrupt).
                calls.push(Call::Interrupt);
            }
            (None, Some(_)) => {}
            // Our answer to a request of the agent's: a permission request.
            (Some(rpc), None) => {
                if let Some(c) = self.answer_call(rpc, m) {
                    calls.push(c);
                }
            }
            (None, None) => {}
        }
        (frames, calls)
    }

    fn prompt_call(&mut self, rpc: &Value, p: &Value) -> Result<Call, Value> {
        if self.conversation.is_none() {
            return Err(error(rpc, INVALID_PARAMS, "there's no conversation yet"));
        }
        let crid = match &p["_meta"]["clientRequestId"] {
            Value::Null => None,
            Value::String(s) => Some(s.clone()),
            _ => return Err(error(rpc, INVALID_PARAMS, "session/prompt: _meta.clientRequestId must be a string")),
        };
        let (mut texts, mut images, mut skipped) = (vec![], vec![], vec![]);
        for b in p["prompt"].as_array().into_iter().flatten() {
            match b["type"].as_str().unwrap_or_default() {
                "text" => {
                    if let Some(t) = b["text"].as_str().filter(|t| !t.is_empty()) {
                        texts.push(t.to_owned());
                    }
                }
                "image" => match (b["data"].as_str().filter(|d| !d.is_empty()), b["mimeType"].as_str()) {
                    (Some(d), m) => images.push((d.to_owned(), m.unwrap_or("image/png").to_owned())),
                    (None, _) => skipped.push("image without data".to_owned()),
                },
                other => skipped.push(other.to_owned()),
            }
        }
        if !skipped.is_empty() {
            // A resource on this machine names nothing in the sandbox.
            warn!(block = self.block, skipped = skipped.join(","), "dropped prompt content a Managoat agent can't use");
        }
        let text = texts.join("\n");
        if text.trim().is_empty() && images.is_empty() {
            return Err(error(rpc, INVALID_PARAMS, "session/prompt carried no text or image content"));
        }
        self.prompt = Some(Prompt { rpc: rpc.clone(), crid: crid.clone(), turn: None });
        Ok(Call::Prompt { rpc: rpc.clone(), text, images, crid })
    }

    /// The block answered a permission request: the option it chose goes
    /// to Managoat; a dismissal is a refusal the agent offered (none
    /// offered: nothing is sent, and Managoat's timeout denies it); an
    /// option it never offered is never relayed.
    fn answer_call(&mut self, rpc: &Value, m: &Value) -> Option<Call> {
        let request = key_of(rpc);
        let options = self.perms.remove(&request);
        let outcome = &m["result"]["outcome"];
        let chosen = outcome["optionId"].as_str().filter(|o| !o.is_empty() && outcome["outcome"] == "selected");
        let offered = |o: &str| options.as_ref().is_none_or(|l| l.iter().any(|x| x["optionId"] == o));
        let reject = || {
            options.as_ref().and_then(|l| {
                l.iter()
                    .find(|x| x["kind"].as_str().is_some_and(|k| k.starts_with("reject_")))
                    .and_then(|x| x["optionId"].as_str())
                    .map(str::to_owned)
            })
        };
        let option = match chosen {
            Some(o) if offered(o) => o.to_owned(),
            Some(o) => {
                warn!(block = self.block, option = o, "an option the agent never offered: denying instead");
                reject()?
            }
            None => reject()?,
        };
        Some(Call::Answer { request, option })
    }

    // ------------------------------------------------ the calls' answers

    /// The conversation was created (or reopened): answer `rpc`.
    pub fn opened(&mut self, rpc: &Value, c: &Conversation, fresh: bool) -> Vec<Value> {
        self.conversation = Some(c.id.clone());
        let mut out = vec![if fresh { result(rpc, json!({ "sessionId": c.id })) } else { result(rpc, json!({})) }];
        out.extend(self.snapshot(c));
        out
    }

    /// The conversation as Managoat has it now, for the status line.
    pub fn snapshot(&mut self, c: &Conversation) -> Vec<Value> {
        if !c.status.is_empty() {
            self.status = c.status.clone();
        }
        let sandbox = c.extra.get("sandbox").cloned().unwrap_or_default();
        self.sandbox_id = c.sandbox_id.clone().or_else(|| sandbox["id"].as_str().map(str::to_owned));
        self.machine = Machine::of(&sandbox);
        if self.status != "running" {
            self.turn = None;
        }
        self.status_frame().into_iter().collect()
    }

    pub fn failed(&mut self, rpc: &Value, why: String) -> Vec<Value> {
        vec![error(rpc, INTERNAL, why)]
    }

    /// The prompt couldn't be sent.
    pub fn prompt_failed(&mut self, rpc: &Value, e: &Error) -> Vec<Value> {
        if self.prompt.as_ref().is_some_and(|p| p.rpc == *rpc) {
            self.prompt = None;
        }
        let retry = e.transient() || matches!(e.code(), "conversation_busy" | "fleet_full" | "sandbox_unavailable");
        let why = if retry {
            format!("couldn't send the prompt ({e}): retry shortly")
        } else {
            format!("couldn't send the prompt: {e}")
        };
        vec![error(rpc, INTERNAL, why)]
    }

    /// The interrupt's answer. A 409 (no turn running) is a cancel that
    /// raced the turn's end, not a failure: a prompt that never got a turn
    /// is answered `cancelled` now.
    pub fn interrupted(&mut self, r: Result<(), Error>) -> Vec<Value> {
        match r {
            Ok(()) => vec![],
            Err(e) if e.status() == Some(409) => {
                info!(block = self.block, "the cancel came after the turn ended");
                match self.prompt.take_if(|p| p.turn.is_none()) {
                    Some(p) => vec![result(&p.rpc, json!({ "stopReason": "cancelled" }))],
                    None => vec![],
                }
            }
            Err(e) => {
                // Nobody to tell: the turn goes on, and the prompt answers
                // with whatever it does.
                warn!(block = self.block, error = %e, "interrupt failed");
                vec![]
            }
        }
    }

    // ------------------------------------------------------------ events

    /// One event: the frames it makes, the last carrying its id. One at or
    /// below the cursor was handed on already.
    pub fn event(&mut self, ev: &Ev) -> Vec<Value> {
        if ev.id > 0 && ev.id <= self.cursor {
            return vec![];
        }
        let mut out = match (ev.kind.as_str(), ev.stream.as_str()) {
            ("output", "acp") => self.acp(&ev.data).into_iter().collect(),
            // stdout and stderr on an ACP conversation are the adapter's own
            // diagnostics, not the agent talking.
            ("output", _) => vec![],
            ("stage", _) => self.stage(ev),
            _ => vec![],
        };
        if ev.id > 0 {
            self.cursor = ev.id;
            if let Some(last) = out.last_mut() {
                last[EVENT_KEY] = json!({ "fountain_event": ev.id });
            }
        }
        out.retain(|f| !f.is_null());
        out
    }

    /// Replayed history (`session/load`): only the updates, never a
    /// request long since answered or a stage; each turn's start is its
    /// prompt (from [`Core::prompts`]), so turns read apart.
    pub fn replayed(&mut self, ev: &Ev) -> Option<Value> {
        if ev.id > 0 && ev.id <= self.cursor {
            return None;
        }
        let f = if ev.kind == "stage" && ev.stage == "turn" && ev.state == "started" {
            let tid = ev.meta()["turn_id"].as_str().map(str::to_owned).or_else(|| ev.turn_id.clone());
            tid.and_then(|t| self.prompts.get(&t).cloned()).map(|text| {
                json!({ "jsonrpc": "2.0", "method": "session/update", "params": {
                    "sessionId": self.conversation,
                    "update": { "sessionUpdate": "user_message_chunk", "content": { "type": "text", "text": text } } } })
            })
        } else {
            (ev.kind == "output" && ev.stream == "acp")
                .then(|| self.acp(&ev.data))
                .flatten()
                .filter(|f| f["method"] == "session/update")
        };
        if ev.id > 0 {
            self.cursor = ev.id;
        }
        f.map(|mut f| {
            if ev.id > 0 {
                f[EVENT_KEY] = json!({ "fountain_event": ev.id });
            }
            f
        })
    }

    /// One stored ACP line: a `session/update` with our session id, or a
    /// permission request to ask the block. Anything else the sandbox's
    /// adapter said (its answers to Managoat, say) isn't ours to relay.
    fn acp(&mut self, line: &str) -> Option<Value> {
        let mut m: Value = match serde_json::from_str(line.trim()) {
            Ok(m) => m,
            Err(e) => {
                warn!(block = self.block, error = %e, "an acp line that isn't JSON");
                return None;
            }
        };
        let conv = self.conversation.clone().unwrap_or_default();
        match m["method"].as_str() {
            Some("session/update") => {
                if !m["params"].is_object() {
                    m["params"] = json!({});
                }
                m["params"]["sessionId"] = json!(conv);
                move_locations(&mut m["params"]["update"]);
                Some(json!({ "jsonrpc": "2.0", "method": "session/update", "params": m["params"] }))
            }
            Some("session/request_permission") if m.get("id").is_some_and(|i| !i.is_null()) => {
                if !m["params"].is_object() {
                    m["params"] = json!({});
                }
                m["params"]["sessionId"] = json!(conv);
                move_locations(&mut m["params"]["toolCall"]);
                let options = m["params"]["options"].as_array().cloned().unwrap_or_default();
                self.perms.insert(key_of(&m["id"]), options);
                Some(
                    json!({ "jsonrpc": "2.0", "id": m["id"], "method": "session/request_permission", "params": m["params"] }),
                )
            }
            other => {
                debug!(block = self.block, method = other, "not relaying a message that isn't an update");
                None
            }
        }
    }

    /// End the prompt, if one is waiting (and `bound`: only the turn it
    /// opened, or any).
    fn end(&mut self, ours: impl Fn(&Prompt) -> bool, answer: Result<&str, String>) -> Option<Value> {
        let p = self.prompt.take_if(|p| ours(p))?;
        Some(match answer {
            Ok(stop) => result(&p.rpc, json!({ "stopReason": stop })),
            Err(why) => error(&p.rpc, INTERNAL, why),
        })
    }

    fn stage(&mut self, ev: &Ev) -> Vec<Value> {
        let meta = ev.meta();
        let mut out: Vec<Value> = vec![];
        let tid = meta["turn_id"].as_str().map(str::to_owned).or_else(|| ev.turn_id.clone());
        // A prompt's turn: the one it opened, else (posted, but its start
        // not seen) one nobody else is known to have started.
        let started = self.started.clone();
        let mine = |p: &Prompt| match (&p.turn, &tid) {
            (Some(t), Some(id)) => t == id,
            (None, Some(id)) => started.get(id).is_none_or(|other| !other),
            (None, None) => true,
            (Some(_), None) => false,
        };
        match (ev.stage.as_str(), ev.state.as_str()) {
            ("turn", "started") => {
                let crid = meta["client_request_id"].as_str();
                let autonomous = meta["origin"] == "autonomous";
                let mut other = true;
                if let (Some(p), Some(id), false) = (self.prompt.as_mut(), &tid, autonomous)
                    && p.turn.is_none()
                    && (crid.is_none() || crid == p.crid.as_deref())
                {
                    p.turn = Some(id.clone());
                    other = false;
                }
                if let Some(id) = &tid {
                    if self.started.len() > 64 {
                        self.started.clear();
                    }
                    self.started.insert(id.clone(), other);
                }
                self.status = "running".into();
                self.turn = tid.clone();
                self.machine = Machine::Awake;
            }
            ("turn", end @ ("done" | "failed" | "interrupted")) => {
                let stop = meta["stop_reason"].as_str().filter(|s| !s.is_empty());
                let answer = match (end, stop) {
                    ("done", s) => Ok(s.unwrap_or("end_turn")),
                    ("interrupted", _) => Ok("cancelled"),
                    // A refusal or a cancellation is failed on Managoat but
                    // is an ACP stop reason: "declined", not "broke".
                    (_, Some(s)) => Ok(s),
                    (_, None) => Err(format!("the turn failed: {}", reason_text(&meta))),
                };
                out.extend(self.end(mine, answer));
                if tid.is_none() || self.turn == tid {
                    self.turn = None;
                }
                if self.turn.is_none() && self.status == "running" {
                    self.status = "idle".into();
                }
            }
            ("provision", "started") => self.machine = Machine::Starting,
            ("provision", "done") => self.machine = Machine::Awake,
            ("provision", "failed") => {
                let why = format!("the sandbox never started: {}", reason_text(&meta));
                out.extend(self.end(|_| true, Err(why)));
                self.machine = Machine::None;
            }
            ("reattach", "started") => self.machine = Machine::Waking,
            ("reattach", "done") => self.machine = Machine::Awake,
            ("reattach", "failed") => {
                let why = "couldn't reattach to the sandbox: prompt again to provision a fresh one".to_owned();
                out.extend(self.end(|_| true, Err(why)));
                self.machine = Machine::Parked;
            }
            // A prompt answered `queued` that didn't run: its wake failed,
            // or another prompt opened the turn first. Only one with no turn
            // yet; `retryable` says whether sending it again can work.
            ("wake", "failed") => {
                let reason = reason_text(&meta);
                let why = if meta["retryable"] == true {
                    format!("Managoat couldn't wake the sandbox for the prompt ({reason}): retry shortly")
                } else {
                    format!("Managoat couldn't wake the sandbox for the prompt ({reason})")
                };
                out.extend(self.end(|p| p.turn.is_none(), Err(why)));
                if self.machine == Machine::Waking || self.machine == Machine::Starting {
                    self.machine = Machine::Parked;
                }
            }
            ("terminate", "done") => {
                out.extend(self.end(|_| true, Err("the conversation was terminated".into())));
                self.status = "terminated".into();
                self.machine = Machine::None;
                self.turn = None;
            }
            ("sandbox", "done") => match meta["event"].as_str().unwrap_or_default() {
                // Idle suspend keeps the machine; the next prompt resumes it.
                "suspended" => {
                    out.extend(self.end(|p| p.turn.is_some(), Ok("end_turn")));
                    self.machine = Machine::Parked;
                }
                "admission_refused" | "at_capacity" => {
                    let why = format!("the prompt was not started ({}): retry shortly", reason_text(&meta));
                    out.extend(self.end(|_| true, Err(why)));
                }
                // Between turns: the adapter went; the next prompt starts it.
                "connection_lost" => {
                    let why = format!("the sandbox's agent went away mid-turn: {}", reason_text(&meta));
                    out.extend(self.end(|p| p.turn.is_some(), Err(why)));
                }
                _ => {
                    let why = format!("the sandbox was reclaimed mid-turn: {}", reason_text(&meta));
                    out.extend(self.end(|_| true, Err(why)));
                    self.machine = Machine::Parked;
                    if self.status == "running" {
                        self.status = "idle".into();
                    }
                }
            },
            // Answered somewhere (here, another client, the timeout): its
            // card goes.
            ("request", "done") => {
                let id = meta["request_id"].clone();
                if !id.is_null() {
                    self.perms.remove(&key_of(&id));
                    out.push(json!({ "jsonrpc": "2.0", "method": "$/cancel_request", "params": { "requestId": id } }));
                }
            }
            ("model", "failed") => {
                // The turn goes on with the runtime's default model.
                warn!(block = self.block, requested = %meta["requested"], "Managoat's runtime refused the agent's model");
            }
            _ => {}
        }
        out.extend(self.status_frame());
        out
    }

    /// The status, if it changed since the block was last told.
    fn status_frame(&mut self) -> Option<Value> {
        let p = json!({
            "conversation": self.conversation,
            "status": self.status,
            "machine": self.machine.name(),
            "sandbox_id": self.sandbox_id,
            "turn_id": self.turn,
            "prompt_turn": self.prompt.as_ref().and_then(|p| p.turn.clone()),
        });
        if self.shown.as_ref() == Some(&p) {
            return None;
        }
        self.shown = Some(p.clone());
        Some(json!({ "jsonrpc": "2.0", "method": STATUS, "params": p }))
    }
}

// --------------------------------------------------------- follow-ers here

/// Conversations a block of this daemon follows (F2's attention leaves
/// their permission requests to the block's own card).
fn followed() -> &'static Mutex<HashMap<String, usize>> {
    static F: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
    F.get_or_init(Default::default)
}

/// Whether a block on this daemon follows `conversation`.
#[allow(dead_code)] // F2's attention cards.
pub fn is_followed(conversation: &str) -> bool {
    followed().lock().unwrap().get(conversation).is_some_and(|n| *n > 0)
}

struct Following(String);

impl Following {
    fn new(c: &str) -> Self {
        *followed().lock().unwrap().entry(c.to_owned()).or_default() += 1;
        Self(c.to_owned())
    }
}

impl Drop for Following {
    fn drop(&mut self) {
        let mut f = followed().lock().unwrap();
        if let Some(n) = f.get_mut(&self.0) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                f.remove(&self.0);
            }
        }
    }
}

// ------------------------------------------------------------- the driver

/// What the follower hands the driver.
enum Got {
    Ev(Ev),
    /// The conversation is gone (404, 410): the link closes.
    Gone(String),
}

/// A client with no total timeout, for streams.
fn stream_http() -> reqwest::Client {
    static H: OnceLock<reqwest::Client> = OnceLock::new();
    H.get_or_init(|| super::client::http(None)).clone()
}

/// The login a block's link uses: its CLI profile's, else the daemon's.
pub async fn client_for(profile: Option<&str>) -> Result<Client, String> {
    // Blocks are brought back before the daemon sets its account up.
    let mut account = account::get();
    for _ in 0..100 {
        if account.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        account = account::get();
    }
    let account = account.ok_or("Managoat isn't set up on this daemon")?;
    Ok(account.resolve_profile(profile).await?.client())
}

/// Start a block's link: frames from `rx` go to Managoat, and what comes
/// back goes to `out`, until `stop` (or `rx` closes: the daemon is going,
/// and the conversation goes on without us).
pub fn spawn(
    rt: &tokio::runtime::Handle,
    l: FountainLink,
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    stop: mpsc::UnboundedReceiver<()>,
    out: LinkOut,
) {
    rt.spawn(drive(l, rx, stop, out));
}

async fn drive(
    l: FountainLink,
    mut rx: mpsc::UnboundedReceiver<Vec<u8>>,
    mut stop: mpsc::UnboundedReceiver<()>,
    out: LinkOut,
) {
    let mut core = Core::new(l.block, l.cursor.is_some() || l.adopt.is_some());
    let mut d = Driver { l: l.clone(), out, client: None, follower: None, events: None, following: None };
    if let Some(a) = l.adopt.clone() {
        core.adopt(a.conversation.clone(), l.cursor.unwrap_or(0), a.prompt.clone(), a.turn.clone());
        match d.client().await {
            Ok(c) => {
                // A prompt sent before the restart: its turn, if it opened
                // one we didn't see start.
                if let Some((_, Some(crid), None)) = core.waiting() {
                    let crid = crid.to_owned();
                    if let Ok(turns) = c.turns(&a.conversation).await
                        && let Some(t) = turns.items.iter().find(|t| t.client_request_id.as_deref() == Some(&crid))
                        && let Some(p) = core.prompt.as_mut()
                    {
                        p.turn = Some(t.id.clone());
                    }
                }
                match c.conversation(&a.conversation).await {
                    Ok(conv) => {
                        let f = core.snapshot(&conv);
                        d.emit(f);
                    }
                    Err(e) if matches!(e.status(), Some(404 | 410)) => {
                        return (d.out)(Err(format!("conversation {} is gone from Managoat", a.conversation)));
                    }
                    Err(e) => warn!(block = l.block, error = %e, "couldn't read the conversation"),
                }
                d.follow(&a.conversation, core.cursor);
            }
            Err(e) => return (d.out)(Err(format!("can't reach Managoat: {e}"))),
        }
    }
    loop {
        tokio::select! {
            line = rx.recv() => {
                let Some(line) = line else { break };
                let Ok(m) = serde_json::from_slice::<Value>(&line) else { continue };
                d.outbound(&mut core, &m).await;
            }
            _ = stop.recv() => break,
            got = next(&mut d.events) => match got {
                Some(Got::Ev(ev)) => {
                    let f = core.event(&ev);
                    d.emit(f);
                }
                Some(Got::Gone(why)) => {
                    d.stop_following();
                    return (d.out)(Err(why));
                }
                None => d.events = None,
            },
        }
    }
    d.stop_following();
}

async fn next(rx: &mut Option<mpsc::UnboundedReceiver<Got>>) -> Option<Got> {
    match rx {
        Some(r) => r.recv().await,
        None => std::future::pending().await,
    }
}

struct Driver {
    l: FountainLink,
    out: LinkOut,
    client: Option<Client>,
    follower: Option<tokio::task::AbortHandle>,
    events: Option<mpsc::UnboundedReceiver<Got>>,
    following: Option<Following>,
}

impl Driver {
    fn emit(&self, frames: Vec<Value>) {
        for f in frames {
            (self.out)(Ok(f.to_string().into_bytes()));
        }
    }

    async fn client(&mut self) -> Result<Client, String> {
        if let Some(c) = &self.client {
            return Ok(c.clone());
        }
        let c = client_for(self.l.profile.as_deref()).await?;
        self.client = Some(c.clone());
        Ok(c)
    }

    fn follow(&mut self, conversation: &str, cursor: u64) {
        self.stop_following();
        let Some(c) = self.client.clone() else { return };
        let (tx, rx) = mpsc::unbounded_channel();
        let t = tokio::spawn(follower(c, conversation.to_owned(), cursor, tx));
        self.follower = Some(t.abort_handle());
        self.events = Some(rx);
        self.following = Some(Following::new(conversation));
    }

    fn stop_following(&mut self) {
        if let Some(t) = self.follower.take() {
            t.abort();
        }
        self.events = None;
        self.following = None;
    }

    async fn outbound(&mut self, core: &mut Core, m: &Value) {
        let (frames, calls) = core.outbound(m);
        self.emit(frames);
        for call in calls {
            let f = self.call(core, call).await;
            self.emit(f);
        }
    }

    async fn call(&mut self, core: &mut Core, call: Call) -> Vec<Value> {
        let c = match self.client().await {
            Ok(c) => c,
            Err(e) => {
                return match call {
                    Call::Create { rpc }
                    | Call::Resume { rpc, .. }
                    | Call::Load { rpc, .. }
                    | Call::Prompt { rpc, .. } => core.failed(&rpc, e),
                    Call::Interrupt | Call::Answer { .. } => {
                        warn!(block = self.l.block, error = e, "no Managoat login");
                        vec![]
                    }
                };
            }
        };
        match call {
            Call::Create { rpc } => match self.create(&c).await {
                Ok(conv) => {
                    let f = core.opened(&rpc, &conv, true);
                    self.follow(&conv.id, 0);
                    f
                }
                Err(e) => core.failed(&rpc, format!("couldn't start a conversation on Managoat: {e}")),
            },
            Call::Resume { rpc, conversation } => match c.conversation(&conversation).await {
                Ok(conv) => {
                    let f = core.opened(&rpc, &conv, false);
                    self.follow(&conv.id, core.cursor);
                    f
                }
                Err(e) => core.failed(&rpc, format!("couldn't open conversation {conversation} on Managoat: {e}")),
            },
            Call::Load { rpc, conversation } => {
                let conv = match c.conversation(&conversation).await {
                    Ok(conv) => conv,
                    Err(e) => {
                        return core
                            .failed(&rpc, format!("couldn't open conversation {conversation} on Managoat: {e}"));
                    }
                };
                core.conversation = Some(conv.id.clone());
                core.prompts = match c.turns(&conv.id).await {
                    Ok(t) => t.items.into_iter().map(|t| (t.id, t.prompt)).filter(|(_, p)| !p.is_empty()).collect(),
                    Err(e) => {
                        warn!(block = self.l.block, error = %e, "couldn't read the conversation's prompts");
                        HashMap::new()
                    }
                };
                // The replay goes before the answer, as ACP has it.
                core.cursor = 0;
                let mut replayed = 0;
                let drained = drain(&c, &conv.id, 0, |ev| {
                    if let Some(f) = core.replayed(&ev) {
                        replayed += 1;
                        (self.out)(Ok(f.to_string().into_bytes()));
                    }
                })
                .await;
                if let Err(e) = drained {
                    return core.failed(&rpc, format!("couldn't replay conversation {}: {e}", conv.id));
                }
                info!(block = self.l.block, conversation = conv.id, replayed, "loaded a Managoat conversation");
                let mut f = core.opened(&rpc, &conv, false);
                if let Some(first) = f.first_mut() {
                    first[EVENT_KEY] = json!({ "fountain_event": core.cursor });
                }
                self.follow(&conv.id, core.cursor);
                f
            }
            Call::Prompt { rpc, text, images, crid } => {
                let Some(conv) = core.conversation.clone() else { return core.failed(&rpc, "no conversation".into()) };
                match c.prompt_images(&conv, &text, &images, crid.as_deref()).await {
                    Ok(p) => {
                        debug!(block = self.l.block, status = p.status, "prompt sent");
                        vec![]
                    }
                    Err(e) => core.prompt_failed(&rpc, &e),
                }
            }
            Call::Interrupt => match core.conversation.clone() {
                Some(conv) => core.interrupted(c.interrupt(&conv).await),
                None => vec![],
            },
            Call::Answer { request, option } => {
                let Some(conv) = core.conversation.clone() else { return vec![] };
                match c.answer(&conv, &request, &option).await {
                    Ok(()) => debug!(block = self.l.block, request, option, "answered a permission request"),
                    // Another client, the timeout or the turn's end got
                    // there first: the first answer wins (#940).
                    Err(e) if matches!(e.status(), Some(404 | 409)) => {
                        info!(block = self.l.block, request, "the permission request was already resolved")
                    }
                    Err(e) => {
                        warn!(block = self.l.block, request, error = %e, "couldn't answer the permission request")
                    }
                }
                vec![]
            }
        }
    }

    /// `session/new`: the agent, vault and environment by name or id.
    async fn create(&self, c: &Client) -> Result<Conversation, String> {
        let want = self.l.agent.clone().filter(|a| !a.is_empty()).ok_or("the block names no Managoat agent")?;
        let agent_id = if uuid_like(&want) {
            want
        } else {
            let agents = c.agents().await.map_err(|e| e.to_string())?;
            agents
                .items
                .iter()
                .find(|a| a.name == want || a.id == want)
                .map(|a| a.id.clone())
                .ok_or_else(|| format!("no agent {want:?} on this Managoat account"))?
        };
        let named = |list: Vec<super::client::Named>, want: &str, what: &str| {
            list.iter()
                .find(|n| n.id == want || n.name == want)
                .map(|n| n.id.clone())
                .ok_or_else(|| format!("no {what} named {want:?}"))
        };
        let vault_id = match self.l.vault.as_deref().filter(|v| !v.is_empty()) {
            None => None,
            Some(v) if uuid_like(v) => Some(v.to_owned()),
            Some(v) => Some(named(c.vaults().await.map_err(|e| e.to_string())?.items, v, "vault")?),
        };
        let environment_id = match self.l.environment.as_deref().filter(|v| !v.is_empty()) {
            None => None,
            Some(v) if uuid_like(v) => Some(v.to_owned()),
            Some(v) => Some(named(c.environments().await.map_err(|e| e.to_string())?.items, v, "environment")?),
        };
        let new = NewConversation {
            agent_id,
            vault_id,
            environment_id,
            model: self.l.model.clone(),
            labels: self.l.labels.clone(),
            permission_policy: permission_policy(self.l.permission.as_deref().unwrap_or("ask"))?,
            ..Default::default()
        };
        Ok(c.create(&new).await.map_err(|e| e.to_string())?.conversation)
    }
}

fn uuid_like(s: &str) -> bool {
    s.len() == 36 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-') && s.matches('-').count() == 4
}

/// `--permission`'s grammar: a verdict (`ask`, `auto_allow`, `auto_deny`)
/// for every tool, and `key=verdict` pairs for a tool's title or ACP kind.
pub fn permission_policy(s: &str) -> Result<Option<Value>, String> {
    let mut policy = Map::new();
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = match part.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => ("default", part),
        };
        if k.is_empty() {
            return Err(format!("permission: {part:?} has an empty tool key"));
        }
        if !matches!(v, "auto_allow" | "ask" | "auto_deny") {
            return Err(format!("permission: unknown verdict {v:?} (want auto_allow, ask or auto_deny)"));
        }
        policy.insert(k.to_owned(), json!(v));
    }
    Ok((!policy.is_empty()).then_some(Value::Object(policy)))
}

// ---------------------------------------------------------------- reading

/// One SSE message as an event.
fn ev_of(m: &super::stream::Message) -> Option<Ev> {
    let id = m.id.as_deref().and_then(|i| i.trim().parse().ok()).unwrap_or(0);
    let p: Value = serde_json::from_str(&m.data).ok()?;
    Some(Ev::from_payload(id, &m.event, &p))
}

/// Everything stored after `after`, oldest first, then the stream closes
/// (`wait=false`). The last id read.
async fn drain(c: &Client, conv: &str, after: u64, mut each: impl FnMut(Ev)) -> Result<u64, Error> {
    let res = c.open_stream(&stream_http(), conv, "acp,stage", false, after).await?;
    let body = tokio::time::timeout(Duration::from_secs(120), res.bytes())
        .await
        .map_err(|_| Error::Unreachable("the history took too long".into()))?
        .map_err(|e| Error::Unreachable(format!("the history broke off: {e}")))?;
    let mut p = Parser::default();
    let mut last = after;
    let mut msgs = p.push(&body);
    msgs.extend(p.push(b"\n\n"));
    for m in msgs {
        if let Some(ev) = ev_of(&m) {
            if ev.id > 0 && ev.id <= last {
                continue;
            }
            last = last.max(ev.id);
            each(ev);
        }
    }
    Ok(last)
}

/// Follow the conversation from `cursor`, handing each new event on: the
/// account stream's fan-out when this daemon runs it, else the
/// conversation's own stream.
async fn follower(c: Client, conv: String, mut cursor: u64, tx: mpsc::UnboundedSender<Got>) {
    let hub = account::get().filter(|a| a.stream.lock().unwrap().enabled);
    let mut failures = 0u32;
    if let Some(a) = hub {
        let mut sub = a.hub().subscribe(&conv);
        loop {
            // What came since the cursor, then what the hub hands on.
            let send = |ev: Ev, cursor: &mut u64| {
                if ev.id > 0 && ev.id <= *cursor {
                    return true;
                }
                *cursor = (*cursor).max(ev.id);
                tx.send(Got::Ev(ev)).is_ok()
            };
            let mut got = vec![];
            match drain(&c, &conv, cursor, |ev| got.push(ev)).await {
                Ok(_) => failures = 0,
                Err(e) if matches!(e.status(), Some(404 | 410)) => {
                    let _ = tx.send(Got::Gone(format!("conversation {conv} is gone from Managoat")));
                    return;
                }
                Err(e) => {
                    failures += 1;
                    warn!(conversation = conv, error = %e, "catching up a Managoat conversation");
                    tokio::time::sleep(backoff(failures)).await;
                    continue;
                }
            }
            for ev in got {
                if !send(ev, &mut cursor) {
                    return;
                }
            }
            loop {
                let lost = tokio::select! {
                    r = sub.recv() => match r {
                        Ok(ev) => {
                            if ev.kind == "conversations" {
                                continue;
                            }
                            let e = Ev::from_payload(ev.id, &ev.kind, &ev.data);
                            if !send(e, &mut cursor) {
                                return;
                            }
                            false
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            info!(conversation = conv, n, "fell behind the account stream; catching up");
                            true
                        }
                        Err(broadcast::error::RecvError::Closed) => return,
                    },
                    // The account stream down: read what it would have
                    // brought from the conversation itself.
                    _ = tokio::time::sleep(Duration::from_secs(15)) => !a.stream.lock().unwrap().connected,
                };
                if lost {
                    break;
                }
            }
        }
    }
    let idle = super::stream::idle();
    loop {
        match follow_once(&c, &conv, &mut cursor, idle, &tx).await {
            Ok(()) => {
                failures = 0;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(e) if matches!(e.status(), Some(404 | 410)) => {
                let _ = tx.send(Got::Gone(format!("conversation {conv} is gone from Managoat")));
                return;
            }
            Err(e) => {
                failures += 1;
                let pause = e.retry_after().unwrap_or_else(|| backoff(failures));
                warn!(conversation = conv, error = %e, retry_ms = pause.as_millis() as u64, "following a Managoat conversation");
                tokio::time::sleep(pause).await;
            }
        }
        if tx.is_closed() {
            return;
        }
    }
}

fn backoff(failures: u32) -> Duration {
    let first = std::env::var("ARUGULA_FOUNTAIN_STREAM_RETRY_MS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(1));
    first.saturating_mul(1 << failures.saturating_sub(1).min(5)).min(Duration::from_secs(60))
}

/// One connection to the conversation's stream, until Managoat closes it
/// (or it goes silent).
async fn follow_once(
    c: &Client,
    conv: &str,
    cursor: &mut u64,
    idle: Duration,
    tx: &mpsc::UnboundedSender<Got>,
) -> Result<(), Error> {
    let mut res =
        match tokio::time::timeout(idle, c.open_stream(&stream_http(), conv, "acp,stage", true, *cursor)).await {
            Err(_) => return Err(Error::Unreachable("no answer".into())),
            Ok(r) => r?,
        };
    let mut p = Parser::default();
    loop {
        let chunk = match tokio::time::timeout(idle, res.chunk()).await {
            Err(_) => return Ok(()),
            Ok(Err(e)) => return Err(Error::Unreachable(format!("the stream broke: {e}"))),
            Ok(Ok(None)) => return Ok(()),
            Ok(Ok(Some(c))) => c,
        };
        for m in p.push(&chunk) {
            let Some(ev) = ev_of(&m) else { continue };
            if ev.id > 0 && ev.id <= *cursor {
                continue;
            }
            *cursor = (*cursor).max(ev.id);
            if tx.send(Got::Ev(ev)).is_err() {
                return Ok(());
            }
        }
    }
}

// ------------------------------------------------- the block's own actions

/// What a sandbox pane reads.
pub enum Read {
    Diff,
    GitStatus,
    Files(Option<String>),
    File(String),
}

/// Wake, Interrupt and Terminate a block's conversation.
pub async fn act(profile: Option<&str>, conversation: &str, action: &str) -> Result<Value, String> {
    let c = client_for(profile).await?;
    match action {
        "wake" => {
            let w = c.wake(conversation).await.map_err(|e| e.to_string())?;
            Ok(json!({ "status": w.status }))
        }
        "interrupt" => match c.interrupt(conversation).await {
            Ok(()) => Ok(json!({ "interrupted": true })),
            Err(e) if e.status() == Some(409) => Ok(json!({ "interrupted": false, "why": "no turn is running" })),
            Err(e) => Err(e.to_string()),
        },
        "terminate" => {
            c.terminate(conversation).await.map_err(|e| e.to_string())?;
            Ok(json!({ "terminated": true }))
        }
        a => Err(format!("{a}: not a conversation action (wake, interrupt, terminate)")),
    }
}

/// Read a conversation's sandbox. A parked one is `Ok(None)`: reads never
/// wake it.
pub async fn read(profile: Option<&str>, conversation: &str, what: Read) -> Result<Option<Value>, String> {
    let c = client_for(profile).await?;
    let conv = c.conversation(conversation).await.map_err(|e| e.to_string())?;
    let sandbox = conv
        .sandbox_id
        .clone()
        .or_else(|| conv.extra.get("sandbox").and_then(|s| s["id"].as_str()).map(str::to_owned))
        .ok_or("the conversation has no sandbox yet")?;
    let r = match what {
        Read::Diff => c.diff(&sandbox, &Default::default()).await.map(|d| json!(d)),
        Read::GitStatus => c.git_status(&sandbox, None, Some("all")).await.map(|s| json!(s)),
        Read::Files(p) => c.files(&sandbox, p.as_deref()).await.map(|l| json!(l)),
        Read::File(p) => c.file(&sandbox, &p, Some(arugula_proto::fs::READ_MAX)).await.map(|f| json!(f)),
    };
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.code() == "sandbox_not_ready" || e.status() == Some(409) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests;
