//! The agents block (M77, #399): the team's agent catalog, a card per agent
//! grouped by person and machine (offline machines' as last seen), and this
//! machine's recipes to offer. Config `{dir?}`: the project whose recipes it
//! lists (the owner's last pick).
//!
//! Methods: `refresh` (ask every machine now); `recipes {dir}` (a project's
//! recipes, offered or not); `offer {agent, dir}` and `unoffer {agent}`;
//! `run_here {agent, cwd?, prompt?}`, a Claude Code beside it as one of this
//! machine's offered recipes; `run_there {machine, agent, prompt?}`, one
//! another of the account's machines offers, run there (in a session of its
//! name) for the client to go to; and `revoke {account, agent}`, a standing
//! grant taken back. Those are the owner's (authz.rs).
//!
//! **Consent (M79):** one Team agents block holds the cards for tasks that
//! wait for this machine's own account (`tasks.rs`): one card at a time,
//! *Allow once*, *Allow for an hour* or *Deny* (declining is denying). When
//! a task needs one and none is open, one opens in session `a2a`. A card's
//! answer from another account's device (a team's other owner) is refused
//! before it gets here (api.rs).

use std::{
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex, Weak},
};

use arugula_proto::{
    BlockType,
    api::OpenRequest,
    ask::{Ask, AskKind},
};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    catalog::{self, Catalog},
    tasks::{self, Answer},
};
use crate::{
    block::{Block, BlockCtx, no_method},
    mux::AskReply,
    server::App,
    store::now_ms,
};

/// The block that holds consent cards: the first Team agents block that
/// opened and is still open.
static HUB: LazyLock<Mutex<Option<Weak<AgentsBlock>>>> = LazyLock::new(Default::default);

fn hub() -> Option<Arc<AgentsBlock>> {
    HUB.lock().unwrap().as_ref().and_then(Weak::upgrade).filter(|b| !b.closed())
}

/// A task waits for this machine's own account: show its card, on the hub,
/// or on a Team agents block opened for it in session `a2a`.
pub async fn consent_needed(app: &Arc<App>) {
    if let Some(b) = hub() {
        b.ctx.changed();
        b.raise().await;
        return;
    }
    let req = OpenRequest {
        kind: BlockType::Agents,
        config: json!({}),
        session: Some("a2a".into()),
        local: true,
        ..Default::default()
    };
    if let Some(Err(e)) = app.mux.api(|r| crate::mux::Api::Open(req, None, r)).await {
        tracing::warn!(error = e, "can't open a block for a task's consent card");
    }
}

/// A waiting task was answered (or ended): the next card.
pub fn consent_settled(_app: &Arc<App>) {
    if let Some(b) = hub() {
        b.ctx.changed();
        let me = b.clone();
        b.ctx.rt.spawn(async move { me.raise().await });
    }
}

/// A waiting task ended without an answer (its caller canceled it): its
/// card comes down, and the next one goes up.
pub fn task_gone(id: &str) {
    let Some(b) = hub() else { return };
    let up = b.asking.lock().unwrap().take_if(|(t, _)| t == id);
    if let Some((t, token)) = up {
        b.ctx.withdraw(&t, token);
    }
    b.ctx.changed();
    let me = b.clone();
    b.ctx.rt.spawn(async move { me.raise().await });
}

/// The card for a waiting task.
fn card(t: &tasks::Task) -> Ask {
    let device = t.caller.device.as_deref().map(|d| format!(" on {d}")).unwrap_or_default();
    let claims = match (t.caller.claims["machine"].as_str(), t.caller.claims["pane"].as_u64()) {
        (Some(m), Some(p)) => format!(" (says it's from {m}, pane %{p})"),
        (Some(m), None) => format!(" (says it's from {m})"),
        _ => String::new(),
    };
    let text: String = t.text.chars().take(600).collect();
    let how = if t.pr { ", and open a pull request from there as you" } else { "" };
    let message = format!("{}{device}{claims} asks {} to work in {}{how}: {text}", t.caller.name, t.agent, t.dir);
    let schema = json!({
        "type": "object",
        "properties": {
            "answer": { "type": "string", "title": "Let it run?", "default": "once",
                "oneOf": [
                    { "const": "once", "title": "Allow once" },
                    { "const": "hour", "title": "Allow for an hour", "description": format!("{}'s tasks for {}, until you revoke it", t.caller.name, t.agent) },
                    { "const": "deny", "title": "Deny" },
                ] },
        },
        "required": ["answer"],
    });
    Ask {
        id: t.id.clone(),
        kind: AskKind::Form,
        message,
        questions: None,
        schema: Some(schema),
        url: None,
        accepted: false,
        tool_call_id: None,
        source: "a2a".into(),
        agent: Some(format!("{}'s agent", t.caller.name)),
        at_ms: t.at_ms,
        tool: None,
        input: None,
        suggestions: None,
        session: None,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Config {
    /// The project whose recipes it lists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dir: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct State {
    catalog: Catalog,
    loading: bool,
    error: Option<String>,
    /// The project whose recipes are listed, and those recipes.
    dir: Option<String>,
    recipes: Vec<Value>,
    recipes_error: Option<String>,
    /// What the last action did, to show.
    said: Option<String>,
    updated_ms: u64,
    /// Tasks from other people waiting for this machine's own account.
    waiting: Vec<Value>,
    /// Standing grants: whose tasks for which agent, until when.
    grants: Vec<tasks::Grant>,
}

/// The agent catalog's kind: it holds what runs an invite from a card
/// (#234), which is set once the daemon is up.
pub struct AgentsKind(pub crate::invite::Hook);

impl crate::block::BlockKind for AgentsKind {
    fn create(&self, ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        AgentsBlock::create(ctx, self.0.clone(), config)
    }
}

pub struct AgentsBlock {
    ctx: BlockCtx,
    hook: crate::invite::Hook,
    me: Weak<AgentsBlock>,
    config: Mutex<Config>,
    state: Mutex<State>,
    /// The card up now: its task, and the mux's token.
    asking: Mutex<Option<(String, u64)>>,
    asking_lock: tokio::sync::Mutex<()>,
    closed: std::sync::atomic::AtomicBool,
}

impl AgentsBlock {
    fn create(ctx: BlockCtx, hook: crate::invite::Hook, config: Value) -> Result<Arc<dyn Block>, String> {
        let config: Config = serde_json::from_value(config).map_err(|e| format!("agents config: {e}"))?;
        let state = State { loading: true, dir: config.dir.clone(), ..State::default() };
        let b = Arc::new_cyclic(|me| Self {
            ctx,
            hook,
            me: me.clone(),
            config: Mutex::new(config),
            state: Mutex::new(state),
            asking: Mutex::new(None),
            asking_lock: tokio::sync::Mutex::new(()),
            closed: std::sync::atomic::AtomicBool::new(false),
        });
        {
            let mut h = HUB.lock().unwrap();
            if h.as_ref().and_then(Weak::upgrade).is_none_or(|x| x.closed()) {
                *h = Some(Arc::downgrade(&b));
            }
        }
        let me = b.clone();
        b.ctx.rt.spawn(async move {
            me.raise().await;
            me.read(catalog::FRESH_MS).await;
            me.list_recipes();
        });
        Ok(b)
    }

    fn closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn is_hub(&self) -> bool {
        hub().is_some_and(|h| std::ptr::eq(Arc::as_ptr(&h), self))
    }

    /// The oldest waiting task's card, if none is up (the hub's alone).
    fn raise(&self) -> futures_util::future::BoxFuture<'_, ()> {
        Box::pin(async move {
            if !self.is_hub() {
                return;
            }
            let _one = self.asking_lock.lock().await;
            if self.asking.lock().unwrap().is_some() || self.closed() {
                return;
            }
            let Some(t) = tasks::waiting().into_iter().next() else { return };
            match self.ctx.ask(card(&t)).await {
                Ok((token, rx)) => {
                    *self.asking.lock().unwrap() = Some((t.id.clone(), token));
                    let Some(me) = self.me.upgrade() else { return };
                    self.ctx.rt.spawn(async move {
                        let (reply, by) = rx.await.unwrap_or((AskReply::Withdrawn, None));
                        me.answered(&t.id, token, reply, by).await;
                    });
                }
                Err(e) => tracing::warn!(pane = self.ctx.id, error = e, "can't show a task's card"),
            }
        })
    }

    async fn answered(&self, id: &str, token: u64, reply: AskReply, by: Option<arugula_proto::Driver>) {
        {
            let mut asking = self.asking.lock().unwrap();
            if asking.as_ref() != Some(&(id.to_owned(), token)) {
                return;
            }
            *asking = None;
        }
        // Only the owner (and, api.rs, of this machine's own account).
        let owner = by.as_ref().is_some_and(|b| b.who == "owner");
        let answer = match (&reply, owner) {
            (AskReply::Answer(c), true) => Answer::parse(c["answer"].as_str().unwrap_or("once")),
            (AskReply::Decline, true) => Some(Answer::Deny),
            _ => None,
        };
        if let Some(a) = answer {
            let who = by.map(|b| b.name).unwrap_or_default();
            tracing::info!(task = id, answer = ?a, by = who, "a task's card answered");
            if let Err(e) = tasks::answer(id, a) {
                tracing::warn!(task = id, error = e, "a task's answer came too late");
            }
            self.said(match a {
                Answer::Once => format!("allowed {id} once"),
                Answer::Hour => format!("allowed {id}, and its caller's tasks for this agent for an hour"),
                Answer::Deny => format!("denied {id}"),
            });
        }
        self.ctx.changed();
        self.raise().await;
    }

    /// The daemon (set once the server is: blocks are made first).
    fn app(&self) -> Result<Arc<App>, String> {
        self.hook.get().and_then(Weak::upgrade).ok_or_else(|| "the daemon is starting".to_owned())
    }

    async fn read(&self, max_age_ms: u64) {
        let app = match self.app() {
            Ok(a) => a,
            Err(e) => {
                self.state.lock().unwrap().error = Some(e);
                return self.ctx.changed();
            }
        };
        if !crate::labs::on(&self.ctx.state_dir, arugula_proto::flags::AGENTS) {
            let mut st = self.state.lock().unwrap();
            st.loading = false;
            st.error = Some("The agent catalog is in Labs: turn on Agent catalog in Developer settings".into());
            drop(st);
            return self.ctx.changed();
        }
        self.state.lock().unwrap().loading = true;
        self.ctx.changed();
        let c = catalog::get(&app, max_age_ms).await;
        let mut st = self.state.lock().unwrap();
        st.catalog = c;
        st.loading = false;
        st.error = None;
        st.updated_ms = now_ms();
        drop(st);
        self.ctx.changed();
    }

    /// The project's recipes (and the user's), with where each is offered
    /// from.
    fn list_recipes(&self) {
        let Some(dir) = self.config.lock().unwrap().dir.clone() else { return };
        let home = crate::home();
        let path = super::super::fountain::wear::expand(&dir, &home);
        let offered = super::offers(&self.ctx.state_dir);
        let list: Vec<Value> = super::all(&path, &home)
            .into_iter()
            .map(|r| {
                let mut v = serde_json::to_value(&r).unwrap_or_default();
                v["offered_from"] = json!(offered.iter().find(|o| o.agent == r.name).map(|o| &o.dir));
                v
            })
            .collect();
        let mut st = self.state.lock().unwrap();
        st.recipes_error = (!path.is_dir()).then(|| format!("{dir} isn't a directory here"));
        st.dir = Some(dir);
        st.recipes = list;
        drop(st);
        self.ctx.changed();
    }

    fn said(&self, s: String) {
        self.state.lock().unwrap().said = Some(s);
        self.ctx.changed();
    }

    async fn offer(&self, args: Value, on: bool) -> Result<Value, String> {
        let agent = args["agent"].as_str().filter(|a| !a.is_empty()).ok_or("{\"agent\": NAME}")?.to_owned();
        let dir = if on {
            let d = args["dir"].as_str().map(str::to_owned).or_else(|| self.config.lock().unwrap().dir.clone());
            Some(d.ok_or("{\"dir\": the project it works in}")?)
        } else {
            None
        };
        let list = super::set_offer(&self.ctx.state_dir, &crate::home(), &agent, dir.as_deref())?;
        if let Ok(app) = self.app() {
            app.control.poke();
        }
        self.list_recipes();
        self.said(if on {
            format!("{agent} is offered to your team")
        } else {
            format!("{agent} is no longer offered")
        });
        self.read(0).await;
        Ok(json!({ "offers": list }))
    }

    async fn run_here(&self, args: Value) -> Result<Value, String> {
        let agent = args["agent"].as_str().filter(|a| !a.is_empty()).ok_or("run_here needs {\"agent\": NAME}")?;
        if self.ctx.sprite.is_some() {
            return Err("Run here runs a recipe on this host: open the catalog on it".into());
        }
        let offer = super::offers(&self.ctx.state_dir).into_iter().find(|o| o.agent == agent);
        let given = args["cwd"].as_str().map(str::trim).filter(|c| !c.is_empty());
        let cwd: PathBuf = match (given, &offer) {
            (Some(c), _) => super::super::fountain::wear::expand(c, &crate::home()),
            (None, Some(o)) => PathBuf::from(&o.dir),
            (None, None) => return Err(format!("{agent} isn't offered here: say where to run it (cwd)")),
        };
        if !cwd.is_dir() {
            return Err(format!("{} isn't a directory here", cwd.display()));
        }
        super::find(&cwd, &crate::home(), agent)?;
        let mut config = json!({ "agent": "claude", "recipe": agent, "cwd": cwd });
        if let Some(p) = args["prompt"].as_str().filter(|p| !p.is_empty()) {
            config["prompt"] = json!(p);
        }
        let req = OpenRequest {
            kind: BlockType::Agent,
            config,
            split: Some(self.ctx.id),
            from_pane: Some(self.ctx.id),
            local: true,
            ..Default::default()
        };
        let block = self.ctx.open(req).await?;
        self.said(format!("{agent} runs here in %{block}"));
        Ok(json!({ "block": block, "agent": agent, "cwd": cwd }))
    }
}

impl Block for AgentsBlock {
    fn kind(&self) -> BlockType {
        BlockType::Agents
    }

    fn config(&self) -> Value {
        serde_json::to_value(&*self.config.lock().unwrap()).unwrap_or_default()
    }

    fn state(&self) -> Value {
        let mut st = self.state.lock().unwrap();
        st.waiting = tasks::waiting()
            .iter()
            .map(|t| json!({ "id": t.id, "agent": t.agent, "caller": t.caller.name, "account": t.caller.account, "device": t.caller.device, "claims": t.caller.claims, "text": t.text, "dir": t.dir, "pr": t.pr, "at_ms": t.at_ms }))
            .collect();
        st.grants = tasks::grants(&self.ctx.state_dir);
        serde_json::to_value(&*st).unwrap_or_default()
    }

    fn text(&self) -> String {
        let st = self.state.lock().unwrap();
        let mut out = String::from("Team agents\n");
        if let Some(e) = &st.error {
            out.push_str(&format!("{e}\n"));
        }
        if st.loading && st.updated_ms == 0 {
            out.push_str("asking every machine…\n");
            return out;
        }
        let mut any = false;
        for (s, c) in st.catalog.agents() {
            any = true;
            out.push_str(&format!("  {}\n", Catalog::line(s, c)));
        }
        if !any {
            out.push_str("  none offered\n");
        }
        for s in st.catalog.machines.iter().filter(|s| s.note.is_some()) {
            out.push_str(&format!("{}: {}\n", s.name, s.note.as_deref().unwrap_or_default()));
        }
        out
    }

    fn call(&self, method: &str, args: Value) -> BoxFuture<'static, Result<Value, String>> {
        let me = self.me.upgrade();
        match method {
            "refresh" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                me.read(0).await;
                me.list_recipes();
                Ok(json!({ "machines": me.state.lock().unwrap().catalog.machines.len() }))
            }),
            "recipes" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let dir =
                    args["dir"].as_str().map(str::trim).filter(|d| !d.is_empty()).ok_or("{\"dir\": a project}")?;
                me.config.lock().unwrap().dir = Some(dir.to_owned());
                me.list_recipes();
                let st = me.state.lock().unwrap();
                match &st.recipes_error {
                    Some(e) => Err(e.clone()),
                    None => Ok(json!({ "recipes": st.recipes.len() })),
                }
            }),
            "offer" => Box::pin(async move { me.ok_or("closed")?.offer(args, true).await }),
            "unoffer" => Box::pin(async move { me.ok_or("closed")?.offer(args, false).await }),
            "run_here" => Box::pin(async move { me.ok_or("closed")?.run_here(args).await }),
            "run_there" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let agent = args["agent"].as_str().filter(|a| !a.is_empty()).ok_or("{\"machine\", \"agent\"}")?;
                let machine = args["machine"].as_str().filter(|m| !m.is_empty()).ok_or("{\"machine\", \"agent\"}")?;
                let prompt = args["prompt"].as_str();
                let v = super::delegate::run_there(&me.app()?, machine, agent, prompt).await?;
                me.said(format!("{agent} runs on {} in %{}", v["machine"].as_str().unwrap_or(machine), v["block"]));
                Ok(v)
            }),
            "revoke" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                let account = args["account"].as_str().ok_or("{\"account\", \"agent\"}")?;
                let agent = args["agent"].as_str().ok_or("{\"account\", \"agent\"}")?;
                let had = tasks::revoke(&me.ctx.state_dir, account, agent)?;
                me.said(if had { format!("{agent}'s grant taken back") } else { "no such grant".into() });
                Ok(json!({ "revoked": had }))
            }),
            m => {
                let e = no_method(BlockType::Agents, m);
                Box::pin(async move { Err(e) })
            }
        }
    }

    fn close(&self) {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some((id, token)) = self.asking.lock().unwrap().take() {
            self.ctx.withdraw(&id, token);
        }
        // Another Team agents block takes the cards over (one opens if none
        // is), while any task waits.
        if !tasks::waiting().is_empty()
            && let Ok(app) = self.app()
        {
            self.ctx.rt.spawn(async move { consent_needed(&app).await });
        }
    }
}
