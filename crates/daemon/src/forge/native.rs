//! M203 (#1274): a project's own issue (M194) as an issue block. It's a
//! pin, `pin: {kind: "issue", global_id: "iss_…"}` in its config (kept in
//! `layout.json`), onto the platform's issue: nothing of it is kept here
//! but its id, number, project and title.
//! - **Whose it is.** It's read as the machine's owner, and its state goes
//!   to everyone who sees the pane (teammates, guests), so the state says
//!   only its id and number. The issue itself (and the kept title) is the
//!   owner's, by the `native_issue` call (`authz`: the owner's only).
//!   Anyone else reads it as themselves on the platform, or sees `#N`.
//!
//! - **Opening one.** `show` kind issue and `arugula open` with `iss_…`,
//!   or `#N` in a project ([`open_config`]). MCP's `#N` is the platform's
//!   when `project` is given, or when it isn't in a clone (the owner's
//!   personal project, as `arugula issue` reads it, M199); in a clone it's
//!   the clone's forge's, as before.
//! - **Reading.** The issue, as this machine for its owner (`GET
//!   /api/daemon/issues/{id}`): the owner's membership of the project, and
//!   the machine's `threads.read` grant over it, as any read by its agents
//!   (M197, M200's routes). Without the grant the block says how to give
//!   it; a person looking at it on the web reads it as themselves anyway
//!   (the shell's issue view, which is the platform page's). Read once when
//!   it opens, then every minute while a client draws it, and at once on
//!   `refresh` (the web's, when it hears the issue changed).
//! - **Unreachable.** What the platform said last stays, marked stale
//!   (`pin.stale`) with why, until it answers again. There's nothing else
//!   to read a native issue from.

use std::sync::{Arc, Mutex, Weak};

use arugula_control_wire::issues::{Issue, agent_issue, agent_project_issue};
use arugula_proto::{BlockType, forge::ForgePin};
use futures_util::future::BoxFuture;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{intervals, model::ItemKind};
use crate::{
    block::{Block, BlockCtx, Summary, no_method},
    control::CloudRefused,
    review::Live,
    store::now_ms,
};

/// How often a drawn block reads the issue again. Its reads count against
/// the machine's agents' (M200's hourly budget); a page that hears the
/// issue change asks for one at once (`refresh`).
const EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// A native issue's id: `iss_` and 32 hex characters.
pub fn is_issue_id(s: &str) -> bool {
    s.strip_prefix("iss_").is_some_and(|h| h.len() == 32 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The native issue a block's config is pinned to, if it's one.
pub fn pinned(config: &Value) -> Option<&str> {
    let pin = &config["pin"];
    (pin["kind"] == "issue").then(|| pin["global_id"].as_str()).flatten().filter(|id| is_issue_id(id))
}

/// A project as a path takes it: its id, or `personal`.
fn project_ok(p: &str) -> Result<&str, String> {
    let ok = p == "personal"
        || p.strip_prefix("prj_")
            .is_some_and(|h| !h.is_empty() && h.len() <= 64 && h.bytes().all(|b| b.is_ascii_hexdigit()));
    if ok { Ok(p) } else { Err(format!("{p:?} isn't a project: its id (prj_…) or personal")) }
}

/// What `show` kind issue (or the shell's Open issue) means by its
/// `issue`, when it's a native issue: `iss_…`, or `#N` / `N` with a
/// `project`, or outside any clone. `None`: the forge's, as before.
pub async fn open_config(c: &Value) -> Result<Option<Value>, String> {
    if let Some(id) = pinned(c) {
        return Ok(Some(config(id, None)));
    }
    let Some(what) = c["issue"].as_str().map(str::trim) else { return Ok(None) };
    if is_issue_id(what) {
        return Ok(Some(config(what, None)));
    }
    let Ok(n) = what.strip_prefix('#').unwrap_or(what).parse::<u64>() else { return Ok(None) };
    let project = match c["project"].as_str().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => project_ok(p)?.to_owned(),
        // In a clone, N is its forge's issue (M37), as it always was.
        None if in_a_clone(c["dir"].as_str()).await => return Ok(None),
        None => "personal".to_owned(),
    };
    let control = super::live::control().ok_or(CloudRefused::NotJoined.to_string())?;
    let v = control
        .cloud_now(Method::GET, &agent_project_issue(&project, n), None)
        .await
        .map_err(|e| format!("#{n} in {project}: {e}"))?;
    let issue: Issue = serde_json::from_value(v).map_err(|e| format!("the platform's issue: {e}"))?;
    if !is_issue_id(&issue.id) {
        return Err(format!("the platform's issue has an id that isn't one: {:?}", issue.id));
    }
    Ok(Some(config(&issue.id, Some(&issue))))
}

async fn in_a_clone(dir: Option<&str>) -> bool {
    match dir.filter(|d| !d.is_empty()) {
        Some(d) => super::remote_of(d).await.is_ok(),
        None => false,
    }
}

fn config(id: &str, issue: Option<&Issue>) -> Value {
    let mut c = json!({ "kind": "issue", "pin": { "kind": "issue", "global_id": id } });
    if let Some(i) = issue {
        c["number"] = json!(i.number);
        c["project"] = json!(i.project);
        c["title"] = json!(i.title);
    }
    c
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    #[serde(default = "issue_kind")]
    kind: ItemKind,
    pin: ForgePin,
    /// What it was when last read, for its tab before the next read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    number: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

fn issue_kind() -> ItemKind {
    ItemKind::Issue
}

/// What clients draw, which goes to everyone who sees the pane: how the
/// read stands, and nothing of the issue but its number.
#[derive(Debug, Clone, Serialize)]
struct State {
    kind: ItemKind,
    /// A project's own issue (M203), not a forge's: the web draws the
    /// platform page's issue view.
    native: bool,
    /// The pin, with `stale` while the platform can't be reached.
    pin: ForgePin,
    number: Option<u64>,
    /// This machine holds a read of it, for its owner (`native_issue`).
    read: bool,
    /// When the platform last answered (ms).
    read_ms: Option<u64>,
    /// Why it isn't shown: no grant, not the owner's, gone.
    error: Option<String>,
    /// Not read yet.
    loading: bool,
    watching: bool,
}

pub struct NativeIssue {
    ctx: BlockCtx,
    me: Weak<NativeIssue>,
    config: Mutex<Config>,
    state: Mutex<State>,
    /// The platform's issue, as this machine read it last: the owner's.
    issue: Mutex<Option<Issue>>,
    live: Live,
    reading: tokio::sync::Mutex<()>,
}

impl NativeIssue {
    pub fn create(ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        let mut config: Config = serde_json::from_value(config).map_err(|e| format!("issue config: {e}"))?;
        if !is_issue_id(&config.pin.global_id) || config.pin.kind != ItemKind::Issue {
            return Err(format!("{:?} isn't a native issue's id (iss_…)", config.pin.global_id));
        }
        config.kind = ItemKind::Issue;
        config.pin.stale = None;
        let state = State {
            kind: ItemKind::Issue,
            native: true,
            pin: config.pin.clone(),
            number: config.number,
            read: false,
            read_ms: None,
            error: None,
            loading: true,
            watching: false,
        };
        let b = Arc::new_cyclic(|me| Self {
            ctx,
            me: me.clone(),
            config: Mutex::new(config),
            state: Mutex::new(state),
            issue: Mutex::new(None),
            live: Live::default(),
            reading: tokio::sync::Mutex::new(()),
        });
        // Read once, drawn or not: `show` answers with it.
        let me = b.clone();
        crate::task::spawn_on(&b.ctx.rt, async move { me.read().await });
        Ok(b)
    }

    fn id(&self) -> String {
        self.config.lock().unwrap().pin.global_id.clone()
    }

    /// Read the issue again, as this machine for its owner.
    async fn read(&self) {
        let _one = self.reading.lock().await;
        let got = match super::live::control() {
            Some(c) => c.cloud_now(Method::GET, &agent_issue(&self.id()), None).await,
            None => Err(CloudRefused::NotJoined),
        };
        self.took(got.map_err(Into::into).and_then(|v| {
            serde_json::from_value::<Issue>(v).map_err(|e| Got::Refused(format!("the platform's issue: {e}")))
        }));
        self.ctx.changed();
    }

    fn took(&self, got: Result<Issue, Got>) {
        let mut st = self.state.lock().unwrap();
        let mut issue = self.issue.lock().unwrap();
        st.loading = false;
        match got {
            Ok(i) => {
                {
                    let mut c = self.config.lock().unwrap();
                    c.number = Some(i.number);
                    c.project = Some(i.project.clone());
                    c.title = Some(i.title.clone());
                }
                st.number = Some(i.number);
                st.read = true;
                st.read_ms = Some(now_ms());
                st.error = None;
                st.pin.stale = None;
                *issue = Some(i);
            }
            // What it said last stays: the platform may be back next read.
            Err(Got::Later(why)) => st.pin.stale = Some(why),
            // Refused (no grant, not the owner's any more, gone): nothing
            // of it stays, here or in `layout.json`, and the refusal says
            // why.
            Err(Got::Refused(why)) => {
                {
                    let mut c = self.config.lock().unwrap();
                    (c.number, c.project, c.title) = (None, None, None);
                }
                *issue = None;
                st.number = None;
                st.read = false;
                st.error = Some(why);
                st.pin.stale = None;
            }
        }
    }

    /// What the owner sees of it (`native_issue`): the issue as this
    /// machine read it last, else what's kept of it, and how that stands.
    fn owners(&self) -> Value {
        let c = self.config.lock().unwrap().clone();
        let st = self.state.lock().unwrap();
        json!({
            "issue": *self.issue.lock().unwrap(),
            "number": c.number,
            "project": c.project,
            "title": c.title,
            "read_ms": st.read_ms,
            "stale": st.pin.stale,
            "error": st.error,
        })
    }

    fn watch(&self, round: u64) {
        let Some(me) = self.me.upgrade() else { return };
        let every = EVERY.min(intervals().0 * 12);
        crate::task::spawn_on(&self.ctx.rt, async move {
            while me.live.on(round) {
                me.read().await;
                tokio::time::sleep(every).await;
            }
        });
    }

    /// Its line for everyone who sees the pane: its number and id, never
    /// its title.
    fn line(&self) -> String {
        let st = self.state.lock().unwrap();
        let id = &st.pin.global_id;
        match st.number {
            Some(n) => format!("#{n} · {id}"),
            None => format!("Issue {id}"),
        }
    }
}

/// Why a read didn't give the issue.
enum Got {
    /// The platform (or control) couldn't be reached, or was down.
    Later(String),
    /// It answered, and refused, or what it said wasn't an issue.
    Refused(String),
}

impl From<CloudRefused> for Got {
    fn from(e: CloudRefused) -> Self {
        match e {
            CloudRefused::Unreachable(_) | CloudRefused::Maintenance => Got::Later(e.to_string()),
            CloudRefused::Said { status: 429, said, .. } => Got::Later(said),
            CloudRefused::Said { status, said, .. } if status >= 500 => {
                Got::Later(format!("Arugula control's platform didn't answer: {said}"))
            }
            CloudRefused::Said { said, .. } => Got::Refused(said),
            CloudRefused::NotJoined => {
                Got::Refused("this machine hasn't joined Arugula control, where the project's issues are".into())
            }
            e => Got::Refused(e.to_string()),
        }
    }
}

impl Block for NativeIssue {
    fn kind(&self) -> BlockType {
        BlockType::Forge
    }

    fn config(&self) -> Value {
        serde_json::to_value(&*self.config.lock().unwrap()).unwrap_or_default()
    }

    fn state(&self) -> Value {
        serde_json::to_value(&*self.state.lock().unwrap()).unwrap_or_default()
    }

    /// As `capture --text` gives it, to whoever may capture the pane: no
    /// title or body (the owner's, by `native_issue`).
    fn text(&self) -> String {
        let mut out = format!("{}\n", self.line());
        let st = self.state.lock().unwrap();
        if let Some(s) = &st.pin.stale {
            out.push_str(&format!("(as Arugula control said it last: {s})\n"));
        }
        if let Some(e) = &st.error {
            out.push_str(&format!("error: {e}\n"));
        }
        out
    }

    fn call(&self, method: &str, _args: Value) -> BoxFuture<'static, Result<Value, String>> {
        let Some(me) = self.me.upgrade() else {
            return Box::pin(async { Err("the block closed".to_owned()) });
        };
        match method {
            "refresh" => Box::pin(async move {
                me.read().await;
                Ok(me.state())
            }),
            "state" => Box::pin(async move { Ok(me.state()) }),
            // The owner's only (`authz`, and the handler for another
            // account's devices).
            "native_issue" => Box::pin(async move { Ok(me.owners()) }),
            m => {
                let e = format!(
                    "{}: a project's own issue changes on the platform page, with `arugula issue`, or through MCP",
                    no_method(BlockType::Forge, m)
                );
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
        Summary { title: st.number.map(|n| format!("#{n}")), ..Default::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "iss_0123456789abcdef0123456789abcdef";

    #[test]
    fn a_pin_is_a_native_issues_id_only() {
        assert!(is_issue_id(ID));
        assert!(!is_issue_id("iss_0123"));
        assert!(!is_issue_id("iss_0123456789abcdef0123456789abcdeg"));
        assert!(!is_issue_id("fi_0123456789abcdef0123456789abcdef"));
        assert_eq!(pinned(&json!({ "pin": { "kind": "issue", "global_id": ID } })), Some(ID));
        assert_eq!(pinned(&json!({ "pin": { "kind": "pr", "global_id": ID } })), None);
        assert_eq!(pinned(&json!({ "pin": { "kind": "issue", "global_id": "fi_00" } })), None, "M172's pin");
        assert_eq!(pinned(&json!({ "repo": "acme/app", "number": 1 })), None);
    }

    #[tokio::test]
    async fn what_shows_opens_as_a_native_issue() {
        let c = |v: Value| async move { open_config(&v).await };
        let pin = c(json!({ "issue": ID })).await.unwrap().unwrap();
        assert_eq!(pin, json!({ "kind": "issue", "pin": { "kind": "issue", "global_id": ID } }));
        assert_eq!(pinned(&pin), Some(ID));
        // The forge's, as before: a link, OWNER/REPO#N, new.
        for issue in ["https://github.com/acme/app/issues/3", "acme/app#3", "new"] {
            assert_eq!(c(json!({ "issue": issue })).await.unwrap(), None, "{issue}");
        }
        // N in this clone is its forge's.
        let here = env!("CARGO_MANIFEST_DIR");
        assert_eq!(c(json!({ "issue": "#3", "dir": here })).await.unwrap(), None);
        // A project that isn't one is refused before anything is asked.
        assert!(c(json!({ "issue": "#3", "project": "../x" })).await.is_err());
    }

    #[test]
    fn a_config_from_an_older_layout_reads() {
        let cfg: Config = serde_json::from_value(json!({ "pin": { "kind": "issue", "global_id": ID } })).unwrap();
        assert_eq!((cfg.kind, cfg.number), (ItemKind::Issue, None));
    }
}
