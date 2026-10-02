//! S21 (throwaway, branch only): a chant workspace as a block.
//!
//! Config `{root, env?}`. It reads the workspace through chant's read
//! contract on the block's host (`ls`, `check`, `records`, `status <env>`,
//! one `sh -c`, the reads in parallel), with the spike's own reader and
//! composer (`spikes/s21-chant-workspace/src/model.rs`), so the block shows
//! exactly what the spike prints. While drawn it looks at a cheap
//! fingerprint every [`POLL`] and reads again only when that changes,
//! and on `refresh`.
//!
//! A pending gate is attention (`needs_input`, "delivery: ship waits at
//! gate approve-ship"), and `approve {member, op, gate}` resolves it with
//! chant's own `approve`.
//!
//! Methods: `refresh`, `approve {member, op, gate}`, `member {name}` (its
//! directory, for opening panes there), `state`.

#[path = "../../../spikes/s21-chant-workspace/src/model.rs"]
#[allow(dead_code)]
mod model;

use std::{
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use futures_util::future::BoxFuture;
use illogical_proto::{Attention, BlockType};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    block::{Block, BlockCtx, Summary, no_method},
    review::{Live, Runner, every, log},
    store::now_ms,
};

/// How often the fingerprint is looked at while drawn (a full read only
/// when it changes).
const POLL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Deserialize)]
struct Config {
    root: String,
    #[serde(default = "local")]
    env: String,
}

fn local() -> String {
    "local".into()
}

pub struct Workspace {
    ctx: BlockCtx,
    me: Weak<Workspace>,
    config: Config,
    runner: Result<Runner, String>,
    state: Mutex<model::State>,
    /// The fingerprint at the last read.
    seen: Mutex<Option<String>>,
    /// What attention was last asked for, so it's asked once per change.
    asked: Mutex<Option<String>>,
    live: Live,
    reading: tokio::sync::Mutex<()>,
}

impl Workspace {
    pub fn create(ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        let config: Config = serde_json::from_value(config).map_err(|e| format!("workspace config: {e}"))?;
        if config.root.is_empty() {
            return Err("a workspace block needs a root".into());
        }
        let runner = Runner::of(&ctx);
        let state = model::State { root: config.root.clone(), env: config.env.clone(), loading: true, ..Default::default() };
        log(&ctx, &json!({ "e": "view", "root": config.root, "env": config.env }));
        let w = Arc::new_cyclic(|me| Self {
            ctx,
            me: me.clone(),
            config,
            runner,
            state: Mutex::new(state),
            seen: Mutex::new(None),
            asked: Mutex::new(None),
            live: Live::default(),
            reading: tokio::sync::Mutex::new(()),
        });
        let me = w.clone();
        w.ctx.rt.spawn(async move { me.load().await });
        Ok(w)
    }

    async fn load(&self) {
        let _one = self.reading.lock().await;
        if self.live.closed() {
            return;
        }
        let print = self.fingerprint().await;
        *self.seen.lock().unwrap() = print;
        let mut st = match &self.runner {
            Err(e) => model::State { error: Some(e.clone()), ..Default::default() },
            Ok(r) => {
                let args = [self.config.root.clone(), model::READER.to_owned(), self.config.env.clone()];
                match r.sh(model::SCRIPT, &args).await {
                    Err(e) => model::State { error: Some(e), ..Default::default() },
                    Ok((out, _)) => match serde_json::from_slice::<Value>(&out) {
                        Ok(raw) => model::compose(&raw, &self.config.env),
                        Err(e) => model::State { error: Some(format!("the reader said something else: {e}")), ..Default::default() },
                    },
                }
            }
        };
        if st.root.is_empty() {
            st.root = self.config.root.clone();
        }
        st.env = self.config.env.clone();
        st.updated_ms = now_ms();
        let why = st.attention();
        *self.state.lock().unwrap() = st;
        self.ctx.changed();
        let mut asked = self.asked.lock().unwrap();
        if *asked != why {
            match &why {
                Some(w) => self.ctx.attention(Attention::NeedsInput, w.clone()),
                None => self.ctx.attention(Attention::Idle, "nothing waits"),
            }
            *asked = why;
        }
    }

    /// The fingerprint now, if it can be had.
    async fn fingerprint(&self) -> Option<String> {
        let r = self.runner.as_ref().ok()?;
        let (out, _) = r.sh(model::FINGERPRINT, std::slice::from_ref(&self.config.root)).await.ok()?;
        Some(String::from_utf8_lossy(&out).trim().to_owned())
    }

    fn watch(&self, round: u64) {
        let Some(me) = self.me.upgrade() else { return };
        let every = every(matches!(self.runner, Ok(ref r) if r.local()), POLL).max(POLL);
        self.ctx.rt.spawn(async move {
            while me.live.on(round) {
                tokio::time::sleep(every).await;
                if !me.live.on(round) {
                    break;
                }
                let now = me.fingerprint().await;
                let changed = now.is_none() || *me.seen.lock().unwrap() != now;
                if changed {
                    me.load().await;
                }
            }
        });
    }

    async fn approve(&self, member: String, op: String, gate: String, by: Option<String>) -> Result<Value, String> {
        let (chant, path) = {
            let st = self.state.lock().unwrap();
            let chant = st.chant.clone().ok_or("no chant here")?;
            let m = st.members.iter().find(|m| m.name == member).ok_or_else(|| format!("no member {member:?}"))?;
            (chant, m.path.clone())
        };
        let runner = self.runner.clone()?;
        let script = r#"cd "$1" || exit 1; c=$2; shift 2
command -v node >/dev/null 2>&1 || PATH=$("${SHELL:-sh}" -ic 'printf %s "$PATH"' 2>/dev/null </dev/null) && export PATH
exec "$c" approve "$@" 2>&1"#;
        let mut args = vec![path, chant, op.clone(), gate.clone()];
        if let Some(by) = &by {
            args.extend(["--approver".into(), by.clone()]);
        }
        let (out, code) = runner.sh(script, &args).await?;
        let said = String::from_utf8_lossy(&out).replace('\x1b', "");
        log(&self.ctx, &json!({ "e": "approve", "member": member, "op": op, "gate": gate, "by": by, "code": code }));
        self.load().await;
        match code {
            Some(0) => Ok(json!({ "approved": true, "said": said.trim() })),
            _ => Err(said.trim().to_owned()),
        }
    }
}

impl Block for Workspace {
    fn kind(&self) -> BlockType {
        BlockType::Workspace
    }

    fn config(&self) -> Value {
        json!({ "root": self.config.root, "env": self.config.env })
    }

    fn state(&self) -> Value {
        let mut v = serde_json::to_value(&*self.state.lock().unwrap()).unwrap_or_default();
        v["watching"] = self.live.drawn().into();
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
        let arg = |k: &str| args[k].as_str().map(str::to_owned);
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
            "approve" => {
                let (member, op, gate) = (arg("member"), arg("op"), arg("gate"));
                let by = by.map(str::to_owned);
                Box::pin(async move {
                    let me = me.ok_or("closed")?;
                    let (Some(member), Some(op), Some(gate)) = (member, op, gate) else {
                        return Err("approve needs {\"member\", \"op\", \"gate\"}".into());
                    };
                    me.approve(member, op, gate, by).await
                })
            }
            "member" => {
                let st = self.state.lock().unwrap();
                let name = arg("name").unwrap_or_default();
                let m = st.members.iter().find(|m| m.name == name).map(|m| json!({ "name": m.name, "path": m.path, "kind": m.kind }));
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
            project: crate::review::project(&self.config.root, matches!(self.runner, Ok(ref r) if r.local())),
            cwd: Some(self.config.root.clone()),
            title: Some(format!("{} (chant)", st.name.clone().unwrap_or_else(|| "workspace".into()))),
            ..Default::default()
        }
    }
}
