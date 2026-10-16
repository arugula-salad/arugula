//! The Managoat account view (#942, F2): the person's Managoat account as a
//! block. Its first tab is the account's conversations, every one of them
//! (started here, by the CLI, by a script or by another conversation), with
//! live status from the account stream, an unread dot, a label filter and a
//! fan-out's tree; its second the account's agents, read-only (chant's are
//! still edited in agent-specs), each with *Run on Managoat* and *Spec*. It
//! also holds the cards for the account's permission requests
//! ([`attention`]).
//!
//! **Config** `{tab?, labels?, limit?, profile?, filter?, specs?}`: the tab
//! (`conversations`, the default, or `agents`), the label filter
//! (`["key:value", …]`, combined with AND, as Managoat's `?label=`), how
//! many conversations to read ([`conversations::PAGE`] more each *More*), a
//! `managoat` CLI profile to use instead of the account's login
//! ([`login`]), the agents' filters ([`catalog::Filter`]) and the
//! agent-specs checkout *Spec* looks in. Never a key. A block saved as M43's
//! catalog has none of the first three, and opens as the account view.
//!
//! **The login** is the daemon's: [`account::Account::resolve`] (its own
//! sign-in, else `$MANAGOAT_API_KEY`, else the CLI's credentials), unless
//! the config names a CLI profile. The key is held in memory only.
//!
//! **Reads.** The conversations (`GET /api/conversations?limit=&label=`) and
//! the agents (`GET /api/agents`, environments' names from `GET
//! /api/environments`) are read when the block opens, then every
//! [`interval`] while a client draws it, and on `refresh`
//! (`ARUGULA_FOUNTAIN_POLL_MS` sets the interval). On the daemon that runs
//! the account stream, the list follows it instead of polling: a `turn` or
//! `wake` stage event changes a row's status in place, and a `conversations`
//! signal (the list changed) reads the list again, at most once every
//! [`relist`] while drawn and ten times that while not, since Managoat
//! allows a key 600 requests a minute and signals each second of output.
//!
//! **Actions:** `tab {tab}`; `labels {labels}` (the filter, kept) and
//! `more`; `open_conversation {id}` opens a conversation in an agent block
//! beside this one (an `agent: managoat` block on the native link, F3:
//! loading it once, following and never starting a new one) and marks it read;
//! `mark_read {id}` (`POST /api/conversations/:id/read`); `tree {id}` shows
//! its spawn tree (`GET /api/conversations/:id/tree`), `untree` puts it
//! away; `answer_request {id, allow, always?}` answers a permission request
//! (the card's Allow and Deny come here). The agents tab's: `run {agent}`
//! (*Run on Managoat*: a new conversation with agent NAME, in an agent block
//! beside this one); `spec {agent}` (the agent-specs file that declares a
//! `managed-by: chant` agent, as a file block; for any other, or with no
//! checkout, the agent as `GET /api/agents/{id}` returns it with literal
//! values redacted, shown in the block); `unspec`; `filter {...}`, `profile
//! {name}`, `specs {dir}`; `agents {query?, source?}` and `agent {name}`
//! read without changing anything.
//!
//! **Retired:** `run_here` (M44's *Run here*, a Claude Code here wearing the
//! agent; 2026-10-15) and `view: runner` (M45b, gone with Managoat's
//! runners): a block saved with either comes back as the account view,
//! serde ignoring the key.
//!
//! **The log** (`blocks/%N/`): what it was pointed at, and what was opened
//! or answered from it.

pub mod account;
pub mod api;
pub mod attention;
pub mod catalog;
pub mod client;
pub mod conversations;
pub mod link;
pub mod login;
pub mod recipes;
pub mod stream;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, LazyLock, Mutex, Weak},
    time::Duration,
};

use arugula_proto::{Attention, BlockType, ReasonKind, WorkKind, api::OpenRequest};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tracing::{info, warn};

use self::{
    api::{Agent, Client},
    attention::Request,
    catalog::{Card, Counts, Filter, Source},
    conversations::{Effect, Node, Row},
    login::Login,
    stream::Event,
};
use crate::{
    block::{Block, BlockCtx, Summary, no_method},
    review::{Live, Runner},
    store::now_ms,
};

/// How often a drawn view reads everything again.
pub fn interval() -> Duration {
    static AT: std::sync::OnceLock<Duration> = std::sync::OnceLock::new();
    *AT.get_or_init(|| {
        std::env::var("ARUGULA_FOUNTAIN_POLL_MS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .map(Duration::from_millis)
            .unwrap_or(POLL)
    })
}

/// A few minutes: agents change when someone applies agent-specs or an
/// app makes one, not by the second.
const POLL: Duration = Duration::from_secs(180);

/// Where *Spec* looks when the block hasn't been told, if it's there.
pub const DEFAULT_SPECS: &str = "~/agent-specs";

/// The last list read for an account, so MCP's tools don't read it again
/// within a poll: keyed by the host the login was read on, the base URL,
/// the profile and a hash of the key, so another host's account, or a new
/// login, never gets this one's list.
static LAST: LazyLock<Mutex<HashMap<String, Recalled>>> = LazyLock::new(Mutex::default);

/// When a list was read, the list, and how many rows didn't parse.
type Recalled = (u64, Arc<Vec<Agent>>, usize);

/// The account's agents, as read (or recalled).
pub(crate) struct Agents {
    pub login: Login,
    pub agents: Arc<Vec<Agent>>,
    pub unreadable: usize,
}

fn cache_key(runner: &Runner, login: &Login) -> String {
    use sha2::Digest;
    let host = match runner {
        Runner::Local { .. } => "local".to_owned(),
        Runner::Machine { sprite, .. } => format!("machine:{sprite}"),
    };
    let key = hex::encode(sha2::Sha256::digest(login.key.as_bytes()));
    format!("{host} {} {} {}", login.base_url, login.profile, &key[..16])
}

fn remember(runner: &Runner, login: &Login, agents: &Arc<Vec<Agent>>, unreadable: usize) {
    LAST.lock().unwrap().insert(cache_key(runner, login), (now_ms(), agents.clone(), unreadable));
}

fn recalled(runner: &Runner, login: &Login, within: Duration) -> Option<(Arc<Vec<Agent>>, usize)> {
    let (at, agents, unreadable) = LAST.lock().unwrap().get(&cache_key(runner, login)).cloned()?;
    (now_ms().saturating_sub(at) <= within.as_millis() as u64).then_some((agents, unreadable))
}

/// What one read gets.
struct Read {
    login: Login,
    agents: Arc<Vec<Agent>>,
    unreadable: usize,
}

/// Log in on the runner's host and read the account's agents (and its
/// environments' names).
async fn read_account(runner: &Runner, profile: Option<&str>) -> Result<Read, String> {
    let found = login::read(runner).await?;
    let login = login::resolve(&found, profile)?;
    let (agents, unreadable, _) = read_agents(runner, &login).await?;
    Ok(Read { login, agents, unreadable })
}

/// The account's agents and its environments' names, with `login`.
async fn read_agents(
    runner: &Runner,
    login: &Login,
) -> Result<(Arc<Vec<Agent>>, usize, BTreeMap<String, String>), String> {
    let client = Client::new(&login.base_url, &login.key);
    let (agents, envs) = tokio::join!(client.agents(), client.environments());
    let listing = agents.map_err(|e| e.to_string())?;
    let (mut agents, unreadable) = (listing.items, listing.unreadable);
    catalog::sort(&mut agents);
    let envs = match envs {
        Ok(e) => catalog::env_names(&e),
        Err(e) => {
            warn!(error = %e, "fountain environments");
            BTreeMap::new()
        }
    };
    let agents = Arc::new(agents);
    remember(runner, login, &agents, unreadable);
    Ok((agents, unreadable, envs))
}

/// The daemon's own host, as a block's [`Runner::user`] would be: for
/// MCP's tools, which have no block.
pub(crate) async fn local_runner(shell_env: &crate::shellenv::ShellEnv) -> Runner {
    let env: Vec<(String, String)> = std::env::vars().collect();
    let shell = shell_env.local().await;
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_else(|| "/".into());
    Runner::Local { env: crate::shellenv::merge(&env, &shell, None), home }
}

/// The account's agents for MCP (`list`'s Managoat kinds): the last
/// list read within a poll, else a fresh one.
pub(crate) async fn agents_for(runner: &Runner, profile: Option<&str>) -> Result<Agents, String> {
    let found = login::read(runner).await?;
    let login = login::resolve(&found, profile)?;
    if let Some((agents, unreadable)) = recalled(runner, &login, interval().min(POLL)) {
        return Ok(Agents { login, agents, unreadable });
    }
    let r = read_account(runner, profile).await?;
    Ok(Agents { login: r.login, agents: r.agents, unreadable: r.unreadable })
}

/// What a client or an agent is told when some rows didn't parse.
pub fn unreadable_note(n: usize) -> Option<String> {
    match n {
        0 => None,
        1 => Some("1 agent couldn't be read (Managoat sent something this Arugula doesn't understand)".into()),
        n => Some(format!("{n} agents couldn't be read (Managoat sent something this Arugula doesn't understand)")),
    }
}

/// An agent by name or id.
pub fn find<'a>(agents: &'a [Agent], which: &str) -> Option<&'a Agent> {
    let which = which.trim();
    agents.iter().find(|a| a.id == which).or_else(|| agents.iter().find(|a| a.name == which)).or_else(|| {
        let mut m = agents.iter().filter(|a| a.name.eq_ignore_ascii_case(which));
        let one = m.next()?;
        m.next().is_none().then_some(one)
    })
}

/// Compact rows for MCP's `fountain_agents`: the filter's agents as cards.
pub fn rows(agents: &[Agent], filter: &Filter) -> Vec<Card> {
    agents.iter().filter(|a| filter.matches(a)).map(|a| catalog::card(a, &BTreeMap::new())).collect()
}

/// M43: the person's Managoat agents, read with their own login on this
/// host (`GET /api/fountain/agents`, `arugula managoat agents`): compact
/// cards, filtered.
pub async fn agents_answer(
    app: &crate::server::App,
    q: arugula_proto::api::FountainQuery,
) -> Result<arugula_proto::api::FountainAgents, crate::api::ApiError> {
    use crate::api::bad;
    let mut f = catalog::Filter::default();
    f.apply(&json!({ "query": q.query, "source": q.source })).map_err(bad)?;
    let runner = local_runner(&app.mux.shell_env).await;
    let got = agents_for(&runner, q.profile.as_deref()).await.map_err(bad)?;
    let rows = rows(&got.agents, &f);
    Ok(arugula_proto::api::FountainAgents {
        base_url: got.login.base_url,
        profile: got.login.profile,
        total: got.agents.len(),
        filter: serde_json::to_value(&f).unwrap_or_default(),
        agents: rows.iter().map(|r| serde_json::to_value(r).unwrap_or_default()).collect(),
        unreadable: got.unreadable,
    })
}

/// MCP's `list` kind `fountain_conversations` (#942): the account's
/// conversations, read with the daemon's login, and their open permission
/// requests. Read-only.
pub async fn conversations_answer(
    labels: &[String],
    status: Option<String>,
    limit: Option<u32>,
) -> Result<(String, Value), String> {
    if crate::labs::DIRECT_MANAGOAT_RETIRED {
        return Err(crate::agent::defs::CLOUD_MIGRATION.into());
    }
    let labels = conversations::parse_labels(&json!(labels))?;
    let account = account::get().ok_or("Managoat isn't set up on this daemon yet")?;
    let login = account.resolve().await?;
    let limit = limit.unwrap_or(conversations::PAGE).clamp(1, conversations::MAX_LIMIT);
    let q = client::ConversationQuery {
        limit: Some(limit),
        labels: conversations::label_pairs(&labels),
        status: status.filter(|s| !s.trim().is_empty()),
        ..Default::default()
    };
    let l = login.client().conversations(&q).await.map_err(|e| e.to_string())?;
    let open = attention::requests().open();
    let mut rows = conversations::rows(&l.items, &[]);
    for r in &mut rows {
        r.asking = open.iter().filter(|q| q.conversation_id == r.id).count();
    }
    let filter = if labels.is_empty() { String::new() } else { format!(" ({})", labels.join(", ")) };
    let mut text = format!("{} conversations on {}{filter}:\n", rows.len(), login.base_url);
    for r in &rows {
        text.push_str(&conversations::line(r));
        text.push('\n');
    }
    if !open.is_empty() {
        text.push_str(&format!("{} waiting for the user's answer:\n", open.len()));
        for q in &open {
            text.push_str(&format!("  {}: {}\n", conversations::short(&q.conversation_id), q.title()));
        }
    }
    let requests: Vec<Value> = open
        .iter()
        .map(|q| {
            json!({ "id": q.id, "conversation_id": q.conversation_id, "title": q.title(), "tool": q.tool,
                    "expires_ms": q.expires_ms })
        })
        .collect();
    let v = json!({
        "base_url": login.base_url,
        "conversations": rows,
        "unreadable": l.unreadable,
        "requests": requests,
    });
    Ok((text, v))
}

/// The whole recipe for MCP's `fountain_agent` and the block's `agent`: as Managoat
/// returns it, a `${VAR}` left as it is, but an MCP header or env value
/// typed in literally (which may be a secret) as `<redacted>`; plus where
/// it comes from.
pub fn recipe(a: &Agent) -> Value {
    let mut v = serde_json::to_value(a.redacted()).unwrap_or_default();
    let (src, app) = catalog::source(a);
    v["source"] = json!(src);
    if let Some(app) = app {
        v["app"] = json!(app);
    }
    v
}

/// How long a drawn view waits after the account stream says the list
/// changed before it reads it again (ten times that while nobody draws it).
pub fn relist() -> Duration {
    static AT: std::sync::OnceLock<Duration> = std::sync::OnceLock::new();
    *AT.get_or_init(|| {
        std::env::var("ARUGULA_FOUNTAIN_RELIST_MS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_secs(2))
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Tab {
    #[default]
    Conversations,
    Agents,
}

impl Tab {
    fn is_default(&self) -> bool {
        *self == Tab::Conversations
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Config {
    #[serde(default, skip_serializing_if = "Tab::is_default")]
    tab: Tab,
    /// The conversations' label filter: `key:value`, all of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    labels: Vec<String>,
    /// How many conversations to read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    limit: Option<u32>,
    /// A `managoat` CLI profile, in place of the account's login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    // No `view`: a saved `view: runner` (M45b, before Managoat removed
    // runners) is ignored like any unknown key.
    #[serde(default, skip_serializing_if = "Filter::is_empty")]
    filter: Filter,
    /// The agent-specs checkout (`~/…` allowed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    specs: Option<String>,
    // No `here`: *Run here* (M44) was retired, and a saved one is ignored.
}

impl Config {
    fn limit(&self) -> u32 {
        self.limit.unwrap_or(conversations::PAGE).clamp(1, conversations::MAX_LIMIT)
    }
}

/// An open permission request as the block shows it.
#[derive(Debug, Clone, Serialize)]
struct Asking {
    #[serde(flatten)]
    request: Request,
    /// What it asks to do.
    title: String,
    /// The conversation's name.
    conversation: String,
    /// An agent block here that shows that conversation (it asks there).
    here: Option<arugula_proto::PaneId>,
}

#[derive(Debug, Clone, Serialize)]
struct LabelCount {
    label: String,
    count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct Tree {
    root: String,
    nodes: Vec<Node>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct State {
    tab: Tab,
    /// The CLI profile in use (none for this daemon's own sign-in), and the
    /// ones the credentials file has.
    profile: Option<String>,
    profiles: Vec<String>,
    base_url: Option<String>,
    /// Where the key came from: `arugula`, `env` or `file`.
    key_from: Option<&'static str>,
    loading: bool,
    /// Why the account (its agents) can't be read.
    error: Option<String>,

    /// The conversations, newest activity first, the filter's.
    conversations: Vec<Row>,
    conv_unreadable: usize,
    conv_error: Option<String>,
    labels: Vec<String>,
    label_counts: Vec<LabelCount>,
    limit: u32,
    /// The page is full: there may be more.
    more: bool,
    tree: Option<Tree>,
    /// Open permission requests, newest first, and the one on the card.
    requests: Vec<Asking>,
    card: Option<String>,
    /// This daemon follows the account stream (`--managoat-account-stream`)
    /// and is connected: the list is live.
    stream: bool,
    live: bool,
    relists: u64,

    /// The agents the filter shows, in the catalog's order.
    agents: Vec<Card>,
    /// How many there are in all.
    total: usize,
    /// Rows Managoat sent that didn't parse, and what to say about them.
    unreadable: usize,
    unreadable_note: Option<String>,
    filter: Filter,
    /// How many each chip would show.
    counts: Counts,
    /// The agent-specs checkout *Spec* uses, and when there's none, why.
    specs: Option<String>,
    specs_why: Option<String>,
    /// *Spec* for an agent agent-specs doesn't declare: `{agent, json,
    /// note}`, the agent as Managoat returns it (literal values redacted).
    spec: Option<Value>,
    updated_ms: u64,
    polls: u64,
    watching: bool,
    /// The last action's result, for the client.
    said: Option<String>,
}

pub struct FountainBlock {
    ctx: BlockCtx,
    me: Weak<FountainBlock>,
    config: Mutex<Config>,
    state: Mutex<State>,
    runner: tokio::sync::OnceCell<Result<Runner, String>>,
    /// The last lists read, whole (the state holds what's shown).
    agents: Mutex<Arc<Vec<Agent>>>,
    convs: Mutex<Vec<client::Conversation>>,
    envs: Mutex<BTreeMap<String, String>>,
    login: Mutex<Option<Login>>,
    /// When the list is read again (the stream said it changed).
    relist_at: Mutex<Option<tokio::time::Instant>>,
    /// The requests its card was raised for, and the one on it.
    raised: Mutex<BTreeSet<String>>,
    card: Mutex<Option<Asking>>,
    live: Live,
    wake: tokio::sync::Notify,
    reading: tokio::sync::Mutex<()>,
    listing: tokio::sync::Mutex<()>,
}

/// The next event off the account stream; a lag reads as "the list
/// changed", so the view reads it again. Never, without a stream.
async fn next_event(rx: &mut Option<broadcast::Receiver<Arc<Event>>>) -> Arc<Event> {
    let Some(r) = rx else { return std::future::pending().await };
    match r.recv().await {
        Ok(ev) => ev,
        Err(broadcast::error::RecvError::Lagged(_)) => {
            Arc::new(Event { id: 0, kind: "conversations".into(), conversation_id: None, data: Value::Null })
        }
        Err(broadcast::error::RecvError::Closed) => {
            *rx = None;
            std::future::pending().await
        }
    }
}

/// A reason for a card that says more (the newest request).
fn input_reason(headline: String) -> arugula_proto::Reason {
    arugula_proto::Reason {
        kind: ReasonKind::Input,
        since_ms: now_ms(),
        headline,
        command: None,
        exit: None,
        duration_ms: None,
        bundle: None,
        ask: None,
        gate: None,
        diff: None,
        actions: vec![arugula_proto::Action::Dismiss],
    }
}

impl FountainBlock {
    pub fn create(ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        if crate::labs::DIRECT_MANAGOAT_RETIRED && !ctx.restoring {
            return Err(crate::agent::defs::CLOUD_MIGRATION.into());
        }
        let config: Config = serde_json::from_value(config).map_err(|e| format!("fountain config: {e}"))?;
        let state = State {
            tab: config.tab,
            profile: config.profile.clone(),
            filter: config.filter.clone(),
            specs: config.specs.clone(),
            labels: config.labels.clone(),
            limit: config.limit(),
            loading: true,
            ..State::default()
        };
        crate::review::log(&ctx, &json!({ "e": "view", "view": "account", "profile": config.profile }));
        let b = Arc::new_cyclic(|me| Self {
            ctx,
            me: me.clone(),
            config: Mutex::new(config),
            state: Mutex::new(state),
            runner: tokio::sync::OnceCell::new(),
            agents: Mutex::new(Arc::default()),
            convs: Mutex::default(),
            envs: Mutex::new(BTreeMap::new()),
            login: Mutex::new(None),
            relist_at: Mutex::new(None),
            raised: Mutex::default(),
            card: Mutex::new(None),
            live: Live::default(),
            wake: tokio::sync::Notify::new(),
            reading: tokio::sync::Mutex::new(()),
            listing: tokio::sync::Mutex::new(()),
        });
        attention::requests().hold(b.ctx.id);
        let me = b.clone();
        b.ctx.rt.spawn(async move { me.run().await });
        Ok(b)
    }

    async fn runner(&self) -> Result<Runner, String> {
        self.runner.get_or_init(|| async { Runner::user(&self.ctx).await }).await.clone()
    }

    async fn run(self: Arc<Self>) {
        let mut events = account::get().map(|a| a.hub().subscribe_all());
        let reqs = attention::requests();
        let mut changes = reqs.subscribe();
        self.read().await;
        self.sync_requests().await;
        loop {
            tokio::select! {
                _ = sleep_for(self.next_wait()) => {}
                _ = self.wake.notified() => {}
                ev = next_event(&mut events) => self.on_event(&ev),
                r = changes.changed() => {
                    if r.is_ok() {
                        self.sync_requests().await;
                    }
                }
            }
            if self.live.closed() {
                break;
            }
            if self.relist_at.lock().unwrap().is_some_and(|t| t <= tokio::time::Instant::now()) {
                self.relist().await;
            }
            if let Some(every) = self.every()
                && self.age_ms() >= every.as_millis() as u64
            {
                self.read().await;
            }
        }
    }

    /// How often it reads everything now; `None`: not until it's drawn.
    fn every(&self) -> Option<Duration> {
        self.live.drawn().then(interval)
    }

    /// Until the next read: the poll's, or the list's after a signal.
    fn next_wait(&self) -> Option<Duration> {
        let poll = self.every().map(|e| e.saturating_sub(Duration::from_millis(self.age_ms())));
        let list = self.relist_at.lock().unwrap().map(|t| t.saturating_duration_since(tokio::time::Instant::now()));
        match (poll, list) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// How long since the last read (or try).
    fn age_ms(&self) -> u64 {
        now_ms().saturating_sub(self.state.lock().unwrap().updated_ms)
    }

    /// The account stream says something: a row's status, or read the list.
    fn on_event(&self, ev: &Event) {
        let effect = conversations::apply(&mut self.state.lock().unwrap().conversations, ev);
        match effect {
            Effect::None => {}
            Effect::Changed => self.ctx.changed(),
            Effect::Relist => {
                let d = if self.live.drawn() { relist() } else { relist() * 10 };
                let mut at = self.relist_at.lock().unwrap();
                if at.is_none() {
                    *at = Some(tokio::time::Instant::now() + d);
                }
            }
        }
    }

    /// The login: the account's, unless the config names a CLI profile;
    /// and the profiles the CLI's credentials have.
    async fn login_for(&self, runner: &Runner, profile: Option<&str>) -> (Result<Login, String>, Vec<String>) {
        let found = login::read(runner).await;
        let profiles = found.as_ref().map(login::profiles).unwrap_or_default();
        if profile.is_none()
            && let Some(a) = account::get()
        {
            let env_profile = found.as_ref().ok().map(|f| f.env_profile.trim().to_owned()).filter(|p| !p.is_empty());
            let r = a.resolve().await.map(|r| Login {
                profile: env_profile.unwrap_or_else(|| "default".into()),
                base_url: r.base_url,
                key: r.key,
                from: match r.source {
                    account::Source::Arugula => login::From::Arugula,
                    account::Source::Env => login::From::Env,
                    account::Source::File => login::From::File,
                },
            });
            return (r, profiles);
        }
        (found.and_then(|f| login::resolve(&f, profile)), profiles)
    }

    async fn read(&self) {
        let _one = self.reading.lock().await;
        if self.live.closed() {
            return;
        }
        let runner = match self.runner().await {
            Ok(r) => r,
            Err(e) => return self.fail(e),
        };
        let profile = self.config.lock().unwrap().profile.clone();
        let (login, profiles) = self.login_for(&runner, profile.as_deref()).await;
        let specs = self.specs_dir(&runner).await;
        let login = match login {
            Ok(l) => l,
            Err(e) => {
                {
                    let mut st = self.state.lock().unwrap();
                    st.profiles = profiles;
                    (st.specs, st.specs_why) = specs;
                }
                return self.fail(e);
            }
        };
        let (agents, convs) = tokio::join!(read_agents(&runner, &login), self.list(&login));
        {
            let mut st = self.state.lock().unwrap();
            st.profile = (login.from != login::From::Arugula).then(|| login.profile.clone());
            st.profiles = profiles;
            st.base_url = Some(login.base_url.clone());
            st.key_from = Some(match login.from {
                login::From::Arugula => "arugula",
                login::From::Env => "env",
                login::From::File => "file",
            });
            st.loading = false;
            st.polls += 1;
            st.updated_ms = now_ms();
            (st.specs, st.specs_why) = specs;
            st.error = agents.as_ref().err().cloned();
        }
        *self.login.lock().unwrap() = Some(login.clone());
        if let Ok((agents, unreadable, envs)) = agents {
            info!(pane = self.ctx.id, agents = agents.len(), base = login.base_url, "fountain account read");
            {
                let mut st = self.state.lock().unwrap();
                st.unreadable = unreadable;
                st.unreadable_note = unreadable_note(unreadable);
            }
            *self.envs.lock().unwrap() = envs;
            *self.agents.lock().unwrap() = agents;
            self.refilter();
        }
        self.take_list(convs);
        self.sync_requests().await;
    }

    /// The conversations, with the filter and the page.
    async fn list(&self, login: &Login) -> Result<api::Listing<client::Conversation>, String> {
        let (labels, limit) = {
            let c = self.config.lock().unwrap();
            (conversations::label_pairs(&c.labels), c.limit())
        };
        let q = client::ConversationQuery { limit: Some(limit), labels, ..Default::default() };
        client::Client::new(&login.base_url, &login.key).conversations(&q).await.map_err(|e| e.to_string())
    }

    /// The list again (the stream said it changed, or the filter did).
    async fn relist(&self) {
        let _one = self.listing.lock().await;
        *self.relist_at.lock().unwrap() = None;
        let login = self.login.lock().unwrap().clone();
        let Some(login) = login else { return self.read().await };
        let got = self.list(&login).await;
        self.state.lock().unwrap().relists += 1;
        self.take_list(got);
        self.sync_requests().await;
    }

    fn take_list(&self, got: Result<api::Listing<client::Conversation>, String>) {
        match got {
            Ok(l) => {
                {
                    let mut st = self.state.lock().unwrap();
                    st.conv_error = None;
                    st.conv_unreadable = l.unreadable;
                }
                *self.convs.lock().unwrap() = l.items;
            }
            Err(e) => {
                warn!(pane = self.ctx.id, error = %e, "fountain conversations");
                self.state.lock().unwrap().conv_error = Some(e);
            }
        }
        self.rebuild_rows();
    }

    /// The rows, from the list read, the agents' names and the requests.
    fn rebuild_rows(&self) {
        let convs = self.convs.lock().unwrap().clone();
        let agents = self.agents.lock().unwrap().clone();
        let (labels, limit) = {
            let c = self.config.lock().unwrap();
            (c.labels.clone(), c.limit())
        };
        let mut rows = conversations::rows(&convs, &agents);
        let mut st = self.state.lock().unwrap();
        for r in &mut rows {
            r.asking = st.requests.iter().filter(|a| a.request.conversation_id == r.id).count();
            // What the stream said since, until the list says otherwise.
            if let Some(old) = st.conversations.iter().find(|o| o.id == r.id)
                && old.status == r.status
            {
                r.live = old.live.clone();
            }
        }
        st.label_counts = conversations::label_counts(&rows, 24)
            .into_iter()
            .map(|(label, count)| LabelCount { label, count })
            .collect();
        st.more = rows.len() as u32 >= limit && limit < conversations::MAX_LIMIT;
        st.conversations = rows;
        st.labels = labels;
        st.limit = limit;
        st.live = account::get().is_some_and(|a| {
            let s = a.stream.lock().unwrap();
            s.enabled && s.connected
        });
        st.stream = account::get().is_some_and(|a| a.stream.lock().unwrap().enabled);
        drop(st);
        self.ctx.changed();
    }

    /// The requests as they are now: the list, and the card if this block
    /// holds them (a push for each new one).
    async fn sync_requests(&self) {
        let reqs = attention::requests();
        let open = reqs.open();
        let holder = reqs.holder() == Some(self.ctx.id);
        // An agent block here that follows the conversation asks itself.
        let mut here: HashMap<String, arugula_proto::PaneId> = HashMap::new();
        if !open.is_empty() {
            for (pane, config, _) in self.ctx.agents().await {
                if matches!(config["agent"].as_str(), Some("managoat" | "fountain"))
                    && let Some(sid) = config["session_id"].as_str()
                {
                    here.insert(sid.to_owned(), pane);
                }
            }
        }
        let asking: Vec<Asking> = {
            let st = self.state.lock().unwrap();
            open.into_iter()
                .map(|r| {
                    let conversation = st
                        .conversations
                        .iter()
                        .find(|c| c.id == r.conversation_id)
                        .map(conversations::name)
                        .unwrap_or_else(|| format!("conversation {}", conversations::short(&r.conversation_id)));
                    Asking { title: r.title(), conversation, here: here.get(&r.conversation_id).copied(), request: r }
                })
                .collect()
        };
        let mine: Vec<&Asking> = asking.iter().filter(|a| holder && a.here.is_none()).collect();
        let ids: BTreeSet<String> = mine.iter().map(|a| a.request.id.clone()).collect();
        let card = mine.first().map(|a| (*a).clone());
        let before = std::mem::replace(&mut *self.raised.lock().unwrap(), ids.clone());
        {
            let mut st = self.state.lock().unwrap();
            for r in &mut st.conversations {
                r.asking = asking.iter().filter(|a| a.request.conversation_id == r.id).count();
            }
            st.card = card.as_ref().map(|c| c.request.id.clone());
            st.requests = asking;
        }
        let headline = card.as_ref().map(|c| format!("{} wants to run {}", c.conversation, c.title));
        *self.card.lock().unwrap() = card;
        let new = ids.difference(&before).count();
        match headline {
            None if !before.is_empty() => self.ctx.attention(Attention::Idle, "answered"),
            None => {}
            Some(why) if new > 0 => {
                // A card already up for another: let it go, so this one
                // pushes too.
                if !before.is_empty() {
                    self.ctx.attention(Attention::Idle, "another request");
                }
                self.ctx.attention(Attention::NeedsInput, why);
            }
            Some(why) if ids != before => self.ctx.update_reason(input_reason(why)),
            Some(_) => {}
        }
        self.ctx.changed();
    }

    fn fail(&self, e: String) {
        {
            let mut st = self.state.lock().unwrap();
            st.loading = false;
            st.error = Some(e);
            st.updated_ms = now_ms();
        }
        self.ctx.changed();
    }

    /// The state's cards and counts, from the list and the filter.
    fn refilter(&self) {
        let filter = self.config.lock().unwrap().filter.clone();
        let agents = self.agents.lock().unwrap().clone();
        let envs = self.envs.lock().unwrap().clone();
        let cards: Vec<Card> = agents.iter().filter(|a| filter.matches(a)).map(|a| catalog::card(a, &envs)).collect();
        {
            let mut st = self.state.lock().unwrap();
            st.total = agents.len();
            st.counts = catalog::counts(&agents);
            st.agents = cards;
            st.filter = filter;
        }
        self.rebuild_rows();
    }

    /// The agent-specs checkout: the configured one, else the default if
    /// it's there; and if neither, why.
    async fn specs_dir(&self, runner: &Runner) -> (Option<String>, Option<String>) {
        let given = self.config.lock().unwrap().specs.clone();
        let dir = given.clone().unwrap_or_else(|| DEFAULT_SPECS.to_owned());
        let (out, _) = runner.sh(SPECS_DIR, std::slice::from_ref(&dir)).await.unwrap_or_default();
        let out = String::from_utf8_lossy(&out);
        match out.trim().strip_prefix("ok ") {
            Some(real) => (Some(real.to_owned()), None),
            None if given.is_some() => (None, Some(format!("{dir} has no src/agents: pick the agent-specs checkout"))),
            None => (None, Some("where's your agent-specs checkout? pick it to open specs from it".into())),
        }
    }

    fn agent(&self, which: &str) -> Result<Agent, String> {
        let agents = self.agents.lock().unwrap().clone();
        if agents.is_empty() {
            let e = self.state.lock().unwrap().error.clone();
            return Err(e.unwrap_or_else(|| "the agents aren't read yet".into()));
        }
        find(&agents, which).cloned().ok_or_else(|| format!("no agent {which:?} on this Managoat account"))
    }

    fn said(&self, s: String) {
        self.state.lock().unwrap().said = Some(s);
        self.ctx.changed();
    }

    fn client(&self) -> Result<client::Client, String> {
        let login = self.login.lock().unwrap().clone().ok_or("not signed in to Managoat yet")?;
        Ok(client::Client::new(&login.base_url, &login.key))
    }

    /// A conversation, from the list or read.
    async fn conversation(&self, id: &str) -> Result<client::Conversation, String> {
        let listed = self.convs.lock().unwrap().iter().find(|c| c.id == id).cloned();
        match listed {
            Some(c) => Ok(c),
            None => self.client()?.conversation(id).await.map_err(|e| e.to_string()),
        }
    }

    /// Open a conversation: today's `agent: managoat` block beside this
    /// one, loading it and following it (never a new one).
    async fn open_conversation(&self, args: Value) -> Result<Value, String> {
        let id = args["id"].as_str().filter(|a| !a.is_empty()).ok_or("open_conversation needs {\"id\": ID}")?;
        let c = self.conversation(id).await?;
        let agent = c.agent_id.clone().filter(|a| !a.is_empty()).ok_or("this conversation names no agent")?;
        let mut config = json!({ "agent": "managoat", "managoat_agent": agent, "session_id": c.id, "follow": true });
        if let Some(t) = c.title.as_deref().filter(|t| !t.trim().is_empty()) {
            config["title"] = json!(t);
        }
        if let Some(p) = self.config.lock().unwrap().profile.clone() {
            config["profile"] = json!(p);
        }
        let block = self.ctx.open(self.beside(BlockType::Agent, config)).await?;
        crate::review::log(&self.ctx, &json!({ "e": "open", "conversation": c.id, "block": block }));
        if c.unread
            && let Err(e) = self.mark_read(&c.id).await
        {
            info!(conversation = c.id, error = %e, "marking it read");
        }
        let name = self
            .state
            .lock()
            .unwrap()
            .conversations
            .iter()
            .find(|r| r.id == c.id)
            .map(conversations::name)
            .unwrap_or_else(|| conversations::short(&c.id));
        self.said(format!("{name} opened in %{block}"));
        Ok(json!({ "block": block, "conversation_id": c.id }))
    }

    async fn mark_read(&self, id: &str) -> Result<(), String> {
        self.client()?.mark_read(id).await.map_err(|e| e.to_string())?;
        {
            let mut st = self.state.lock().unwrap();
            if let Some(r) = st.conversations.iter_mut().find(|r| r.id == id) {
                r.unread = false;
            }
        }
        if let Some(c) = self.convs.lock().unwrap().iter_mut().find(|c| c.id == id) {
            c.unread = false;
        }
        self.ctx.changed();
        Ok(())
    }

    async fn tree(&self, id: &str) -> Result<Value, String> {
        let nodes = self.client()?.tree(id).await.map_err(|e| e.to_string())?;
        let nodes = {
            let st = self.state.lock().unwrap();
            conversations::tree(&nodes.items, &st.conversations)
        };
        let out = json!({ "root": id, "nodes": nodes });
        self.state.lock().unwrap().tree = Some(Tree { root: id.to_owned(), nodes });
        self.ctx.changed();
        Ok(out)
    }

    /// Allow or deny a permission request (the card's, or any open one).
    async fn answer_request(&self, args: Value) -> Result<Value, String> {
        let id = match args["id"].as_str().filter(|i| !i.is_empty()) {
            Some(i) => i.to_owned(),
            None => self.card.lock().unwrap().as_ref().map(|c| c.request.id.clone()).ok_or("no request is open")?,
        };
        let allow = args["allow"].as_bool().ok_or("answer_request needs {\"id\": ID, \"allow\": true|false}")?;
        let always = args["always"].as_bool().unwrap_or(false);
        let account = account::get().ok_or("Managoat isn't set up yet")?;
        let title = attention::requests().get(&id).map(|r| r.title());
        let out = attention::requests().answer(&account, &id, allow, always).await;
        let how = if allow { if always { "allowed always" } else { "allowed" } } else { "denied" };
        crate::review::log(&self.ctx, &json!({ "e": "answer", "request": id, "how": how, "ok": out.is_ok() }));
        match &out {
            Ok(_) => self.said(format!("{how}: {}", title.unwrap_or_default())),
            Err(e) => self.said(e.clone()),
        }
        out
    }

    /// *Run on Managoat*: today's agent block, beside the view.
    async fn run_fountain(&self, args: Value) -> Result<Value, String> {
        let which = args["agent"].as_str().filter(|a| !a.is_empty()).ok_or("run needs {\"agent\": NAME}")?;
        let a = self.agent(which)?;
        let mut config = json!({ "agent": "managoat", "managoat_agent": a.name });
        if let Some(p) = self.config.lock().unwrap().profile.clone() {
            config["profile"] = json!(p);
        }
        if let Some(p) = args["prompt"].as_str().filter(|p| !p.is_empty()) {
            config["prompt"] = json!(p);
        }
        let block = self.ctx.open(self.beside(BlockType::Agent, config)).await?;
        crate::review::log(&self.ctx, &json!({ "e": "run", "agent": a.name, "block": block }));
        self.said(format!("{} runs on Managoat in %{block}", a.name));
        Ok(json!({ "block": block, "agent": a.name }))
    }

    /// *Spec*: the file that declares a chant-managed agent, else the agent
    /// as Managoat returns it, shown in the block (the console's agent
    /// pages are gone: managoat/managoat#2732).
    async fn spec(&self, args: Value) -> Result<Value, String> {
        let which = args["agent"].as_str().filter(|a| !a.is_empty()).ok_or("spec needs {\"agent\": NAME}")?;
        let a = self.agent(which)?;
        let chant = catalog::source(&a).0 == Source::AgentSpecs;
        let runner = self.runner().await?;
        let (dir, why) = self.specs_dir(&runner).await;
        {
            let mut st = self.state.lock().unwrap();
            (st.specs, st.specs_why) = (dir.clone(), why.clone());
        }
        let mut note = None;
        if chant {
            match &dir {
                Some(d) => {
                    let (out, _) = runner.sh(SPEC_FILE, &[d.clone(), a.name.clone()]).await?;
                    let out = String::from_utf8_lossy(&out);
                    if let Some((path, line)) = out.lines().find_map(|l| l.strip_prefix("ok ")?.rsplit_once(' ')) {
                        let line: u32 = line.parse().unwrap_or(1);
                        let block =
                            self.ctx.open(self.beside(BlockType::File, json!({ "path": path, "line": line }))).await?;
                        crate::review::log(&self.ctx, &json!({ "e": "spec", "agent": a.name, "path": path }));
                        self.said(format!("{}'s spec: {path}", a.name));
                        return Ok(json!({ "block": block, "kind": "file", "path": path, "line": line }));
                    }
                    note = Some(format!("no `name: \"{}\"` under {d}/src/agents", a.name));
                }
                None => note = why,
            }
        }
        // Read again, in case it changed since the list; the list's if
        // that fails.
        let login = self.login.lock().unwrap().clone().ok_or("not logged in")?;
        let a = match Client::new(&login.base_url, &login.key).agent(&a.id).await {
            Ok(fresh) => fresh,
            Err(e) => {
                info!(agent = a.name, error = %e, "reading the agent again for Spec; using the list's");
                a
            }
        };
        let shown = recipe(&a);
        crate::review::log(&self.ctx, &json!({ "e": "spec", "agent": a.name, "id": a.id }));
        self.state.lock().unwrap().spec = Some(json!({ "agent": a.name, "json": shown, "note": note }));
        self.said(match &note {
            Some(n) => format!("{}: {n}; showing it as Managoat has it", a.name),
            None => format!("{} as Managoat has it", a.name),
        });
        Ok(json!({ "kind": "json", "agent": a.name, "json": shown, "note": note }))
    }

    fn beside(&self, kind: BlockType, config: Value) -> OpenRequest {
        OpenRequest {
            kind,
            config,
            session: None,
            split: Some(self.ctx.id),
            from_pane: Some(self.ctx.id),
            vm: false,
            image: None,
            host: None,
            local: self.ctx.sprite.is_none(),
        }
    }
}

async fn sleep_for(d: Option<Duration>) {
    match d {
        Some(d) => tokio::time::sleep(d).await,
        None => std::future::pending().await,
    }
}

/// `$1` a directory (`~/…` allowed): `ok REAL` if it has `src/agents`.
const SPECS_DIR: &str = r#"dir=$1
case $dir in "~") dir=$HOME ;; "~/"*) dir=$HOME/${dir#"~/"} ;; esac
[ -d "$dir/src/agents" ] && cd -- "$dir" 2>/dev/null && echo "ok $(pwd -P)""#;

/// `$1` the checkout, `$2` the agent's name: `ok PATH LINE` for the first
/// `.ts` file under `src/agents` that declares `name: "$2"`.
///
/// A grep, not a chant read (#305): agent-specs is a chant project, not a
/// workspace, so no contract document names the file that declares an
/// agent, and building it to ask would cost seconds for a file to open.
const SPEC_FILE: &str = r#"cd -- "$1" 2>/dev/null || exit 0
grep -rnF --include='*.ts' -e "name: \"$2\"" -e "name: '$2'" -e "name:\"$2\"" src/agents 2>/dev/null | head -n 1 | {
  IFS=: read -r f n _ && [ -n "$f" ] && echo "ok $(pwd -P)/$f $n"
}"#;

impl Block for FountainBlock {
    fn kind(&self) -> BlockType {
        BlockType::Fountain
    }

    fn config(&self) -> Value {
        serde_json::to_value(&*self.config.lock().unwrap()).unwrap_or_default()
    }

    fn state(&self) -> Value {
        let mut v = serde_json::to_value(&*self.state.lock().unwrap()).unwrap_or_default();
        v["watching"] = self.live.drawn().into();
        v
    }

    fn text(&self) -> String {
        let st = self.state.lock().unwrap();
        let mut out = String::from("Managoat");
        if let Some(b) = &st.base_url {
            out.push_str(&format!(" on {b}"));
        }
        if let Some(p) = st.profile.as_deref().filter(|p| *p != "default") {
            out.push_str(&format!(" (profile {p})"));
        }
        out.push('\n');
        if let Some(e) = &st.error {
            out.push_str(&format!("{e}\n"));
        }
        if st.loading {
            out.push_str("reading…\n");
            return out;
        }
        if !st.requests.is_empty() {
            out.push_str(&format!("{} waiting for an answer:\n", st.requests.len()));
            for a in &st.requests {
                out.push_str(&format!("  {} wants to run {} ({})\n", a.conversation, a.title, a.request.id));
            }
        }
        let filter = if st.labels.is_empty() { String::new() } else { format!(" ({})", st.labels.join(", ")) };
        match &st.conv_error {
            Some(e) => out.push_str(&format!("Conversations{filter}: {e}\n")),
            None => {
                let live = if st.live { ", live" } else { "" };
                out.push_str(&format!("{} conversations{filter}{live}\n", st.conversations.len()));
                for r in &st.conversations {
                    out.push_str(&format!("  {}\n", conversations::line(r)));
                }
            }
        }
        if let Some(t) = &st.tree {
            out.push_str(&format!("Tree of {}:\n", conversations::short(&t.root)));
            for n in &t.nodes {
                let name = n.title.clone().unwrap_or_else(|| conversations::short(&n.id));
                let status = n.status.as_deref().unwrap_or("");
                out.push_str(&format!("  {}{name} [{status}]\n", "  ".repeat(n.depth)));
            }
        }
        if let Some(n) = &st.unreadable_note {
            out.push_str(&format!("{n}\n"));
        }
        let f = st.filter.describe();
        if f.is_empty() {
            out.push_str(&format!("{} agents\n", st.total));
        } else {
            out.push_str(&format!("{} of {} agents ({f})\n", st.agents.len(), st.total));
        }
        for c in &st.agents {
            out.push_str(&format!("  {}\n", catalog::line(c)));
        }
        out
    }

    fn call(&self, method: &str, args: Value) -> BoxFuture<'static, Result<Value, String>> {
        let me = self.me.upgrade();
        match method {
            "refresh" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                me.read().await;
                let st = me.state.lock().unwrap();
                match &st.error {
                    Some(e) => Err(e.clone()),
                    None => Ok(json!({ "total": st.total, "shown": st.agents.len(),
                                       "conversations": st.conversations.len() })),
                }
            }),
            "tab" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let tab: Tab = serde_json::from_value(args["tab"].clone())
                    .map_err(|_| "tab: conversations or agents".to_owned())?;
                me.config.lock().unwrap().tab = tab;
                me.state.lock().unwrap().tab = tab;
                me.ctx.changed();
                Ok(json!({ "tab": tab }))
            }),
            "labels" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let labels = conversations::parse_labels(&args["labels"])?;
                {
                    let mut c = me.config.lock().unwrap();
                    c.labels = labels.clone();
                    c.limit = None;
                }
                me.relist().await;
                let st = me.state.lock().unwrap();
                match &st.conv_error {
                    Some(e) => Err(e.clone()),
                    None => Ok(json!({ "labels": labels, "shown": st.conversations.len() })),
                }
            }),
            "more" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                {
                    let mut c = me.config.lock().unwrap();
                    c.limit = Some((c.limit() + conversations::PAGE).min(conversations::MAX_LIMIT));
                }
                me.relist().await;
                let st = me.state.lock().unwrap();
                Ok(json!({ "limit": st.limit, "shown": st.conversations.len() }))
            }),
            "open_conversation" => Box::pin(async move { me.ok_or("closed")?.open_conversation(args).await }),
            "mark_read" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let id = args["id"].as_str().ok_or("mark_read needs {\"id\": ID}")?;
                me.mark_read(id).await?;
                Ok(json!({ "read": id }))
            }),
            "tree" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let id = args["id"].as_str().ok_or("tree needs {\"id\": ID}")?;
                me.tree(id).await
            }),
            "untree" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                me.state.lock().unwrap().tree = None;
                me.ctx.changed();
                Ok(json!({}))
            }),
            "answer_request" => Box::pin(async move { me.ok_or("closed")?.answer_request(args).await }),
            "requests" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let st = me.state.lock().unwrap();
                Ok(json!({ "requests": st.requests, "card": st.card }))
            }),
            "filter" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                me.config.lock().unwrap().filter.apply(&args)?;
                me.refilter();
                let st = me.state.lock().unwrap();
                Ok(json!({ "filter": st.filter, "shown": st.agents.len(), "total": st.total }))
            }),
            "profile" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let name = args["name"].as_str().filter(|n| !n.is_empty()).map(str::to_owned);
                me.config.lock().unwrap().profile = name.clone();
                me.state.lock().unwrap().loading = true;
                me.ctx.changed();
                me.read().await;
                let st = me.state.lock().unwrap();
                match &st.error {
                    Some(e) => Err(e.clone()),
                    None => Ok(json!({ "profile": st.profile, "total": st.total })),
                }
            }),
            "specs" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let dir = args["dir"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_owned);
                me.config.lock().unwrap().specs = dir;
                let runner = me.runner().await?;
                let (dir, why) = me.specs_dir(&runner).await;
                {
                    let mut st = me.state.lock().unwrap();
                    (st.specs, st.specs_why) = (dir.clone(), why.clone());
                }
                me.ctx.changed();
                match (dir, why) {
                    (Some(d), _) => Ok(json!({ "specs": d })),
                    (None, why) => Err(why.unwrap_or_default()),
                }
            }),
            "run" | "run_fountain" => Box::pin(async move { me.ok_or("closed")?.run_fountain(args).await }),
            "spec" => Box::pin(async move { me.ok_or("closed")?.spec(args).await }),
            "unspec" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                me.state.lock().unwrap().spec = None;
                me.ctx.changed();
                Ok(json!({}))
            }),
            "agents" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let mut f = Filter::default();
                f.apply(&args)?;
                let agents = me.agents.lock().unwrap().clone();
                let rows = rows(&agents, &f);
                Ok(json!({ "total": agents.len(), "agents": rows }))
            }),
            "agent" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let which = args["name"].as_str().or(args["agent"].as_str()).ok_or("agent needs {\"name\": NAME}")?;
                Ok(recipe(&me.agent(which)?))
            }),
            "state" => {
                let s = self.state();
                Box::pin(async move { Ok(s) })
            }
            m => {
                let e = no_method(BlockType::Fountain, m);
                Box::pin(async move { Err(e) })
            }
        }
    }

    fn drawn(&self, on: bool) {
        self.live.set(on);
        if on && let Some(at) = self.relist_at.lock().unwrap().as_mut() {
            // Drawn again: what changed meanwhile is read now.
            *at = (*at).min(tokio::time::Instant::now() + relist());
        }
        self.wake.notify_one();
        self.ctx.changed();
    }

    fn close(&self) {
        self.live.close();
        attention::requests().release(self.ctx.id);
        self.wake.notify_one();
    }

    fn push_extra(&self) -> Option<Value> {
        let c = self.card.lock().unwrap().clone()?;
        Some(json!({ "approve": { "id": c.request.id, "title": format!("{}: {}", c.conversation, c.title) } }))
    }

    fn waiting(&self) -> Option<crate::block::Waiting> {
        let c = self.card.lock().unwrap().clone()?;
        Some(crate::block::Waiting {
            id: c.request.id.clone(),
            what: arugula_proto::AskWhat::Approve,
            headline: format!("{} wants to run {}", c.conversation, c.title),
            agent: "Managoat".into(),
            cwd: None,
            at_ms: c.request.at_ms,
            questions: None,
        })
    }

    fn summary(&self) -> Summary {
        let st = self.state.lock().unwrap();
        let title = if st.loading {
            "Managoat".to_owned()
        } else if !st.requests.is_empty() {
            format!("Managoat ({} asking)", st.requests.len())
        } else {
            let unread = st.conversations.iter().filter(|c| c.unread).count();
            match unread {
                0 => format!("Managoat ({} conversations)", st.conversations.len()),
                n => format!("Managoat ({} conversations, {n} unread)", st.conversations.len()),
            }
        };
        Summary { work: Some(WorkKind::Fountain), title: Some(title), ..Summary::default() }
    }
}
