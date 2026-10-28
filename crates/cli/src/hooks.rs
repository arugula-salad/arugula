//! `arugula hooks`: put Claude Code's hooks for Arugula into its
//! settings.json (`install`), or say which are there (`status`). The merge
//! only ever adds: every other key and every other hook stays as it is, and
//! a hook already there (same event, same matcher, same command) isn't
//! added twice, so a second `install` changes nothing.
//!
//! `install` also adds `permissions.allow` rules (#655), in two tiers, so an
//! agent in auto mode can use the CLI it was just wired to: the read-only
//! commands and MCP tools by default, the acting ones only with
//! `--allow-acting`. Those only ever add too, and `status` says which tier
//! is there.
//!
//! The Claude Code plugin (plugins/claude-code, #229) carries the same
//! hooks; a test holds its hooks/hooks.json to [`HOOKS_SNIPPET`]. Claude
//! Code runs a plugin's hook beside an identical one in settings.json (it
//! only dedupes across settings files), so the plugin's copy stands aside
//! when a settings file has the same hook ([`plugin_copy_stands_aside`]):
//! someone who ran `install` and then added the plugin gets each hook once.
//! `status` names the plugin when settings enable it, and `install` then
//! adds only the permission rules.

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
    "SessionStart": [
      { "hooks": [{ "type": "command", "command": "arugula inbox", "asyncRewake": true, "timeout": 86400 }] },
      { "hooks": [{ "type": "command", "command": "arugula hook" }] }
    ],
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

/// The Claude Code plugin's name (plugins/claude-code, #229): `enabledPlugins`
/// lists it as `arugula@<marketplace>`.
pub const PLUGIN: &str = "arugula";

/// How Claude Code names the plugin's MCP tools: `mcp__plugin_<plugin>_<server>__`,
/// where the plugin's .mcp.json names its server `arugula` too.
const PLUGIN_TOOLS: &str = "mcp__plugin_arugula_arugula__";

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
    // M216: reading runs. Starting one is `arugula run:*`'s prefix, which
    // is acting (and the CLI refuses an agent's start).
    "Bash(arugula runs list:*)",
    "Bash(arugula runs ls:*)",
    "Bash(arugula runs show:*)",
    "Bash(arugula runs logs:*)",
    // The MCP tools with `read_only_hint` in crates/daemon/src/mcp_server/tools.rs
    // (the CLI can't depend on the daemon; docs/mcp-permissions.json lists
    // the same, and a test here checks it).
    "mcp__arugula__read_output",
    "mcp__arugula__wait",
    "mcp__arugula__list",
    "mcp__arugula__capabilities",
    "mcp__arugula__history",
    "mcp__arugula__read_forge",
    "mcp__arugula__forge_read",
    "mcp__arugula__read_invite",
    "mcp__arugula__read_file",
    "mcp__arugula__read_cloud",
    "mcp__arugula__read_thread",
    "mcp__arugula__search_threads",
    "mcp__arugula__read_run",
    "mcp__arugula__read_deploy",
    "mcp__arugula__read_session",
];

/// What `install` adds beside [`READ_RULES`], without `--allow-acting`
/// (#963): `doc` builds a Doc in a block beside the pane, and writes only
/// that block's own folder. It is a write, so docs/mcp-permissions.json
/// lists it under `ask`, with the other writes.
const OWN_WRITE_RULES: &[&str] = &["mcp__arugula__doc"];

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
    "Bash(arugula draft:*)",
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
    "mcp__arugula__forge_draft",
    "mcp__arugula__invite_person",
    "mcp__arugula__device_call",
    "mcp__arugula__cloud",
    "mcp__arugula__link_repo",
    "mcp__arugula__post_thread",
    "mcp__arugula__issue_write",
    "mcp__arugula__start_run",
    "mcp__arugula__plan_deploy",
    "mcp__arugula__session_start",
    "mcp__arugula__session_reply",
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
    ///
    /// With `--json`, says what changed: `settings` (the file),
    /// `hooks_added`, `rules_added` and `plugin`.
    Install {
        /// Write DIR/.claude/settings.json instead of the user's
        /// ($CLAUDE_CONFIG_DIR/settings.json, else ~/.claude/settings.json).
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
    /// Which hooks and permission rules are there (exits 0 even if some aren't).
    Status {
        /// Check DIR/.claude/settings.json instead of the user's
        /// ($CLAUDE_CONFIG_DIR/settings.json, else ~/.claude/settings.json).
        #[arg(long, value_name = "DIR")]
        project: Option<PathBuf>,
    },
}

pub fn run(cmd: HooksCmd, json_out: bool) -> anyhow::Result<i32> {
    match cmd {
        HooksCmd::Install { project, dry_run, allow_acting } => {
            install_for(&settings_path(project)?, dry_run, allow_acting, plugin_for_user(), json_out)
        }
        HooksCmd::Status { project } => status_for(&settings_path(project)?, json_out, plugin_for_user()),
    }
    .map(|()| 0)
}

fn home() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).context("no HOME")
}

fn settings_path(project: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    match project {
        Some(dir) => Ok(dir.join(".claude/settings.json")),
        None => Ok(user_settings_in(&home()?, std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from))),
    }
}

/// The user's settings.json that Claude Code reads: `$CLAUDE_CONFIG_DIR`'s
/// when that's set, else `~/.claude`'s, never both (#879).
fn user_settings_in(home: &Path, config_dir: Option<PathBuf>) -> PathBuf {
    let dir = config_dir.filter(|d| !d.as_os_str().is_empty()).unwrap_or_else(|| home.join(".claude"));
    dir.join("settings.json")
}

/// The user's own settings.json, as [`user_settings_in`] has it.
fn user_settings() -> Vec<PathBuf> {
    settings_path(None).into_iter().collect()
}

/// The plugin's `enabledPlugins` key (`arugula@arugula`) when `settings`
/// turns it on.
fn plugin_enabled(settings: &Value) -> Option<String> {
    let enabled = settings["enabledPlugins"].as_object()?;
    enabled
        .iter()
        .find(|(k, v)| k.split_once('@').is_some_and(|(name, _)| name == PLUGIN) && v.as_bool() == Some(true))
        .map(|(k, _)| k.clone())
}

/// The plugin, when the user's settings enable it (a project's settings.json
/// may too: `install` and `status` check the file they're given as well).
fn plugin_for_user() -> Option<String> {
    user_settings().iter().filter_map(|p| read_settings(p).ok()).find_map(|s| plugin_enabled(&s))
}

/// Whether `dir` holds the Arugula plugin (its `.claude-plugin/plugin.json`
/// names it).
fn is_our_plugin(dir: &Path) -> bool {
    let manifest = fs::read_to_string(dir.join(".claude-plugin/plugin.json")).unwrap_or_default();
    serde_json::from_str::<Value>(&manifest).is_ok_and(|m| m["name"] == PLUGIN)
}

/// The settings files whose hooks this Claude Code session runs, as far as
/// `install` writes them: the user's, and the project's (`CLAUDE_PROJECT_DIR`,
/// which Claude Code sets for every hook) with its local one.
fn session_settings() -> Vec<PathBuf> {
    let mut out = user_settings();
    if let Some(p) = std::env::var_os("CLAUDE_PROJECT_DIR").map(PathBuf::from) {
        out.push(p.join(".claude/settings.json"));
        out.push(p.join(".claude/settings.local.json"));
    }
    out.dedup();
    out
}

/// Whether one of `files` has the hook that runs `command` on `event` (any
/// event of the snippet's, when the hook can't tell, as `attention` can't:
/// each of its commands is on one event only).
fn settings_have(files: &[Value], command: &str, event: Option<&str>) -> bool {
    snippet()
        .iter()
        .filter(|(e, _)| event.is_none_or(|ev| ev == e.as_str()))
        .flat_map(|(e, groups)| groups.as_array().into_iter().flatten().map(move |g| (e, g)))
        .filter(|(_, g)| g["hooks"].as_array().into_iter().flatten().any(|h| h["command"] == command))
        .any(|(e, g)| {
            files.iter().any(|s| s["hooks"][e.as_str()].as_array().is_some_and(|groups| has_group(groups, g)))
        })
}

/// Whether this run of a hook (`command`, as the snippet has it) is the
/// plugin's copy of one a settings file also has, and should do nothing:
/// Claude Code runs both (it dedupes only across settings files), and two
/// would put every question and permission prompt on two cards. The
/// plugin's hooks run with `CLAUDE_PLUGIN_ROOT` set to the plugin; the
/// settings' ones without it.
pub fn plugin_copy_stands_aside(command: &str, event: Option<&str>) -> bool {
    let Some(root) = std::env::var_os("CLAUDE_PLUGIN_ROOT") else { return false };
    if !is_our_plugin(Path::new(&root)) {
        return false;
    }
    let files: Vec<Value> = session_settings().iter().filter_map(|p| read_settings(p).ok()).collect();
    settings_have(&files, command, event)
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

/// What `merge` did.
#[derive(Debug, Default, PartialEq)]
struct Merged {
    /// Hooks that weren't there.
    added: bool,
    /// Permission rules that weren't there (#655).
    allowed: bool,
}

impl Merged {
    fn changed(&self) -> bool {
        self.added || self.allowed
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

/// The rules (the default tier: read-only and [`OWN_WRITE_RULES`]; and the
/// acting one with `allow_acting`) that `allow` doesn't have, added at the
/// end. Whether any were.
fn add_rules(allow: &mut Vec<Value>, allow_acting: bool, plugin: bool) -> bool {
    let acting = if allow_acting { ACTING_RULES } else { &[] };
    let by_default = READ_RULES.iter().chain(OWN_WRITE_RULES);
    let mut rules: Vec<String> = by_default.clone().chain(acting).map(|r| (*r).to_owned()).collect();
    if plugin {
        // The plugin's MCP tools go by another name (#229).
        let tools: Vec<String> = by_default
            .chain(acting)
            .filter_map(|r| r.strip_prefix("mcp__arugula__"))
            .map(|t| format!("{PLUGIN_TOOLS}{t}"))
            .collect();
        rules.extend(tools);
    }
    let mut added = false;
    for rule in rules {
        if !allow.iter().any(|a| a.as_str() == Some(&rule)) {
            allow.push(json!(rule));
            added = true;
        }
    }
    added
}

/// [`merge_for`] without the plugin.
#[cfg(test)]
fn merge(settings: &mut Value, allow_acting: bool) -> anyhow::Result<Merged> {
    merge_for(settings, allow_acting, false)
}

/// `settings` with the snippet's hooks added where missing (unless the plugin has them: `plugin`), and the permission
/// rules (read-only, and acting with `allow_acting`; with `plugin`, for the
/// plugin's tools too) added where missing: what changed.
fn merge_for(settings: &mut Value, allow_acting: bool, plugin: bool) -> anyhow::Result<Merged> {
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
    let mut changed = false;
    for (event, wanted) in want.into_iter().filter(|_| !plugin) {
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
    let allowed = add_rules(allow_list(root)?, allow_acting, plugin);
    Ok(Merged { added: changed, allowed })
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

#[cfg(test)]
fn install(path: &Path, dry_run: bool, allow_acting: bool) -> anyhow::Result<()> {
    install_for(path, dry_run, allow_acting, None, false)
}

/// What `install --json` says it did: Getting started's "Use Claude Code
/// with Arugula" runs it and says so in its own words.
fn said_json(path: &Path, merged: &Merged, plugin: Option<&str>) -> Value {
    json!({ "settings": path, "hooks_added": merged.added, "rules_added": merged.allowed, "plugin": plugin })
}

/// `install`, where the user's settings may enable the plugin (`plugin`, its
/// `enabledPlugins` key): then its hooks are the plugin's, and settings.json
/// gets only the permission rules. `json_out`: what changed, as JSON.
fn install_for(
    path: &Path,
    dry_run: bool,
    allow_acting: bool,
    plugin: Option<String>,
    json_out: bool,
) -> anyhow::Result<()> {
    let mut settings = read_settings(path)?;
    let plugin = plugin_enabled(&settings).or(plugin);
    let merged = merge_for(&mut settings, allow_acting, plugin.is_some())?;
    if json_out && !dry_run {
        if merged.changed() || !path.exists() {
            write_atomic(path, &pretty(&settings))?;
        }
        println!("{}", serde_json::to_string_pretty(&said_json(path, &merged, plugin.as_deref())).unwrap_or_default());
        return Ok(());
    }
    if let Some(p) = &plugin
        && !dry_run
    {
        println!("The Claude Code plugin ({p}) brings Arugula's hooks, so none are added to settings.json.");
    }
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
    if merged.added {
        said.push("hooks added");
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
    Missing,
}

/// Per event: whether every Arugula entry for it is in `settings`.
fn present(settings: &Value) -> Vec<(String, Has)> {
    let all = |event: &str, wanted: &Value| {
        let groups = settings["hooks"][event].as_array().map(Vec::as_slice).unwrap_or_default();
        wanted.as_array().into_iter().flatten().all(|w| has_group(groups, w))
    };
    snippet()
        .into_iter()
        .map(|(event, wanted)| {
            let has = if all(&event, &wanted) { Has::Present } else { Has::Missing };
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

/// The default tier (read-only, and [`OWN_WRITE_RULES`]) and the acting tier
/// of `settings`' `permissions.allow`.
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
    let default: Vec<&str> = READ_RULES.iter().chain(OWN_WRITE_RULES).copied().collect();
    (tier(&default), tier(ACTING_RULES))
}

/// A `.claude.json` with an MCP server named `arugula` that isn't the
/// plugin's own command (`arugula mcp`, which Claude Code counts as the same
/// server): beside the plugin, Claude Code runs both and lists every tool
/// twice.
fn manual_mcp_beside_plugin() -> Option<PathBuf> {
    let config_dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
    configs(&home().ok()?, config_dir).into_iter().map(|(_, c)| c).find(|c| {
        let Ok(mut v) = read_settings(c) else { return false };
        mcp_servers(&mut v)
            .iter()
            .filter_map(|s| s.get(PLUGIN))
            .any(|s| s["command"] != PLUGIN || s["args"] != json!(["mcp"]))
    })
}

/// `status`, where the user's settings may enable the plugin (`plugin`, its
/// `enabledPlugins` key).
fn status_for(path: &Path, json_out: bool, plugin: Option<String>) -> anyhow::Result<()> {
    let settings = read_settings(path)?;
    let plugin = plugin_enabled(&settings).or(plugin);
    let events = present(&settings);
    let (read, acting) = tiers(&settings);
    let installed = plugin.is_some() || events.iter().all(|(_, h)| *h != Has::Missing);
    if json_out {
        let map: Map<String, Value> =
            events.iter().map(|(e, h)| (e.clone(), Value::Bool(*h != Has::Missing))).collect();
        let permissions = json!({ "read": read.says(), "acting": acting.says() });
        let v = json!({
            "settings": path, "installed": installed, "events": map, "permissions": permissions,
            "plugin": plugin,
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return Ok(());
    }
    println!("{}", path.display());
    for (event, h) in &events {
        let says = match h {
            Has::Present => "present",
            Has::Missing if plugin.is_some() => "the plugin's",
            Has::Missing => "missing",
        };
        println!("  {event:<20} {says}");
    }
    if let Some(p) = &plugin {
        println!("Claude Code plugin");
        println!("  {p:<20} enabled: its hooks run where settings.json doesn't have the same one");
    }
    println!("permissions.allow");
    println!("  {:<20} {}", "read", read.says());
    println!("  {:<20} {}", "acting", acting.says());
    if !installed {
        println!("`arugula hooks install` adds the missing ones (or install the Claude Code plugin).");
    }
    if read != Tier::Present {
        println!("`arugula hooks install` adds the read-only permission rules.");
    }
    if acting != Tier::Present {
        println!(
            "`arugula hooks install --allow-acting` adds the acting ones (an agent can then start agents and type into panes without asking)."
        );
    }
    if plugin.is_some()
        && let Some(c) = manual_mcp_beside_plugin()
    {
        println!(
            "{} has an MCP server `arugula` of its own beside the plugin's: `claude mcp remove arugula` leaves one copy of each tool.",
            c.display()
        );
    }
    Ok(())
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

/// Claude Code's configs here: its settings.json and its `.claude.json`,
/// under `~` and under `CLAUDE_CONFIG_DIR` when that's set.
fn configs(home: &Path, config_dir: Option<PathBuf>) -> Vec<(PathBuf, PathBuf)> {
    let mut out = vec![(home.join(".claude/settings.json"), home.join(".claude.json"))];
    if let Some(d) = config_dir.filter(|d| d != &home.join(".claude")) {
        out.push((d.join("settings.json"), d.join(".claude.json")));
    }
    out
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
        assert_eq!(v["permissions"]["allow"].as_array().unwrap().len(), 1 + READ_RULES.len() + OWN_WRITE_RULES.len());
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre[0]["hooks"][0]["command"], "my-lint");
        assert_eq!(pre.len(), 3);
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop[0]["hooks"][0]["command"], "say done");
        assert_eq!(stop.len(), 3);
        assert!(present(&v).iter().all(|(_, p)| *p == Has::Present));
    }

    /// `install --json` writes the file and says what it added; nothing,
    /// the second time.
    #[test]
    fn install_json_says_what_changed() {
        let path = temp("json").join("settings.json");
        install_for(&path, false, false, None, true).unwrap();
        let mut v = read_settings(&path).unwrap();
        assert!(present(&v).iter().all(|(_, h)| *h == Has::Present));
        let m = merge(&mut v, false).unwrap();
        let said = said_json(&path, &m, None);
        assert_eq!(said, json!({ "settings": path, "hooks_added": false, "rules_added": false, "plugin": null }));
        let fresh = said_json(&path, &Merged { added: true, allowed: true }, Some("arugula@arugula"));
        assert_eq!((fresh["hooks_added"].clone(), fresh["plugin"].clone()), (json!(true), json!("arugula@arugula")));
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

    /// #1486: an install from before `SessionStart` ran `arugula hook` lacks
    /// it, `status` says so, and `install` adds it once.
    #[test]
    fn an_old_install_gets_the_session_start_hook_once() {
        let mut old = snippet();
        old.get_mut("SessionStart").unwrap().as_array_mut().unwrap().truncate(1);
        let old = json!({ "hooks": Value::Object(old) });
        let missing: Vec<_> =
            present(&old).into_iter().filter(|(_, p)| *p == Has::Missing).map(|(event, _)| event).collect();
        assert_eq!(missing, ["SessionStart"]);
        let (v, changed) = merged(&old.to_string());
        assert!(changed);
        let starts = v["hooks"]["SessionStart"].as_array().unwrap();
        let hooks = |c: &str| starts.iter().filter(|g| g["hooks"][0]["command"] == c).count();
        assert_eq!((hooks("arugula inbox"), hooks("arugula hook")), (1, 1), "{starts:?}");
        assert_eq!(starts.len(), 2);
        let (again, changed) = merged(&v.to_string());
        assert!(!changed);
        assert_eq!(again, v);
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

    /// #879: with another config dir in effect, install and status use its
    /// settings.json, not `~/.claude`'s.
    #[test]
    fn user_settings_follow_claude_config_dir() {
        let home = Path::new("/home/me");
        let rig = PathBuf::from("/tmp/rig/claude");
        assert_eq!(user_settings_in(home, Some(rig.clone())), rig.join("settings.json"));
        assert_eq!(user_settings_in(home, None), home.join(".claude/settings.json"));
        assert_eq!(user_settings_in(home, Some(PathBuf::new())), home.join(".claude/settings.json"));
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
            "Bash(arugula runs list:*)",
            "Bash(arugula runs ls:*)",
            "Bash(arugula runs show:*)",
            "Bash(arugula runs logs:*)",
            "mcp__arugula__read_output",
            "mcp__arugula__wait",
            "mcp__arugula__list",
            "mcp__arugula__capabilities",
            "mcp__arugula__history",
            "mcp__arugula__read_forge",
            "mcp__arugula__forge_read",
            "mcp__arugula__read_invite",
            "mcp__arugula__read_file",
            "mcp__arugula__read_cloud",
            "mcp__arugula__read_thread",
            "mcp__arugula__search_threads",
            "mcp__arugula__read_run",
            "mcp__arugula__read_deploy",
            "mcp__arugula__read_session",
        ];
        assert_eq!(READ_RULES, read);
        let acting_cli = [
            "agent", "run", "send", "keys", "close", "call", "open", "edit", "view", "diff", "pr", "issue", "draft",
            "cd", "rerun", "upload", "mouse", "share", "invite",
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
        // ties them to the tools. `delegate` is the one it leaves out (a
        // flag's); `doc`, a write it asks for, an install allows (#963).
        let snippet: Value = serde_json::from_str(include_str!("../../../docs/mcp-permissions.json")).unwrap();
        let list = |key: &str| -> Vec<String> {
            snippet["permissions"][key].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_owned()).collect()
        };
        let mcp = |rules: &[&str]| -> Vec<String> {
            rules.iter().filter(|r| r.starts_with("mcp__")).map(|r| (*r).to_owned()).collect()
        };
        let (read, acting) = (mcp(READ_RULES), mcp(ACTING_RULES));
        let flagged = ["mcp__arugula__delegate"];
        let without =
            |v: &[String]| -> Vec<String> { v.iter().filter(|r| !flagged.contains(&r.as_str())).cloned().collect() };
        assert_eq!(without(&read), list("allow"));
        let mut a = without(&acting);
        a.extend(mcp(OWN_WRITE_RULES));
        let mut want = list("ask");
        a.sort();
        want.sort();
        assert_eq!(a, want);
    }

    #[test]
    fn install_adds_the_read_tier_only_by_default() {
        let (v, changed) = merged("{}");
        assert!(changed);
        // The read tier, and the one write that is a Doc's own (#963).
        let want: Vec<&str> = READ_RULES.iter().chain(OWN_WRITE_RULES).copied().collect();
        assert_eq!(allowed(&v), want);
        assert!(allowed(&v).iter().all(|r| !ACTING_RULES.contains(&r.as_str())));
        assert_eq!(tiers(&v), (Tier::Present, Tier::Missing));
    }

    #[test]
    fn the_default_install_allows_doc_in_both_spellings_and_dry_run_shows_it() {
        let (v, _) = merged("{}");
        assert!(allowed(&v).contains(&"mcp__arugula__doc".to_owned()));
        assert!(!allowed(&v).iter().any(|r| r.contains("plugin")), "the plugin's spelling is for the plugin");
        // With the plugin enabled: its spelling too, still without --allow-acting.
        let mut v = json!({});
        merge_for(&mut v, false, true).unwrap();
        assert!(allowed(&v).contains(&"mcp__arugula__doc".to_owned()));
        assert!(allowed(&v).contains(&format!("{PLUGIN_TOOLS}doc")));
        // --dry-run prints the settings it would write.
        assert!(pretty(&v).contains("mcp__plugin_arugula_arugula__doc"));
        assert!(!allowed(&v).iter().any(|r| r == &format!("{PLUGIN_TOOLS}run")), "acting only with the flag");
    }

    #[test]
    fn an_install_from_before_docs_is_partial_until_it_is_run_again() {
        // Every read rule, and not the Doc's: the default tier is partial,
        // so `hooks status` and capabilities say to install again.
        let mut v = json!({ "permissions": { "allow": READ_RULES } });
        assert_eq!(tiers(&v), (Tier::Partial, Tier::Missing));
        assert!(merge(&mut v, false).unwrap().allowed);
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
        for rule in READ_RULES.iter().chain(OWN_WRITE_RULES).chain(ACTING_RULES) {
            assert_eq!(rules.iter().filter(|r| *r == rule).count(), 1, "{rule}");
        }
        assert_eq!(allow.len(), 4 + READ_RULES.len() + OWN_WRITE_RULES.len() + ACTING_RULES.len() - 1);
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

    const PLUGIN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins/claude-code");

    fn plugin_file(path: &str) -> Value {
        let text = fs::read_to_string(format!("{PLUGIN_DIR}/{path}")).unwrap_or_else(|e| panic!("{path}: {e}"));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// #229: the plugin's hooks are exactly what `install` writes, plus its
    /// one check for a missing CLI, so the two can't drift. A change to
    /// HOOKS_SNIPPET goes in plugins/claude-code/hooks/hooks.json too.
    #[test]
    fn plugin_hooks_are_the_snippet() {
        let plugin = plugin_file("hooks/hooks.json");
        let mut hooks = plugin["hooks"].as_object().unwrap().clone();
        // Only the plugin's own: the SessionStart check that `arugula` is on PATH.
        let mut own = vec![];
        for (event, groups) in hooks.iter_mut() {
            groups.as_array_mut().unwrap().retain(|g| {
                let ours = g["hooks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|h| h["command"].as_str().unwrap().starts_with("arugula "));
                if !ours {
                    own.push((event.clone(), g.clone()));
                }
                ours
            });
        }
        assert_eq!(
            Value::Object(hooks),
            Value::Object(snippet()),
            "plugins/claude-code/hooks/hooks.json's hooks: HOOKS_SNIPPET's, as `arugula hooks install` writes them"
        );
        assert_eq!(own.len(), 1, "{own:?}");
        let (event, check) = &own[0];
        assert_eq!(event, "SessionStart");
        let command = check["hooks"][0]["command"].as_str().unwrap();
        assert!(command.starts_with("command -v arugula >/dev/null 2>&1 || echo '{"), "{command}");
        let said: Value = serde_json::from_str(command.split_once("echo '").unwrap().1.trim_end_matches('\'')).unwrap();
        assert!(said["systemMessage"].as_str().unwrap().contains("not on PATH"), "{said}");

        // The manifest, the MCP server and the marketplace's entry name it
        // as PLUGIN and PLUGIN_TOOLS do.
        assert_eq!(plugin_file(".claude-plugin/plugin.json")["name"], PLUGIN);
        let servers = plugin_file(".mcp.json")["mcpServers"].as_object().unwrap().clone();
        assert_eq!(servers.len(), 1);
        let (name, server) = servers.iter().next().unwrap();
        assert_eq!(PLUGIN_TOOLS, format!("mcp__plugin_{PLUGIN}_{name}__"));
        assert_eq!((&server["command"], &server["args"]), (&json!("arugula"), &json!(["mcp"])));
        let market: Value = serde_json::from_str(
            &fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.claude-plugin/marketplace.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(market["plugins"][0]["name"], PLUGIN);
        assert_eq!(market["plugins"][0]["source"], "./plugins/claude-code");
        assert!(Path::new(PLUGIN_DIR).join("skills/arugula/SKILL.md").is_file());
    }

    #[test]
    fn plugin_copy_stands_aside_only_for_a_hook_settings_has() {
        let (full, _) = merged(MINE);
        let mine: Value = serde_json::from_str(MINE).unwrap();
        let stop_only: Value = serde_json::from_str(
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"arugula attention done"}]}]}}"#,
        )
        .unwrap();
        // Installed: every copy of the plugin's stands aside.
        for (event, groups) in snippet() {
            for g in groups.as_array().unwrap() {
                let command = g["hooks"][0]["command"].as_str().unwrap();
                assert!(settings_have(&[mine.clone(), full.clone()], command, Some(&event)), "{event} {command}");
            }
        }
        // None of ours there: the plugin's run.
        assert!(!settings_have(std::slice::from_ref(&mine), "arugula hook", Some("PermissionRequest")));
        assert!(!settings_have(&[], "arugula ask", Some("PreToolUse")));
        // Only Stop's attention: that one stands aside, the rest run.
        assert!(settings_have(std::slice::from_ref(&stop_only), "arugula attention done", None));
        assert!(!settings_have(std::slice::from_ref(&stop_only), "arugula attention needs-input", None));
        assert!(!settings_have(std::slice::from_ref(&stop_only), "arugula inbox", Some("Stop")));
        assert!(!settings_have(&[stop_only], "arugula hook", Some("PreToolUse")));
        // `arugula ask` on PreToolUse is the AskUserQuestion group's, not a bare one.
        let bare_ask: Value = serde_json::from_str(
            r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"arugula ask"}]}]}}"#,
        )
        .unwrap();
        assert!(!settings_have(&[bare_ask], "arugula ask", Some("PreToolUse")));
    }

    #[test]
    fn plugin_is_seen_in_enabled_plugins() {
        let on = json!({ "enabledPlugins": { "other@x": true, "arugula@arugula": true } });
        assert_eq!(plugin_enabled(&on).as_deref(), Some("arugula@arugula"));
        for off in [
            json!({ "enabledPlugins": { "arugula@arugula": false } }),
            json!({ "enabledPlugins": { "arugula-ish@x": true } }),
            json!({}),
        ] {
            assert_eq!(plugin_enabled(&off), None, "{off}");
        }
        assert!(is_our_plugin(Path::new(PLUGIN_DIR)));
        assert!(!is_our_plugin(&std::env::temp_dir()));
    }

    #[test]
    fn with_the_plugin_install_adds_rules_not_hooks() {
        let mut v: Value = serde_json::from_str(MINE).unwrap();
        let m = merge_for(&mut v, false, true).unwrap();
        assert_eq!(m, Merged { added: false, allowed: true });
        let mine: Value = serde_json::from_str(MINE).unwrap();
        assert_eq!(v["hooks"], mine["hooks"]);
        let allow = allowed(&v);
        assert!(allow.contains(&"mcp__arugula__wait".to_owned()));
        assert!(allow.contains(&"mcp__plugin_arugula_arugula__wait".to_owned()));
        assert!(!allow.iter().any(|r| r == "mcp__plugin_arugula_arugula__run"), "acting only with the flag");
        assert_eq!(tiers(&v), (Tier::Present, Tier::Missing));
        // Again: nothing.
        assert_eq!(merge_for(&mut v, false, true).unwrap(), Merged::default());

        // Settings that enable it are enough, on disk.
        let dir = temp("plugin");
        let path = dir.join(".claude/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"enabledPlugins":{"arugula@arugula":true}}"#).unwrap();
        install(&path, false, false).unwrap();
        let s = read_settings(&path).unwrap();
        assert!(s.get("hooks").is_none_or(|h| h.as_object().is_some_and(Map::is_empty)), "{s}");
        assert!(allowed(&s).contains(&"mcp__plugin_arugula_arugula__list".to_owned()));
        let _ = fs::remove_dir_all(&dir);
    }
}
