//! MCP tools and kinds a feature crate adds (#232). The tool list
//! (`tools.rs`) is the base's table with these put in where each says, so
//! the list an agent sees keeps its order (and `mcp-tool-list.json`, which
//! pins it, its lines) whichever crate a tool lives in. A call to one runs
//! its [`Run`]. Registered before `main`, by the binary.

use std::sync::Mutex;

use futures_util::future::BoxFuture;
use serde_json::Value;

use super::tools::{Args, Call, Def, Kind, Out};

/// A call to a registered tool or kind: the call, and its arguments (a
/// kind's without `kind`).
pub type Run = for<'a> fn(&'a Call<'a>, Value) -> BoxFuture<'a, Out>;

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

/// The registered call for `tool` (and its kind), if a crate added it.
pub(crate) fn run_of(tool: &str, kind: Option<&str>) -> Option<Run> {
    match kind {
        Some(k) => KINDS.lock().unwrap().iter().find(|x| x.tool == tool && x.kind.name == k).map(|x| x.run),
        None => TOOLS.lock().unwrap().iter().find(|t| (t.def)().name == tool).map(|t| t.run),
    }
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

    fn run<'a>(_: &'a Call<'a>, _: Value) -> BoxFuture<'a, Out> {
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
