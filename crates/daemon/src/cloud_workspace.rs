//! Cloud workspace blocks (#1117): one of the machine owner's Cloud
//! workspaces beside a pane, which `show` kind `cloud_workspace` opens.
//!
//! A Cloud workspace lives in control, not on this machine, and control's
//! page refuses to be framed, so the block isn't a frame: while a client
//! draws it, it reads the workspace and its tabs from control's
//! `/api/cloud/...` routes, signed with this machine's key as `list` and
//! `read_cloud` are (`Control::cloud`), and pushes them as its state. The
//! renderer reads a tab's transcript, its requests and the machine's
//! changes, and prompts, answers and interrupts, through the block's `cloud`
//! method, which goes to control the same way and reaches only this
//! workspace's routes (and the agents and models a tab can run). Control
//! acts as the machine's owner for whoever may call it, as it does for any
//! agent here (#1094's decision).

use std::{
    sync::{Arc, Mutex, OnceLock, Weak},
    time::Duration,
};

use arugula_proto::BlockType;
use futures_util::future::BoxFuture;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    block::{Block, BlockCtx, BlockKind, Summary, no_method},
    review::Live,
};

/// How often a drawn block reads the workspace again: as often as control's
/// page does.
const POLL: Duration = Duration::from_secs(3);

/// Control, as a Cloud workspace block reaches it.
pub trait Remote: Send + Sync {
    /// Control's `/api/cloud/...` route, as this machine's owner.
    fn call(&self, method: Method, path: String, body: Option<Value>) -> BoxFuture<'_, Result<Value, String>>;
    /// The workspace (or one of its tabs) on control's page.
    fn link(&self, workspace: &str, tab: Option<&str>) -> Option<String>;
}

impl Remote for crate::control::Control {
    // Not waiting out a maintenance window (M141): the block says so, calmly,
    // and its next read (3s on) or click tries again.
    fn call(&self, method: Method, path: String, body: Option<Value>) -> BoxFuture<'_, Result<Value, String>> {
        Box::pin(async move { self.cloud_now(method, &path, body.as_ref()).await.map_err(|e| e.to_string()) })
    }

    fn link(&self, workspace: &str, tab: Option<&str>) -> Option<String> {
        let e = self.enrolled()?;
        let tab = tab.map(|t| format!("/{t}")).unwrap_or_default();
        Some(format!("{}/#cloud-ws={workspace}{tab}", e.saved.url.trim_end_matches('/')))
    }
}

static REMOTE: OnceLock<Arc<dyn Remote>> = OnceLock::new();

/// The daemon's control, for Cloud workspace blocks made after this.
pub fn init(control: Arc<crate::control::Control>) {
    let _ = REMOTE.set(control);
}

/// An id as control makes them (`cw_…`, `cwc_…`), so nothing else reaches
/// its paths.
pub fn id(what: &str, prefix: &str, s: &str) -> Result<String, String> {
    let s = s.trim();
    match s.strip_prefix(prefix) {
        Some(hex) if !hex.is_empty() && hex.len() <= 64 && hex.bytes().all(|c| c.is_ascii_hexdigit()) => {
            Ok(s.to_owned())
        }
        _ => Err(format!("{s:?} isn't a {what} id: they look like {prefix}… (from list kind {what}s)")),
    }
}

/// What a call to the block may reach of control's `/api/cloud/`: `path`
/// (relative to it, with its query) when it's this workspace's or one of
/// the reads a new tab needs, and nothing that could step out of them.
fn allowed(workspace: &str, method: &Method, path: &str) -> Result<(), String> {
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    let refuse = || Err(format!("a Cloud workspace block reaches only {workspace}'s routes, not {method} {route}"));
    if query.contains('#') {
        return refuse();
    }
    // No way out of the route: no dot segments, encoded or not, and no
    // encoded slashes or backslashes.
    let lower = route.to_ascii_lowercase();
    if lower.contains("%2e") || lower.contains("%2f") || lower.contains("%5c") || route.contains('\\') {
        return refuse();
    }
    let segments: Vec<&str> = route.split('/').collect();
    if segments.iter().any(|s| s.is_empty() || *s == "." || *s == "..") {
        return refuse();
    }
    match segments.as_slice() {
        ["agents" | "models"] if *method == Method::GET => Ok(()),
        // Deleting it, or changing its settings, stays on control's page.
        ["workspaces", w] if *w == workspace && *method == Method::GET => Ok(()),
        ["workspaces", w, _, ..] if *w == workspace && [Method::GET, Method::POST, Method::DELETE].contains(method) => {
            Ok(())
        }
        _ => refuse(),
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Config {
    cloud_workspace: String,
    /// The tab it was opened on, if one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tab: Option<String>,
}

impl Config {
    fn parse(config: Value) -> Result<Self, String> {
        let mut c: Config = serde_json::from_value(config).map_err(|e| format!("cloud workspace config: {e}"))?;
        c.cloud_workspace = id("cloud_workspace", "cw_", &c.cloud_workspace)?;
        c.tab = c.tab.map(|t| id("cloud_tab", "cwc_", &t)).transpose()?;
        Ok(c)
    }
}

#[derive(Debug, Clone, Default, Serialize)]
struct State {
    cloud_workspace: String,
    tab: Option<String>,
    /// Control's record of it (`GET /api/cloud/workspaces/{id}`), none
    /// until it's read.
    workspace: Option<Value>,
    tabs: Vec<Value>,
    /// It on control's page.
    link: Option<String>,
    /// Why the last read failed.
    error: Option<String>,
    /// Not read yet.
    loading: bool,
    /// Whether a client draws it, so it reads.
    watching: bool,
}

pub struct CloudWorkspaceKind;

impl BlockKind for CloudWorkspaceKind {
    fn create(&self, ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        let remote = REMOTE.get().cloned();
        Ok(CloudWorkspaceBlock::new(ctx, Config::parse(config)?, remote))
    }

    fn check(&self, config: &Value) -> Result<(), String> {
        Config::parse(config.clone()).map(|_| ())
    }
}

pub struct CloudWorkspaceBlock {
    ctx: BlockCtx,
    me: Weak<CloudWorkspaceBlock>,
    config: Config,
    remote: Option<Arc<dyn Remote>>,
    state: Mutex<State>,
    live: Live,
}

impl CloudWorkspaceBlock {
    fn new(ctx: BlockCtx, config: Config, remote: Option<Arc<dyn Remote>>) -> Arc<Self> {
        let link = remote.as_ref().and_then(|r| r.link(&config.cloud_workspace, config.tab.as_deref()));
        let state = State {
            cloud_workspace: config.cloud_workspace.clone(),
            tab: config.tab.clone(),
            link,
            loading: true,
            ..Default::default()
        };
        let b = Arc::new_cyclic(|me| Self {
            ctx,
            me: me.clone(),
            config,
            remote,
            state: Mutex::new(state),
            live: Live::default(),
        });
        // Read once, drawn or not: `show` answers with it, and read_output
        // and history have its text.
        let me = b.clone();
        b.ctx.rt.spawn(async move { me.read().await });
        b
    }

    fn remote(&self) -> Result<Arc<dyn Remote>, String> {
        self.remote.clone().ok_or_else(|| crate::control::CloudRefused::NotJoined.to_string())
    }

    /// Control's route under `/api/cloud/`.
    async fn cloud(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value, String> {
        self.remote()?.call(method, format!("/api/cloud/{path}"), body).await
    }

    /// Read the workspace and its tabs again.
    async fn read(&self) {
        let w = &self.config.cloud_workspace;
        let read = async {
            let workspace = self.cloud(Method::GET, &format!("workspaces/{w}"), None).await?;
            let tabs = self.cloud(Method::GET, &format!("workspaces/{w}/tabs"), None).await?;
            Ok::<_, String>((workspace, tabs["cloud_tabs"].as_array().cloned().unwrap_or_default()))
        };
        let got = read.await;
        {
            let mut st = self.state.lock().unwrap();
            st.loading = false;
            match got {
                Ok((workspace, tabs)) => {
                    st.workspace = Some(workspace);
                    st.tabs = tabs;
                    st.error = None;
                }
                // What was read stays: control may be back on the next read.
                Err(e) => st.error = Some(e),
            }
        }
        self.ctx.changed();
    }

    fn watch(&self, round: u64) {
        let Some(me) = self.me.upgrade() else { return };
        self.ctx.rt.spawn(async move {
            while me.live.on(round) {
                me.read().await;
                tokio::time::sleep(POLL).await;
            }
        });
    }

    /// The block's `cloud` method: `{method, path, body?}` to control, for
    /// this workspace.
    async fn proxy(self: Arc<Self>, args: Value) -> Result<Value, String> {
        let method = match args["method"].as_str().unwrap_or("GET").to_ascii_uppercase().as_str() {
            "GET" => Method::GET,
            "POST" => Method::POST,
            "DELETE" => Method::DELETE,
            m => return Err(format!("a Cloud workspace block doesn't {m}")),
        };
        let path = args["path"].as_str().ok_or("cloud needs path")?.trim_start_matches('/').to_owned();
        allowed(&self.config.cloud_workspace, &method, &path)?;
        let body = args.get("body").filter(|b| !b.is_null()).cloned();
        let write = method != Method::GET;
        let out = self.cloud(method, &path, body).await;
        // A tab opened, prompted or closed: show it now, not in 3s.
        if write {
            let me = self.clone();
            self.ctx.rt.spawn(async move { me.read().await });
        }
        out
    }
}

/// A Cloud workspace's status, as its page says it.
fn status_word(w: &Value) -> String {
    match w["status"].as_str().unwrap_or("?") {
        "provisioning" => match w["setup_step"].as_str() {
            Some(step) => format!("setting up ({})", step.replace('_', " ")),
            None => "setting up".into(),
        },
        "ready" if w["sandbox_status"] == "suspended" => "parked".into(),
        s => s.to_owned(),
    }
}

impl Block for CloudWorkspaceBlock {
    fn kind(&self) -> BlockType {
        BlockType::CloudWorkspace
    }

    fn config(&self) -> Value {
        serde_json::to_value(&self.config).unwrap_or_default()
    }

    fn state(&self) -> Value {
        serde_json::to_value(&*self.state.lock().unwrap()).unwrap_or_default()
    }

    fn text(&self) -> String {
        let st = self.state.lock().unwrap();
        let mut out = String::new();
        match &st.workspace {
            Some(w) => out.push_str(&format!(
                "Cloud workspace {} ({}): {}\n",
                w["name"].as_str().unwrap_or("?"),
                st.cloud_workspace,
                status_word(w)
            )),
            None => out.push_str(&format!("Cloud workspace {}\n", st.cloud_workspace)),
        }
        for t in &st.tabs {
            out.push_str(&format!(
                "tab {} {} ({}, {}) in {}\n",
                t["id"].as_str().unwrap_or("?"),
                t["agent_name"].as_str().unwrap_or("?"),
                t["model"].as_str().unwrap_or("?"),
                t["status"].as_str().unwrap_or("?"),
                t["cwd"].as_str().unwrap_or("?"),
            ));
        }
        if let Some(e) = &st.error {
            out.push_str(&format!("error: {e}\n"));
        }
        if let Some(l) = &st.link {
            out.push_str(&format!("on control's page: {l}\n"));
        }
        out
    }

    fn call(&self, method: &str, args: Value) -> BoxFuture<'static, Result<Value, String>> {
        let Some(me) = self.me.upgrade() else {
            return Box::pin(async { Err("the block closed".to_owned()) });
        };
        match method {
            "cloud" => Box::pin(me.proxy(args)),
            "refresh" => Box::pin(async move {
                me.read().await;
                Ok(me.state())
            }),
            m => {
                let e = no_method(BlockType::CloudWorkspace, m);
                Box::pin(async move { Err(e) })
            }
        }
    }

    fn drawn(&self, on: bool) {
        if let Some(round) = self.live.set(on) {
            self.watch(round);
        }
        let mut st = self.state.lock().unwrap();
        if st.watching != self.live.drawn() {
            st.watching = self.live.drawn();
            drop(st);
            self.ctx.changed();
        }
    }

    fn close(&self) {
        self.live.close();
    }

    fn summary(&self) -> Summary {
        let st = self.state.lock().unwrap();
        Summary {
            title: st.workspace.as_ref().and_then(|w| w["name"].as_str()).map(str::to_owned),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_call_reaches_only_its_own_workspace() {
        let w = "cw_ab12";
        let ok = [
            (Method::GET, "workspaces/cw_ab12"),
            (Method::GET, "workspaces/cw_ab12/tabs"),
            (Method::POST, "workspaces/cw_ab12/tabs"),
            (Method::GET, "workspaces/cw_ab12/tabs/cwc_ff/events?after=3"),
            (Method::POST, "workspaces/cw_ab12/tabs/cwc_ff/requests/r%201"),
            (Method::DELETE, "workspaces/cw_ab12/tabs/cwc_ff"),
            (Method::GET, "workspaces/cw_ab12/diff?path=%2Fhome%2Fsprite%2Fapp&ref=HEAD"),
            (Method::GET, "agents"),
            (Method::GET, "models"),
        ];
        for (m, p) in ok {
            assert!(allowed(w, &m, p).is_ok(), "{m} {p}");
        }
        let refused = [
            (Method::DELETE, "workspaces/cw_ab12"),
            (Method::GET, "workspaces/cw_ab13/tabs"),
            (Method::GET, "workspaces"),
            (Method::GET, "workspaces/cw_ab12/../cw_cd"),
            (Method::GET, "workspaces/cw_ab12/%2e%2e/cw_cd"),
            (Method::GET, "workspaces/cw_ab12/tabs%2F..%2F..%2Fx"),
            (Method::GET, "workspaces/cw_ab12//tabs"),
            (Method::GET, "secrets"),
            (Method::GET, "past-tabs"),
            (Method::POST, "agents"),
            (Method::PUT, "workspaces/cw_ab12/tabs"),
        ];
        for (m, p) in refused {
            assert!(allowed(w, &m, p).is_err(), "{m} {p}");
        }
    }

    #[test]
    fn a_config_holds_controls_ids_only() {
        let c = Config::parse(json!({ "cloud_workspace": " cw_ab ", "tab": "cwc_01" })).unwrap();
        assert_eq!((c.cloud_workspace.as_str(), c.tab.as_deref()), ("cw_ab", Some("cwc_01")));
        assert!(Config::parse(json!({ "cloud_workspace": "cw_../x" })).is_err());
        assert!(Config::parse(json!({ "cloud_workspace": "cw_ab", "tab": "cw_ab" })).is_err());
        assert!(Config::parse(json!({})).is_err());
    }

    #[test]
    fn its_status_reads_as_the_page_says_it() {
        assert_eq!(
            status_word(&json!({"status": "provisioning", "setup_step": "setup_script"})),
            "setting up (setup script)"
        );
        assert_eq!(status_word(&json!({"status": "ready", "sandbox_status": "suspended"})), "parked");
        assert_eq!(status_word(&json!({"status": "failed"})), "failed");
    }
}
