//! Recipes on Managoat (#942, F4): an agent recipe (a Claude Code subagent
//! file, [`crate::labs::agents`]) run in a Managoat sandbox, beside running
//! it here (M76) and on another of the account's machines.
//!
//! **One mutable Managoat agent per recipe** (Jake, 2026-10-15): before each
//! launch the recipe is compiled ([`compile`]) and upserted with `POST
//! /api/apply` as `recipe-<name>`, with `metadata: {managed-by: arugula,
//! source_path, recipe_hash}`. An unchanged recipe reports `unchanged` and
//! writes nothing. Editing the recipe changes that agent, so an older
//! conversation gets the new system prompt on its next wake; the
//! conversation's `arugula.recipe_hash` label says which version started
//! it. An agent of that name that Arugula didn't make (chant's, one made by
//! hand) is never written: the run is refused.
//!
//! **What carries over, and what doesn't** (each loss is named on the run,
//! as M76 names what it left out):
//! - the body is `system` (Managoat writes it to `~/.claude/CLAUDE.md`);
//! - `model`: `sonnet`, `opus` and `haiku` become a canonical
//!   `anthropic/…` id, a `claude-…` id gets the prefix; `inherit` leaves the
//!   agent's alone (a new agent needs one: [`DEFAULT_MODEL`]);
//! - `skills` are inline `{name, content}`: `SKILL.md` only, so a skill's
//!   other files stay here;
//! - `mcpServers` keep their `${VAR}`s as they are: Managoat substitutes
//!   them from the environment's and the vault's secrets at spawn. A server
//!   with a value typed in (not a reference) is left out, so no credential
//!   leaves this machine without the explicit copy below; so is one that
//!   wants an OAuth sign-in, or whose command is a path on this machine;
//! - `tools`, `disallowedTools` and `permissionMode` become a coarse
//!   `permission_policy` over ACP's kinds: no `Bash` denies `execute`, no
//!   `Edit`/`Write` denies `edit` (and `delete`, `move`), no
//!   `WebFetch`/`WebSearch` denies `fetch`. Managoat has no tool allowlist.
//!
//! **The working directory.** A sandbox clones its own checkout, so the
//! project's `origin` (as https) becomes an environment `repo-<owner>-<name>`
//! with `repositories: [{url, mount_path: /work/<name>}]`, applied with the
//! agent. Uncommitted changes and unpushed commits don't travel, and the
//! run says so. An environment of that name Arugula didn't make is used as
//! it is.
//!
//! **Secrets: a preflight, and an explicit copy** (Q6). [`preflight`]
//! compares the `${VAR}`s the agent uses with the keys (never values) of the
//! environment and vault the run will have, and names what's missing.
//! [`copy`] is one variable, on a person's say-so: resolved on this machine
//! as wearing resolves it (Infisical through agent-specs, the shell, `gh`)
//! and written once into the vault they chose. Nothing is ever copied
//! automatically, and no value is logged, shown or kept.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tracing::info;

use super::client::{Client, Holder, Named, Resource};
use crate::labs::{
    agents::{self as recipes, Recipe},
    wearing::{self as fw, LeftOut, Lookup},
};

/// The metadata key (and labels) that mark what Arugula made.
pub const MANAGED_BY: &str = "managed-by";
pub const ARUGULA: &str = "arugula";
pub const LABEL_RECIPE: &str = "arugula.recipe";
pub const LABEL_HASH: &str = "arugula.recipe_hash";
pub const LABEL_MACHINE: &str = "arugula.machine";

/// A new agent's model when the recipe inherits one: Managoat needs a model
/// on a claude agent.
pub const DEFAULT_MODEL: &str = "anthropic/claude-sonnet-5-5";

/// Where an environment's repository is cloned in the sandbox.
pub const WORK: &str = "/work";

/// The Managoat agent a recipe becomes.
pub fn agent_name(recipe: &str) -> String {
    format!("recipe-{recipe}")
}

/// A recipe's `model` as Managoat's canonical `provider/model`, or none
/// when it has no such form.
pub fn model(m: &str) -> Option<String> {
    let m = m.trim();
    let id = match m {
        "sonnet" => "anthropic/claude-sonnet-5-5".to_owned(),
        "opus" => "anthropic/claude-opus-5-5".to_owned(),
        "haiku" => "anthropic/claude-haiku-5-5".to_owned(),
        m if m.starts_with("anthropic/") => m.to_owned(),
        m if m.starts_with("claude-") => format!("anthropic/{m}"),
        _ => return None,
    };
    // Managoat's own check: `^[a-z0-9_-]+/[a-z0-9._-]+$`.
    let ok = |s: &str, extra: &str| {
        !s.is_empty()
            && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-".contains(c) || extra.contains(c))
    };
    let (p, rest) = id.split_once('/')?;
    (ok(p, "") && ok(rest, ".")).then_some(id)
}

// ---------------------------------------------------------------- policy

/// Claude Code's tools, by the ACP kinds a permission policy can name.
const CLASSES: &[(&[&str], &[&str], &str)] = &[
    (&["Bash"], &["execute"], "Bash"),
    (&["Edit", "Write", "MultiEdit", "NotebookEdit"], &["edit", "delete", "move"], "Edit or Write"),
    (&["WebFetch", "WebSearch"], &["fetch"], "WebFetch or WebSearch"),
];

/// A tool's name without its rule (`Bash(git:*)` is `Bash`).
fn bare(t: &str) -> &str {
    t.split('(').next().unwrap_or(t).trim()
}

/// The recipe's tool lists and permission mode as a permission policy, and
/// what that loses.
pub fn policy(r: &Recipe) -> (BTreeMap<String, String>, Vec<String>) {
    let mut p: BTreeMap<String, String> = BTreeMap::new();
    let mut notes = vec![];
    fn set(p: &mut BTreeMap<String, String>, k: &str, v: &str) {
        p.insert(k.to_owned(), v.to_owned());
    }
    let default = match r.permission_mode.as_deref() {
        None | Some("default") => "ask",
        Some("acceptEdits") => {
            for k in ["edit", "delete", "move"] {
                set(&mut p, k, "auto_allow");
            }
            "ask"
        }
        Some("bypassPermissions") => "auto_allow",
        Some("plan") => {
            notes.push(
                "permissionMode plan: Managoat has no plan mode, so edits and commands are denied (by ACP kind) instead"
                    .to_owned(),
            );
            for k in ["edit", "delete", "move", "execute"] {
                set(&mut p, k, "auto_deny");
            }
            "ask"
        }
        Some("dontAsk") => {
            notes.push("permissionMode dontAsk: anything but reading and searching is denied".to_owned());
            for k in ["read", "search", "think"] {
                set(&mut p, k, "auto_allow");
            }
            "auto_deny"
        }
        Some(m) => {
            notes.push(format!("permissionMode {m}: Managoat has no such mode, so it asks"));
            "ask"
        }
    };
    set(&mut p, "default", default);
    let allowed = |t: &str| {
        (r.tools.is_empty() || r.tools.iter().any(|x| bare(x) == t))
            && !r.disallowed_tools.iter().any(|x| x.trim() == t)
    };
    let mut denied = vec![];
    for (tools, kinds, said) in CLASSES {
        if !tools.iter().any(|t| allowed(t)) {
            for k in *kinds {
                set(&mut p, k, "auto_deny");
            }
            denied.push(format!("no {said}: {} denied", kinds.join(", ")));
        }
    }
    if !r.tools.is_empty() || !r.disallowed_tools.is_empty() {
        let kept = if denied.is_empty() { "nothing denied".to_owned() } else { denied.join("; ") };
        notes.push(format!(
            "tools: Managoat has no tool allowlist, so only what ACP's kinds can say carries over ({kept})"
        ));
    }
    let scoped: Vec<&String> = r.tools.iter().chain(&r.disallowed_tools).filter(|t| t.contains('(')).collect();
    if !scoped.is_empty() {
        let list: Vec<&str> = scoped.iter().map(|s| s.as_str()).collect();
        notes.push(format!("{}: a tool's rules don't carry over, only the tool's kind", list.join(", ")));
    }
    (p, notes)
}

/// A policy as `--permission`'s grammar (the block's `permission`), which
/// the launch sends: the agent's own, so it never widens it.
pub fn permission_text(p: &BTreeMap<String, String>) -> String {
    let mut parts = vec![format!("default={}", p.get("default").map_or("ask", String::as_str))];
    parts.extend(p.iter().filter(|(k, _)| *k != "default").map(|(k, v)| format!("{k}={v}")));
    parts.join(",")
}

// ---------------------------------------------------------------- the repo

/// The project's checkout as an environment Managoat clones.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Repo {
    /// `origin`, as https (no credentials).
    pub url: String,
    /// The environment: `repo-<owner>-<name>`.
    pub environment: String,
    pub mount: String,
    /// The secret a private repository clones with (GitHub's token).
    pub secret_key: Option<String>,
    /// What's here that won't be there.
    pub stays: Vec<String>,
}

/// `origin`'s URL as https, and its owner and name. None for one that
/// isn't https or ssh (a path).
pub fn https(remote: &str) -> Option<(String, String, String)> {
    let r = remote.trim();
    let (host, path) = if let Some(rest) = r.strip_prefix("https://") {
        let (auth, path) = rest.split_once('/')?;
        // Credentials in the URL never leave.
        (auth.rsplit('@').next()?.to_owned(), path.to_owned())
    } else if let Some(rest) = r.strip_prefix("ssh://") {
        let (auth, path) = rest.split_once('/')?;
        let host = auth.rsplit('@').next()?;
        (host.split(':').next()?.to_owned(), path.to_owned())
    } else if !r.contains("://") && r.contains(':') && !r.starts_with('/') {
        // scp-like: `git@host:owner/name.git`.
        let (auth, path) = r.split_once(':')?;
        (auth.rsplit('@').next()?.to_owned(), path.to_owned())
    } else {
        return None;
    };
    let path = path.trim_matches('/').trim_end_matches(".git").to_owned();
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let (name, owner) = segs.split_last()?;
    if host.is_empty() || owner.is_empty() || host.contains(char::is_whitespace) {
        return None;
    }
    Some((format!("https://{host}/{path}.git"), owner.join("-"), (*name).to_owned()))
}

/// The project at `dir` as a [`Repo`], or why it has none.
pub async fn repo(dir: &Path) -> Result<Repo, String> {
    let git = |args: &[&str]| {
        let mut argv = vec!["git".to_owned(), "-C".to_owned(), dir.display().to_string()];
        argv.extend(args.iter().map(|a| (*a).to_owned()));
        async move { fw::run(&argv, None, &[]).await }
    };
    let remote = git(&["remote", "get-url", "origin"])
        .await
        .map_err(|_| format!("{} has no origin, so the sandbox has no checkout of it", dir.display()))?;
    let (url, owner, name) = https(&remote)
        .ok_or_else(|| format!("its origin ({}) isn't a URL Managoat can clone (https or ssh)", remote.trim()))?;
    let mut stays = vec![];
    if git(&["status", "--porcelain"]).await.is_ok_and(|s| !s.trim().is_empty()) {
        stays.push("uncommitted changes".to_owned());
    }
    match git(&["rev-list", "--count", "@{u}..HEAD"]).await {
        Ok(n) if n.trim() != "0" => stays.push(format!("{} unpushed commit(s)", n.trim())),
        Ok(_) => {}
        Err(_) => stays.push("this branch, which isn't on origin".to_owned()),
    }
    let github = url.starts_with("https://github.com/");
    Ok(Repo {
        environment: format!("repo-{}-{}", fw::slug(&owner), fw::slug(&name)),
        mount: format!("{WORK}/{}", fw::component(&name)),
        secret_key: github.then(|| "GITHUB_TOKEN".to_owned()),
        url,
        stays,
    })
}

// ---------------------------------------------------------------- compiling

/// A recipe as a Managoat agent (and its project as an environment).
#[derive(Debug, Clone, Serialize)]
pub struct Compiled {
    pub recipe: String,
    /// `recipe-<name>`.
    pub agent: String,
    pub source_path: String,
    /// The agent's spec, its metadata included.
    pub spec: Value,
    /// The project's environment's, when it has one.
    pub environment_spec: Option<Value>,
    /// Of both specs, without the metadata: what the run's label carries.
    pub hash: String,
    /// The recipe's model, canonical; none: it inherits.
    pub model: Option<String>,
    /// The launch's permission policy, as the block takes it.
    pub permission: String,
    /// Every `${VAR}` the agent and its checkout use.
    pub vars: BTreeSet<String>,
    pub repo: Option<Repo>,
    /// Why there's no checkout, when there isn't.
    pub no_repo: Option<String>,
    /// What didn't come along, and why.
    pub left_out: Vec<LeftOut>,
    /// What came along changed.
    pub notes: Vec<String>,
}

/// Every file under `dir` but its `SKILL.md`, relative.
fn other_files(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, d: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() && e.file_name() != ".git" {
                walk(root, &p, out);
            } else if t.is_file() {
                // `/` on Windows too: these are paths inside the skill.
                let rel = p.strip_prefix(root).unwrap_or(&p);
                let rel = rel.iter().map(|c| c.to_string_lossy()).collect::<Vec<_>>().join("/");
                if rel != "SKILL.md" {
                    out.push(rel);
                }
            }
        }
    }
    let mut out = vec![];
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// The recipe's skills as inline `{name, content}`, and what stays here.
fn skills(r: &Recipe, dir: &Path, home: &Path) -> (Vec<Value>, Vec<LeftOut>) {
    let (mut out, mut left) = (vec![], vec![]);
    let mut seen = BTreeSet::new();
    for name in &r.skills {
        let safe = fw::component(name);
        if !seen.insert(safe.clone()) {
            continue;
        }
        let from = [dir.join(".claude/skills").join(&safe), home.join(".claude/skills").join(&safe)]
            .into_iter()
            .find(|d| d.join("SKILL.md").is_file());
        let Some(d) = from else {
            left.push(LeftOut {
                name: format!("skill {name}"),
                why: "it isn't in .claude/skills here or in ~/.claude/skills".into(),
            });
            continue;
        };
        match std::fs::read_to_string(d.join("SKILL.md")) {
            Ok(content) => out.push(json!({ "name": safe, "content": content })),
            Err(e) => {
                left.push(LeftOut { name: format!("skill {name}"), why: format!("its SKILL.md: {e}") });
                continue;
            }
        }
        let others = other_files(&d);
        if !others.is_empty() {
            let mut shown: Vec<&str> = others.iter().take(5).map(String::as_str).collect();
            if others.len() > 5 {
                shown.push("…");
            }
            left.push(LeftOut {
                name: format!("skill {name}'s other files"),
                why: format!(
                    "Managoat's inline skills carry SKILL.md only, so {} file(s) stay here ({})",
                    others.len(),
                    shown.join(", ")
                ),
            });
        }
    }
    (out, left)
}

/// The recipe's MCP servers as Managoat keeps them, `${VAR}`s as they are,
/// and the ones that can't go.
fn servers(r: &Recipe, dir: &Path, home: &Path) -> (Map<String, Value>, Vec<LeftOut>) {
    let (found, mut left) = recipes::wear::servers_of(r, dir, home);
    let mut out = Map::new();
    for (name, s) in found {
        let typed: Vec<&str> = s
            .headers
            .iter()
            .chain(s.env.iter())
            .filter(|(_, v)| !fw::is_reference(v))
            .map(|(k, _)| k.as_str())
            .collect();
        let why = if !typed.is_empty() {
            Some(format!(
                "{} {} a value typed in, not a ${{VAR}}: Arugula doesn't copy it to Managoat (put it in a vault and reference it)",
                typed.join(", "),
                if typed.len() == 1 { "holds" } else { "hold" }
            ))
        } else if let Some(host) = s.url.as_deref().and_then(fw::oauth_host) {
            Some(format!("{host} wants an OAuth sign-in, which a sandbox can't do"))
        } else if s.command.as_deref().is_some_and(|c| c.starts_with('/') || c.starts_with('~')) {
            Some("its command is a path on this machine".to_owned())
        } else {
            None
        };
        match why {
            Some(why) => left.push(LeftOut { name, why }),
            None => {
                out.insert(name, serde_json::to_value(&s).unwrap_or_default());
            }
        }
    }
    left.sort_by(|a, b| a.name.cmp(&b.name));
    (out, left)
}

/// Compile the recipe, working in `dir`, to a Managoat agent (and its
/// project, `repo`, to an environment).
pub fn compile(r: &Recipe, dir: &Path, home: &Path, repo: Result<Repo, String>) -> Compiled {
    let (repo, no_repo) = match repo {
        Ok(r) => (Some(r), None),
        Err(e) => (None, Some(e)),
    };
    let mut notes = vec![];
    let (skills, mut left_out) = skills(r, dir, home);
    let (servers, left) = servers(r, dir, home);
    left_out.extend(left);
    let model = match r.model.as_deref() {
        None => None,
        Some(m) => match model(m) {
            Some(id) => Some(id),
            None => {
                left_out.push(LeftOut {
                    name: format!("model {m}"),
                    why: "it isn't a Claude model Managoat can name (sonnet, opus, haiku or a claude-… id)".into(),
                });
                None
            }
        },
    };
    let (policy, policy_notes) = policy(r);
    notes.extend(policy_notes);
    let mut system = r.prompt.clone();
    if let Some(repo) = &repo {
        system.push_str(&format!("\n\nYour project is a git checkout at {} ({}).", repo.mount, repo.url));
    }
    let mut spec = json!({
        "runtime": "claude",
        "description": r.description,
        "system": system,
        "skills": skills,
        "mcp_servers": servers,
        "permission_policy": policy,
    });
    if let Some(m) = &model {
        spec["model"] = json!(m);
    }
    let environment_spec = repo.as_ref().map(|r| {
        let mut entry = json!({ "url": r.url, "mount_path": r.mount });
        if let Some(k) = &r.secret_key {
            entry["secret_key"] = json!(k);
        }
        json!({ "repositories": [entry] })
    });
    let hash = {
        let mut h = Sha256::new();
        h.update(serde_json::to_vec(&json!({ "agent": spec, "environment": environment_spec })).unwrap_or_default());
        hex::encode(h.finalize())[..16].to_owned()
    };
    let source_path = r.path.display().to_string();
    spec["metadata"] = json!({ MANAGED_BY: ARUGULA, "source_path": source_path, "recipe_hash": hash });
    let environment_spec = environment_spec.map(|mut e| {
        e["metadata"] = json!({ MANAGED_BY: ARUGULA });
        e
    });
    let mut vars = BTreeSet::new();
    fw::refs_in(&spec["mcp_servers"], &mut vars);
    if let Some(k) = repo.as_ref().and_then(|r| r.secret_key.clone()) {
        vars.insert(k);
    }
    Compiled {
        recipe: r.name.clone(),
        agent: agent_name(&r.name),
        source_path,
        spec,
        environment_spec,
        hash,
        model,
        permission: permission_text(&policy),
        vars,
        repo,
        no_repo,
        left_out,
        notes,
    }
}

// ---------------------------------------------------------------- finding it

/// What a run asks for.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Ask {
    pub agent: String,
    /// The project it works in; none: where it's offered from.
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    /// A vault for the run (name or id).
    #[serde(default)]
    pub vault: Option<String>,
    /// An environment instead of the project's (name or id).
    #[serde(default)]
    pub environment: Option<String>,
}

impl Ask {
    fn vault(&self) -> Option<&str> {
        self.vault.as_deref().map(str::trim).filter(|v| !v.is_empty())
    }

    fn environment(&self) -> Option<&str> {
        self.environment.as_deref().map(str::trim).filter(|v| !v.is_empty())
    }
}

/// The recipe a run names, and the project it works in: `cwd`, else where
/// it's offered from.
pub fn locate(state_dir: &Path, home: &Path, a: &Ask) -> Result<(Recipe, PathBuf), String> {
    let agent = a.agent.trim();
    if agent.is_empty() {
        return Err("which recipe? {\"agent\": NAME}".into());
    }
    let given = a.cwd.as_deref().map(str::trim).filter(|c| !c.is_empty());
    let dir = match given {
        Some(c) => fw::expand(c, home),
        None => match recipes::offers(state_dir).into_iter().find(|o| o.agent == agent) {
            Some(o) => PathBuf::from(o.dir),
            None => return Err(format!("{agent} isn't offered here: say which project it works in (cwd)")),
        },
    };
    if !dir.is_dir() {
        return Err(format!("{} isn't a directory here", dir.display()));
    }
    Ok((recipes::find(&dir, home, agent)?, dir))
}

/// Find and compile the recipe a run names.
pub async fn prepare(state_dir: &Path, home: &Path, a: &Ask) -> Result<(Compiled, PathBuf), String> {
    let (r, dir) = locate(state_dir, home, a)?;
    let repo = repo(&dir).await;
    Ok((compile(&r, &dir, home, repo), dir))
}

/// The flag a recipe on Managoat needs: recipes' (`agents`). Managoat's
/// own left Labs (#942, F5).
pub fn flags(state_dir: &Path) -> Result<(), String> {
    if crate::labs::DIRECT_MANAGOAT_RETIRED {
        return Err(crate::agent::defs::CLOUD_MIGRATION.into());
    }
    crate::labs::flag(state_dir, arugula_proto::flags::AGENTS)
}

// ---------------------------------------------------------------- Managoat

fn managed(metadata: Option<&Value>) -> bool {
    metadata.and_then(|m| m[MANAGED_BY].as_str()) == Some(ARUGULA)
}

fn by_name<'a>(list: &'a [Named], want: &str) -> Option<&'a Named> {
    list.iter().find(|n| n.id == want || n.name == want)
}

/// One variable the agent uses, and where the run would get it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Var {
    pub name: String,
    /// `vault` or `environment`; none: missing.
    pub on: Option<String>,
}

/// What a run would be, before it is.
#[derive(Debug, Clone, Serialize)]
pub struct Preflight {
    pub recipe: String,
    pub agent: String,
    pub hash: String,
    /// The agent is on Managoat already (Arugula's).
    pub exists: bool,
    /// Why it can't run: an agent of its name Arugula didn't make.
    pub refused: Option<String>,
    pub repo: Option<Repo>,
    pub no_repo: Option<String>,
    /// The environment and vault the run gets (names).
    pub environment: Option<String>,
    pub vault: Option<String>,
    /// To choose from.
    pub environments: Vec<String>,
    pub vaults: Vec<String>,
    pub vars: Vec<Var>,
    pub missing: Vec<String>,
    pub model: Option<String>,
    pub permission: String,
    pub left_out: Vec<LeftOut>,
    pub notes: Vec<String>,
}

impl Preflight {
    /// One paragraph, for MCP and the CLI.
    pub fn text(&self) -> String {
        let mut s = format!("{} runs on Managoat as {}", self.recipe, self.agent);
        if let Some(r) = &self.repo {
            s.push_str(&format!(", in a checkout of {} ({})", r.url, r.environment));
        }
        s.push('.');
        if !self.missing.is_empty() {
            s.push_str(&format!(
                " {} variable(s) missing on Managoat: {}.",
                self.missing.len(),
                self.missing.join(", ")
            ));
        }
        s
    }
}

/// The environment the run gets: the one asked for, else the project's.
fn run_environment(c: &Compiled, a: &Ask) -> Option<String> {
    a.environment().map(str::to_owned).or_else(|| c.repo.as_ref().map(|r| r.environment.clone()))
}

/// Whether the agent's name is free for Arugula: none, or Arugula's.
async fn agent_state(client: &Client, c: &Compiled) -> Result<(bool, Option<String>), String> {
    let agents = client.agents().await.map_err(|e| format!("Managoat's agents: {e}"))?.items;
    Ok(match agents.iter().find(|x| x.name == c.agent) {
        None => (false, None),
        Some(x) if managed(Some(&Value::Object(x.metadata.clone()))) => (true, None),
        Some(_) => (
            true,
            Some(format!(
                "Managoat has an agent named {} that Arugula didn't make (it isn't managed-by: arugula): it's left alone, so this recipe can't run there under that name",
                c.agent
            )),
        ),
    })
}

/// The keys of an environment's and a vault's secrets (and the
/// environment's plain `env_vars`): never values.
async fn keys(
    client: &Client,
    envs: &[Named],
    vaults: &[Named],
    environment: Option<&str>,
    vault: Option<&str>,
) -> Result<(BTreeSet<String>, BTreeSet<String>), String> {
    let mut on_env = BTreeSet::new();
    let mut on_vault = BTreeSet::new();
    if let Some(e) = environment.and_then(|e| by_name(envs, e)) {
        let l =
            client.secret_keys(Holder::Environment, &e.id).await.map_err(|x| format!("{}'s secrets: {x}", e.name))?;
        on_env.extend(l.items.into_iter().map(|k| k.key));
        on_env.extend(e.extra.get("env_vars").and_then(Value::as_object).into_iter().flatten().map(|(k, _)| k.clone()));
    }
    if let Some(name) = vault {
        let v = by_name(vaults, name).ok_or_else(|| format!("no vault named {name:?} on Managoat"))?;
        let l = client.secret_keys(Holder::Vault, &v.id).await.map_err(|x| format!("{}'s secrets: {x}", v.name))?;
        on_vault.extend(l.items.into_iter().map(|k| k.key));
    }
    Ok((on_env, on_vault))
}

/// What the run would be: the agent, its checkout, and the variables it
/// uses that Managoat doesn't have (by key).
pub async fn preflight(client: &Client, c: &Compiled, a: &Ask) -> Result<Preflight, String> {
    let (exists, refused) = agent_state(client, c).await?;
    let envs = client.environments().await.map_err(|e| format!("Managoat's environments: {e}"))?.items;
    let vaults = client.vaults().await.map_err(|e| format!("Managoat's vaults: {e}"))?.items;
    let environment = run_environment(c, a);
    let vault = a.vault().map(str::to_owned);
    let (on_env, on_vault) = keys(client, &envs, &vaults, environment.as_deref(), vault.as_deref()).await?;
    let vars: Vec<Var> = c
        .vars
        .iter()
        .map(|n| Var {
            name: n.clone(),
            on: if on_vault.contains(n) {
                Some("vault".into())
            } else if on_env.contains(n) {
                Some("environment".into())
            } else {
                None
            },
        })
        .collect();
    let missing = vars.iter().filter(|v| v.on.is_none()).map(|v| v.name.clone()).collect();
    let mut notes = c.notes.clone();
    if c.model.is_none() && !exists {
        notes.push(format!("model inherit: Managoat needs one for a new agent, so it starts on {DEFAULT_MODEL}"));
    }
    if let (Some(r), Some(e)) = (&c.repo, environment.as_deref())
        && e != r.environment
    {
        notes.push(format!(
            "environment {e} replaces the project's ({}), so the sandbox has no checkout of it",
            r.environment
        ));
    }
    Ok(Preflight {
        recipe: c.recipe.clone(),
        agent: c.agent.clone(),
        hash: c.hash.clone(),
        exists,
        refused,
        repo: c.repo.clone(),
        no_repo: c.no_repo.clone(),
        environment,
        vault,
        environments: envs.into_iter().map(|e| e.name).collect(),
        vaults: vaults.into_iter().map(|v| v.name).collect(),
        vars,
        missing,
        model: c.model.clone(),
        permission: c.permission.clone(),
        left_out: c.left_out.clone(),
        notes,
    })
}

/// Copy one variable the agent uses from this machine to a vault, as the
/// person asked: resolved here (Infisical through agent-specs, the shell,
/// `gh`) and written once. Answers where it came from; never the value.
pub async fn copy(client: &Client, c: &Compiled, var: &str, vault: &str, l: &Lookup<'_>) -> Result<String, String> {
    if !c.vars.contains(var) {
        return Err(format!("{} doesn't use {var}", c.recipe));
    }
    let vaults = client.vaults().await.map_err(|e| format!("Managoat's vaults: {e}"))?.items;
    let v = by_name(&vaults, vault).ok_or_else(|| format!("no vault named {vault:?} on Managoat"))?;
    let found = fw::resolve(&BTreeSet::from([var.to_owned()]), l).await;
    let (Some(value), Some(from)) = (found.values.get(var), found.from.get(var)) else {
        return Err(format!(
            "{var} isn't set on this machine (Infisical through agent-specs, the shell environment, gh), so there's nothing to copy"
        ));
    };
    client
        .set_secret(Holder::Vault, &v.id, var, value)
        .await
        .map_err(|e| format!("writing {var} to {}: {e}", v.name))?;
    info!(var, vault = v.name, from, "copied a variable to a Managoat vault, as asked");
    Ok(format!("{var} copied from {from} to vault {}", v.name))
}

/// What an upsert did.
#[derive(Debug, Clone, Serialize)]
pub struct Upserted {
    pub agent_id: String,
    /// `Kind name: action` for each row.
    pub rows: Vec<String>,
    pub notes: Vec<String>,
}

/// Upsert the agent (and the project's environment) with one apply, then
/// read the agent's id back.
pub async fn upsert(client: &Client, c: &Compiled) -> Result<Upserted, String> {
    let (exists, refused) = agent_state(client, c).await?;
    if let Some(r) = refused {
        return Err(r);
    }
    let mut notes = vec![];
    let mut spec = c.spec.clone();
    if c.model.is_none() && !exists {
        spec["model"] = json!(DEFAULT_MODEL);
        notes.push(format!("model inherit: a new agent starts on {DEFAULT_MODEL}"));
    }
    let mut resources = vec![];
    if let (Some(r), Some(env)) = (&c.repo, &c.environment_spec) {
        let envs = client.environments().await.map_err(|e| format!("Managoat's environments: {e}"))?.items;
        match envs.iter().find(|e| e.name == r.environment) {
            Some(e) if !managed(e.extra.get("metadata")) => {
                notes.push(format!("environment {} isn't Arugula's: it's used as it is", r.environment));
            }
            _ => resources.push(Resource {
                kind: "Environment".into(),
                name: r.environment.clone(),
                spec: Some(env.clone()),
            }),
        }
    }
    resources.push(Resource { kind: "Agent".into(), name: c.agent.clone(), spec: Some(spec) });
    let applied = client.apply(&resources).await.map_err(|e| format!("applying {}: {e}", c.agent))?;
    let mut rows = vec![];
    for a in &applied {
        if a.action == "error" {
            let why = a.errors.as_ref().map(Value::to_string).unwrap_or_else(|| "no reason given".into());
            return Err(format!("Managoat refused {} {}: {why}", a.kind, a.name));
        }
        rows.push(format!("{} {}: {}", a.kind, a.name, a.action));
    }
    let agents = client.agents().await.map_err(|e| format!("reading {} back: {e}", c.agent))?.items;
    let id = agents
        .iter()
        .find(|a| a.name == c.agent)
        .map(|a| a.id.clone())
        .ok_or_else(|| format!("{} was applied but isn't listed", c.agent))?;
    info!(agent = c.agent, hash = c.hash, rows = rows.join("; "), "a recipe upserted on Managoat");
    Ok(Upserted { agent_id: id, rows, notes })
}

/// The agent block a run opens: a conversation with the upserted agent on
/// the native link, labelled with the recipe, its hash and this machine.
pub fn block_config(c: &Compiled, up: &Upserted, a: &Ask, machine: &str) -> Value {
    let mut labels = BTreeMap::new();
    labels.insert(LABEL_RECIPE, c.recipe.clone());
    labels.insert(LABEL_HASH, c.hash.clone());
    labels.insert(LABEL_MACHINE, machine.to_owned());
    let mut config = json!({
        "agent": "managoat",
        "managoat_agent": up.agent_id,
        "labels": labels,
        "permission": c.permission,
        "title": format!("{} on Managoat", c.recipe),
    });
    if let Some(e) = run_environment(c, a) {
        config["environment"] = json!(e);
    }
    if let Some(v) = a.vault() {
        config["vault"] = json!(v);
    }
    if let Some(p) = a.prompt.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        config["prompt"] = json!(p);
    }
    config
}

/// What a run did, in a sentence or two.
pub fn summary(c: &Compiled, up: &Upserted) -> String {
    let mut s = format!("{} runs on Managoat as {} ({})", c.recipe, c.agent, up.rows.join(", "));
    if let Some(r) = &c.repo {
        s.push_str(&format!(", in a checkout of {}", r.url));
        if !r.stays.is_empty() {
            s.push_str(&format!(" ({} stay here)", r.stays.join(", ")));
        }
    }
    s.push('.');
    let mut lost: Vec<String> = c.left_out.iter().map(|l| format!("{} ({})", l.name, l.why)).collect();
    lost.extend(c.notes.iter().cloned());
    lost.extend(up.notes.iter().cloned());
    if !lost.is_empty() {
        s.push_str(&format!(" Didn't carry over as is: {}.", lost.join("; ")));
    }
    s
}

/// Everything a run needs but a block: the recipe compiled and upserted,
/// and the block's config. The caller opens it.
pub async fn launch(state_dir: &Path, home: &Path, machine: &str, a: &Ask) -> Result<(Value, String), String> {
    flags(state_dir)?;
    let (c, _) = prepare(state_dir, home, a).await?;
    let client = super::link::client_for(None).await?;
    let up = upsert(&client, &c).await?;
    Ok((block_config(&c, &up, a, machine), summary(&c, &up)))
}

/// [`copy`], for a run's recipe, resolving as wearing does: this machine's
/// environment (the user's shell's over the daemon's), agent-specs if it's
/// checked out, and the run's environment and vault (for agent-specs'
/// mapping).
pub async fn copy_here(
    client: &Client,
    state_dir: &Path,
    home: &Path,
    env: &[(String, String)],
    a: &Ask,
    var: &str,
    vault: &str,
) -> Result<String, String> {
    flags(state_dir)?;
    let (c, _) = prepare(state_dir, home, a).await?;
    let specs = fw::expand(super::DEFAULT_SPECS, home);
    let l = Lookup {
        env,
        home,
        specs: specs.join(".infisical.json").is_file().then_some(specs),
        environment: run_environment(&c, a),
        vault: Some(vault.to_owned()),
    };
    copy(client, &c, var, vault, &l).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe(front: &str) -> Recipe {
        recipes::parse(&format!("---\nname: fixer\n{front}---\nFix tests.\n"), Path::new("/p/.claude/agents/fixer.md"))
            .unwrap()
    }

    #[test]
    fn models_become_canonical_and_inherit_is_none() {
        assert_eq!(model("haiku").as_deref(), Some("anthropic/claude-haiku-5-5"));
        assert_eq!(model("sonnet").as_deref(), Some("anthropic/claude-sonnet-5-5"));
        assert_eq!(model("opus").as_deref(), Some("anthropic/claude-opus-5-5"));
        assert_eq!(model("claude-fable-5-1").as_deref(), Some("anthropic/claude-fable-5-1"));
        assert_eq!(model("anthropic/claude-opus-5").as_deref(), Some("anthropic/claude-opus-5"));
        assert_eq!(model("gpt-5"), None);
        assert_eq!(model("opus[1m]"), None);
        assert_eq!(recipe("model: inherit\n").model, None);
    }

    #[test]
    fn tools_and_modes_become_a_coarse_policy_and_say_so() {
        // No list, no mode: ask for everything.
        let (p, notes) = policy(&recipe(""));
        assert_eq!(permission_text(&p), "default=ask");
        assert!(notes.is_empty());
        // Read and Grep only: no Bash, no Edit, no WebFetch.
        let (p, notes) = policy(&recipe("tools: Read, Grep\n"));
        assert_eq!(p["execute"], "auto_deny");
        assert_eq!(p["edit"], "auto_deny");
        assert_eq!(p["fetch"], "auto_deny");
        assert!(notes[0].contains("no tool allowlist"), "{notes:?}");
        // Bash allowed by a rule; Write but not Edit; acceptEdits.
        let (p, notes) = policy(&recipe("tools: Read, Bash(git:*), Write\npermissionMode: acceptEdits\n"));
        assert!(!p.contains_key("execute"));
        assert_eq!(p["edit"], "auto_allow");
        assert!(notes.iter().any(|n| n.contains("Bash(git:*)")), "{notes:?}");
        // disallowedTools: Bash.
        let (p, _) = policy(&recipe("disallowedTools: [Bash]\n"));
        assert_eq!(p["execute"], "auto_deny");
        assert!(!p.contains_key("edit"));
        // acceptEdits with no Edit: deny wins.
        let (p, _) = policy(&recipe("tools: [Read]\npermissionMode: acceptEdits\n"));
        assert_eq!(p["edit"], "auto_deny");
        let (p, notes) = policy(&recipe("permissionMode: plan\n"));
        assert_eq!((p["edit"].as_str(), p["execute"].as_str()), ("auto_deny", "auto_deny"));
        assert!(notes[0].contains("plan"));
        assert_eq!(policy(&recipe("permissionMode: bypassPermissions\n")).0["default"], "auto_allow");
        // The block's grammar parses back to the same policy.
        let (p, _) = policy(&recipe("tools: Read\n"));
        let back = super::super::link::permission_policy(&permission_text(&p)).unwrap().unwrap();
        assert_eq!(back, serde_json::to_value(&p).unwrap());
    }

    #[test]
    fn origins_become_https_without_credentials() {
        let want = |u: &str, o: &str, n: &str| Some((u.to_owned(), o.to_owned(), n.to_owned()));
        assert_eq!(
            https("git@github.com:acme/widgets.git"),
            want("https://github.com/acme/widgets.git", "acme", "widgets")
        );
        assert_eq!(
            https("https://github.com/acme/widgets"),
            want("https://github.com/acme/widgets.git", "acme", "widgets")
        );
        assert_eq!(
            https("https://x-access-token:ghp_secret@github.com/acme/widgets.git\n"),
            want("https://github.com/acme/widgets.git", "acme", "widgets")
        );
        assert_eq!(
            https("ssh://git@gitlab.example.com:2222/group/sub/widgets.git"),
            want("https://gitlab.example.com/group/sub/widgets.git", "group-sub", "widgets")
        );
        assert_eq!(https("/srv/git/widgets.git"), None);
        assert_eq!(https("file:///srv/git/widgets.git"), None);
        assert_eq!(https("https://github.com/"), None);
    }

    #[test]
    fn a_recipe_compiles_to_an_agent_and_names_what_stays() {
        let t =
            std::env::temp_dir().join(format!("arugula-ftn-compile-{}-{}", std::process::id(), crate::store::now_ms()));
        let (proj, home) = (t.join("p"), t.join("h"));
        std::fs::create_dir_all(proj.join(".claude/skills/testing/scripts")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(proj.join(".claude/skills/testing/SKILL.md"), "---\nname: testing\n---\nRun them.").unwrap();
        std::fs::write(proj.join(".claude/skills/testing/scripts/run.sh"), "make test").unwrap();
        std::fs::write(
            proj.join(".mcp.json"),
            r#"{"mcpServers":{"docs":{"command":"npx","args":["-y","docs-mcp"],"env":{"KEY":"${DOCS_KEY}"}},"typed":{"type":"http","url":"https://example.com/mcp","headers":{"Authorization":"Bearer hunter2"}},"local":{"command":"/home/me/bin/mcp"}}}"#,
        )
        .unwrap();
        let text = "---\nname: fixer\ndescription: Fixes tests\ntools: Read, Edit\nmodel: haiku\nskills: [testing, gone]\nmcpServers: [docs, typed, local, nowhere]\n---\nFix tests.\n";
        let path = proj.join(".claude/agents/fixer.md");
        let r = recipes::parse(text, &path).unwrap();
        let repo = Repo {
            url: "https://github.com/acme/widgets.git".into(),
            environment: "repo-acme-widgets".into(),
            mount: "/work/widgets".into(),
            secret_key: Some("GITHUB_TOKEN".into()),
            stays: vec![],
        };
        let c = compile(&r, &proj, &home, Ok(repo.clone()));
        assert_eq!(c.agent, "recipe-fixer");
        assert_eq!(c.spec["runtime"], "claude");
        assert_eq!(c.spec["model"], "anthropic/claude-haiku-5-5");
        assert!(c.spec["system"].as_str().unwrap().starts_with("Fix tests."));
        assert!(c.spec["system"].as_str().unwrap().contains("/work/widgets"));
        assert_eq!(c.spec["skills"], json!([{ "name": "testing", "content": "---\nname: testing\n---\nRun them." }]));
        // The reference goes as it is; the typed-in value doesn't go at all.
        assert_eq!(c.spec["mcp_servers"]["docs"]["env"]["KEY"], "${DOCS_KEY}");
        assert!(c.spec["mcp_servers"].get("typed").is_none());
        assert!(!c.spec.to_string().contains("hunter2"));
        assert_eq!(c.spec["permission_policy"]["execute"], "auto_deny");
        assert_eq!(c.spec["metadata"][MANAGED_BY], ARUGULA);
        assert_eq!(c.spec["metadata"]["recipe_hash"], c.hash);
        assert_eq!(c.vars, BTreeSet::from(["DOCS_KEY".to_owned(), "GITHUB_TOKEN".to_owned()]));
        let env = c.environment_spec.as_ref().unwrap();
        assert_eq!(env["repositories"][0]["url"], "https://github.com/acme/widgets.git");
        assert_eq!(env["repositories"][0]["secret_key"], "GITHUB_TOKEN");
        let left: Vec<&str> = c.left_out.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(
            left,
            ["skill testing's other files", "skill gone", "local", "nowhere", "typed"],
            "{:?}",
            c.left_out
        );
        assert!(c.left_out[0].why.contains("scripts/run.sh"));
        // The hash follows what's compiled.
        assert_eq!(compile(&r, &proj, &home, Ok(repo.clone())).hash, c.hash);
        std::fs::write(proj.join(".claude/skills/testing/SKILL.md"), "Run them all.").unwrap();
        assert_ne!(compile(&r, &proj, &home, Ok(repo)).hash, c.hash);
        // No checkout: no environment, and why.
        let c = compile(&r, &proj, &home, Err("no origin".into()));
        assert!(c.environment_spec.is_none() && c.no_repo.as_deref() == Some("no origin"));
        let _ = std::fs::remove_dir_all(&t);
    }
}
