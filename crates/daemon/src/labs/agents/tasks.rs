//! Tasks for this machine's offered agents (M78 #400, with M79 #401's
//! consent), over A2A 1.0's JSON-RPC binding at `POST /api/a2a/agents/NAME`:
//! `SendMessage`, `GetTask`, `ListTasks` and `CancelTask`, ported from S34.
//!
//! - **Who's asking** is the account of the device on the channel
//!   (`e2e::Caller`), not its role here: every owner of a team is `Owner`
//!   on its machines. A request with no channel (this machine's own socket
//!   or loopback) is this machine's own account.
//! - **A task** runs the recipe as an agent block (session `a2a`) in a git
//!   worktree of the offer's project, detached at its `HEAD`. It ends with
//!   the agent's last reply and a patch against that commit, as artifacts.
//! - **Or a pull request** (`deliver: pr` in the message's `arugula`
//!   metadata): the worktree is a new branch, `a2a/AGENT-TASK`, off the
//!   project's origin's default branch as just fetched, and the agent is
//!   told (after its prompt) to commit there, push and open a pull request
//!   with the project's own tooling, as this machine's owner, without
//!   merging it. The reply names the pull request: a `pr` artifact carries
//!   its link and the branch. The patch comes too, in case it didn't.
//! - **Consent (M79):** a task from another account waits (`SUBMITTED`)
//!   until this machine's own account allows it on a card, held by the
//!   Team agents block (opened in session `a2a` when none is) and pushed:
//!   *Allow once*, *Allow for an hour* (that account and agent: a grant,
//!   listed and revocable) or *Deny* (`REJECTED`). A team's other owner
//!   can't answer it: answers are checked by account.
//! - **Inside a task**, a permission the agent asks for is this machine's
//!   owner's (the task stays `WORKING` and says what it waits on), and a
//!   question is the caller's (`INPUT_REQUIRED`, answered by the next
//!   `SendMessage` on the task).
//! - **Durable:** each task is `<state>/a2a/tasks/ID.json`, written on every
//!   change; a restarted daemon follows those still running and asks again
//!   for those still waiting. Grants are `<state>/a2a/grants.json`.
//! - **Cleaned up** once its caller has read the ended task (not when this
//!   machine's owner looks at someone else's): its block closes and its
//!   worktree goes, and a pull request's branch too once it's pushed.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use arugula_proto::{BlockType, PaneId, api::OpenRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{oneshot, watch};
use tracing::{info, warn};

use crate::{mux::Api, server::App, store::now_ms};

/// Permission modes that skip every check: never for a task (S34).
const SKIPS_CHECKS: [&str; 3] = ["bypassPermissions", "dontAsk", "full-access"];

/// "Allow for an hour".
pub const GRANT_MS: u64 = 3_600_000;

pub const SUBMITTED: &str = "TASK_STATE_SUBMITTED";
pub const WORKING: &str = "TASK_STATE_WORKING";
pub const INPUT_REQUIRED: &str = "TASK_STATE_INPUT_REQUIRED";
pub const COMPLETED: &str = "TASK_STATE_COMPLETED";
pub const FAILED: &str = "TASK_STATE_FAILED";
pub const CANCELED: &str = "TASK_STATE_CANCELED";
pub const REJECTED: &str = "TASK_STATE_REJECTED";
const TERMINAL: [&str; 4] = [COMPLETED, FAILED, CANCELED, REJECTED];

/// Who sent a task.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Asker {
    /// Its account; empty for this machine's own.
    pub account: String,
    pub name: String,
    /// The device on the channel, proven by its key: `name (id)`.
    #[serde(default)]
    pub device: Option<String>,
    /// What its message says of where it came from (machine, pane): a
    /// claim, shown as one.
    #[serde(default)]
    pub claims: Value,
}

impl Asker {
    pub fn own(&self) -> bool {
        self.account.is_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub context: String,
    pub agent: String,
    pub caller: Asker,
    pub state: String,
    pub status: Option<String>,
    /// The first message's text, for the consent card.
    #[serde(default)]
    pub text: String,
    pub block: Option<PaneId>,
    pub worktree: Option<PathBuf>,
    /// The project it was offered from.
    #[serde(default)]
    pub dir: String,
    pub base: Option<String>,
    pub artifacts: Vec<Value>,
    pub history: Vec<Value>,
    pub at_ms: u64,
    /// Its caller read it once it ended: what it left behind can go.
    #[serde(default)]
    pub collected: bool,
    /// Delivered as a pull request, not only a patch.
    #[serde(default)]
    pub pr: bool,
    /// A pull request's branch, and what it's off (`origin/main`).
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub from: Option<String>,
}

struct Live {
    task: Task,
    tx: watch::Sender<u64>,
}

#[derive(Default)]
struct Store {
    dir: Option<PathBuf>,
    tasks: HashMap<String, Live>,
}

static STORE: LazyLock<Mutex<Store>> = LazyLock::new(Default::default);

/// Waiting for this machine's own account: the answer goes here.
static PENDING: LazyLock<Mutex<HashMap<String, oneshot::Sender<Answer>>>> = LazyLock::new(Default::default);

/// The owner's answer to a waiting task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Once,
    Hour,
    Deny,
}

impl Answer {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "once" | "allow" => Some(Answer::Once),
            "hour" => Some(Answer::Hour),
            "deny" => Some(Answer::Deny),
            _ => None,
        }
    }
}

fn tasks_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("a2a").join("tasks")
}

/// The tasks kept on disk, read once: those still running are followed
/// again, and those still waiting for the owner ask again. Run at startup.
pub fn resume(app: &Arc<App>) {
    let dir = tasks_dir(app.control.state_dir());
    let mut loaded = vec![];
    {
        let mut st = STORE.lock().unwrap();
        if st.dir.is_some() {
            return;
        }
        st.dir = Some(dir.clone());
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let Ok(b) = std::fs::read(e.path()) else { continue };
            let Ok(t) = serde_json::from_slice::<Task>(&b) else { continue };
            let (tx, _) = watch::channel(0);
            loaded.push(t.clone());
            st.tasks.insert(t.id.clone(), Live { task: t, tx });
        }
    }
    for t in loaded {
        match t.state.as_str() {
            SUBMITTED => {
                let app = app.clone();
                tokio::spawn(async move { after_consent(app, t.id.clone(), t.text.clone()).await });
            }
            WORKING => {
                let app = app.clone();
                tokio::spawn(async move { follow(app, t.id).await });
            }
            _ => {}
        }
    }
}

fn ensure_loaded(app: &Arc<App>) {
    if STORE.lock().unwrap().dir.is_none() {
        resume(app);
    }
}

fn save(t: &Task) {
    let Some(dir) = STORE.lock().unwrap().dir.clone() else { return };
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(b) = serde_json::to_vec_pretty(t)
        && let Err(e) = crate::store::write_atomic(&dir.join(format!("{}.json", t.id)), &b)
    {
        warn!(task = t.id, error = %e, "can't keep the task");
    }
}

fn update(id: &str, f: impl FnOnce(&mut Task)) {
    let saved = {
        let mut st = STORE.lock().unwrap();
        let Some(l) = st.tasks.get_mut(id) else { return };
        f(&mut l.task);
        l.tx.send_modify(|v| *v += 1);
        l.task.clone()
    };
    save(&saved);
}

pub fn snapshot(id: &str) -> Option<Task> {
    STORE.lock().unwrap().tasks.get(id).map(|l| l.task.clone())
}

/// Tasks waiting for this machine's own account, oldest first.
pub fn waiting() -> Vec<Task> {
    let ids: Vec<String> = PENDING.lock().unwrap().keys().cloned().collect();
    let mut v: Vec<Task> = ids.iter().filter_map(|i| snapshot(i)).collect();
    v.sort_by_key(|t| t.at_ms);
    v
}

/// This machine's own account's answer to a waiting task.
pub fn answer(id: &str, a: Answer) -> Result<(), String> {
    let tx = PENDING.lock().unwrap().remove(id).ok_or("no task waits for that (it was answered, or ended)")?;
    let _ = tx.send(a);
    Ok(())
}

// ---------------------------------------------------------------- grants

/// A standing "allow for an hour": this account's tasks for this agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub account: String,
    pub name: String,
    pub agent: String,
    pub until_ms: u64,
}

fn grants_file(state_dir: &Path) -> PathBuf {
    state_dir.join("a2a").join("grants.json")
}

/// The grants that still stand.
pub fn grants(state_dir: &Path) -> Vec<Grant> {
    let all: Vec<Grant> =
        std::fs::read(grants_file(state_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    all.into_iter().filter(|g| g.until_ms > now_ms()).collect()
}

fn save_grants(state_dir: &Path, g: &[Grant]) -> Result<(), String> {
    let _ = std::fs::create_dir_all(state_dir.join("a2a"));
    let b = serde_json::to_vec_pretty(g).map_err(|e| e.to_string())?;
    crate::store::write_atomic(&grants_file(state_dir), &b).map_err(|e| format!("can't keep the grant: {e}"))
}

fn grant(state_dir: &Path, caller: &Asker, agent: &str) {
    let mut g = grants(state_dir);
    g.retain(|x| !(x.account == caller.account && x.agent == agent));
    g.push(Grant {
        account: caller.account.clone(),
        name: caller.name.clone(),
        agent: agent.to_owned(),
        until_ms: now_ms() + GRANT_MS,
    });
    if let Err(e) = save_grants(state_dir, &g) {
        warn!(error = e, "grant not kept");
    }
}

/// Take a grant back: the next task from that account for that agent asks
/// again. True if there was one.
pub fn revoke(state_dir: &Path, account: &str, agent: &str) -> Result<bool, String> {
    let mut g = grants(state_dir);
    let before = g.len();
    g.retain(|x| !(x.account == account && x.agent == agent));
    save_grants(state_dir, &g)?;
    Ok(g.len() != before)
}

fn granted(state_dir: &Path, caller: &Asker, agent: &str) -> bool {
    grants(state_dir).iter().any(|g| g.account == caller.account && g.agent == agent)
}

// ---------------------------------------------------------------- A2A JSON

fn agent_message(text: &str, task: &str, context: &str) -> Value {
    json!({
        "messageId": format!("m{}", now_ms()),
        "role": "ROLE_AGENT",
        "taskId": task,
        "contextId": context,
        "parts": [{ "text": text }],
    })
}

impl Task {
    /// As A2A 1.0 has it (`historyLength`: how many of its messages).
    pub fn json(&self, history: Option<usize>) -> Value {
        let mut status = json!({ "state": self.state, "timestamp": iso(now_ms()) });
        if let Some(s) = &self.status {
            status["message"] = agent_message(s, &self.id, &self.context);
        }
        let mut t = json!({
            "id": self.id,
            "contextId": self.context,
            "status": status,
            "artifacts": self.artifacts,
            "metadata": { "arugula": {
                "agent": self.agent,
                "block": self.block,
                "base": self.base,
                "caller": self.caller.name,
                "at_ms": self.at_ms,
                "deliver": if self.pr { "pr" } else { "patch" },
                "branch": self.branch,
            } },
        });
        let h: Vec<Value> = match history {
            Some(0) => vec![],
            Some(n) => self.history.iter().rev().take(n).rev().cloned().collect(),
            None => self.history.clone(),
        };
        if !h.is_empty() {
            t["history"] = json!(h);
        }
        t
    }
}

pub fn iso(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil from days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, ms % 1000)
}

fn ok(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn text_of(message: &Value) -> String {
    message["parts"].as_array().into_iter().flatten().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n")
}

/// Who's asking: the account of the device on the channel, or this
/// machine's own.
pub fn asker(app: &App, dev: Option<&crate::e2e::Caller>, claims: Value) -> Asker {
    let own = app.control.enrolled().map(|e| e.saved.cert.account.clone());
    match dev {
        Some(d) if Some(&d.account) != own.as_ref() => Asker {
            account: d.account.clone(),
            name: app.control.name_of_account(&d.account).unwrap_or_else(|| d.account.clone()),
            device: Some(format!("{} ({})", d.name, d.device)),
            claims,
        },
        _ => Asker {
            name: "this machine's owner".into(),
            device: dev.map(|d| d.name.clone()),
            claims,
            ..Default::default()
        },
    }
}

fn may_see(c: &Asker, t: &Task) -> bool {
    c.own() || c.account == t.caller.account
}

/// One JSON-RPC call on the agent `name`, offered from `dir`.
pub async fn rpc(app: Arc<App>, name: &str, dir: &str, caller: Asker, req: Value) -> Value {
    ensure_loaded(&app);
    let id = req["id"].clone();
    let p = &req["params"];
    match req["method"].as_str().unwrap_or_default() {
        "SendMessage" => send(app, id, name, dir, caller, p).await,
        "GetTask" => {
            let Some(t) = p["id"].as_str().and_then(snapshot).filter(|t| may_see(&caller, t) && t.agent == name) else {
                return err(&id, -32001, "no such task");
            };
            if TERMINAL.contains(&t.state.as_str()) && !t.collected && caller.account == t.caller.account {
                update(&t.id, |x| x.collected = true);
                tokio::spawn(clean_up(app.clone(), t.id.clone()));
            }
            ok(&id, t.json(p["historyLength"].as_u64().map(|n| n as usize)))
        }
        "ListTasks" => {
            let mut all: Vec<Task> = STORE
                .lock()
                .unwrap()
                .tasks
                .values()
                .filter(|l| l.task.agent == name && may_see(&caller, &l.task))
                .map(|l| l.task.clone())
                .collect();
            all.sort_by_key(|t| std::cmp::Reverse(t.at_ms));
            let v: Vec<Value> = all.iter().take(50).map(|t| t.json(Some(0))).collect();
            ok(&id, json!({ "tasks": v, "nextPageToken": "" }))
        }
        "CancelTask" => {
            let Some(t) = p["id"].as_str().and_then(snapshot).filter(|t| may_see(&caller, t) && t.agent == name) else {
                return err(&id, -32001, "no such task");
            };
            if TERMINAL.contains(&t.state.as_str()) {
                return err(&id, -32002, "it has already ended");
            }
            cancel(&app, &t, &caller.name).await;
            ok(&id, snapshot(&t.id).map(|t| t.json(Some(0))).unwrap_or_default())
        }
        m => err(&id, -32601, &format!("{m}: not a method this speaks (SendMessage, GetTask, ListTasks, CancelTask)")),
    }
}

/// Stop a task, by whoever: its turn canceled, and it's `CANCELED`.
pub async fn cancel(app: &App, t: &Task, by: &str) {
    // `CANCELED` first: a task waiting on its owner wakes to the Deny below
    // and must find it ended, not still `SUBMITTED` to call rejected (#719).
    update(&t.id, |x| {
        x.state = CANCELED.into();
        x.status = Some(format!("canceled by {by}"));
    });
    let was_waiting = PENDING.lock().unwrap().remove(&t.id);
    if let Some(tx) = was_waiting {
        let _ = tx.send(Answer::Deny);
        super::block::task_gone(&t.id);
    }
    if let Some(b) = t.block
        && let Some(Some(block)) = app.mux.api(|r| Api::Block(b, r)).await
    {
        let _ = block.call("cancel", json!({})).await;
    }
    info!(task = t.id, by, "A2A task canceled");
}

async fn send(app: Arc<App>, id: Value, name: &str, dir: &str, caller: Asker, p: &Value) -> Value {
    let message = p["message"].clone();
    let text = text_of(&message);
    if text.trim().is_empty() {
        return err(&id, -32602, "the message has no text part");
    }
    // A follow-up on a task this caller has: the next turn, or the answer it
    // was waiting for.
    let task_id = if let Some(t) = message["taskId"].as_str() {
        let Some(task) = snapshot(t).filter(|x| may_see(&caller, x) && x.agent == name) else {
            return err(&id, -32001, "no such task");
        };
        if task.state != INPUT_REQUIRED && task.state != COMPLETED {
            return err(&id, -32602, &format!("the task is {}; send to it once it waits", task.state));
        }
        if task.collected {
            return err(&id, -32602, "that task was collected and cleaned up: send a new one");
        }
        update(t, |x| {
            x.history.push(message.clone());
            x.state = WORKING.into();
            x.status = None;
        });
        let answering = task.state == INPUT_REQUIRED;
        tokio::spawn(turn(app.clone(), t.to_owned(), text, answering));
        t.to_owned()
    } else {
        let t = format!("t{}{:04x}", now_ms(), rand_u16());
        let context = message["contextId"].as_str().map(str::to_owned).unwrap_or_else(|| format!("c-{t}"));
        let task = Task {
            id: t.clone(),
            context,
            agent: name.to_owned(),
            caller: caller.clone(),
            state: SUBMITTED.into(),
            text: text.clone(),
            dir: dir.to_owned(),
            pr: message["metadata"]["arugula"]["deliver"] == "pr",
            history: vec![message.clone()],
            at_ms: now_ms(),
            ..Default::default()
        };
        let (tx, _) = watch::channel(0);
        STORE.lock().unwrap().tasks.insert(t.clone(), Live { task: task.clone(), tx });
        save(&task);
        info!(task = t, agent = name, caller = caller.name, account = caller.account, "A2A task");
        tokio::spawn(after_consent(app.clone(), t.clone(), text));
        t
    };
    // Until it ends or waits on someone, unless asked not to wait (and at
    // most a little: a channel request doesn't wait long).
    if !p["configuration"]["returnImmediately"].as_bool().unwrap_or(false) {
        let rx = STORE.lock().unwrap().tasks.get(&task_id).map(|l| l.tx.subscribe());
        if let Some(mut rx) = rx {
            let _ = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let s = snapshot(&task_id).map(|t| t.state).unwrap_or_default();
                    if TERMINAL.contains(&s.as_str()) || s == INPUT_REQUIRED {
                        break;
                    }
                    if rx.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await;
        }
    }
    ok(&id, json!({ "task": snapshot(&task_id).map(|t| t.json(Some(0))) }))
}

fn rand_u16() -> u16 {
    let b = arugula_e2e::random::<2>();
    u16::from_be_bytes(b)
}

pub(super) async fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn fail(t: &str, why: String) {
    warn!(task = t, why, "A2A task failed");
    update(t, |x| {
        x.state = FAILED.into();
        x.status = Some(why);
    });
}

/// The owner's OK (or a grant, or the caller being this machine's own
/// account), then the task starts.
async fn after_consent(app: Arc<App>, t: String, text: String) {
    let Some(task) = snapshot(&t) else { return };
    let state = app.control.state_dir().to_owned();
    if !task.caller.own() && !granted(&state, &task.caller, &task.agent) {
        update(&t, |x| x.status = Some(format!("waiting for this machine's owner to allow {}'s task", x.caller.name)));
        let (tx, rx) = oneshot::channel();
        PENDING.lock().unwrap().insert(t.clone(), tx);
        super::block::consent_needed(&app).await;
        info!(task = t, by = task.caller.name, device = ?task.caller.device, "A2A task waits for the owner");
        match rx.await {
            Ok(Answer::Once) => {}
            Ok(Answer::Hour) => grant(&state, &task.caller, &task.agent),
            Ok(Answer::Deny) | Err(_) => {
                // Checked under the store's lock: a cancel may land between
                // a look and a write.
                update(&t, |x| {
                    if x.state == SUBMITTED {
                        x.state = REJECTED.into();
                        x.status = Some("this machine's owner said no".into());
                    }
                });
                return;
            }
        }
        super::block::consent_settled(&app);
    }
    if snapshot(&t).is_none_or(|x| x.state != SUBMITTED) {
        return;
    }
    if let Err(e) = start(&app, &t).await {
        return fail(&t, e);
    }
    update(&t, |x| {
        x.state = WORKING.into();
        x.status = None;
    });
    turn(app, t, text, false).await;
}

/// The branch an origin's pull requests go to, as `origin/NAME`: what its
/// `HEAD` names, else `main` or `master`.
async fn default_branch(dir: &Path) -> Result<String, String> {
    if let Ok(r) = git(dir, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]).await {
        return Ok(r.trim().to_owned());
    }
    for b in ["origin/main", "origin/master"] {
        if git(dir, &["rev-parse", "--verify", "--quiet", &format!("{b}^{{commit}}")]).await.is_ok() {
            return Ok(b.to_owned());
        }
    }
    Err("its origin has no default branch (origin/HEAD, main or master) to open a pull request against".into())
}

/// What a pull request's agent is told, after its own prompt.
fn pr_brief(dir: &str, branch: &str, from: &str, base: &str) -> String {
    let short: String = base.chars().take(12).collect();
    format!(
        "Deliver this task as a pull request. You're in a fresh worktree of {dir} on the branch `{branch}`, \
         made from {from} ({short}) as just fetched: work here, not in another checkout or worktree. Commit your \
         work on this branch, push it to origin, and open a pull request against {} with this project's own \
         tooling and conventions (its contributing guide or CLAUDE.md says how). Don't merge it. End your reply \
         with the pull request's URL, or with why there isn't one.",
        from.trim_start_matches("origin/")
    )
}

/// A worktree of the offer's project and an agent block wearing the recipe.
async fn start(app: &Arc<App>, t: &str) -> Result<(), String> {
    let task = snapshot(t).ok_or("gone")?;
    let dir = PathBuf::from(&task.dir);
    let wt = app.control.state_dir().join("a2a").join("work").join(t);
    let mut brief = None;
    let base = if task.pr {
        git(&dir, &["fetch", "--quiet", "origin"])
            .await
            .map_err(|e| format!("a pull request needs its origin: {e}"))?;
        let from = default_branch(&dir).await?;
        let base = git(&dir, &["rev-parse", &format!("{from}^{{commit}}")]).await?.trim().to_owned();
        let branch = format!("a2a/{}-{t}", task.agent);
        git(&dir, &["worktree", "add", "-q", "-b", &branch, &wt.display().to_string(), &base]).await?;
        brief = Some(pr_brief(&task.dir, &branch, &from, &base));
        update(t, |x| {
            x.branch = Some(branch.clone());
            x.from = Some(from.clone());
        });
        base
    } else {
        let base = git(&dir, &["rev-parse", "HEAD"]).await?.trim().to_owned();
        git(&dir, &["worktree", "add", "-q", "--detach", &wt.display().to_string(), &base]).await?;
        base
    };
    let mut config = json!({ "agent": "claude", "recipe": task.agent, "cwd": wt.display().to_string() });
    if let Some(b) = brief {
        config["brief"] = json!(b);
    }
    // Someone else's task never runs with every check off: a recipe that
    // asks for that runs in the default mode instead (S34). Its other
    // modes (acceptEdits, plan) stand.
    let recipe = super::find(&dir, &crate::home(), &task.agent)?;
    if recipe.permission_mode.as_deref().is_some_and(|m| SKIPS_CHECKS.contains(&m)) {
        config["permission_mode"] = json!("default");
    }
    let req =
        OpenRequest { kind: BlockType::Agent, config, session: Some("a2a".into()), local: true, ..Default::default() };
    let block = match app.mux.api(|r| Api::Open(req, None, r)).await {
        Some(Ok(b)) => b,
        Some(Err(e)) => return Err(format!("opening its block: {e}")),
        None => return Err("the daemon is shutting down".into()),
    };
    update(t, |x| {
        x.block = Some(block);
        x.worktree = Some(wt.clone());
        x.base = Some(base.clone());
    });
    Ok(())
}

/// One turn of the task's agent, followed to where it ends.
async fn turn(app: Arc<App>, t: String, text: String, answering: bool) {
    use arugula_proto::api::PromptResult;
    let Some(task) = snapshot(&t) else { return };
    let Some(block) = task.block else { return fail(&t, "no block".into()) };
    // The receiver's permission cards say whose task they're for.
    let by = Some(format!("{}'s A2A task {t}", task.caller.name));
    // The recipe's worn first: that can take a while.
    match crate::ops::prompt::prompt(&app, block, text, answering, Duration::from_secs(300), by).await {
        Ok(PromptResult::Stalled { why, .. }) => return fail(&t, why),
        Err(e) => return fail(&t, e),
        Ok(_) => {}
    }
    follow(app, t).await
}

/// Watch a working task's block until its turn ends or waits on someone.
async fn follow(app: Arc<App>, t: String) {
    let Some(block) = snapshot(&t).and_then(|x| x.block) else { return fail(&t, "no block".into()) };
    loop {
        if snapshot(&t).is_none_or(|x| TERMINAL.contains(&x.state.as_str())) {
            return;
        }
        let Some(b) = app.mux.api(|r| Api::Block(block, r)).await.flatten() else {
            return fail(&t, "its block closed".into());
        };
        let s = b.state();
        let pending = s["pending"].as_array().map_or(!s["pending"].is_null(), |p| !p.is_empty());
        let question = s["asks"].as_array().into_iter().flatten().find(|a| a["accepted"] != true).cloned();
        match s["attention"].as_str().unwrap_or_default() {
            "working" => update(&t, |x| {
                if x.status.is_some() {
                    x.status = None;
                }
            }),
            _ if pending => {
                let what = s["pending"]
                    .pointer("/0/title")
                    .or(s["pending"].pointer("/title"))
                    .and_then(Value::as_str)
                    .unwrap_or("a permission")
                    .to_owned();
                update(&t, |x| x.status = Some(format!("waiting for this machine's owner to approve: {what}")));
            }
            _ if question.is_some() => {
                let q = question.unwrap_or_default();
                let msg = q["message"].as_str().filter(|m| !m.is_empty()).unwrap_or("it asks a question").to_owned();
                update(&t, |x| {
                    x.state = INPUT_REQUIRED.into();
                    x.status = Some(msg);
                });
                return;
            }
            _ if s["status"] == "exited" => {
                return fail(&t, s["error"].as_str().unwrap_or("the agent exited").to_owned());
            }
            _ if s["status"] == "starting" || s["wearing"] == true => {}
            _ => return finish(&app, &t, block).await,
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// The turn ended: its last reply, and the patch against where it started.
async fn finish(app: &App, t: &str, block: PaneId) {
    let Some(task) = snapshot(t) else { return };
    let reply = match app.mux.api(|r| Api::Block(block, r)).await.flatten() {
        Some(b) => b.state()["entries"]
            .as_array()
            .and_then(|e| {
                e.iter().rev().find(|e| e["type"] == "agent").and_then(|e| e["text"].as_str()).map(str::to_owned)
            })
            .unwrap_or_default(),
        None => String::new(),
    };
    let mut artifacts = vec![json!({
        "artifactId": format!("{t}-reply"),
        "name": "reply",
        "parts": [{ "text": reply, "mediaType": "text/markdown" }],
    })];
    if let (Some(wt), Some(base)) = (&task.worktree, &task.base)
        && let Some(p) = patch(t, wt, base).await
    {
        artifacts.push(p);
    }
    if let (Some(branch), Some(wt), Some(base)) = (&task.branch, &task.worktree, &task.base) {
        artifacts.push(pr_artifact(t, &reply, branch, task.from.as_deref().unwrap_or_default(), wt, base).await);
    }
    let short: String = reply.chars().take(2000).collect();
    let msg = agent_message(&short, t, &task.context);
    update(t, |x| {
        x.artifacts = artifacts;
        x.history.push(msg);
        x.state = COMPLETED.into();
        x.status = None;
    });
    info!(task = t, "A2A task completed");
}

/// The worktree's changes against `base`, as a patch artifact. New files
/// count (intent-to-add); new binary files don't (a test run's leavings,
/// like `__pycache__`, made a whole patch fail to apply in S34): they're
/// named instead.
async fn patch(t: &str, wt: &Path, base: &str) -> Option<Value> {
    let _ = git(wt, &["add", "-A", "-N", "."]).await;
    let tracked: std::collections::HashSet<String> =
        git(wt, &["ls-tree", "-r", "--name-only", base]).await.unwrap_or_default().lines().map(str::to_owned).collect();
    let mut left_out = vec![];
    for l in git(wt, &["diff", "--numstat", base]).await.unwrap_or_default().lines() {
        let f: Vec<&str> = l.splitn(3, '\t').collect();
        if let [a, b, path] = f.as_slice()
            && *a == "-"
            && *b == "-"
            && !tracked.contains(*path)
        {
            left_out.push((*path).to_owned());
        }
    }
    let mut args: Vec<String> = vec!["diff".into(), "--binary".into(), base.to_owned(), "--".into(), ".".into()];
    args.extend(left_out.iter().map(|p| format!(":(exclude,literal){p}")));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match git(wt, &args).await {
        Ok(p) if !p.is_empty() => {
            let mut stat_args = args.clone();
            stat_args.insert(1, "--stat");
            let stat = git(wt, &stat_args).await.unwrap_or_default();
            Some(json!({
                "artifactId": format!("{t}-patch"),
                "name": "patch",
                "description": format!("Changes against {base}"),
                "parts": [{ "text": p, "mediaType": "text/x-diff", "filename": format!("{t}.patch") }],
                "metadata": { "arugula": { "base": base, "stat": stat, "left_out": left_out } },
            }))
        }
        Ok(_) => None,
        Err(e) => {
            warn!(task = t, error = e, "no patch");
            None
        }
    }
}

/// The last pull request link in `reply`: GitHub's, Forgejo's and Gitea's
/// (`/pull/N`, `/pulls/N`), GitLab's (`/merge_requests/N`).
pub fn pr_link(reply: &str) -> Option<String> {
    reply
        .split(|c: char| c.is_whitespace() || "()<>[]\"'`".contains(c))
        .filter(|w| w.starts_with("https://") || w.starts_with("http://"))
        .map(|w| w.trim_end_matches(['.', ',', ';', ':', '!', '?']))
        .rfind(|w| {
            let mut parts = w.rsplit('/');
            let n = parts.next().unwrap_or_default();
            !n.is_empty()
                && n.chars().all(|c| c.is_ascii_digit())
                && matches!(parts.next(), Some("pull" | "pulls" | "merge_requests"))
        })
        .map(str::to_owned)
}

/// A pull request task's own artifact: its link (from the reply), its
/// branch, the commits on it and whether origin has them.
async fn pr_artifact(t: &str, reply: &str, branch: &str, from: &str, wt: &Path, base: &str) -> Value {
    let url = pr_link(reply);
    let commits: u64 = git(wt, &["rev-list", "--count", &format!("{base}..HEAD")])
        .await
        .ok()
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0);
    let pushed =
        match (git(wt, &["rev-parse", "HEAD"]).await, git(wt, &["rev-parse", &format!("origin/{branch}")]).await) {
            (Ok(a), Ok(b)) => a.trim() == b.trim(),
            _ => false,
        };
    let text = match &url {
        Some(u) => format!("{u} (branch {branch}, {commits} commit(s) off {from})"),
        None => format!(
            "No pull request link in its reply. Its branch is {branch}, {commits} commit(s) off {from}, {}.",
            if pushed { "pushed" } else { "not pushed" }
        ),
    };
    json!({
        "artifactId": format!("{t}-pr"),
        "name": "pr",
        "parts": [{ "text": text }],
        "metadata": { "arugula": { "url": url, "branch": branch, "from": from, "commits": commits, "pushed": pushed } },
    })
}

/// The caller has the result: close the task's block and drop its worktree
/// (and a pull request's branch here, once origin has it).
async fn clean_up(app: Arc<App>, t: String) {
    let Some(task) = snapshot(&t) else { return };
    if let Some(b) = task.block {
        let _ = app.mux.api(|r| Api::Close(b, r)).await;
    }
    let dir = PathBuf::from(&task.dir);
    if let Some(wt) = &task.worktree
        && let Err(e) = git(&dir, &["worktree", "remove", "--force", &wt.display().to_string()]).await
    {
        warn!(task = t, error = e, "worktree not removed");
    }
    // Unpushed, it stays: it's the work.
    if let Some(b) = &task.branch
        && let (Ok(here), Ok(there)) =
            (git(&dir, &["rev-parse", b]).await, git(&dir, &["rev-parse", &format!("origin/{b}")]).await)
        && here.trim() == there.trim()
    {
        let _ = git(&dir, &["branch", "-D", b]).await;
    }
    info!(task = t, "A2A task cleaned up");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_rfc3339() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_791_269_938_206), "2026-10-06T06:58:58.206Z");
    }

    #[test]
    fn answers_and_who_sees_a_task() {
        assert_eq!(Answer::parse("hour"), Some(Answer::Hour));
        assert_eq!(Answer::parse("allow"), Some(Answer::Once));
        assert_eq!(Answer::parse("x"), None);
        let ale = Asker { account: "a2".into(), name: "ale".into(), ..Default::default() };
        let bo = Asker { account: "a3".into(), name: "bo".into(), ..Default::default() };
        let t = Task { caller: ale.clone(), ..Default::default() };
        assert!(may_see(&ale, &t) && !may_see(&bo, &t));
        assert!(may_see(&Asker::default(), &t), "this machine's owner sees every task");
    }

    #[test]
    fn a_pull_request_link_is_the_last_one_in_the_reply() {
        assert_eq!(
            pr_link("Opened https://github.com/o/r/pull/12. (Was https://github.com/o/r/pull/9)").as_deref(),
            Some("https://github.com/o/r/pull/9")
        );
        assert_eq!(
            pr_link("PR: <https://git.example/me/r/pulls/3>, merged? No.").as_deref(),
            Some("https://git.example/me/r/pulls/3")
        );
        assert_eq!(
            pr_link("see https://gitlab.com/g/p/-/merge_requests/41").as_deref(),
            Some("https://gitlab.com/g/p/-/merge_requests/41")
        );
        assert_eq!(pr_link("https://github.com/o/r/pulls and https://github.com/o/r/issues/4"), None);
        assert_eq!(pr_link("no link"), None);
    }

    #[test]
    fn a_pull_request_brief_names_the_branch_and_where_it_goes() {
        let b = pr_brief("/p", "a2a/fixer-t1", "origin/trunk", "0123456789abcdef");
        assert!(b.contains("`a2a/fixer-t1`") && b.contains("origin/trunk (0123456789ab)"), "{b}");
        assert!(b.contains("against trunk") && b.contains("Don't merge"), "{b}");
    }

    #[test]
    fn grants_last_an_hour_per_account_and_agent_and_go_when_revoked() {
        let d = std::env::temp_dir().join(format!("arugula-grants-{}-{}", std::process::id(), now_ms()));
        let ale = Asker { account: "a2".into(), name: "ale".into(), ..Default::default() };
        assert!(!granted(&d, &ale, "fixer"));
        grant(&d, &ale, "fixer");
        assert!(granted(&d, &ale, "fixer"));
        assert!(!granted(&d, &ale, "reviewer"));
        assert!(!granted(&d, &Asker { account: "a3".into(), ..Default::default() }, "fixer"));
        assert_eq!(grants(&d)[0].name, "ale");
        assert!(revoke(&d, "a2", "fixer").unwrap());
        assert!(!granted(&d, &ale, "fixer"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
