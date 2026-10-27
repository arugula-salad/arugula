//! Getting started's buttons: what the welcome screens used to tell you to
//! type, run by the daemon for its owner.
//!
//! - `POST /api/setup/tailscale`: `tailscale serve --bg --https=443` in
//!   front of this daemon, so the phone reaches it on the tailnet.
//! - `POST /api/setup/control`: ask Arugula control to add this machine
//!   (`arugulad join`); the answer is the code and where to approve it,
//!   and the daemon waits for the approval in the background. The link
//!   carries a join secret (#760), so an approval whose MAC checks out is
//!   saved at once. With an older control (no secret), the page shows the
//!   account's fingerprint to check against the approving device, and
//!   `POST /api/setup/control/confirm` (`{"same": true}`) saves the join;
//!   `false` drops it. On a machine control dropped (#325), the same
//!   button joins again: the old enrollment is set aside first.
//! - `POST /api/setup/claude`: `claude mcp add arugula -- arugula mcp`.
//! - `POST /api/setup/agents/{kind}` (#335), "Use Claude Code with
//!   Arugula": installs (or updates) the agent's ACP adapter, waiting for
//!   npm, and for Claude Code adds its hooks (`arugula hooks install`, so a
//!   terminal Claude's questions and permission prompts come to cards) and
//!   the MCP server too; the answer says what changed (`done`), naming the
//!   settings file. `arugula setup claude` asks the same.
//!
//! When control says this machine's key was removed from its account
//! (#330), the daemon makes a new key and starts a join itself; `GET
//! /api/setup` shows that as `control.removed`, beside the new code. Back
//! into the same account, it saves the join without asking again.
//!
//! `GET /api/setup` says how far along each one is; `?part=agents`, only
//! the agents' (each adapter's state, whether its agent's CLI is on this
//! machine, and whether Claude Code has the MCP server and the hooks), with `cli`: how a
//! new terminal runs `arugula` (#883). When a step needs
//! something only a person can do (a one-time sudo, a switch in
//! Tailscale's admin console, a sign-in), the error says so, with the
//! command or the link.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_proto::{
    op::ops::{AgentCanGet, SetupAgent, SetupClaude, SetupControl, SetupControlConfirm, SetupGet, SetupTailscale},
    setup::{ControlConfirmRequest, ControlJoinRequest, Outcome, SetupQuery},
};
use axum::Router;
use serde::Serialize;
use serde_json::Value;

use crate::agent::{
    adapters::{ADAPTERS, Adapter, State as AdapterState},
    defs::Kind,
};
use crate::{control::AccountCheck, ops::OpRoutes, server::App};

pub const CONTROL: &str = "https://control.arugula.io";
const TAILSCALE_ADMIN_DNS: &str = "https://login.tailscale.com/admin/dns";
const TAILSCALE_DOWNLOAD: &str = "https://tailscale.com/download";

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .op::<SetupGet>()
        .op::<AgentCanGet>()
        .op::<SetupTailscale>()
        .op::<SetupControl>()
        .op::<SetupControlConfirm>()
        .op::<SetupClaude>()
        .op::<SetupAgent>()
}

#[derive(Serialize)]
struct Status {
    tailscale: TailscaleStatus,
    control: ControlStatus,
    claude: ClaudeStatus,
    /// Each adapter's state (`adapters::Status::json`), with `found`: its
    /// agent's CLI is on this machine (#335).
    adapters: Vec<Value>,
    cli: CliStatus,
}

/// How a new terminal here runs the CLI (#883), for the welcome card.
#[derive(Serialize, Default)]
struct CliStatus {
    /// `arugula` when a new terminal's login shell finds it, else its full
    /// path (`~/.local/bin/arugula`).
    command: String,
    /// The startup files Arugula put `~/.local/bin` on PATH in: a terminal
    /// open since before needs the full path.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    added: Vec<String>,
}

/// `CliStatus`, from the login shell (a fresh one, with a clean
/// environment: the daemon's own PATH isn't a terminal's).
async fn cli_status() -> CliStatus {
    #[cfg(unix)]
    {
        use arugula_proto::shell_path::{State, check};
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return CliStatus::default() };
        let shell = PathBuf::from(crate::login_shell());
        let h = home.clone();
        let Ok(state) = crate::task::spawn_blocking(move || check(&h, &shell)).await else {
            return CliStatus { command: "arugula".into(), added: vec![] };
        };
        let tilde = |p: &Path| p.strip_prefix(&home).map_or(p.display().to_string(), |r| format!("~/{}", r.display()));
        let installed = home.join(".local/bin/arugula").is_file();
        let command = if state.found() || !installed { "arugula".into() } else { "~/.local/bin/arugula".into() };
        let added = match &state {
            State::Marked(files) if installed => files.iter().map(|f| tilde(f)).collect(),
            _ => vec![],
        };
        CliStatus { command, added }
    }
    #[cfg(not(unix))]
    CliStatus { command: "arugula".into(), added: vec![] }
}

#[derive(Serialize, Default)]
struct TailscaleStatus {
    /// `missing` (no Tailscale here), `stopped`, `needs-login`, `running`.
    state: &'static str,
    /// This machine's MagicDNS name.
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    /// HTTPS certificates are on for the tailnet (serve needs them).
    https: bool,
    /// `tailscale serve` sends the tailnet's HTTPS to this daemon.
    serving: bool,
    /// Where the phone opens it.
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    /// A request has come in over the tailnet.
    seen: bool,
}

#[derive(Serialize, Default)]
struct ControlStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    joined: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    team: Option<String>,
    /// A join waiting for someone to approve it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pending: Option<Pending>,
    /// An approved join waiting for the person to check the account.
    #[serde(skip_serializing_if = "Option::is_none")]
    confirm: Option<Confirm>,
    /// The last join that didn't work, and why.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Control said this machine's old key was removed from its account
    /// (#330), so the join waiting has a new key: what it said, when and
    /// by which device, and both keys' fingerprints.
    #[serde(skip_serializing_if = "Option::is_none")]
    removed: Option<crate::control::Removed>,
    /// A join `arugulad join` started on this machine (#329): its code
    /// and where to approve it. Only one join runs at a time.
    #[serde(skip_serializing_if = "Option::is_none")]
    elsewhere: Option<crate::control::JoinLockInfo>,
    /// The control the button joins: `--control`, else Arugula cloud
    /// (#207), or the one that dropped this machine (#325). The page sends
    /// it back with the join.
    url: String,
    /// How this machine stands with control now (#325): as `/api/host`'s
    /// `control_state`.
    state: crate::control::State,
}

#[derive(Serialize, Clone)]
struct Pending {
    /// The whole code, with the join secret after a dot (#760) when there
    /// is one: what to type where the link can't be opened.
    code: String,
    approve: String,
    /// When the code stops working (ms since the epoch).
    expires_ms: u64,
}

/// What the page shows to check before the join is saved.
#[derive(Serialize, Clone)]
struct Confirm {
    /// The account's fingerprint.
    account: String,
    /// The device that approved it, by name.
    approver: String,
    /// "your account" or "the team X".
    place: String,
}

#[derive(Serialize, Default)]
struct ClaudeStatus {
    installed: bool,
    /// `claude mcp get arugula` finds it.
    tools: bool,
    /// `arugula hooks status` says Arugula's hooks are there (or its
    /// plugin's): a terminal Claude's questions come to cards.
    hooks: bool,
}

/// The join in flight, if any, and the last one's failure.
static JOIN: Mutex<(Option<Pending>, Option<String>)> = Mutex::new((None, None));
/// An approved join, until the person says the account is theirs.
static APPROVED: Mutex<Option<crate::control::Approved>> = Mutex::new(None);
/// The old key control said was removed, until this machine is in again.
static REMOVED: Mutex<Option<crate::control::Removed>> = Mutex::new(None);

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// The daemon's port, for what serve points at.
fn port(app: &App) -> u16 {
    app.access.port()
}

/// `name` on the owner's shell's PATH (which a service's own PATH may lack),
/// then in the usual places.
async fn find(app: &App, name: &str, also: &[&str]) -> Option<PathBuf> {
    let shell = app.mux.shell_env.local().await;
    let path = shell.get("PATH").map(str::to_owned).or_else(|| std::env::var("PATH").ok()).unwrap_or_default();
    std::env::split_paths(&path).map(|d| d.join(name)).chain(also.iter().map(PathBuf::from)).find(|p| p.is_file())
}

/// Runs a command with the shell's PATH: its exit, stdout and stderr.
async fn run(app: &App, bin: &Path, args: &[&str], timeout: Duration) -> Result<(bool, String, String), String> {
    let shell = app.mux.shell_env.local().await;
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args).kill_on_drop(true).stdin(std::process::Stdio::null());
    if let Some(p) = shell.get("PATH") {
        cmd.env("PATH", p);
    }
    if bin.to_string_lossy().contains("Tailscale.app") {
        // The macOS app's binary is the CLI only when told so: started
        // from a service, it tries to open the GUI and fails.
        cmd.env("TAILSCALE_BE_CLI", "1");
    }
    match tokio::time::timeout(timeout, cmd.output()).await {
        Err(_) => Err(format!("{} didn't finish in {} s", bin.display(), timeout.as_secs())),
        Ok(Err(e)) => Err(format!("{}: {e}", bin.display())),
        Ok(Ok(o)) => Ok((
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).trim().to_owned(),
        )),
    }
}

// ---- tailscale

const TAILSCALE_ELSEWHERE: [&str; 4] = [
    "/usr/bin/tailscale",
    "/usr/local/bin/tailscale",
    "/opt/homebrew/bin/tailscale",
    "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
];

async fn tailscale(app: &App) -> Option<PathBuf> {
    find(app, "tailscale", &TAILSCALE_ELSEWHERE).await
}

async fn tailscale_status(app: &App) -> TailscaleStatus {
    let seen = app.tailnet_seen.load(std::sync::atomic::Ordering::Relaxed);
    let Some(ts) = tailscale(app).await else {
        return TailscaleStatus { state: "missing", seen, ..Default::default() };
    };
    let Ok((true, out, _)) = run(app, &ts, &["status", "--json"], Duration::from_secs(5)).await else {
        return TailscaleStatus { state: "stopped", seen, ..Default::default() };
    };
    let v: Value = serde_json::from_str(&out).unwrap_or_default();
    let state = match v["BackendState"].as_str() {
        Some("Running") => "running",
        Some("NeedsLogin" | "NeedsMachineAuth") => "needs-login",
        _ => "stopped",
    };
    let host = v["Self"]["DNSName"].as_str().map(|h| h.trim_end_matches('.').to_owned()).filter(|h| !h.is_empty());
    let https = v["CertDomains"].as_array().is_some_and(|d| !d.is_empty());
    let serving = match run(app, &ts, &["serve", "status", "--json"], Duration::from_secs(5)).await {
        Ok((true, out, _)) => serves(&out, port(app)),
        _ => false,
    };
    TailscaleStatus {
        state,
        url: host.as_ref().filter(|_| serving).map(|h| format!("https://{h}")),
        host,
        https,
        serving,
        seen,
    }
}

/// Whether `tailscale serve status --json` proxies something to this port.
fn serves(json: &str, port: u16) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return false };
    let targets = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    v["Web"].as_object().is_some_and(|web| {
        web.values().any(|site| {
            site["Handlers"].as_object().is_some_and(|h| {
                h.values()
                    .any(|handler| handler["Proxy"].as_str().is_some_and(|p| targets.iter().any(|t| p.contains(t))))
            })
        })
    })
}

pub(crate) async fn tailscale_serve(app: &Arc<App>) -> Outcome {
    let Some(ts) = tailscale(app).await else {
        return Outcome::err("Tailscale isn't installed on this machine.").link("Get Tailscale", TAILSCALE_DOWNLOAD);
    };
    let st = tailscale_status(app).await;
    match st.state {
        "running" => {}
        "needs-login" => {
            return Outcome::err("Tailscale is installed but not signed in.").fix(format!("{} up", tailscale_cmd(&ts)));
        }
        _ => return Outcome::err("Tailscale isn't running.").fix(format!("{} up", tailscale_cmd(&ts))),
    }
    if st.serving {
        return Outcome::ok();
    }
    if !st.https {
        return Outcome::err("Turn on MagicDNS and HTTPS certificates for your tailnet first, then try again.")
            .link("Tailscale's DNS settings", TAILSCALE_ADMIN_DNS);
    }
    let target = format!("http://127.0.0.1:{}", port(app));
    // Serve may stop to ask for Serve to be turned on for the tailnet: it
    // prints the link and waits, so this gives up after a while and passes
    // the link on.
    match run(app, &ts, &["serve", "--bg", "--yes", "--https=443", &target], Duration::from_secs(20)).await {
        Ok((true, _, _)) => Outcome::ok(),
        Ok((false, out, err)) => serve_failed(&ts, &format!("{out}\n{err}")),
        Err(e) => Outcome::err(e),
    }
}

fn tailscale_cmd(ts: &Path) -> String {
    if ts.starts_with("/Applications") { ts.display().to_string() } else { "tailscale".into() }
}

fn serve_failed(ts: &Path, said: &str) -> Outcome {
    let said = said.trim();
    if let Some(url) = said.split_whitespace().find(|w| w.starts_with("https://login.tailscale.com/")) {
        return Outcome::err("Serve isn't turned on for your tailnet yet.").link("Turn on Serve", url);
    }
    if said.contains("Access denied") || said.contains("operator") || said.contains("permission") {
        // Linux: serve's config is tailscaled's; letting this user change
        // it takes root once.
        return Outcome::err("Tailscale needs to let you change serve without sudo, once.")
            .fix(format!("sudo {} set --operator=$USER", tailscale_cmd(ts)));
    }
    Outcome::err(if said.is_empty() { "tailscale serve failed".to_owned() } else { said.to_owned() })
}

// ---- control

fn control_status(app: &App) -> ControlStatus {
    let state = control_state(app);
    // Dropped by control (#325): not joined, as far as joining again goes.
    let joined = app.control.enrolled().filter(|_| !state.is_dropped());
    let saved = joined.as_ref().map(|e| &e.saved);
    let (pending, error) = JOIN.lock().unwrap().clone();
    ControlStatus {
        joined: saved.map(|s| s.url.clone()),
        team: saved
            .and_then(|s| s.roster.as_ref().map(|r| r.name.clone()).or_else(|| Some(s.team.as_ref()?.team.clone()))),
        pending: pending.filter(|p| p.expires_ms > now_ms() && saved.is_none()),
        confirm: APPROVED.lock().unwrap().as_ref().filter(|_| saved.is_none()).map(|a| Confirm {
            account: a.joined.account.clone(),
            approver: a.joined.approver.clone(),
            place: a.joined.place.clone(),
        }),
        error: error.filter(|_| saved.is_none()),
        removed: REMOVED.lock().unwrap().clone().filter(|_| saved.is_none()),
        elsewhere: crate::control::JoinLock::held(app.control.state_dir()).filter(|h| !h.mine() && saved.is_none()),
        // Joining again after a drop goes back to the same control.
        url: state.url.clone().filter(|_| state.is_dropped()).unwrap_or_else(|| app.control.default_url.clone()),
        state,
    }
}

/// This machine's standing with control (#325), with the join waiting for
/// approval when it was dropped: the one the daemon asked for by itself
/// after its key was removed (#330), Getting started's, or `arugulad
/// join`'s (#329). `/api/host`'s `control_state`.
pub fn control_state(app: &App) -> crate::control::State {
    let mut s = app.control.state();
    if s.is_dropped() {
        let own = JOIN.lock().unwrap().0.clone().filter(|p| p.expires_ms > now_ms()).map(|p| (p.code, p.approve));
        let cli = || {
            let h = crate::control::JoinLock::held(app.control.state_dir()).filter(|h| !h.mine())?;
            Some((h.code?, h.approve?))
        };
        if let Some((code, approve)) = own.or_else(cli) {
            s.code = Some(code);
            s.approve = Some(approve);
        }
    }
    s
}

pub(crate) async fn control_join(app: &Arc<App>, req: ControlJoinRequest) -> Value {
    let dropped = app.control.state().is_dropped();
    if (app.control.enrolled().is_some() && !dropped) || APPROVED.lock().unwrap().is_some() {
        return serde_json::to_value(control_status(app)).unwrap();
    }
    // One at a time: asking again while a code is open returns that code.
    if let (Some(p), _) = &*JOIN.lock().unwrap()
        && p.expires_ms > now_ms()
    {
        return serde_json::json!({ "pending": p });
    }
    // Control dropped this machine (#325): set what it had aside, so the
    // join starts fresh.
    if dropped && let Err(e) = app.control.forget_dropped() {
        let e = format!("can't set the dropped enrollment aside: {e}");
        JOIN.lock().unwrap().1 = Some(e.clone());
        return serde_json::json!({ "error": e });
    }
    let url = req.url.filter(|u| !u.trim().is_empty()).unwrap_or_else(|| app.control.default_url.clone());
    start_join(app, &url, req.team.as_deref()).await
}

/// Control said this machine's key was removed (#330): ask `url` to add
/// it again, with a new key, unless a join is under way already.
pub fn rejoin(app: Arc<App>, url: String) {
    crate::task::spawn(async move {
        if APPROVED.lock().unwrap().is_some()
            || JOIN.lock().unwrap().0.as_ref().is_some_and(|p| p.expires_ms > now_ms())
        {
            return;
        }
        let v = start_join(&app, &url, None).await;
        match v["pending"]["approve"].as_str() {
            Some(at) => tracing::warn!(approve = at, "joining again with a new key: approve it on a signed-in device"),
            None => tracing::warn!(error = %v["error"], "can't join again with a new key"),
        }
    });
}

/// Ask control for a code and wait for its approval in the background.
async fn start_join(app: &Arc<App>, url: &str, team: Option<&str>) -> Value {
    let name = app.hosts.name().to_owned();
    let dir = app.control.state_dir().to_owned();
    match crate::control::join_start(url, &name, team, &dir, "Getting started").await {
        Ok(p) => {
            let pending = Pending {
                code: p.whole_code(),
                approve: p.approve_url(),
                expires_ms: now_ms() + p.expires_in_secs * 1000,
            };
            let rejoining = p.renewed.as_ref().and_then(|r| r.root.clone());
            if let Some(r) = &p.renewed {
                *REMOVED.lock().unwrap() = Some(r.clone());
            }
            *JOIN.lock().unwrap() = (Some(pending.clone()), None);
            let app = app.clone();
            crate::task::spawn(async move {
                let r = crate::control::join_finish(p).await;
                // The approving device answered the link's secret (#760),
                // or it's back in the account it was removed from (the
                // person checked that one when it first joined): saved.
                let r = match r {
                    Ok(a)
                        if a.check == AccountCheck::Proven
                            || rejoining.as_deref().is_some_and(|root| a.is_account(root)) =>
                    {
                        let saved = a.save(app.control.state_dir()).map(|_| ());
                        if saved.is_ok() {
                            *REMOVED.lock().unwrap() = None;
                        }
                        app.control.poke();
                        *JOIN.lock().unwrap() = (None, saved.err().map(|e| e.to_string()));
                        return;
                    }
                    // The link had a secret, and the approval no answer to
                    // it: the account is only control's word.
                    Ok(a) if a.check == AccountCheck::Missing => {
                        a.refuse(app.control.state_dir()).await;
                        *JOIN.lock().unwrap() = (
                            None,
                            Some("Not joined: the approval came without the approving device's answer to this machine's secret (approved from the code alone?), so the account is only control's word. Connect again, and open the link, or type the whole code.".into()),
                        );
                        return;
                    }
                    r => r,
                };
                let mut j = JOIN.lock().unwrap();
                match r {
                    Ok(a) => {
                        *APPROVED.lock().unwrap() = Some(a);
                        *j = (None, None);
                    }
                    Err(e) => *j = (None, Some(e.to_string())),
                }
            });
            serde_json::json!({ "pending": pending, "removed": REMOVED.lock().unwrap().clone() })
        }
        Err(e) => {
            JOIN.lock().unwrap().1 = Some(e.to_string());
            serde_json::json!({ "error": e.to_string() })
        }
    }
}

/// The person compared the account's fingerprints: save the join (where the
/// running daemon picks it up), or drop it.
pub(crate) async fn control_confirm(app: &Arc<App>, req: ControlConfirmRequest) -> Value {
    let Some(a) = APPROVED.lock().unwrap().take() else {
        return serde_json::json!({ "control": control_status(app) });
    };
    let error = if !req.same {
        let e = format!(
            "Not joined: control approved this machine into the account {}, which isn't the one your device shows. Don't add machines through this control.",
            a.joined.account
        );
        a.refuse(app.control.state_dir()).await;
        Some(e)
    } else {
        let r = a.save(app.control.state_dir());
        if r.is_ok() {
            *REMOVED.lock().unwrap() = None;
        }
        app.control.poke();
        r.err().map(|e| e.to_string())
    };
    JOIN.lock().unwrap().1 = error;
    serde_json::json!({ "control": control_status(app) })
}

// ---- claude code

async fn claude(app: &App) -> Option<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let local = [format!("{home}/.local/bin/claude"), format!("{home}/.claude/local/claude")];
    let also: Vec<&str> =
        local.iter().map(String::as_str).chain(["/opt/homebrew/bin/claude", "/usr/local/bin/claude"]).collect();
    find(app, "claude", &also).await
}

/// The `arugula` CLI that Claude Code should start: beside this daemon,
/// else on the PATH.
async fn cli(app: &App) -> Option<PathBuf> {
    let beside =
        std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("arugula"))).filter(|p| p.is_file());
    match beside {
        Some(p) => Some(p),
        None => find(app, "arugula", &[]).await,
    }
}

/// The MCP server's name in Claude Code: its tools are `mcp__arugula__*`.
const MCP_NAME: &str = crate::mcp::SERVER_NAME;

/// `claude mcp get NAME`: the scope it's registered in (`user`, `local`
/// or `project`), if it is.
async fn mcp_scope(app: &App, c: &Path, name: &str) -> Option<String> {
    match run(app, c, &["mcp", "get", name], Duration::from_secs(15)).await {
        Ok((true, out, _)) => Some(scope_of(&out).unwrap_or("user").to_owned()),
        _ => None,
    }
}

/// The scope in `claude mcp get`'s answer (`Scope: User config (...)`).
fn scope_of(out: &str) -> Option<&'static str> {
    let line = out.lines().find_map(|l| l.trim().strip_prefix("Scope:"))?.trim().to_lowercase();
    ["user", "local", "project"].into_iter().find(|s| line.starts_with(s))
}

async fn claude_status(app: &App) -> ClaudeStatus {
    let hooks = hooks_status(app).await;
    let Some(c) = claude(app).await else { return ClaudeStatus { hooks, ..Default::default() } };
    ClaudeStatus { installed: true, tools: mcp_scope(app, &c, MCP_NAME).await.is_some(), hooks }
}

/// `arugula --json hooks status`: whether the settings.json Claude Code
/// reads has Arugula's hooks, or the plugin brings them.
async fn hooks_status(app: &App) -> bool {
    let Some(cli) = cli(app).await else { return false };
    match run(app, &cli, &["--json", "hooks", "status"], Duration::from_secs(10)).await {
        Ok((true, out, _)) => serde_json::from_str::<Value>(&out).is_ok_and(|v| v["installed"] == true),
        _ => false,
    }
}

/// Claude Code's hooks for Arugula, added by `arugula hooks install` (the
/// one installer): what changed in its settings.json, a line each (none:
/// they were there already).
async fn add_hooks(app: &App) -> Result<Vec<String>, Outcome> {
    let Some(cli) = cli(app).await else {
        return Err(Outcome::err("Can't find the Arugula CLI next to the daemon."));
    };
    match run(app, &cli, &["--json", "hooks", "install"], Duration::from_secs(20)).await {
        Ok((true, out, _)) => Ok(hooks_lines(&serde_json::from_str(&out).unwrap_or_default())),
        Ok((false, out, err)) => {
            let said = if err.is_empty() { out.trim().to_owned() } else { err };
            Err(Outcome::err(format!("arugula hooks install: {said}")).fix(format!("{} hooks install", cli.display())))
        }
        Err(e) => Err(Outcome::err(e)),
    }
}

/// What `hooks install --json` said it did, in the done lines: the
/// settings file by name, under `~` when it's there.
fn hooks_lines(said: &Value) -> Vec<String> {
    let path = said["settings"].as_str().unwrap_or("settings.json");
    let home = std::env::var("HOME").unwrap_or_default();
    let file = match Path::new(path).strip_prefix(&home) {
        Ok(rest) if !home.is_empty() => format!("~/{}", rest.display()),
        _ => path.to_owned(),
    };
    let mut lines = vec![];
    if said["hooks_added"] == true {
        lines.push(format!(
            "Added Arugula's hooks to {file}: a terminal Claude's questions and permission prompts come to cards here and on your phone, from its next start."
        ));
    }
    if said["rules_added"] == true {
        lines.push(format!(
            "Allowed Arugula's read-only commands and tools in {file} (permissions.allow), so Claude Code uses them without asking."
        ));
    }
    lines
}

pub(crate) async fn claude_mcp(app: &Arc<App>) -> Outcome {
    match add_mcp(app).await {
        Ok(done) => Outcome { done, ..Outcome::ok() },
        Err(o) => o,
    }
}

/// Claude Code's MCP server added: what changed, a line (none: it was
/// there already).
async fn add_mcp(app: &App) -> Result<Vec<String>, Outcome> {
    let Some(c) = claude(app).await else {
        return Err(Outcome::err("Claude Code isn't installed on this machine.")
            .link("Install Claude Code", "https://docs.anthropic.com/en/docs/claude-code"));
    };
    if mcp_scope(app, &c, MCP_NAME).await.is_some() {
        return Ok(vec![]);
    }
    let Some(cli) = cli(app).await else {
        return Err(Outcome::err("Can't find the Arugula CLI next to the daemon."));
    };
    let cli = cli.display().to_string();
    let args = ["mcp", "add", "--scope", "user", MCP_NAME, "--", &cli, "mcp"];
    match run(app, &c, &args, Duration::from_secs(20)).await {
        Ok((true, _, _)) => Ok(vec![
            "Added Arugula's MCP server to Claude Code: it can start its helpers as panes in your next session.".into(),
        ]),
        Ok((false, out, err)) => {
            let said = if err.is_empty() { out.trim().to_owned() } else { err };
            Err(Outcome::err(format!("claude mcp add: {said}")))
        }
        Err(e) => Err(Outcome::err(e)),
    }
}

// ---- agents' adapters (#335)

const NODE_DOWNLOAD: &str = "https://nodejs.org/en/download";
/// How long an install may take (npm fetching a few MB, or a slow network).
const NPM_TIMEOUT: Duration = Duration::from_secs(300);
/// One install at a time.
static INSTALLING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Where else an agent's CLI is, when the shell's PATH doesn't have it.
async fn agent_cli(app: &App, a: &Adapter) -> Option<PathBuf> {
    if a.kind == Kind::Claude {
        return claude(app).await;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let local = format!("{home}/.local/bin/{}", a.cli);
    let brew = format!("/opt/homebrew/bin/{}", a.cli);
    let usr = format!("/usr/local/bin/{}", a.cli);
    find(app, a.cli, &[&local, &brew, &usr]).await
}

/// Every adapter's state, as an agent block would find it, and whether its
/// agent's CLI is here.
async fn adapters_status(app: &App) -> Vec<Value> {
    let Ok((home, env)) = crate::api::agent_env(app).await else { return Vec::new() };
    let mut list =
        crate::task::spawn_blocking(move || crate::agent::adapters::all(&home, &env)).await.unwrap_or_default();
    for (v, a) in list.iter_mut().zip(ADAPTERS) {
        v["found"] = agent_cli(app, a).await.is_some().into();
    }
    list
}

/// What `use_agent` said it did, a line each.
fn installed_line(a: &Adapter, was: &AdapterState) -> String {
    match was {
        AdapterState::Installed { version: Some(v), .. } => {
            format!(
                "Updated {}'s adapter from {v} to {}: agent panes use it from their next start.",
                a.label,
                a.pinned()
            )
        }
        _ => format!("Installed {}'s adapter ({}): {} runs as agent panes here.", a.label, a.pinned(), a.label),
    }
}

/// The adapter, installed or updated to the pin, waiting for npm: what
/// changed (a line), or nothing when it was current.
async fn install(app: &App, a: &'static Adapter) -> Result<Option<String>, Outcome> {
    let (home, env) = crate::api::agent_env(app).await.map_err(|e| Outcome::err(e.1))?;
    let (st, path, home) = crate::task::spawn_blocking(move || {
        let st = crate::agent::adapters::status(a, &home, &env);
        let path = crate::agent::adapters::node_path(&home, &env);
        (st, path, home)
    })
    .await
    .map_err(|e| Outcome::err(e.to_string()))?;
    if let AdapterState::NoNode { .. } = st.state {
        return Err(Outcome::err(format!(
            "{}. Install Node (nodejs.org, or `mise use -g node@22`) and try again, or run this once Node is there:",
            st.why()
        ))
        .fix(st.npm)
        .link("Get Node", NODE_DOWNLOAD));
    }
    if st.current() {
        return Ok(None);
    }
    let Ok(_one) = INSTALLING.try_lock() else {
        return Err(Outcome::err("An adapter is being installed already: try again when it's done."));
    };
    let Some(cmd) = crate::agent::adapters::npm_install(a, &home, &path) else {
        return Err(Outcome::err("There's no npm next to this machine's Node: install it, or run this:").fix(st.npm));
    };
    let mut cmd = tokio::process::Command::from(cmd);
    cmd.kill_on_drop(true);
    tracing::info!(adapter = a.package, "installing an agent's adapter (#335)");
    match tokio::time::timeout(NPM_TIMEOUT, cmd.output()).await {
        Err(_) => {
            Err(Outcome::err(format!("npm didn't finish in {} s: run it yourself:", NPM_TIMEOUT.as_secs())).fix(st.npm))
        }
        Ok(Err(e)) => Err(Outcome::err(format!("npm: {e}")).fix(st.npm)),
        Ok(Ok(o)) if !o.status.success() => {
            let said = String::from_utf8_lossy(&o.stderr);
            let tail: Vec<&str> = said.lines().filter(|l| !l.trim().is_empty()).collect();
            let tail = tail[tail.len().saturating_sub(4)..].join("\n");
            Err(Outcome::err(format!(
                "npm install failed: {}",
                if tail.is_empty() { "it didn't say why" } else { &tail }
            ))
            .fix(st.npm))
        }
        Ok(Ok(_)) => Ok(Some(installed_line(a, &st.state))),
    }
}

/// `POST /api/setup/agents/{kind}`: "Use Claude Code with Arugula" (or
/// Codex): its adapter installed or brought up to the pin, and for Claude
/// Code Arugula's MCP server and hooks added. Run by the person, never by itself.
pub(crate) async fn use_agent(app: &Arc<App>, kind: String) -> Outcome {
    let Some(a) = ADAPTERS.iter().find(|a| a.dir == kind || a.cli == kind) else {
        return Outcome::err(format!("no agent {kind} to set up (claude or codex)"));
    };
    let mut done = Vec::new();
    match install(app, a).await {
        Ok(Some(line)) => done.push(line),
        Ok(None) => {}
        Err(o) => return o,
    }
    if a.kind == Kind::Claude {
        match add_mcp(app).await {
            Ok(lines) => done.extend(lines),
            Err(o) => {
                done.extend(o.done.iter().cloned());
                return Outcome { done, ..o };
            }
        }
        // After the MCP server, which finds Claude Code first: no
        // settings.json made where there's no Claude Code to read it.
        match add_hooks(app).await {
            Ok(lines) => done.extend(lines),
            Err(o) => return Outcome { done, ..o },
        }
    }
    if done.is_empty() {
        done.push(match a.kind {
            Kind::Claude => {
                "Already set up: agent panes work, and Claude Code has Arugula's hooks and MCP server.".into()
            }
            _ => format!("Already set up: {} runs as agent panes here.", a.label),
        });
    }
    Outcome { done, ..Outcome::ok() }
}

// ---- status

/// `GET /api/setup`: how far along each part is (`?part=` for one).
pub(crate) async fn status(app: &Arc<App>, q: SetupQuery) -> Value {
    if q.part.as_deref() == Some("control") {
        return serde_json::json!({ "control": control_status(app) });
    }
    if q.part.as_deref() == Some("agents") {
        let (claude, adapters, cli) = tokio::join!(claude_status(app), adapters_status(app), cli_status());
        return serde_json::json!({ "claude": claude, "adapters": adapters, "cli": cli });
    }
    let (tailscale, claude, adapters, cli) =
        tokio::join!(tailscale_status(app), claude_status(app), adapters_status(app), cli_status());
    serde_json::to_value(Status { tailscale, control: control_status(app), claude, adapters, cli }).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_an_install_says_it_did() {
        let claude = ADAPTERS.iter().find(|a| a.kind == Kind::Claude).unwrap();
        let fresh = installed_line(claude, &AdapterState::Missing);
        assert_eq!(
            fresh,
            format!("Installed Claude Code's adapter ({}): Claude Code runs as agent panes here.", claude.pinned())
        );
        let old = AdapterState::Installed { version: Some("0.81.2".into()), on_path: false };
        assert!(installed_line(claude, &old).starts_with("Updated Claude Code's adapter from 0.81.2 to "));
        // An outcome's lines go out only when there are some.
        let o = serde_json::to_value(Outcome::ok()).unwrap();
        assert!(o.get("done").is_none());
        let o = serde_json::to_value(Outcome { done: vec![fresh], ..Outcome::ok() }).unwrap();
        assert_eq!(o["done"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn what_a_hooks_install_says_it_did() {
        let home = std::env::var("HOME").unwrap_or_default();
        let said = serde_json::json!({
            "settings": format!("{home}/.claude/settings.json"), "hooks_added": true, "rules_added": false,
        });
        let lines = hooks_lines(&said);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("Added Arugula's hooks to ~/.claude/settings.json: "), "{lines:?}");
        let said = serde_json::json!({ "settings": "/x/settings.json", "hooks_added": false, "rules_added": true });
        assert!(hooks_lines(&said)[0].contains(" in /x/settings.json "));
        assert!(hooks_lines(&serde_json::json!({ "settings": "/x", "hooks_added": false })).is_empty());
    }

    #[test]
    fn scope_from_claude_mcp_get() {
        let out = "arugula:\n  Scope: Local config (private to you in this project)\n  Status: ✓ Connected\n";
        assert_eq!(scope_of(out), Some("local"));
        assert_eq!(scope_of("x:\n  Scope: User config (available in all your projects)\n"), Some("user"));
        assert_eq!(scope_of("x:\n  Scope: Project config (shared via .mcp.json)\n"), Some("project"));
        assert_eq!(scope_of("x:\n  Type: stdio\n"), None);
    }

    #[test]
    fn serve_status_finds_this_port() {
        let json = r#"{"TCP":{"443":{"HTTPS":true}},"Web":{"geek.tail.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:7681"}}}}}"#;
        assert!(serves(json, 7681));
        assert!(!serves(json, 7682));
        assert!(!serves("{}", 7681));
    }

    #[test]
    fn serve_failures_say_what_to_do() {
        let ts = Path::new("/usr/bin/tailscale");
        let o = serve_failed(
            ts,
            "Serve is not enabled on your tailnet.\nTo enable, visit:\n\n https://login.tailscale.com/f/serve?node=abc",
        );
        assert_eq!(o.link.unwrap().url, "https://login.tailscale.com/f/serve?node=abc");
        let o = serve_failed(ts, "Access denied: serve config denied");
        assert_eq!(o.fix.as_deref(), Some("sudo tailscale set --operator=$USER"));
        let mac = Path::new("/Applications/Tailscale.app/Contents/MacOS/Tailscale");
        assert!(serve_failed(mac, "permission denied").fix.unwrap().starts_with("sudo /Applications/"));
    }
}
