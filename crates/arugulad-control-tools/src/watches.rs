//! `watch` (Arugula #631): wake this session when something happens, after
//! its turn has ended. An agent makes a watch, ends its turn, and is woken by
//! a follow-up when it fires. Control evaluates it (`/api/daemon/watches`),
//! and the message comes back as an `agents.mention` for this Claude Code
//! session alone (`agent_mentions::watched`).
//!
//! - **Its session.** A watch is keyed to the calling pane's Claude Code
//!   session: a terminal's from its hooks, or an agent block's. A pane that
//!   can't take follow-ups (no hooks heard, or no agent) is refused, since
//!   nothing could ever wake it. So is a hosted box's session, for now.
//! - **Its targets.** A pull request on github.com in a repo linked through
//!   Arugula's GitHub App, until it merges or closes. Control refuses a repo
//!   without the App, and a PR the owner can't see answers as one that
//!   doesn't exist.
//! - **Events.** A type in the event bus's catalog (`events::CATALOG`),
//!   matched on its fields, with the agent's note handed back when it
//!   fires. `types` lists the catalog. A type that matches on project,
//!   given none, gets the current project (`Call::current_project`, as
//!   `issue_write`'s) when it names an issue by `#N` or no subject at all; a
//!   subject named by its id is left anywhere.
//! - **Local watches.** The daemon's own types (a pane's, an agent's:
//!   `agent.idle`, `pane.exited`, `pane.output.matched`) are held by this daemon
//!   (`arugulad_base::local_watches`), with no account, under the same
//!   rules. `list` shows them with the cloud's, and `cancel` takes either.
//! - **Once.** It fires once, or expires (24 hours unless told, at most 7
//!   days) with one last message saying it ended without firing.
//! - **Waits.** `wait` with an event is a short-lived watch, made and
//!   checked as `create`'s ([`wait_event`],
//!   https://app.arugula.io/issues/iss_1545620a176cab44b21084c27493241e):
//!   the call blocks until it fires, and its message is the answer.

use arugula_control_wire::events::{self, AGENT_EVENTS, EventsPage};
use arugula_control_wire::watch::{
    AGENT_WATCHES, AGENT_WATCHES_CANCEL, EventTarget, TARGETS_MAX, WatchCancel, WatchCancelled, WatchCreate, WatchList,
    WatchMade, WatchState, WatchTarget, Watcher, event_target, event_types as types, expires_ms as expires,
    until_word_parse as until,
};
use std::time::Duration;

use arugula_proto::PaneId;
use arugulad_base::forge_wait::PrRef;
use arugulad_base::local_watches::{self, On};
use reqwest::Method;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::control::CloudRefused;
use crate::mcp::tools::{Call, Kind, Out, done, parse};

pub(crate) const TOOL: &str = "watch";

pub(crate) const DESCRIPTION: &str = "Wake this session later, when something happens: create a watch, then end your turn (watch ends your turn; wait instead blocks it until something happens). When it fires, you get a follow-up saying what happened (ids and states only), as a chat mention arrives. It fires once. It expires after 24 hours unless you say (expires, at most 7d), and then you get one message saying it ended without firing. Targets, any of which fires it: pr (OWNER/REPO#N or its link) until merged, closed, checks_done, checks_failed or ready (checks green and it can merge cleanly), in a repo linked to Arugula's GitHub App (else use wait with pr); session (a hosted agent session, ses_) until it settles; run (run_ or dep_) until it ends; room (a chat room's id, with parent for one thread and from for one person) until a reply you didn't write; event (a type from kind types, like issue.closed) with match (some of the type's match fields, like {\"issue\": \"#12\"}) and note (why you watch, at most 200 characters, handed back when it fires). This machine's own (agent.idle, pane.exited; match {\"pane\": \"4\"}) need no account. So does pane.output.matched, with match {\"pane\": \"4\", \"pattern\": \"ready|ERROR\"}: it fires when a new line of the pane's output matches the regex (ANSI stripped, as wait's pattern is), never for output already there when you make it, and its message quotes the line. If it has already happened, create says so and makes no watch. A watch belongs to this Claude Code session: a new session in this pane doesn't get it. Works from a terminal with Arugula's hooks installed, or an agent block.";

/// `watch`'s kinds.
pub(crate) const KINDS: &[Kind] = &[
    Kind {
        name: "create",
        description: "Wake this session when a PR, a hosted session, a chat room, a run or a deploy gets somewhere, or an event happens (an agent or a pane on this machine's too). End your turn after.",
        schema: schema_for_type::<CreateArgs>,
        flag: None,
    },
    Kind {
        name: "list",
        description: "This session's live watches (all: every session's on this machine).",
        schema: schema_for_type::<ListArgs>,
        flag: None,
    },
    Kind {
        name: "cancel",
        description: "Cancel one of this session's watches by id, or all of them.",
        schema: schema_for_type::<CancelArgs>,
        flag: None,
    },
    Kind {
        name: "types",
        description: "The event types create's event takes: what each means, its match fields, and whether this machine can watch it now.",
        schema: schema_for_type::<TypesArgs>,
        flag: None,
    },
    Kind {
        name: "events",
        description: "A project's event log after a number, oldest first: the events you could watch, and where to read on.",
        schema: schema_for_type::<EventsArgs>,
        flag: None,
    },
];

#[derive(Default, Deserialize, JsonSchema)]
pub struct CreateArgs {
    /// A pull request: OWNER/REPO#N, or its link on github.com, or several.
    #[serde(default)]
    pub pr: Option<PrList>,
    /// A hosted agent session (ses_…), or several: until it settles (done,
    /// waiting for input, failed or stopped).
    #[serde(default)]
    pub session: Option<PrList>,
    /// A run (run_…) or a deploy (dep_…), or several: until it ends.
    #[serde(default)]
    pub run: Option<PrList>,
    /// A chat room (its id, as read_thread gives it): until a reply in it
    /// that this session didn't write.
    #[serde(default)]
    pub room: Option<String>,
    /// With room: only a reply under this thread (its root message's
    /// number).
    #[serde(default)]
    pub parent: Option<i64>,
    /// With room: only a reply by this person (login or name).
    #[serde(default)]
    pub from: Option<String>,
    /// merged (the default), closed (merged or not), checks_done (none
    /// queued or running), checks_failed, or ready (checks green and it can
    /// merge cleanly).
    #[serde(default)]
    pub until: Option<String>,
    /// An event type (types lists them): fires when an event of it happens
    /// that has every match field given.
    #[serde(default)]
    pub event: Option<String>,
    /// With event: field and value pairs the event must have, from the
    /// type's match fields ({"issue": "#12"}). Without project, a type that
    /// matches on it gets the current project's, so #N is its issue.
    #[serde(default, rename = "match")]
    pub matches: Option<serde_json::Map<String, Value>>,
    /// With event: why you watch, at most 200 characters: it comes back
    /// with the event when it fires.
    #[serde(default)]
    pub note: Option<String>,
    /// How long to wait before it ends without firing: 30m, 6h, 3d. Default
    /// 24h, at most 7d.
    #[serde(default)]
    pub expires: Option<String>,
}

/// One id, or several. Whichever target happens first fires the watch.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PrList {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize, JsonSchema)]
pub struct ListArgs {
    /// Every session's watches on this machine, not only this session's.
    #[serde(default)]
    pub all: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct TypesArgs {}

#[derive(Deserialize, JsonSchema)]
pub struct EventsArgs {
    /// The project (prj_… or its name). Default: the current project.
    #[serde(default)]
    pub project: Option<String>,
    /// Events numbered after this: 0 for the oldest kept, or the last
    /// read's next_after.
    #[serde(default)]
    pub after: u64,
    /// Only these types (types lists them), or one.
    #[serde(default)]
    pub types: Option<PrList>,
    /// At most this many (default 50, at most 200).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CancelArgs {
    /// The watch's id (wch_…).
    #[serde(default)]
    pub id: Option<String>,
    /// All of this session's watches.
    #[serde(default)]
    pub all: bool,
}

/// Whether the caller's watch is the daemon's to hold: an event of its
/// own types.
fn local(a: &CreateArgs) -> Option<On> {
    a.event.as_deref().map(str::trim).and_then(On::of)
}

/// What a local watch asks for, checked here, before the session is known:
/// its type, a pane, and maybe an exit code or a pattern, alone.
fn local_ask(a: CreateArgs, on: On) -> Result<local_watches::Ask, String> {
    let name = on.name();
    if a.pr.is_some() || a.session.is_some() || a.run.is_some() || a.room.is_some() || a.until.is_some() {
        return Err(format!(
            "event {name} is watched by this machine, on its own: make another watch for pr, session, run or room"
        ));
    }
    let EventTarget { matches, note, .. } = event_target(Watcher::Machine, Some(name.into()), a.matches, a.note)?;
    let mut matches = matches.unwrap_or_default();
    let pane = match matches.remove("pane") {
        Some(v) => pane_of(&v).ok_or_else(|| format!("match pane: a pane's or block's id, like 4 or %4, not {v}"))?,
        None => {
            return Err(format!("event {name} needs match pane: the pane's or block's id, like {{\"pane\": \"4\"}}"));
        }
    };
    let exit = match matches.remove("exit") {
        Some(v) => Some(code_of(&v).ok_or_else(|| format!("match exit: an exit code, like 0, not {v}"))?),
        None => None,
    };
    let pattern = match matches.remove("pattern") {
        Some(Value::String(p)) if !p.is_empty() => Some(p),
        Some(v) => return Err(format!("match pattern: a regex, like \"ERROR|panic\", not {v}")),
        None => None,
    };
    if on == On::Output && pattern.is_none() {
        return Err(format!(
            "event {name} needs match pattern: a regex, like {{\"pane\": \"4\", \"pattern\": \"ERROR\"}}"
        ));
    }
    Ok(local_watches::Ask {
        session: String::new(),
        session_name: None,
        on,
        pane,
        exit,
        pattern,
        note,
        expires_in_ms: a.expires.as_deref().map(expires).transpose()?,
        wait: false,
    })
}

/// A pane's id as a match gives it: 4, "4" or "%4".
fn pane_of(v: &Value) -> Option<PaneId> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|n| PaneId::try_from(n).ok()),
        Value::String(s) => s.trim().trim_start_matches('%').parse().ok(),
        _ => None,
    }
}

/// An exit code as a match gives it: 1 or "1".
fn code_of(v: &Value) -> Option<i32> {
    match v {
        Value::Number(n) => n.as_i64().and_then(|n| i32::try_from(n).ok()),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// What control's answer says when it fails.
fn said(e: CloudRefused) -> String {
    match e {
        CloudRefused::NotJoined => "this machine hasn't joined Arugula cloud, where watches are kept: ask your person to join it (Getting started on the web page, or `arugulad join`)".into(),
        CloudRefused::OldControl | CloudRefused::Said { status: 404 | 405, .. } => {
            "Arugula cloud doesn't keep watches yet: use wait with pr to block until it happens".into()
        }
        e => e.to_string(),
    }
}

/// `watch`.
pub(crate) async fn watch(c: &Call<'_>, kind: &str, args: Value) -> Out {
    c.owners_own(TOOL).await?;
    match kind {
        "create" => create(c, parse(args)?).await,
        "list" => list(c, parse(args)?).await,
        "cancel" => cancel(c, parse(args)?).await,
        "types" => {
            let _: TypesArgs = parse(args)?;
            let text = types(Watcher::Machine, c.app().control.enrolled().is_some());
            done(text.clone(), serde_json::json!({ "types": text }))
        }
        "events" => read_events(c, parse(args)?).await,
        k => Err(format!("{TOOL} kind {k} isn't wired up")),
    }
}

/// The caller's Claude Code session and its name, or why it has none a
/// watch could wake.
async fn session(c: &Call<'_>) -> Result<(String, Option<String>), String> {
    if c.app().control.enrolled().is_some_and(|e| e.saved.is_hosted()) {
        return Err("watch doesn't wake a session on a hosted box yet: reply in the session's thread instead".into());
    }
    let pane = c.own_pane().ok_or("watch wakes the session it's called from: call it from a pane in Arugula")?;
    let Some(id) = arugulad_base::agent_mentions::conversation(c.app(), pane).await else {
        return Err(format!(
            "%{pane} can't be woken: a watch wakes a Claude Code session through Arugula's hooks (`arugula hooks install`), or an agent block, and this pane has neither a hooked Claude Code session nor a block's"
        ));
    };
    let name = arugulad_base::agent_mentions::session_name(c.app(), pane).await;
    Ok((id, name))
}

/// When, as people read it: UTC to the minute, `2026-11-14T10:42Z`.
fn when(ms: i64) -> String {
    let t = arugula_e2e::deploy::chant::timestamp(u64::try_from(ms).unwrap_or(0));
    format!("{}Z", t.get(..16).unwrap_or(&t))
}

/// What `create` asks for: its targets, event and expiry, checked here,
/// before the session is known.
fn create_body(a: CreateArgs) -> Result<WatchCreate, String> {
    let until = until(a.until.as_deref())?;
    let list = |l: Option<PrList>| match l {
        None => vec![],
        Some(PrList::One(p)) => vec![p],
        Some(PrList::Many(ps)) => ps,
    };
    let prs = list(a.pr);
    let mut targets = vec![];
    for id in list(a.session) {
        let id = id.trim().to_owned();
        if !id.starts_with("ses_") {
            return Err(format!("session {id}: a hosted agent session's id, ses_…"));
        }
        targets.push(WatchTarget::Session { id });
    }
    for id in list(a.run) {
        let id = id.trim().to_owned();
        if !id.starts_with("run_") && !id.starts_with("dep_") {
            return Err(format!("run {id}: a run's id (run_…) or a deploy's (dep_…)"));
        }
        targets.push(WatchTarget::Run { id });
    }
    match a.room.map(|r| r.trim().to_owned()).filter(|r| !r.is_empty()) {
        Some(room) => targets.push(WatchTarget::Thread { room, parent: a.parent, from: a.from }),
        None if a.parent.is_some() || a.from.is_some() => return Err("parent and from go with a room".into()),
        None => {}
    }
    let EventTarget { event, matches, note } = event_target(Watcher::Machine, a.event, a.matches, a.note)?;
    if prs.is_empty() && targets.is_empty() && event.is_none() {
        return Err("create needs something to watch: pr, session, run, room, or event".into());
    }
    for p in &prs {
        let r = PrRef::parse(p).ok_or_else(|| format!("pr {p}: OWNER/REPO#N, or its link on github.com"))?;
        if r.host != "github.com" {
            return Err(format!("pr {p}: a watch is on github.com only; use wait with pr"));
        }
        targets.push(WatchTarget::Pr { repo: r.repo, number: r.number, until });
    }
    let expires_in_ms = a.expires.as_deref().map(expires).transpose()?;
    Ok(WatchCreate { session: String::new(), session_name: None, targets, expires_in_ms, event, matches, note })
}

/// Whether `body`'s event target wants the current project in its match:
/// its type matches on project, and the match names none. That's how `#N`
/// in it means the current project's.
fn wants_project(body: &WatchCreate) -> bool {
    let Some(t) = body.event.as_deref().and_then(events::lookup) else { return false };
    if !t.matches.iter().any(|f| f.name == "project")
        || body.matches.as_ref().is_some_and(|m| m.contains_key("project"))
    {
        return false;
    }
    // A subject named by its id (`iss_…`, `run_…`) is anywhere the owner can
    // read: limiting it to this project would keep it from ever firing. An
    // issue's `#N` means this project's, and no subject at all means the
    // project's own.
    let named = |k: &str| body.matches.as_ref().and_then(|m| m.get(k)).and_then(Value::as_str);
    match named("issue") {
        Some(i) => i.starts_with('#'),
        None => ["run", "deploy", "session"].iter().all(|k| named(k).is_none()),
    }
}

async fn create(c: &Call<'_>, a: CreateArgs) -> Out {
    if let Some(on) = local(&a) {
        let mut ask = local_ask(a, on)?;
        (ask.session, ask.session_name) = session(c).await?;
        let made = local_watches::make(c.app(), ask).await?;
        return made_said(made, "This machine made no watch, and said nothing of why");
    }
    let mut body = create_body(a)?;
    (body.session, body.session_name) = session(c).await?;
    // Without one, the watch goes as asked: the cloud says if it needs one.
    if wants_project(&body)
        && let Ok(project) = c.current_project(None).await
    {
        body.matches.get_or_insert_default().insert("project".into(), Value::String(project));
    }
    let body = serde_json::to_value(&body).map_err(|e| e.to_string())?;
    let v = c.app().control.cloud(Method::POST, AGENT_WATCHES, Some(&body)).await.map_err(said)?;
    let made: WatchMade =
        serde_json::from_value(v).map_err(|e| format!("Arugula cloud sent an answer I can't read: {e}"))?;
    made_said(made, "Arugula cloud made no watch, and said nothing of why")
}

/// How much longer than its call a wait's watch lives: it's cancelled when
/// the call ends, so this only bounds one whose cancel was lost.
const WAIT_SLACK_MS: u64 = 5 * 60_000;

/// `wait` with `event`: a short-lived watch on it, checked as `create`
/// checks one, whose message is the call's answer instead of a follow-up.
/// What has already happened answers at once. It's cancelled when the call
/// ends without it: at `limit` ("still running"), or when its client goes
/// away.
///
/// The watch has a session of its own (`wait_…`), never a conversation's:
/// no follow-up queue takes its message, and the call listens for it before
/// the cloud makes it, so one that fires at once isn't missed.
pub(crate) async fn wait_event(
    c: &Call<'_>,
    event: &str,
    matches: Option<serde_json::Map<String, Value>>,
    limit: Duration,
) -> Out {
    c.owners_own("wait with event").await?;
    let a = CreateArgs { event: Some(event.trim().to_owned()), matches, ..CreateArgs::default() };
    let what = format!("waiting for {}", event.trim());
    if let Some(on) = local(&a) {
        let (session, session_name, expires_in_ms) = wait_session(c, limit).await;
        let ask = local_watches::Ask { session, session_name, expires_in_ms, wait: true, ..local_ask(a, on)? };
        let made = local_watches::make(c.app(), ask).await?;
        let Some(id) = made.id.clone().filter(|_| made.state == WatchState::Live) else {
            return already(made, "This machine made no watch, and said nothing of why");
        };
        let watching = local_watches::Waiting::new(&c.app().local_watches, id);
        return match c.waiting(&what, limit, c.app().local_watches.fired(&watching.id)).await {
            Some(Some(text)) => fired(&watching.id, text),
            Some(None) => Err(format!("its watch {} was cancelled before it fired", watching.id)),
            None => not_yet(on.name(), &watching.id, limit),
        };
    }
    if c.app().control.enrolled().is_some_and(|e| e.saved.is_hosted()) {
        return Err("wait with event doesn't hear the cloud's events on a hosted box yet: use watch from a terminal or an agent block on a machine".into());
    }
    let mut body = create_body(a)?;
    if wants_project(&body)
        && let Ok(project) = c.current_project(None).await
    {
        body.matches.get_or_insert_default().insert("project".into(), Value::String(project));
    }
    let event = body.event.clone().unwrap_or_default();
    match cloud_wait(c, body, &what, limit).await? {
        CloudWaited::Fired(id, text) => fired(&id, text),
        CloudWaited::Already(made) => already(made, "Arugula cloud made no watch, and said nothing of why"),
        CloudWaited::NotYet(id) => not_yet(&event, &id, limit),
        CloudWaited::Lost(id) => Err(format!("its watch {id} stopped being listened for before it fired")),
    }
}

/// A wait's watch's session (`wait_…`, the call's own), its name as
/// people read it (the calling pane's session's), and how long it lives.
async fn wait_session(c: &Call<'_>, limit: Duration) -> (String, Option<String>, Option<u64>) {
    let session =
        format!("wait_{}", arugulad_base::push::random::<8>().iter().map(|b| format!("{b:02x}")).collect::<String>());
    let name = match c.own_pane() {
        Some(p) => arugulad_base::agent_mentions::session_name(c.app(), p).await,
        None => None,
    };
    let expires = u64::try_from(limit.as_millis()).unwrap_or(u64::MAX).saturating_add(WAIT_SLACK_MS);
    (session, name, Some(expires))
}

/// What a wait's cloud watch came to.
enum CloudWaited {
    /// It fired: its id and message.
    Fired(String, String),
    /// No watch was made: what had already happened, or nothing said.
    Already(WatchMade),
    /// The call ended first (its time, or its client went away): the
    /// watch is cancelled.
    NotYet(String),
    /// Its message can't reach the call any more.
    Lost(String),
}

/// A short-lived cloud watch for `body`'s targets, on a session of the
/// call's own (`wait_…`), which the call listens on before the cloud makes
/// it, so one that fires at once isn't missed; cancelled unless it fired.
/// Err when the cloud made none: why.
async fn cloud_wait(c: &Call<'_>, mut body: WatchCreate, what: &str, limit: Duration) -> Result<CloudWaited, String> {
    (body.session, body.session_name, body.expires_in_ms) = wait_session(c, limit).await;
    let listening = Listening::new(c.app(), body.session.clone());
    let got = c.app().agent_mentions.wait_for(&listening.session);
    let body = serde_json::to_value(&body).map_err(|e| e.to_string())?;
    let v = c.app().control.cloud(Method::POST, AGENT_WATCHES, Some(&body)).await.map_err(said)?;
    let made: WatchMade =
        serde_json::from_value(v).map_err(|e| format!("Arugula cloud sent an answer I can't read: {e}"))?;
    let Some(id) = made.id.clone().filter(|_| made.state == WatchState::Live) else {
        return Ok(CloudWaited::Already(made));
    };
    *listening.watch.lock().unwrap() = Some(id.clone());
    Ok(match c.waiting(what, limit, got).await {
        Some(Ok(text)) => {
            listening.watch.lock().unwrap().take();
            CloudWaited::Fired(id, text)
        }
        Some(Err(_)) => CloudWaited::Lost(id),
        None => CloudWaited::NotYet(id),
    })
}

/// How long `wait` reads, at least, once its watch fired with its time
/// nearly up: what fired it is read from control at once.
pub(crate) const READ_AFTER: Duration = Duration::from_secs(10);

/// What `wait`'s run or session targets got from their watch.
pub(crate) enum Watched {
    /// It happened (fired, or already had): read it as the long-poll would.
    Happened,
    /// The call ended first: its watch is cancelled.
    NotYet,
    /// No watch to wait on (not joined, a hosted box, an agent a guest
    /// stands behind, or the cloud refused it): long-poll as before.
    Unwatched,
}

/// `wait` with `run` or `agent_session`, served underneath as `event` is:
/// a short-lived cloud watch on `targets` (any of them fires it), which the
/// cloud fires from its event log, so the call asks control nothing until
/// then (https://app.arugula.io/issues/iss_1545620a176cab44b21084c27493241e).
/// The caller reads what happened after, as it did, for the same answer.
pub(crate) async fn watched(c: &Call<'_>, targets: Vec<WatchTarget>, what: &str, limit: Duration) -> Watched {
    let joined = c.app().control.enrolled().is_some_and(|e| !e.saved.is_hosted());
    // More than a watch takes are long-polled.
    if !joined || targets.len() > TARGETS_MAX || c.owners_own("wait").await.is_err() {
        return Watched::Unwatched;
    }
    let body = WatchCreate { targets, ..WatchCreate::default() };
    match cloud_wait(c, body, what, limit).await {
        Ok(CloudWaited::Fired(..)) => Watched::Happened,
        Ok(CloudWaited::Already(made)) if made.state == WatchState::AlreadyMet => Watched::Happened,
        Ok(CloudWaited::NotYet(_)) => Watched::NotYet,
        // Refused, made none, or its message can't reach the call.
        Ok(CloudWaited::Already(_) | CloudWaited::Lost(_)) | Err(_) => Watched::Unwatched,
    }
}

/// A wait's cloud watch while its call listens: dropped, the call stops
/// listening, and a watch that didn't fire is cancelled.
struct Listening {
    app: std::sync::Arc<arugulad_base::server::App>,
    session: String,
    /// The watch, once made, until it fired.
    watch: std::sync::Mutex<Option<String>>,
}

impl Listening {
    fn new(app: &std::sync::Arc<arugulad_base::server::App>, session: String) -> Self {
        Listening { app: app.clone(), session, watch: Default::default() }
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        self.app.agent_mentions.unwait(&self.session);
        let Some(id) = self.watch.lock().unwrap().take() else { return };
        let (app, session) = (self.app.clone(), self.session.clone());
        arugulad_base::task::spawn(async move {
            let body = WatchCancel { session, id: Some(id), all: false };
            let body = serde_json::to_value(body).unwrap_or_default();
            // One that can't be cancelled expires soon ([`WAIT_SLACK_MS`]),
            // and its message reaches no one.
            let _ = app.control.cloud(Method::POST, AGENT_WATCHES_CANCEL, Some(&body)).await;
        });
    }
}

/// A wait's answer when what it waits for had already happened.
fn already(made: WatchMade, nothing: &str) -> Out {
    match made.state {
        WatchState::AlreadyMet => {
            let text = format!("It has already happened: {}.", made.met.as_deref().unwrap_or("met"));
            done(text, serde_json::json!({ "state": "already_met", "met": made.met }))
        }
        _ => Err(nothing.into()),
    }
}

/// A wait's answer when its watch fired: the watch's own message.
fn fired(id: &str, text: String) -> Out {
    done(text.clone(), serde_json::json!({ "state": "fired", "watch": id, "text": text }))
}

/// A wait's answer at its time: still running.
fn not_yet(event: &str, id: &str, waited: Duration) -> Out {
    let secs = waited.as_secs();
    done(
        format!("No {event} yet after {secs}s: still running. Its watch {id} is cancelled; call wait again."),
        serde_json::json!({ "state": "still running", "event": event }),
    )
}

/// What a create's answer says.
fn made_said(made: WatchMade, nothing: &str) -> Out {
    let text = match (made.state, &made.id) {
        (WatchState::AlreadyMet, _) => {
            format!("It has already happened: {}. No watch was made.", made.met.as_deref().unwrap_or("met"))
        }
        (_, Some(id)) => format!(
            "Watch {id} is live until {}. End your turn now: you'll get a follow-up when it fires, or when it ends without firing.",
            made.expires_ms.map(when).unwrap_or_else(|| "it expires".into())
        ),
        (_, None) => nothing.into(),
    };
    done(text, made)
}

async fn list(c: &Call<'_>, a: ListArgs) -> Out {
    let session = match a.all {
        true => None,
        false => Some(session(c).await?.0),
    };
    let mut got = WatchList { watches: c.app().local_watches.list(session.as_deref()) };
    // The cloud's, when this machine has joined it.
    let mut unread = None;
    if c.app().control.enrolled().is_some() {
        let path = match &session {
            None => AGENT_WATCHES.to_owned(),
            Some(s) => format!("{AGENT_WATCHES}?session={s}"),
        };
        let cloud = match c.app().control.cloud(Method::GET, &path, None).await {
            Ok(v) => serde_json::from_value::<WatchList>(v)
                .map_err(|e| format!("Arugula cloud sent an answer I can't read: {e}")),
            Err(e) => Err(said(e)),
        };
        match cloud {
            Ok(l) => got.watches.extend(l.watches),
            Err(e) if got.watches.is_empty() => return Err(e),
            Err(e) => unread = Some(e),
        }
    }
    got.watches.sort_by_key(|w| std::cmp::Reverse(w.created_ms));
    let lines: Vec<String> = got
        .watches
        .iter()
        .map(|w| {
            let whose =
                if a.all { format!(" ({})", w.session_name.as_deref().unwrap_or(&w.session)) } else { "".into() };
            format!("{}{whose}: {}, until {}", w.id, w.targets.join(" or "), when(w.expires_ms))
        })
        .collect();
    let mut text = match lines.len() {
        0 => "No live watches".to_owned(),
        n => format!("{n} live watch{}:\n{}", if n == 1 { "" } else { "es" }, lines.join("\n")),
    };
    if let Some(e) = unread {
        text.push_str(&format!("\n(Arugula cloud's watches couldn't be read: {e})"));
    }
    done(text, got)
}

/// `events`: the project's log after `after`, as the cloud lets this
/// machine's agents watch it (Arugula #1273).
async fn read_events(c: &Call<'_>, a: EventsArgs) -> Out {
    let types = match &a.types {
        None => vec![],
        Some(PrList::One(t)) => events::read_types([t.as_str()])?,
        Some(PrList::Many(ts)) => events::read_types(ts.iter().map(String::as_str))?,
    };
    if c.app().control.enrolled().is_none() {
        return Err(said(CloudRefused::NotJoined));
    }
    let project = c.current_project(a.project.as_deref()).await?;
    let v = c.app().control.cloud(Method::GET, &events_path(project, a.after, &types, a.limit), None).await;
    let v = v.map_err(|e| match e {
        CloudRefused::OldControl | CloudRefused::Said { status: 404 | 405, .. } => {
            "Arugula cloud doesn't serve the event log to agents yet".into()
        }
        e => said(e),
    })?;
    let page: EventsPage =
        serde_json::from_value(v).map_err(|e| format!("Arugula cloud sent an answer I can't read: {e}"))?;
    done(events::page_text(&page), page)
}

/// [`AGENT_EVENTS`] with its query, [`events::EventsQuery`].
fn events_path(project: String, after: u64, types: &[String], limit: Option<u32>) -> String {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    q.append_pair("project", &project).append_pair("after", &after.to_string());
    if !types.is_empty() {
        q.append_pair("types", &types.join(","));
    }
    if let Some(l) = limit {
        q.append_pair("limit", &l.clamp(1, events::READ_LIMIT_MAX).to_string());
    }
    format!("{AGENT_EVENTS}?{}", q.finish())
}

async fn cancel(c: &Call<'_>, a: CancelArgs) -> Out {
    let id = a.id.map(|i| i.trim().to_owned()).filter(|i| !i.is_empty());
    if id.is_none() && !a.all {
        return Err("cancel needs an id (wch_…), or all".into());
    }
    let (session, _) = session(c).await?;
    let watches = &c.app().local_watches;
    // One of this machine's own: the cloud doesn't know it.
    if let Some(i) = id.as_deref().filter(|i| watches.has(i)) {
        return cancelled(WatchCancelled { cancelled: watches.cancel(&session, Some(i)) });
    }
    let mut got = WatchCancelled { cancelled: if a.all { watches.cancel(&session, None) } else { vec![] } };
    // The cloud's, when this machine has joined it.
    let mut unreached = None;
    if c.app().control.enrolled().is_some() {
        let body = serde_json::to_value(WatchCancel { session, id, all: a.all }).map_err(|e| e.to_string())?;
        let cloud = match c.app().control.cloud(Method::POST, AGENT_WATCHES_CANCEL, Some(&body)).await {
            Ok(v) => serde_json::from_value::<WatchCancelled>(v)
                .map_err(|e| format!("Arugula cloud sent an answer I can't read: {e}")),
            Err(e) => Err(said(e)),
        };
        match cloud {
            Ok(c) => got.cancelled.extend(c.cancelled),
            Err(e) if got.cancelled.is_empty() => return Err(e),
            Err(e) => unreached = Some(e),
        }
    }
    let mut out = cancelled(got)?;
    if let Some(e) = unreached
        && let Some(Value::String(t)) = out.get_mut("summary")
    {
        t.push_str(&format!("\n(Arugula cloud's watches couldn't be cancelled: {e})"));
    }
    Ok(out)
}

/// What a cancel's answer says.
fn cancelled(got: WatchCancelled) -> Out {
    let text = match got.cancelled.as_slice() {
        [] => "No live watch of this session's was cancelled".to_owned(),
        ids => format!(
            "Cancelled {}. No message follows for {}.",
            ids.join(", "),
            if ids.len() == 1 { "it" } else { "them" }
        ),
    };
    done(text, got)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arugula_control_wire::watch::{EXPIRES_MAX_MS, NOTE_MAX, PrUntil};
    use serde_json::json;

    #[test]
    fn expires_reads_minutes_hours_and_days_up_to_a_week() {
        assert_eq!(expires("30m"), Ok(30 * 60_000));
        assert_eq!(expires("6h"), Ok(6 * 3_600_000));
        assert_eq!(expires(" 7d "), Ok(EXPIRES_MAX_MS));
        assert!(expires("8d").is_err());
        assert!(expires("0h").is_err());
        assert!(expires("soon").is_err());
        assert!(expires("6").is_err());
    }

    #[test]
    fn until_is_merged_unless_said() {
        assert_eq!(until(None), Ok(PrUntil::Merged));
        assert_eq!(until(Some("closed")), Ok(PrUntil::Closed));
        assert_eq!(until(Some("ready")), Ok(PrUntil::Ready));
        assert_eq!(until(Some("checks_failed")), Ok(PrUntil::ChecksFailed));
        assert!(until(Some("green")).is_err());
    }

    fn args(v: Value) -> CreateArgs {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn an_event_target_goes_in_the_watch() {
        let body = create_body(args(json!({
            "event": "issue.closed",
            "match": { "issue": "#12", "by": "ada" },
            "note": " ship the fix once it's closed ",
            "run": "run_1",
        })))
        .unwrap();
        assert_eq!(body.event.as_deref(), Some("issue.closed"));
        assert_eq!(body.note.as_deref(), Some("ship the fix once it's closed"));
        assert_eq!(body.targets, vec![WatchTarget::Run { id: "run_1".into() }]);
        let wire = serde_json::to_value(&body).unwrap();
        assert_eq!(wire["event"], json!("issue.closed"));
        assert_eq!(wire["match"], json!({ "issue": "#12", "by": "ada" }));
        assert_eq!(wire["note"], json!("ship the fix once it's closed"));
        // Alone, it's enough to watch.
        let alone = create_body(args(json!({ "event": "run.ended" }))).unwrap();
        assert!(alone.targets.is_empty() && alone.matches.is_none() && alone.note.is_none());
        // Without one, the body is what it was.
        let old = serde_json::to_value(create_body(args(json!({ "session": "ses_1" }))).unwrap()).unwrap();
        let keys: Vec<&str> = old.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["expires_in_ms", "session", "session_name", "targets"]);
    }

    #[test]
    fn an_event_target_is_checked_against_the_catalog() {
        let err = |v| create_body(args(v)).err().unwrap();
        assert_eq!(
            err(json!({ "event": "issue.renamed" })),
            "event issue.renamed: no such type; watch types lists them"
        );
        assert_eq!(
            err(json!({ "event": "issue.closed", "match": { "label": "bug" } })),
            "match label: issue.closed matches on issue, project, by"
        );
        assert_eq!(
            err(json!({ "event": "github.push", "match": { "pr": "1" } })),
            "match pr: github.push matches on repo, branch, by"
        );
        let release = json!({ "event": "github.release.published", "match": { "repo": "o/r", "tag": "v1.2.0" } });
        assert_eq!(create_body(args(release)).unwrap().event.as_deref(), Some("github.release.published"));
        let merged = json!({ "event": "github.pr.merged", "match": { "repo": "o/r", "outside": true } });
        assert_eq!(create_body(args(merged)).unwrap().event.as_deref(), Some("github.pr.merged"));
        assert!(
            create_body(args(json!({ "event": "issue.labeled", "match": { "issue": "#3", "label": "bug" } }))).is_ok()
        );
        assert!(err(json!({ "event": "issue.closed", "note": "x".repeat(NOTE_MAX + 1) })).contains("at most 200"));
        assert!(create_body(args(json!({ "event": "issue.closed", "note": "é".repeat(NOTE_MAX) }))).is_ok());
        assert_eq!(err(json!({ "run": "run_1", "note": "why" })), "match and note go with an event");
        assert!(err(json!({})).contains("or event"));
    }

    #[test]
    fn a_type_that_matches_on_project_wants_the_current_one() {
        let wants = |v| wants_project(&create_body(args(v)).unwrap());
        assert!(wants(json!({ "event": "issue.closed", "match": { "issue": "#12" } })));
        assert!(wants(json!({ "event": "run.ended" })));
        assert!(!wants(json!({ "event": "issue.closed", "match": { "issue": "#12", "project": "prj_2" } })));
        assert!(!wants(json!({ "event": "issue.closed", "match": { "issue": "iss_9" } })), "an id is anywhere");
        assert!(!wants(json!({ "event": "run.ended", "match": { "run": "run_1" } })), "an id is anywhere");
        assert!(!wants(json!({ "event": "chat.reply", "match": { "room": "r1" } })), "chat.reply has no project");
        assert!(!wants(json!({ "run": "run_1" })));
    }

    #[test]
    fn events_asks_for_a_projects_log_after_a_number() {
        assert_eq!(events_path("prj_1".into(), 0, &[], None), "/api/daemon/events?project=prj_1&after=0");
        let types = ["issue.closed".to_owned(), "run.ended".to_owned()];
        assert_eq!(
            events_path("prj_1".into(), 41, &types, Some(1000)),
            "/api/daemon/events?project=prj_1&after=41&types=issue.closed%2Crun.ended&limit=200"
        );
    }

    #[test]
    fn types_lists_the_catalog_and_what_this_machine_can_watch() {
        let joined = types(Watcher::Machine, true);
        assert_eq!(joined.lines().count(), 2 * events::CATALOG.len(), "{joined}");
        assert!(joined.contains("issue.closed: A native issue was closed. [watchable now]\n  match: issue (the issue, iss_… or its #N in the project); project (the project, prj_…); by (the login of who closed it)"), "{joined}");
        assert!(joined.contains("pane.exited: A pane's process exited. [watchable now, by this machine]"), "{joined}");
        for name in [
            "issue.labeled",
            "draft.answered",
            "machine.updated",
            "github.pr.checks_done",
            "github.push",
            "github.release.published",
        ] {
            assert!(joined.contains(&format!("\n{name}: ")), "{name}: {joined}");
        }
        assert!(joined.contains("outside (true for a fork's PR or a non-member's"), "{joined}");
        let alone = types(Watcher::Machine, false);
        assert!(
            alone.contains(
                "issue.closed: A native issue was closed. [not now: this machine hasn't joined Arugula cloud]"
            ),
            "{alone}"
        );
        // With no account, this machine's own types are still watchable.
        assert!(alone.contains("agent.idle: An agent on this machine finished its turn and waits for a prompt. [watchable now, by this machine]"), "{alone}");
        assert_eq!(alone.matches("watchable now").count(), 5, "{alone}");
    }

    #[test]
    fn a_local_event_is_this_machines_to_watch_on_a_pane() {
        let a =
            args(json!({ "event": " agent.idle ", "match": { "pane": "%4" }, "note": " ship it ", "expires": "6h" }));
        let on = local(&a).unwrap();
        let ask = local_ask(a, on).unwrap();
        assert_eq!((ask.on, ask.pane, ask.exit), (On::Idle, 4, None));
        assert_eq!(ask.note.as_deref(), Some("ship it"));
        assert_eq!(ask.expires_in_ms, Some(6 * 3_600_000));
        let exited = |m: Value| {
            let a = args(json!({ "event": "pane.exited", "match": m }));
            let on = local(&a).unwrap();
            local_ask(a, on)
        };
        let ask = exited(json!({ "pane": 7, "exit": "3" })).unwrap();
        assert_eq!((ask.on, ask.pane, ask.exit), (On::Exited, 7, Some(3)));
        assert!(exited(json!({ "exit": 0 })).unwrap_err().contains("needs match pane"));
        let output = |m: Value| {
            let a = args(json!({ "event": "pane.output.matched", "match": m }));
            let on = local(&a).unwrap();
            local_ask(a, on)
        };
        let ask = output(json!({ "pane": "%4", "pattern": "ERROR|panic" })).unwrap();
        assert_eq!((ask.on, ask.pattern.as_deref()), (On::Output, Some("ERROR|panic")));
        assert!(output(json!({ "pane": 4 })).unwrap_err().contains("needs match pattern"));
        assert!(
            output(json!({ "pane": 4, "pattern": "x", "exit": 1 })).unwrap_err().contains("matches on pane, pattern")
        );

        assert!(exited(json!({ "pane": "build" })).unwrap_err().starts_with("match pane: a pane's"));
        assert_eq!(
            exited(json!({ "pane": 7, "state": "x" })).unwrap_err(),
            "match state: pane.exited matches on pane, exit"
        );
        // On its own: a cloud target is another watch.
        let both = args(json!({ "event": "agent.idle", "match": { "pane": 4 }, "pr": "a/b#1" }));
        assert!(local_ask(both, On::Idle).unwrap_err().contains("make another watch"));
        // The cloud's types aren't the daemon's.
        assert!(local(&args(json!({ "event": "issue.closed" }))).is_none());
    }

    #[test]
    fn when_is_utc_to_the_minute() {
        assert_eq!(when(0), "1970-01-01T00:00Z");
        assert_eq!(when(1_794_652_920_000), "2026-11-14T10:42Z");
    }
}
