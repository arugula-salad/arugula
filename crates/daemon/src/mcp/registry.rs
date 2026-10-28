//! MCP tools and kinds a feature crate adds (#232). The tool list
//! (`tools.rs`) is the base's table with these put in where each says, so
//! the list an agent sees keeps its order (and `mcp-tool-list.json`, which
//! pins it, its lines) whichever crate a tool lives in. A call to one runs
//! its [`Run`]. Registered before `main`, by the binary.
//!
//! Control's tools (`arugulad-control-tools`) also say what the base asks
//! of them: whether they reach control from here, and `wait` on a run
//! ([`ControlTools`]).

use std::{
    sync::{Mutex, OnceLock},
    time::Duration,
};

use futures_util::future::BoxFuture;
use serde_json::Value;

use super::{
    results::{CloudReach, RunsReach, ThreadsReach},
    tools::{Args, Call, Def, Kind, Out},
};
use crate::server::App;

/// What the base asks of control's tools: `capabilities` and the card
/// *What your agent can do here* say whether each reaches control from
/// here, and `wait` with `run` follows a run.
pub trait ControlTools: Send + Sync + 'static {
    /// Whether agents here drive the owner's Cloud workspaces (#1094).
    fn cloud_reach<'a>(&'a self, app: &'a App) -> BoxFuture<'a, CloudReach>;
    /// Whether thread tools are live here (M198).
    fn threads_reach<'a>(&'a self, app: &'a App) -> BoxFuture<'a, ThreadsReach>;
    /// Whether run tools are live here (M216).
    fn runs_reach<'a>(&'a self, app: &'a App) -> BoxFuture<'a, RunsReach>;
    /// `wait` with `run`: until it ends, or `limit`.
    fn wait_run<'a>(&'a self, call: &'a Call<'a>, run: &'a str, limit: Duration) -> BoxFuture<'a, Out>;
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
pub(crate) async fn reach(app: &App) -> (CloudReach, ThreadsReach, RunsReach) {
    match CONTROL_TOOLS.get() {
        Some(c) => tokio::join!(c.cloud_reach(app), c.threads_reach(app), c.runs_reach(app)),
        None => (
            CloudReach { reachable: false, note: NO_CONTROL_TOOLS.into() },
            ThreadsReach { live: false, scopes: vec![], note: NO_CONTROL_TOOLS.into() },
            RunsReach { live: false, projects: vec![], note: NO_CONTROL_TOOLS.into() },
        ),
    }
}

/// `wait` with `run` ([`ControlTools::wait_run`]).
pub(crate) async fn wait_run(call: &Call<'_>, run: &str, limit: Duration) -> Out {
    match CONTROL_TOOLS.get() {
        Some(c) => c.wait_run(call, run, limit).await,
        None => Err(format!("{NO_CONTROL_TOOLS}: it can't wait on a run")),
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
pub(crate) fn add(defs: &mut Vec<Def>) {
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
pub(crate) fn run_of(tool: &str, kind: Option<&str>) -> Option<Run> {
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
