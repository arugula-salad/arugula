//! M76: wear a recipe in a Claude Code agent block on this host, the way
//! M44 wears a Fountain agent (`fountain/wear.rs`, whose server handling
//! this reuses whole). The block gets:
//!
//! - **the prompt**, appended to Claude Code's after a line naming the
//!   agent (`claude-agent-acp` drops the SDK's `agent` option, so the
//!   subagent can't be selected directly: S34 q1);
//! - **its model, `tools` and `disallowedTools`**, and its `permissionMode`
//!   unless the block asks for another;
//! - **its skills**, by name from the project's `.claude/skills/` then the
//!   user's: each one's `SKILL.md` preloaded after the prompt, as Claude
//!   Code does for a subagent (its `tools` may leave out the Skill tool),
//!   and the whole skill in a local plugin under
//!   `~/.cache/arugula/recipes/<name>/v…/` for its other files (a new one
//!   each time it's worn; old ones go after a week);
//! - **its MCP servers**: one written into the recipe, or one it names from
//!   the project's local config (`~/.claude.json`'s `projects`), its
//!   `.mcp.json`, then the user's `~/.claude.json`, as Claude Code looks
//!   them up. `${VAR}`s come from the user's shell environment (and `gh
//!   auth token`); every credential goes by reference, never on a command
//!   line ([`super::super::fountain::wear::servers`]). Those and Arugula's
//!   own are all it gets (`--strict-mcp-config`): not the account's
//!   claude.ai connectors, which an agent offered to other people mustn't
//!   carry.
//!
//! What can't come along (a skill or server it can't find, a variable that
//! isn't set, a server that wants an OAuth sign-in) is named on the block,
//! with why.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde_json::{Value, json};
use tracing::info;

use super::{Recipe, RecipeServer};
use crate::{
    labs::fountain::{
        api::{Agent, McpServer},
        wear::{self as fw, Info, LeftOut, Lookup, Worn},
    },
    review::Runner,
    store::now_ms,
};

/// The prompt: a line naming the agent, its body, then each skill it
/// preloads.
fn system(r: &Recipe, skills: &[(String, String)]) -> String {
    let mut s = format!("You are running as the agent \"{}\".\n\n{}", r.name, r.prompt);
    for (name, text) in skills {
        s.push_str(&format!("\n\n---\n\nThe skill \"{name}\" (preloaded):\n\n{}", text.trim()));
    }
    s
}

/// MCP servers configured for a project at `dir`, by name, as Claude Code
/// would find them: local scope, then the project's `.mcp.json`, then the
/// user's.
fn configured(dir: &Path, home: &Path) -> BTreeMap<String, Value> {
    let read = |p: PathBuf| -> Value {
        std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
    };
    let user = read(home.join(".claude.json"));
    let project = read(dir.join(".mcp.json"));
    let local = &user["projects"][dir.display().to_string()];
    let mut out = BTreeMap::new();
    for scope in [&user["mcpServers"], &project["mcpServers"], &local["mcpServers"]] {
        for (k, v) in scope.as_object().into_iter().flatten() {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// The recipe's servers as Fountain's shape, and the ones it names that
/// aren't configured anywhere.
fn servers_of(r: &Recipe, dir: &Path, home: &Path) -> (BTreeMap<String, McpServer>, Vec<LeftOut>) {
    let known = configured(dir, home);
    let mut out = BTreeMap::new();
    let mut left = vec![];
    for s in &r.mcp_servers {
        let (name, config) = match s {
            RecipeServer::Inline { name, config } => (name, Some(config.clone())),
            RecipeServer::Named(name) => (name, known.get(name).cloned()),
        };
        let Some(config) = config else {
            left.push(LeftOut {
                name: name.clone(),
                why: "it isn't configured for this project or user (.mcp.json, ~/.claude.json)".into(),
            });
            continue;
        };
        match serde_json::from_value::<McpServer>(config) {
            Ok(m) => {
                out.insert(name.clone(), m);
            }
            Err(e) => left.push(LeftOut { name: name.clone(), why: format!("its config doesn't parse: {e}") }),
        }
    }
    (out, left)
}

/// The recipe's skills as a local plugin under `cache`: those found, and
/// the names that weren't.
#[allow(clippy::type_complexity)]
fn bundle(
    r: &Recipe,
    dir: &Path,
    home: &Path,
    cache: &Path,
) -> Result<(PathBuf, String, Vec<(String, String)>, Vec<String>), String> {
    let parent = cache.join(fw::component(&r.name));
    let version = format!("v{}-{}", now_ms(), std::process::id());
    let plugin = parent.join(&version).join("plugin");
    let skills_dir = plugin.join("skills");
    std::fs::create_dir_all(&skills_dir).map_err(|e| format!("can't make {}: {e}", skills_dir.display()))?;
    std::fs::create_dir_all(plugin.join(".claude-plugin")).map_err(|e| e.to_string())?;
    let plugin_name = format!("recipe-{}", fw::slug(&r.name));
    let manifest = json!({
        "name": plugin_name,
        "description": format!("Skills of the agent {}, worn by Arugula", r.name),
        "version": "0.0.0",
    });
    std::fs::write(plugin.join(".claude-plugin/plugin.json"), serde_json::to_vec_pretty(&manifest).unwrap_or_default())
        .map_err(|e| e.to_string())?;
    let (mut got, mut missing) = (vec![], vec![]);
    for name in &r.skills {
        let safe = fw::component(name);
        let from = [dir.join(".claude/skills").join(&safe), home.join(".claude/skills").join(&safe)]
            .into_iter()
            .find(|d| d.join("SKILL.md").is_file());
        match from {
            Some(d) if !got.iter().any(|(n, _)| *n == safe) => {
                fw::copy_dir(&d, &skills_dir.join(&safe)).map_err(|e| format!("copying the skill {name}: {e}"))?;
                let text = std::fs::read_to_string(d.join("SKILL.md")).map_err(|e| format!("the skill {name}: {e}"))?;
                got.push((safe, text));
            }
            Some(_) => {}
            None => missing.push(format!("{name} (not in .claude/skills here or in ~/.claude/skills)")),
        }
    }
    // Older versions go once nothing could still be on them.
    for e in std::fs::read_dir(&parent).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n != version && n.starts_with('v') && fw::age(&e.path()).is_some_and(|a| a > fw::OLD) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    Ok((plugin, plugin_name, got, missing))
}

/// Wear the recipe `name` for a block working in `cwd` on this host.
pub async fn wear(runner: &Runner, cwd: &Path, name: &str) -> Result<Worn, String> {
    let Runner::Local { env, home } = runner else {
        return Err("a recipe runs on this host, not on a machine".into());
    };
    let r = super::find(cwd, home, name)?;
    let cache = fw::cache_root(env, home).with_file_name("recipes");
    let (plugin, plugin_name, skills, skills_missing) = bundle(&r, cwd, home, &cache)?;
    let (mcp_servers, mut left) = servers_of(&r, cwd, home);
    let as_agent = Agent { name: r.name.clone(), mcp_servers, ..Default::default() };
    let mut names = BTreeSet::new();
    for s in as_agent.mcp_servers.values() {
        fw::refs_in(&serde_json::to_value(s).unwrap_or_default(), &mut names);
    }
    let lookup = Lookup { env, home, specs: None, environment: None, vault: None };
    let found = fw::resolve(&names, &lookup).await;
    let served = fw::servers(&as_agent, &found, true).await;
    left.extend(served.left);
    left.sort_by(|a, b| a.name.cmp(&b.name));
    info!(
        agent = r.name,
        skills = skills.len(),
        servers = served.infos.len(),
        left_out = left.len(),
        "wearing a recipe"
    );
    Ok(Worn {
        info: Info {
            agent: r.name.clone(),
            id: r.path.display().to_string(),
            model: r.model.clone(),
            plugin: plugin_name,
            bundle: plugin.parent().map(|p| p.display().to_string()).unwrap_or_default(),
            skills: skills.iter().map(|(n, _)| n.clone()).collect(),
            skills_missing,
            servers: served.infos,
            left_out: left,
            recipe: Some(r.path.display().to_string()),
        },
        system: system(&r, &skills),
        plugin,
        servers: served.list,
        env: served.env,
        secrets: served.secrets,
        tools: r.tools.clone(),
        disallowed_tools: r.disallowed_tools.clone(),
        permission_mode: r.permission_mode.clone(),
        strict_mcp: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_recipe_brings_its_skills_and_servers_or_says_why_not() {
        let t = std::env::temp_dir().join(format!("arugula-wear-recipe-{}-{}", std::process::id(), now_ms()));
        let (proj, home) = (t.join("p"), t.join("h"));
        std::fs::create_dir_all(proj.join(".claude/agents")).unwrap();
        std::fs::create_dir_all(proj.join(".claude/skills/testing")).unwrap();
        std::fs::write(proj.join(".claude/skills/testing/SKILL.md"), "---\nname: testing\n---\nRun the tests.")
            .unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            proj.join(".claude/agents/fixer.md"),
            "---\nname: fixer\ntools: [Read, Edit]\npermissionMode: acceptEdits\nmodel: haiku\nskills: [testing, gone]\nmcpServers:\n  - docs\n  - nowhere\n  - inline:\n      type: http\n      url: https://example.invalid/mcp\n      headers:\n        Authorization: Bearer ${ARUGULA_TEST_RECIPE_TOKEN}\n---\nFix tests.\n",
        )
        .unwrap();
        std::fs::write(
            proj.join(".mcp.json"),
            r#"{"mcpServers":{"docs":{"command":"docs-mcp","args":["--stdio"],"env":{"KEY":"${ARUGULA_TEST_RECIPE_UNSET}"}}}}"#,
        )
        .unwrap();
        let env = vec![
            ("XDG_CACHE_HOME".to_owned(), t.join("cache").display().to_string()),
            ("ARUGULA_TEST_RECIPE_TOKEN".to_owned(), "s3cr3t-token".to_owned()),
        ];
        let runner = Runner::Local { env, home: home.clone() };
        let w = wear(&runner, &proj, "fixer").await.unwrap();
        assert_eq!(w.info.skills, ["testing"]);
        assert!(w.plugin.join("skills/testing/SKILL.md").is_file());
        assert_eq!(w.info.skills_missing.len(), 1, "{:?}", w.info.skills_missing);
        let left: Vec<&str> = w.info.left_out.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(left, ["docs", "nowhere"], "{:?}", w.info.left_out);
        assert_eq!(w.info.servers.len(), 1);
        // The credential goes by reference, its value in the env only.
        let s = serde_json::to_string(&w.servers).unwrap();
        assert!(!s.contains("s3cr3t-token"), "{s}");
        assert!(w.env.iter().any(|(_, v)| v.contains("s3cr3t-token")));
        assert!(w.secrets.iter().any(|v| v.contains("s3cr3t-token")));
        let mut meta = json!({});
        w.dress(&mut meta);
        assert_eq!(meta["claudeCode"]["options"]["tools"], json!(["Read", "Edit"]));
        assert_eq!(meta["claudeCode"]["options"]["model"], "haiku");
        let system = meta["systemPrompt"]["append"].as_str().unwrap();
        assert!(system.starts_with("You are running as the agent \"fixer\""), "{system}");
        assert!(system.contains("The skill \"testing\" (preloaded)") && system.ends_with("Run the tests."), "{system}");
        assert_eq!(meta["claudeCode"]["options"]["extraArgs"], json!({ "strict-mcp-config": "" }));
        assert_eq!(w.permission_mode(), Some("acceptEdits"));
        assert!(w.text().contains("As the agent fixer"));
        let _ = std::fs::remove_dir_all(&t);
    }
}
