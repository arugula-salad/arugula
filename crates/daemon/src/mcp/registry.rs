//! MCP tools and kinds a feature crate adds (#232). The tool list
//! (`arugulad-mcp`'s `tools.rs`) is the base's table with these put in where each says, so
//! the list an agent sees keeps its order (and `mcp-tool-list.json`, which
//! pins it, its lines) whichever crate a tool lives in. A call to one runs
//! its [`Run`]. Registered before `main`, by the binary.
//!
//! Control's tools (`arugulad-control-tools`) also say what the base asks
//! of them: whether they reach control from here, and `wait` on a run
//! or an event ([`ControlTools`]).

use std::{
    sync::{Mutex, OnceLock},
    time::Duration,
};

use futures_util::future::BoxFuture;
use serde_json::Value;

use super::{
    results::{BoardsReach, CloudReach, RunsReach, ThreadsReach, TrackersReach},
    tools::{Args, Call, Def, Kind, Out},
};
use crate::server::App;

/// What the base asks of control's tools: `capabilities` and the card
/// *What your agent can do here* say whether each reaches control from
/// here, `wait` with `run` follows a run, and with `agent_session` agent
/// sessions.
pub trait ControlTools: Send + Sync + 'static {
    /// Whether agents here drive the owner's Cloud workspaces (#1094).
    fn cloud_reach<'a>(&'a self, app: &'a App) -> BoxFuture<'a, CloudReach>;
    /// Whether thread tools are live here (M198).
    fn threads_reach<'a>(&'a self, app: &'a App) -> BoxFuture<'a, ThreadsReach>;
    /// Whether run tools are live here (M216).
    fn runs_reach<'a>(&'a self, app: &'a App) -> BoxFuture<'a, RunsReach>;
    /// Whether `tracker_write` is live here (#359).
    fn trackers_reach<'a>(&'a self, app: &'a App) -> BoxFuture<'a, TrackersReach>;
    /// The projects whose boards the machine's grants reach.
    fn boards_reach<'a>(&'a self, app: &'a App) -> BoxFuture<'a, BoardsReach>;
    /// `wait` with `run`: until it ends, or `limit`.
    fn wait_run<'a>(&'a self, call: &'a Call<'a>, run: &'a str, limit: Duration) -> BoxFuture<'a, Out>;
    /// `wait` with `agent_session`: until any of them settles, or `limit`.
    fn wait_sessions<'a>(&'a self, call: &'a Call<'a>, ids: &'a str, limit: Duration) -> BoxFuture<'a, Out>;
    /// `wait` with `event`: a short-lived watch, until it fires or `limit`.
    fn wait_event<'a>(
        &'a self,
        call: &'a Call<'a>,
        event: &'a str,
        matches: Option<serde_json::Map<String, Value>>,
        limit: Duration,
    ) -> BoxFuture<'a, Out>;
    /// The chief of staff's first prompt as an agent block (Arugula #1337):
    /// its standing instructions, then `prompt`.
    fn chief_of_staff_prompt(&self, prompt: &str) -> String;
    /// A board lead's first prompt (Arugula #1440): its standing
    /// instructions, then `prompt`.
    fn board_lead_prompt(&self, prompt: &str) -> String;
    /// An issue agent's first prompt (Arugula #1440): its standing
    /// instructions, then `prompt`.
    fn issue_agent_prompt(&self, prompt: &str) -> String;
    /// Make `session`, an agent block's on this machine, the lead of the
    /// person's staff room in `project` (Arugula #1338). Answers the room.
    fn lead_staff<'a>(&'a self, app: &'a App, project: &'a str, session: &'a str) -> BoxFuture<'a, Out>;
    /// The person's staff room in `project`, with its lead (Arugula #1340).
    fn staff_room<'a>(&'a self, app: &'a App, project: &'a str) -> BoxFuture<'a, Out>;
    /// Tell the staff room's lead `session` that it's moved (Arugula #1340):
    /// `notice` in `room`, by `by`, the session taking over, as this
    /// machine's agent; or, for a hosted session (`ses_…`), stop it with
    /// `notice` as the reason.
    fn stand_down<'a>(
        &'a self,
        app: &'a App,
        room: &'a str,
        session: &'a str,
        notice: &'a str,
        by: &'a str,
    ) -> BoxFuture<'a, Out>;
}

static CONTROL_TOOLS: OnceLock<Box<dyn ControlTools>> = OnceLock::new();

/// Puts control's tools in this daemon (once, before `main`).
pub fn control_tools(c: Box<dyn ControlTools>) {
    if CONTROL_TOOLS.set(c).is_err() {
        tracing::warn!("control's tools were registered twice");
    }
}

/// What this build has none of says so.
const NO_CONTROL_TOOLS: &str = "This build of Arugula has no control tools";

/// What [`ControlTools`] says of Cloud workspaces, conversations and
/// runs from here; none reached without them.
pub async fn reach(app: &App) -> (CloudReach, ThreadsReach, RunsReach, TrackersReach) {
    match CONTROL_TOOLS.get() {
        Some(c) => tokio::join!(c.cloud_reach(app), c.threads_reach(app), c.runs_reach(app), c.trackers_reach(app)),
        None => (
            CloudReach { reachable: false, note: NO_CONTROL_TOOLS.into() },
            ThreadsReach { live: false, scopes: vec![], note: NO_CONTROL_TOOLS.into(), answered: false },
            RunsReach { live: false, projects: vec![], note: NO_CONTROL_TOOLS.into(), answered: false },
            TrackersReach { live: false, projects: vec![], note: NO_CONTROL_TOOLS.into() },
        ),
    }
}

/// What [`ControlTools`] says of boards from here: asked once, and only
/// where boards are listed.
pub async fn boards_reach(app: &App) -> BoardsReach {
    match CONTROL_TOOLS.get() {
        Some(c) => c.boards_reach(app).await,
        None => BoardsReach { projects: vec![], note: NO_CONTROL_TOOLS.into(), answered: false },
    }
}

/// `wait` with `run` ([`ControlTools::wait_run`]).
pub async fn wait_run(call: &Call<'_>, run: &str, limit: Duration) -> Out {
    match CONTROL_TOOLS.get() {
        Some(c) => c.wait_run(call, run, limit).await,
        None => Err(format!("{NO_CONTROL_TOOLS}: it can't wait on a run")),
    }
}

/// `wait` with `agent_session` ([`ControlTools::wait_sessions`]).
pub async fn wait_sessions(call: &Call<'_>, ids: &str, limit: Duration) -> Out {
    match CONTROL_TOOLS.get() {
        Some(c) => c.wait_sessions(call, ids, limit).await,
        None => Err(format!("{NO_CONTROL_TOOLS}: it can't wait on sessions")),
    }
}

/// `wait` with `event` ([`ControlTools::wait_event`]).
pub async fn wait_event(
    call: &Call<'_>,
    event: &str,
    matches: Option<serde_json::Map<String, Value>>,
    limit: Duration,
) -> Out {
    match CONTROL_TOOLS.get() {
        Some(c) => c.wait_event(call, event, matches, limit).await,
        None => Err(format!("{NO_CONTROL_TOOLS}: it can't wait on an event")),
    }
}

/// The recipe `start_agent` starts a chief of staff with (Arugula #1338),
/// as `session_start` takes it for a hosted one.
pub const CHIEF_OF_STAFF: &str = "chief-of-staff";

/// [`ControlTools::chief_of_staff_prompt`].
pub fn chief_of_staff_prompt(prompt: &str) -> Result<String, String> {
    match CONTROL_TOOLS.get() {
        Some(c) => Ok(c.chief_of_staff_prompt(prompt)),
        None => Err(format!("{NO_CONTROL_TOOLS}: it can't start a chief of staff")),
    }
}

/// The recipe a board's lead runs (Arugula #1440): `start_agent {recipe}`
/// on this machine, and a ladder start's lead by default.
pub const BOARD_LEAD: &str = "board-lead";

/// The recipe an agent started on an issue runs (Arugula #1440): it leads
/// the issue's thread. Every start on an issue puts it first; naming it is
/// the same as not.
pub const ISSUE_AGENT: &str = "issue-agent";

/// Whether `name` is one of Arugula's own recipes, which a block reads as
/// its first prompt, not one it wears (`.claude/agents/NAME.md`).
pub fn own_recipe(name: &str) -> bool {
    [CHIEF_OF_STAFF, BOARD_LEAD, ISSUE_AGENT].contains(&name.trim())
}

/// [`ControlTools::board_lead_prompt`], for the lead of `board` when it's
/// known.
pub fn board_lead_prompt(board: Option<&str>, prompt: &str) -> Result<String, String> {
    let prompt = match board {
        Some(b) => format!("Your board is {b}.\n\n{}", prompt.trim()),
        None => prompt.trim().to_owned(),
    };
    match CONTROL_TOOLS.get() {
        Some(c) => Ok(c.board_lead_prompt(&prompt)),
        None => Err(format!("{NO_CONTROL_TOOLS}: it can't start a board's lead")),
    }
}

/// [`ControlTools::issue_agent_prompt`], for an agent on `issue` (on
/// `board`, when it's known). Without control's tools, it has no thread to
/// lead: `prompt` with its issue.
pub fn issue_agent_prompt(issue: &str, board: Option<&str>, prompt: &str) -> String {
    let about = match board {
        Some(b) => format!("Your issue is {issue}, on the board {b}."),
        None => format!("Your issue is {issue}."),
    };
    let prompt = format!("{about}\n\n{}", prompt.trim());
    match CONTROL_TOOLS.get() {
        Some(c) => c.issue_agent_prompt(&prompt),
        None => prompt,
    }
}

/// [`ControlTools::lead_staff`].
pub async fn lead_staff(app: &App, project: &str, session: &str) -> Out {
    match CONTROL_TOOLS.get() {
        Some(c) => c.lead_staff(app, project, session).await,
        None => Err(format!("{NO_CONTROL_TOOLS}: it can't lead a staff room")),
    }
}

/// [`ControlTools::staff_room`].
pub async fn staff_room(app: &App, project: &str) -> Out {
    match CONTROL_TOOLS.get() {
        Some(c) => c.staff_room(app, project).await,
        None => Err(format!("{NO_CONTROL_TOOLS}: it can't read a staff room")),
    }
}

/// [`ControlTools::stand_down`].
pub async fn stand_down(app: &App, room: &str, session: &str, notice: &str, by: &str) -> Out {
    match CONTROL_TOOLS.get() {
        Some(c) => c.stand_down(app, room, session, notice, by).await,
        None => Err(format!("{NO_CONTROL_TOOLS}: it can't move a chief of staff")),
    }
}

/// A call to a registered tool or kind: the call, the kind (a grouped
/// tool's, which `route` took out of the arguments), and the arguments.
pub type Run = for<'a> fn(&'a Call<'a>, Option<&'static str>, Value) -> BoxFuture<'a, Out>;

/// A tool of a feature crate's.
pub struct Tool {
    /// The tool it's listed after, or `None` for first.
    pub after: Option<&'static str>,
    pub def: fn() -> Def,
    pub run: Run,
}

/// A kind of a grouped tool, from a feature crate.
pub struct ToolKind {
    /// The grouped tool (`show`, `doc`).
    pub tool: &'static str,
    /// The kind it's listed after, or `None` for first.
    pub after: Option<&'static str>,
    pub kind: &'static Kind,
    pub run: Run,
}

static TOOLS: Mutex<Vec<Tool>> = Mutex::new(Vec::new());
static KINDS: Mutex<Vec<ToolKind>> = Mutex::new(Vec::new());

/// Adds a tool. Its name must be new.
pub fn tool(t: Tool) {
    TOOLS.lock().unwrap().push(t);
}

/// Adds a kind to a grouped tool. Its name must be new in that tool.
pub fn kind(k: ToolKind) {
    KINDS.lock().unwrap().push(k);
}

/// Puts the registered tools and kinds into the base's table.
pub fn add(defs: &mut Vec<Def>) {
    add_these(defs, &TOOLS.lock().unwrap(), &KINDS.lock().unwrap());
}

fn add_these(defs: &mut Vec<Def>, tools: &[Tool], kinds: &[ToolKind]) {
    // In the order registered: one may follow another.
    for t in tools {
        let at = match t.after {
            None => 0,
            Some(a) => defs.iter().position(|d| d.name == a).map_or(defs.len(), |i| i + 1),
        };
        defs.insert(at, (t.def)());
    }
    for k in kinds {
        let Some(Def { args: Args::Kinds { kinds, .. }, .. }) = defs.iter_mut().find(|d| d.name == k.tool) else {
            continue;
        };
        let at = match k.after {
            None => 0,
            Some(a) => kinds.iter().position(|x| x.name == a).map_or(kinds.len(), |i| i + 1),
        };
        kinds.insert(at, k.kind);
    }
}

/// The registered call for `tool` (and its kind), if a crate added it: a
/// kind added to a tool, or a tool of the crate's own (grouped or not).
pub fn run_of(tool: &str, kind: Option<&str>) -> Option<Run> {
    if let Some(k) = kind
        && let Some(x) = KINDS.lock().unwrap().iter().find(|x| x.tool == tool && x.kind.name == k)
    {
        return Some(x.run);
    }
    TOOLS.lock().unwrap().iter().find(|t| (t.def)().name == tool).map(|t| t.run)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &'static str, kinds: Option<Vec<&'static Kind>>) -> Def {
        Def {
            name,
            title: "",
            description: "",
            args: match kinds {
                None => Args::One(rmcp::handler::server::common::schema_for_type::<serde_json::Value>),
                Some(kinds) => Args::Kinds { kinds, default: None },
            },
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: false,
            flag: None,
            output: None,
            human: super::super::tools::Human::In("test"),
        }
    }

    fn run<'a>(_: &'a Call<'a>, _: Option<&'static str>, _: Value) -> BoxFuture<'a, Out> {
        Box::pin(async { Ok(Value::Null) })
    }

    static A: Kind = Kind {
        name: "a",
        description: "",
        schema: rmcp::handler::server::common::schema_for_type::<serde_json::Value>,
        flag: None,
    };
    static B: Kind = Kind { name: "b", ..A };
    static C: Kind = Kind { name: "c", ..A };

    #[test]
    fn registered_tools_and_kinds_go_where_they_say() {
        let mut defs = vec![def("one", None), def("group", Some(vec![&A, &C])), def("three", None)];
        let tools = [
            Tool { after: Some("one"), def: || def("two", None), run },
            Tool { after: None, def: || def("zero", None), run },
            Tool { after: Some("nowhere"), def: || def("last", None), run },
        ];
        let kinds = [ToolKind { tool: "group", after: Some("a"), kind: &B, run }];
        add_these(&mut defs, &tools, &kinds);
        let names: Vec<&str> = defs.iter().map(|d| d.name).collect();
        assert_eq!(names, ["zero", "one", "two", "group", "three", "last"]);
        let Args::Kinds { kinds, .. } = &defs[3].args else { panic!("a group") };
        assert_eq!(kinds.iter().map(|k| k.name).collect::<Vec<_>>(), ["a", "b", "c"]);
    }
}
