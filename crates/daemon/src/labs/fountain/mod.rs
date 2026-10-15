//! The Fountain catalog: the person's Fountain account as a block, every
//! agent as a read-only card, with where it comes from, filters, and *Run
//! on Fountain*. (#942 turns this block into the account view; the agents
//! list stays in it, read-only: chant's agents are still edited in
//! agent-specs.)
//!
//! **Config** `{profile?, filter?, specs?}`: the credentials profile
//! ([`login`]), the filters ([`catalog::Filter`]: search, source, runtime, provider), and
//! the agent-specs checkout *Spec* looks in. Never a key.
//!
//! **Reads** go through Fountain's HTTP API ([`api`]) with the key of the
//! person's own CLI login, held in memory only. The list is read when the
//! block opens, then every [`interval`] while a client draws it (none while
//! nobody does; drawn again, it reads if the last read is that old), and on
//! `refresh`. `ARUGULA_FOUNTAIN_POLL_MS` sets the interval (tests).
//! Environment names come from `GET /api/environments`. The catalog is
//! read-only: agent-specs stays the one place a curated agent is edited.
//!
//! **Actions:** `run {agent}` (*Run on Fountain*: an agent block running
//! `fountain acp --agent NAME`, beside this one); `spec {agent}` (the
//! agent-specs file that declares a `managed-by: chant` agent, as a file
//! block; for any other, or with no checkout, the agent as Fountain returns
//! it, `GET /api/agents/{id}` with literal values redacted, shown in the
//! block: Fountain's console has no agent pages any more); `unspec` puts
//! that away. `filter {...}` changes the filters, `profile {name}` the
//! login, `specs {dir}` the checkout. `agents {query?, source?}` and `agent
//! {name}` read without changing anything (MCP's `list` kinds
//! `fountain_agents` and `fountain_agent`).
//!
//! **`run_here`** (*Run here*, M44: a Claude Code here wearing the agent)
//! was retired on 2026-10-15 (#942): recipes are what runs here. A block
//! saved with `here` (where it last ran one) comes back without it: serde
//! ignores the key.
//!
//! **`view: runner`** (M45b, this host as the account's Fountain runner)
//! is gone: Fountain removed self-hosted runners on 2026-10-14 (its ADR
//! 0071). A block saved with it comes back as the catalog: `Config` has no
//! `view`, and serde ignores the key.
//!
//! **The log** (`blocks/%N/`): what it was pointed at, and what was opened
//! from it.

pub mod account;
pub mod api;
pub mod catalog;
pub mod client;
pub mod login;
pub mod stream;

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, LazyLock, Mutex, Weak},
    time::Duration,
};

use arugula_proto::{BlockType, WorkKind, api::OpenRequest};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};

use self::{
    api::{Agent, Client},
    catalog::{Card, Counts, Filter, Source},
    login::Login,
};
use crate::{
    block::{Block, BlockCtx, Summary, no_method},
    review::{Live, Runner},
    store::now_ms,
};

/// How often a drawn catalog reads the list again.
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    // No `view`: the catalog is the only one. A saved `view: runner` (M45b,
    // before Fountain removed runners) is ignored like any unknown key.
    #[serde(default, skip_serializing_if = "Filter::is_empty")]
    filter: Filter,
    /// The agent-specs checkout (`~/…` allowed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    specs: Option<String>,
    // No `here`: *Run here* (M44) was retired, and a saved one is ignored.
}

#[derive(Debug, Clone, Default, Serialize)]
struct State {
    /// The profile in use, and the ones the credentials file has.
    profile: Option<String>,
    profiles: Vec<String>,
    base_url: Option<String>,
    /// Where the key came from: `env` or `file`.
    key_from: Option<&'static str>,
    loading: bool,
    error: Option<String>,
    /// The agents the filter shows, in the catalog's order.
    agents: Vec<Card>,
    /// How many there are in all.
    total: usize,
    /// Rows Fountain sent that didn't parse, and what to say about them.
    unreadable: usize,
    unreadable_note: Option<String>,
    filter: Filter,
    /// How many each chip would show.
    counts: Counts,
    /// The agent-specs checkout *Spec* uses, and when there's none, why.
    specs: Option<String>,
    specs_why: Option<String>,
    /// *Spec* for an agent agent-specs doesn't declare: `{agent, json,
    /// note}`, the agent as Fountain returns it (literal values redacted).
    spec: Option<Value>,
    updated_ms: u64,
    polls: u64,
    watching: bool,
    /// The last action's result, for the client.
    said: Option<String>,
}

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
    profiles: Vec<String>,
    agents: Arc<Vec<Agent>>,
    unreadable: usize,
    envs: BTreeMap<String, String>,
}

/// Log in on the runner's host and read the account's agents (and its
/// environments' names).
async fn read_account(runner: &Runner, profile: Option<&str>) -> Result<Read, String> {
    let found = login::read(runner).await?;
    let profiles = login::profiles(&found);
    let login = login::resolve(&found, profile)?;
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
    remember(runner, &login, &agents, unreadable);
    Ok(Read { login, profiles, agents, unreadable, envs })
}

/// The daemon's own host, as a block's [`Runner::user`] would be: for
/// MCP's tools, which have no block.
pub(crate) async fn local_runner(shell_env: &crate::shellenv::ShellEnv) -> Runner {
    let env: Vec<(String, String)> = std::env::vars().collect();
    let shell = shell_env.local().await;
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_else(|| "/".into());
    Runner::Local { env: crate::shellenv::merge(&env, &shell, None), home }
}

/// The account's agents for MCP (`list`'s Fountain kinds): the last
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
        1 => Some("1 agent couldn't be read (Fountain sent something this Arugula doesn't understand)".into()),
        n => Some(format!("{n} agents couldn't be read (Fountain sent something this Arugula doesn't understand)")),
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

/// M43: the person's Fountain agents, read with their own login on this
/// host (`GET /api/fountain/agents`, `arugula fountain agents`): compact
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

/// The whole recipe for MCP's `fountain_agent` and the block's `agent`: as Fountain
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

pub struct FountainBlock {
    ctx: BlockCtx,
    me: Weak<FountainBlock>,
    config: Mutex<Config>,
    state: Mutex<State>,
    runner: tokio::sync::OnceCell<Result<Runner, String>>,
    /// The last list read, whole (the state holds the filter's cards).
    agents: Mutex<Arc<Vec<Agent>>>,
    envs: Mutex<BTreeMap<String, String>>,
    login: Mutex<Option<Login>>,
    live: Live,
    wake: tokio::sync::Notify,
    reading: tokio::sync::Mutex<()>,
}

impl FountainBlock {
    pub fn create(ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        let config: Config = serde_json::from_value(config).map_err(|e| format!("fountain config: {e}"))?;
        let state = State {
            profile: config.profile.clone(),
            filter: config.filter.clone(),
            specs: config.specs.clone(),
            loading: true,
            ..State::default()
        };
        crate::review::log(&ctx, &json!({ "e": "view", "view": "catalog", "profile": config.profile }));
        let b = Arc::new_cyclic(|me| Self {
            ctx,
            me: me.clone(),
            config: Mutex::new(config),
            state: Mutex::new(state),
            runner: tokio::sync::OnceCell::new(),
            agents: Mutex::new(Arc::default()),
            envs: Mutex::new(BTreeMap::new()),
            login: Mutex::new(None),
            live: Live::default(),
            wake: tokio::sync::Notify::new(),
            reading: tokio::sync::Mutex::new(()),
        });
        let me = b.clone();
        b.ctx.rt.spawn(async move { me.run().await });
        Ok(b)
    }

    async fn runner(&self) -> Result<Runner, String> {
        self.runner.get_or_init(|| async { Runner::user(&self.ctx).await }).await.clone()
    }

    async fn run(self: Arc<Self>) {
        self.read().await;
        loop {
            // Every interval while drawn, else until it's drawn.
            match self.next_wait() {
                Some(left) => tokio::select! {
                    _ = tokio::time::sleep(left) => {}
                    _ = self.wake.notified() => {}
                },
                None => self.wake.notified().await,
            }
            if self.live.closed() {
                break;
            }
            if let Some(every) = self.every()
                && self.age_ms() >= every.as_millis() as u64
            {
                self.read().await;
            }
        }
    }

    /// How often it reads now; `None`: not until it's drawn.
    fn every(&self) -> Option<Duration> {
        self.live.drawn().then(interval)
    }

    fn next_wait(&self) -> Option<Duration> {
        Some(self.every()?.saturating_sub(Duration::from_millis(self.age_ms())))
    }

    /// How long since the last read (or try).
    fn age_ms(&self) -> u64 {
        now_ms().saturating_sub(self.state.lock().unwrap().updated_ms)
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
        let read = read_account(&runner, profile.as_deref()).await;
        let specs = self.specs_dir(&runner).await;
        match read {
            Ok(r) => {
                info!(pane = self.ctx.id, agents = r.agents.len(), base = r.login.base_url, "fountain catalog read");
                {
                    let mut st = self.state.lock().unwrap();
                    st.profile = Some(r.login.profile.clone());
                    st.profiles = r.profiles;
                    st.base_url = Some(r.login.base_url.clone());
                    st.unreadable = r.unreadable;
                    st.unreadable_note = unreadable_note(r.unreadable);
                    st.key_from = Some(match r.login.from {
                        login::From::Env => "env",
                        login::From::File => "file",
                    });
                    st.loading = false;
                    st.error = None;
                    st.polls += 1;
                    st.updated_ms = now_ms();
                    (st.specs, st.specs_why) = specs;
                }
                *self.login.lock().unwrap() = Some(r.login);
                *self.envs.lock().unwrap() = r.envs;
                *self.agents.lock().unwrap() = r.agents;
                self.refilter();
            }
            Err(e) => {
                // The profiles to pick from, even when the one asked for
                // isn't there.
                let profiles = login::read(&runner).await.map(|f| login::profiles(&f)).unwrap_or_default();
                {
                    let mut st = self.state.lock().unwrap();
                    st.profiles = profiles;
                    (st.specs, st.specs_why) = specs;
                }
                self.fail(e)
            }
        }
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
        self.ctx.changed();
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
        find(&agents, which).cloned().ok_or_else(|| format!("no agent {which:?} on this Fountain account"))
    }

    fn said(&self, s: String) {
        self.state.lock().unwrap().said = Some(s);
        self.ctx.changed();
    }

    /// *Run on Fountain*: today's agent block, beside the catalog.
    async fn run_fountain(&self, args: Value) -> Result<Value, String> {
        let which = args["agent"].as_str().filter(|a| !a.is_empty()).ok_or("run needs {\"agent\": NAME}")?;
        let a = self.agent(which)?;
        let mut config = json!({ "agent": "fountain", "fountain_agent": a.name });
        if let Some(p) = self.config.lock().unwrap().profile.clone() {
            config["profile"] = json!(p);
        }
        if let Some(p) = args["prompt"].as_str().filter(|p| !p.is_empty()) {
            config["prompt"] = json!(p);
        }
        let block = self.ctx.open(self.beside(BlockType::Agent, config)).await?;
        crate::review::log(&self.ctx, &json!({ "e": "run", "agent": a.name, "block": block }));
        self.said(format!("{} runs on Fountain in %{block}", a.name));
        Ok(json!({ "block": block, "agent": a.name }))
    }

    /// *Spec*: the file that declares a chant-managed agent, else the agent
    /// as Fountain returns it, shown in the block (the console's agent
    /// pages are gone: managoat/fountain#2732).
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
            Some(n) => format!("{}: {n}; showing it as Fountain has it", a.name),
            None => format!("{} as Fountain has it", a.name),
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
        let mut out = String::from("Fountain agents");
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
        if let Some(n) = &st.unreadable_note {
            out.push_str(&format!("{n}\n"));
        }
        if st.loading {
            out.push_str("reading…\n");
            return out;
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
                    None => Ok(json!({ "total": st.total, "shown": st.agents.len() })),
                }
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
        self.wake.notify_one();
        self.ctx.changed();
    }

    fn close(&self) {
        self.live.close();
        self.wake.notify_one();
    }

    fn summary(&self) -> Summary {
        let st = self.state.lock().unwrap();
        let title = if st.loading {
            "Fountain agents".to_owned()
        } else if st.agents.len() == st.total {
            format!("Fountain agents ({})", st.total)
        } else {
            format!("Fountain agents ({} of {})", st.agents.len(), st.total)
        };
        Summary { work: Some(WorkKind::Fountain), title: Some(title), ..Summary::default() }
    }
}
