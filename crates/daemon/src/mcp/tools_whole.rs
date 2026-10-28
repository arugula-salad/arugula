//! Checks of the whole tool list (#232): what an agent sees is the base's
//! table with the feature crates' tools in it (`registry.rs`), so these run
//! where every feature is registered, the binary's tests
//! (`crates/arugulad/src/whole_tests.rs`), with the cargo feature
//! `whole-tests`. Each panics with what's wrong.

use super::*;

/// Every Labs flag on, or none.
fn fl(labs: bool) -> arugula_proto::flags::On {
    if labs { arugula_proto::flags::On::all() } else { arugula_proto::flags::On::none() }
}

/// `tools/list` for every scope, with and without labs, against the
/// checked-in file: names, descriptions, annotations and every input
/// schema. `ARUGULA_BLESS=1` rewrites the file after a change meant to
/// alter what agents see. The file is a Labs build's (which has the
/// forge too): a build without Labs has no golden of its own, so it
/// checks the listing in
/// `unlisted_tools_are_not_listed_without_labs_but_are_there`.
pub fn the_tool_list_and_each_schema_are_unchanged() {
    if !crate::lab_flags::BUILT {
        return;
    }
    let mut all = serde_json::Map::new();
    for (scope_name, scope) in [("full", Scope::Full), ("read", Scope::Read), ("block", Scope::Block(1))] {
        for labs in [true, false] {
            let tools = serde_json::to_value(list(scope, fl(labs))).unwrap();
            all.insert(format!("{scope_name}, labs {labs}"), tools);
        }
    }
    let got = serde_json::to_string_pretty(&Value::Object(all)).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp-tool-list.json");
    if std::env::var_os("ARUGULA_BLESS").is_some() {
        std::fs::write(&path, &got).unwrap();
    }
    let want = std::fs::read_to_string(&path).expect("tests/fixtures/mcp-tool-list.json (ARUGULA_BLESS=1 writes it)");
    assert!(got == want, "the tool list changed; ARUGULA_BLESS=1 rewrites {}", path.display());
}

pub fn annotations_are_honest() {
    let all = list(Scope::Full, fl(false));
    assert_eq!(all.len(), 35 - no_forge());
    let ro: Vec<&str> = all
        .iter()
        .filter(|t| t.annotations.as_ref().and_then(|a| a.read_only_hint) == Some(true))
        .map(|t| t.name.as_ref())
        .collect();
    let mut want = vec![
        "read_output",
        "wait",
        "list",
        "capabilities",
        "history",
        "read_forge",
        "forge_read",
        "read_invite",
        "read_file",
        "read_cloud",
        "read_thread",
        "search_threads",
        "read_run",
        "read_deploy",
    ];
    want.retain(|n| crate::forges::built() || !FORGE_TOOLS.contains(n));
    assert_eq!(ro, want);
    assert_eq!(list(Scope::Read, fl(false)).len(), ro.len(), "a read token sees the read-only tools only");
    let hint = |n: &str| all.iter().find(|t| t.name == n).unwrap().annotations.clone().unwrap();
    assert_eq!(hint("close").destructive_hint, Some(true));
    // A group is as careful as its most careful kind: draft has merge.
    if crate::forges::built() {
        assert_eq!(hint("draft").destructive_hint, Some(true));
    }
    assert_eq!((hint("show").destructive_hint, hint("show").open_world_hint), (Some(false), Some(true)));
}

/// Studio apps and chant workspaces are for what a stranger doesn't
/// have: without `labs` their kinds (and the `delegate` and `doc` tools)
/// aren't listed, with it all are, and a caller who names one reaches it
/// either way.
pub fn unlisted_tools_are_not_listed_without_labs_but_are_there() {
    for scope in [Scope::Full, Scope::Read] {
        let with: Vec<String> = list(scope, fl(true)).iter().map(|t| t.name.to_string()).collect();
        for d in all_defs().iter().filter(|d| scope == Scope::Full || d.read_only) {
            if !crate::forges::built() && FORGE_TOOLS.contains(&d.name) {
                continue;
            }
            assert!(with.contains(&d.name.to_string()), "{} isn't listed for {scope:?} with labs", d.name);
        }
    }
    // A hidden kind is filtered out of its group: not in kind's enum,
    // nor its line in the description, nor the arguments only it takes.
    for (tool, kind) in flagged_kinds() {
        assert!(!kinds_of(false, tool).contains(&kind), "{tool} kind {kind} is listed without labs");
        assert!(kinds_of(true, tool).contains(&kind), "{tool} kind {kind} isn't listed with labs");
        // Still there by its kind.
        route(tool, json!({ "kind": kind }), fl(false)).unwrap_or_else(|e| panic!("{tool} {kind}: {e}"));
        // A tool all of whose kinds are flagged (`doc`) isn't listed at all.
        let Some(t) = list(Scope::Full, fl(false)).into_iter().find(|t| t.name == tool) else {
            assert!(defs(fl(false)).iter().all(|d| d.name != tool), "{tool}");
            continue;
        };
        let kinds = t.input_schema["properties"]["kind"]["enum"].clone();
        assert!(!kinds.as_array().unwrap().contains(&json!(kind)), "{tool}: {kinds}");
        assert!(!t.description.as_deref().unwrap().contains(&format!("\n- {kind}")), "{tool}: {kind}");
    }
    let show = list(Scope::Full, fl(false)).into_iter().find(|t| t.name == "show").unwrap();
    for arg in ["app", "env"] {
        assert!(show.input_schema["properties"].get(arg).is_none(), "show takes {arg} without labs");
    }
    // Daemon-side Managoat was removed: its kinds, by either name, and
    // its old tools say where cloud agents went; none is listed.
    let list_tool = list(Scope::Full, fl(false)).into_iter().find(|t| t.name == "list").unwrap();
    for (tool, kind) in [
        ("show", "managoat"),
        ("show", "fountain"),
        ("list", "managoat_agents"),
        ("list", "managoat_agent"),
        ("list", "managoat_conversations"),
        ("list", "fountain_agents"),
    ] {
        assert!(!kinds_of(true, tool).contains(&kind), "{tool} kind {kind}");
        let e = route(tool, json!({ "kind": kind }), fl(true)).err().unwrap();
        assert_eq!(e, crate::agent::defs::CLOUD_MIGRATION, "{tool} {kind}");
    }
    for old in ["open_fountain", "list_agents", "read_agent"] {
        assert!(route(old, json!({}), fl(true)).is_err(), "{old}");
    }
    for args in [
        json!({ "agent": "managoat", "prompt": "hi" }),
        json!({ "agent": "fountain", "fountain_agent": "arena", "prompt": "hi" }),
        json!({ "managoat_agent": "arena", "prompt": "hi" }),
        json!({ "recipe": "r", "on": "managoat", "prompt": "hi" }),
    ] {
        assert!(managoat_start(&args), "{args}");
    }
    assert!(!managoat_start(&json!({ "recipe": "r", "on": "here", "prompt": "hi" })));
    for (t, arg) in
        [(&show, "query"), (&show, "source"), (&show, "profile"), (&list_tool, "name"), (&list_tool, "source")]
    {
        assert!(t.input_schema["properties"].get(arg).is_none(), "{} {arg}", t.name);
    }
    let start = list(Scope::Full, fl(true)).into_iter().find(|t| t.name == "start_agent").unwrap();
    for arg in ["managoat_agent", "on", "vault", "environment"] {
        assert!(start.input_schema["properties"].get(arg).is_none(), "start_agent {arg}");
    }
    // An error names only the kinds that are listed.
    let e = route("show", json!({ "kind": "terminal" }), fl(false)).err().unwrap();
    // (The Managoat-backed `cloud_workspace` block went in M190.)
    let named = |k: &str| e.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').any(|w| w == k);
    assert!(named("port") && !named("cloud_workspace") && !e.contains("managoat") && !named("workspace"), "{e}");
    let every: Vec<&str> = all_defs().iter().map(|d| d.name).collect();
    for name in THREAD_TOOLS {
        assert!(defs(fl(false)).iter().any(|d| d.name == name), "{name} is everyone's");
    }
    assert_eq!(every.len(), 38);
    assert_eq!(defs(fl(false)).len(), 35 - no_forge());
    assert_eq!(defs(fl(true)).len(), 38 - no_forge());
    assert_eq!(every.len(), defs(fl(false)).len() + 3 + no_forge(), "and delegate and the board tools");
}

/// docs/mcp-permissions.json allows the read-only tools and asks for the
/// rest, every one of them: a new tool fails this until it's listed there
/// (#114). docs.arugula.io shows the same snippet, and the site repo's
/// check-code holds it to this file.
pub fn permissions_snippet_is_every_tool() {
    const SNIPPET: &str = include_str!("../../../../docs/mcp-permissions.json");
    let list = |key: &str| -> Vec<&str> {
        let at = SNIPPET
            .find(&format!("\"{key}\": ["))
            .unwrap_or_else(|| panic!("no {key} list in docs/mcp-permissions.json"));
        let body = &SNIPPET[at..];
        let body = &body[..body.find(']').unwrap()];
        body.split('"')
            .filter_map(|s| s.strip_prefix("mcp__arugula__"))
            .filter(|n| crate::forges::built() || !FORGE_TOOLS.contains(n))
            .collect()
    };
    // The snippet lists what a stranger sees.
    let defs = defs(fl(false));
    let want = |ro: bool| -> Vec<&str> { defs.iter().filter(|d| d.read_only == ro).map(|d| d.name).collect() };
    assert_eq!(
        list("allow"),
        want(true),
        "docs/mcp-permissions.json's allow list: the read-only tools, in defs() order"
    );
    assert_eq!(list("ask"), want(false), "docs/mcp-permissions.json's ask list: every other tool, in defs() order");
}

/// The card follows the tool list: a tool its flag leaves out, and a
/// line whose need this machine lacks, aren't on it.
pub fn the_card_lists_what_this_machine_offers() {
    let tools = |c: &[arugula_proto::api::AgentCan]| c.iter().map(|c| c.tool.clone()).collect::<Vec<_>>();
    let all = tools(&can_do(fl(true), |_| true));
    let bare = tools(&can_do(fl(false), |n| n == Needs::Nothing));
    for t in ["run", "show", "doc", "start_agent", "history"] {
        assert!(all.contains(&t.to_owned()) && bare.contains(&t.to_owned()), "{t}");
    }
    // Parts of another's line never show on their own.
    assert!(!all.contains(&"wait".to_owned()) && !all.contains(&"close".to_owned()));
    // A flag: boards, and delegating (the agent catalog's).
    assert!(all.contains(&super::super::boards::READ_TOOL.to_owned()));
    assert!(all.contains(&"delegate".to_owned()) && !bare.contains(&"delegate".to_owned()));
    // What control and its grants give.
    for t in
        ["cloud", super::super::runs::START_TOOL, super::super::threads::READ_TOOL, super::super::issues::WRITE_TOOL]
    {
        assert!(all.contains(&t.to_owned()) && !bare.contains(&t.to_owned()), "{t}");
    }
    if crate::forges::built() {
        assert!(all.contains(&"draft".to_owned()) && !bare.contains(&"draft".to_owned()));
        let signed = tools(&can_do(fl(false), |n| matches!(n, Needs::Nothing | Needs::Forge)));
        assert!(signed.contains(&"draft".to_owned()) && !signed.contains(&"cloud".to_owned()));
    }
}

/// #963: `doc` is a tool, with `new` and `fill`, and `capabilities`
/// offers it: everywhere, with no flag, since Docs left Labs (native
/// #204).
pub fn doc_is_listed_with_no_flag() {
    use arugula_proto::flags::On;
    let Some(Def { args: Args::Kinds { kinds, .. }, .. }) = defs(On::none()).into_iter().find(|d| d.name == "doc")
    else {
        panic!("doc has kinds")
    };
    assert_eq!(kinds.iter().map(|k| k.name).collect::<Vec<_>>(), ["new", "list", "open", "fill", "asset", "share"]);
    let offers = block_offers(On::none());
    let offer = offers.iter().find(|o| o.kind == "doc").expect("capabilities offers a doc");
    assert_eq!(offer.open, "doc {kind: \"new\"}");
    // The schema carries the outline's shape.
    let tool = list(Scope::Full, On::none()).into_iter().find(|t| t.name == "doc").unwrap();
    let schema = Value::Object((*tool.input_schema).clone()).to_string();
    assert!(schema.contains("heading") && schema.contains("intent") && schema.contains("markdown"), "{schema}");
    // ...and the picture's: a file or its bytes.
    assert!(schema.contains("\"path\"") && schema.contains("\"data\""), "{schema}");
}

/// The thread tools (M198): agents are in conversations again
/// (aru-091), through control's platform, with no thread kept here.
const THREAD_TOOLS: [&str; 3] = ["read_thread", "search_threads", "post_thread"];

/// How many of the tools a build without the forge doesn't list.
fn no_forge() -> usize {
    if crate::forges::built() { 0 } else { FORGE_TOOLS.len() }
}

/// The kinds that aren't listed without a flag: the table's, and the
/// operations' own (`Op::FLAG`).
fn flagged_kinds() -> Vec<(&'static str, &'static str)> {
    let mut all: Vec<_> = UNLISTED.iter().map(|(t, k, _)| (*t, *k)).collect();
    for d in all_defs() {
        if let Args::Kinds { kinds, .. } = &d.args {
            all.extend(kinds.iter().filter(|k| k.flag.is_some()).map(|k| (d.name, k.name)));
        }
    }
    all
}

/// The kinds of a group a tool has, listed with `labs` or without.
fn kinds_of(labs: bool, tool: &str) -> Vec<&'static str> {
    defs(fl(labs))
        .into_iter()
        .find(|d| d.name == tool)
        .map(|d| match d.args {
            Args::Kinds { kinds, .. } => kinds.iter().map(|k| k.name).collect(),
            Args::One(_) => Vec::new(),
        })
        .unwrap_or_default()
}
