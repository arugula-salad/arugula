//! M34: a chant workspace as a block (S21's spike, finished).
//!
//! Config `{root, env}` (env defaults to `local`: the environment whose
//! gates and releases `status` reads). It reads the workspace through
//! chant's read contract on the block's host, with the workspace's own
//! chant ([`model`]): `ls`, `check`, `records` and `status`, in one `sh -c`.
//!
//! **Freshness.** A full read costs about 7.5 CPU-seconds, so it runs on
//! open, on `refresh` and after `approve`, and otherwise only when a cheap
//! git fingerprint changes ([`model::FINGERPRINT`]) and then holds still
//! for one more poll, so a burst of edits (a `chant run`, an agent at work
//! in a member) costs one read, not one per poll. A move of
//! `chant/lifecycle` (a gate reached, a release) reads at once ([`next`]).
//! The fingerprint is
//! looked at every [`POLL`] while some client draws the block (and on this
//! host the lifecycle ref alone every [`REF_POLL`] in between, so a gate
//! shows about a second and a read after `chant run` exits), and every
//! [`IDLE_POLL`] when none does and the workspace is on this host, so a gate
//! reached while nobody looks still reaches the swarm and push. Nothing
//! here fetches.
//!
//! **A VM's, while nobody draws it (#313)**: every [`VM_IDLE_POLL`], and
//! only while its provider says it's running (asking doesn't wake it; a
//! stopped, sleeping, gone or unreachable VM isn't looked at, and one whose
//! fingerprint can't be had isn't read). Each look is one provider status
//! call and one exec of the fingerprint (about 0.01 CPU-s on the VM). An
//! exec may count as activity to the provider and keep the VM from
//! sleeping, so once the workspace has held still for [`VM_QUIET`] looks it
//! looks only every [`VM_RESTING`]th, until it changes or the VM sleeps and
//! wakes again ([`vm_look`]).
//!
//! **Gates.** A gate waiting in any member is attention: `needs_input`
//! with a `gate` reason made from the [`Gate`] ([`crate::gate::reason`]).
//! `approve` runs `status`'s own `chant approve` line in the member's
//! directory (#302: `--plan` binds it to the plan read, `--sign` for a
//! signed gate), `--actor` the principal of the person who asked (#75: the
//! owner or an editor; guests can't call it). Config `actor` is the owner's
//! principal and `principals` maps editors' Arugula names to theirs
//! (ws-080: `github:<login>` or a signer); an editor's approval is
//! `--relayed-by` the owner, and a signed gate is the owner's to approve
//! here. It's logged, with who, in the block's log and its history.
//!
//! **What a gate enforces (#617).** A gate's card names the decisions
//! covering its member, from chant's intent graph over the member's
//! directory (`graph --intent <dir>`, [`model::INTENT`]): chant does the
//! `member:` and `path:` covering and follows supersession, so the block
//! joins nothing. That read runs `git log` over the member (about a second
//! on a small repository, 40 s and more on chant's own), so it runs only
//! for a member with a gate waiting, after the full read and apart from it,
//! one member at a time. It's kept until `HEAD` or the lifecycle ref moves
//! (an edit in the tree doesn't), and while nobody draws the block it runs
//! once per gate raised. The gate shows at once, its decisions when they
//! come, and the last ones read stay on it while the next read runs. The
//! attention names the gates by headline and keys only: a read landing
//! updates the card while the attention lasts, and asks nothing again, so
//! a gate dismissed stays dismissed. The plan digest and the member's last
//! release come from `status`.
//!
//! **Why a member is the way it is (#618).** `why {member, open}` opens a
//! member card (or closes it): while some client draws the block, an open
//! card's member gets the same intent read, and the run ledger (`chant
//! workspace runs`, about the last two weeks) is read once for all of
//! them, each kept until `HEAD` or the lifecycle ref moves. A card is open
//! until a client closes it or no client draws the block; a client still
//! showing it asks again. The card shows the decisions covering the member,
//! how many commits no decision covers (`intent-commit-undecided`, a tag:
//! only gates are attention), its recent runs (its declared agent
//! sessions', and those chant's walk joined to its commits) and the leases
//! on it (`status`'s `leases`, in its ledger or on a work item covering
//! it). An Arugula run's id names the pane that ran it; a run links to
//! that pane when the agent block there recorded it, and a lease to the
//! pane of the run under its token. Those joins are listed in
//! DECISIONS.md. `hud {url}` keeps hud's
//! address, where a proposed decision is reviewed.
//!
//! **Decision points (#621).** The full read lists the questions open for
//! people (`points --open`, chant 0.91 and newer), and each is a gate with
//! a `point` source on the same attention: `answer {key, answer}` runs
//! `chant workspace points answer <id> --answer <value> --by <principal>`
//! in the root, with the same principals and `--relayed-by` as `approve`,
//! and chant writes the answer record. An op gate that asks a point
//! (chant#3170) is that question, answered the same way, not with `chant
//! approve`. An agent's question to its people stays Arugula's (ws-091):
//! these are only the workspace's points records.
//!
//! `expire` (same arguments, same people, logged the same way) turns a gate
//! down (#310): `chant approve <op> <gate> --expire` clears its pending fact
//! without approving it, so the next run stops there again.
//!
//! `principals {actor, principals}` sets either (the owner's: it says who
//! chant records); kept in the config like `env`, and in the state so the
//! block can show them. An `actor` of `""` or null clears it; `principals`
//! replaces the map.
//!
//! **Envs (#312).** A block watches one env, and says which; `env {name}`
//! switches it (kept in the config) and reads again. The state's `envs`
//! are the ones chant has written releases for on `chant/lifecycle`, from
//! one `git ls-tree` per full read ([`ENVS`]), plus `local` and the one
//! watched, for the block's menu. Gates in the other envs aren't read:
//! each env is another `status`, another chant process (about 2-3
//! CPU-seconds) on every full read, for every block, VM or not.
//!
//! **The graph (#620).** `graph` starts behold on the workspace and frames
//! it through the block's site ([`graph`]); a settled read tells behold.
//!
//! Methods: `refresh`, `approve {member, op, gate}` (or `{key}`; the first
//! gate if none), `expire` (the same), `answer {key, answer}`, `member
//! {name}` (its directory, for opening panes there), `env {name}`,
//! `principals {actor, principals}`, `why {member, open}`, `hud {url}`,
//! `graph`, `state`.

mod graph;
mod model;
pub mod why;

pub use graph::stop_all as stop_beholds;

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use arugula_proto::api::HistoryKind;
use arugula_proto::{Attention, BlockType, Gate, GateSource, ReasonKind};
use futures_util::future::BoxFuture;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    block::{Block, BlockCtx, Summary, no_method},
    review::{Live, Runner, log},
    store::{Event, now_ms},
};

/// How often the fingerprint is looked at while drawn (a full read only
/// when it changes).
const POLL: Duration = Duration::from_secs(3);
/// While drawn, on this host: how often just the `chant/lifecycle` ref is
/// looked at ([`model::LIFECYCLE`], one `git rev-parse`) between full
/// fingerprints, so a gate waits on this, not [`POLL`]. Not on a VM, where
/// each look is an exec through its provider.
const REF_POLL: Duration = Duration::from_secs(1);
/// ...and while nobody draws it, for a workspace on this host.
const IDLE_POLL: Duration = Duration::from_secs(5);
/// ...and for one on a VM, while the VM runs.
const VM_IDLE_POLL: Duration = Duration::from_secs(30);
/// Looks at a VM's workspace with nothing changed (10 minutes) before it's
/// looked at less often...
const VM_QUIET: u32 = 20;
/// ...every this many (5 minutes).
const VM_RESTING: u32 = 10;

/// `sh -c ENVS sh ROOT`: the release ledgers on `chant/lifecycle`, one
/// path a line (`ENV/releases.jsonl`, or `_members/M/ENV/releases.jsonl`).
/// Nothing if there's no such ref yet.
const ENVS: &str = r#"cd "$1" 2>/dev/null || exit 0
git ls-tree -r --name-only refs/heads/chant/lifecycle 2>/dev/null | grep '/releases\.jsonl$'"#;

/// The envs [`ENVS`] names, with `local` and the one watched, sorted.
fn envs(ledgers: &str, watched: &str) -> Vec<String> {
    let mut out: Vec<String> = ledgers
        .lines()
        .filter_map(|l| l.trim().strip_suffix("/releases.jsonl"))
        .map(|d| d.rsplit('/').next().unwrap_or(d))
        .filter(|e| !e.is_empty() && !e.starts_with('_'))
        .map(str::to_owned)
        .chain(["local".to_owned(), watched.to_owned()])
        .collect();
    out.sort();
    out.dedup();
    out
}

/// hud's address as given: an http(s) URL with a host, and no query or
/// fragment (the block adds `/decisions#<id>`), without a trailing slash;
/// empty clears it.
fn hud_url(url: &str) -> Result<Option<String>, String> {
    let given = url.trim();
    if given.is_empty() {
        return Ok(None);
    }
    let bad = || format!("{given:?} isn't hud's address (http://host or https://host, maybe with a path)");
    let u = url::Url::parse(given).map_err(|_| bad())?;
    if !matches!(u.scheme(), "http" | "https")
        || u.host_str().is_none_or(str::is_empty)
        || u.query().is_some()
        || u.fragment().is_some()
        || !u.username().is_empty()
        || u.password().is_some()
    {
        return Err(bad());
    }
    Ok(Some(u.as_str().trim_end_matches('/').to_owned()))
}

/// An env name `status` can take as its argument.
fn env_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.starts_with('-') || name.contains(|c: char| c.is_whitespace() || c == '/') {
        return Err(format!("{name:?} isn't an environment's name"));
    }
    Ok(name.to_owned())
}

#[derive(Debug, Clone, Deserialize)]
struct Config {
    root: String,
    #[serde(default = "local")]
    env: String,
    /// The owner's chant principal: chant runs here, and signs, as them.
    #[serde(default)]
    actor: Option<String>,
    /// Arugula names (an editor's login) to chant principals.
    #[serde(default)]
    principals: std::collections::BTreeMap<String, String>,
    /// Where hud reviews this workspace's records (#618): its address, for
    /// a proposed decision's link (`<hud>/decisions#<id>`).
    #[serde(default)]
    hud: Option<String>,
}

fn local() -> String {
    "local".into()
}

/// Who chant records: the owner's principal and editors' (ws-080).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Principals {
    actor: Option<String>,
    principals: std::collections::BTreeMap<String, String>,
}

/// A chant principal as given: trimmed, one word (it's an argument to
/// `--actor`).
fn principal_name(p: &str) -> Result<String, String> {
    let p = p.trim();
    if p.is_empty() || p.starts_with('-') || p.contains(char::is_whitespace) {
        return Err(format!("{p:?} isn't a chant principal (github:<login>, or a signer's name)"));
    }
    Ok(p.to_owned())
}

/// `who` with what `principals {actor, principals}` asks for: a key left
/// out keeps what was there; an `actor` of `""` or null clears it, and
/// `principals` replaces the map (an empty principal drops its name).
fn set_principals(who: &Principals, args: &Value) -> Result<Principals, String> {
    let mut out = who.clone();
    if let Some(a) = args.get("actor") {
        out.actor = match a {
            Value::Null => None,
            Value::String(s) if s.trim().is_empty() => None,
            Value::String(s) => Some(principal_name(s)?),
            _ => return Err("actor is a chant principal (a string), or null to clear it".into()),
        };
    }
    if let Some(m) = args.get("principals") {
        let m =
            m.as_object().ok_or("principals maps Arugula names to chant principals: {\"name\": \"github:login\"}")?;
        out.principals.clear();
        for (name, p) in m {
            let name = name.trim();
            let p = p.as_str().ok_or_else(|| format!("{name}'s principal isn't a string"))?;
            if name.is_empty() || p.trim().is_empty() {
                continue;
            }
            out.principals.insert(name.to_owned(), principal_name(p)?);
        }
    }
    Ok(out)
}

pub struct Workspace {
    ctx: BlockCtx,
    me: Weak<Workspace>,
    config: Config,
    /// The env watched: `config.env` at open, then what `env` chose.
    env: Mutex<String>,
    /// The envs chant knows ([`envs`]), at the last read.
    envs: Mutex<Vec<String>>,
    /// `config.actor` and `config.principals` at open, then what
    /// `principals` set.
    who: Mutex<Principals>,
    runner: tokio::sync::OnceCell<Result<Runner, String>>,
    state: Mutex<model::State>,
    /// The fingerprint at the last read.
    seen: Mutex<Option<String>>,
    /// A changed fingerprint the last poll saw, waiting to hold still.
    pending: Mutex<Option<String>>,
    /// The gates attention last asked for ([`raised_key`]), so it's asked
    /// once per change of gate. Starts as `Some("")` so the first read
    /// clears any left from before a restart.
    raised: Mutex<Option<String>>,
    /// The intent reads (#617), the run ledger and the cards open (#618).
    why: Mutex<Why>,
    /// `config.hud` at open, then what `hud` set.
    hud: Mutex<Option<String>>,
    /// One intent read at a time.
    intent_reading: tokio::sync::Mutex<()>,
    live: Live,
    reading: tokio::sync::Mutex<()>,
    /// behold, once someone asked for the graph.
    graph: Arc<graph::Graph>,
}

impl Workspace {
    pub fn create(ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        let mut config: Config = serde_json::from_value(config).map_err(|e| format!("workspace config: {e}"))?;
        if config.root.is_empty() {
            return Err("a workspace block needs a root".into());
        }
        config.hud = hud_url(config.hud.as_deref().unwrap_or_default())?;
        let who =
            set_principals(&Principals::default(), &json!({ "actor": config.actor, "principals": config.principals }))?;
        (config.actor, config.principals) = (who.actor, who.principals);
        if let Some(rest) = config.root.strip_prefix("~/").filter(|_| ctx.sprite.is_none()) {
            config.root = ctx.home.join(rest).display().to_string();
        }
        let state =
            model::State { root: config.root.clone(), env: config.env.clone(), loading: true, ..Default::default() };
        log(&ctx, &json!({ "e": "view", "root": config.root, "env": config.env }));
        let w = Arc::new_cyclic(|me| Self {
            ctx,
            me: me.clone(),
            env: Mutex::new(config.env.clone()),
            envs: Mutex::new(envs("", &config.env)),
            who: Mutex::new(Principals { actor: config.actor.clone(), principals: config.principals.clone() }),
            hud: Mutex::new(config.hud.clone()),
            config,
            runner: tokio::sync::OnceCell::new(),
            state: Mutex::new(state),
            seen: Mutex::new(None),
            pending: Mutex::new(None),
            raised: Mutex::new(Some(String::new())),
            why: Mutex::default(),
            intent_reading: tokio::sync::Mutex::new(()),
            live: Live::default(),
            reading: tokio::sync::Mutex::new(()),
            graph: Arc::default(),
        });
        let me = w.clone();
        w.ctx.rt.spawn(async move {
            me.load().await;
            me.idle().await;
        });
        Ok(w)
    }

    async fn runner(&self) -> Result<Runner, String> {
        // The user's shell environment (#74): their node and chant, as a
        // pane would find them.
        self.runner.get_or_init(|| Runner::user(&self.ctx)).await.clone()
    }

    fn local(&self) -> bool {
        self.ctx.sprite.is_none()
    }

    /// A full read, and attention for what it found.
    async fn load(&self) {
        let _one = self.reading.lock().await;
        if self.live.closed() {
            return;
        }
        self.state.lock().unwrap().loading = true;
        self.ctx.changed();
        let print = self.fingerprint().await;
        *self.seen.lock().unwrap() = print;
        let env = self.env.lock().unwrap().clone();
        let mut st = match self.runner().await {
            Err(e) => model::State { error: Some(e), ..Default::default() },
            Ok(r) => {
                if let Ok((out, _)) = r.sh(ENVS, std::slice::from_ref(&self.config.root)).await {
                    *self.envs.lock().unwrap() = envs(&String::from_utf8_lossy(&out), &env);
                }
                let args = [self.config.root.clone(), model::READER.to_owned(), env.clone()];
                match r.sh(model::SCRIPT, &args).await {
                    Err(e) => model::State { error: Some(e), ..Default::default() },
                    Ok((out, _)) => match serde_json::from_slice::<Value>(&out) {
                        Ok(raw) => model::compose(&raw, &env),
                        Err(e) => model::State {
                            error: Some(format!("the reader said something else: {e}")),
                            ..Default::default()
                        },
                    },
                }
            }
        };
        if st.root.is_empty() {
            st.root = self.config.root.clone();
        }
        if st.headline.is_none() {
            st.headline = st.error.clone();
        }
        st.env = env;
        st.updated_ms = now_ms();
        for g in &mut st.gates {
            if let GateSource::Chant { machine, .. } | GateSource::Point { machine, .. } = &mut g.source {
                machine.clone_from(&self.ctx.sprite);
            }
        }
        // Attached and swapped in under the state's lock, so a read that
        // lands meanwhile (`changed_why`, which takes it too) isn't lost.
        let (gates, wanted) = {
            let mut cur = self.state.lock().unwrap();
            self.attach(&mut st);
            *cur = st;
            (cur.gates.clone(), !self.wanted(&cur).is_empty())
        };
        self.raise(&gates);
        self.ctx.changed();
        if wanted {
            self.read_why();
        }
    }

    /// What's wanted and not had ([`wanted`]): open cards' only while some
    /// client draws the block, so a card left open in a closed tab costs
    /// nothing.
    fn wanted(&self, st: &model::State) -> Wanted {
        let seen = self.seen.lock().unwrap().clone();
        wanted(st, &self.why.lock().unwrap(), intent_key(seen.as_deref()), self.live.drawn())
    }

    /// What the reads had last, onto the gates and open cards.
    fn attach(&self, st: &mut model::State) {
        attach(st, &self.why.lock().unwrap());
    }

    /// Reads what's wanted, apart from the full read.
    fn read_why(&self) {
        if let Some(me) = self.me.upgrade() {
            self.ctx.rt.spawn(async move { me.read_intents().await });
        }
    }

    /// Reads what [`Workspace::wanted`] names, one read at a time: the run
    /// ledger first (one quick read for every open card), then each
    /// member's intent. What each says goes on the state as it lands (and
    /// on the gate's attention).
    async fn read_intents(&self) {
        let _one = self.intent_reading.lock().await;
        loop {
            let (wanted, chant, version, root) = {
                let st = self.state.lock().unwrap();
                (self.wanted(&st), st.chant.clone(), st.version.clone(), st.root.clone())
            };
            if self.live.closed() {
                return;
            }
            let print = intent_key(self.seen.lock().unwrap().as_deref()).map(str::to_owned);
            if wanted.runs {
                let got = match &chant {
                    None => Err(model::NO_CHANT.to_owned()),
                    Some(chant) => self.ledger(&root, chant).await,
                };
                self.why.lock().unwrap().ledger = Some(Known { print, got, gates: vec![] });
                self.changed_why();
                continue;
            }
            let Some((name, dir)) = wanted.members.into_iter().next() else { return };
            let got = match (chant, version) {
                (None, _) => Err(model::NO_CHANT.to_owned()),
                (_, Some(v)) if !model::at_least(&v, model::INTENT_FLOOR) => Err(format!(
                    "chant {v} can't say which decisions cover a member: {} or newer can",
                    model::INTENT_FLOOR
                )),
                (Some(chant), _) => self.intent(&root, &chant, &dir).await,
            };
            if let Err(e) = &got {
                log(&self.ctx, &json!({ "e": "intent", "member": name, "error": e }));
            } else if let Ok(i) = &got {
                log(&self.ctx, &json!({ "e": "intent", "member": name, "ms": i.ms, "decisions": i.decisions.len() }));
            }
            let gates = self.state.lock().unwrap().gates.iter().filter(|g| g.member == name).map(Gate::key).collect();
            self.why.lock().unwrap().intents.insert(name, Known { print, got, gates });
            self.changed_why();
        }
    }

    /// A read landed: onto the state, the gate's attention and the clients.
    fn changed_why(&self) {
        let gates = {
            let mut st = self.state.lock().unwrap();
            self.attach(&mut st);
            st.gates.clone()
        };
        self.raise(&gates);
        self.ctx.changed();
        self.graph.notify(&self.ctx, self.seen.lock().unwrap().clone().as_deref());
    }

    /// The run ledger: `chant workspace runs --json` in the root. A run's
    /// pane is named only when the agent block in the pane its id names
    /// (`arugula-<pane>-<ms>`) wrote it: an id is only a string in the
    /// ledger, and another daemon numbers its panes too.
    async fn ledger(&self, root: &str, chant: &str) -> Result<Vec<arugula_proto::workspace::RunRef>, String> {
        let r = self.runner().await?;
        let (out, _) = r.sh(model::RUNS, &[root.to_owned(), chant.to_owned()]).await?;
        let mut runs = model::runs(&serde_json::from_slice::<Value>(&out).unwrap_or(Value::Null))?;
        for run in &mut runs {
            let Some(pane) = arugula_proto::workspace::RunRef::pane_of(&run.id) else { continue };
            if self.ctx.block(pane).await.is_some_and(|b| b.wrote_run(&run.id)) {
                run.pane = Some(pane);
            }
        }
        Ok(runs)
    }

    /// One member's intent read: `graph --intent <dir>` in the root.
    async fn intent(&self, root: &str, chant: &str, dir: &str) -> Result<model::Intent, String> {
        let r = self.runner().await?;
        let t = std::time::Instant::now();
        let args = [root.to_owned(), chant.to_owned(), dir.to_owned()];
        // The host's limit on a command (30 s) cuts off a long one.
        let (out, _) = r.sh(model::INTENT, &args).await.map_err(|e| {
            if e.contains("too long") { "chant took too long to read them (over 30 s)".to_owned() } else { e }
        })?;
        let ms = t.elapsed().as_millis() as u64;
        let doc = serde_json::from_slice::<Value>(&out).unwrap_or(Value::Null);
        model::intent(&doc, ms)
    }

    /// Gates waiting are attention; none, and it's let go ([`raise`]).
    fn raise(&self, gates: &[Gate]) {
        let mut raised = self.raised.lock().unwrap();
        match raise(raised.as_deref(), gates) {
            Raise::Ask(r) => self.ctx.reason(Attention::NeedsInput, r),
            Raise::Update(r) => self.ctx.update_reason(r),
            Raise::Clear => self.ctx.clear(ReasonKind::Gate),
            Raise::Nothing => {}
        }
        *raised = raised_key(gates);
    }

    /// The fingerprint now, if it can be had.
    async fn fingerprint(&self) -> Option<String> {
        let r = self.runner().await.ok()?;
        let (out, _) = r.sh(model::FINGERPRINT, std::slice::from_ref(&self.config.root)).await.ok()?;
        Some(String::from_utf8_lossy(&out).trim().to_owned())
    }

    /// Whether the lifecycle ref is not what it was at the last read (one
    /// `git rev-parse`; can't tell, and it hasn't).
    async fn ref_moved(&self) -> bool {
        let Ok(r) = self.runner().await else { return false };
        let Ok((out, _)) = r.sh(model::LIFECYCLE, std::slice::from_ref(&self.config.root)).await else { return false };
        let now = String::from_utf8_lossy(&out);
        ref_moved(self.seen.lock().unwrap().as_deref(), now.trim())
    }

    /// Reads again if the fingerprint moved and settled, or the lifecycle
    /// ref moved ([`next`]).
    async fn check(&self) {
        let now = self.fingerprint().await;
        self.settle(now).await;
    }

    /// [`Workspace::check`] with the fingerprint in hand: what it did.
    async fn settle(&self, now: Option<String>) -> Next {
        let seen = self.seen.lock().unwrap().clone();
        let what = {
            let mut pending = self.pending.lock().unwrap();
            let what = next(seen.as_deref(), pending.as_deref(), now.as_deref());
            *pending = if what == Next::Wait { now } else { None };
            what
        };
        if what == Next::Read {
            self.load().await;
        }
        what
    }

    /// While drawn: the fingerprint every [`POLL`], from the start, and on
    /// this host the lifecycle ref every [`REF_POLL`] between ([`look`]).
    fn watch(&self, round: u64) {
        let Some(me) = self.me.upgrade() else { return };
        self.ctx.rt.spawn(async move {
            let quick = me.local();
            let mut tick = 0u32;
            while me.live.on(round) {
                match look(tick, quick) {
                    Look::Full => me.check().await,
                    Look::Ref => {
                        if me.ref_moved().await {
                            me.check().await;
                        }
                    }
                }
                tick = tick.wrapping_add(1);
                tokio::time::sleep(if quick { REF_POLL } else { POLL }).await;
            }
        });
    }

    /// While nobody draws it: every [`IDLE_POLL`] on this host, every
    /// [`VM_IDLE_POLL`] on a running VM ([`vm_look`]).
    async fn idle(&self) {
        if !self.local() {
            return self.idle_vm().await;
        }
        while !self.live.closed() {
            tokio::time::sleep(IDLE_POLL).await;
            if !self.live.drawn() && !self.live.closed() {
                self.check().await;
            }
        }
    }

    async fn idle_vm(&self) {
        let (Some(provider), Some(sprite)) = (self.ctx.provider.clone(), self.ctx.sprite.clone()) else { return };
        let mut vm = VmIdle::default();
        while !self.live.closed() {
            tokio::time::sleep(VM_IDLE_POLL).await;
            if self.live.drawn() {
                vm = VmIdle::default();
                continue;
            }
            if self.live.closed() {
                break;
            }
            let status = provider.status(&sprite).await.ok().flatten().map(|s| s.status);
            if !vm_look(status.as_deref(), &mut vm) {
                continue;
            }
            // Unreachable after all: not read (the block keeps what it had).
            let Some(now) = self.fingerprint().await else { continue };
            let what = self.settle(Some(now)).await;
            vm_looked(&mut vm, what != Next::Nothing);
        }
    }

    /// The gate `args` names: `{member, op, gate}`, `{key}`, or the first.
    fn find_gate(&self, args: &Value) -> Result<Gate, String> {
        let st = self.state.lock().unwrap();
        let arg = |k: &str| args[k].as_str();
        let want = match (arg("key"), arg("member"), arg("op"), arg("gate")) {
            (Some(k), ..) => Some(k.to_owned()),
            (None, Some(m), Some(o), Some(g)) => Some(format!("{m}/{o}/{g}")),
            (None, None, None, None) => None,
            _ => {
                return Err(
                    "approve needs {\"member\", \"op\", \"gate\"} (or {\"key\"}, or nothing for the first)".into()
                );
            }
        };
        match want {
            None => st.gates.first().cloned().ok_or_else(|| "no gate is waiting".into()),
            Some(k) => {
                st.gates.iter().find(|g| g.key() == k).cloned().ok_or_else(|| {
                    format!("no gate {k} is waiting (it was approved, or hasn't been read yet: refresh)")
                })
            }
        }
    }

    /// Who chant records for `by` (ws-080). `relayed` is set by the
    /// daemon, never the caller: someone other than the owner asked.
    fn chant_by(&self, args: &Value, by: Option<&str>, version: Option<String>) -> crate::gate::ChantBy {
        let relayed = args["relayed"].as_bool().unwrap_or(false);
        let who = self.who.lock().unwrap().clone();
        let named = by.map(|n| model::principal(n, &who.principals));
        crate::gate::ChantBy {
            actor: if relayed { named } else { who.actor.clone().or(named) },
            relayed,
            relayed_by: who.actor.filter(|_| version.is_some_and(|v| model::at_least(&v, model::RELAYED_BY))),
        }
    }

    /// Approve the gate `args` names, or with `expire` turn it down (#310:
    /// `chant approve --expire`, so the next run stops there again). The
    /// same people may, and it's logged the same way.
    async fn approve(&self, args: Value, by: Option<String>, expire: bool) -> Result<Value, String> {
        let gate = self.find_gate(&args)?;
        if matches!(gate.source, GateSource::Point { .. }) {
            return Err(format!("{} is a decision point's question: answer it with one of its choices", gate.key()));
        }
        let (chant, version) = {
            let st = self.state.lock().unwrap();
            (st.chant.clone().ok_or(model::NO_CHANT)?, st.version.clone())
        };
        let runner = self.runner().await?;
        let via = crate::gate::Via::Chant {
            runner: &runner,
            chant: &chant,
            by: self.chant_by(&args, by.as_deref(), version),
        };
        let result = if expire {
            crate::gate::expire(&gate, &via).await
        } else {
            crate::gate::approve(&gate, by.as_deref(), &via).await
        };
        let (ok, said) = match &result {
            Ok(s) | Err(s) => (result.is_ok(), s.clone()),
        };
        let (e, done) = if expire { ("expire", "expired") } else { ("approve", "approved") };
        log(
            &self.ctx,
            &json!({ "e": e, "member": gate.member, "op": gate.op, "gate": gate.gate, "by": by, "ok": ok, "said": said }),
        );
        // The block's history says who approved what (`arugula history`).
        if let Ok(mut l) = self.ctx.log() {
            let at = l.end();
            let dir = match &gate.source {
                GateSource::Chant { dir, .. } => dir,
                GateSource::Hud { box_url, .. } => box_url,
                GateSource::Forge { url, .. } => url,
                GateSource::Point { root, .. } => root,
            };
            let text = format!("{done} {}: {} at gate {}", gate.member, gate.op, gate.gate);
            let _ = l.record(
                at,
                Event::Command {
                    at_ms: now_ms(),
                    text: Some(text),
                    cwd: Some(dir.clone()),
                    by: by.clone(),
                    kind: HistoryKind::Answer,
                },
            );
            let _ = l.record(at, Event::End { at_ms: now_ms(), exit: Some(if ok { 0 } else { 1 }) });
        }
        let said = result?;
        self.load().await;
        Ok(json!({ done: gate.key(), "gate": gate, "by": by, "said": said }))
    }

    /// Answer the decision point's question `args` names (`{key, answer}`)
    /// with one of its choices (#621): `chant workspace points answer`, as
    /// the person who asked, recorded like an approval.
    async fn answer(&self, args: Value, by: Option<String>) -> Result<Value, String> {
        let gate = self.find_gate(&args)?;
        let GateSource::Point { choices, .. } = &gate.source else {
            return Err(format!("{} waits at a gate: approve it, or expire it", gate.key()));
        };
        let value = match &args["answer"] {
            Value::String(v) => v.clone(),
            Value::Bool(b) => b.to_string(),
            _ => return Err("answer needs {\"key\", \"answer\"}: one of its choices".into()),
        };
        let label = choices.iter().find(|c| c.value == value).map_or(value.clone(), |c| c.label.clone());
        let (chant, version) = {
            let st = self.state.lock().unwrap();
            (st.chant.clone().ok_or(model::NO_CHANT)?, st.version.clone())
        };
        let runner = self.runner().await?;
        let via = crate::gate::Via::Chant {
            runner: &runner,
            chant: &chant,
            by: self.chant_by(&args, by.as_deref(), version),
        };
        let result = crate::gate::answer(&gate, &value, &via).await;
        let (ok, said) = match &result {
            Ok(s) | Err(s) => (result.is_ok(), s.clone()),
        };
        log(&self.ctx, &json!({ "e": "answer", "key": gate.key(), "answer": value, "by": by, "ok": ok, "said": said }));
        if let Ok(mut l) = self.ctx.log() {
            let at = l.end();
            let text = format!("answered {}: {label}", gate.headline());
            let _ = l.record(
                at,
                Event::Command {
                    at_ms: now_ms(),
                    text: Some(text),
                    cwd: Some(self.config.root.clone()),
                    by: by.clone(),
                    kind: HistoryKind::Answer,
                },
            );
            let _ = l.record(at, Event::End { at_ms: now_ms(), exit: Some(if ok { 0 } else { 1 }) });
        }
        let said = result?;
        self.load().await;
        Ok(json!({ "answered": gate.key(), "answer": value, "label": label, "gate": gate, "by": by, "said": said }))
    }
}

/// A read, what it was read at ([`intent_key`]), and for a member's
/// intent the gates waiting in it when it was read.
struct Known<T> {
    print: Option<String>,
    got: Result<T, String>,
    gates: Vec<String>,
}

/// What an intent read (and the run ledger) depends on, from a
/// fingerprint: its first two words, the `chant/lifecycle` ref and `HEAD`
/// ([`model::FINGERPRINT`]). An edit in the working tree doesn't move it;
/// a commit, a gate, a release or a run recorded does.
fn intent_key(print: Option<&str>) -> Option<&str> {
    let p = print?;
    let end = p.match_indices(' ').nth(1).map_or(p.len(), |(i, _)| i);
    Some(&p[..end])
}

/// The reads behind a gate's decisions (#617) and a member card's why
/// (#618).
#[derive(Default)]
struct Why {
    /// Each member's intent read, by name.
    intents: HashMap<String, Known<model::Intent>>,
    /// The run ledger, for open cards.
    ledger: Option<Known<Vec<arugula_proto::workspace::RunRef>>>,
    /// The members whose cards a client opened (`why {member}`), until
    /// one closes it or no client draws the block.
    open: HashSet<String>,
}

/// What [`wanted`] says to read.
#[derive(Debug, Default, PartialEq, Eq)]
struct Wanted {
    /// Members (name, directory) whose intent read isn't had.
    members: Vec<(String, String)>,
    /// The run ledger.
    runs: bool,
}

impl Wanted {
    fn is_empty(&self) -> bool {
        self.members.is_empty() && !self.runs
    }
}

/// What isn't had at `key`: the intent of each member with a chant gate
/// waiting, and while some client draws the block (`drawn`), of each
/// member whose card is open, and the run ledger. While nobody draws it,
/// only a gate no read was made for yet: the one read when it's raised,
/// for the swarm and the phone.
fn wanted(st: &model::State, why: &Why, key: Option<&str>, drawn: bool) -> Wanted {
    let mut out = Wanted::default();
    let mut want = |name: &String| {
        if !out.members.iter().any(|(n, _)| n == name) {
            let dir = st.members.iter().find(|m| m.name == *name).map(|m| m.dir.clone());
            out.members.push((name.clone(), dir.unwrap_or_else(|| ".".into())));
        }
    };
    // A gate in a member (`status`'s, which carry a `why`), not a decision
    // point's question asked of the workspace.
    for g in st.gates.iter().filter(|g| g.why.is_some()) {
        let fresh = match why.intents.get(&g.member) {
            Some(k) if drawn => k.print.as_deref() == key,
            Some(k) => k.gates.contains(&g.key()),
            None => false,
        };
        if !fresh {
            want(&g.member);
        }
    }
    let open: Vec<&String> = st.members.iter().map(|m| &m.name).filter(|n| drawn && why.open.contains(*n)).collect();
    for name in &open {
        if !why.intents.get(*name).is_some_and(|k| k.print.as_deref() == key) {
            want(name);
        }
    }
    out.runs = !open.is_empty() && !why.ledger.as_ref().is_some_and(|k| k.print.as_deref() == key);
    out
}

/// What the reads had last, onto the gates of their members and the open
/// cards, until a fresh read replaces it: a card keeps what it shows while
/// the next read runs.
fn attach(st: &mut model::State, why: &Why) {
    let intent = |name: &str| why.intents.get(name).map(|k| &k.got);
    for g in &mut st.gates {
        let (Some(w), Some(got)) = (g.why.as_mut(), intent(&g.member)) else { continue };
        match got {
            Ok(i) => (w.decisions, w.note) = (Some(i.decisions.clone()), None),
            Err(e) => (w.decisions, w.note) = (None, Some(e.clone())),
        }
    }
    let ledger = why.ledger.as_ref().map(|k| &k.got);
    let leases = st.leases.clone();
    for m in &mut st.members {
        if why.open.contains(&m.name) {
            model::member_why(m, &leases, intent(&m.name), ledger);
        }
    }
}

/// What [`raise`] does about the gates' attention.
#[derive(Debug, Clone, PartialEq)]
enum Raise {
    /// Another gate, or the first: ask (a push, the rail).
    Ask(arugula_proto::Reason),
    /// The same gates, saying more (their decisions read): the card says
    /// so while the attention lasts, and asks nothing again.
    Update(arugula_proto::Reason),
    /// None waits: let go.
    Clear,
    /// None waits, and none did.
    Nothing,
}

/// What names the gates for attention: the reason's headline and every
/// gate's key. Their `why` isn't in it, so a read landing, or the
/// fingerprint moving, never asks again for a gate someone dismissed.
fn raised_key(gates: &[Gate]) -> Option<String> {
    let reason = crate::gate::reason(gates)?;
    let mut keys: Vec<String> = gates.iter().map(Gate::key).collect();
    keys.sort();
    Some(format!("{}\n{}", reason.headline, keys.join("\n")))
}

/// Given what attention last asked for (`was`), what these gates call for.
fn raise(was: Option<&str>, gates: &[Gate]) -> Raise {
    match (crate::gate::reason(gates), raised_key(gates)) {
        (Some(r), now) if now.as_deref() == was => Raise::Update(r),
        (Some(r), _) => Raise::Ask(r),
        (None, _) if was.is_none() => Raise::Nothing,
        (None, _) => Raise::Clear,
    }
}

/// What a poll does about the fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    Nothing,
    /// It moved: look again next poll.
    Wait,
    Read,
}

/// The `chant/lifecycle` word of a fingerprint.
fn lifecycle(print: Option<&str>) -> Option<&str> {
    print.and_then(|p| p.split_whitespace().next())
}

/// Given the fingerprint at the last read (`seen`), the changed one the
/// poll before saw (`pending`) and the one now: read when the lifecycle ref
/// moved (a gate or a release: at once), or when a change held still for
/// one poll; wait while the working tree is still moving. No fingerprint at
/// all reads, so the block says why.
fn next(seen: Option<&str>, pending: Option<&str>, now: Option<&str>) -> Next {
    match now {
        None => Next::Read,
        Some(n) if seen == Some(n) => Next::Nothing,
        _ if lifecycle(seen) != lifecycle(now) => Next::Read,
        Some(n) if pending == Some(n) => Next::Read,
        Some(_) => Next::Wait,
    }
}

/// What a drawn block's poll looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    /// The whole fingerprint ([`Workspace::check`]).
    Full,
    /// Only the lifecycle ref; the whole fingerprint if it moved.
    Ref,
}

/// The `tick`th poll of a drawn block, polled every [`REF_POLL`] when
/// `quick` (on this host), else every [`POLL`]: the full fingerprint on the
/// first and then once a [`POLL`], the lifecycle ref alone between.
fn look(tick: u32, quick: bool) -> Look {
    let every = (POLL.as_millis() / REF_POLL.as_millis()).max(1) as u32;
    if !quick || tick.is_multiple_of(every) { Look::Full } else { Look::Ref }
}

/// Whether the lifecycle ref's word now differs from the one in the
/// fingerprint at the last read. Nothing read yet: the next full look reads.
fn ref_moved(seen: Option<&str>, now: &str) -> bool {
    !now.is_empty() && seen.is_some_and(|s| lifecycle(Some(s)) != Some(now))
}

/// A VM workspace's idle looks: how many in a row found nothing changed,
/// and how many were skipped since the last, once it's resting.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct VmIdle {
    quiet: u32,
    skipped: u32,
}

/// Whether an idle poll looks at a VM's workspace, given what its provider
/// says of it: only when it's `running`, and once it has been quiet for
/// [`VM_QUIET`] looks, every [`VM_RESTING`]th poll. A VM not running starts
/// it over, so one woken again is looked at every poll.
fn vm_look(status: Option<&str>, vm: &mut VmIdle) -> bool {
    if status != Some("running") {
        *vm = VmIdle::default();
        return false;
    }
    if vm.quiet < VM_QUIET {
        return true;
    }
    vm.skipped += 1;
    if vm.skipped >= VM_RESTING {
        vm.skipped = 0;
        return true;
    }
    false
}

/// After a look: whether the fingerprint had moved.
fn vm_looked(vm: &mut VmIdle, moved: bool) {
    if moved {
        *vm = VmIdle::default();
    } else {
        vm.quiet = vm.quiet.saturating_add(1);
    }
}

impl Block for Workspace {
    fn kind(&self) -> BlockType {
        BlockType::Workspace
    }

    fn config(&self) -> Value {
        let who = self.who.lock().unwrap();
        json!({
            "root": self.config.root, "env": *self.env.lock().unwrap(), "actor": who.actor, "principals": who.principals,
            "hud": *self.hud.lock().unwrap(),
        })
    }

    fn state(&self) -> Value {
        let mut v = serde_json::to_value(&*self.state.lock().unwrap()).unwrap_or_default();
        v["watching"] = self.live.drawn().into();
        v["envs"] = json!(*self.envs.lock().unwrap());
        let who = self.who.lock().unwrap();
        v["actor"] = json!(who.actor);
        v["principals"] = json!(who.principals);
        v["hud"] = json!(*self.hud.lock().unwrap());
        v["graph"] = json!(self.graph.status());
        v
    }

    fn text(&self) -> String {
        self.state.lock().unwrap().text()
    }

    fn call(&self, method: &str, args: Value) -> BoxFuture<'static, Result<Value, String>> {
        self.call_by(method, args, None)
    }

    fn call_by(&self, method: &str, args: Value, by: Option<&str>) -> BoxFuture<'static, Result<Value, String>> {
        let me = self.me.upgrade();
        match method {
            "refresh" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                // Read the decisions again too (an uncommitted record moves
                // no key); what's shown stays until they land.
                {
                    let mut why = me.why.lock().unwrap();
                    for k in why.intents.values_mut() {
                        (k.print, k.gates) = (None, vec![]);
                    }
                    if let Some(k) = why.ledger.as_mut() {
                        k.print = None;
                    }
                }
                me.load().await;
                let st = me.state.lock().unwrap();
                match &st.error {
                    Some(e) => Err(e.clone()),
                    None => Ok(json!({ "members": st.members.len(), "gates": st.gates.len(), "ms": st.ms })),
                }
            }),
            "approve" | "expire" => {
                let (by, expire) = (by.map(str::to_owned), method == "expire");
                Box::pin(async move { me.ok_or("closed")?.approve(args, by, expire).await })
            }
            "answer" => {
                let by = by.map(str::to_owned);
                Box::pin(async move { me.ok_or("closed")?.answer(args, by).await })
            }
            "env" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let name = env_name(args["name"].as_str().unwrap_or_default())?;
                let was = std::mem::replace(&mut *me.env.lock().unwrap(), name.clone());
                if was != name {
                    log(&me.ctx, &json!({ "e": "env", "env": name, "was": was }));
                    {
                        let mut all = me.envs.lock().unwrap();
                        if !all.contains(&name) {
                            all.push(name.clone());
                            all.sort();
                        }
                    }
                    // The gates of the env it watched aren't its any more.
                    me.state.lock().unwrap().gates.clear();
                    me.load().await;
                }
                Ok(json!({ "env": name, "was": was }))
            }),
            "principals" => {
                let set = {
                    let mut who = self.who.lock().unwrap();
                    set_principals(&who, &args).map(|new| {
                        let was = std::mem::replace(&mut *who, new.clone());
                        (new, was)
                    })
                };
                if let Ok((new, was)) = &set
                    && new != was
                {
                    log(&self.ctx, &json!({ "e": "principals", "actor": new.actor, "principals": new.principals }));
                    // Kept in the config: the layout saves it at the next write.
                    self.ctx.changed();
                }
                Box::pin(async move {
                    let (new, _) = set?;
                    Ok(json!({ "actor": new.actor, "principals": new.principals }))
                })
            }
            "member" => {
                let st = self.state.lock().unwrap();
                let name = args["name"].as_str().unwrap_or_default().to_owned();
                let m = st.members.iter().find(|m| m.name == name).map(
                    |m| json!({ "name": m.name, "path": m.path, "kind": m.kind, "nested": m.nested, "gates": m.gates }),
                );
                Box::pin(async move { m.ok_or_else(|| format!("no member {name:?}")) })
            }
            // #618: a member card opened (or closed): its decisions, runs
            // and leases are read now, and kept up while it's open.
            "why" => {
                let name = args["member"].as_str().unwrap_or_default().to_owned();
                let open = args["open"].as_bool().unwrap_or(true);
                let known = self.state.lock().unwrap().members.iter().any(|m| m.name == name);
                if known {
                    let mut why = self.why.lock().unwrap();
                    if open {
                        why.open.insert(name.clone())
                    } else {
                        why.open.remove(&name)
                    };
                }
                if known {
                    let mut st = self.state.lock().unwrap();
                    if !open && let Some(m) = st.members.iter_mut().find(|m| m.name == name) {
                        m.why = None;
                    }
                    self.attach(&mut st);
                    drop(st);
                    self.ctx.changed();
                    if open {
                        self.read_why();
                    }
                }
                Box::pin(async move {
                    if !known {
                        return Err(format!("no member {name:?}"));
                    }
                    Ok(json!({ "member": name, "open": open }))
                })
            }
            "hud" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let url = hud_url(args["url"].as_str().unwrap_or_default())?;
                log(&me.ctx, &json!({ "e": "hud", "url": url }));
                *me.hud.lock().unwrap() = url.clone();
                me.ctx.changed();
                Ok(json!({ "hud": url }))
            }),
            "graph" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let runner = me.runner().await?;
                let weak = me.me.clone();
                let changed = move || {
                    if let Some(w) = weak.upgrade() {
                        w.ctx.changed();
                    }
                };
                let src = me.graph.ensure(&me.ctx, &runner, &me.config.root, changed).await?;
                Ok(json!({ "src": src }))
            }),
            "state" => {
                let s = self.state();
                Box::pin(async move { Ok(s) })
            }
            m => {
                let e = no_method(BlockType::Workspace, m);
                Box::pin(async move { Err(e) })
            }
        }
    }

    fn drawn(&self, on: bool) {
        if let Some(round) = self.live.set(on) {
            self.watch(round);
            // A gate raised while nobody looked is read at this commit.
            self.read_why();
        }
        // #618: nobody draws it, so no card is open. A client still showing
        // one asks again (`why`) when it sees the card without its why.
        if !on {
            self.why.lock().unwrap().open.clear();
            for m in &mut self.state.lock().unwrap().members {
                m.why = None;
            }
        }
        self.ctx.changed();
    }

    fn close(&self) {
        self.live.close();
        self.graph.close(&self.ctx);
    }

    fn summary(&self) -> Summary {
        let st = self.state.lock().unwrap();
        Summary {
            project: crate::review::project(&self.config.root, self.local()),
            cwd: Some(self.config.root.clone()),
            title: Some(format!("{} (chant)", st.name.clone().unwrap_or_else(|| "workspace".into()))),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        Known, Look, Next, POLL, Principals, REF_POLL, Raise, VM_QUIET, VM_RESTING, VmIdle, Wanted, Why, attach,
        env_name, envs, hud_url, intent_key, look, model, next, raise, raised_key, ref_moved, set_principals, vm_look,
        vm_looked, wanted,
    };

    #[test]
    fn a_gate_s_decisions_are_read_once_per_commit_and_kept_meanwhile() {
        // #617: a gate waiting wants its member's intent read; once had at
        // this HEAD and lifecycle ref it goes on the gate, and isn't read
        // again until a commit or the ledger moves it. An edit doesn't.
        let raw: serde_json::Value = serde_json::from_str(include_str!("fixtures/reference-raw.json")).unwrap();
        let doc: serde_json::Value = serde_json::from_str(include_str!("fixtures/intent-delivery.json")).unwrap();
        let mut st = model::compose(&raw, "local");
        let mut why = Why::default();
        let delivery = vec![("delivery".to_owned(), "delivery".to_owned())];
        let key = |print: &str| intent_key(Some(print)).map(str::to_owned);
        assert_eq!(wanted(&st, &why, key("lc h1 9 9").as_deref(), true).members, delivery);
        let gates = vec!["delivery/release/approve-release".to_owned()];
        let got = Ok(model::intent(&doc, 1400).unwrap());
        why.intents.insert("delivery".to_owned(), Known { print: key("lc h1 9 9"), got, gates: gates.clone() });
        // An edit in the tree: the same key, no read.
        assert!(wanted(&st, &why, key("lc h1 7 7").as_deref(), true).is_empty());
        attach(&mut st, &why);
        assert_eq!(st.gates[0].why.as_ref().unwrap().decisions.as_ref().unwrap()[0].id, "toy-001");
        assert!(st.text().contains("    enforces toy-001 A person approves each ship\n"), "{}", st.text());
        // A commit: read again, and meanwhile the gate keeps what it had
        // (no flicker to "Reading").
        assert_eq!(wanted(&st, &why, key("lc h2 7 7").as_deref(), true).members, delivery);
        let mut fresh = model::compose(&raw, "local");
        attach(&mut fresh, &why);
        assert_eq!(fresh.gates[0].why.as_ref().unwrap().decisions.as_ref().unwrap()[0].id, "toy-001");
        // ...until a fresh read replaces it; a failure is a note.
        why.intents
            .insert("delivery".to_owned(), Known { print: key("lc h2 7 7"), got: Err("too slow".into()), gates });
        attach(&mut fresh, &why);
        let why = fresh.gates[0].why.as_ref().unwrap();
        assert_eq!((why.decisions.as_ref(), why.note.as_deref()), (None, Some("too slow")));
        // No gate, no read.
        fresh.gates.clear();
        assert!(wanted(&fresh, &Why::default(), key("lc h2 7 7").as_deref(), true).is_empty());
    }

    #[test]
    fn nobody_looking_reads_a_gate_once_when_it_s_raised() {
        let raw: serde_json::Value = serde_json::from_str(include_str!("fixtures/reference-raw.json")).unwrap();
        let st = model::compose(&raw, "local");
        let mut why = Why::default();
        // Raised while nobody draws the block: read, for the swarm and phone.
        assert_eq!(wanted(&st, &why, Some("lc h1"), false).members.len(), 1);
        let gates = vec!["delivery/release/approve-release".to_owned()];
        why.intents.insert("delivery".to_owned(), Known { print: Some("lc h1".into()), got: Err("x".into()), gates });
        // Commits since, still nobody looking: no more reads.
        assert!(wanted(&st, &why, Some("lc h9"), false).is_empty());
        // Someone looks: read at this commit.
        assert_eq!(wanted(&st, &why, Some("lc h9"), true).members.len(), 1);
        // Another gate raised in the member while nobody looks: read once.
        let mut raw2 = raw.clone();
        for g in raw2["reads"]["status"]["json"]["members"][1]["gates"].as_array_mut().unwrap() {
            g["state"] = "pending".into();
        }
        assert_eq!(wanted(&model::compose(&raw2, "local"), &why, Some("lc h9"), false).members.len(), 1);
    }

    #[test]
    fn a_dismissed_gate_isn_t_asked_again_when_the_tree_or_its_decisions_move() {
        // #617: attention names the gates by headline and keys, never their
        // `why`. Raised, then dismissed (the pane goes idle); the
        // fingerprint moves (a fresh read resets `why`), and the decisions
        // land: each is an update, which the mux applies only while the
        // pane still needs input, so nothing is asked again.
        let raw: serde_json::Value = serde_json::from_str(include_str!("fixtures/reference-raw.json")).unwrap();
        let doc: serde_json::Value = serde_json::from_str(include_str!("fixtures/intent-delivery.json")).unwrap();
        let first = model::compose(&raw, "local");
        assert!(matches!(raise(Some(""), &first.gates), Raise::Ask(_)));
        let was = raised_key(&first.gates);
        // The fingerprint moved: a new read, `why` back to nothing read.
        let moved = model::compose(&raw, "local");
        assert!(matches!(raise(was.as_deref(), &moved.gates), Raise::Update(_)));
        // The decisions landed.
        let mut read = moved.clone();
        let mut why = Why::default();
        let got = Ok(model::intent(&doc, 1).unwrap());
        why.intents.insert("delivery".to_owned(), Known { print: None, got, gates: vec![] });
        attach(&mut read, &why);
        let Raise::Update(r) = raise(was.as_deref(), &read.gates) else { panic!("asked again") };
        assert_eq!(r.gate.unwrap().why.unwrap().decisions.unwrap()[0].id, "toy-001");
        // Another gate is a new ask; none is let go.
        let mut raw2 = raw.clone();
        for g in raw2["reads"]["status"]["json"]["members"][1]["gates"].as_array_mut().unwrap() {
            g["state"] = "pending".into();
        }
        assert!(matches!(raise(was.as_deref(), &model::compose(&raw2, "local").gates), Raise::Ask(_)));
        assert_eq!(raise(was.as_deref(), &[]), Raise::Clear);
        assert_eq!(raise(None, &[]), Raise::Nothing);
    }

    #[test]
    fn an_intent_read_s_key_is_the_lifecycle_ref_and_head() {
        assert_eq!(intent_key(Some("lc h1 123 45")), Some("lc h1"));
        assert_eq!(intent_key(Some("gone")), Some("gone"));
        assert_eq!(intent_key(None), None);
    }

    #[test]
    fn an_open_card_reads_its_member_and_the_runs_while_drawn() {
        // #618: a card opened wants its member's intent and the run ledger,
        // only while some client draws the block; what they say goes on
        // the card, with its leases linked to the pane behind them.
        let mut raw: serde_json::Value = serde_json::from_str(include_str!("fixtures/reference-raw.json")).unwrap();
        // delivery's gates approved: nothing but the card wants a read.
        for g in raw["reads"]["status"]["json"]["members"][1]["gates"].as_array_mut().unwrap() {
            g["state"] = "approved".into();
        }
        raw["agents"] = serde_json::json!([{ "name": "shipper", "member": "delivery" }]);
        raw["reads"]["status"]["json"]["leases"] = serde_json::json!([{ "item": "W-001", "holder": "shipper",
        "token": "106ffd19-97f5-476c-824e-12ec93af6f28", "acquiredAt": "2026-10-08T01:13:30.857Z",
        "expiresAt": "2026-10-08T01:23:30.857Z", "state": "active", "ref": "refs/chant/lease/work/W-001",
        "member": null }]);
        let mut st = model::compose(&raw, "local");
        let mut why = Why::default();
        assert!(wanted(&st, &why, Some("lc h1"), true).is_empty());
        why.open.insert("delivery".into());
        let want = Wanted { members: vec![("delivery".into(), "delivery".into())], runs: true };
        assert_eq!(wanted(&st, &why, Some("lc h1"), true), want);
        // Nobody draws it: nothing, however long the card was left open.
        assert!(wanted(&st, &why, Some("lc h1"), false).is_empty());
        // Open, nothing read yet: the card says it's reading.
        attach(&mut st, &why);
        let card = st.members.iter().find(|m| m.name == "delivery").unwrap();
        assert_eq!(card.why.as_ref().unwrap().decisions, None);
        // Read. The daemon names a run's pane only once the agent block in
        // that pane says it wrote the run (`ledger`): here, pane 7 did.
        let intent: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/intent-delivery-work.json")).unwrap();
        let runs: serde_json::Value = serde_json::from_str(include_str!("fixtures/runs.json")).unwrap();
        let mut runs = model::runs(&runs).unwrap();
        assert_eq!(runs[0].pane, None);
        runs[0].pane = Some(7);
        let key = || Some("lc h1".to_owned());
        why.intents.insert("delivery".into(), Known { print: key(), got: model::intent(&intent, 900), gates: vec![] });
        why.ledger = Some(Known { print: key(), got: Ok(runs), gates: vec![] });
        assert!(wanted(&st, &why, Some("lc h1"), true).is_empty());
        attach(&mut st, &why);
        let card = st.members.iter().find(|m| m.name == "delivery").unwrap();
        let w = card.why.as_ref().unwrap();
        assert_eq!(w.decisions.as_ref().unwrap()[0].id, "toy-001");
        assert_eq!(
            w.runs.iter().map(|r| (r.id.as_str(), r.pane)).collect::<Vec<_>>(),
            [("arugula-7-1759880000000", Some(7))]
        );
        // The flat ledger's lease is on delivery through W-001, which chant's
        // walk says covers it, and the run under its token ran in pane 7.
        assert_eq!(card.leases.iter().map(|l| (l.item.as_str(), l.pane)).collect::<Vec<_>>(), [("W-001", Some(7))]);
        // The app card isn't open: no why, no lease.
        let app = st.members.iter().find(|m| m.name == "app").unwrap();
        assert!(app.why.is_none() && app.leases.is_empty());
        // A commit: read again, and the card keeps what it shows meanwhile.
        assert_eq!(wanted(&st, &why, Some("lc h2"), true), want);
        let mut fresh = model::compose(&raw, "local");
        attach(&mut fresh, &why);
        let card = fresh.members.iter().find(|m| m.name == "delivery").unwrap();
        assert_eq!(card.why.as_ref().unwrap().decisions.as_ref().unwrap()[0].id, "toy-001");
    }

    /// A fingerprint: the lifecycle ref's word, then the tree's checksum.
    fn fp(lifecycle: &str, tree: u32) -> String {
        format!("{lifecycle} {tree} 1234")
    }

    /// Polls a sequence of fingerprints from a read at `start`; how many
    /// full reads they cost.
    fn reads(start: &str, polls: &[String]) -> usize {
        let (mut seen, mut pending, mut n) = (Some(start.to_owned()), None::<String>, 0);
        for now in polls {
            let what = next(seen.as_deref(), pending.as_deref(), Some(now));
            pending = (what == Next::Wait).then(|| now.clone());
            if what == Next::Read {
                n += 1;
                seen = Some(now.clone());
            }
        }
        n
    }

    #[test]
    fn a_burst_of_edits_costs_one_read() {
        // The tree changes at every poll for a while (an agent editing a
        // member), then holds still.
        let mut polls: Vec<String> = (1..=8).map(|t| fp("aaa", t)).collect();
        polls.extend([fp("aaa", 8), fp("aaa", 8), fp("aaa", 8)]);
        assert_eq!(reads(&fp("aaa", 0), &polls), 1);
    }

    #[test]
    fn nothing_moved_reads_nothing() {
        assert_eq!(next(Some("a 1"), None, Some("a 1")), Next::Nothing);
        assert_eq!(reads(&fp("aaa", 0), &[fp("aaa", 0), fp("aaa", 0)]), 0);
    }

    #[test]
    fn a_change_waits_one_poll() {
        assert_eq!(next(Some("a 1"), None, Some("a 2")), Next::Wait);
        assert_eq!(next(Some("a 1"), Some("a 2"), Some("a 2")), Next::Read);
        // Still moving: wait again.
        assert_eq!(next(Some("a 1"), Some("a 2"), Some("a 3")), Next::Wait);
    }

    #[test]
    fn a_gate_reads_at_once() {
        // `chant run` exits at a gate: the lifecycle ref moves, while the
        // tree is still changing. The very next poll reads.
        assert_eq!(next(Some("aaa 1"), None, Some("bbb 2")), Next::Read);
        assert_eq!(next(Some("aaa 1"), Some("aaa 2"), Some("bbb 3")), Next::Read);
        // The ref appearing for the first run's gate counts too.
        assert_eq!(next(Some("- 1"), None, Some("bbb 1")), Next::Read);
        let mut polls: Vec<String> = (1..=4).map(|t| fp("aaa", t)).collect();
        polls.push(fp("bbb", 5));
        assert_eq!(reads(&fp("aaa", 0), &polls), 1);
    }

    #[test]
    fn drawn_here_the_lifecycle_ref_is_looked_at_every_second_and_the_rest_every_poll() {
        // On this host: a full fingerprint first and once a POLL, the ref
        // alone the other ticks, so a gate waits at most REF_POLL to be seen
        // while full looks are no more frequent than before.
        let every = (POLL.as_millis() / REF_POLL.as_millis()) as u32;
        assert!(REF_POLL <= std::time::Duration::from_secs(1) && every >= 3);
        let ticks: Vec<Look> = (0..2 * every).map(|t| look(t, true)).collect();
        assert_eq!(ticks.iter().filter(|l| **l == Look::Full).count(), 2);
        assert_eq!(ticks[0], Look::Full);
        assert_eq!(ticks[every as usize], Look::Full);
        assert!(ticks[1..every as usize].iter().all(|l| *l == Look::Ref));
        // On a VM: the full fingerprint every POLL, as before.
        assert!((0..10).all(|t| look(t, false) == Look::Full));

        // The quick look: a moved ref sends the poll to the full fingerprint
        // (which then reads at once, as `next` says).
        assert!(ref_moved(Some("aaa 1 2"), "bbb"));
        assert!(ref_moved(Some("- 1 2"), "bbb"));
        assert!(!ref_moved(Some("aaa 1 2"), "aaa"));
        // Couldn't tell, or nothing read yet: leave it to the full look.
        assert!(!ref_moved(Some("aaa 1 2"), ""));
        assert!(!ref_moved(None, "aaa"));
        assert_eq!(next(Some("aaa 1 2"), None, Some("bbb 1 2")), Next::Read);
    }

    #[test]
    fn no_fingerprint_reads_so_the_block_says_why() {
        assert_eq!(next(Some("a 1"), None, None), Next::Read);
        // Nothing read before: the first fingerprint reads.
        assert_eq!(next(None, None, Some("a 1")), Next::Read);
    }

    #[test]
    fn the_envs_chant_has_ledgers_for() {
        let ledgers = "local/releases.jsonl\n_members/delivery/prod/releases.jsonl\n\
                       _members/delivery/local/releases.jsonl\nstaging/releases.jsonl\n_gates/releases.jsonl\n";
        assert_eq!(envs(ledgers, "local"), ["local", "prod", "staging"]);
        // No lifecycle ref yet: local, and the one watched.
        assert_eq!(envs("", "qa"), ["local", "qa"]);
    }

    #[test]
    fn principals_are_set_and_cleared() {
        let none = Principals::default();
        let p = set_principals(&none, &json!({ "actor": " github:sam ", "principals": { "val": "github:val-x" } }))
            .unwrap();
        assert_eq!(p.actor.as_deref(), Some("github:sam"));
        assert_eq!(p.principals.get("val").map(String::as_str), Some("github:val-x"));
        // A key left out keeps what was there.
        let q = set_principals(&p, &json!({ "principals": { "jo": "github:jo", "gone": "" } })).unwrap();
        assert_eq!(q.actor.as_deref(), Some("github:sam"));
        assert_eq!(q.principals.keys().collect::<Vec<_>>(), ["jo"]);
        // An empty actor, or null, clears it.
        assert_eq!(set_principals(&q, &json!({ "actor": "" })).unwrap().actor, None);
        assert_eq!(set_principals(&q, &json!({ "actor": null })).unwrap().actor, None);
        // A principal is one word, not a flag.
        for bad in [
            json!({ "actor": "--sign" }),
            json!({ "actor": "a b" }),
            json!({ "actor": 3 }),
            json!({ "principals": { "x": "-y" } }),
        ] {
            assert!(set_principals(&q, &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn hud_s_address_is_a_url() {
        assert_eq!(hud_url(" https://hud.example/ "), Ok(Some("https://hud.example".into())));
        assert_eq!(hud_url("http://127.0.0.1:4000/__hud"), Ok(Some("http://127.0.0.1:4000/__hud".into())));
        assert_eq!(hud_url(""), Ok(None));
        for bad in [
            "hud.example",
            "javascript:alert(1)",
            "https://a b",
            "file:///etc/passwd",
            "http://",
            "https://hud.example/?x=1",
            "https://hud.example/#x",
            "https://user:pw@hud.example",
            "data:text/html,hi",
        ] {
            assert!(hud_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_env_is_a_name_not_a_flag() {
        assert_eq!(env_name(" prod ").as_deref(), Ok("prod"));
        for bad in ["", "--json", "a b", "a/b"] {
            assert!(env_name(bad).is_err(), "{bad:?}");
        }
    }

    /// Polls a VM's idle watch for `polls` rounds with `status`, nothing
    /// changing; which rounds looked.
    fn looks(vm: &mut VmIdle, status: Option<&str>, polls: u32) -> Vec<u32> {
        (0..polls)
            .filter(|_| {
                let look = vm_look(status, vm);
                if look {
                    vm_looked(vm, false);
                }
                look
            })
            .collect()
    }

    #[test]
    fn a_running_vm_is_looked_at_every_poll_while_it_changes() {
        let mut vm = VmIdle::default();
        for _ in 0..50 {
            assert!(vm_look(Some("running"), &mut vm));
            vm_looked(&mut vm, true);
        }
    }

    #[test]
    fn a_vm_not_running_or_unreachable_is_never_looked_at() {
        for status in [Some("warm"), Some("cold"), Some("stopped"), None] {
            let mut vm = VmIdle::default();
            assert!(looks(&mut vm, status, 100).is_empty(), "{status:?}");
        }
    }

    #[test]
    fn a_quiet_vm_is_looked_at_less_often_until_it_sleeps_and_wakes() {
        let mut vm = VmIdle::default();
        // Quiet: every poll for VM_QUIET looks, then every VM_RESTING-th.
        let n = VM_QUIET + 3 * VM_RESTING;
        let seen = looks(&mut vm, Some("running"), n);
        assert_eq!(seen.len() as u32, VM_QUIET + 3);
        assert_eq!(seen[VM_QUIET as usize], VM_QUIET + VM_RESTING - 1);
        // A change found while resting: every poll again.
        vm_looked(&mut vm, true);
        assert_eq!(vm, VmIdle::default());
        assert!(vm_look(Some("running"), &mut vm));
        // Asleep, then woken: every poll again.
        let _ = looks(&mut vm, Some("running"), n);
        assert!(!vm_look(Some("warm"), &mut vm));
        assert!(vm_look(Some("running"), &mut vm));
    }
}
