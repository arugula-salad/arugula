//! Telemetry (M101, #983): counting what happens here, and sending it to
//! the hosted control once the owner said yes (`arugula_proto::telemetry`
//! holds the answer and the rules that keep it off). What a report holds is
//! `arugula_control_wire::telemetry`; `docs/telemetry.md` says it for people.
//!
//! - **Counting** goes through [`count`], one counter for the daemon, and
//!   only while telemetry is on: off, nothing is counted at all. The mux
//!   tells it of terminals and blocks ([`Hooks`], its `Usage`); the edge of
//!   clients and MCP sessions; agent blocks of their runtime.
//! - **What's counted and not yet sent** is kept in `telemetry.counts.json`,
//!   saved hourly and at stop, so a restart loses at most an hour.
//! - **When:** `opted-in` right after a yes, `started` a minute after
//!   startup, `daily` once a day. A report that doesn't go is sent again at
//!   the next try, an hour on; what's older than a week is dropped.
//! - **Crashes** (M102) wait in `crashes/`, a file each, written by the
//!   panic hook of `arugulad`, `arugula` and the app
//!   (`arugula_proto::telemetry::crash`). They go with the next report
//!   that goes anyway (`opted-in`, `started`, `daily`, or one tried again),
//!   [`MAX_CRASHES`] at a time, and each file is deleted once a report
//!   holding it was taken. A no empties `crashes/`.
//! - **Signed** when the machine is joined to the control reports go to (the
//!   hosted one, unless `ARUGULA_TELEMETRY_URL` says). A signed report
//!   control refuses (401: the enrolment is stale) goes again unsigned, and
//!   reports stay unsigned until the enrolment changes.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use arugula_control_wire::telemetry::{
    AgentRuntime, Arch, BlockKind, Channel, ControlKind, Counts, Crash, Event, LabsFlag, MAX_CRASHES, MAX_EVENTS, Os,
    Report, Surface, TELEMETRY, View,
};
use arugula_e2e::now_ms;
use arugula_proto::{
    BlockType,
    telemetry::{self as consent, COUNTS_FILE, TelemetryInfo, TelemetryNoteRequest, crash},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info, warn};

const HOUR: Duration = Duration::from_secs(3600);
const WEEK_MS: u64 = 7 * 24 * 3600 * 1000;
/// The install id a preview shows before there is one (never sent: control
/// takes only 16 hex digits).
pub const PREVIEW_INSTALL: &str = "<made when you say yes>";
/// The most reports kept waiting.
const MAX_QUEUED: usize = 4 * MAX_EVENTS;

/// How the daemon reaches control, for a report: which one it's joined to,
/// and its signature on a request there.
pub trait Signer: Send + Sync {
    /// The control's URL and the enrolment's certificate signature, which
    /// a new join changes.
    fn joined(&self) -> Option<(String, String)>;
    /// The signature header's value for a POST of `body` to `path`.
    fn sign_post(&self, path: &str, body: &[u8]) -> Option<String>;
}

/// The environment, as a test gives it.
pub type Env = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// When things happen. Tests shorten them.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// From startup to `started`.
    pub started_after: Duration,
    /// How often the loop looks.
    pub tick: Duration,
    /// From one `daily` to the next.
    pub day: Duration,
    /// From a report that didn't go to the next try.
    pub retry: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            started_after: Duration::from_secs(60),
            tick: Duration::from_secs(60),
            day: Duration::from_secs(24 * 3600),
            retry: HOUR,
        }
    }
}

/// `telemetry.counts.json`: today's counts, and the reports not yet sent,
/// for the install they belong to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Kept {
    install: String,
    /// When today's counts began (ms).
    since: u64,
    counts: Counts,
    #[serde(default)]
    queue: Vec<Queued>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Queued {
    /// When it was queued (ms).
    at: u64,
    event: Event,
}

/// What the daemon keeps while it runs.
#[derive(Debug)]
struct Live {
    kept: Kept,
    /// `started` is queued, this run.
    started: bool,
    /// Counted since the last save: saved within the hour.
    dirty: bool,
    /// The queue changed since the last save: saved at once.
    queued: bool,
    saved: Instant,
    /// No try before then: the last one failed.
    wait_until: Option<Instant>,
    /// The enrolment control refused a signature from (its certificate's
    /// signature): reports go unsigned until it changes.
    refused: Option<String>,
}

/// The daemon's telemetry.
pub struct Telemetry {
    dir: PathBuf,
    url: String,
    env: Env,
    timing: Timing,
    service: bool,
    channel: Channel,
    up: Instant,
    http: reqwest::Client,
    signer: OnceLock<Arc<dyn Signer>>,
    /// Counting is on: refreshed at each look and each answer.
    on: AtomicBool,
    live: Mutex<Live>,
    wake: tokio::sync::Notify,
}

static NOW: OnceLock<Arc<Telemetry>> = OnceLock::new();

/// The running daemon's, once it has one.
pub fn get() -> Option<&'static Arc<Telemetry>> {
    NOW.get()
}

/// Count something, if telemetry is on. M103's view counts and M102's web
/// errors come in here too.
pub fn count(f: impl FnOnce(&mut Counts)) {
    if let Some(t) = get() {
        t.count(f);
    }
}

/// A client attached, from `surface`.
pub fn client(surface: Surface) {
    count(|c| *c.clients.entry(surface).or_default() += 1);
}

/// An agent block started (not one restored).
pub fn agent_started(runtime: AgentRuntime) {
    count(|c| *c.agents_started.entry(runtime).or_default() += 1);
}

/// The most one web client's note (`telemetry.note`, M103) adds to any one
/// count: a minute's is far less.
pub const NOTE_MAX: u32 = 1_000;
/// The most a day's count of a view, or of web errors, reaches.
pub const DAY_MAX: u32 = 100_000;

/// A web client's note, into the day's counts (if telemetry is on).
pub fn note(n: &TelemetryNoteRequest) {
    count(|c| add_note(c, n));
}

/// A note's views and web errors, into `c`: a view it doesn't know counts
/// as `other`, and each count is capped ([`NOTE_MAX`], [`DAY_MAX`]), so no
/// one page can run them up.
pub fn add_note(c: &mut Counts, n: &TelemetryNoteRequest) {
    let mut views = std::collections::BTreeMap::<View, u32>::new();
    for (name, k) in &n.views {
        let view = serde_json::from_value(Value::String(name.clone())).unwrap_or(View::Other);
        let v = views.entry(view).or_default();
        *v = v.saturating_add(*k).min(NOTE_MAX);
    }
    let add = |to: &mut u32, k: u32| *to = to.saturating_add(k.min(NOTE_MAX)).min(DAY_MAX);
    for (view, k) in views {
        add(c.views.entry(view).or_default(), k);
    }
    add(&mut c.web_errors, n.web_errors);
}

/// What the mux tells telemetry of (its `Usage`).
#[derive(Debug)]
pub struct Hooks;

impl arugula_mux::mux::Usage for Hooks {
    fn pane_opened(&self, on_machine: bool, open: usize) {
        count(|c| {
            c.panes_opened += 1;
            c.panes_peak = c.panes_peak.max(open as u32);
            let kind = if on_machine { BlockKind::Vm } else { BlockKind::Terminal };
            *c.blocks_opened.entry(kind).or_default() += 1;
        });
    }

    fn block_opened(&self, kind: BlockType, config: &Value) {
        let kind = block_kind(kind, config);
        count(|c| *c.blocks_opened.entry(kind).or_default() += 1);
    }
}

/// A block type as telemetry names it.
fn block_kind(kind: BlockType, config: &Value) -> BlockKind {
    match kind {
        BlockType::Terminal => BlockKind::Terminal,
        BlockType::Browser => BlockKind::Browser,
        BlockType::Agent if config.get("import").is_some_and(|i| !i.is_null()) => BlockKind::Conversation,
        BlockType::Agent => BlockKind::Agent,
        BlockType::Chat => BlockKind::Chat,
        BlockType::Editor => BlockKind::Editor,
        BlockType::Diff => BlockKind::Review,
        BlockType::File => BlockKind::Files,
        BlockType::Workspace => BlockKind::Workspace,
        BlockType::App => BlockKind::Studio,
        BlockType::Forge if config["kind"] == "issue" => BlockKind::Issue,
        BlockType::Forge => BlockKind::Pr,
        BlockType::Remote
        | BlockType::Invite
        | BlockType::Agents
        | BlockType::Doc
        | BlockType::TeamChat
        | BlockType::Board
        | BlockType::People
        | BlockType::Unknown => BlockKind::Other,
    }
}

/// An agent block's runtime, from what it runs (`claude`, `codex`,
/// `managoat`, or an ACP server's command).
pub fn runtime(agent: &str, command: &[String]) -> AgentRuntime {
    match agent {
        "claude" => AgentRuntime::Claude,
        "codex" => AgentRuntime::Codex,
        "managoat" | "fountain" => AgentRuntime::Cloud,
        _ => {
            let bin = command.first().map(|c| Path::new(c).file_name().unwrap_or_default().to_string_lossy());
            match bin.as_deref() {
                Some(b) if b.starts_with("gemini") => AgentRuntime::Gemini,
                Some(b) if b.starts_with("claude") => AgentRuntime::Claude,
                Some(b) if b.starts_with("codex") => AgentRuntime::Codex,
                _ => AgentRuntime::Other,
            }
        }
    }
}

/// How this binary was installed, from where it is: `Other` when unsure.
pub fn channel(exe: &Path, home: &Path) -> Channel {
    let p = exe.to_string_lossy().replace('\\', "/");
    if p.contains("/Cellar/") || p.contains("/homebrew/") || p.contains("/linuxbrew/") {
        Channel::Homebrew
    } else if p.ends_with(".app/Contents/MacOS/arugulad") || exe.with_file_name("arugula-desktop").exists() {
        Channel::Desktop
    } else if p.contains("/target/debug/") || p.contains("/target/release/") || exe.starts_with(home.join(".cargo/bin"))
    {
        Channel::Source
    } else if exe.starts_with(home.join(".local/bin")) || exe.starts_with(home.join("AppData/Local/Programs/arugula")) {
        // `arugulad install` puts it there, for install.sh and install.ps1
        // (and the desktop app, which runs the same).
        Channel::Script
    } else {
        Channel::Other
    }
}

/// Whether it runs as a service: systemd, launchd, Windows' logon task.
fn as_service(env: &Env) -> bool {
    ["INVOCATION_ID", "XPC_SERVICE_NAME", "ARUGULA_LOG_FILE"]
        .iter()
        .any(|k| env(k).is_some_and(|v| !v.is_empty() && v != "0"))
}

fn read_kept(dir: &Path) -> Option<Kept> {
    let b = std::fs::read(dir.join(COUNTS_FILE)).ok()?;
    serde_json::from_slice(&b)
        .map_err(|e| warn!(error = %e, "telemetry.counts.json doesn't parse: starting again"))
        .ok()
}

impl Telemetry {
    /// The daemon's, from its environment. A test build's is always off.
    pub fn from_env(dir: &Path, home: &Path) -> Arc<Self> {
        let env: Env = if cfg!(test) {
            Arc::new(|k: &str| (k == consent::ENV).then(|| "off".to_owned()))
        } else {
            Arc::new(|k: &str| std::env::var(k).ok())
        };
        let channel = std::env::current_exe().map(|e| channel(&e, home)).unwrap_or_default();
        Self::new(dir, env, Timing::default(), channel)
    }

    pub fn new(dir: &Path, env: Env, timing: Timing, channel: Channel) -> Arc<Self> {
        let state = consent::enabled_in(dir, &*env);
        let kept = read_kept(dir).filter(|k| state.install() == Some(k.install.as_str())).unwrap_or_default();
        // Built before `up` starts: loading the system's roots can take a
        // few hundred ms on a busy machine, which `started_after` mustn't
        // count.
        let http = crate::roots::http().timeout(Duration::from_secs(20)).build().expect("http client");
        Arc::new(Telemetry {
            dir: dir.to_owned(),
            url: consent::url_in(&*env),
            service: as_service(&env),
            env,
            timing,
            channel,
            up: Instant::now(),
            http,
            signer: OnceLock::new(),
            on: AtomicBool::new(state.on),
            live: Mutex::new(Live {
                kept,
                started: false,
                dirty: false,
                queued: false,
                saved: Instant::now(),
                wait_until: None,
                refused: None,
            }),
            wake: tokio::sync::Notify::new(),
        })
    }

    /// Becomes the daemon's ([`get`]), signing through `signer`, and looks
    /// every [`Timing::tick`] from now on.
    pub fn start(self: &Arc<Self>, signer: Arc<dyn Signer>) {
        let _ = self.signer.set(signer);
        let _ = NOW.set(self.clone());
        let me = self.clone();
        crate::task::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(me.timing.tick) => {}
                    _ = me.wake.notified() => {}
                }
                me.look().await;
            }
        });
    }

    fn state(&self) -> consent::State {
        consent::enabled_in(&self.dir, &*self.env)
    }

    pub fn count(&self, f: impl FnOnce(&mut Counts)) {
        if !self.on.load(Ordering::Relaxed) {
            return;
        }
        let mut l = self.live.lock().unwrap();
        f(&mut l.kept.counts);
        l.dirty = true;
    }

    /// The owner answered (`telemetry.set`): follow at once.
    pub fn answered(&self) {
        let state = self.state();
        self.on.store(state.on, Ordering::Relaxed);
        if !state.on {
            self.live.lock().unwrap().kept = Kept::default();
            self.no_crashes(&state);
        }
        self.wake.notify_one();
    }

    /// One look: follow the answer, queue what's due and send what's
    /// queued.
    pub async fn look(&self) {
        let state = self.state();
        self.on.store(state.on, Ordering::Relaxed);
        let Some(install) = state.install().map(str::to_owned) else {
            // Off: nothing kept in memory. A no deleted the file already.
            self.live.lock().unwrap().kept = Kept::default();
            self.no_crashes(&state);
            return;
        };
        let now = now_ms();
        let kept = {
            let mut l = self.live.lock().unwrap();
            if l.kept.install != install {
                // A new yes (or one made while the daemon was stopped).
                let via = state.consent.as_ref().and_then(|c| c.via.clone()).unwrap_or_default();
                let via = serde_json::from_value(Value::String(via)).unwrap_or(Surface::Other);
                l.kept = Kept {
                    install: install.clone(),
                    since: now,
                    queue: vec![Queued { at: now, event: Event::OptedIn { via } }],
                    ..Default::default()
                };
                l.wait_until = None;
                l.queued = true;
            }
            if !l.started && self.up.elapsed() >= self.timing.started_after {
                l.started = true;
                l.kept.queue.push(Queued { at: now, event: Event::Started { service: self.service } });
                l.queued = true;
            }
            if now.saturating_sub(l.kept.since) >= self.timing.day.as_millis() as u64 {
                let mut counts = std::mem::take(&mut l.kept.counts);
                counts.flags_on = self.flags_on();
                l.kept.queue.push(Queued { at: now, event: Event::Daily(counts) });
                l.kept.since = now;
                l.queued = true;
            }
            let before = l.kept.queue.len();
            l.kept.queue.retain(|q| now.saturating_sub(q.at) < WEEK_MS);
            let over = l.kept.queue.len().saturating_sub(MAX_QUEUED);
            l.kept.queue.drain(..over);
            l.queued |= l.kept.queue.len() != before;
            let waiting = l.wait_until.is_some_and(|t| Instant::now() < t);
            // Crashes go with a report that goes anyway.
            (!waiting && !l.kept.queue.is_empty()).then(|| l.kept.clone())
        };
        if let Some(kept) = kept {
            let crashes = self.crashes(now, true);
            let report = self.report(&kept, crashes.iter().map(|(_, c)| c.clone()).collect());
            let sent = self.send(&report).await;
            let mut l = self.live.lock().unwrap();
            if sent && l.kept.install == report.install {
                let n = report.events.len().min(l.kept.queue.len());
                l.kept.queue.drain(..n);
                for (file, _) in &crashes {
                    if let Err(e) = std::fs::remove_file(file)
                        && e.kind() != std::io::ErrorKind::NotFound
                    {
                        warn!(error = %e, file = %file.display(), "can't delete a crash that was sent");
                    }
                }
                l.wait_until = None;
                l.queued = true;
            } else if !sent {
                l.wait_until = Some(Instant::now() + self.timing.retry);
            }
        }
        let save = {
            let l = self.live.lock().unwrap();
            l.queued || (l.dirty && l.saved.elapsed() >= HOUR)
        };
        if save {
            self.save();
        }
    }

    /// The crashes waiting in `crashes/`, oldest first, at most a report's
    /// worth, with their files. With `tidy`, a file that doesn't parse, a
    /// crash control wouldn't take and one a week old are deleted on the
    /// way; without, they're only left out.
    fn crashes(&self, now: u64, tidy: bool) -> Vec<(PathBuf, Crash)> {
        let mut out = Vec::new();
        for (_, file) in crash::list(&self.dir) {
            if out.len() == MAX_CRASHES {
                break;
            }
            let c = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Crash>(&b).ok());
            match c.filter(|c| now.saturating_sub(c.at) < WEEK_MS && takes(c)) {
                Some(c) => out.push((file, c)),
                None if tidy => {
                    debug!(file = %file.display(), "a crash control wouldn't take: deleted");
                    let _ = std::fs::remove_file(&file);
                }
                None => {}
            }
        }
        out
    }

    /// Off because the owner said no (by hand in the file, say): no
    /// crashes either. Off for any other reason, they wait.
    fn no_crashes(&self, state: &consent::State) {
        if state.why == consent::Why::No
            && let Err(e) = crash::clear(&self.dir)
        {
            warn!(error = %e, "can't empty crashes/");
        }
    }

    /// Writes what's kept, if telemetry is on and there's anything new.
    pub fn save(&self) {
        let mut l = self.live.lock().unwrap();
        let new = l.dirty || l.queued;
        if !new || l.kept.install.is_empty() || self.state().install() != Some(l.kept.install.as_str()) {
            return;
        }
        let mut text = serde_json::to_vec_pretty(&l.kept).expect("the counts serialize");
        text.push(b'\n');
        match write_private(&self.dir.join(COUNTS_FILE), &text) {
            Ok(()) => {
                l.dirty = false;
                l.queued = false;
                l.saved = Instant::now();
            }
            Err(e) => warn!(error = %e, "can't save telemetry.counts.json"),
        }
    }

    fn flags_on(&self) -> Vec<LabsFlag> {
        arugula_proto::flags::all(&self.dir)
            .into_iter()
            .filter(|(_, on)| *on)
            .filter_map(|(f, _)| serde_json::from_value(Value::String(f.name.into())).ok())
            .collect()
    }

    fn control_kind(&self) -> ControlKind {
        match self.signer.get().and_then(|s| s.joined()) {
            None => ControlKind::None,
            Some((url, _)) if same(&url, consent::HOSTED) => ControlKind::Hosted,
            Some(_) => ControlKind::SelfHosted,
        }
    }

    /// The next report from what's `kept`, at most a report's worth.
    fn report(&self, kept: &Kept, crashes: Vec<Crash>) -> Report {
        Report {
            install: kept.install.clone(),
            version: arugula_core::version::get().into(),
            os: match std::env::consts::OS {
                "linux" => Os::Linux,
                "macos" => Os::Macos,
                "windows" => Os::Windows,
                _ => Os::Other,
            },
            arch: match std::env::consts::ARCH {
                "x86_64" => Arch::X86_64,
                "aarch64" => Arch::Aarch64,
                _ => Arch::Other,
            },
            channel: self.channel,
            control: self.control_kind(),
            events: kept.queue.iter().take(MAX_EVENTS).map(|q| q.event.clone()).collect(),
            crashes,
        }
    }

    /// The enrolment to sign with: joined to the control reports go to, and
    /// not one control refused.
    fn signing(&self) -> Option<(&Arc<dyn Signer>, String)> {
        let s = self.signer.get()?;
        let (url, cert) = s.joined()?;
        let refused = self.live.lock().unwrap().refused.clone();
        (same(&url, &self.url) && refused.as_deref() != Some(cert.as_str())).then_some((s, cert))
    }

    /// Whether control took it.
    async fn send(&self, report: &Report) -> bool {
        if let Err(why) = report.check() {
            warn!(why, "a telemetry report control wouldn't take: dropped");
            return true;
        }
        let body = serde_json::to_vec(report).expect("a report serializes");
        let post = |sig: Option<String>| {
            let mut r = self
                .http
                .post(format!("{}{TELEMETRY}", self.url))
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.clone());
            if let Some(sig) = sig {
                r = r.header(crate::control::AUTH, sig);
            }
            r.send()
        };
        let signing = self.signing();
        let sig = signing.as_ref().and_then(|(s, _)| s.sign_post(TELEMETRY, &body));
        let signed = sig.is_some();
        let mut res = post(sig).await;
        if signed && res.as_ref().is_ok_and(|r| r.status() == reqwest::StatusCode::UNAUTHORIZED) {
            // A stale enrolment: unsigned from now on, until it changes.
            info!("control refused this machine's signature on telemetry: sending it unsigned");
            self.live.lock().unwrap().refused = signing.map(|(_, cert)| cert);
            res = post(None).await;
        }
        match res {
            Ok(r) if r.status().is_success() => {
                debug!(events = report.events.len(), signed, "telemetry sent");
                true
            }
            Ok(r) => {
                debug!(status = %r.status(), "telemetry not taken: trying again later");
                false
            }
            Err(e) => {
                debug!(error = %e, "telemetry not sent: trying again later");
                false
            }
        }
    }

    /// `telemetry.get`: the state, and the next report as it would go now:
    /// what's queued, then today's counts so far as the coming `daily`.
    /// While it's off, what a yes would send first, with this machine's
    /// own version, OS, arch, channel and control, no counts, and
    /// [`PREVIEW_INSTALL`] for the install id, marked `preview_only`.
    pub fn info(&self) -> TelemetryInfo {
        let state = self.state();
        let first = |install: &str| {
            let via = state.consent.as_ref().and_then(|c| c.via.clone()).unwrap_or_default();
            let via = serde_json::from_value(Value::String(via)).unwrap_or(Surface::Other);
            let mut queue = vec![Queued { at: 0, event: Event::OptedIn { via } }];
            queue.push(Queued { at: 0, event: Event::Started { service: self.service } });
            Kept { install: install.to_owned(), queue, ..Default::default() }
        };
        let mut kept = match state.install() {
            Some(install) => {
                let l = self.live.lock().unwrap();
                if l.kept.install == install { l.kept.clone() } else { first(install) }
            }
            None => first(PREVIEW_INSTALL),
        };
        if !kept.queue.iter().any(|q| matches!(q.event, Event::Daily(_))) {
            let mut counts = kept.counts.clone();
            counts.flags_on = self.flags_on();
            kept.queue.push(Queued { at: 0, event: Event::Daily(counts) });
        }
        let crashes =
            if state.on { self.crashes(now_ms(), false).into_iter().map(|(_, c)| c).collect() } else { vec![] };
        let mut preview = serde_json::to_value(self.report(&kept, crashes)).expect("a report serializes");
        if state.install().is_none() {
            preview["preview_only"] = Value::Bool(true);
        }
        let preview = Some(preview);
        let c = state.consent.as_ref();
        TelemetryInfo {
            on: state.on,
            why: state.why,
            asked: c.map(|c| c.asked),
            via: c.and_then(|c| c.via.clone()),
            install: state.install().map(str::to_owned),
            url: self.url.clone(),
            signed: self.signer.get().and_then(|s| s.joined()).is_some_and(|(u, _)| same(&u, &self.url)),
            preview,
        }
    }

    /// How it reaches control, for one that isn't started
    /// (`arugulad telemetry status`, with [`FromFile`]).
    pub fn set_signer(&self, signer: Arc<dyn Signer>) {
        let _ = self.signer.set(signer);
    }
}

/// Whether control takes `crash`, alone in a report.
fn takes(crash: &Crash) -> bool {
    let r = Report {
        install: "0000000000000000".into(),
        version: "0.0.0".into(),
        os: Os::Other,
        arch: Arch::Other,
        channel: Channel::Other,
        control: ControlKind::None,
        events: vec![],
        crashes: vec![crash.clone()],
    };
    r.check().is_ok()
}

fn same(a: &str, b: &str) -> bool {
    a.trim_end_matches('/').eq_ignore_ascii_case(b.trim_end_matches('/'))
}

/// A new file renamed over the old, private to the owner.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let r = (|| {
        let mut f = o.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

/// The control in `control.json`, for a process that isn't the daemon
/// (`arugulad telemetry status`): it says where, and signs nothing.
pub struct FromFile(pub Option<(String, String)>);

impl Signer for FromFile {
    fn joined(&self) -> Option<(String, String)> {
        self.0.clone()
    }

    fn sign_post(&self, _: &str, _: &[u8]) -> Option<String> {
        None
    }
}

/// The surface of a WebSocket client, from its request's headers and query.
/// The web client on a phone says so in its User-Agent. The desktop app's
/// webview looks like a browser there, so the page says it's the app's
/// window itself (`/ws?via=desktop`, M103).
pub fn surface_of(headers: &axum::http::HeaderMap, query: Option<&str>) -> Surface {
    if query.is_some_and(|q| q.split('&').any(|kv| kv == "via=desktop")) {
        return Surface::Desktop;
    }
    let ua = headers.get(axum::http::header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    if ["iPhone", "iPad", "Android", "Mobile"].iter().any(|m| ua.contains(m)) { Surface::Phone } else { Surface::Web }
}

#[cfg(test)]
mod tests;
