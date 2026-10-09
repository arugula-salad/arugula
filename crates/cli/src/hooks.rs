//! `arugula hooks`: put Claude Code's hooks for Arugula into its
//! settings.json (`install`), or say which are there (`status`). The merge
//! only ever adds: every other key and every other hook stays as it is, and
//! a hook already there (same event, same matcher, same command) isn't
//! added twice, so a second `install` changes nothing. The one exception is
//! our own hooks under the old name (`illogical hook`, #505): `install`
//! renames those in place rather than adding a second copy beside them.
//! `arugulad install` does the same, with Claude Code's MCP server, before
//! it takes the old names off this machine (`rename-old`, #534).
//!
//! `install` also adds `permissions.allow` rules (#655), in two tiers, so an
//! agent in auto mode can use the CLI it was just wired to: the read-only
//! commands and MCP tools by default, the acting ones only with
//! `--allow-acting`. Those only ever add too, and `status` says which tier
//! is there.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use clap::Subcommand;
use serde_json::{Map, Value, json};

/// The hooks Arugula wants in Claude Code's settings.json. This is the
/// JSON in docs.arugula.io/cli § "Claude Code in a pane": arugula-salad/site
/// checks its copy against this one daily, so change that page with it.
pub const HOOKS_SNIPPET: &str = r#"{
  "hooks": {
    "Notification": [{ "hooks": [{ "type": "command", "command": "arugula attention needs-input" }] }],
    "Stop": [
      { "hooks": [{ "type": "command", "command": "arugula attention done" }] },
      { "hooks": [{ "type": "command", "command": "arugula inbox", "asyncRewake": true, "timeout": 86400 }] }
    ],
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "arugula inbox", "asyncRewake": true, "timeout": 86400 }] }],
    "PreToolUse": [
      { "matcher": "AskUserQuestion", "hooks": [{ "type": "command", "command": "arugula ask", "timeout": 604800 }] },
      { "hooks": [{ "type": "command", "command": "arugula hook" }] }
    ],
    "PermissionRequest": [{ "hooks": [{ "type": "command", "command": "arugula hook", "timeout": 604800 }] }],
    "PostToolUse": [{ "hooks": [{ "type": "command", "command": "arugula hook" }] }],
    "PostToolUseFailure": [{ "hooks": [{ "type": "command", "command": "arugula hook" }] }],
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "arugula hook" }] }]
  }
}"#;

/// The `permissions.allow` rules `install` adds by default (#655): what only
/// reads. Claude Code's `Bash(prefix:*)` matches a command that starts with
/// the prefix, so each is one subcommand and never `Bash(arugula:*)`. `hosts`
/// and `shares` are exact, with no `:*`: `hosts add|rm|token|revoke` and
/// `shares revoke` change things. `attention` sets attention, so it is in
/// neither tier.
const READ_RULES: &[&str] = &[
    "Bash(arugula ls:*)",
    "Bash(arugula describe:*)",
    "Bash(arugula tail:*)",
    "Bash(arugula wait:*)",
    "Bash(arugula history:*)",
    "Bash(arugula log:*)",
    "Bash(arugula search:*)",
    "Bash(arugula capture:*)",
    "Bash(arugula process:*)",
    "Bash(arugula status:*)",
    "Bash(arugula fs:*)",
    "Bash(arugula hooks status:*)",
    "Bash(arugula hosts)",
    "Bash(arugula shares)",
    // The MCP tools with `read_only_hint` in crates/daemon/src/mcp/tools.rs
    // (the CLI can't depend on the daemon; docs/mcp-permissions.json lists
    // the same, and a test here checks it).
    "mcp__arugula__read_output",
    "mcp__arugula__wait",
    "mcp__arugula__list",
    "mcp__arugula__history",
    "mcp__arugula__read_forge",
    "mcp__arugula__read_invite",
    "mcp__arugula__read_file",
    "mcp__arugula__read_thread",
];

/// What `install --allow-acting` adds beside [`READ_RULES`] (#655): what
/// types into panes, starts agents, closes things or shares. Allowing these
/// lets an agent do all of that without asking.
const ACTING_RULES: &[&str] = &[
    "Bash(arugula agent:*)",
    "Bash(arugula run:*)",
    "Bash(arugula send:*)",
    "Bash(arugula keys:*)",
    "Bash(arugula close:*)",
    "Bash(arugula call:*)",
    "Bash(arugula open:*)",
    "Bash(arugula edit:*)",
    "Bash(arugula view:*)",
    "Bash(arugula diff:*)",
    "Bash(arugula pr:*)",
    "Bash(arugula issue:*)",
    "Bash(arugula cd:*)",
    "Bash(arugula rerun:*)",
    "Bash(arugula upload:*)",
    "Bash(arugula mouse:*)",
    "Bash(arugula share:*)",
    "Bash(arugula invite:*)",
    // Every other MCP tool; see the comment above `READ_RULES`.
    "mcp__arugula__run",
    "mcp__arugula__send_input",
    "mcp__arugula__attach",
    "mcp__arugula__close",
    "mcp__arugula__show",
    "mcp__arugula__delegate",
    "mcp__arugula__start_agent",
    "mcp__arugula__prompt_agent",
    "mcp__arugula__agent_respond",
    "mcp__arugula__draft",
    "mcp__arugula__invite_person",
    "mcp__arugula__device_call",
    "mcp__arugula__post_thread",
];

#[derive(Subcommand)]
pub enum HooksCmd {
    /// Merge the hooks into Claude Code's settings.json.
    ///
    /// Everything already there is kept. Running it again changes nothing.
    ///
    /// Also adds `permissions.allow` rules for the commands and MCP tools
    /// that only read (`arugula ls`, `list`, `read_output`, ...). The ones
    /// that act (`arugula agent`, `run`, `send`, `send_input`, ...)
    /// only with `--allow-acting`. Run each `arugula` command on its own,
    /// not in a pipeline or `&&` chain, or the rules don't match it.
    Install {
        /// Write DIR/.claude/settings.json instead of ~/.claude/settings.json.
        #[arg(long, value_name = "DIR")]
        project: Option<PathBuf>,
        /// Print the resulting JSON; write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Also allow the commands and tools that act: an agent can then
        /// start other agents and type into panes without asking.
        #[arg(long)]
        allow_acting: bool,
    },
    /// Which events have the arugula hook, and which permission rules (exits 0 even if some don't).
    Status {
        /// Check DIR/.claude/settings.json instead of ~/.claude/settings.json.
        #[arg(long, value_name = "DIR")]
        project: Option<PathBuf>,
    },
    /// Move Claude Code's config off the old name (for `arugulad install`).
    ///
    /// In each Claude Code config here: our hooks under the old name renamed, and an MCP server that runs
    /// `illogical mcp` pointed at `arugula` (its name kept, so its tools'
    /// names and permission rules stay). Prints `{"old": true}` while
    /// anything there still runs `illogical`.
    #[command(hide = true)]
    RenameOld,
}

pub fn run(cmd: HooksCmd, json_out: bool) -> anyhow::Result<i32> {
    match cmd {
        HooksCmd::Install { project, dry_run, allow_acting } => {
            install(&settings_path(project)?, dry_run, allow_acting)
        }
        HooksCmd::Status { project } => status(&settings_path(project)?, json_out),
        HooksCmd::RenameOld => rename_old_here(),
    }
    .map(|()| 0)
}

fn home() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).context("no HOME")
}

fn settings_path(project: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let base = match project {
        Some(dir) => dir,
        None => PathBuf::from(std::env::var_os("HOME").context("no HOME")?),
    };
    Ok(base.join(".claude/settings.json"))
}

fn snippet() -> Map<String, Value> {
    let v: Value = serde_json::from_str(HOOKS_SNIPPET).expect("HOOKS_SNIPPET is JSON");
    match v["hooks"].clone() {
        Value::Object(m) => m,
        _ => unreachable!("HOOKS_SNIPPET has a hooks object"),
    }
}

/// Whether `groups` (an event's array) has a group with this matcher and
/// one of this group's commands. A group with another matcher is another
/// entry: `AskUserQuestion`'s `arugula ask` is not the bare `arugula hook`.
fn has_group(groups: &[Value], want: &Value) -> bool {
    let commands = |g: &Value| -> Vec<String> {
        g["hooks"].as_array().into_iter().flatten().filter_map(|h| h["command"].as_str().map(str::to_owned)).collect()
    };
    let wanted = commands(want);
    groups.iter().any(|g| g.get("matcher") == want.get("matcher") && commands(g).iter().any(|c| wanted.contains(c)))
}

/// Every command the snippet has (`arugula hook`, `arugula inbox`, ...).
fn our_commands() -> Vec<String> {
    let mut out: Vec<String> = snippet()
        .values()
        .flat_map(|v| v.as_array().cloned().unwrap_or_default())
        .flat_map(|g| g["hooks"].as_array().cloned().unwrap_or_default())
        .filter_map(|h| h["command"].as_str().map(str::to_owned))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// One of our commands under the old name (#505, #534), as `arugula` would
/// write it: `illogical hook` is `arugula hook`. Only the exact commands
/// install wrote; a path or anything else someone typed is theirs.
fn renamed(command: &str, ours: &[String]) -> Option<String> {
    let rest = command.strip_prefix(arugula_proto::rename::OLD)?.strip_prefix(' ')?;
    let new = format!("arugula {rest}");
    ours.contains(&new).then_some(new)
}

/// Our hooks under the old name, renamed in place (#505, #534), across every
/// event; a group that then repeats one already there (same matcher, same
/// commands, all ours) goes. Whether anything changed.
fn rename_old(hooks: &mut Map<String, Value>) -> bool {
    let ours = our_commands();
    let mut changed = false;
    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else { continue };
        let mut renamed_here = false;
        for g in groups.iter_mut() {
            for h in g.get_mut("hooks").and_then(Value::as_array_mut).into_iter().flatten() {
                if let Some(new) = h["command"].as_str().and_then(|c| renamed(c, &ours)) {
                    h["command"] = Value::String(new);
                    renamed_here = true;
                }
            }
        }
        if !renamed_here {
            continue;
        }
        changed = true;
        let key = |g: &Value| -> Option<(Option<Value>, Vec<String>)> {
            let mut cmds: Vec<String> = g["hooks"]
                .as_array()?
                .iter()
                .map(|h| h["command"].as_str().map(str::to_owned))
                .collect::<Option<_>>()?;
            if cmds.is_empty() || !cmds.iter().all(|c| ours.contains(c)) {
                return None;
            }
            cmds.sort();
            Some((g.get("matcher").cloned(), cmds))
        };
        let mut seen = Vec::new();
        groups.retain(|g| match key(g) {
            Some(k) if seen.contains(&k) => false,
            Some(k) => {
                seen.push(k);
                true
            }
            None => true,
        });
    }
    changed
}

/// What `merge` did.
#[derive(Debug, Default, PartialEq)]
struct Merged {
    /// Hooks that weren't there.
    added: bool,
    /// Ours under the old name, now under the new (#505, #534).
    renamed: bool,
    /// Permission rules that weren't there (#655).
    allowed: bool,
}

impl Merged {
    fn changed(&self) -> bool {
        self.added || self.renamed || self.allowed
    }
}

/// `permissions.allow` of `root`, made if it isn't there; refused if it
/// isn't an array.
fn allow_list(root: &mut Map<String, Value>) -> anyhow::Result<&mut Vec<Value>> {
    let permissions = root.entry("permissions").or_insert_with(|| json!({}));
    let Some(permissions) = permissions.as_object_mut() else {
        bail!("`permissions` in settings.json isn't an object; leaving it alone");
    };
    let allow = permissions.entry("allow").or_insert_with(|| json!([]));
    match allow.as_array_mut() {
        Some(allow) => Ok(allow),
        None => bail!("`permissions.allow` in settings.json isn't an array; leaving it alone"),
    }
}

/// The rules (the read-only tier, and the acting one with `allow_acting`)
/// that `allow` doesn't have, added at the end. Whether any were.
fn add_rules(allow: &mut Vec<Value>, allow_acting: bool) -> bool {
    let acting = if allow_acting { ACTING_RULES } else { &[] };
    let mut added = false;
    for rule in READ_RULES.iter().chain(acting) {
        if !allow.iter().any(|a| a.as_str() == Some(rule)) {
            allow.push(json!(rule));
            added = true;
        }
    }
    added
}

/// `settings` with our old-named hooks renamed, the snippet's hooks added
/// where missing, and the permission rules (read-only, and acting with
/// `allow_acting`) added where missing: what changed.
fn merge(settings: &mut Value, allow_acting: bool) -> anyhow::Result<Merged> {
    let Some(root) = settings.as_object_mut() else {
        bail!("settings.json isn't a JSON object; leaving it alone");
    };
    // Refused before anything changes, so a refusal leaves settings whole.
    allow_list(root)?;
    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    let Some(hooks) = hooks.as_object_mut() else {
        bail!("`hooks` in settings.json isn't an object; leaving it alone");
    };
    let want = snippet();
    for (event, groups) in hooks.iter() {
        if want.contains_key(event) && !groups.is_array() {
            bail!("`hooks.{event}` in settings.json isn't an array; leaving it alone");
        }
    }
    let renamed = rename_old(hooks);
    let mut changed = false;
    for (event, wanted) in want {
        let groups = hooks.entry(event.clone()).or_insert_with(|| json!([]));
        let Some(groups) = groups.as_array_mut() else {
            bail!("`hooks.{event}` in settings.json isn't an array; leaving it alone");
        };
        for want in wanted.as_array().into_iter().flatten() {
            if !has_group(groups, want) {
                groups.push(want.clone());
                changed = true;
            }
        }
    }
    let allowed = add_rules(allow_list(root)?, allow_acting);
    Ok(Merged { added: changed, renamed, allowed })
}

fn read_settings(path: &Path) -> anyhow::Result<Value> {
    match fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(json!({})),
        Ok(s) => {
            serde_json::from_str(&s).with_context(|| format!("{} isn't valid JSON; leaving it alone", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// A temp file beside `path`, then a rename, so Claude Code never reads half
/// a file. A symlinked settings.json (dotfiles) is written through, and the
/// mode of an existing file is kept.
fn write_atomic(path: &Path, text: &str) -> anyhow::Result<()> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = target.parent().with_context(|| format!("{} has no directory", path.display()))?;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("settings.json");
    let tmp = dir.join(format!(".{name}.arugula-{}", std::process::id()));
    let result = (|| {
        fs::write(&tmp, text)?;
        if let Ok(meta) = fs::metadata(&target) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        fs::rename(&tmp, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.with_context(|| format!("writing {}", target.display()))
}

fn pretty(v: &Value) -> String {
    format!("{}\n", serde_json::to_string_pretty(v).unwrap_or_default())
}

fn install(path: &Path, dry_run: bool, allow_acting: bool) -> anyhow::Result<()> {
    let mut settings = read_settings(path)?;
    let merged = merge(&mut settings, allow_acting)?;
    let text = pretty(&settings);
    if dry_run {
        print!("{text}");
        return Ok(());
    }
    if !merged.changed() && path.exists() {
        println!("{}: the hooks and permission rules are already there", path.display());
        return Ok(());
    }
    write_atomic(path, &text)?;
    let mut said = vec![];
    match (merged.renamed, merged.added) {
        (true, true) => said.push("hooks renamed from `illogical` to `arugula`, and the missing ones added"),
        (true, false) => said.push("hooks renamed from `illogical` to `arugula`"),
        (false, true) => said.push("hooks added"),
        (false, false) => {}
    }
    if merged.allowed {
        said.push(if allow_acting {
            "permission rules added (read-only, and acting)"
        } else {
            "permission rules added (read-only)"
        });
    }
    println!("{}: {}", path.display(), said.join("; "));
    Ok(())
}

/// Where an event's hooks are.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Has {
    Present,
    /// All there, some under the old name (#505, #534): they run `illogical`,
    /// which this machine may no longer have (#534), until `install`
    /// renames them.
    Old,
    Missing,
}

/// Per event: whether every Arugula entry for it is in `settings`.
fn present(settings: &Value) -> Vec<(String, Has)> {
    let mut renamed = settings["hooks"].as_object().cloned().unwrap_or_default();
    rename_old(&mut renamed);
    let all = |hooks: &Value, event: &str, wanted: &Value| {
        let groups = hooks[event].as_array().map(Vec::as_slice).unwrap_or_default();
        wanted.as_array().into_iter().flatten().all(|w| has_group(groups, w))
    };
    let renamed = Value::Object(renamed);
    snippet()
        .into_iter()
        .map(|(event, wanted)| {
            let has = if all(&settings["hooks"], &event, &wanted) {
                Has::Present
            } else if all(&renamed, &event, &wanted) {
                Has::Old
            } else {
                Has::Missing
            };
            (event, has)
        })
        .collect()
}

/// How much of a tier's rules `permissions.allow` has.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Tier {
    Present,
    Partial,
    Missing,
}

impl Tier {
    fn says(self) -> &'static str {
        match self {
            Tier::Present => "present",
            Tier::Partial => "partial",
            Tier::Missing => "missing",
        }
    }
}

/// The read-only tier and the acting tier of `settings`' `permissions.allow`.
fn tiers(settings: &Value) -> (Tier, Tier) {
    let allow = settings["permissions"]["allow"].as_array().map(Vec::as_slice).unwrap_or_default();
    let tier = |rules: &[&str]| {
        let has = rules.iter().filter(|r| allow.iter().any(|a| a.as_str() == Some(r))).count();
        match has {
            0 => Tier::Missing,
            n if n == rules.len() => Tier::Present,
            _ => Tier::Partial,
        }
    };
    (tier(READ_RULES), tier(ACTING_RULES))
}

fn status(path: &Path, json_out: bool) -> anyhow::Result<()> {
    let settings = read_settings(path)?;
    let events = present(&settings);
    let (read, acting) = tiers(&settings);
    let installed = events.iter().all(|(_, h)| *h != Has::Missing);
    let old: Vec<&str> = events.iter().filter(|(_, h)| *h == Has::Old).map(|(e, _)| e.as_str()).collect();
    if json_out {
        // `events` stays a bool per event: an old-named hook counts.
        let map: Map<String, Value> =
            events.iter().map(|(e, h)| (e.clone(), Value::Bool(*h != Has::Missing))).collect();
        let permissions = json!({ "read": read.says(), "acting": acting.says() });
        let v = json!({
            "settings": path, "installed": installed, "events": map, "old_name": old, "permissions": permissions
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return Ok(());
    }
    println!("{}", path.display());
    for (event, h) in &events {
        let says = match h {
            Has::Present => "present",
            Has::Old => "present, under the old name (illogical)",
            Has::Missing => "missing",
        };
        println!("  {event:<20} {says}");
    }
    println!("permissions.allow");
    println!("  {:<20} {}", "read", read.says());
    println!("  {:<20} {}", "acting", acting.says());
    match (installed, old.is_empty()) {
        (false, _) => println!("`arugula hooks install` adds the missing ones."),
        (true, false) => println!("`arugula hooks install` updates them to the new name."),
        (true, true) => {}
    }
    if read != Tier::Present {
        println!("`arugula hooks install` adds the read-only permission rules.");
    }
    if acting != Tier::Present {
        println!(
            "`arugula hooks install --allow-acting` adds the acting ones (an agent can then start agents and type into panes without asking)."
        );
    }
    Ok(())
}

/// The program a command runs is ours under the old name: `illogical` or
/// `illogicald`, bare or as a path, `.exe` or not.
fn runs_old(command: &str) -> bool {
    let program = command.split_whitespace().next().unwrap_or("");
    let name = program.rsplit(['/', '\\']).next().unwrap_or("");
    let name =
        name.len().checked_sub(4).filter(|&i| name[i..].eq_ignore_ascii_case(".exe")).map_or(name, |i| &name[..i]);
    name == "illogical" || name == "illogicald"
}

/// Each `mcpServers` in Claude Code's `.claude.json`: the user's, and each
/// project's.
fn mcp_servers(v: &mut Value) -> Vec<&mut Map<String, Value>> {
    let Some(root) = v.as_object_mut() else { return vec![] };
    let mut out = vec![];
    for (k, x) in root.iter_mut() {
        match k.as_str() {
            "mcpServers" => out.extend(x.as_object_mut()),
            "projects" => {
                for p in x.as_object_mut().into_iter().flat_map(|p| p.values_mut()) {
                    out.extend(p.get_mut("mcpServers").and_then(Value::as_object_mut));
                }
            }
            _ => {}
        }
    }
    out
}

/// MCP servers that run `illogical mcp`, run with `arugula` (`cli`, where
/// the old one was a path and none is beside it) instead. Their names
/// stay. Whether anything changed.
fn rename_mcp(v: &mut Value, cli: &Path) -> bool {
    let mut changed = false;
    for servers in mcp_servers(v) {
        for server in servers.values_mut() {
            let Some(command) = server["command"].as_str().filter(|c| runs_old(c)) else { continue };
            if server["args"][0] != "mcp" || command.contains(char::is_whitespace) {
                continue;
            }
            let new = if !command.contains(['/', '\\']) {
                "arugula".to_owned()
            } else {
                let exe = if command.to_ascii_lowercase().ends_with(".exe") { "arugula.exe" } else { "arugula" };
                let beside = Path::new(command).with_file_name(exe);
                if beside.is_file() { beside } else { cli.to_path_buf() }.display().to_string()
            };
            server["command"] = Value::String(new);
            changed = true;
        }
    }
    changed
}

/// Something in `settings` (its hooks) or `claude_json` (its MCP servers)
/// still runs `illogical`.
fn calls_old(settings: &Value, claude_json: &mut Value) -> bool {
    let hooks = settings["hooks"].as_object().into_iter().flat_map(|m| m.values());
    let groups = hooks.flat_map(|g| g.as_array().into_iter().flatten());
    let commands = groups.flat_map(|g| g["hooks"].as_array().into_iter().flatten());
    commands.filter_map(|h| h["command"].as_str()).any(runs_old)
        || mcp_servers(claude_json).iter().flat_map(|s| s.values()).filter_map(|s| s["command"].as_str()).any(runs_old)
}

/// Claude Code's configs here: its settings.json and its `.claude.json`,
/// under `~` and under `CLAUDE_CONFIG_DIR` when that's set.
fn configs(home: &Path, config_dir: Option<PathBuf>) -> Vec<(PathBuf, PathBuf)> {
    let mut out = vec![(home.join(".claude/settings.json"), home.join(".claude.json"))];
    if let Some(d) = config_dir.filter(|d| d != &home.join(".claude")) {
        out.push((d.join("settings.json"), d.join(".claude.json")));
    }
    out
}

/// [`HooksCmd::RenameOld`] on `configs`, with `cli` for the MCP server:
/// whether anything there still runs `illogical` after.
fn rename_old_in(configs: &[(PathBuf, PathBuf)], cli: &Path) -> bool {
    let mut old = false;
    for (settings_file, claude_file) in configs {
        let settings = read_settings(settings_file).map(|mut v| {
            if let Some(hooks) = v.get_mut("hooks").and_then(Value::as_object_mut)
                && rename_old(hooks)
            {
                match write_atomic(settings_file, &pretty(&v)) {
                    Ok(()) => println!("{}: hooks renamed from `illogical` to `arugula`", settings_file.display()),
                    Err(e) => eprintln!("arugula: {e:#}"),
                }
            }
        });
        let claude = read_settings(claude_file).map(|mut v| {
            if rename_mcp(&mut v, cli) {
                match write_atomic(claude_file, &pretty(&v)) {
                    Ok(()) => println!("{}: Claude Code's MCP server runs `arugula` now", claude_file.display()),
                    Err(e) => eprintln!("arugula: {e:#}"),
                }
            }
        });
        // Read again: Claude Code may have written either meanwhile, and
        // one we can't read could be calling anything.
        old |= settings.is_err() || claude.is_err();
        match (read_settings(settings_file), read_settings(claude_file)) {
            (Ok(s), Ok(mut c)) => old |= calls_old(&s, &mut c),
            _ => old = true,
        }
    }
    old
}

fn rename_old_here() -> anyhow::Result<()> {
    let configs = configs(&home()?, std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from));
    let cli = std::env::current_exe()?;
    let old = rename_old_in(&configs, &cli);
    println!("{}", json!({ "old": old }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the system temp dir.
    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arugula-hooks-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A settings file with a user's own hooks on PreToolUse and Stop, and
    /// keys that aren't hooks.
    const MINE: &str = r#"{
  "model": "opus",
  "permissions": { "allow": ["Bash(ls:*)"] },
  "hooks": {
    "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "my-lint" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "say done" }] }]
  }
}"#;

    fn merged(text: &str) -> (Value, bool) {
        let mut v: Value = serde_json::from_str(text).unwrap();
        let changed = merge(&mut v, false).unwrap().changed();
        (v, changed)
    }

    #[test]
    fn merge_keeps_what_is_there() {
        let (v, changed) = merged(MINE);
        assert!(changed);
        assert_eq!(v["model"], "opus");
        assert_eq!(v["permissions"]["allow"][0], "Bash(ls:*)");
        assert_eq!(v["permissions"]["allow"].as_array().unwrap().len(), 1 + READ_RULES.len());
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre[0]["hooks"][0]["command"], "my-lint");
        assert_eq!(pre.len(), 3);
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop[0]["hooks"][0]["command"], "say done");
        assert_eq!(stop.len(), 3);
        assert!(present(&v).iter().all(|(_, p)| *p == Has::Present));
    }

    #[test]
    fn merge_twice_changes_nothing() {
        let (once, _) = merged(MINE);
        let (twice, changed) = merged(&serde_json::to_string(&once).unwrap());
        assert!(!changed);
        assert_eq!(once, twice);
        let (from_empty, _) = merged("{}");
        assert_eq!(from_empty["hooks"], Value::Object(snippet()));
    }

    #[test]
    fn ask_matcher_is_its_own_entry() {
        // A bare `arugula ask` (no matcher) isn't the AskUserQuestion one,
        // and the bare `arugula hook` isn't it either.
        let bare = r#"{"hooks":{"PreToolUse":[
            {"hooks":[{"type":"command","command":"arugula ask"}]},
            {"matcher":"Bash","hooks":[{"type":"command","command":"arugula hook"}]}]}}"#;
        let (v, changed) = merged(bare);
        assert!(changed);
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 4);
        assert!(pre.iter().any(|g| g["matcher"] == "AskUserQuestion"));
        assert!(pre.iter().any(|g| g.get("matcher").is_none() && g["hooks"][0]["command"] == "arugula hook"));
        // Only the AskUserQuestion one present: the bare hook is still missing.
        let only_ask = r#"{"hooks":{"PreToolUse":[{"matcher":"AskUserQuestion","hooks":[{"type":"command","command":"arugula ask"}]}]}}"#;
        let (v, _) = merged(only_ask);
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn odd_settings_are_refused_not_replaced() {
        for bad in [r#"[]"#, r#"{"hooks": []}"#, r#"{"hooks": {"Stop": {}}}"#] {
            let mut v: Value = serde_json::from_str(bad).unwrap();
            assert!(merge(&mut v, false).is_err(), "{bad}");
        }
    }

    #[test]
    fn install_writes_once_and_is_byte_identical_after() {
        let dir = temp("install");
        let path = dir.join(".claude/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, MINE).unwrap();
        install(&path, false, false).unwrap();
        let first = fs::read(&path).unwrap();
        install(&path, false, false).unwrap();
        assert_eq!(first, fs::read(&path).unwrap());
        let v: Value = serde_json::from_slice(&first).unwrap();
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "say done");
        assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "my-lint");
        // No temp file left beside it.
        let names: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["settings.json"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_creates_the_file_and_dry_run_writes_nothing() {
        let dir = temp("create");
        let path = dir.join(".claude/settings.json");
        install(&path, true, false).unwrap();
        assert!(!path.exists());
        install(&path, false, false).unwrap();
        assert!(present(&read_settings(&path).unwrap()).iter().all(|(_, p)| *p == Has::Present));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_json_is_left_alone() {
        let dir = temp("invalid");
        let path = dir.join("settings.json");
        fs::write(&path, "{ not json").unwrap();
        assert!(install(&path, false, false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_per_event() {
        let (v, _) = merged(MINE);
        assert!(present(&v).iter().all(|(_, p)| *p == Has::Present));
        let none: Value = serde_json::from_str(MINE).unwrap();
        assert!(present(&none).iter().all(|(_, p)| *p == Has::Missing));
        // Half of Stop's entries is not enough for Stop.
        let half = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"arugula attention done"}]}]}}"#;
        let p = present(&serde_json::from_str(half).unwrap());
        assert!(p.iter().any(|(e, h)| e == "Stop" && *h == Has::Missing));
    }

    /// What `illogical hooks install` wrote before the rename (#505, #534), in
    /// a settings file with the user's own hooks and one that only looks
    /// like ours.
    fn old_install() -> Value {
        let old: Value = serde_json::from_str(&HOOKS_SNIPPET.replace("\"arugula ", "\"illogical ")).unwrap();
        let mut v: Value = serde_json::from_str(MINE).unwrap();
        for (event, groups) in old["hooks"].as_object().unwrap() {
            let list = v["hooks"].as_object_mut().unwrap().entry(event.clone()).or_insert_with(|| json!([]));
            list.as_array_mut().unwrap().extend(groups.as_array().unwrap().iter().cloned());
        }
        v["hooks"]["Stop"].as_array_mut().unwrap().push(json!({ "hooks": [
            { "type": "command", "command": "/opt/illogical/bin/illogical hook" },
            { "type": "command", "command": "illogical hooks-are-mine" }
        ] }));
        v
    }

    fn commands(v: &Value) -> Vec<String> {
        let mut out = vec![];
        for groups in v["hooks"].as_object().unwrap().values() {
            for g in groups.as_array().unwrap() {
                for h in g["hooks"].as_array().unwrap() {
                    out.push(h["command"].as_str().unwrap().to_owned());
                }
            }
        }
        out
    }

    #[test]
    fn old_named_hooks_are_renamed_not_doubled() {
        let mut v = old_install();
        assert!(present(&v).iter().all(|(_, h)| *h == Has::Old));
        let m = merge(&mut v, false).unwrap();
        assert_eq!(m, Merged { added: false, renamed: true, allowed: true });
        assert!(present(&v).iter().all(|(_, h)| *h == Has::Present));
        // As many groups as a fresh install beside the user's own.
        let (fresh, _) = merged(MINE);
        for event in snippet().keys() {
            let n = |v: &Value| v["hooks"][event].as_array().unwrap().len();
            let extra = usize::from(event == "Stop");
            assert_eq!(n(&v), n(&fresh) + extra, "{event}");
        }
        let cmds = commands(&v);
        assert!(cmds.iter().all(|c| !c.starts_with("illogical ") || c == "illogical hooks-are-mine"), "{cmds:?}");
        // The user's own, and the ones that only look like ours, as they were.
        assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "my-lint");
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "say done");
        assert!(cmds.contains(&"/opt/illogical/bin/illogical hook".to_owned()));
        assert!(cmds.contains(&"illogical hooks-are-mine".to_owned()));
        assert_eq!(v["model"], "opus");
        // Again: nothing.
        let before = v.clone();
        assert_eq!(merge(&mut v, false).unwrap(), Merged::default());
        assert_eq!(v, before);
    }

    #[test]
    fn old_and_new_both_there_leave_one() {
        // Someone ran a new install by hand beside the old one: one copy each.
        let mut v = old_install();
        let fresh: Value = serde_json::from_str(HOOKS_SNIPPET).unwrap();
        for (event, groups) in fresh["hooks"].as_object().unwrap() {
            v["hooks"][event].as_array_mut().unwrap().extend(groups.as_array().unwrap().iter().cloned());
        }
        assert!(present(&v).iter().all(|(_, h)| *h == Has::Present));
        let m = merge(&mut v, false).unwrap();
        assert!(m.renamed && !m.added);
        let (fresh, _) = merged(MINE);
        assert_eq!(v["hooks"]["PreToolUse"], fresh["hooks"]["PreToolUse"]);
        assert_eq!(v["hooks"]["SessionStart"], fresh["hooks"]["SessionStart"]);
    }

    #[test]
    fn half_old_half_missing() {
        // Only the old Stop hooks: Stop is there under the old name, the
        // rest is missing, and install renames and adds.
        let v: Value = serde_json::from_str(
            r#"{"hooks":{"Stop":[
            {"hooks":[{"type":"command","command":"illogical attention done"}]},
            {"hooks":[{"type":"command","command":"illogical inbox","asyncRewake":true,"timeout":86400}]}]}}"#,
        )
        .unwrap();
        let p = present(&v);
        assert!(p.iter().any(|(e, h)| e == "Stop" && *h == Has::Old));
        assert!(p.iter().any(|(e, h)| e == "Notification" && *h == Has::Missing));
        let mut v = v;
        assert_eq!(merge(&mut v, false).unwrap(), Merged { added: true, renamed: true, allowed: true });
        let (from_empty, _) = merged("{}");
        assert_eq!(v, from_empty);
    }

    #[test]
    fn install_renames_on_disk() {
        let dir = temp("rename");
        let path = dir.join(".claude/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, pretty(&old_install())).unwrap();
        install(&path, false, false).unwrap();
        let first = fs::read(&path).unwrap();
        assert!(present(&serde_json::from_slice(&first).unwrap()).iter().all(|(_, h)| *h == Has::Present));
        install(&path, false, false).unwrap();
        assert_eq!(first, fs::read(&path).unwrap());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn knows_the_old_programs() {
        for c in
            ["illogical", "illogical hook", "/home/me/.local/bin/illogical mcp", r"C:\x\illogical.EXE", "illogicald"]
        {
            assert!(runs_old(c), "{c}");
        }
        for c in ["arugula hook", "my-illogical", "/opt/illogical/bin/arugula", "say illogical", ""] {
            assert!(!runs_old(c), "{c}");
        }
    }

    /// #534: `arugulad install` renames our hooks and points the MCP
    /// server at `arugula`, keeping its name; whatever else still runs
    /// `illogical` is reported.
    #[test]
    fn rename_old_renames_hooks_and_the_mcp_server() {
        let home = temp("rename-old");
        let (settings, claude) = (home.join(".claude/settings.json"), home.join(".claude.json"));
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        let bin = home.join(".local/bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("arugula"), "").unwrap();
        let mut old = old_install();
        // Not ours to rename (a path someone typed), so still old.
        old["hooks"]["Stop"].as_array_mut().unwrap().pop();
        fs::write(&settings, pretty(&old)).unwrap();
        let path = bin.join("illogical").display().to_string();
        fs::write(
            &claude,
            pretty(&json!({
                "numStartups": 9,
                "mcpServers": {
                    "illogical": { "type": "stdio", "command": "illogical", "args": ["mcp"], "env": {} },
                    "other": { "command": "illogical-ish", "args": ["mcp"] }
                },
                "projects": { "/p": { "mcpServers": { "mine": { "command": path, "args": ["mcp", "--socket", "/s"] } } } }
            })),
        )
        .unwrap();
        let configs = configs(&home, None);
        let cli = Path::new("/elsewhere/arugula");
        assert!(!rename_old_in(&configs, cli), "nothing runs illogical after");
        let s = read_settings(&settings).unwrap();
        assert!(present(&s).iter().all(|(_, h)| *h == Has::Present));
        assert_eq!(s["model"], "opus");
        let c = read_settings(&claude).unwrap();
        assert_eq!(c["mcpServers"]["illogical"]["command"], "arugula", "renamed program, same name");
        assert_eq!(c["mcpServers"]["illogical"]["args"], json!(["mcp"]));
        assert_eq!(c["mcpServers"]["other"]["command"], "illogical-ish");
        assert_eq!(c["projects"]["/p"]["mcpServers"]["mine"]["command"], bin.join("arugula").display().to_string());
        assert_eq!(c["numStartups"], 9);
        // Again: nothing to do, nothing written.
        let before = (fs::read(&settings).unwrap(), fs::read(&claude).unwrap());
        assert!(!rename_old_in(&configs, cli));
        assert_eq!(before, (fs::read(&settings).unwrap(), fs::read(&claude).unwrap()));

        // A hook by path is someone's own: kept, and reported.
        let mut s = s;
        s["hooks"]["Stop"].as_array_mut().unwrap().push(json!({ "hooks": [
            { "type": "command", "command": "/opt/illogical/bin/illogical hook" }
        ] }));
        fs::write(&settings, pretty(&s)).unwrap();
        assert!(rename_old_in(&configs, cli));
        // No config at all: nothing runs illogical.
        let none = temp("rename-old-none");
        assert!(!rename_old_in(&super::configs(&none, None), cli));
        assert!(!none.join(".claude.json").exists());
        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(&none);
    }

    /// `permissions.allow` of `v`, as strings.
    fn allowed(v: &Value) -> Vec<String> {
        v["permissions"]["allow"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_owned()).collect()
    }

    #[test]
    fn rules_are_exactly_these() {
        // The strings Claude Code matches on, so a typo or a widened
        // `Bash(arugula:*)` shows here.
        let read = [
            "Bash(arugula ls:*)",
            "Bash(arugula describe:*)",
            "Bash(arugula tail:*)",
            "Bash(arugula wait:*)",
            "Bash(arugula history:*)",
            "Bash(arugula log:*)",
            "Bash(arugula search:*)",
            "Bash(arugula capture:*)",
            "Bash(arugula process:*)",
            "Bash(arugula status:*)",
            "Bash(arugula fs:*)",
            "Bash(arugula hooks status:*)",
            "Bash(arugula hosts)",
            "Bash(arugula shares)",
            "mcp__arugula__read_output",
            "mcp__arugula__wait",
            "mcp__arugula__list",
            "mcp__arugula__history",
            "mcp__arugula__read_forge",
            "mcp__arugula__read_invite",
            "mcp__arugula__read_file",
            "mcp__arugula__read_thread",
        ];
        assert_eq!(READ_RULES, read);
        let acting_cli = [
            "agent", "run", "send", "keys", "close", "call", "open", "edit", "view", "diff", "pr", "issue", "cd",
            "rerun", "upload", "mouse", "share", "invite",
        ];
        let acting: Vec<&str> = ACTING_RULES.iter().copied().filter(|r| r.starts_with("Bash(")).collect();
        let want: Vec<String> = acting_cli.iter().map(|c| format!("Bash(arugula {c}:*)")).collect();
        assert_eq!(acting, want);
        for rule in READ_RULES.iter().chain(ACTING_RULES) {
            assert_ne!(*rule, "Bash(arugula:*)");
            assert!(!rule.contains("hooks install") && !rule.contains("attention"), "{rule}");
            assert!(!ACTING_RULES.contains(rule) || !READ_RULES.contains(rule), "{rule} is in both tiers");
        }
    }

    #[test]
    fn mcp_rules_follow_the_permissions_snippet() {
        // docs/mcp-permissions.json is what the daemon's own test checks
        // against `read_only` (tools.rs), so tying the CLI's lists to it
        // ties them to the tools. The chat tools and `delegate` are the
        // ones it leaves out (flags).
        let snippet: Value = serde_json::from_str(include_str!("../../../docs/mcp-permissions.json")).unwrap();
        let list = |key: &str| -> Vec<String> {
            snippet["permissions"][key].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_owned()).collect()
        };
        let mcp = |rules: &[&str]| -> Vec<String> {
            rules.iter().filter(|r| r.starts_with("mcp__")).map(|r| (*r).to_owned()).collect()
        };
        let (read, acting) = (mcp(READ_RULES), mcp(ACTING_RULES));
        let flagged = ["mcp__arugula__read_thread", "mcp__arugula__post_thread", "mcp__arugula__delegate"];
        let without =
            |v: &[String]| -> Vec<String> { v.iter().filter(|r| !flagged.contains(&r.as_str())).cloned().collect() };
        assert_eq!(without(&read), list("allow"));
        let mut a = without(&acting);
        let mut want = list("ask");
        a.sort();
        want.sort();
        assert_eq!(a, want);
    }

    #[test]
    fn install_adds_the_read_tier_only_by_default() {
        let (v, changed) = merged("{}");
        assert!(changed);
        assert_eq!(allowed(&v), READ_RULES);
        assert!(allowed(&v).iter().all(|r| !ACTING_RULES.contains(&r.as_str())));
        assert_eq!(tiers(&v), (Tier::Present, Tier::Missing));
    }

    #[test]
    fn allow_acting_adds_both_and_keeps_every_existing_entry() {
        let mut v: Value = serde_json::from_str(MINE).unwrap();
        v["permissions"]["deny"] = json!(["Bash(rm:*)"]);
        // One of ours already there, and an entry that only looks like one.
        v["permissions"]["allow"] = json!(["Bash(ls:*)", "Bash(arugula ls:*)", "Bash(arugula lsx:*)", 7]);
        assert!(merge(&mut v, true).unwrap().allowed);
        let allow = v["permissions"]["allow"].as_array().unwrap();
        assert_eq!(
            allow[..4],
            [json!("Bash(ls:*)"), json!("Bash(arugula ls:*)"), json!("Bash(arugula lsx:*)"), json!(7)]
        );
        let rules = allowed_strings(allow);
        for rule in READ_RULES.iter().chain(ACTING_RULES) {
            assert_eq!(rules.iter().filter(|r| *r == rule).count(), 1, "{rule}");
        }
        assert_eq!(allow.len(), 4 + READ_RULES.len() + ACTING_RULES.len() - 1);
        assert_eq!(v["permissions"]["deny"], json!(["Bash(rm:*)"]));
        assert_eq!(v["model"], "opus");
        assert_eq!(tiers(&v), (Tier::Present, Tier::Present));
        // Again, with or without the flag: nothing, and nothing taken away.
        let before = v.clone();
        assert_eq!(merge(&mut v, true).unwrap(), Merged::default());
        assert_eq!(merge(&mut v, false).unwrap(), Merged::default());
        assert_eq!(v, before);
    }

    fn allowed_strings(a: &[Value]) -> Vec<String> {
        a.iter().filter_map(|a| a.as_str().map(str::to_owned)).collect()
    }

    #[test]
    fn a_later_install_without_the_flag_keeps_the_acting_rules() {
        let mut v = json!({});
        merge(&mut v, true).unwrap();
        let before = v.clone();
        assert_eq!(merge(&mut v, false).unwrap(), Merged::default());
        assert_eq!(v, before);
        // The other way: read first, acting added later, nothing doubled.
        let mut v = json!({});
        merge(&mut v, false).unwrap();
        assert!(merge(&mut v, true).unwrap().allowed);
        let mut w = json!({});
        merge(&mut w, true).unwrap();
        assert_eq!(v, w);
    }

    #[test]
    fn odd_permissions_are_refused_not_replaced() {
        for bad in [r#"{"permissions": []}"#, r#"{"permissions": {"allow": "x"}}"#, r#"{"permissions": {"allow": {}}}"#]
        {
            let mut v: Value = serde_json::from_str(bad).unwrap();
            let before = v.clone();
            assert!(merge(&mut v, true).is_err(), "{bad}");
            assert_eq!(v, before, "{bad}: left whole");
        }
    }

    #[test]
    fn status_says_present_partial_or_missing_per_tier() {
        let none: Value = serde_json::from_str(MINE).unwrap();
        assert_eq!(tiers(&none), (Tier::Missing, Tier::Missing));
        assert_eq!(tiers(&json!({})), (Tier::Missing, Tier::Missing));
        let one = json!({ "permissions": { "allow": [READ_RULES[0], ACTING_RULES[0]] } });
        assert_eq!(tiers(&one), (Tier::Partial, Tier::Partial));
        let (v, _) = merged(MINE);
        assert_eq!(tiers(&v), (Tier::Present, Tier::Missing));
        let mut w: Value = serde_json::from_str(MINE).unwrap();
        merge(&mut w, true).unwrap();
        assert_eq!(tiers(&w), (Tier::Present, Tier::Present));
        // A tier missing one rule is partial.
        w["permissions"]["allow"].as_array_mut().unwrap().retain(|r| r != READ_RULES[3]);
        assert_eq!(tiers(&w), (Tier::Partial, Tier::Present));
        assert_eq!(
            (Tier::Present.says(), Tier::Partial.says(), Tier::Missing.says()),
            ("present", "partial", "missing")
        );
    }

    #[test]
    fn install_on_disk_adds_tiers_once_and_dry_run_writes_nothing() {
        let dir = temp("tiers");
        let path = dir.join(".claude/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, MINE).unwrap();
        install(&path, true, true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), MINE);
        install(&path, false, false).unwrap();
        let read_only = read_settings(&path).unwrap();
        assert_eq!(allowed(&read_only)[0], "Bash(ls:*)");
        assert_eq!(tiers(&read_only), (Tier::Present, Tier::Missing));
        install(&path, false, true).unwrap();
        let first = fs::read(&path).unwrap();
        assert_eq!(tiers(&serde_json::from_slice(&first).unwrap()), (Tier::Present, Tier::Present));
        install(&path, false, true).unwrap();
        install(&path, false, false).unwrap();
        assert_eq!(first, fs::read(&path).unwrap());
        let _ = fs::remove_dir_all(&dir);
    }
}
