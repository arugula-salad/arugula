//! Wearing an agent definition on this host: what M76's recipes (a Claude
//! Code subagent file, [`super::agents::wear`]) and A2A delegation
//! ([`super::agents::delegate`]) share. Moved here from M44's
//! `fountain/wear.rs` when wearing a Fountain agent locally was retired
//! (2026-10-15, #942); the behaviour is the same.
//!
//! **MCP servers** get Fountain's substitution ([`substitute`]): `${VAR}`
//! (an UPPER_SNAKE name) is replaced, `$$` is a literal `$`, a value isn't
//! expanded again, and every unset name is reported. A variable's value
//! comes, in order, from ([`resolve`]):
//! 1. Infisical, when a [`Lookup`] names an agent-specs checkout (its
//!    `.infisical.json`): the environment's mapping in `dist/fountain.yaml`
//!    maps the variable to an `infisical:///<env>/<KEY>` URI (or to a
//!    literal value), read with `infisical secrets get KEY --env ENV --path
//!    PATH --plain --silent` there. A variable it doesn't map is tried as
//!    itself in env `dev`. (Recipes name none today.)
//! 2. The user's shell environment (#74).
//! 3. Helpers: `GITHUB_TOKEN` (and `GH_TOKEN`) from `gh auth token`.
//!
//! A server with an unset variable is left out, and so is one that needs an
//! OAuth sign-in, which a headless Claude Code can't do (no auth header,
//! and a known OAuth host, or a 401 with `WWW-Authenticate` when probed).
//! So is a Fountain connection, which only Fountain's sandboxes have. Every
//! credential goes to the session by reference, never on a command line
//! ([`servers`]). The block's header names what was left out, and why.
//!
//! `ARUGULA_INFISICAL_BIN` and `ARUGULA_GH_BIN` are for tests, which never
//! reach the real ones.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::LazyLock,
    time::Duration,
};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tracing::info;

/// A cached version this old is removed (no block runs that long on one).
pub(crate) const OLD: Duration = Duration::from_secs(7 * 24 * 3600);

/// How long one `infisical`, `gh` or `git` may take.
const TOOL_TIMEOUT: Duration = Duration::from_secs(60);

/// How long an OAuth probe may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(4);

/// MCP hosts known to want an OAuth sign-in (S24: `mem0`).
const OAUTH_HOSTS: &[&str] = &["mcp.mem0.ai"];

static REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\$|\$\{([A-Z_][A-Z0-9_]*)\}").unwrap());

// ---------------------------------------------------------------- substitution

/// Add the variables `s` references to `out` (`$$` is none).
pub fn refs(s: &str, out: &mut BTreeSet<String>) {
    for c in REF.captures_iter(s) {
        if let Some(m) = c.get(1) {
            out.insert(m.as_str().to_owned());
        }
    }
}

/// Every variable referenced anywhere in `v` (strings at any depth).
pub fn refs_in(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::String(s) => refs(s, out),
        Value::Array(l) => l.iter().for_each(|x| refs_in(x, out)),
        Value::Object(m) => m.values().for_each(|x| refs_in(x, out)),
        _ => {}
    }
}

/// Fountain's substitution of one string (`Managoat.Substitution`): each
/// `${VAR}` from `vars`, `$$` a literal `$`, once (a value isn't expanded
/// again). With any variable missing, every missing name, sorted.
pub fn substitute(s: &str, vars: &BTreeMap<String, String>) -> Result<String, BTreeSet<String>> {
    let mut missing = BTreeSet::new();
    refs(s, &mut missing);
    missing.retain(|k| !vars.contains_key(k));
    if !missing.is_empty() {
        return Err(missing);
    }
    Ok(REF
        .replace_all(s, |c: &regex::Captures| match c.get(1) {
            None => "$".to_owned(),
            Some(m) => vars[m.as_str()].clone(),
        })
        .into_owned())
}

/// [`substitute`] over a whole value: strings at any depth (not keys);
/// every missing name across all of it.
pub fn substitute_value(v: &Value, vars: &BTreeMap<String, String>) -> Result<Value, BTreeSet<String>> {
    let mut missing = BTreeSet::new();
    let out = walk(v, vars, &mut missing);
    if missing.is_empty() { Ok(out) } else { Err(missing) }
}

fn walk(v: &Value, vars: &BTreeMap<String, String>, missing: &mut BTreeSet<String>) -> Value {
    match v {
        Value::String(s) => match substitute(s, vars) {
            Ok(s) => Value::String(s),
            Err(m) => {
                missing.extend(m);
                v.clone()
            }
        },
        Value::Array(l) => Value::Array(l.iter().map(|x| walk(x, vars, missing)).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), walk(x, vars, missing))).collect()),
        v => v.clone(),
    }
}

// ---------------------------------------------------------------- the shapes

/// One MCP server, as Claude Code's config writes it: `http`/`sse` with a
/// `url` and `headers`, or `stdio` with a `command`, `args` and `env`. Any
/// string may hold `${VAR}`s.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct McpServer {
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "string_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, deserialize_with = "string_list", skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, deserialize_with = "string_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// What stands in for a value typed in literally (not a `${VAR}`
/// reference) when a recipe is shown.
pub const REDACTED: &str = "<redacted>";

impl McpServer {
    /// As it may be shown: a header or env value that isn't (or doesn't
    /// hold) a `${VAR}` reference may be a secret someone typed in, and
    /// becomes [`REDACTED`].
    pub fn redacted(&self) -> Self {
        let hide = |m: &BTreeMap<String, String>| {
            m.iter().map(|(k, v)| (k.clone(), if is_reference(v) { v.clone() } else { REDACTED.to_owned() })).collect()
        };
        Self { headers: hide(&self.headers), env: hide(&self.env), ..self.clone() }
    }
}

/// Whether a value holds a `${VAR}` reference.
pub fn is_reference(v: &str) -> bool {
    v.find("${").is_some_and(|at| v[at..].contains('}'))
}

/// A map of strings: null is empty, a null value is left out, a number or
/// boolean is its text.
fn string_map<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeMap<String, String>, D::Error> {
    let m = Option::<Map<String, Value>>::deserialize(d)?.unwrap_or_default();
    Ok(m.into_iter().filter_map(|(k, v)| text(v).map(|v| (k, v))).collect())
}

/// A list of strings, as [`string_map`].
fn string_list<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let l = Option::<Vec<Value>>::deserialize(d)?.unwrap_or_default();
    Ok(l.into_iter().filter_map(text).collect())
}

fn text(v: Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s),
        v => Some(v.to_string()),
    }
}

// ---------------------------------------------------------------- MCP servers

/// A server (as Fountain keeps it: Claude Code's config shape) as ACP's
/// `session/new` takes it: http `{type, name, url, headers: [{name,
/// value}]}`, stdio `{name, command, args, env: [{name, value}]}`. Or why
/// it can't be.
pub fn acp_server(name: &str, s: &Value) -> Result<Value, String> {
    let pairs = |v: &Value| -> Vec<Value> {
        v.as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| v.as_str().map(|v| json!({ "name": k, "value": v })))
            .collect()
    };
    if let Some(url) = s["url"].as_str() {
        let kind = match s["type"].as_str().unwrap_or("http") {
            "http" | "streamable-http" | "streamableHttp" => "http",
            "sse" => "sse",
            other => return Err(format!("its type {other:?} isn't one Claude Code takes")),
        };
        return Ok(json!({ "type": kind, "name": name, "url": url, "headers": pairs(&s["headers"]) }));
    }
    if let Some(command) = s["command"].as_str() {
        let args: Vec<&str> = s["args"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        return Ok(json!({ "name": name, "command": command, "args": args, "env": pairs(&s["env"]) }));
    }
    if s.get("connection").is_some() {
        return Err("it's a Fountain connection, which only Fountain's sandboxes have".into());
    }
    Err("it has neither a url nor a command".into())
}

/// Whether an http server's headers carry credentials of their own.
fn has_auth(acp: &Value) -> bool {
    acp["headers"].as_array().into_iter().flatten().any(|h| {
        let n = h["name"].as_str().unwrap_or("").to_ascii_lowercase();
        n == "authorization" || n.contains("api-key") || n.contains("apikey") || n.contains("token")
    })
}

/// A URL on a host known to want an OAuth sign-in.
pub fn oauth_host(url: &str) -> Option<String> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    OAUTH_HOSTS.contains(&host.as_str()).then_some(host)
}

/// What a server answered an unauthenticated `initialize`: an OAuth
/// sign-in is what it wants when it's a 401 that says how to authenticate.
pub fn says_oauth(status: u16, www_authenticate: bool) -> bool {
    status == 401 && www_authenticate
}

/// Ask an http server, without credentials, to initialize (briefly). An
/// answer that doesn't come counts as no.
async fn probe_oauth(url: &str) -> bool {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "arugula", "version": env!("CARGO_PKG_VERSION") } } });
    let req = crate::forge::http()
        .post(url)
        .header("accept", "application/json, text/event-stream")
        .json(&body)
        .timeout(PROBE_TIMEOUT)
        .send();
    match tokio::time::timeout(PROBE_TIMEOUT, req).await {
        Ok(Ok(r)) => says_oauth(r.status().as_u16(), r.headers().contains_key("www-authenticate")),
        _ => false,
    }
}

// ---------------------------------------------------------------- agent-specs

/// What `dist/fountain.yaml` says: each Environment's and Vault's secrets
/// (`key` → `value`: an `infisical://` URI or a literal), and each Agent's
/// environment. A small reader for what chant writes, not YAML at large.
///
/// It reads chant's build output, not a contract document (#305):
/// agent-specs is a chant project, not a workspace, and `dist/fountain.yaml`
/// is what its fountain lexicon writes for Fountain to apply, so it is the
/// file Fountain itself trusts. No chant read gives an Environment's
/// secrets mapping without building.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Specs {
    /// `(kind, name)` → key → value.
    pub secrets: BTreeMap<(String, String), BTreeMap<String, String>>,
    /// An agent's name → its environment's.
    pub environments: BTreeMap<String, String>,
}

/// A scalar as YAML writes it: double-quoted (JSON's escapes), single-quoted
/// (`''`), or plain.
fn scalar(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('"') {
        return serde_json::from_str::<String>(s).unwrap_or_else(|_| s.trim_matches('"').to_owned());
    }
    if let Some(inner) = s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    s.split(" #").next().unwrap_or(s).trim().to_owned()
}

pub fn parse_specs(yaml: &str) -> Specs {
    let mut out = Specs::default();
    for doc in yaml.split("\n---") {
        let mut kind = String::new();
        let mut name = String::new();
        let mut env = None;
        let mut secrets: BTreeMap<String, String> = BTreeMap::new();
        let (mut in_spec, mut in_secrets) = (false, false);
        let mut key: Option<String> = None;
        for line in doc.lines() {
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            let t = line.trim();
            if indent == 0 {
                in_spec = t == "spec:";
                in_secrets = false;
                if let Some(k) = t.strip_prefix("kind:") {
                    kind = scalar(k);
                }
                continue;
            }
            if !in_spec {
                continue;
            }
            if indent == 2 {
                in_secrets = t == "secrets:";
                if let Some(n) = t.strip_prefix("name:") {
                    name = scalar(n);
                } else if let Some(e) = t.strip_prefix("environment:") {
                    env = Some(scalar(e));
                }
                continue;
            }
            if !in_secrets {
                continue;
            }
            let item = t.strip_prefix("- ").unwrap_or(t);
            if let Some(k) = item.strip_prefix("key:") {
                key = Some(scalar(k));
            } else if let (Some(v), Some(k)) = (item.strip_prefix("value:"), key.as_ref()) {
                secrets.insert(k.clone(), scalar(v));
            }
        }
        if name.is_empty() {
            continue;
        }
        match kind.as_str() {
            "Environment" | "Vault" => {
                out.secrets.insert((kind, name), secrets);
            }
            "Agent" => {
                if let Some(e) = env {
                    out.environments.insert(name, e);
                }
            }
            _ => {}
        }
    }
    out
}

/// Where an `infisical://[project]/<env>/<path…>/<KEY>` URI points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfisicalRef {
    pub project: Option<String>,
    pub env: String,
    pub path: String,
    pub key: String,
}

impl InfisicalRef {
    pub fn parse(uri: &str) -> Option<Self> {
        let rest = uri.strip_prefix("infisical://")?;
        let (project, rest) = rest.split_once('/')?;
        let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
        let (key, mid) = segs.split_last()?;
        let (env, path) = mid.split_first()?;
        Some(Self {
            project: (!project.is_empty()).then(|| project.to_owned()),
            env: (*env).to_owned(),
            path: format!("/{}", path.join("/")),
            key: (*key).to_owned(),
        })
    }

    /// As the header says where a value came from.
    pub fn label(&self) -> String {
        let path = if self.path == "/" { String::new() } else { format!("{}/", self.path.trim_matches('/')) };
        format!("Infisical {}/{path}{}", self.env, self.key)
    }
}

// ---------------------------------------------------------------- secrets

/// The values found for some variables, and where each came from.
#[derive(Default)]
pub struct Found {
    pub values: BTreeMap<String, String>,
    pub from: BTreeMap<String, String>,
}

impl std::fmt::Debug for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Found").field("from", &self.from).finish_non_exhaustive()
    }
}

/// Where the variables are looked up: the host's environment (the daemon's
/// with the user's shell's over it), and the agent-specs checkout.
pub struct Lookup<'a> {
    pub env: &'a [(String, String)],
    pub home: &'a Path,
    pub specs: Option<PathBuf>,
    /// The agent's environment's name, for the agent-specs mapping.
    pub environment: Option<String>,
    /// Its vault's, whose secrets win over the environment's (as on
    /// Fountain).
    pub vault: Option<String>,
}

impl Lookup<'_> {
    fn var(&self, k: &str) -> Option<String> {
        self.env
            .iter()
            .rev()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
            .or_else(|| std::env::var(k).ok())
            .filter(|v| !v.is_empty())
    }
}

/// A tool's output, run on this host with `env` (stdin closed; within
/// [`TOOL_TIMEOUT`]): stdout if it exited 0, else why not.
async fn run(argv: &[String], cwd: Option<&Path>, env: &[(String, String)]) -> Result<String, String> {
    let (bin, args) = argv.split_first().ok_or("nothing to run")?;
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let out = tokio::time::timeout(TOOL_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("{bin} took too long"))?
        .map_err(|e| format!("can't run {bin}: {e}"))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let err = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    Err(format!("{bin} failed{}", if err.is_empty() { String::new() } else { format!(": {err}") }))
}

/// A setting from the host's environment (the block's, then the daemon's).
fn setting(env: &[(String, String)], var: &str, default: &str) -> String {
    env.iter()
        .rev()
        .find(|(k, _)| k == var)
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var(var).ok())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| default.to_owned())
}

/// `~` and `~/…` against `home`.
pub fn expand(p: &str, home: &Path) -> PathBuf {
    match p.trim() {
        "~" => home.to_owned(),
        p => p.strip_prefix("~/").map(|r| home.join(r)).unwrap_or_else(|| PathBuf::from(p)),
    }
}

/// The values of `names`, from Infisical, then the shell environment, then
/// helpers. Names none of them has are left out.
pub async fn resolve(names: &BTreeSet<String>, l: &Lookup<'_>) -> Found {
    let mut found = Found::default();
    if names.is_empty() {
        return found;
    }
    // 1. Infisical, through agent-specs' mapping.
    if let Some(specs) = l.specs.as_ref().filter(|d| d.join(".infisical.json").is_file()) {
        let parsed =
            std::fs::read_to_string(specs.join("dist/fountain.yaml")).map(|y| parse_specs(&y)).unwrap_or_default();
        let of = |kind: &str, name: &Option<String>| {
            name.as_ref().and_then(|n| parsed.secrets.get(&(kind.to_owned(), n.clone()))).cloned().unwrap_or_default()
        };
        let mut mapped = of("Environment", &l.environment);
        mapped.extend(of("Vault", &l.vault));
        let infisical = setting(l.env, "ARUGULA_INFISICAL_BIN", "infisical");
        let jobs = names.iter().map(|name| {
            let (specs, infisical, mapped) = (specs.clone(), infisical.clone(), &mapped);
            async move {
                let (target, fallback) = match mapped.get(name) {
                    Some(v) if v.starts_with("infisical://") => (InfisicalRef::parse(v)?, false),
                    // A literal in agent-specs (a git identity): public.
                    Some(v) => return Some((name.clone(), v.clone(), "agent-specs".to_owned())),
                    None => {
                        (InfisicalRef { project: None, env: "dev".into(), path: "/".into(), key: name.clone() }, true)
                    }
                };
                let mut argv = vec![
                    infisical,
                    "secrets".into(),
                    "get".into(),
                    target.key.clone(),
                    "--env".into(),
                    target.env.clone(),
                    "--path".into(),
                    target.path.clone(),
                    "--plain".into(),
                    "--silent".into(),
                ];
                if let Some(p) = &target.project {
                    argv.extend(["--projectId".into(), p.clone()]);
                }
                match run(&argv, Some(&specs), l.env).await {
                    Ok(v) if !v.trim().is_empty() => {
                        let from = if fallback {
                            format!("{} (not mapped in agent-specs: tried as itself)", target.label())
                        } else {
                            target.label()
                        };
                        Some((name.clone(), v.trim_end_matches(['\n', '\r']).to_owned(), from))
                    }
                    Ok(_) => None,
                    Err(e) => {
                        info!(var = name, error = e, "not in Infisical");
                        None
                    }
                }
            }
        });
        for (name, value, from) in futures_util::future::join_all(jobs).await.into_iter().flatten() {
            found.values.insert(name.clone(), value);
            found.from.insert(name, from);
        }
    }
    // 2. The shell environment.
    for name in names {
        if found.values.contains_key(name) {
            continue;
        }
        if let Some(v) = l.var(name) {
            found.values.insert(name.clone(), v);
            found.from.insert(name.clone(), "shell environment".into());
        }
    }
    // 3. Helpers.
    for name in ["GITHUB_TOKEN", "GH_TOKEN"] {
        if names.contains(name) && !found.values.contains_key(name) {
            let gh = setting(l.env, "ARUGULA_GH_BIN", "gh");
            match run(&[gh, "auth".into(), "token".into()], Some(l.home), l.env).await {
                Ok(t) if !t.trim().is_empty() => {
                    found.values.insert(name.to_owned(), t.trim().to_owned());
                    found.from.insert(name.to_owned(), "gh auth token".into());
                }
                Ok(_) => {}
                Err(e) => info!(error = e, "gh auth token"),
            }
        }
    }
    found
}

// ---------------------------------------------------------------- caches

/// Where Arugula caches bundles: `$XDG_CACHE_HOME` (else `~/.cache`)
/// `/arugula/fountain`.
pub fn cache_root(env: &[(String, String)], home: &Path) -> PathBuf {
    let xdg = env
        .iter()
        .rev()
        .find(|(k, _)| k == "XDG_CACHE_HOME")
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var("XDG_CACHE_HOME").ok())
        .filter(|v| v.starts_with('/'));
    let base = xdg.map(PathBuf::from).unwrap_or_else(|| home.join(".cache"));
    // Bundles cached before the rename are used where they are.
    arugula_core::rename::kept(base.join("arugula/fountain"), base.join("illogical/fountain"))
}

/// A name safe as one path component.
pub(crate) fn component(s: &str) -> String {
    let s: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || "._-@".contains(c) { c } else { '-' }).collect();
    let s = s.trim_matches('.').to_owned();
    if s.is_empty() { "_".into() } else { s }
}

pub(crate) fn slug(s: &str) -> String {
    let s: String = s.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() { "agent".into() } else { s }
}

/// A file's age, if it's there.
pub(crate) fn age(p: &Path) -> Option<Duration> {
    std::fs::metadata(p).ok()?.modified().ok()?.elapsed().ok()
}

pub(crate) fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let t = e.file_type()?;
        let dest = to.join(e.file_name());
        if t.is_dir() {
            if e.file_name() != ".git" {
                copy_dir(&e.path(), &dest)?;
            }
        } else if t.is_file() {
            std::fs::copy(e.path(), &dest)?;
        }
        // Symlinks are left out: they could point anywhere.
    }
    Ok(())
}

// ---------------------------------------------------------------- wearing

/// A server that came along, and where its variables came from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ServerInfo {
    pub name: String,
    /// http, sse or stdio.
    pub kind: String,
    /// `NAME from WHERE` for each variable it uses.
    pub vars: Vec<String>,
}

/// One that didn't, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LeftOut {
    pub name: String,
    pub why: String,
}

/// What the block shows: nothing secret.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Info {
    pub agent: String,
    pub id: String,
    pub model: Option<String>,
    pub plugin: String,
    pub bundle: String,
    pub skills: Vec<String>,
    pub skills_missing: Vec<String>,
    pub servers: Vec<ServerInfo>,
    pub left_out: Vec<LeftOut>,
    /// M76: the recipe (a Claude Code subagent file) it was worn from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe: Option<String>,
}

impl Info {
    /// One paragraph for `capture --text`.
    pub fn text(&self) -> String {
        let list = |v: &[String]| if v.is_empty() { "none".to_owned() } else { v.join(", ") };
        let servers: Vec<String> = self.servers.iter().map(|s| s.name.clone()).collect();
        let what = match &self.recipe {
            Some(path) => format!("As the agent {} ({path})", self.agent),
            None => format!("As the agent {}", self.agent),
        };
        let mut out = format!("{what}. Skills: {}. MCP servers: {}.", list(&self.skills), list(&servers));
        let mut missing: Vec<String> = self.left_out.iter().map(|l| format!("{} ({})", l.name, l.why)).collect();
        missing.extend(self.skills_missing.iter().map(|s| format!("skill {s}")));
        if !missing.is_empty() {
            out.push_str(&format!(" Didn't carry over: {}.", missing.join("; ")));
        }
        out
    }
}

/// An agent, worn: what `session/new` gets. The servers' values (secrets
/// among them) are in memory only.
pub struct Worn {
    pub info: Info,
    pub system: String,
    pub plugin: PathBuf,
    /// ACP's shape, each credential a reference into `env`.
    pub servers: Vec<Value>,
    /// What the references are: the adapter's environment.
    pub env: Vec<(String, String)>,
    /// Every value that went into them which may be secret, to keep out of
    /// logs.
    pub secrets: Vec<String>,
    /// M76: a recipe's tool lists (Claude Code's `tools`, `disallowedTools`).
    pub tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    /// M76: the permission mode a recipe's session starts in, unless the
    /// block says another.
    pub permission_mode: Option<String>,
    /// M76: only its own MCP servers (and Arugula's): none of the account's
    /// claude.ai connectors, which Claude Code adds otherwise.
    pub strict_mcp: bool,
}

impl std::fmt::Debug for Worn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Worn").field("info", &self.info).finish_non_exhaustive()
    }
}

impl Worn {
    /// Put it on `session/new`'s `_meta`: the system prompt, the plugin and
    /// the model, keeping `settingSources: []`.
    pub fn dress(&self, meta: &mut Value) {
        crate::agent::defs::wear_meta(meta, &self.system, &self.plugin, self.info.model.as_deref());
        let o = &mut meta["claudeCode"]["options"];
        if !self.tools.is_empty() {
            o["tools"] = json!(self.tools);
        }
        if !self.disallowed_tools.is_empty() {
            o["disallowedTools"] = json!(self.disallowed_tools);
        }
        if self.strict_mcp {
            // As the adapter passes its own flags ("" is no value).
            o["extraArgs"]["strict-mcp-config"] = json!("");
        }
    }

    /// The permission mode it asks for (a recipe's `permissionMode`).
    pub fn permission_mode(&self) -> Option<&str> {
        self.permission_mode.as_deref()
    }

    /// The servers it adds to the agent's own, each credential a reference.
    pub fn servers(&self) -> &[Value] {
        &self.servers
    }

    /// What the references stand for: the adapter's environment.
    pub fn env(&self) -> &[(String, String)] {
        &self.env
    }

    /// Every value that may be secret, to keep out of logs.
    pub fn secrets(&self) -> &[String] {
        &self.secrets
    }

    /// What the block shows of it (nothing secret), as JSON.
    pub fn info(&self) -> Value {
        serde_json::to_value(&self.info).unwrap_or_default()
    }

    /// [`Info::text`], for `capture --text`.
    pub fn text(&self) -> String {
        self.info.text()
    }
}

/// What the session gets of the agent's servers.
#[derive(Default)]
pub struct Served {
    /// ACP's shape, every credential a `${ARUGULA_FTN_…}` reference.
    pub list: Vec<Value>,
    /// What those references are: the adapter's environment (owner-only,
    /// unlike a command line).
    pub env: Vec<(String, String)>,
    pub infos: Vec<ServerInfo>,
    pub left: Vec<LeftOut>,
    /// Every value that may be secret, to keep out of logs.
    pub secrets: Vec<String>,
}

/// An environment variable's name for one value of a server (stable, so a
/// restart names it the same).
fn env_name(server: &str, part: &str, key: &str, taken: &[(String, String)]) -> String {
    let clean = |s: &str| -> String {
        s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect()
    };
    let base = if key.is_empty() {
        format!("ARUGULA_FTN_{}_{part}", clean(server))
    } else {
        format!("ARUGULA_FTN_{}_{part}_{}", clean(server), clean(key))
    };
    let mut name = base.clone();
    let mut n = 2;
    while taken.iter().any(|(k, _)| *k == name) {
        name = format!("{base}_{n}");
        n += 1;
    }
    name
}

/// What Claude Code's own `${…}` expansion would take hold of.
fn expandable(s: &str) -> bool {
    s.contains("${")
}

const LITERAL: &str =
    "it holds a literal ${…} (Fountain's $$ escape), which Claude Code would expand, so it can't be passed on as it is";

/// One server as the session gets it: `resolved` (substituted) in ACP's
/// shape, with each header and env value (and a URL that had a variable)
/// moved into `env` and replaced by a reference that Claude Code expands
/// itself (S24/M44: it does, in headers, URLs and a stdio server's env).
/// The SDK puts the whole config on `claude`'s command line, where any
/// local user can read it: so only references go there. A variable in a
/// stdio server's command or arguments would land on that server's own
/// command line, so such a server is left out.
fn by_reference(
    name: &str,
    raw: &Value,
    resolved: &Value,
    env: &mut Vec<(String, String)>,
) -> Result<(Value, Value), String> {
    // Why not, from the recipe as written (never from a resolved value).
    acp_server(name, raw)?;
    let full = acp_server(name, resolved)?;
    let mut acp = full.clone();
    let mut added: Vec<(String, String)> = vec![];
    let move_value = |part: &str, key: &str, v: &str, added: &mut Vec<(String, String)>| -> Result<String, String> {
        if expandable(v) {
            return Err(LITERAL.into());
        }
        let all: Vec<(String, String)> = env.iter().chain(added.iter()).cloned().collect();
        let n = env_name(name, part, key, &all);
        added.push((n.clone(), v.to_owned()));
        Ok(format!("${{{n}}}"))
    };
    if acp.get("url").is_some() {
        let url = full["url"].as_str().unwrap_or_default();
        let mut had = BTreeSet::new();
        refs(raw["url"].as_str().unwrap_or_default(), &mut had);
        if !had.is_empty() {
            acp["url"] = json!(move_value("URL", "", url, &mut added)?);
        } else if expandable(url) {
            return Err(LITERAL.into());
        }
        for h in acp["headers"].as_array_mut().into_iter().flatten() {
            let k = h["name"].as_str().unwrap_or_default().to_owned();
            let v = h["value"].as_str().unwrap_or_default().to_owned();
            h["value"] = json!(move_value("H", &k, &v, &mut added)?);
        }
    } else {
        let mut had = BTreeSet::new();
        refs(raw["command"].as_str().unwrap_or_default(), &mut had);
        for a in raw["args"].as_array().into_iter().flatten() {
            refs(a.as_str().unwrap_or_default(), &mut had);
        }
        if !had.is_empty() {
            let vars: Vec<String> = had.iter().map(|v| format!("${{{v}}}")).collect();
            return Err(format!(
                "{} in its command line would be readable by anyone on this machine (ps)",
                vars.join(", ")
            ));
        }
        let cmd = std::iter::once(&full["command"]).chain(full["args"].as_array().into_iter().flatten());
        if cmd.filter_map(Value::as_str).any(expandable) {
            return Err(LITERAL.into());
        }
        for e in acp["env"].as_array_mut().into_iter().flatten() {
            let k = e["name"].as_str().unwrap_or_default().to_owned();
            let v = e["value"].as_str().unwrap_or_default().to_owned();
            e["value"] = json!(move_value("E", &k, &v, &mut added)?);
        }
    }
    env.extend(added);
    Ok((acp, full))
}

/// Turn an agent's servers (by name) into what the session gets:
/// substituted, each credential passed by reference ([`by_reference`]),
/// those that can't come left out with why.
pub async fn servers(mcp_servers: &BTreeMap<String, McpServer>, found: &Found, probe: bool) -> Served {
    let mut out = Served::default();
    let mut probes = vec![];
    let mut fulls: Vec<Value> = vec![];
    let mut keys: Vec<Vec<String>> = vec![];
    for (name, s) in mcp_servers {
        if name == crate::mcp::SERVER_NAME {
            out.left.push(LeftOut { name: name.clone(), why: "Arugula's own server has that name".into() });
            continue;
        }
        let raw = serde_json::to_value(s).unwrap_or_default();
        let mut used = BTreeSet::new();
        refs_in(&raw, &mut used);
        let resolved = match substitute_value(&raw, &found.values) {
            Ok(v) => v,
            Err(missing) => {
                let names: Vec<String> = missing.into_iter().collect();
                let why = match names.as_slice() {
                    [one] => format!("${{{one}}} isn't set (not in Infisical, the shell environment or a helper)"),
                    _ => format!(
                        "{} aren't set (not in Infisical, the shell environment or a helper)",
                        names.iter().map(|n| format!("${{{n}}}")).collect::<Vec<_>>().join(", ")
                    ),
                };
                out.left.push(LeftOut { name: name.clone(), why });
                continue;
            }
        };
        let mut env = out.env.clone();
        let (acp, full) = match by_reference(name, &raw, &resolved, &mut env) {
            Ok(v) => v,
            Err(why) => {
                out.left.push(LeftOut { name: name.clone(), why });
                continue;
            }
        };
        let kind = acp["type"].as_str().unwrap_or("stdio").to_owned();
        if kind != "stdio" && !has_auth(&acp) {
            let url = full["url"].as_str().unwrap_or("").to_owned();
            if let Some(h) = oauth_host(&url) {
                out.left.push(LeftOut { name: name.clone(), why: format!("it needs an OAuth sign-in ({h})") });
                continue;
            }
            if probe {
                probes.push((out.list.len(), url));
            }
        }
        // Values that may be secret: resolved ones, any typed in literally
        // (as M43's recipes hide them), and everything moved to the env.
        for v in used.iter().filter_map(|k| found.values.get(k)) {
            out.secrets.push(v.clone());
        }
        for (_, v) in s.headers.iter().chain(s.env.iter()) {
            if !is_reference(v) {
                out.secrets.push(v.clone());
            }
        }
        let vars = used.iter().map(|k| format!("{k} from {}", found.from.get(k).map_or("?", String::as_str))).collect();
        keys.push(env[out.env.len()..].iter().map(|(k, _)| k.clone()).collect());
        out.env = env;
        out.infos.push(ServerInfo { name: name.clone(), kind, vars });
        out.list.push(acp);
        fulls.push(full);
    }
    // Ask the ones without credentials whether they want an OAuth sign-in.
    let answers = futures_util::future::join_all(probes.iter().map(|(_, url)| probe_oauth(url))).await;
    let gone: BTreeSet<usize> = probes.iter().zip(answers).filter(|(_, oauth)| *oauth).map(|((i, _), _)| *i).collect();
    for i in gone.iter().rev() {
        out.list.remove(*i);
        let full = fulls.remove(*i);
        let info = out.infos.remove(*i);
        // Its references go with it.
        let mine = keys.remove(*i);
        out.env.retain(|(k, _)| !mine.contains(k));
        let host =
            url::Url::parse(full["url"].as_str().unwrap_or("")).ok().and_then(|u| u.host_str().map(str::to_owned));
        out.left.push(LeftOut {
            name: info.name,
            why: format!("it needs an OAuth sign-in{}", host.map(|h| format!(" ({h})")).unwrap_or_default()),
        });
    }
    out.left.sort_by(|a, b| a.name.cmp(&b.name));
    out.secrets.extend(out.env.iter().map(|(_, v)| v.clone()));
    out.secrets.retain(|s| s.len() >= 6);
    out.secrets.sort();
    out.secrets.dedup();
    // A reason never carries a value.
    for l in &mut out.left {
        if let Some(clean) = scrub(&l.why, &out.secrets) {
            l.why = clean;
        }
    }
    out
}

/// Replace every secret in a JSON line with `<redacted>` (as it's escaped
/// in JSON, and as it is).
pub fn scrub(line: &str, secrets: &[String]) -> Option<String> {
    let mut out: Option<String> = None;
    for s in secrets {
        let escaped = serde_json::to_string(s).unwrap_or_default();
        let escaped = escaped.trim_matches('"');
        for needle in [escaped, s.as_str()] {
            if needle.len() < 6 {
                continue;
            }
            let cur = out.as_deref().unwrap_or(line);
            if cur.contains(needle) {
                out = Some(cur.replace(needle, REDACTED));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bundles cached before the rename are used where they are.
    /// Unix: the runner's XDG_CACHE_HOME is a Unix path.
    #[cfg(unix)]
    #[test]
    fn bundles_cached_before_the_rename_stay_put() {
        let tmp = std::env::temp_dir().join(format!("arugula-cache-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let env = vec![("XDG_CACHE_HOME".to_owned(), tmp.display().to_string())];
        assert_eq!(cache_root(&env, Path::new("/nowhere")), tmp.join("arugula/fountain"));
        std::fs::create_dir_all(tmp.join("illogical/fountain")).unwrap();
        assert_eq!(cache_root(&env, Path::new("/nowhere")), tmp.join("illogical/fountain"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn vars(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
        kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    /// `Managoat.Substitution`'s cases.
    #[test]
    fn substitution_as_fountain_does_it() {
        let ok = |s: &str, v: &[(&str, &str)]| substitute(s, &vars(v)).unwrap();
        assert_eq!(ok("hello ${NAME}", &[("NAME", "world")]), "hello world");
        assert_eq!(ok("${FIRST} ${LAST}", &[("FIRST", "John"), ("LAST", "Doe")]), "John Doe");
        assert_eq!(ok("${X} and ${X}", &[("X", "42")]), "42 and 42");
        assert_eq!(ok("plain text", &[]), "plain text");
        assert_eq!(ok("", &[]), "");
        // $$ is a literal $, next to a reference too; $${VAR} escapes one.
        assert_eq!(ok("$$", &[]), "$");
        assert_eq!(ok("$$${AMOUNT}", &[("AMOUNT", "100")]), "$100");
        assert_eq!(ok("$$$$", &[]), "$$");
        assert_eq!(ok("$${KEEP}", &[]), "${KEEP}");
        // Once: a value isn't expanded again.
        assert_eq!(ok("${A}", &[("A", "${B}"), ("B", "no")]), "${B}");
        // Only UPPER_SNAKE names are references.
        assert_eq!(ok("${lower} $HOME ${}", &[]), "${lower} $HOME ${}");
        // Missing: every one, sorted; nothing half-done.
        let err = |s: &str, v: &[(&str, &str)]| substitute(s, &vars(v)).unwrap_err().into_iter().collect::<Vec<_>>();
        assert_eq!(err("${MISSING}", &[]), ["MISSING"]);
        assert_eq!(err("${C} ${A} ${B}", &[]), ["A", "B", "C"]);
        assert_eq!(err("${A} ${B}", &[("A", "present")]), ["B"]);
        // Recursively, across a whole value.
        let v = json!({ "outer": { "inner": "${VAL}" }, "list": ["hello ${NAME}", 1], "num": 42 });
        assert_eq!(
            substitute_value(&v, &vars(&[("VAL", "42"), ("NAME", "world")])).unwrap(),
            json!({ "outer": { "inner": "42" }, "list": ["hello world", 1], "num": 42 })
        );
        let v = json!({ "a": "${FOO}", "b": ["${BAR}"] });
        assert_eq!(substitute_value(&v, &vars(&[])).unwrap_err().into_iter().collect::<Vec<_>>(), ["BAR", "FOO"]);
    }

    #[test]
    fn servers_in_acps_shape() {
        let http = acp_server(
            "github",
            &json!({ "type": "http", "url": "https://api.githubcopilot.com/mcp/", "headers": { "Authorization": "Bearer t" } }),
        )
        .unwrap();
        assert_eq!(
            http,
            json!({ "type": "http", "name": "github", "url": "https://api.githubcopilot.com/mcp/", "headers": [{ "name": "Authorization", "value": "Bearer t" }] })
        );
        assert!(has_auth(&http));
        let stdio =
            acp_server("chant", &json!({ "command": "npx", "args": ["@x/chant", "serve"], "env": { "K": "v" } }))
                .unwrap();
        assert_eq!(
            stdio,
            json!({ "name": "chant", "command": "npx", "args": ["@x/chant", "serve"], "env": [{ "name": "K", "value": "v" }] })
        );
        assert!(acp_server("gmail", &json!({ "connection": "c" })).unwrap_err().contains("Fountain connection"));
        assert!(acp_server("x", &json!({ "type": "ws", "url": "wss://x" })).is_err());
    }

    #[test]
    fn oauth_detection() {
        assert_eq!(oauth_host("https://mcp.mem0.ai/mcp").as_deref(), Some("mcp.mem0.ai"));
        assert_eq!(oauth_host("https://mcp.context7.com/mcp"), None);
        assert!(says_oauth(401, true));
        assert!(!says_oauth(401, false), "a bare 401 is a bad key, not a sign-in");
        assert!(!says_oauth(200, true) && !says_oauth(406, false));
    }

    /// pr-reviewer's MCP servers, as the Fountain fixture has them.
    fn pr_reviewer() -> BTreeMap<String, McpServer> {
        let v: Value = serde_json::from_str(include_str!("../../tests/fixtures/fountain/agents.json")).unwrap();
        let pr = v["data"].as_array().unwrap().iter().find(|a| a["name"] == "pr-reviewer").unwrap();
        serde_json::from_value(pr["mcp_servers"].clone()).unwrap()
    }

    /// pr-reviewer as the fixture has it: github through a variable,
    /// context7 open, mem0 OAuth. Credentials go by reference.
    #[tokio::test]
    async fn servers_resolve_or_are_left_out() {
        let pr = pr_reviewer();
        // The fixture's variables are all ${X}.
        let found = Found { values: vars(&[("X", "fake-secret-value-123")]), from: vars(&[("X", "gh auth token")]) };
        let out = servers(&pr, &found, false).await;
        let names: Vec<&str> = out.list.iter().map(|s| s["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["context7", "github"]);
        assert_eq!(out.list[1]["headers"][0]["value"], "${ARUGULA_FTN_GITHUB_H_AUTHORIZATION}");
        assert_eq!(
            out.env,
            [("ARUGULA_FTN_GITHUB_H_AUTHORIZATION".to_owned(), "Bearer fake-secret-value-123".to_owned())]
        );
        assert!(!serde_json::to_string(&out.list).unwrap().contains("fake-secret"), "only references");
        assert_eq!(out.infos[1].vars, ["X from gh auth token"]);
        assert_eq!(out.left, [LeftOut { name: "mem0".into(), why: "it needs an OAuth sign-in (mcp.mem0.ai)".into() }]);
        assert_eq!(out.secrets, ["Bearer fake-secret-value-123", "fake-secret-value-123"]);
        // Unset: left out, saying which.
        let out = servers(&pr, &Found::default(), false).await;
        assert_eq!(out.list.len(), 1);
        assert_eq!(out.left[0].name, "github");
        assert!(out.left[0].why.contains("${X} isn't set"), "{:?}", out.left);
        assert!(out.secrets.is_empty() && out.env.is_empty());
        // A Fountain connection, Arugula's own name, a variable on a
        // command line, a literal ${…}, a URL with a variable, and a type
        // that's wrong (said from the recipe, not its values).
        let mut odd: BTreeMap<String, McpServer> = BTreeMap::new();
        let mut add = |n: &str, v: Value| {
            odd.insert(n.into(), serde_json::from_value(v).unwrap());
        };
        add("gmail", json!({ "connection": "c" }));
        add("arugula", json!({ "command": "x" }));
        add("argv", json!({ "command": "tool", "args": ["--key", "${X}"] }));
        add("escaped", json!({ "type": "http", "url": "https://h.example.com/mcp", "headers": { "A": "$${KEEP}" } }));
        add(
            "in-url",
            json!({ "type": "http", "url": "https://h.example.com/mcp?k=${X}", "headers": { "Authorization": "t" } }),
        );
        add("weird", json!({ "type": "${X}", "url": "https://h.example.com/mcp" }));
        add("stdio-env", json!({ "command": "tool", "args": ["serve"], "env": { "TOKEN": "${X}" } }));
        let out = servers(&odd, &found, false).await;
        let left: Vec<(&str, &str)> = out.left.iter().map(|l| (l.name.as_str(), l.why.as_str())).collect();
        assert_eq!(left.iter().map(|l| l.0).collect::<Vec<_>>(), ["argv", "arugula", "escaped", "gmail", "weird"]);
        assert!(left[0].1.contains("${X} in its command line"), "{left:?}");
        assert!(left[2].1.contains("literal ${"), "{left:?}");
        assert_eq!(left[4].1, "its type \"${X}\" isn't one Claude Code takes", "the recipe's, not the value");
        let url = out.list.iter().find(|s| s["name"] == "in-url").unwrap();
        assert_eq!(url["url"], "${ARUGULA_FTN_IN_URL_URL}");
        let stdio = out.list.iter().find(|s| s["name"] == "stdio-env").unwrap();
        assert_eq!(stdio["env"][0]["value"], "${ARUGULA_FTN_STDIO_ENV_E_TOKEN}");
        assert!(out.env.iter().any(|(k, v)| k == "ARUGULA_FTN_IN_URL_URL" && v.ends_with("k=fake-secret-value-123")));
        assert!(!serde_json::to_string(&out.list).unwrap().contains("fake-secret"));
        assert!(!format!("{:?}", out.left).contains("fake-secret"));
    }

    /// Infisical through agent-specs: the environment's mapping, the
    /// vault's over it, and a variable neither maps tried as itself.
    // Unix: a fake CLI made executable by its mode.
    #[cfg(unix)]
    #[tokio::test]
    async fn resolving_through_environment_then_vault() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = std::env::temp_dir().join(format!("arugula-resolve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let specs = tmp.join("specs");
        std::fs::create_dir_all(specs.join("dist")).unwrap();
        std::fs::write(specs.join(".infisical.json"), "{}").unwrap();
        std::fs::write(
            specs.join("dist/fountain.yaml"),
            "kind: Environment\nspec:\n  name: e\n  secrets:\n    - key: TOKEN\n      value: \"infisical:///dev/ENV_KEY\"\n    - key: ONLY_ENV\n      value: \"infisical:///dev/ONLY_ENV_KEY\"\n---\nkind: Vault\nspec:\n  name: v\n  secrets:\n    - key: TOKEN\n      value: \"infisical:///dev/VAULT_KEY\"\n",
        )
        .unwrap();
        let fake = tmp.join("infisical");
        std::fs::write(&fake, "#!/bin/sh\necho \"value-of-$3\"\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = vec![("ARUGULA_INFISICAL_BIN".to_owned(), fake.display().to_string())];
        let names: BTreeSet<String> = ["TOKEN", "ONLY_ENV", "OTHER"].map(str::to_owned).into();
        let lookup = |vault: Option<&str>| Lookup {
            env: &env,
            home: &tmp,
            specs: Some(specs.clone()),
            environment: Some("e".into()),
            vault: vault.map(str::to_owned),
        };
        let f = resolve(&names, &lookup(Some("v"))).await;
        assert_eq!(f.values["TOKEN"], "value-of-VAULT_KEY", "the vault wins");
        assert_eq!(f.from["TOKEN"], "Infisical dev/VAULT_KEY");
        assert_eq!(f.values["ONLY_ENV"], "value-of-ONLY_ENV_KEY");
        assert_eq!(f.values["OTHER"], "value-of-OTHER");
        assert_eq!(f.from["OTHER"], "Infisical dev/OTHER (not mapped in agent-specs: tried as itself)");
        let f = resolve(&names, &lookup(None)).await;
        assert_eq!(f.values["TOKEN"], "value-of-ENV_KEY");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn agent_specs_mapping() {
        let yaml = "apiVersion: fountain.dev/v1\nkind: Agent\nmetadata:\n  name: pr-reviewer\nspec:\n  name: pr-reviewer\n  runtime: claude\n  environment: eng\n  mcp_servers:\n    github:\n      headers:\n        Authorization: \"Bearer ${GITHUB_TOKEN}\"\n---\napiVersion: fountain.dev/v1\nkind: Environment\nmetadata:\n  name: eng\nspec:\n  name: eng\n  packages:\n    apt:\n      - jq\n  secrets:\n    - key: GITHUB_TOKEN\n      value: \"infisical:///dev/GITHUB_TOKEN\"\n    - key: DEEP\n      value: 'infisical://proj/prod/apps/x/DEEP_KEY'\n    - key: GIT_AUTHOR_NAME\n      value: someone\n  metadata:\n    managed-by: chant\n---\nkind: Vault\nspec:\n  name: v\n  secrets:\n    - key: GITHUB_TOKEN\n      value: \"infisical:///dev/OTHER\"\n";
        let s = parse_specs(yaml);
        assert_eq!(s.environments["pr-reviewer"], "eng");
        let eng = &s.secrets[&("Environment".to_owned(), "eng".to_owned())];
        assert_eq!(eng["GITHUB_TOKEN"], "infisical:///dev/GITHUB_TOKEN");
        assert_eq!(eng["GIT_AUTHOR_NAME"], "someone");
        assert_eq!(s.secrets[&("Vault".to_owned(), "v".to_owned())]["GITHUB_TOKEN"], "infisical:///dev/OTHER");
        let r = InfisicalRef::parse(&eng["GITHUB_TOKEN"]).unwrap();
        assert_eq!(r, InfisicalRef { project: None, env: "dev".into(), path: "/".into(), key: "GITHUB_TOKEN".into() });
        assert_eq!(r.label(), "Infisical dev/GITHUB_TOKEN");
        let deep = InfisicalRef::parse(&eng["DEEP"]).unwrap();
        assert_eq!(
            (deep.project.as_deref(), deep.env.as_str(), deep.path.as_str(), deep.key.as_str()),
            (Some("proj"), "prod", "/apps/x", "DEEP_KEY")
        );
        assert_eq!(deep.label(), "Infisical prod/apps/x/DEEP_KEY");
        assert_eq!(InfisicalRef::parse("infisical:///dev"), None);
    }

    #[test]
    fn scrubbing() {
        let secrets = vec!["s3cret-\"quoted\"-value".to_owned(), "short".to_owned()];
        let line = json!({ "a": "x s3cret-\"quoted\"-value y", "b": "short" }).to_string();
        let out = scrub(&line, &secrets).unwrap();
        assert!(!out.contains("s3cret") && out.contains("<redacted>"), "{out}");
        assert!(out.contains("short"), "too short to be scrubbed (would wreck the JSON)");
        serde_json::from_str::<Value>(&out).unwrap();
        assert_eq!(scrub("{}", &secrets), None);
    }

    #[test]
    fn literal_values_are_redacted_when_shown() {
        let s: McpServer = serde_json::from_value(serde_json::json!({
            "type": "http", "url": "https://mcp-1.example.com/mcp",
            "headers": { "Authorization": "Bearer ${X}", "X-Api-Key": "typed-in-literally", "X-Mixed": "k=${Y};v=1" },
            "env": { "TOKEN": "also-literal", "REF": "${Z}" }
        }))
        .unwrap();
        let r = s.redacted();
        assert_eq!(r.headers["Authorization"], "Bearer ${X}");
        assert_eq!(r.headers["X-Api-Key"], REDACTED);
        assert_eq!(r.headers["X-Mixed"], "k=${Y};v=1");
        assert_eq!((r.env["TOKEN"].as_str(), r.env["REF"].as_str()), (REDACTED, "${Z}"));
        assert_eq!(s.headers["X-Api-Key"], "typed-in-literally", "kept as read, to wear");
        assert!(!is_reference("${unclosed") && is_reference("a${B}c"));
    }
}
