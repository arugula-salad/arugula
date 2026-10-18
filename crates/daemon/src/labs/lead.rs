//! A conversation's lead (#858): one Claude Code agent block per
//! conversation, on its home machine, that drives its links with the
//! owner's rights.
//!
//! - **Started with the conversation** ([`start`]), on the user's default
//!   model, in a tab of its own in the `conversations` session. Its token
//!   is [`Scope::Lead`](crate::mcp::Scope): its block's, plus the links by
//!   name. A lost lead is replaced the same way: the new block has the
//!   links and the last messages in its first turn, and the conversation
//!   keeps its id and timeline.
//! - **`@lead` is its next prompt** ([`tell`]). The brief is in its system
//!   prompt, but that isn't sent again when its session resumes, so each
//!   turn's prompt carries what it needs: who wrote what, what happened in
//!   the conversation since its last turn (the record's cursor), the links
//!   by name and how to answer.
//! - **Links by name** ([`route`]): `prompt_agent`, `wait` and
//!   `read_output` take `L1`, `L2`… as their pane. One here becomes its pane
//!   number for that call alone; one on another machine is done there,
//!   through the relay, under its grant (the grant maker's rights, an
//!   editor's at most). Nothing else reaches a link: its cards are the
//!   people's, and `send_input` and `run` are a step past prompting its
//!   agent.
//! - **Woken by the links it waits on:** a link it prompted that ends its
//!   turn ([`link_ended`]) is its next prompt, now if it's idle, else when
//!   its turn ends ([`watch`]).
//! - **Replies** are its `post_thread` calls ([`posted`]), or its turn's
//!   last message when it posted nothing. A reply to an `@lead` notifies
//!   whoever asked.
//! - **`start_agent`** from the lead opens beside it, and the new block is
//!   a link ([`adopt`]): its grant is made here, with the owner's rights.
//!
//! What it has seen, whom it owes an answer and which links it waits on are
//! in the conversation's record (`ConvLead`), so a restart loses none of
//! it ([`resume`]).

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use arugula_mux::mux::LeadChange;
use arugula_proto::{
    Attention, BlockType, ConvId, ConvLink, Conversation, Driver, EventKind, PaneId, ThreadMsg, ThreadTarget,
    api::{ConvGrantRequest, ConvLinkRequest, OpenRequest, PromptResult, WaitResult},
};
use serde_json::{Value, json};
use tracing::{info, warn};

use super::links::{self, Refusal};
use crate::{
    acl::Principal,
    mcp::tools::{Call, Out, done},
    mux::{Api, ConvReq, ThreadPost},
    server::App,
};

/// The session every lead gets a tab of its own in.
const SESSION: &str = "conversations";

/// Who the lead's messages are by.
pub const WHO: &str = "lead";

/// What a new lead (a lost one's replacement) is shown of the
/// conversation: its last messages.
const LAST_N: usize = 20;

/// The most messages a turn's prompt lists; `read_thread` has the rest.
const SHOWN: usize = 30;

/// The tools a lead uses without asking: its conversation's and its links'.
/// Answering a link's card isn't one: people do that.
const TOOLS: [&str; 6] = [
    "mcp__arugula__prompt_agent",
    "mcp__arugula__wait",
    "mcp__arugula__read_output",
    "mcp__arugula__read_thread",
    "mcp__arugula__post_thread",
    "mcp__arugula__list",
];

/// What a lead is told once, in its system prompt. Each turn's prompt
/// says the same in short, since this isn't sent again on a resume.
const BRIEF: &str = "You lead an Arugula conversation. People talk in it, and it links panes (each running an \
agent, often Claude Code) on one or more machines, named L1, L2, ... Each message that calls you (@lead) is your \
next prompt, with what happened in the conversation since your last turn; so is a link you prompted ending its turn. \
Drive a link with Arugula's tools, giving its name as the pane: prompt_agent (prompt it, waiting for its turn), wait \
(until idle or needs_input) and read_output (screen: true for what it shows). Never give a link's pane number: a \
number reaches only your own tab. Prompt every link that should work before waiting on any, so they work at once. \
When a link asks for a permission or a question, don't answer it: the people answer it in the conversation, where it \
shows as a card; say what it asks if it matters, and wait. Your own permission prompts are cards there too, \
answered by the people. list kind links shows the links and their state. \
start_agent opens a helper beside you, which becomes the next link. Answer the people with post_thread, giving no \
pane or session: it posts in your conversation. If you end your turn without posting, your last message is posted \
for you. read_thread has the whole conversation. Keep replies short.";

// ---------------------------------------------------------------- record

/// Every conversation kept here (the record, whoever owns it).
async fn get(app: &App, id: ConvId) -> Result<Conversation, Refusal> {
    let all = app.mux.api(Api::Convs).await.ok_or((503, "shutting down".to_owned()))?;
    all.into_iter().find(|c| c.id == id).ok_or((404, "no such conversation".to_owned()))
}

async fn change(app: &App, id: ConvId, change: LeadChange) -> Option<Conversation> {
    match app.mux.api(|r| Api::Conv(ConvReq::Lead { id, change }, Principal::Owner, r)).await {
        Some(Ok(c)) => Some(c),
        Some(Err(e)) => {
            warn!(conv = %id, error = e.1, "can't keep the lead's state");
            None
        }
        None => None,
    }
}

/// The conversation `block` leads, if it's a lead.
pub async fn of(app: &App, block: PaneId) -> Option<ConvId> {
    let all = app.mux.api(Api::Convs).await?;
    all.into_iter().find(|c| c.lead.as_ref().is_some_and(|l| l.block == block)).map(|c| c.id)
}

/// Who the conversation's owner is here: whose rights the lead acts with.
fn owner(c: &Conversation) -> Principal {
    if c.owner == "owner" {
        Principal::Owner
    } else {
        Principal::User { id: c.owner.clone(), name: c.owner_name.clone(), pic: None }
    }
}

async fn block(app: &App, id: PaneId) -> Option<Arc<dyn crate::block::Block>> {
    app.mux.api(|r| Api::Block(id, r)).await.flatten()
}

/// The lead's block, if it's still there.
async fn alive(app: &App, c: &Conversation) -> Option<Arc<dyn crate::block::Block>> {
    block(app, c.lead.as_ref()?.block).await
}

/// Whether a block is in a turn now: working, or waiting on its own card
/// (#859), which is still its turn.
fn working(b: &Arc<dyn crate::block::Block>) -> bool {
    matches!(b.state()["attention"].as_str(), Some("working" | "needs_input"))
}

async fn messages(app: &App, id: ConvId) -> Vec<ThreadMsg> {
    match app.mux.api(|r| Api::ThreadGet(ThreadTarget::Conv(id), Principal::Owner, r)).await {
        Some(Ok(m)) => m,
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------- start

/// The user's default model: Claude Code's `model` setting, in the
/// daemon's Claude directory. None: Claude Code's own default.
fn user_model() -> Option<String> {
    // S38 prototype: the rig runs its leads on a cheap model.
    if let Ok(m) = std::env::var("S38_LEAD_MODEL") {
        return Some(m);
    }
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| dirs_home().map(|h| h.join(".claude")))?;
    let v: Value = serde_json::from_slice(&std::fs::read(dir.join("settings.json")).ok()?).ok()?;
    v["model"].as_str().map(str::trim).filter(|m| !m.is_empty()).map(str::to_owned)
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

/// `POST /api/convs/{id}/lead`, and a new conversation's: start its lead,
/// or a new one in place of a lost one. Refused while its lead is there.
pub async fn start(app: &Arc<App>, id: ConvId) -> Result<Conversation, Refusal> {
    let c = get(app, id).await?;
    if let Some(l) = &c.lead
        && block(app, l.block).await.is_some()
    {
        return Err((409, format!("it has a lead, %{}", l.block)));
    }
    // A new lead is shown the last messages and the links in its first
    // turn: everything after the cursor.
    let msgs = messages(app, id).await;
    let cursor = msgs.len().checked_sub(LAST_N + 1).map(|i| msgs[i].id).unwrap_or(0);
    let mut config = json!({
        "agent": "claude",
        "title": format!("lead · {}", c.title),
        "allow": TOOLS.iter().map(|t| json!({ "tool": t })).collect::<Vec<_>>(),
        "brief": BRIEF,
    });
    if let Some(m) = user_model() {
        config["model"] = json!(m);
    }
    let req = OpenRequest { kind: BlockType::Agent, config, session: Some(SESSION.into()), ..Default::default() };
    let b = match app.mux.api(|r| Api::Open(req, None, r)).await {
        Some(Ok(b)) => b,
        Some(Err(e)) => return Err((502, format!("the lead couldn't start: {e}"))),
        None => return Err((503, "shutting down".into())),
    };
    let started = LeadChange::Start { block: b, at: crate::store::now_ms(), cursor };
    let c = change(app, id, started).await.ok_or((500, "can't keep the lead".to_owned()))?;
    info!(conv = %id, block = b, "a conversation's lead started");
    watch(app, id, b);
    Ok(c)
}

// ---------------------------------------------------------------- turns

/// Why the lead gets a prompt.
enum Why<'a> {
    /// Someone wrote `@lead`.
    Asked(&'a ThreadMsg),
    /// Links it prompted ended their turns.
    Woke(&'a [String]),
    /// A restart cut its turn off.
    Restarted,
}

fn line(s: &str, max: usize) -> String {
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max { s } else { s.chars().take(max).collect::<String>() + "…" }
}

/// Where a link is, to say.
fn place(l: &ConvLink) -> String {
    let what = if l.title.is_empty() { "a pane".to_owned() } else { l.title.clone() };
    let at = if l.machine.is_none() { format!("{} (this machine)", l.machine_name) } else { l.machine_name.clone() };
    match &l.closed {
        Some(why) => format!("{what} on {at}, closed: {why}"),
        None => format!("{what} on {at}"),
    }
}

/// A turn's prompt: why, what happened since the lead's last turn, the
/// links by name, and how to answer.
fn prompt(c: &Conversation, msgs: &[ThreadMsg], why: &Why<'_>) -> String {
    let cursor = c.lead.as_ref().map_or(0, |l| l.cursor);
    let asked = match why {
        Why::Asked(m) => Some(m.id),
        _ => None,
    };
    let mut out = match why {
        Why::Asked(m) => format!("{} wrote in the conversation \"{}\": {}\n", m.name, c.title, m.text.trim()),
        Why::Woke(links) => {
            let (names, verb) = match links {
                [one] => (one.clone(), "ended its turn"),
                more => (more.join(" and "), "ended their turns"),
            };
            format!("{names} (you prompted it) {verb}, in the conversation \"{}\".\n", c.title)
        }
        Why::Restarted => format!(
            "Arugula restarted while you were working in the conversation \"{}\", which cut your turn off. \
             Carry on from what happened since.\n",
            c.title
        ),
    };
    let since: Vec<&ThreadMsg> = msgs.iter().filter(|m| m.id > cursor && Some(m.id) != asked).collect();
    if !since.is_empty() {
        out.push_str("\nSince your last turn:\n");
        if since.len() > SHOWN {
            out.push_str(&format!(
                "({} earlier messages: read_thread with after {cursor} has them)\n",
                since.len() - SHOWN
            ));
        }
        for m in since.iter().skip(since.len().saturating_sub(SHOWN)) {
            out.push_str(&format!("- #{} {}: {}\n", m.id, m.name, line(&m.text, 300)));
        }
    }
    if c.links.is_empty() {
        out.push_str("\nNo panes are linked yet.\n");
    } else {
        out.push_str("\nLinks (give the name as the pane, never a number):\n");
        for l in &c.links {
            out.push_str(&format!("- {}: {}\n", l.name, place(l)));
        }
    }
    out.push_str(
        "\nPrompt the links that should work before waiting on any. Their cards are the people's to answer. \
         Answer with post_thread (no pane or session: it posts here), or end your turn and your last message is \
         posted.",
    );
    out
}

/// Hand the lead its next prompt, and move its cursor. `Ok(true)`: it went
/// straight in; `Ok(false)`: it's queued behind the lead's turn.
async fn deliver(app: &App, c: &Conversation, why: Why<'_>) -> Result<bool, String> {
    let b = alive(app, c).await.ok_or("the lead's block is gone")?;
    let msgs = messages(app, c.id).await;
    let text = prompt(c, &msgs, &why);
    let now = !working(&b);
    let by = match &why {
        Why::Asked(m) if m.who != "owner" => Some(m.name.clone()),
        _ => None,
    };
    b.call_by("send", json!({ "text": text }), by.as_deref()).await?;
    let cursor = msgs.last().map_or(0, |m| m.id);
    change(app, c.id, LeadChange::Turn { cursor }).await;
    if let Why::Woke(links) = why {
        change(app, c.id, LeadChange::Woken(links.to_vec())).await;
    }
    Ok(now)
}

/// An `@lead` in conversation `id` (from someone in it, as the mux
/// checked): the lead's next prompt. A lost lead is replaced first.
pub async fn tell(app: &Arc<App>, id: ConvId, msg: &ThreadMsg) -> Result<bool, String> {
    let mut c = get(app, id).await.map_err(|e| e.1)?;
    if alive(app, &c).await.is_none() {
        c = start(app, id).await.map_err(|e| e.1)?;
        info!(conv = %id, "the lead was gone: a new one took its place");
    }
    let c = change(app, id, LeadChange::Asked(msg.who.clone())).await.unwrap_or(c);
    deliver(app, &c, Why::Asked(msg)).await
}

/// The lead posted message `msg` in its conversation: a reply, which
/// tells whoever asked it.
pub async fn posted(app: &App, id: ConvId, msg: u64) {
    let Some(c) = change(app, id, LeadChange::Posted).await else { return };
    let asked = c.lead.map(|l| l.asked).unwrap_or_default();
    if !asked.is_empty() {
        change(app, id, LeadChange::Replied { who: asked, msg }).await;
    }
}

/// Post `text` in the conversation as the lead's reply.
async fn reply(app: &App, id: ConvId, text: String) -> Option<u64> {
    let post = ThreadPost {
        target: ThreadTarget::Conv(id),
        who: Principal::Owner,
        acting: None,
        as_agent: Some(Driver { who: WHO.into(), name: WHO.into() }),
        text,
        quote: None,
        event: None,
    };
    match super::convstore::post(app, post).await {
        Some(Ok((m, _, _))) => Some(m.id),
        Some(Err(e)) => {
            warn!(conv = %id, error = e.1, "can't post the lead's reply");
            None
        }
        None => None,
    }
}

/// The text of the lead's last turn's last message, if it said anything.
fn last_message(b: &Arc<dyn crate::block::Block>) -> Option<String> {
    let state = b.state();
    let entries = state["entries"].as_array()?;
    let turn = entries.iter().rposition(|e| e["type"] == "user").map_or(0, |i| i + 1);
    entries[turn..]
        .iter()
        .rev()
        .find(|e| e["type"] == "agent")
        .and_then(|e| e["text"].as_str())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

/// When the block's last turn began: its prompt's going in (ms).
fn turn_began(b: &Arc<dyn crate::block::Block>) -> u64 {
    let state = b.state();
    let entries = state["entries"].as_array().map(Vec::as_slice).unwrap_or_default();
    entries.iter().rev().find(|e| e["type"] == "user").and_then(|e| e["at_ms"].as_u64()).unwrap_or(0)
}

/// A turn of the lead's ended: post its last message unless it posted,
/// then tell it about links that ended meanwhile.
async fn turn_ended(app: &App, id: ConvId, b: &Arc<dyn crate::block::Block>) {
    let Ok(c) = get(app, id).await else { return };
    if c.lead.is_none() {
        return;
    }
    // Whether it posted this turn: a reply since the turn's prompt went in.
    // Not a count kept across turns: a prompt queued behind a turn starts
    // as that turn ends, and its block may never look idle in between, so
    // that turn's end isn't seen here (#859).
    let began = turn_began(b);
    let msgs = messages(app, id).await;
    let replied = msgs.iter().any(|m| m.at >= began && m.who == WHO && m.event.is_none());
    if !replied
        && let Some(text) = last_message(b)
        && let Some(msg) = reply(app, id, text).await
    {
        posted(app, id, msg).await;
    }
    let Some(c) = change(app, id, LeadChange::TurnDone).await else { return };
    let woke = c.lead.as_ref().map(|l| l.woke.clone()).unwrap_or_default();
    if !woke.is_empty()
        && let Err(e) = deliver(app, &c, Why::Woke(&woke)).await
    {
        warn!(conv = %id, error = e, "can't wake the lead");
    }
}

/// The runners following each lead's turns, by conversation.
static WATCHING: LazyLock<Mutex<HashMap<ConvId, tokio::task::AbortHandle>>> = LazyLock::new(Default::default);

/// Follow the lead `lead`'s turns: each one's end is [`turn_ended`]. What
/// it asks itself (a tool outside its allow list, or a question) is a card
/// in the conversation (#859), by the links' rules: held for a moment, so
/// one its allow list or a standing rule answers isn't posted.
pub fn watch(app: &Arc<App>, id: ConvId, lead: PaneId) {
    let app = app.clone();
    let task = tokio::spawn(async move {
        let mut events = app.mux.events();
        let mut busy = block(&app, lead).await.is_some_and(|b| working(&b));
        let mut asks = super::events::Rules::new(WHO);
        loop {
            let due = asks.due().map(|at| Duration::from_millis(at.saturating_sub(crate::store::now_ms())));
            let got = match due {
                Some(d) => match tokio::time::timeout(d, events.recv()).await {
                    Ok(got) => got,
                    Err(_) => {
                        if let Some(card) = asks.tick(crate::store::now_ms()) {
                            super::events::post_lead_card(&app, id, card).await;
                        }
                        continue;
                    }
                },
                None => events.recv().await,
            };
            if let Ok(e) = &got
                && e.pane == Some(lead)
                && matches!(e.kind, EventKind::Attention { .. })
            {
                // Only its cards: its turn's end is its reply, below.
                asks.on(&e.kind, crate::store::now_ms());
            }
            match got {
                Ok(e) if e.pane == Some(lead) => match e.kind {
                    EventKind::Attention { state: Attention::Working, .. } => busy = true,
                    EventKind::Attention { state: Attention::Done | Attention::Idle, .. } if busy => {
                        busy = false;
                        if let Some(b) = block(&app, lead).await {
                            turn_ended(&app, id, &b).await;
                        }
                    }
                    EventKind::Closed => {
                        info!(conv = %id, block = lead, "a conversation's lead closed: the next @lead starts another");
                        break;
                    }
                    _ => {}
                },
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => break,
            }
        }
    });
    if let Some(old) = WATCHING.lock().unwrap().insert(id, task.abort_handle()) {
        old.abort();
    }
}

/// Stop following a conversation's lead (it was deleted).
pub fn stop(id: ConvId) {
    if let Some(h) = WATCHING.lock().unwrap().remove(&id) {
        h.abort();
    }
}

/// Link `link` of conversation `id` ended its turn (the watcher posted
/// it): if the lead waited on it, that's its next prompt, now if it's
/// idle, else when its turn ends.
pub async fn link_ended(app: &App, id: ConvId, link: &str) {
    let Ok(c) = get(app, id).await else { return };
    if !c.lead.as_ref().is_some_and(|l| l.awaiting.iter().any(|a| a == link)) {
        return;
    }
    let Some(c) = change(app, id, LeadChange::LinkEnded(link.to_owned())).await else { return };
    let Some(b) = alive(app, &c).await else { return };
    if working(&b) {
        return;
    }
    let woke = c.lead.as_ref().map(|l| l.woke.clone()).unwrap_or_default();
    if !woke.is_empty()
        && let Err(e) = deliver(app, &c, Why::Woke(&woke)).await
    {
        warn!(conv = %id, error = e, "can't wake the lead");
    }
}

/// At start-up: follow each lead again, and give it what it was owed. A
/// link it waited on that finished while the daemon was down wakes it; a
/// turn the restart cut off is taken up again when someone waits on its
/// answer.
pub fn resume(app: &Arc<App>) {
    let app = app.clone();
    tokio::spawn(async move {
        let convs = app.mux.api(Api::Convs).await.unwrap_or_default();
        for c in convs {
            // S38: one kept in control is led by the lease's holder, which
            // hands it each @lead; nothing here resumes it.
            if super::convstore::stored(c.id) {
                continue;
            }
            let Some(l) = &c.lead else { continue };
            if block(&app, l.block).await.is_none() {
                continue;
            }
            watch(&app, c.id, l.block);
            let app = app.clone();
            tokio::spawn(async move { catch_up(&app, c).await });
        }
    });
}

async fn catch_up(app: &Arc<App>, c: Conversation) {
    let Some(l) = c.lead.clone() else { return };
    // Its block is resuming its session: let it.
    let Some(b) = settled(app, l.block).await else { return };
    // Links it waited on that are done now.
    if !l.awaiting.is_empty()
        && let Ok(now) = links::statuses(app, c.id, &owner(&c)).await
    {
        for s in now {
            let done = matches!(s.attention, Some(Attention::Done | Attention::Idle))
                || s.state == arugula_proto::ConvLinkState::Closed;
            if done && l.awaiting.contains(&s.name) {
                change(app, c.id, LeadChange::LinkEnded(s.name)).await;
            }
        }
    }
    let Ok(c) = get(app, c.id).await else { return };
    let Some(l) = &c.lead else { return };
    let cut = b.state()["cut"] == true || b.state()["attention"] == "needs_input";
    let r = if !l.woke.is_empty() {
        let woke = l.woke.clone();
        deliver(app, &c, Why::Woke(&woke)).await
    } else if cut && !l.asked.is_empty() {
        deliver(app, &c, Why::Restarted).await
    } else {
        return;
    };
    if let Err(e) = r {
        warn!(conv = %c.id, error = e, "can't take the lead up again after a restart");
    }
}

/// The block once it's done starting (at most a minute), unless it's gone.
async fn settled(app: &App, id: PaneId) -> Option<Arc<dyn crate::block::Block>> {
    for _ in 0..60 {
        let b = block(app, id).await?;
        if b.state()["status"] != "starting" {
            return Some(b);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    block(app, id).await
}

// ---------------------------------------------------------------- MCP

/// `L2` (any case) as a link's name, if `s` is one.
fn link_name(s: &str) -> Option<String> {
    let s = s.trim();
    let digits = s.strip_prefix(['L', 'l'])?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| format!("L{digits}"))
}

/// Why `tool` doesn't take link `name`.
fn not_on_links(tool: &str, name: &str) -> String {
    match tool {
        "agent_respond" => format!(
            "{name}'s permission prompts and questions are the people's to answer, on its card in the conversation: \
             say what it asks if it matters, and wait on it"
        ),
        "send_input" | "run" | "attach" => {
            format!("{tool} doesn't reach a link: prompt its agent with prompt_agent {{pane: \"{name}\"}}")
        }
        _ => format!("{tool} doesn't take a link: links are for prompt_agent, wait and read_output"),
    }
}

/// A lead's call that names a link: `Ok(args)` to go on with, its pane
/// number put in for a link here (for this call alone); `Err(out)` for one
/// done on another machine, or refused.
pub async fn route(call: &Call<'_>, conv: ConvId, tool: &str, mut args: Value) -> Result<Value, Out> {
    let named = ["pane", "beside", "split", "block"]
        .into_iter()
        .find_map(|k| args.get(k).and_then(Value::as_str).and_then(link_name).map(|n| (k, n)));
    let Some((key, name)) = named else { return Ok(args) };
    if key != "pane" || !matches!(tool, "prompt_agent" | "wait" | "read_output") {
        return Err(Err(not_on_links(tool, &name)));
    }
    let app = call.app();
    let c = get(app, conv).await.map_err(|e| Err(e.1))?;
    let Some(l) = c.links.iter().find(|l| l.name == name).cloned() else {
        let names: Vec<&str> = c.links.iter().filter(|l| l.closed.is_none()).map(|l| l.name.as_str()).collect();
        return Err(Err(match names.as_slice() {
            [] => format!("no link {name}: this conversation links no panes"),
            n => format!("no link {name}: the links are {}", n.join(", ")),
        }));
    };
    if let Some(why) = &l.closed {
        return Err(Err(format!("{name} is closed: {why}")));
    }
    if tool == "prompt_agent" {
        change(app, conv, LeadChange::Await(name.clone())).await;
    }
    let here = links::is_here(app, &l.machine);
    if here {
        // Here: as its grant still allows.
        let me = links::me(app);
        // S38 prototype: a conversation kept in control is led by whichever
        // of the owner's machines holds the lease, so its grant names the
        // conversation, not one machine (q5).
        let ours = |g: &arugula_proto::ConvGrant| {
            g.conv == conv && (g.device == me || super::convstore::leading(conv).is_some())
        };
        if !links::grants(app, l.pane).await.iter().any(ours) {
            return Err(Err(format!("{name}'s grant was revoked, or its pane closed")));
        }
        call.set_link(&name, l.pane);
        args["pane"] = json!(l.pane);
        if tool != "wait" {
            return Ok(args);
        }
    }
    let out = match tool {
        "wait" => wait(call, &l, conv, here, &args).await,
        _ => remote(call, &l, conv, tool, &args).await,
    };
    if let Ok(v) = &out {
        after(app, conv, &name, tool, v).await;
    }
    Err(out)
}

/// Whether a link's answer says its turn is over.
fn finished(v: &Value) -> bool {
    v["result"] == "done" || (v["result"] != "timeout" && matches!(v["state"].as_str(), Some("done" | "idle")))
}

/// After a call on link `name` came back with `v`: a turn the lead saw end
/// itself won't wake it.
pub async fn after(app: &App, conv: ConvId, name: &str, tool: &str, v: &Value) {
    if matches!(tool, "prompt_agent" | "wait") && finished(v) {
        change(app, conv, LeadChange::Settled(name.to_owned())).await;
    }
}

/// `tool` on link `l` on another machine, through the relay, under its
/// grant there.
async fn remote(call: &Call<'_>, l: &ConvLink, conv: ConvId, tool: &str, a: &Value) -> Out {
    let app = call.app();
    let mid = l.machine.clone().unwrap_or_default();
    let m = crate::peer::find_by_id(&app.control, &mid)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{} ({}) isn't reachable from here now", l.name, l.machine_name))?;
    let conv = conv.to_string();
    let headers = [(links::CONV_HEADER, conv.as_str())];
    let pane = l.pane;
    // One request on the channel holds it for at most this long; a longer
    // wait answers "still running", as a wait here does.
    let limit = Call::limit(a["timeout"].as_f64()).min(Duration::from_secs(90));
    let secs = limit.as_secs_f64();
    let ask = |method: &'static str, path: String, body: Option<Value>, within: Duration| {
        let (m, headers) = (&m, &headers);
        async move {
            crate::peer::request_within(&app.control, m, method, &path, body.as_ref(), headers, within)
                .await
                .map_err(|e| format!("{} didn't answer: {e}", m.name))
        }
    };
    let refused = |status: u16, body: &[u8]| format!("{} on {} said {status}: {}", l.name, m.name, links::said(body));
    let (name, at) = (&l.name, &m.name);
    match tool {
        "prompt_agent" => {
            let text = a["text"].as_str().ok_or("prompt_agent needs text")?;
            let body = json!({ "text": text, "answering": a["answering"].as_bool().unwrap_or(false), "timeout": secs });
            let fut = ask("POST", format!("/api/panes/{pane}/prompt"), Some(body), limit + Duration::from_secs(10));
            let (status, got) = call
                .waiting(&format!("waiting for {name}'s turn"), limit + Duration::from_secs(10), fut)
                .await
                .ok_or_else(|| format!("{name} on {at} didn't answer within {secs}s: wait on it"))??;
            if status >= 400 {
                return Err(refused(status, &got));
            }
            let r: PromptResult = serde_json::from_slice(&got).map_err(|e| format!("{name}: {e}"))?;
            let summary = match &r {
                PromptResult::Done => format!("{name} finished its turn; read_output (screen: true) for what it said"),
                PromptResult::NeedsInput { question, .. } => format!(
                    "{name} asks: {}. It's a card in the conversation, for the people: wait on {name}",
                    question.as_deref().unwrap_or("for someone")
                ),
                PromptResult::Blocked { question, .. } => format!(
                    "{name} was already waiting on someone ({}), so nothing was typed; the people answer it in the conversation",
                    question.as_deref().unwrap_or("a question")
                ),
                PromptResult::Stalled { why, .. } => format!("{name} stalled: {why}"),
                PromptResult::StillRunning => format!("{name} is still working: wait on it until idle"),
            };
            let mut v = serde_json::to_value(&r).map_err(|e| e.to_string())?;
            v["link"] = json!(name);
            done(summary, v)
        }
        "read_output" => {
            let screen = a["screen"].as_bool().unwrap_or(false);
            let path = if screen {
                format!("/api/panes/{pane}/capture")
            } else {
                format!("/api/panes/{pane}/capture?scope=scrollback")
            };
            let (status, got) = ask("GET", path, None, Duration::from_secs(20)).await?;
            if status >= 400 {
                return Err(refused(status, &got));
            }
            let text = String::from_utf8_lossy(&got);
            let max = a["max_chars"].as_u64().unwrap_or(16_000).clamp(200, 40_000) as usize;
            let text = text.trim_end();
            let cut: String = text.chars().rev().take(max).collect::<Vec<_>>().into_iter().rev().collect();
            let what = if screen { "screen" } else { "latest output" };
            done(format!("{name}'s {what} on {at}:\n{cut}"), json!({ "link": name, "text": cut }))
        }
        _ => Err(not_on_links(tool, name)),
    }
}

/// How often a link's card is looked at again, while the lead waits on the
/// link to finish: the people answer it.
const CARD_POLL: Duration = Duration::from_secs(2);

/// `wait` on link `l`: until it asks (`needs_input`), or until it's done
/// with its turn (`idle`). Waiting until idle waits through its cards,
/// which are the people's: the lead can't answer them. A link here is
/// waited on here; one elsewhere through the relay.
async fn wait(call: &Call<'_>, l: &ConvLink, conv: ConvId, here: bool, a: &Value) -> Out {
    let app = call.app();
    let name = &l.name;
    let until = match a["until"].as_str().unwrap_or("idle") {
        "needs_input" | "needs-input" => "needs-input",
        "idle" => "idle",
        other => return Err(format!("{name} is a link: wait on it until idle or needs_input, not {other}")),
    };
    let limit = Call::limit(a["timeout"].as_f64());
    let m = if here {
        None
    } else {
        let mid = l.machine.clone().unwrap_or_default();
        Some(
            crate::peer::find_by_id(&app.control, &mid)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("{name} ({}) isn't reachable from here now", l.machine_name))?,
        )
    };
    let at = m.as_ref().map_or_else(|| l.machine_name.clone(), |m| m.name.clone());
    let conv = conv.to_string();
    let headers = [(links::CONV_HEADER, conv.as_str())];
    let pane = l.pane;
    let once = |left: Duration| {
        let (m, headers) = (&m, &headers);
        async move {
            match m {
                None => match tokio::time::timeout(left, crate::api::wait_until(app, pane, until, None)).await {
                    Ok(r) => r,
                    Err(_) => Ok(WaitResult::Timeout),
                },
                Some(m) => {
                    // One request holds the channel for at most 90 s.
                    let secs = left.min(Duration::from_secs(90)).as_secs_f64();
                    let path = format!("/api/panes/{pane}/wait?until={until}&timeout={secs}");
                    let (status, got) = crate::peer::request_within(
                        &app.control,
                        m,
                        "GET",
                        &path,
                        None,
                        headers,
                        Duration::from_secs_f64(secs) + Duration::from_secs(10),
                    )
                    .await
                    .map_err(|e| format!("{} didn't answer: {e}", m.name))?;
                    if status >= 400 {
                        return Err(format!("{name} on {} said {status}: {}", m.name, links::said(&got)));
                    }
                    serde_json::from_slice::<WaitResult>(&got).map_err(|e| format!("{name}: {e}"))
                }
            }
        }
    };
    let what =
        if until == "idle" { format!("waiting for {name} to finish") } else { format!("waiting for {name} to ask") };
    let began = std::time::Instant::now();
    let r = call
        .waiting(&what, limit, async {
            loop {
                let left = limit.saturating_sub(began.elapsed());
                if left.is_zero() {
                    return Ok(WaitResult::Timeout);
                }
                let r = once(left).await?;
                // Its card is the people's: wait on through it.
                if until == "idle" && matches!(r, WaitResult::Attention { state: Attention::NeedsInput, .. }) {
                    tokio::time::sleep(CARD_POLL.min(left)).await;
                    continue;
                }
                return Ok::<_, String>(r);
            }
        })
        .await
        .unwrap_or(Ok(WaitResult::Timeout))?;
    let secs = limit.as_secs();
    let summary = match &r {
        WaitResult::Attention { state: Attention::NeedsInput, ask } => format!(
            "{name} asks: {}. It's a card in the conversation, for the people: wait on {name} until idle",
            ask.as_ref().map(|a| a.headline()).unwrap_or_else(|| "for someone".into())
        ),
        WaitResult::Attention { state: Attention::Done, .. } => {
            format!("{name} finished its turn; read_output (screen: true) for what it said")
        }
        WaitResult::Attention { .. } => format!("{name} is idle"),
        WaitResult::Timeout => format!("{name} is still working after {secs}s: wait on it again"),
        WaitResult::Exit { code } => {
            format!("{name}'s program exited{}", code.map(|c| format!(" ({c})")).unwrap_or_default())
        }
        other => format!("{name}: {}", serde_json::to_string(other).unwrap_or_default()),
    };
    let mut v = serde_json::to_value(&r).map_err(|e| e.to_string())?;
    v["link"] = json!(name);
    v["machine"] = json!(at);
    done(summary, v)
}

/// `list` kind `links`: the lead's links, each with its state.
pub async fn links_now(call: &Call<'_>) -> Out {
    let conv = call.scope().leads().ok_or("only a conversation's lead has links")?;
    let app = call.app();
    let c = get(app, conv).await.map_err(|e| e.1)?;
    let now = links::statuses(app, conv, &owner(&c)).await.map_err(|e| e.1)?;
    let rows: Vec<Value> = c
        .links
        .iter()
        .map(|l| {
            let s = now.iter().find(|s| s.name == l.name);
            json!({
                "link": l.name,
                "what": place(l),
                "state": s.map(|s| s.state),
                "attention": s.and_then(|s| s.attention),
                "why": s.and_then(|s| s.why.clone()),
            })
        })
        .collect();
    let lines: Vec<String> = rows
        .iter()
        .map(|r| {
            let state = r["attention"].as_str().or(r["state"].as_str()).unwrap_or("?");
            format!("{}: {} ({state})", r["link"].as_str().unwrap_or("?"), r["what"].as_str().unwrap_or(""))
        })
        .collect();
    let summary = match lines.len() {
        0 => format!("\"{}\" links no panes yet", c.title),
        _ => format!("\"{}\" links:\n{}", c.title, lines.join("\n")),
    };
    done(summary, json!({ "conversation": conv, "links": rows }))
}

/// The lead started `pane` beside itself (`start_agent`): it's a link, by
/// a grant made here with the owner's rights, as the lead acts with them.
pub async fn adopt(app: &Arc<App>, conv: ConvId, pane: PaneId, title: Option<String>) -> Result<String, Refusal> {
    let c = get(app, conv).await?;
    let who = owner(&c);
    let grant = ConvGrantRequest { device: None, device_name: None, conv, conv_title: Some(c.title.clone()) };
    links::grant(app, pane, grant, &who).await?;
    let req =
        ConvLinkRequest { machine: None, pane, epoch: None, title: Some(title.unwrap_or_else(|| "helper".into())) };
    let c = links::link(app, conv, req, &who).await?;
    let l = c.links.last().ok_or((500, "no link was made".to_owned()))?;
    super::events::watch(app, conv, l);
    // It was prompted at its start: its turn's end wakes the lead.
    change(app, conv, LeadChange::Await(l.name.clone())).await;
    Ok(l.name.clone())
}
