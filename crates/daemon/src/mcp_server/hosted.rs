//! A cloud workspace's machine's side of M184 (#1245): `POST
//! /api/mcp/call`, which runs a tool another machine's agent asked for
//! here as the owner would ([`call_here`]), and what a machine tool's
//! `cloud_workspace` is ([`take_workspace`], [`with_workspace`]). The
//! driving side is the base's `mcp::hosted`.

use std::sync::Arc;

use axum::{Extension, Json, Router, extract::State, http::StatusCode, routing::post};
use rmcp::model::{CallToolResult, ContentBlock};
use serde::Deserialize;
use serde_json::{Value, json};

use super::tools::ToolCall;
use crate::{
    mcp::{
        Caller, Scope,
        hosted::{CALL, Forwarded, MACHINE_READ},
        tools::{Call, Out, PAGE, PAGE_MAX, done},
    },
    server::App,
};

/// The machine tools that take `workspace`.
pub const TOOLS: &[&str] = &[
    "run",
    "start_agent",
    "prompt_agent",
    "agent_respond",
    "send_input",
    "read_output",
    "read_file",
    "wait",
    "show",
    "close",
    "list",
];

const WORKSPACE_ARG: &str = "A cloud workspace (its hw_… id or its name, from list kind cloud_workspaces): \
do this on its machine instead of here, through Arugula control. Its panes and blocks are that machine's: pass \
cloud_workspace again to reach them.";

/// The argument that names one.
pub const ARG: &str = "cloud_workspace";

/// `schema` with [`ARG`] in it, for the tools in [`TOOLS`] (`wait` has it
/// already, for a Cloud workspace's tab).
pub fn with_workspace(tool: &str, schema: Arc<serde_json::Map<String, Value>>) -> Arc<serde_json::Map<String, Value>> {
    if !TOOLS.contains(&tool) || tool == "wait" {
        return schema;
    }
    let mut s = (*schema).clone();
    if let Some(Value::Object(props)) = s.get_mut("properties") {
        match props.get_mut(ARG) {
            // A grouped tool's kind takes one already (list kind
            // cloud_tabs): said beside it.
            Some(Value::Object(p)) => {
                let was = p.get("description").and_then(Value::as_str).unwrap_or_default().to_owned();
                p.insert(
                    "description".into(),
                    json!(format!("{was} With the other kinds: {WORKSPACE_ARG}").trim().to_owned()),
                );
            }
            _ => {
                props.insert(ARG.into(), json!({ "type": "string", "description": WORKSPACE_ARG }));
            }
        }
    }
    Arc::new(s)
}

/// The cloud workspace a machine tool's call names, taken out of its
/// arguments. A `wait` on a Cloud workspace's tab (`tab`) is `wait`'s own.
pub fn take_workspace(tool: &str, args: &mut Value) -> Option<Result<String, String>> {
    if !TOOLS.contains(&tool) {
        return None;
    }
    let o = args.as_object_mut()?;
    if tool == "wait" && o.get("tab").is_some_and(|t| !t.is_null()) {
        return None;
    }
    // The kind that names a Cloud workspace itself.
    if o.get("kind").and_then(Value::as_str) == Some("cloud_tabs") {
        return None;
    }
    match o.remove(ARG)? {
        Value::Null => None,
        Value::String(s) if !s.trim().is_empty() => Some(Ok(s.trim().to_owned())),
        v => Some(Err(format!("{ARG} is a cloud workspace's id or name, not {v}"))),
    }
}

/// `POST /api/mcp/call`: a tool call another machine makes for an agent of
/// its, on this one (M184: a cloud workspace's machine, driven by one of
/// its project's people's machines). Only the owner here: the machine got
/// in as its person, an owner or an editor of the workspace.
pub fn routes() -> Router<Arc<App>> {
    Router::new().route(CALL, post(call_here))
}

async fn call_here(
    State(app): State<Arc<App>>,
    who: Option<Extension<crate::acl::Principal>>,
    Json(f): Json<Forwarded>,
) -> Result<Json<CallToolResult>, (StatusCode, Json<Value>)> {
    if who.is_some_and(|Extension(p)| !p.is_owner()) {
        return Err((StatusCode::FORBIDDEN, Json(json!({ "error": "only the owner can do that" }))));
    }
    let scope = if f.read_only { Scope::Read } else { Scope::Full };
    let client = if f.client.trim().is_empty() { "another machine".to_owned() } else { f.client.clone() };
    let caller = Caller { scope, token: Some(format!("machine:{client}")), pane: None };
    tracing::info!(tool = %f.tool, client, ?scope, "mcp call from another machine");
    let call = Call::detached(&app, caller, client);
    let call = ToolCall(&call);
    let r = match f.tool.as_str() {
        MACHINE_READ => match machine_read(&call, f.arguments).await {
            Ok(v) => CallToolResult::structured(v),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e)]),
        },
        tool => call.dispatch(tool, f.arguments).await,
    };
    Ok(Json(r))
}

#[derive(Deserialize)]
struct ReadArgs {
    kind: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    staged: bool,
    #[serde(default, rename = "ref")]
    git_ref: Option<String>,
    #[serde(default)]
    max_chars: Option<usize>,
    #[serde(default)]
    pane: Option<u64>,
}

/// At most `max` bytes of `s`, cut on a char boundary, and whether it was.
fn cut(s: &str, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s.to_owned(), false);
    }
    let mut at = max;
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    (s[..at].to_owned(), true)
}

/// This machine's home.
fn home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap_or_else(|| "/".into())
}

/// The repository a git read looks in: `path`, else the home if it's one,
/// else the first repository under it (where setup cloned the spec's).
fn repository(path: Option<&str>) -> Result<std::path::PathBuf, String> {
    repository_in(&home(), path)
}

fn repository_in(home: &std::path::Path, path: Option<&str>) -> Result<std::path::PathBuf, String> {
    if let Some(p) = path.filter(|p| !p.trim().is_empty()) {
        return Ok(p.into());
    }
    if home.join(".git").exists() {
        return Ok(home.to_owned());
    }
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(home)
        .map_err(|e| format!("{}: {e}", home.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join(".git").exists())
        .collect();
    dirs.sort();
    dirs.into_iter().next().ok_or_else(|| format!("no git repository in {}: give path", home.display()))
}

/// git, with nothing a repository's own config could make it run.
async fn git(dir: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let out = tokio::process::Command::new("git")
        .args(["-c", "core.fsmonitor=", "-c", "core.hooksPath=/dev/null", "--no-pager"])
        .args(args)
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .await
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

async fn machine_read(call: &Call<'_>, args: Value) -> Out {
    let a: ReadArgs = serde_json::from_value(args).map_err(|e| format!("bad arguments: {e}"))?;
    if a.kind == "cancel" {
        if call.scope() == Scope::Read {
            return Err("this token may only read".into());
        }
        let pane = a.pane.ok_or("cancel needs the agent block's pane")?;
        call.cancel_block(pane).await?;
        return done(format!("Interrupted %{pane}'s turn."), json!({ "pane": pane }));
    }
    read_here(a).await
}

/// A read of this machine's files and repositories.
async fn read_here(a: ReadArgs) -> Out {
    let max = a.max_chars.unwrap_or(PAGE).clamp(1, PAGE_MAX);
    match a.kind.as_str() {
        "diff" => {
            let dir = repository(a.path.as_deref())?;
            let mut args = vec!["diff", "--no-ext-diff", "--no-textconv", "--no-color", "-M"];
            if a.staged {
                args.push("--staged");
            }
            if let Some(r) = a.git_ref.as_deref() {
                if r.starts_with('-') {
                    return Err(format!("{r:?} isn't a commit, branch or tag"));
                }
                args.push(r);
            }
            args.push("--");
            let root = git(&dir, &["rev-parse", "--show-toplevel"]).await?.trim().to_owned();
            let (diff, truncated) = cut(&git(&dir, &args).await?, max);
            let summary = match diff.is_empty() {
                true => format!("No changes in {root}."),
                false => format!("The diff of {root}{}", if truncated { " (cut short)" } else { "" }),
            };
            done(summary, json!({ "repo_root": root, "diff": diff, "truncated": truncated }))
        }
        "git_status" => {
            let dir = repository(a.path.as_deref())?;
            let root = git(&dir, &["rev-parse", "--show-toplevel"]).await?.trim().to_owned();
            let out = git(&dir, &["status", "--porcelain=v1", "-b", "--untracked-files=all"]).await?;
            let mut lines = out.lines();
            let branch = lines.next().and_then(|l| l.strip_prefix("## ")).map(|b| b.split("...").next().unwrap_or(b));
            let entries: Vec<Value> = lines
                .take(1000)
                .filter(|l| l.len() > 3)
                .map(|l| json!({ "status": l[..2].trim(), "path": &l[3..] }))
                .collect();
            let summary = format!("{root} on {}: {} changed or untracked", branch.unwrap_or("?"), entries.len());
            done(summary, json!({ "repo_root": root, "branch": branch, "entries": entries }))
        }
        "files" => {
            let dir = a.path.filter(|p| !p.trim().is_empty()).map(std::path::PathBuf::from).unwrap_or_else(home);
            let mut entries: Vec<Value> = std::fs::read_dir(&dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?
                .filter_map(|e| e.ok())
                .take(1000)
                .map(|e| {
                    let m = e.metadata().ok();
                    let kind = match &m {
                        Some(m) if m.is_dir() => "dir",
                        Some(m) if m.is_file() => "file",
                        _ => "other",
                    };
                    json!({ "name": e.file_name().to_string_lossy(), "type": kind, "size": m.map(|m| m.len()) })
                })
                .collect();
            entries.sort_by(|x, y| x["name"].as_str().cmp(&y["name"].as_str()));
            let n = entries.len();
            done(
                format!("{}: {n} entr{}", dir.display(), if n == 1 { "y" } else { "ies" }),
                json!({ "path": dir, "entries": entries }),
            )
        }
        "file" => {
            let path = a.path.ok_or("file needs its path")?;
            let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
            if bytes.iter().take(8000).any(|b| *b == 0) {
                return Err(format!("{path} isn't text"));
            }
            let size = bytes.len();
            let (content, truncated) = cut(&String::from_utf8_lossy(&bytes), max);
            let summary = format!("{path}: {size} bytes{}", if truncated { ", the start of it" } else { "" });
            done(summary, json!({ "path": path, "size": size, "content": content, "truncated": truncated }))
        }
        k => Err(format!("no machine read {k}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::hosted::tab_pane;

    #[test]
    fn workspace_goes_on_the_machine_tools_only() {
        let mut args = json!({ "command": "ls", "cloud_workspace": " dev " });
        assert_eq!(take_workspace("run", &mut args), Some(Ok("dev".into())));
        assert_eq!(args, json!({ "command": "ls" }), "taken out");
        assert_eq!(take_workspace("run", &mut json!({ "command": "ls" })), None);
        assert_eq!(take_workspace("run", &mut json!({ "cloud_workspace": null })), None);
        assert!(matches!(take_workspace("run", &mut json!({ "cloud_workspace": 3 })), Some(Err(_))));
        let mut other = json!({ "cloud_workspace": "dev" });
        assert_eq!(take_workspace("cloud", &mut other), None, "not a machine tool");
        assert_eq!(other["cloud_workspace"], "dev");
        // wait: a pane on a hosted one's machine, by id or name; a tab is
        // wait's own.
        let mut w = json!({ "cloud_workspace": "hw_ab", "pane": 3, "until": "idle" });
        assert_eq!(take_workspace("wait", &mut w), Some(Ok("hw_ab".into())));
        let mut w = json!({ "cloud_workspace": "dev", "pane": 3, "until": "idle" });
        assert_eq!(take_workspace("wait", &mut w), Some(Ok("dev".into())));
        assert_eq!(take_workspace("wait", &mut json!({ "cloud_workspace": "hw_ab", "tab": "3" })), None);
        // list kind cloud_tabs names one itself.
        assert_eq!(take_workspace("list", &mut json!({ "kind": "cloud_tabs", "cloud_workspace": "hw_ab" })), None);
        assert!(take_workspace("list", &mut json!({ "kind": "panes", "cloud_workspace": "hw_ab" })).is_some());

        let schema = Arc::new(json!({ "type": "object", "properties": { "pane": {} } }).as_object().unwrap().clone());
        assert!(with_workspace("read_output", schema.clone())["properties"]["cloud_workspace"].is_object());
        assert!(with_workspace("cloud", schema)["properties"].get("cloud_workspace").is_none());

        assert_eq!(tab_pane("%7").unwrap(), json!(7));
        assert!(tab_pane("cwc_ab").is_err());
    }

    /// The reads a driving machine makes for read_cloud: a repository's
    /// diff and status (the first one under home without a path), a
    /// directory, a file; what isn't text, isn't.
    // Hosted boxes are Linux: its paths and shell.
    #[cfg(unix)]
    #[tokio::test]
    async fn reads_here() {
        let dir = std::env::temp_dir().join(format!("arugula-hosted-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("app");
        std::fs::create_dir_all(&repo).unwrap();
        let sh = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        sh(&["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        sh(&["add", "."]);
        sh(&["commit", "-qm", "first"]);
        std::fs::write(repo.join("a.txt"), "two\n").unwrap();
        std::fs::write(repo.join("new.txt"), "new\n").unwrap();
        std::fs::write(repo.join("bin"), b"\0\x01").unwrap();
        let read = |kind: &str, path: Option<&std::path::Path>| ReadArgs {
            kind: kind.into(),
            path: path.map(|p| p.display().to_string()),
            staged: false,
            git_ref: None,
            max_chars: None,
            pane: None,
        };
        let repo_root = repo.canonicalize().unwrap().display().to_string();
        let v = read_here(read("diff", Some(&repo))).await.unwrap();
        assert!(v["diff"].as_str().unwrap().contains("+two"), "{v}");
        assert_eq!(v["repo_root"], repo_root);
        let v = read_here(read("git_status", Some(&repo))).await.unwrap();
        assert_eq!(v["branch"], "main");
        let paths: Vec<&str> = v["entries"].as_array().unwrap().iter().filter_map(|e| e["path"].as_str()).collect();
        assert!(paths.contains(&"a.txt") && paths.contains(&"new.txt"), "{v}");
        let v = read_here(read("files", Some(&repo))).await.unwrap();
        assert!(v["entries"].as_array().unwrap().iter().any(|e| e["name"] == "a.txt" && e["type"] == "file"));
        let v = read_here(read("file", Some(&repo.join("a.txt")))).await.unwrap();
        assert_eq!(v["content"], "two\n");
        assert!(read_here(read("file", Some(&repo.join("bin")))).await.unwrap_err().contains("isn't text"));
        let mut bad = read("diff", Some(&repo));
        bad.git_ref = Some("--output=/tmp/x".into());
        assert!(read_here(bad).await.is_err(), "no option for a ref");
        // Without a path: the first repository under home.
        assert_eq!(repository_in(&dir, None).unwrap(), repo);
        assert!(repository_in(&repo.join("nothing"), None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
