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
//! looked at every [`POLL`] while some client draws the block, and every
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
//! **Envs (#312).** A block watches one env, and says which; `env {name}`
//! switches it (kept in the config) and reads again. The state's `envs`
//! are the ones chant has written releases for on `chant/lifecycle`, from
//! one `git ls-tree` per full read ([`ENVS`]), plus `local` and the one
//! watched, for the block's menu. Gates in the other envs aren't read:
//! each env is another `status`, another chant process (about 2-3
//! CPU-seconds) on every full read, for every block, VM or not.
//!
//! `expire` (same arguments, same people, logged the same way) turns a gate
//! down (#310): `chant approve <op> <gate> --expire` clears its pending fact
//! without approving it, so the next run stops there again.
//!
//! Methods: `refresh`, `approve {member, op, gate}` (or `{key}`; the first
//! gate if none), `expire` (the same), `member {name}` (its directory, for
//! opening panes there), `env {name}`, `state`.

mod model;

use std::{
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
}

fn local() -> String {
    "local".into()
}

pub struct Workspace {
    ctx: BlockCtx,
    me: Weak<Workspace>,
    config: Config,
    /// The env watched: `config.env` at open, then what `env` chose.
    env: Mutex<String>,
    /// The envs chant knows ([`envs`]), at the last read.
    envs: Mutex<Vec<String>>,
    runner: tokio::sync::OnceCell<Result<Runner, String>>,
    state: Mutex<model::State>,
    /// The fingerprint at the last read.
    seen: Mutex<Option<String>>,
    /// A changed fingerprint the last poll saw, waiting to hold still.
    pending: Mutex<Option<String>>,
    /// The gate attention last asked for (its headline), so it's asked once
    /// per change. Starts as `Some("")` so the first read clears any left
    /// from before a restart.
    raised: Mutex<Option<String>>,
    live: Live,
    reading: tokio::sync::Mutex<()>,
}

impl Workspace {
    pub fn create(ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        let mut config: Config = serde_json::from_value(config).map_err(|e| format!("workspace config: {e}"))?;
        if config.root.is_empty() {
            return Err("a workspace block needs a root".into());
        }
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
            config,
            runner: tokio::sync::OnceCell::new(),
            state: Mutex::new(state),
            seen: Mutex::new(None),
            pending: Mutex::new(None),
            raised: Mutex::new(Some(String::new())),
            live: Live::default(),
            reading: tokio::sync::Mutex::new(()),
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
            if let GateSource::Chant { machine, .. } = &mut g.source {
                machine.clone_from(&self.ctx.sprite);
            }
        }
        self.raise(&st.gates);
        *self.state.lock().unwrap() = st;
        self.ctx.changed();
    }

    /// Gates waiting are attention; none, and it's let go.
    fn raise(&self, gates: &[Gate]) {
        let reason = crate::gate::reason(gates);
        let now = reason.as_ref().map(|r| r.headline.clone());
        let mut raised = self.raised.lock().unwrap();
        if *raised == now {
            return;
        }
        match reason {
            Some(r) => self.ctx.reason(Attention::NeedsInput, r),
            None => self.ctx.clear(ReasonKind::Gate),
        }
        *raised = now;
    }

    /// The fingerprint now, if it can be had.
    async fn fingerprint(&self) -> Option<String> {
        let r = self.runner().await.ok()?;
        let (out, _) = r.sh(model::FINGERPRINT, std::slice::from_ref(&self.config.root)).await.ok()?;
        Some(String::from_utf8_lossy(&out).trim().to_owned())
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

    /// While drawn: the fingerprint every [`POLL`], from the start.
    fn watch(&self, round: u64) {
        let Some(me) = self.me.upgrade() else { return };
        self.ctx.rt.spawn(async move {
            while me.live.on(round) {
                me.check().await;
                tokio::time::sleep(POLL).await;
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
        let named = by.map(|n| model::principal(n, &self.config.principals));
        crate::gate::ChantBy {
            actor: if relayed { named } else { self.config.actor.clone().or(named) },
            relayed,
            relayed_by: self
                .config
                .actor
                .clone()
                .filter(|_| version.is_some_and(|v| model::at_least(&v, model::RELAYED_BY))),
        }
    }

    /// Approve the gate `args` names, or with `expire` turn it down (#310:
    /// `chant approve --expire`, so the next run stops there again). The
    /// same people may, and it's logged the same way.
    async fn approve(&self, args: Value, by: Option<String>, expire: bool) -> Result<Value, String> {
        let gate = self.find_gate(&args)?;
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
        json!({ "root": self.config.root, "env": *self.env.lock().unwrap(), "actor": self.config.actor, "principals": self.config.principals })
    }

    fn state(&self) -> Value {
        let mut v = serde_json::to_value(&*self.state.lock().unwrap()).unwrap_or_default();
        v["watching"] = self.live.drawn().into();
        v["envs"] = json!(*self.envs.lock().unwrap());
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
            "member" => {
                let st = self.state.lock().unwrap();
                let name = args["name"].as_str().unwrap_or_default().to_owned();
                let m = st.members.iter().find(|m| m.name == name).map(
                    |m| json!({ "name": m.name, "path": m.path, "kind": m.kind, "nested": m.nested, "gates": m.gates }),
                );
                Box::pin(async move { m.ok_or_else(|| format!("no member {name:?}")) })
            }
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
        }
        self.ctx.changed();
    }

    fn close(&self) {
        self.live.close();
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
    use super::{Next, VM_QUIET, VM_RESTING, VmIdle, env_name, envs, next, vm_look, vm_looked};

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
