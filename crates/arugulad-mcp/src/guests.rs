//! What an agent a guest stands behind may call (M14).
//!
//! A guest who edits a session may start agents in it, each on a VM of
//! their own (`guest_block`), and an agent such an agent starts stands
//! behind the same guest (`Api::StartedBy`). Its token is
//! `Scope::Block`: its own tab, and driving what it started. A tool that
//! reaches past that tab acts as this machine's owner: in Arugula control
//! (Cloud workspaces, threads, boards, runs, deploys, agent sessions,
//! issues, linked repos), on the owner's devices, through other machines'
//! agents, with the owner's forge logins, in the owner's Doc library or
//! their Claude Code conversations here. Only the owner's own agents may
//! call one. `dispatch` asks [`refusal`] before anything else runs, a Cloud
//! workspace's forwarding included, so no tool needs a check of its own.
//!
//! Every tool and kind is in [`TAB`] or [`OWNERS`]. One in neither is the
//! owner's all the same (refused), and the whole list's check
//! (`every_tool_and_kind_is_placed`) fails until it's put in one.

use arugula_proto::flags::On;
use serde_json::Value;

use super::route;
use crate::hosted;

/// A tool, and one of its kinds (`None`: the tool, or every kind of it).
pub type Entry = (&'static str, Option<&'static str>);

/// What an agent a guest stands behind may call: its own tab's panes and
/// blocks, which its scope confines (`readable`, `drivable`, `confine`).
pub const TAB: &[Entry] = &[
    ("run", None),
    ("send_input", None),
    ("attach", None),
    ("read_output", None),
    // With a pane; a run, agent sessions or a Cloud workspace's tab are
    // the owner's ([`OWNERS_ARGS`]).
    ("wait", None),
    ("close", None),
    ("history", None),
    ("capabilities", None),
    ("start_agent", None),
    ("prompt_agent", None),
    ("agent_respond", None),
    ("read_forge", None),
    ("read_file", None),
    ("list", Some("panes")),
    ("show", Some("port")),
    ("show", Some("changes")),
    ("show", Some("file")),
    ("show", Some("conversation")),
    ("show", Some("app")),
    ("show", Some("workspace")),
    ("doc", Some("new")),
    ("doc", Some("fill")),
    ("doc", Some("asset")),
    ("doc", Some("share")),
];

/// What only the owner's own agents may call: it acts as this machine's
/// owner past the agent's tab.
pub const OWNERS: &[Entry] = &[
    ("delegate", None),
    ("list", Some("conversations")),
    ("list", Some("devices")),
    ("list", Some("agents")),
    ("list", Some("cloud_workspaces")),
    ("list", Some("cloud_tabs")),
    ("list", Some("linked_repos")),
    ("list", Some("threads")),
    ("list", Some("sessions")),
    ("show", Some("chat")),
    ("show", Some("board")),
    ("doc", Some("list")),
    ("doc", Some("open")),
    // With the owner's forge logins, private repos too.
    ("show", Some("pr")),
    ("show", Some("issue")),
    ("draft", None),
    ("forge_read", None),
    ("forge_draft", None),
    ("invite_person", None),
    ("read_invite", None),
    ("device_call", None),
    ("read_cloud", None),
    ("cloud", None),
    ("link_repo", None),
    ("read_thread", None),
    ("search_threads", None),
    ("post_thread", None),
    ("issue_write", None),
    ("board_read", None),
    ("board_write", None),
    ("start_run", None),
    ("read_run", None),
    ("plan_deploy", None),
    ("read_deploy", None),
    ("session_start", None),
    ("session_reply", None),
    ("read_session", None),
];

/// Arguments that take a tab's tool past the tab, by tool (`*`: any):
/// a Cloud workspace's machine, and `wait` on a run or agent sessions in
/// control.
pub const OWNERS_ARGS: &[(&str, &str)] = &[("*", hosted::ARG), ("wait", "run"), ("wait", "session")];

fn placed(list: &[Entry], tool: &str, kind: Option<&str>) -> bool {
    list.iter().any(|(t, k)| *t == tool && (k.is_none() || *k == kind))
}

/// Whether `tool` (with `kind`, and `args` without it) is the owner's
/// agents' only.
pub fn owners_only(tool: &str, kind: Option<&str>, args: &Value) -> bool {
    let given = |a: &str| args.get(a).is_some_and(|v| !v.is_null());
    if OWNERS_ARGS.iter().any(|(t, a)| (*t == "*" || *t == tool) && given(a)) {
        return true;
    }
    !placed(TAB, tool, kind)
}

/// Why an agent a guest stands behind may not make this call (`name`, with
/// `args` as it sent them), or `None` when it may. A call `dispatch` would
/// refuse anyway (no such tool or kind) is left to it.
pub fn refusal(name: &str, args: &Value, on: On) -> Option<String> {
    let said = |what: String| Some(format!("{what} acts as this machine's owner: only their own agents may"));
    // Before routing: a Cloud workspace's call is forwarded before its
    // kind is looked at.
    if args.get(hosted::ARG).is_some_and(|v| !v.is_null()) {
        return said(format!("{name} on a Cloud workspace"));
    }
    let (def, kind, args) = route(name, args.clone(), on).ok()?;
    if !owners_only(def.name, kind, &args) {
        return None;
    }
    let what = match kind {
        Some(k) => format!("{} kind {k}", def.name),
        None => match OWNERS_ARGS.iter().find(|(t, a)| *t == def.name && args.get(*a).is_some_and(|v| !v.is_null())) {
            Some((_, a)) => format!("{} with {a}", def.name),
            None => def.name.to_owned(),
        },
    };
    said(what)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn on() -> On {
        On::all()
    }

    #[test]
    fn no_tool_is_both_a_tabs_and_the_owners() {
        for (t, k) in TAB {
            assert!(!placed(OWNERS, t, *k), "{t} {k:?} is in TAB and OWNERS");
        }
        for (t, k) in OWNERS {
            assert!(!placed(TAB, t, *k), "{t} {k:?} is in OWNERS and TAB");
        }
    }

    /// The base's tools (the feature crates' are checked with every crate
    /// registered, in the whole list's checks): a guest's agent is refused
    /// each of the owner's, and keeps its tab's.
    #[test]
    fn a_guests_agent_is_refused_the_owners_tools() {
        for (tool, kind) in
            [("device_call", None), ("list", Some("devices")), ("show", Some("chat")), ("show", Some("board"))]
        {
            let args = kind.map_or_else(|| json!({}), |k| json!({ "kind": k }));
            let why = refusal(tool, &args, on()).unwrap_or_else(|| panic!("{tool} {kind:?} is refused"));
            assert!(why.contains("only their own agents may"), "{why}");
        }
        for (tool, args) in [
            ("run", json!({ "command": "ls" })),
            ("read_output", json!({ "pane": 3 })),
            ("list", json!({ "kind": "panes" })),
            ("list", json!({})),
            ("show", json!({ "kind": "port", "port": 3000 })),
            ("wait", json!({ "pane": 3 })),
            ("history", json!({ "kind": "output", "pattern": "x" })),
            ("capabilities", json!({})),
        ] {
            assert_eq!(refusal(tool, &args, on()), None, "{tool} {args}");
        }
    }

    /// A machine tool's `cloud_workspace` runs it on that workspace's
    /// machine as the owner: refused before it's forwarded, for every
    /// machine tool, and `wait` on a Cloud tab, a run or sessions too.
    #[test]
    fn a_guests_agent_never_reaches_a_cloud_workspace_or_controls_waits() {
        for tool in hosted::TOOLS {
            let args = json!({ "cloud_workspace": "hw_abc", "pane": 1 });
            let why = refusal(tool, &args, on()).unwrap_or_else(|| panic!("{tool} with cloud_workspace is refused"));
            assert!(why.contains("on a Cloud workspace"), "{why}");
        }
        assert!(refusal("wait", &json!({ "cloud_workspace": "hw_abc", "tab": "4" }), on()).is_some());
        assert!(refusal("wait", &json!({ "run": "run_1" }), on()).unwrap().contains("wait with run"));
        assert!(refusal("wait", &json!({ "session": "ses_1" }), on()).unwrap().contains("wait with session"));
        // Null is as good as left out.
        assert_eq!(refusal("run", &json!({ "command": "ls", "cloud_workspace": null }), on()), None);
    }

    /// What dispatch refuses anyway is left to it to say.
    #[test]
    fn what_dispatch_refuses_is_its_to_say() {
        assert_eq!(refusal("no_such_tool", &json!({}), on()), None);
        assert_eq!(refusal("list", &json!({ "kind": "nope" }), on()), None);
    }
}
