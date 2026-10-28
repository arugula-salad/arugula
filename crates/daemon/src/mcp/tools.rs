//! What MCP's tools share (M16, #232): a call ([`Call`]: who, and how to
//! tell them it's still going), a tool's row in the list ([`Def`], a
//! grouped tool's [`Kind`]) and its line on the person's card ([`Human`]),
//! and an answer ([`done`]). The operations' half (`ops.rs`), the feature
//! crates' tools (`registry.rs`) and MCP's server (`mcp_server`, the table
//! and the tools' calls) are each made of these.

use std::{future::Future, sync::Arc, time::Duration};

use arugula_proto::{
    Driver, PaneId, StartedBy,
    api::{HistoryKind, OpenRequest, PaneSummary},
};
use rmcp::{
    RoleServer,
    handler::server::common::schema_for_type,
    model::{ProgressNotificationParam, ProgressToken},
    service::RequestContext,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::time::Instant;

use super::{
    Caller, Scope,
    results::{self},
};
use crate::{
    history::{self, Filter},
    mux::Api,
    server::App,
    store::now_ms,
};

/// A page of output, by default (characters).
pub const PAGE: usize = 16_000;
/// The most any one result carries: Claude Code swaps anything over about
/// 50,000 for a preview and a file (S14).
pub const PAGE_MAX: usize = 40_000;
/// How long a wait (or `run` with `wait`) waits before answering "still
/// running": under interactive Claude Code's 120s auto-background.
const WAIT_DEFAULT: Duration = Duration::from_secs(100);
const WAIT_MAX: Duration = Duration::from_secs(3600);
/// Progress while waiting: over HTTP, Claude Code drops a call that's
/// silent for 60s.
const PROGRESS_EVERY: Duration = Duration::from_secs(15);

fn progress_every() -> Duration {
    // Tests make it short.
    std::env::var("ARUGULA_MCP_PROGRESS_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(PROGRESS_EVERY)
}

// ---------------------------------------------------------------- arguments

/// A pane or block: `7` or `"%7"`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PaneArg {
    Id(PaneId),
    Name(String),
}

impl PaneArg {
    pub fn id(&self) -> Result<PaneId, String> {
        match self {
            PaneArg::Id(n) => Ok(*n),
            PaneArg::Name(s) => s
                .trim()
                .trim_start_matches('%')
                .parse()
                .map_err(|_| format!("{s:?} isn't a pane: give its number, like 7 or \"%7\"")),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct PaneOnly {
    pub pane: PaneArg,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct ListArgs {}

// Read only by the handler, which a build without Labs lacks.
#[cfg_attr(not(feature = "labs"), allow(dead_code))]
#[derive(Deserialize, JsonSchema)]
pub struct OpenAppArgs {
    /// The app's name in the user's studio. Leave it out to list their
    /// apps instead.
    #[serde(default)]
    pub app: Option<String>,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
}

// Read only by the handler, which a build without Labs lacks.

// Read only by the delegate handler, which a build without Labs lacks.
#[cfg_attr(not(feature = "labs"), allow(dead_code))]
#[derive(Deserialize, JsonSchema)]
pub struct DelegateArgs {
    /// The machine that offers the agent, by name (as list kind agents
    /// gives it) or id; this one's too. Leave it out to use the one that
    /// offers it (this machine first).
    #[serde(default)]
    pub machine: Option<String>,
    /// The agent, by name.
    pub agent: String,
    /// send: deliver the work as a pull request in the agent's own project,
    /// which it opens there (not as a patch for you to apply): for a fix
    /// in its project's code, not yours.
    #[serde(default)]
    pub pr: bool,
    /// send: the task; answer: the answer to the question it asked.
    #[serde(default)]
    pub text: Option<String>,
    /// get, answer, cancel: the task's id, as send returned it.
    #[serde(default)]
    pub task: Option<String>,
    /// How long to wait for it to finish, or to ask you something
    /// (seconds; default 300, at most 1800; 0: don't wait).
    #[serde(default)]
    pub wait: Option<u64>,
    /// review, apply: your checkout (default: where you work).
    #[serde(default)]
    pub dir: Option<String>,
}

// Read only by the catalog's handler, which a build without Labs lacks.
#[cfg_attr(not(feature = "labs"), allow(dead_code))]
#[derive(Deserialize, JsonSchema)]
pub struct ListTeamAgentsArgs {
    /// Words to find in an agent's name or description, or its machine's
    /// or owner's name.
    #[serde(default)]
    pub query: Option<String>,
    /// Ask every machine now, not from the last minute's read.
    #[serde(default)]
    pub fresh: bool,
}

// ---------------------------------------------------------------- the list

pub type Schema = Arc<serde_json::Map<String, Value>>;

pub struct Def {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub args: Args,
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    pub open_world: bool,
    /// The flag the machine needs on for it to be listed (`Op::FLAG`).
    pub flag: Option<&'static str>,
    /// Its answer's schema (`outputSchema`), when it is always one type: the
    /// answer's, with `summary`. A grouped tool answers by kind, and a tool
    /// that answers with more than one type in some case, has none.
    pub output: Option<fn() -> Schema>,
    /// What it lets the person ask for, in their words: the card *What your
    /// agent can do here* is these ([`can_do`]).
    pub human: Human,
}

/// A tool's place on the person's card. Every tool has one, so a new tool
/// shows there and a removed one goes (`every_tool_has_a_human_line`).
pub enum Human {
    /// A line of its own.
    Line(About),
    /// Part of another tool's line, named: `wait` is how `run`'s line works,
    /// not something a person asks for. The name is for the test that holds
    /// the card complete (`every_tool_has_a_human_line`).
    In(#[cfg_attr(not(test), allow(dead_code))] &'static str),
}

/// One line of the card.
pub struct About {
    /// The rest of "Ask it to …".
    pub ask: &'static str,
    /// A prompt the person can copy.
    pub example: &'static str,
    /// What this machine needs for it to work, as `capabilities` says.
    pub needs: Needs,
}

/// What a line needs beyond its tool's flag, each as `capabilities` reports
/// it: a forge login here, or control, and a grant there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Needs {
    Nothing,
    /// The user signed in to a forge here (`forges`).
    Forge,
    /// This machine joined to Arugula control (`control.state`).
    Control,
    /// The user's Cloud workspaces reachable (`control.cloud`).
    Cloud,
    /// A thread grant (`control.threads`).
    Threads,
    /// An issues.write grant (`control.threads`' scopes).
    Issues,
    /// A run grant (`control.runs`).
    Runs,
}

pub const fn line(ask: &'static str, example: &'static str, needs: Needs) -> Human {
    Human::Line(About { ask, example, needs })
}

/// What a tool takes.
pub enum Args {
    /// One set of arguments, for a tool that does one thing.
    One(fn() -> Schema),
    /// A grouped tool (#349): its `kind` argument picks one of these jobs,
    /// and its schema is theirs merged. `default`: the kind when none is
    /// given (none: kind is required).
    Kinds { kinds: Vec<&'static Kind>, default: Option<&'static str> },
}

/// One job of a grouped tool, like `show {kind: "port", port: 3000}`. Each
/// is a row, so the kinds that belong to one feature (the studio's) can be left out of their group, and their arguments with them,
/// by filtering its table ([`defs`]).
pub struct Kind {
    pub name: &'static str,
    /// What it does, for the tool's description.
    pub description: &'static str,
    pub schema: fn() -> Schema,
    /// The flag the machine needs on for it to be listed (`Op::FLAG`).
    pub flag: Option<&'static str>,
}

/// A group's table as a tool holds it.
pub fn table(kinds: &'static [Kind]) -> Vec<&'static Kind> {
    kinds.iter().collect()
}

/// No arguments.
#[derive(Deserialize, JsonSchema)]
pub struct NoArgs {}

// ---------------------------------------------------------------- calls

pub type Out = Result<Value, String>;

/// The `outputSchema` of a tool whose successful answer is always a `T`: the
/// schema of `T` as `done` sends it, which is the object with `summary` in it
/// (a required string). The root is `type: object` (an enum's `oneOf` gets
/// it here), since MCP clients may check `structuredContent` against it.
pub fn output_schema<T: JsonSchema + 'static>() -> Schema {
    let mut s = (*schema_for_type::<T>()).clone();
    // The wrapper type's name and doc are noise to an agent.
    s.remove("title");
    s.remove("description");
    s.entry("type").or_insert_with(|| Value::String("object".into()));
    let props = s.entry("properties").or_insert_with(|| Value::Object(Default::default()));
    props["summary"] = json!({ "type": "string" });
    let required = s.entry("required").or_insert_with(|| Value::Array(vec![]));
    required.as_array_mut().expect("required is a list").push(Value::String("summary".into()));
    let mut s = Value::Object(s);
    strip_formats(&mut s);
    let Value::Object(s) = s else { unreachable!() };
    Arc::new(s)
}

/// The formats JSON Schema 2020-12 defines; schemars' others (`uint32`,
/// `int64`, `double`…) are Rust's, and clients warn about each one.
pub const STANDARD_FORMATS: &[&str] = &[
    "date-time",
    "date",
    "time",
    "duration",
    "email",
    "idn-email",
    "hostname",
    "idn-hostname",
    "ipv4",
    "ipv6",
    "uri",
    "uri-reference",
    "iri",
    "iri-reference",
    "uuid",
    "uri-template",
    "json-pointer",
    "relative-json-pointer",
    "regex",
];

/// Drops every `format` that isn't a standard one, anywhere in the schema.
fn strip_formats(v: &mut Value) {
    match v {
        Value::Object(o) => {
            if o.get("format").and_then(Value::as_str).is_some_and(|f| !STANDARD_FORMATS.contains(&f)) {
                o.remove("format");
            }
            o.values_mut().for_each(strip_formats);
        }
        Value::Array(a) => a.iter_mut().for_each(strip_formats),
        _ => {}
    }
}

pub fn done(summary: impl Into<String>, v: impl Serialize) -> Out {
    let mut v = serde_json::to_value(v).map_err(|e| e.to_string())?;
    v["summary"] = Value::String(summary.into());
    Ok(v)
}

pub fn parse<T: DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|e| format!("bad arguments: {e}"))
}

/// One tool call (or resource read): who, and how to tell them it's still
/// going.
pub struct Call<'a> {
    pub app: &'a Arc<App>,
    pub caller: Caller,
    /// The client's name (`claude-code`), for "started by mcp:<client>".
    pub client: String,
    progress: Option<ProgressToken>,
    /// The MCP request, when there is one: a call another machine sends
    /// for a cloud workspace (M184, `hosted.rs`) has none, so no progress
    /// and no headers.
    pub ctx: Option<RequestContext<RoleServer>>,
}

impl<'a> Call<'a> {
    pub fn new(app: &'a Arc<App>, caller: Caller, client: String, ctx: &RequestContext<RoleServer>) -> Self {
        Self { app, caller, client, progress: ctx.meta.get_progress_token(), ctx: Some(ctx.clone()) }
    }

    /// A call with no MCP request behind it (M184: another machine's, for
    /// this hosted one).
    pub fn detached(app: &'a Arc<App>, caller: Caller, client: String) -> Self {
        Self { app, caller, client, progress: None, ctx: None }
    }

    pub fn app(&self) -> &'a Arc<App> {
        self.app
    }

    pub fn by(&self) -> String {
        format!("mcp:{}", self.client)
    }

    pub fn driver(&self) -> Driver {
        Driver { who: self.by(), name: self.by() }
    }

    // ------------------------------------------------------------ helpers
    pub async fn panes(&self) -> Vec<PaneSummary> {
        self.app.mux.api(Api::Panes).await.unwrap_or_default()
    }

    /// Whether this caller is an agent on a machine (a guest's, say): what
    /// it creates must land on a machine too ([`confine`]).
    pub async fn on_machine(&self) -> Result<bool, String> {
        match self.me() {
            Some(me) => Ok(self.readable(me).await?.info.host.is_some()),
            None => Ok(false),
        }
    }

    /// Where a tool defaults to when it's given no pane: the caller's own
    /// (an agent block, or the terminal pane `arugula mcp` runs in).
    pub fn own_pane(&self) -> Option<PaneId> {
        self.caller.pane
    }

    /// What the caller may reach.
    pub fn scope(&self) -> Scope {
        self.caller.scope
    }

    /// The agent block whose token this is: what confines a caller.
    pub fn me(&self) -> Option<PaneId> {
        self.caller.scope.block()
    }

    /// An open pane or block, if this caller may see it.
    pub async fn readable(&self, pane: PaneId) -> Result<PaneSummary, String> {
        let panes = self.panes().await;
        let Some(p) = panes.iter().find(|p| p.info.id == pane).cloned() else {
            return Err(self.gone(pane).await);
        };
        if let Some(me) = self.me() {
            let tab = panes.iter().find(|p| p.info.id == me).map(|p| p.tab);
            if tab != Some(p.tab) {
                return Err(format!("%{pane} is in another tab: this agent's token reaches its own tab only"));
            }
        }
        Ok(p)
    }

    /// An open pane or block this caller may type in, answer for or close:
    /// anything, or for an agent block's token, what it started.
    pub async fn drivable(&self, pane: PaneId) -> Result<PaneSummary, String> {
        let p = self.readable(pane).await?;
        if let Some(me) = self.me()
            && p.info.started_by.as_ref().and_then(|s| s.block) != Some(me)
        {
            return Err(format!("%{pane} wasn't started by this agent: it may read it, not drive or close it"));
        }
        Ok(p)
    }

    /// Why a pane isn't there, as specifically as history can say.
    pub async fn gone(&self, pane: PaneId) -> String {
        let store = self.app.mux.store.clone();
        let last = crate::task::spawn_blocking(move || {
            history::history(
                &store,
                &Filter { pane: Some(pane), kind: Some(HistoryKind::Command), ..Default::default() },
                1,
            )
        })
        .await
        .ok()
        .and_then(|h| h.into_iter().next());
        match last {
            Some(h) => {
                let what = h.text.as_deref().map(|t| format!(" `{}`", one_line(t, 80))).unwrap_or_default();
                let how = match h.exit {
                    Some(code) => format!("exited {code}"),
                    None => "was still running".into(),
                };
                let when = ago(h.ended_ms.unwrap_or(h.started_ms));
                format!("pane %{pane} is gone; its last command{what} {how} {when} (read_output still reads it)")
            }
            None => format!("no pane %{pane} (list shows what's open)"),
        }
    }

    pub async fn started(&self, pane: PaneId) {
        let by = StartedBy { by: self.by(), block: self.me() };
        self.app.mux.send(crate::mux::Cmd::Api(Api::StartedBy(pane, by)));
    }

    async fn notify(&self, done: f64, total: Option<f64>, message: String) {
        let (Some(token), Some(ctx)) = (&self.progress, &self.ctx) else { return };
        let mut p = ProgressNotificationParam::new(token.clone(), done).with_message(message);
        if let Some(t) = total {
            p = p.with_total(t);
        }
        let _ = ctx.peer.notify_progress(p).await;
    }

    /// `fut`, with progress every 15s, for at most `limit`: `None` if it
    /// took longer (or the client gave up).
    pub async fn waiting<T>(&self, what: &str, limit: Duration, fut: impl Future<Output = T>) -> Option<T> {
        let start = Instant::now();
        let every = progress_every();
        let mut tick = tokio::time::interval_at(start + every, every);
        let deadline = tokio::time::sleep(limit);
        tokio::pin!(fut, deadline);
        loop {
            tokio::select! {
                r = &mut fut => return Some(r),
                _ = &mut deadline => return None,
                _ = async { match &self.ctx { Some(c) => c.ct.cancelled().await, None => std::future::pending().await } } => return None,
                _ = tick.tick() => {
                    let secs = start.elapsed().as_secs_f64();
                    self.notify(secs, Some(limit.as_secs_f64()), format!("{what}: {}s", secs.round())).await;
                }
            }
        }
    }

    pub fn limit(timeout: Option<f64>) -> Duration {
        timeout
            .filter(|t| t.is_finite() && *t >= 0.0)
            .map(Duration::from_secs_f64)
            .unwrap_or(WAIT_DEFAULT)
            .min(WAIT_MAX)
    }

    /// Beside `beside`, or the agent itself; on its machine.
    pub async fn beside(&self, beside: Option<&PaneArg>) -> Result<(Option<PaneId>, Option<u32>), String> {
        let beside = match (beside.map(PaneArg::id).transpose()?, self.own_pane()) {
            (Some(b), _) => Some(b),
            (None, own) => own,
        };
        let host = match beside {
            Some(b) => self.readable(b).await?.info.host,
            None => None,
        };
        Ok((beside, host))
    }
}

impl Call<'_> {
    /// One forge operation on the platform (M169): its result, a refusal
    /// as `said` puts it. Never one that sends: only a person does.
    pub async fn forge_op(
        &self,
        op: arugula_control_wire::forge_ops::Op,
        said: fn(crate::control::CloudRefused) -> String,
    ) -> Result<Value, String> {
        if op.sends() {
            return Err("only a person sends: draft it for them".into());
        }
        let body = serde_json::to_value(arugula_control_wire::forge_ops::Call::new(op)).map_err(|e| e.to_string())?;
        let path = arugula_control_wire::forge_ops::FORGE_OPS;
        let v = self.app.control.cloud(reqwest::Method::POST, path, Some(&body)).await.map_err(said)?;
        Ok(v["result"].clone())
    }

    pub async fn open(&self, req: OpenRequest) -> Result<PaneId, String> {
        confine(self.on_machine().await?, open_lands_on_machine(&req))?;
        match self.app.mux.api(|r| Api::Open(req, None, r)).await {
            Some(Ok(b)) => {
                self.started(b).await;
                Ok(b)
            }
            Some(Err(e)) => Err(e),
            None => Err("the daemon is shutting down".into()),
        }
    }
}

/// A pane in `list`.
pub fn entry(p: &PaneSummary) -> results::PaneEntry {
    let i = &p.info;
    results::PaneEntry {
        pane: i.id,
        kind: i.kind,
        session: p.session_name.clone(),
        tab: p.tab,
        tab_name: p.tab_name.clone(),
        machine: i.host.map(|m| format!("m{m}")),
        cwd: i.cwd.clone(),
        command: i.command.clone(),
        title: i.title.clone(),
        running: i.running,
        attention: i.attention,
        why: i.reason.as_ref().map(|r| r.headline.clone()),
        current: i.current.as_ref().and_then(|c| c.text.clone()),
        last: i.last.as_ref().map(|c| results::LastCommand { command: c.text.clone(), exit: c.exit }),
        started_by: i.started_by.as_ref().map(|s| s.by.clone()),
        you: None,
    }
}

pub fn one_line(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    if line.chars().count() <= max {
        return line.to_owned();
    }
    let mut out: String = line.chars().take(max).collect();
    out.push('…');
    out
}

pub fn ago(ms: u64) -> String {
    let secs = now_ms().saturating_sub(ms) / 1000;
    match secs {
        0..60 => format!("{secs}s ago"),
        60..3600 => format!("{}m ago", secs / 60),
        3600..86400 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}

/// The one rule for every tool that creates something (a shell, an agent,
/// a block): an agent on a machine (a guest's, say) creates things on a
/// machine, never on this host, where they'd run as the owner (with the
/// owner's logins and, for a worn recipe, secrets).
pub fn confine(caller_on_machine: bool, lands_on_machine: bool) -> Result<(), String> {
    if caller_on_machine && !lands_on_machine {
        return Err("an agent on a machine creates things on its machine (or a new VM), not on this host".into());
    }
    Ok(())
}

/// Where a block opens: a machine when it has one or makes one.
pub fn open_lands_on_machine(req: &OpenRequest) -> bool {
    req.vm || req.host.is_some()
}
