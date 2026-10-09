//! The agents block (M77, #399): the team's agent catalog, a card per agent
//! grouped by person and machine (offline machines' as last seen), and this
//! machine's recipes to offer. Config `{dir?}`: the project whose recipes it
//! lists (the owner's last pick).
//!
//! Methods: `refresh` (ask every machine now); `recipes {dir}` (a project's
//! recipes, offered or not); `offer {agent, dir}` and `unoffer {agent}`; and
//! `run_here {agent, cwd?, prompt?}`, a Claude Code beside it as one of this
//! machine's offered recipes. Offering and running are the owner's
//! (authz.rs). *Send a task* is M78's.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
};

use arugula_proto::{BlockType, api::OpenRequest};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::catalog::{self, Catalog};
use crate::{
    block::{Block, BlockCtx, no_method},
    server::App,
    store::now_ms,
};

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
}

pub struct AgentsBlock {
    ctx: BlockCtx,
    me: Weak<AgentsBlock>,
    config: Mutex<Config>,
    state: Mutex<State>,
}

impl AgentsBlock {
    pub fn create(ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        let config: Config = serde_json::from_value(config).map_err(|e| format!("agents config: {e}"))?;
        let state = State { loading: true, dir: config.dir.clone(), ..State::default() };
        let b =
            Arc::new_cyclic(|me| Self { ctx, me: me.clone(), config: Mutex::new(config), state: Mutex::new(state) });
        let me = b.clone();
        b.ctx.rt.spawn(async move {
            me.read(catalog::FRESH_MS).await;
            me.list_recipes();
        });
        Ok(b)
    }

    /// The daemon (set once the server is: blocks are made first).
    fn app(&self) -> Result<Arc<App>, String> {
        self.ctx.invite.get().and_then(Weak::upgrade).ok_or_else(|| "the daemon is starting".to_owned())
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
        serde_json::to_value(&*self.state.lock().unwrap()).unwrap_or_default()
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
            m => {
                let e = no_method(BlockType::Agents, m);
                Box::pin(async move { Err(e) })
            }
        }
    }

    fn close(&self) {}
}
