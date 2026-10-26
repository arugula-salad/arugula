//! M16: Arugula as MCP tools, against the real daemon. An rmcp client
//! through `arugula mcp` (the stdio bridge on the Unix socket) and over
//! HTTP with client tokens; and an agent block (`fake_acp.py`) using the
//! server Arugula hands it, scoped to its tab: a dev server and a browser
//! block beside itself, other tabs refused, and a second agent started,
//! waited on and answered. With wisp on this host (skipped without its
//! token), an agent block in a VM too, through the relay the daemon opens
//! into it (#59).

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use crate::agentd;
use crate::replay;

use std::{
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use agentd::*;
use rmcp::{
    ClientHandler, RoleClient, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, Implementation, NumberOrString,
        ProgressNotificationParam, ProgressToken, RequestMetaObject,
    },
    service::{NotificationContext, RunningService},
    transport::{
        StreamableHttpClientTransport, TokioChildProcess, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};

/// The forge's two tools, which a build without the forge doesn't list.
const NO_FORGE: usize = if cfg!(feature = "forge") { 0 } else { 2 };

fn cli_bin() -> PathBuf {
    let bin = Path::new(env!("CARGO_BIN_EXE_arugulad")).with_file_name("arugula");
    let status = Command::new(env!("CARGO")).args(["build", "-q", "-p", "arugula"]).status().unwrap();
    assert!(status.success(), "building the CLI");
    bin
}

/// An MCP client that names itself, and counts progress notifications.
#[derive(Clone)]
struct Client {
    name: &'static str,
    progress: Arc<AtomicUsize>,
}

impl Client {
    fn named(name: &'static str) -> Self {
        Self { name, progress: Default::default() }
    }
}

impl ClientHandler for Client {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(ClientCapabilities::default(), Implementation::new(self.name, "1"))
    }

    async fn on_progress(&self, _p: ProgressNotificationParam, _c: NotificationContext<RoleClient>) {
        self.progress.fetch_add(1, Ordering::Relaxed);
    }
}

type Session = RunningService<RoleClient, Client>;

/// `arugula mcp` against the daemon's socket, as Claude Code or Codex
/// would start it.
async fn bridge(d: &Daemon, client: Client) -> Session {
    bridge_in(d, client, None).await
}

/// The same, started from a terminal pane (`$ARUGULA_PANE`), or from
/// none: not the pane this test runs in, if it runs in one.
async fn bridge_in(d: &Daemon, client: Client, pane: Option<u64>) -> Session {
    let mut cmd = tokio::process::Command::new(cli_bin());
    arugula_testkit::scrub_env(cmd.as_std_mut());
    cmd.arg("--socket").arg(d.sock()).arg("mcp").env_remove("ARUGULA_PANE");
    if let Some(p) = pane {
        cmd.env("ARUGULA_PANE", p.to_string());
    }
    client.serve(TokioChildProcess::new(cmd).unwrap()).await.expect("connecting through arugula mcp")
}

/// `/mcp` over TCP, with a bearer token.
async fn http(d: &Daemon, token: &str, client: Client) -> Result<Session, String> {
    let config =
        StreamableHttpClientTransportConfig::with_uri(format!("http://127.0.0.1:{}/mcp", d.port)).auth_header(token);
    client.serve(StreamableHttpClientTransport::from_config(config)).await.map_err(|e| e.to_string())
}

async fn call_raw(s: &Session, tool: &str, args: Value) -> CallToolResult {
    let params =
        CallToolRequestParams::new(tool.to_owned()).with_arguments(args.as_object().cloned().unwrap_or_default());
    let mut params = params;
    params.meta = Some(RequestMetaObject::with_progress_token(ProgressToken(NumberOrString::String("t".into()))));
    s.call_tool(params).await.unwrap_or_else(|e| panic!("{tool}: {e}"))
}

/// A tool's structured result; panics if it failed.
async fn call(s: &Session, tool: &str, args: Value) -> Value {
    let r = call_raw(s, tool, args.clone()).await;
    assert_ne!(r.is_error, Some(true), "{tool} {args}: {:?}", r.content);
    let v = r.structured_content.expect("structured content");
    assert!(v["summary"].is_string(), "every result says what happened: {v}");
    v
}

/// A tool's error sentence; panics if it worked.
async fn refused(s: &Session, tool: &str, args: Value) -> String {
    let r = call_raw(s, tool, args.clone()).await;
    assert_eq!(r.is_error, Some(true), "{tool} {args} should fail: {:?}", r.structured_content);
    r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect()
}

/// The kinds a grouped tool lists (`show`'s, say).
fn kinds(tools: &[rmcp::model::Tool], tool: &str) -> Vec<String> {
    let t = tools.iter().find(|t| t.name == tool).unwrap_or_else(|| panic!("no {tool}"));
    let kinds = t.input_schema.get("properties").and_then(|p| p.get("kind")).and_then(|k| k.get("enum"));
    kinds.and_then(Value::as_array).into_iter().flatten().filter_map(|k| k.as_str().map(str::to_owned)).collect()
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[tokio::test(flavor = "multi_thread")]
async fn tools_through_the_stdio_bridge() {
    let d = Daemon::child_env(&["--wisp-token-file", "/nonexistent"], &[("ARUGULA_MCP_PROGRESS_MS", "200")]);
    let client = Client::named("claude-code");
    let progress = client.progress.clone();
    let s = bridge(&d, client).await;

    // The tools, with honest annotations: grouped by kind (#349).
    let tools = s.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 32 - NO_FORGE);
    // Chant workspaces aren't listed without their flag,
    // and a caller who names one is told how to turn it on (#347); a build
    // without Labs says it isn't built. Daemon-side Managoat was removed: its
    // kinds aren't listed, and a caller who names one is told where cloud
    // agents went. The thread tools are control's conversations (M198),
    // everyone's.
    for name in ["read_thread", "search_threads", "post_thread"] {
        assert!(tools.iter().any(|t| t.name == name), "{name} isn't listed");
    }
    assert!(!kinds(&tools, "show").contains(&"workspace".to_owned()), "show kind workspace is listed");
    // An app's box, by its address, is everyone's (native #206).
    assert!(kinds(&tools, "show").contains(&"app".to_owned()), "show kind app isn't listed");
    for (tool, kind) in [("show", "managoat"), ("list", "managoat_agents"), ("list", "managoat_agent")] {
        assert!(!kinds(&tools, tool).contains(&kind.to_owned()), "{tool} kind {kind}");
    }
    let said = refused(&s, "list", json!({ "kind": "managoat_agent", "name": "nobody" })).await;
    assert!(said.contains("Cloud workspaces in your Arugula projects") && said.contains("cloud_workspaces"), "{said}");
    let ro = |n: &str| tools.iter().find(|t| t.name == n).unwrap().annotations.as_ref().unwrap().read_only_hint;
    assert_eq!(
        (ro("read_output"), ro("wait"), ro("run"), ro("close")),
        (Some(true), Some(true), Some(false), Some(false))
    );

    // Run and wait: the exit code and the last lines.
    let dir = d.sessions.join("repo");
    std::fs::create_dir_all(&dir).unwrap();
    let r = call(&s, "run", json!({ "command": "echo built-$((6*7))", "cwd": dir, "wait": true, "timeout": 20 })).await;
    assert_eq!((r["state"].as_str(), r["exit"].as_i64()), (Some("done"), Some(0)), "{r}");
    assert!(r["last_lines"].as_str().unwrap().contains("built-42"), "{r}");
    let pane = r["pane"].as_u64().unwrap();

    // It says who started it, in the pane and in history.
    let info = d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == pane).cloned().unwrap();
    assert_eq!(info["started_by"]["by"], "mcp:claude-code", "{info}");
    let failed = call(&s, "run", json!({ "command": "ls /nonexistent-m16", "cwd": dir, "wait": true })).await;
    // GNU ls exits 2 for a missing path, BSD ls (macOS) 1.
    let ls = Command::new("ls").arg("/nonexistent-m16").output().unwrap().status.code();
    assert_eq!(failed["exit"].as_i64(), ls.map(i64::from), "{failed}");
    let h = call(&s, "history", json!({ "failed": true, "cwd": dir })).await;
    let cmds = h["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 1, "{h}");
    assert_eq!(
        (cmds[0]["command"].as_str(), cmds[0]["by"].as_str()),
        (Some("ls /nonexistent-m16"), Some("mcp:claude-code"))
    );
    assert_eq!(cmds[0]["kind"], "command", "{h}");
    // Nothing here is an answer, and a kind that doesn't exist says so.
    let h = call(&s, "history", json!({ "kind": "answer", "cwd": dir })).await;
    assert!(h["commands"].as_array().unwrap().is_empty(), "{h}");
    let h = call(&s, "history", json!({ "kind": "command", "cwd": dir })).await;
    assert!(!h["commands"].as_array().unwrap().is_empty(), "{h}");
    let e = refused(&s, "history", json!({ "kind": "nope" })).await;
    assert!(e.contains("command, answer, agent or action"), "{e}");

    // Paged output: a long command's, a page at a time, all of it.
    let r = call(&s, "run", json!({ "command": "seq 1 4000", "wait": true })).await;
    let p = r["pane"].as_u64().unwrap();
    let mut text = String::new();
    let mut page = call(&s, "read_output", json!({ "pane": p, "last_command": true, "max_chars": 1000 })).await;
    let mut pages = 1;
    while page["more"] == true {
        text.push_str(page["text"].as_str().unwrap());
        assert!(page["text"].as_str().unwrap().len() <= 1000);
        page = call(
            &s,
            "read_output",
            json!({ "pane": p, "last_command": true, "offset": page["next_offset"], "max_chars": 1000 }),
        )
        .await;
        pages += 1;
    }
    text.push_str(page["text"].as_str().unwrap());
    let want: String = (1..=4000).map(|n| format!("{n}\n")).collect();
    assert!(pages > 10, "{pages} pages");
    assert_eq!(text.trim_start_matches("seq 1 4000\n"), want);

    // A long call: progress while it waits, then "still running" with
    // where to pick up; the agent calls again.
    let r = call(&s, "run", json!({ "command": "sleep 30" })).await;
    let p = r["pane"].as_u64().unwrap();
    // Running, not only typed: a ^C while bash is still expanding PS0 (its
    // command-start mark) cancels the line without a command end to wait
    // for.
    d.wait_for("sleep 30 to start", || {
        d.get("/api/panes").as_array().unwrap().iter().any(|x| x["id"] == p && x["current"]["text"] == "sleep 30")
    });
    let before = progress.load(Ordering::Relaxed);
    let w = call(&s, "wait", json!({ "pane": p, "until": "command_end", "timeout": 1.5 })).await;
    assert_eq!(w["state"], "still running", "{w}");
    assert!(w["next_offset"].is_u64() && w["summary"].as_str().unwrap().contains("sleep 30"), "{w}");
    assert!(progress.load(Ordering::Relaxed) > before, "progress while waiting");
    call(&s, "send_input", json!({ "pane": format!("%{p}"), "keys": ["C-c"] })).await;
    let w = call(&s, "wait", json!({ "pane": p, "until": "command_end", "timeout": 10 })).await;
    assert_eq!((w["state"].as_str(), w["exit"].as_i64()), (Some("done"), Some(130)), "{w}");

    // Typing, and matching what comes out.
    call(&s, "send_input", json!({ "pane": p, "text": "echo typed-$((2+2))" })).await;
    let m = call(&s, "wait", json!({ "pane": p, "until": "match", "pattern": "typed-4", "timeout": 10 })).await;
    assert_eq!(m["match"], "typed-4", "{m}");
    let screen = call(&s, "read_output", json!({ "pane": p, "screen": true })).await;
    assert!(screen["text"].as_str().unwrap().contains("typed-4"), "{screen}");
    // Its name before #349 is gone.
    assert!(tools.iter().all(|t| t.name != "capture_screen"));
    assert!(refused(&s, "capture_screen", json!({ "pane": p })).await.contains("no tool"));

    // list, search, and the resources.
    let l = call(&s, "list", json!({})).await;
    assert!(
        l["panes"].as_array().unwrap().iter().any(|e| e["pane"] == p && e["started_by"] == "mcp:claude-code"),
        "{l}"
    );
    let hits = call(&s, "history", json!({ "kind": "output", "pattern": "built-42" })).await;
    assert!(hits["hits"].as_array().unwrap().iter().any(|h| h["pane"] == pane), "{hits}");
    // A grouped tool says what's wrong with a call.
    let why = refused(&s, "show", json!({ "port": 1 })).await;
    assert!(why.contains("show needs a kind"), "{why}");
    let why = refused(&s, "history", json!({ "kind": "output", "pattern": "x", "failed": true })).await;
    assert!(why.contains("failed isn't an argument of history kind output"), "{why}");
    let templates = s.list_all_resource_templates().await.unwrap();
    assert_eq!(templates.len(), 3);
    let res = s
        .read_resource(rmcp::model::ReadResourceRequestParams::new(format!("arugula://pane/{pane}/output")))
        .await
        .unwrap();
    assert!(format!("{:?}", res.contents).contains("built-42"));

    // A file.
    std::fs::write(dir.join("notes.txt"), "line one\nline two\n").unwrap();
    let f = call(&s, "read_file", json!({ "path": "notes.txt", "pane": pane })).await;
    assert_eq!(f["text"], "line one\nline two\n", "{f}");

    // Closed: an error that says what happened, and its output still reads.
    call(&s, "close", json!({ "pane": pane })).await;
    d.wait_for("it to close", || !d.get("/api/panes").as_array().unwrap().iter().any(|x| x["id"] == pane));
    let why = refused(&s, "wait", json!({ "pane": pane, "until": "exit" })).await;
    assert!(why.contains(&format!("pane %{pane} is gone")) && why.contains("exited 0"), "{why}");
    let gone = call(&s, "read_output", json!({ "pane": pane })).await;
    assert!(gone["text"].as_str().unwrap().contains("built-42"), "{gone}");
    s.cancel().await.unwrap();
}

/// #580: a tool that always answers with one type lists it as its
/// `outputSchema`, and a client may refuse an answer that doesn't fit it. So
/// what the real server sends fits what the real server lists, in each shape
/// `run` answers with, and in the others.
#[tokio::test(flavor = "multi_thread")]
async fn what_a_tool_answers_fits_the_output_schema_it_lists() {
    let d = Daemon::child();
    let s = bridge(&d, Client::named("claude-code")).await;
    let tools = s.list_all_tools().await.unwrap();
    // The grouped tools and the ones that answer in more than one shape list none.
    for name in ["list", "show", "send_input", "attach", "read_output", "wait", "history", "prompt_agent"] {
        let t = tools.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("no {name}"));
        assert!(t.output_schema.is_none(), "{name} lists an outputSchema");
    }
    let schema = |name: &str| {
        let t = tools.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("no {name}"));
        let schema = t.output_schema.as_ref().unwrap_or_else(|| panic!("{name} lists no outputSchema"));
        assert_eq!(schema.get("type"), Some(&json!("object")), "{name}: MCP wants an object at the root");
        Value::Object((**schema).clone())
    };
    let fits = |name: &str, answer: &Value| {
        if let Err(e) = arugula_testkit::schema::conforms(answer, &schema(name)) {
            panic!("{name} answered {answer}, which doesn't fit its outputSchema: {e}");
        }
    };

    let dir = d.sessions.join("repo");
    std::fs::create_dir_all(&dir).unwrap();
    let started = call(&s, "run", json!({ "cwd": dir })).await;
    assert_eq!(started["state"], "started", "{started}");
    fits("run", &started);
    let done =
        call(&s, "run", json!({ "command": "echo fits-$((6*7))", "cwd": dir, "wait": true, "timeout": 20 })).await;
    assert_eq!(done["state"], "done", "{done}");
    fits("run", &done);
    let slow = call(&s, "run", json!({ "command": "sleep 30", "cwd": dir, "wait": true, "timeout": 1 })).await;
    assert_eq!(slow["state"], "still running", "{slow}");
    fits("run", &slow);

    std::fs::write(dir.join("notes.txt"), "line one\nline two\n").unwrap();
    let pane = done["pane"].as_u64().unwrap();
    fits("read_file", &call(&s, "read_file", json!({ "path": "notes.txt", "pane": pane })).await);
    for p in [&started, &done, &slow] {
        fits("close", &call(&s, "close", json!({ "pane": p["pane"] })).await);
    }

    s.cancel().await.unwrap();
}

/// `show`'s blocks (#349): a diff of a repository and a file at a line, by
/// kind, as show_changes and show_file did.
#[tokio::test(flavor = "multi_thread")]
async fn show_opens_each_kind_of_block() {
    let d = Daemon::child();
    let s = bridge(&d, Client::named("claude-code")).await;
    let repo = d.sessions.join("shown");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-qm", "a"]);
    std::fs::write(repo.join("a.txt"), "one\ntwo\nthree\n").unwrap();

    let c = call(&s, "show", json!({ "kind": "changes", "repo": repo })).await;
    let files = c["files"].as_array().unwrap();
    assert_eq!((files.len(), files[0]["path"].as_str(), files[0]["add"].as_u64()), (1, Some("a.txt"), Some(1)), "{c}");
    let diff = d.get(&format!("/api/blocks/{}", c["block"]))["info"]["type"].clone();
    assert_eq!(diff, "diff");

    let path = repo.join("a.txt").display().to_string();
    let f = call(&s, "show", json!({ "kind": "file", "path": path, "line": 2 })).await;
    let info = d.get(&format!("/api/blocks/{}", f["block"]))["info"].clone();
    assert_eq!(info["type"], "file", "{info}");
    assert!(f["summary"].as_str().unwrap().contains("at line 2"), "{f}");

    // And a PR or issue block is what read_forge and draft take.
    let why = refused(&s, "read_forge", json!({ "block": f["block"] })).await;
    assert!(why.contains("isn't a PR or issue block"), "{why}");
    let why = refused(&s, "draft", json!({ "kind": "comment", "block": f["block"], "body": "hi" })).await;
    assert!(why.contains("isn't a PR or issue block"), "{why}");
    s.cancel().await.unwrap();
}

/// As Claude Code speaks it (2026-07-28, stateless, `server/discover`), on
/// the daemon's socket.
#[tokio::test(flavor = "multi_thread")]
async fn stateless_clients_get_the_cache_hints_claude_code_wants() {
    use rmcp::{
        ClientLifecycleMode, ClientServiceExt,
        model::{CacheScope, ProtocolVersion},
    };
    let d = Daemon::child();
    let sock = d.sock().display().to_string();
    let transport = StreamableHttpClientTransport::from_unix_socket(sock.as_str(), "http://localhost/mcp");
    let s = Client::named("claude-code")
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Discover { preferred_versions: vec![ProtocolVersion::V_2026_07_28] },
        )
        .await
        .unwrap();
    // Without ttlMs and cacheScope, Claude Code 2.1.287 refuses the list
    // (and retries it, then gives up: no tools).
    let tools = s.list_tools(None).await.unwrap();
    assert_eq!((tools.ttl_ms, tools.cache_scope), (Some(0), Some(CacheScope::Private)));
    assert_eq!(tools.tools.len(), 32 - NO_FORGE, "without labs");
    let t = s.list_resource_templates(None).await.unwrap();
    assert_eq!((t.ttl_ms, t.cache_scope), (Some(0), Some(CacheScope::Private)));
    let r = call(&s, "run", json!({ "command": "echo stateless", "wait": true })).await;
    assert_eq!(r["exit"], 0);
    let info = d.get(&format!("/api/blocks/{}", r["pane"]))["info"].clone();
    assert_eq!(info["started_by"]["by"], "mcp:claude-code", "the name from each request's _meta");
    s.cancel().await.unwrap();
}

/// #379: a Claude Code with a login of its own (`CLAUDE_CONFIG_DIR`, as a
/// second account has) starts agents through `arugula mcp` that use that
/// login, not the daemon's default one.
#[tokio::test(flavor = "multi_thread")]
async fn start_agent_gives_the_agent_its_callers_login() {
    let d = Daemon::child();
    let theirs = d.sessions.join("second-account");
    let mut cmd = tokio::process::Command::new(cli_bin());
    arugula_testkit::scrub_env(cmd.as_std_mut());
    cmd.arg("--socket").arg(d.sock()).arg("mcp").env_remove("ARUGULA_PANE").env("CLAUDE_CONFIG_DIR", &theirs);
    let s = Client::named("claude-code").serve(TokioChildProcess::new(cmd).unwrap()).await.unwrap();
    let args = json!({ "agent": "acp", "command": format!("python3 {}", fake()), "prompt": "env CLAUDE_CONFIG_DIR",
        "cwd": d.sessions });
    let b = call(&s, "start_agent", args).await["block"].as_u64().unwrap();
    call(&s, "wait", json!({ "pane": b, "until": "idle", "timeout": 30 })).await;
    let t = call(&s, "read_output", json!({ "pane": b })).await;
    let want = format!("ENV CLAUDE_CONFIG_DIR={}", theirs.display());
    assert!(t["text"].as_str().unwrap().contains(&want), "{t}");
    s.cancel().await.unwrap();
}

/// Runs `git ARGS` in `dir` (a throwaway repo), as a test identity.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// AR-2 (#1476): `start_agent {worktree}` makes a worktree and branch and
/// runs the agent in it; `close` keeps it while a file is uncommitted, and
/// removes it, and its local branch, once the branch is merged into its
/// base (a bare repo as `origin`).
#[tokio::test(flavor = "multi_thread")]
async fn start_agent_owns_a_worktree_and_close_keeps_unmerged_work() {
    let d = Daemon::child();
    // git prints real paths: macOS's temp dir is under a symlink.
    let tmp = std::env::temp_dir().join(format!("arugula-ar2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let tmp = tmp.canonicalize().unwrap();
    git(&tmp, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
    git(&tmp, &["init", "-q", "-b", "main", "work"]);
    let work = tmp.join("work");
    git(&work, &["commit", "-q", "--allow-empty", "-m", "one"]);
    git(&work, &["remote", "add", "origin", tmp.join("origin.git").to_str().unwrap()]);
    git(&work, &["push", "-q", "origin", "main"]);

    let s = bridge(&d, Client::named("claude-code")).await;
    let start = |branch: &str, prompt: &str| {
        json!({ "agent": "acp", "command": format!("python3 {}", fake()), "prompt": prompt,
            "cwd": work, "worktree": { "branch": branch } })
    };

    // An agent in a worktree of its own: its file lands there.
    let r = call(&s, "start_agent", start("feat-a", "write dirty.txt")).await;
    let a = r["block"].as_u64().unwrap();
    let wa = work.join(".arugula/worktrees/feat-a");
    assert!(r["summary"].as_str().unwrap().contains(wa.to_str().unwrap()), "{r}");
    call(&s, "wait", json!({ "pane": a, "until": "idle", "timeout": 30 })).await;
    assert_eq!(d.state(a)["cwd"], wa.to_str().unwrap());
    assert!(wa.join("dirty.txt").exists());

    // The same branch again is refused, not reused.
    let why = refused(&s, "start_agent", start("feat-a", "hello")).await;
    assert!(why.contains("exists already"), "{why}");

    // An uncommitted file: closed, but kept, with the reason.
    let r = call(&s, "close", json!({ "pane": a })).await;
    assert_eq!(r["worktree"]["outcome"], "kept", "{r}");
    assert!(r["worktree"]["why"].as_str().unwrap().contains("1 uncommitted file"), "{r}");
    assert!(r["summary"].as_str().unwrap().contains("kept worktree"), "{r}");
    assert!(wa.join("dirty.txt").exists());
    assert!(!git(&work, &["branch", "--list", "feat-a"]).is_empty());

    // Merged into origin's main: removed, with its local branch.
    let r = call(&s, "start_agent", start("feat-b", "hello")).await;
    let b = r["block"].as_u64().unwrap();
    let wb = work.join(".arugula/worktrees/feat-b");
    call(&s, "wait", json!({ "pane": b, "until": "idle", "timeout": 30 })).await;
    git(&wb, &["commit", "-q", "--allow-empty", "-m", "work"]);
    git(&wb, &["push", "-q", "origin", "feat-b:feat-b"]);
    let r = call(&s, "close", json!({ "pane": b })).await;
    assert_eq!(r["worktree"]["outcome"], "kept", "pushed, but not merged yet: {r}");
    assert!(r["worktree"]["why"].as_str().unwrap().contains("not merged"), "{r}");
    let c = call(&s, "start_agent", start("feat-c", "hello")).await["block"].as_u64().unwrap();
    let wc = work.join(".arugula/worktrees/feat-c");
    call(&s, "wait", json!({ "pane": c, "until": "idle", "timeout": 30 })).await;
    git(&wc, &["commit", "-q", "--allow-empty", "-m", "work"]);
    git(&wc, &["push", "-q", "origin", "feat-c:feat-c"]);
    git(&wc, &["push", "-q", "origin", "feat-c:main"]);
    let r = call(&s, "close", json!({ "pane": c })).await;
    assert_eq!(r["worktree"]["outcome"], "removed", "{r}");
    assert!(!wc.exists());
    assert!(git(&work, &["branch", "--list", "feat-c"]).is_empty());
    // ...and origin's branch is not touched.
    assert!(!git(&tmp.join("origin.git"), &["branch", "--list", "feat-c"]).is_empty());

    s.cancel().await.unwrap();
    // Git won't remove a worktree with work in it; the test does.
    let _ = std::fs::remove_dir_all(&tmp);
}

/// #629: `start_agent {title}` names the block.
#[tokio::test(flavor = "multi_thread")]
async fn start_agent_names_the_block() {
    let d = Daemon::child();
    let s = bridge(&d, Client::named("claude-code")).await;
    let args = json!({ "agent": "acp", "command": format!("python3 {}", fake()), "prompt": "hello",
        "title": "Parser lane", "cwd": d.sessions });
    let b = call(&s, "start_agent", args).await["block"].as_u64().unwrap();
    call(&s, "wait", json!({ "pane": b, "until": "idle", "timeout": 30 })).await;
    assert_eq!(d.state(b)["title"], "Parser lane");
    s.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_bridge_outlives_a_daemon_restart() {
    let mut d = Daemon::child();
    let s = bridge(&d, Client::named("codex")).await;
    call(&s, "list", json!({})).await;
    // Its session is gone with the daemon; the bridge opens another.
    d.stop();
    d.start();
    let l = call(&s, "list", json!({})).await;
    assert!(l["panes"].is_array());
    s.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn http_with_a_token_until_it_is_revoked() {
    let d = Daemon::child();
    // A client on another machine: a token of its own.
    let t = d.post("/api/mcp/tokens", json!({ "name": "laptop" }));
    let token = t["token"].as_str().unwrap().to_owned();
    assert!(!d.get("/api/mcp/tokens").to_string().contains(&token), "listing never shows a token");
    let s = http(&d, &token, Client::named("claude-code")).await.unwrap();
    let r = call(&s, "run", json!({ "command": "echo from-afar", "wait": true })).await;
    assert_eq!(r["exit"], 0);
    let used = d.get("/api/mcp/tokens");
    assert!(used[0]["used_ms"].is_u64(), "{used}");

    // Read-only tokens see and call the read-only tools only.
    let ro =
        d.post("/api/mcp/tokens", json!({ "name": "watcher", "scope": "read" }))["token"].as_str().unwrap().to_owned();
    let w = http(&d, &ro, Client::named("watcher")).await.unwrap();
    // (read_forge is among the read-only tools only with the forge.)
    assert_eq!(w.list_all_tools().await.unwrap().len(), 14 - NO_FORGE / 2);
    assert!(refused(&w, "run", json!({ "command": "true" })).await.contains("may only read"));
    // Grouped, a write is still a write.
    assert!(refused(&w, "show", json!({ "kind": "port", "port": 1 })).await.contains("may only read"));
    call(&w, "list", json!({ "kind": "devices" })).await;
    // #234: nor ask to invite anyone.
    let invite = json!({ "who": "tailnet:sam@example.com", "pane": 1, "note": "x" });
    assert!(refused(&w, "invite_person", invite).await.contains("may only read"));
    call(&w, "list", json!({})).await;

    // Revoked: cut off at its next call, and it can't start again.
    let (status, _) = d.raw("DELETE", "/api/mcp/tokens/laptop", None);
    assert_eq!(status, 200);
    assert!(s.call_tool(CallToolRequestParams::new("list")).await.is_err(), "revoked mid-session");
    assert!(http(&d, &token, Client::named("claude-code")).await.is_err());
    assert!(http(&d, "ilm_0000", Client::named("claude-code")).await.is_err(), "an unknown token");
    // Something that isn't a bearer token is no way around the owner's check.
    assert_eq!(post_mcp(d.port, "Basic b3duZXI6eA=="), 401);
    assert_eq!(post_mcp(d.port, "Bearer "), 401);
    call(&w, "list", json!({})).await;

    // A web page elsewhere can't use it from a browser (exact Origin).
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"x","version":"1"}}}"#;
    let res = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{}/mcp", d.port))
        .header("Authorization", d.bearer())
        .header("Origin", "https://evil.example")
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
}

/// The agent's own MCP call, through the server Arugula gave it: the
/// result's structured content, or its error.
pub(crate) fn agent_mcp(d: &Daemon, agent: u64, tool: &str, args: Value) -> Result<Value, String> {
    let answers = || -> Vec<String> {
        entries(&d.state(agent))
            .into_iter()
            .filter_map(|e| e["text"].as_str().and_then(|t| t.strip_prefix("MCP ")).map(str::to_owned))
            .collect()
    };
    let before = answers().len();
    d.call(agent, "send", json!({ "text": format!("mcp {tool} {args}") }));
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    let said = loop {
        let now = answers();
        if now.len() > before {
            break now.last().cloned().unwrap();
        }
        assert!(std::time::Instant::now() < deadline, "no answer to {tool}: {}", d.state(agent));
        std::thread::sleep(Duration::from_millis(100));
    };
    let r: Value = serde_json::from_str(&said).unwrap();
    if r["isError"] == true || r.get("error").is_some() {
        return Err(r.to_string());
    }
    Ok(r["structuredContent"].clone())
}

/// A `tools/list` to `/mcp` with this Authorization: the HTTP status.
fn post_mcp(port: u16, auth: &str) -> u16 {
    use std::io::{BufRead, BufReader, Write};
    let mut c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    c.write_all(
        format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: {auth}\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )
    .unwrap();
    let mut line = String::new();
    BufReader::new(c).read_line(&mut line).unwrap();
    line.split_whitespace().nth(1).unwrap().parse().unwrap()
}

fn tab_of(d: &Daemon, pane: u64) -> u64 {
    let panes = d.get("/api/panes");
    panes.as_array().unwrap().iter().find(|p| p["id"] == pane).map(|p| p["tab"].as_u64().unwrap()).expect("open")
}

#[test]
fn an_agent_block_works_in_its_own_tab() {
    let d = Daemon::child_with(&["--wisp-token-file", "/nonexistent", "--block-listen", "127.0.0.1:0"]);
    // Another tab, with work the agent mustn't touch.
    let other = d.post("/api/run", json!({}))["pane"].as_u64().unwrap();
    d.post(&format!("/api/panes/{other}/send"), json!({ "text": "echo other-tab-secret", "enter": true }));
    let a = d.open("hello");
    d.wait(a, "idle");

    // It was given arugula: loopback /mcp with a token of its own, which
    // its log doesn't keep.
    let servers = std::fs::read_dir(&d.sessions)
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with("mcp-"))
        .map(|e| serde_json::from_slice::<Value>(&std::fs::read(e.path()).unwrap()).unwrap())
        .expect("the session's MCP servers");
    let ours = servers.as_array().unwrap().iter().find(|s| s["name"] == "arugula").cloned().unwrap();
    assert_eq!(ours["type"], "http");
    assert_eq!(ours["url"], format!("http://127.0.0.1:{}/mcp", d.port));
    let auth = ours["headers"][0]["value"].as_str().unwrap().to_owned();
    assert!(auth.starts_with("Bearer ilb_"), "{auth}");
    let token = auth.trim_start_matches("Bearer ");
    let log = std::fs::read_dir(d.state.join(format!("blocks/{a}")))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("seg-"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect::<String>();
    assert!(log.contains("<redacted>") && !log.contains(token), "the token stays out of the log");

    // Its project's dev server, in a pane beside it... `python3 -m
    // http.server`'s, without its HTTPServer, which looks up 127.0.0.1's
    // name before it serves (35s on a Mac whose DNS doesn't answer that).
    let port = free_port();
    let serve = format!(
        "python3 -c 'import http.server as h, socketserver as s; \
         v = s.TCPServer((\"127.0.0.1\", {port}), h.SimpleHTTPRequestHandler); \
         print(\"Serving HTTP\", flush=True); v.serve_forever()'"
    );
    let r = agent_mcp(&d, a, "run", json!({ "command": serve, "cwd": d.sessions })).unwrap();
    let server = r["pane"].as_u64().unwrap();
    assert_eq!(tab_of(&d, server), tab_of(&d, a), "in its own tab");
    let m =
        agent_mcp(&d, a, "wait", json!({ "pane": server, "until": "match", "pattern": "Serving HTTP", "timeout": 30 }))
            .unwrap();
    assert_eq!(m["state"], "matched", "{m}");
    // ...and the page in a browser block beside itself.
    let b = agent_mcp(&d, a, "show", json!({ "kind": "port", "port": port })).unwrap()["block"].as_u64().unwrap();
    assert_eq!(tab_of(&d, b), tab_of(&d, a));
    let info = d.get(&format!("/api/blocks/{b}"))["info"].clone();
    assert_eq!((info["type"].as_str(), info["started_by"]["block"].as_u64()), (Some("browser"), Some(a)), "{info}");
    d.wait_for("the page to load", || d.state(b)["title"].as_str().is_some_and(|t| t.contains("Directory listing")));

    // It sees its own tab only.
    let l = agent_mcp(&d, a, "list", json!({})).unwrap();
    let seen: Vec<u64> = l["panes"].as_array().unwrap().iter().map(|p| p["pane"].as_u64().unwrap()).collect();
    assert!(seen.contains(&a) && seen.contains(&server) && seen.contains(&b) && !seen.contains(&other), "{l}");
    // Touching another tab is refused.
    for (tool, args) in [
        ("send_input", json!({ "pane": other, "text": "rm -rf ~" })),
        ("read_output", json!({ "pane": other })),
        ("close", json!({ "pane": other })),
        ("run", json!({ "command": "true", "split": other })),
        ("run", json!({ "command": "true", "vm": true })),
        ("show", json!({ "kind": "port", "port": port, "beside": other })),
        ("show", json!({ "kind": "conversation", "id": "no-such-conversation", "beside": other })),
    ] {
        let e = agent_mcp(&d, a, tool, args.clone()).expect_err(&format!("{tool} {args}"));
        assert!(e.contains("own tab") || e.contains("another tab"), "{tool}: {e}");
    }
    let h = agent_mcp(&d, a, "history", json!({})).unwrap();
    assert!(!h.to_string().contains("other-tab-secret"), "{h}");
    // In its tab, it reads what it didn't start, but doesn't drive it.
    let e = agent_mcp(&d, a, "close", json!({ "pane": a })).unwrap_err();
    assert!(e.contains("wasn't started by this agent"), "{e}");
    agent_mcp(&d, a, "close", json!({ "pane": server })).unwrap();

    // Closed, its token goes with it.
    d.post(&format!("/api/panes/{a}/close"), json!({}));
    d.wait_for("the block to close", || d.raw("GET", &format!("/api/blocks/{a}"), None).0 == 404);
    assert_eq!(post_mcp(d.port, &auth), 401);
}

#[test]
fn one_agent_starts_another_and_answers_its_question() {
    let d = Daemon::child();
    let a = d.open("hello");
    d.wait(a, "idle");
    let b = agent_mcp(
        &d,
        a,
        "start_agent",
        json!({ "agent": "acp", "command": format!("python3 {}", fake()), "prompt": "ask one" }),
    )
    .unwrap()["block"]
        .as_u64()
        .unwrap();
    assert_eq!(tab_of(&d, b), tab_of(&d, a), "beside it");
    let w = agent_mcp(&d, a, "wait", json!({ "pane": b, "until": "needs_input", "timeout": 30 })).unwrap();
    assert_eq!(w["state"], "needs_input", "{w}");
    assert_eq!(w["ask"]["questions"][0]["question"], "Which colour do you prefer?", "{w}");
    agent_mcp(&d, a, "agent_respond", json!({ "pane": b, "action": "answer", "answers": { "question_0": "Blue" } }))
        .unwrap();
    let w = agent_mcp(&d, a, "wait", json!({ "pane": b, "until": "idle", "timeout": 30 })).unwrap();
    assert_ne!(w["state"], "needs_input", "{w}");
    let t = agent_mcp(&d, a, "read_output", json!({ "pane": b })).unwrap();
    assert!(t["text"].as_str().unwrap().contains("You answered: Which colour do you prefer? Blue"), "{t}");
    // Who answered is on record.
    let s = d.state(b);
    assert!(s.to_string().contains("mcp:fake-agent"), "{s}");

    // #163: it hands out tools and a mode, but never every check off.
    let start = |mode: &str| {
        let args = json!({ "agent": "acp", "command": format!("python3 {}", fake()), "prompt": "mode",
            "allow": ["Bash"], "permission_mode": mode });
        agent_mcp(&d, a, "start_agent", args)
    };
    let e = start("bypassPermissions").unwrap_err();
    assert!(e.contains("only the user"), "{e}");
    let c = start("auto").unwrap()["block"].as_u64().unwrap();
    d.wait(c, "idle");
    let s = d.state(c);
    assert_eq!((s["allow"].clone(), s["permission_mode"].as_str()), (json!([{ "tool": "Bash" }]), Some("auto")));
    assert!(entries(&s).iter().any(|e| e["text"] == "Mode: auto"), "{s}");
}

/// M71: an agent attaches a file of its own: to an agent it started, as
/// that agent's prompt with an image in it; into a shell it started, its
/// path pasted; not into a program that wouldn't read a path, nor into
/// what it didn't start.
#[test]
fn an_agent_attaches_a_screenshot_to_a_pane() {
    let tmp = std::env::temp_dir().join(format!("ilg-attach-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let d = Daemon::child_env(&[], &[("TMPDIR", tmp.to_str().unwrap())]);
    let a = d.open("hello");
    d.wait(a, "idle");

    // To an agent it started, as base64.
    let command = format!("python3 {} --images", fake());
    let b = agent_mcp(&d, a, "start_agent", json!({ "agent": "acp", "command": command, "prompt": "hello" })).unwrap()
        ["block"]
        .as_u64()
        .unwrap();
    d.wait(b, "idle");
    agent_mcp(&d, a, "attach", json!({ "pane": b, "data": PNG_B64, "text": "look" })).unwrap();
    d.wait(b, "idle");
    let said = entries(&d.state(b)).into_iter().rev().find(|e| e["type"] == "agent").unwrap();
    assert_eq!(said["text"], "Saw 1 image(s) ['image/png']; text []");

    // Into a shell it started, by its path here.
    let shot = d.sessions.join("shot.png");
    std::fs::write(&shot, png()).unwrap();
    let sh = agent_mcp(&d, a, "run", json!({ "command": "true", "wait": true })).unwrap()["pane"].as_u64().unwrap();
    let r = agent_mcp(&d, a, "attach", json!({ "pane": sh, "path": shot })).unwrap();
    let pasted = r["path"].as_str().unwrap().to_owned();
    assert!(pasted.ends_with(".png") && pasted.contains("arugula-uploads"), "{r}");
    assert_eq!(std::fs::read(&pasted).unwrap(), png());
    d.wait_for("the path on its screen", || {
        let screen = agent_mcp(&d, a, "read_output", json!({ "pane": sh, "screen": true })).unwrap();
        screen["text"].as_str().unwrap_or("").replace('\n', "").contains(&pasted)
    });

    // Not into what wouldn't read a path, nor what it didn't start.
    let busy = agent_mcp(&d, a, "run", json!({ "command": "sleep 60" })).unwrap()["pane"].as_u64().unwrap();
    d.wait_for("sleep in front", || {
        agent_mcp(&d, a, "attach", json!({ "pane": busy, "path": shot })).is_err_and(|e| e.contains("sleep"))
    });
    let e = agent_mcp(&d, a, "attach", json!({ "pane": a, "data": PNG_B64 })).unwrap_err();
    assert!(e.contains("wasn't started by this agent"), "{e}");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[tokio::test(flavor = "multi_thread")]
async fn what_failed_here_yesterday() {
    let d = Daemon::child();
    let s = bridge(&d, Client::named("claude-code")).await;
    let repo = d.sessions.join("repo");
    let elsewhere = d.sessions.join("elsewhere");
    for dir in [&repo, &elsewhere] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let mut yesterday = vec![];
    for (cmd, dir) in [("cargo-test-m16 || false", &repo), ("true", &repo), ("false", &elsewhere)] {
        yesterday
            .push(call(&s, "run", json!({ "command": cmd, "cwd": dir, "wait": true })).await["pane"].as_u64().unwrap());
    }
    // ...made yesterday's: their history, a day and a bit back.
    let re = regex::Regex::new(r#""at_ms":(\d+)"#).unwrap();
    for p in &yesterday {
        let index = d.state.join(format!("blocks/{p}/index"));
        let text = std::fs::read_to_string(&index).unwrap();
        let shifted = re.replace_all(&text, |c: &regex::Captures| {
            format!("\"at_ms\":{}", c[1].parse::<u64>().unwrap() - 30 * 3600 * 1000)
        });
        std::fs::write(&index, shifted.as_bytes()).unwrap();
    }
    // And one today.
    call(&s, "run", json!({ "command": "false # today", "cwd": repo, "wait": true })).await;

    let h = call(&s, "history", json!({ "failed": true, "cwd": repo, "since": "2d", "before": "1d" })).await;
    let cmds: Vec<&str> = h["commands"].as_array().unwrap().iter().map(|c| c["command"].as_str().unwrap()).collect();
    assert_eq!(cmds, ["cargo-test-m16 || false"], "{h}");
    assert!(h["summary"].as_str().unwrap().starts_with("1 failed command"), "{h}");
    s.cancel().await.unwrap();
}

/// #59: an agent block in a wisp VM gets Arugula through the relay the
/// daemon opens into its VM (a guest can't reach the host): scoped to its
/// tab like a local one, its `run` on its own machine, across a daemon
/// restart. Skips without a wisp token on this host.
#[cfg(feature = "labs")]
#[test]
fn an_agent_block_in_a_vm_gets_mcp_through_the_relay() {
    let token = std::env::var_os("ARUGULA_WISP_TOKEN_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share/wisp/token"));
    if !token.exists() {
        eprintln!("SKIP: no wisp token on this host (ARUGULA_WISP_TOKEN_FILE or ~/.local/share/wisp/token)");
        return;
    }
    let mut d = Daemon::child_labs_with(&["--wisp-token-file", token.to_str().unwrap()]);
    let other = d.post("/api/run", json!({}))["pane"].as_u64().unwrap();
    // fake_acp.py itself, run in the guest.
    let src = std::fs::read_to_string(fake()).unwrap();
    let config = json!({ "agent": "acp", "command": ["python3", "-c", src], "prompt": "hello" });
    let a = d.open_with(json!({ "type": "agent", "vm": true, "config": config }));
    d.wait_secs(a, "idle", 300);

    // Its tab, through the relay.
    let l = agent_mcp(&d, a, "list", json!({})).unwrap();
    let seen: Vec<u64> = l["panes"].as_array().unwrap().iter().map(|p| p["pane"].as_u64().unwrap()).collect();
    assert!(seen.contains(&a) && !seen.contains(&other), "{l}");
    // Its run lands on its own machine, beside it (which makes the machine
    // its tab's).
    // And where an agent from before the rename (#509) connects.
    let (sock, was) = (format!("/tmp/arugula-mcp-{a}.sock"), format!("/tmp/illogical-mcp-{a}.sock"));
    let check = format!("test -S {sock} && test -S {was} && echo IN-ITS-VM");
    let r = agent_mcp(&d, a, "run", json!({ "command": check, "wait": true })).unwrap();
    assert_eq!(r["exit"], 0, "{r}");
    assert!(r.to_string().contains("IN-ITS-VM"), "{r}");
    let pane = r["pane"].as_u64().unwrap();
    assert_eq!(tab_of(&d, pane), tab_of(&d, a));
    let m = d.get("/api/machines");
    assert_eq!(m.as_array().unwrap().len(), 1, "{m}");
    assert_eq!(m[0]["owner"], json!({ "tab": tab_of(&d, a) }), "{m}");
    let h = d.get("/api/history?limit=20");
    assert!(h.as_array().unwrap().iter().any(|c| c["by"] == "mcp:fake-agent"), "{h}");
    // Other tabs are refused, as for a local agent.
    let e = agent_mcp(&d, a, "send_input", json!({ "pane": other, "text": "rm -rf ~" })).unwrap_err();
    assert!(e.contains("own tab") || e.contains("another tab"), "{e}");

    // A restarted daemon opens a new relay; the agent's next client gets
    // through it.
    d.stop();
    d.start();
    d.wait_secs(a, "idle", 120);
    let l = agent_mcp(&d, a, "list", json!({})).unwrap();
    assert!(l["panes"].as_array().unwrap().iter().any(|p| p["pane"] == pane), "{l}");

    // Closed with its tab, the machine (and the relay with it) goes.
    d.post(&format!("/api/panes/{a}/close"), json!({}));
    d.post(&format!("/api/panes/{pane}/close"), json!({}));
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !d.get("/api/machines").as_array().unwrap().is_empty() {
        assert!(std::time::Instant::now() < deadline, "its machine should go");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// #147: prompt_agent, one call per turn, no sleeps here: Claude Code
/// (recorded, replayed) in a terminal pane, and an agent block.
#[tokio::test(flavor = "multi_thread")]
async fn prompt_agent_waits_for_the_turn() {
    let d = Daemon::child();
    let s = bridge(&d, Client::named("claude-code")).await;
    let scratch = Scratch::new("mcp-prompt");
    let r = replay::Replay::install(&scratch.join("bin"), "claude", "claude_turn");
    d.post("/api/panes/1/send", json!({"text": r.path(), "enter": true}));
    let wait = |line: &'static str, n: usize| {
        let r = &r;
        tokio::task::block_in_place(move || r.reached(line, n))
    };
    wait("m blocked", 1);
    d.post("/api/panes/1/keys", json!({"keys": ["Down", "Enter"]}));
    wait("m idle", 1);
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while d.get("/api/panes/1/detection")["shown"] != "idle" {
        let why = (d.get("/api/panes/1/detection"), d.get("/api/panes/1/capture"), r.log());
        assert!(std::time::Instant::now() < deadline, "its screen isn't read as idle: {why:?}");
        std::thread::sleep(Duration::from_millis(50));
    }

    let v = call(
        &s,
        "prompt_agent",
        json!({ "pane": 1, "text": "Run this shell command: sleep 4 && touch made-by-claude.txt" }),
    )
    .await;
    assert_eq!(v["result"], "needs_input", "{v}");
    assert_eq!(v["question"], "Claude Code asks to run `sleep 4 && touch made-by-claude.txt`", "{v}");
    // Waiting on that: nothing typed, the question back.
    let v = call(&s, "prompt_agent", json!({ "pane": 1, "text": "never mind" })).await;
    assert_eq!(v["result"], "blocked", "{v}");
    assert!(v["summary"].as_str().unwrap().contains("nothing was typed"), "{v}");
    // Answered (Enter), and through to the end of the turn.
    let v = call(&s, "prompt_agent", json!({ "pane": 1, "text": "", "answering": true })).await;
    assert_eq!(v["result"], "done", "{v}");
    let screen = d.get("/api/panes/1/capture").as_str().unwrap_or_default().to_owned();
    assert!(screen.contains("has been created") && screen.contains("? for shortcuts"), "{screen}");

    // An agent block: its question comes back with the call.
    let b = d.open("hello");
    d.wait(b, "idle");
    let v = call(&s, "prompt_agent", json!({ "pane": b, "text": "ask one" })).await;
    assert_eq!(v["result"], "needs_input", "{v}");
    assert_eq!(v["ask"]["questions"][0]["question"], "Which colour do you prefer?", "{v}");
    let v = call(&s, "prompt_agent", json!({ "pane": b, "text": "recall" })).await;
    assert_eq!(v["result"], "blocked", "{v}");
}

/// #576: who a prompt_agent turn is recorded as: the MCP client, never the
/// owner.
#[tokio::test(flavor = "multi_thread")]
async fn prompt_agent_records_the_client_as_who_prompted() {
    let d = Daemon::child();
    let s = bridge(&d, Client::named("claude-code")).await;
    let b = d.open("hello");
    d.wait(b, "idle");
    let v = call(&s, "prompt_agent", json!({ "pane": b, "text": "recall" })).await;
    assert_eq!(v["result"], "done", "{v}");
    // The block notes who the turn is from, just before it.
    let entries = d.state(b)["entries"].clone();
    let at = entries.as_array().unwrap().iter().position(|e| e["text"] == "recall").unwrap();
    assert_eq!(entries[at - 1]["text"], "A follow-up from mcp:claude-code", "{entries}");
}

/// AR-1 (#1475): a turn that edits one file and commits another comes back
/// with its final message, clean, and the work it left: the edit uncommitted,
/// the one commit and its diffstat. Both prompt_agent and wait (idle) say so.
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_returns_its_message_and_the_work_it_left() {
    let d = Daemon::child();
    let s = bridge(&d, Client::named("claude-code")).await;
    let scratch = Scratch::new("mcp-turn-result");
    let repo = scratch.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git").args(args).current_dir(&repo).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "T"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-q", "-m", "start"]);
    // The reflog counts seconds: the turn starts in a later one than this.
    tokio::time::sleep(Duration::from_millis(1100)).await;

    let config = json!({ "agent": "acp", "command": ["python3", fake()], "cwd": repo, "prompt": "hello" });
    let b = d.open_with(json!({ "type": "agent", "config": config }));
    d.wait(b, "idle");

    let v = call(&s, "prompt_agent", json!({ "pane": b, "text": "work a.txt b.txt" })).await;
    assert_eq!(v["result"], "done", "{v}");
    let turn = &v["turn"];
    assert_eq!(turn["message"], "done", "{v}");
    let work = &turn["work"];
    assert_eq!(work["branch"], "main", "{v}");
    assert_eq!(work["pushed"], false, "{v}");
    assert_eq!(work["uncommitted"], json!(["a.txt"]), "{v}");
    let commits = work["commits"].as_array().unwrap_or_else(|| panic!("no commits: {v}"));
    assert_eq!(commits.len(), 1, "{v}");
    assert!(commits[0].as_str().unwrap().ends_with(" add b.txt"), "{v}");
    assert!(work["diffstat"].as_str().unwrap().contains("b.txt"), "{v}");
    assert!(!work["diffstat"].as_str().unwrap().contains("a.txt"), "{v}");
    assert_eq!(work["head"], git(&["rev-parse", "--short", "HEAD"]), "{v}");
    let summary = v["summary"].as_str().unwrap();
    assert!(summary.contains("UNCOMMITTED: 1 file") && summary.contains("\"done\""), "{summary}");

    // wait (idle) says the same, once the turn is done.
    let w = call(&s, "wait", json!({ "pane": b, "until": "idle" })).await;
    assert_eq!(w["turn"], *turn, "{w}");
    assert!(w["summary"].as_str().unwrap().contains("UNCOMMITTED"), "{w}");

    // A directory that isn't a repo: the message alone.
    let plain = scratch.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let config = json!({ "agent": "acp", "command": ["python3", fake()], "cwd": plain, "prompt": "hello" });
    let p = d.open_with(json!({ "type": "agent", "config": config }));
    d.wait(p, "idle");
    let v = call(&s, "prompt_agent", json!({ "pane": p, "text": "recall" })).await;
    assert_eq!(v["result"], "done", "{v}");
    assert!(v["turn"]["message"].is_string() && v["turn"].get("work").is_none(), "{v}");
}

/// The `labs` file in the state dir lists `delegate`, `doc` and the other
/// jobs (kinds of show and list), and removing it takes them away again,
/// with no restart. Each is read where it's used, on a `stat`. Chat's old
/// pane-thread text never comes back (#1095); M198's thread tools are
/// listed without labs.
#[tokio::test(flavor = "multi_thread")]
async fn labs_lists_all_the_tools() {
    let d = Daemon::child();
    let s = bridge(&d, Client::named("claude-code")).await;
    let names = |tools: &[rmcp::model::Tool]| tools.iter().map(|t| t.name.to_string()).collect::<Vec<_>>();
    let instructions = |s: &Session| s.peer_info().and_then(|i| i.instructions.clone()).unwrap_or_default();
    assert_eq!(s.list_all_tools().await.unwrap().len(), 32 - NO_FORGE);
    assert!(!instructions(&s).contains("read_thread"), "{}", instructions(&s));

    // With Labs built in, the `labs` file lists them.
    #[cfg(feature = "labs")]
    {
        arugula_testkit::labs(&d.state, true);
        let with = s.list_all_tools().await.unwrap();
        assert_eq!(with.len(), 34, "{:?}", names(&with));
        assert!(with.iter().any(|t| t.name == "delegate"), "delegate isn't listed with labs");
        for (tool, kind) in [("show", "app"), ("show", "workspace")] {
            assert!(kinds(&with, tool).contains(&kind.to_owned()), "{tool} kind {kind} isn't listed with labs");
        }
        for (tool, kind) in [("show", "managoat"), ("list", "managoat_agents"), ("list", "managoat_agent")] {
            assert!(!kinds(&with, tool).contains(&kind.to_owned()), "retired {tool} kind {kind} is listed with labs");
        }
        // A new connection's instructions have no thread text.
        let s2 = bridge(&d, Client::named("claude-code")).await;
        assert!(!instructions(&s2).contains("thread"), "{}", instructions(&s2));
        s2.cancel().await.unwrap();

        arugula_testkit::labs(&d.state, false);
        // A `fountain` flag from before #942's F5 changes nothing.
        std::fs::write(d.state.join("flags.json"), r#"{"flags":{"fountain":false}}"#).unwrap();
        let without = s.list_all_tools().await.unwrap();
        assert_eq!(without.len(), 32 - NO_FORGE);
        assert!(!kinds(&without, "show").contains(&"managoat".to_owned()));
        assert!(!kinds(&without, "show").contains(&"workspace".to_owned()));
    }
    // Without it, the file changes nothing: they aren't in this build.
    #[cfg(not(feature = "labs"))]
    {
        arugula_testkit::labs(&d.state, true);
        let with = s.list_all_tools().await.unwrap();
        assert_eq!(with.len(), 32 - NO_FORGE, "{:?}", names(&with));
        assert!(!kinds(&with, "show").contains(&"managoat".to_owned()));
        assert!(!instructions(&s).contains("read_thread"), "{}", instructions(&s));
        arugula_testkit::labs(&d.state, false);
    }
    s.cancel().await.unwrap();
}

/// A Claude Code in a terminal pane, through `arugula mcp`, is that pane
/// where a call leaves one out, and `list` says which it is.
#[cfg(feature = "labs")]
#[tokio::test(flavor = "multi_thread")]
async fn a_terminal_pane_is_the_default_of_the_mcp_server_it_runs() {
    let d = Daemon::child();
    arugula_testkit::labs(&d.state, true);
    let mine = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    let other = d.post("/api/run", json!({}))["pane"].as_u64().unwrap();

    let s = bridge_in(&d, Client::named("claude-code"), Some(mine)).await;
    // Splitting "self" opens beside it, in its tab.
    let r = call(&s, "run", json!({ "split": "self" })).await;
    let split = r["pane"].as_u64().unwrap();
    assert_ne!(split, mine);
    let tab = |p: u64| d.get("/api/panes").as_array().unwrap().iter().find(|x| x["id"] == p).unwrap()["tab"].clone();
    assert_eq!(tab(split), tab(mine), "{r}");
    let l = call(&s, "list", json!({})).await;
    let you: Vec<u64> = l["panes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["you"] == true)
        .map(|p| p["pane"].as_u64().unwrap())
        .collect();
    assert_eq!(you, [mine], "only the caller's row says you: {l}");
    assert!(l["panes"].as_array().unwrap().iter().any(|p| p["pane"] == other), "{l}");
    assert!(l["panes"].as_array().unwrap().iter().all(|p| p.get("you").is_none() || p["you"] == true), "{l}");
    s.cancel().await.unwrap();

    // No pane, as before.
    let s = bridge_in(&d, Client::named("claude-code"), None).await;
    assert!(refused(&s, "run", json!({ "split": "Self" })).await.contains("no pane of its own"));
    let l = call(&s, "list", json!({})).await;
    assert!(l["panes"].as_array().unwrap().iter().all(|p| p.get("you").is_none()), "{l}");
    s.cancel().await.unwrap();

    // A pane that isn't open resolves no better than a wrong one named.
    let s = bridge_in(&d, Client::named("claude-code"), Some(9999)).await;
    let by_default = refused(&s, "read_file", json!({ "path": "notes.txt" })).await;
    assert_eq!(by_default, refused(&s, "read_file", json!({ "path": "notes.txt", "pane": 9999 })).await);
    s.cancel().await.unwrap();
}

/// The header only fills in a default: an agent block's token is its own
/// pane, whatever pane the request says it is in.
#[cfg(feature = "labs")]
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_blocks_token_is_its_own_pane_whatever_the_header_says() {
    use axum::http::{HeaderName, HeaderValue};
    let d = Daemon::child_with(&["--wisp-token-file", "/nonexistent", "--block-listen", "127.0.0.1:0"]);
    arugula_testkit::labs(&d.state, true);
    let other = d.post("/api/run", json!({}))["pane"].as_u64().unwrap();
    let a = d.open("hello");
    d.wait(a, "idle");
    let servers = std::fs::read_dir(&d.sessions)
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with("mcp-"))
        .map(|e| serde_json::from_slice::<Value>(&std::fs::read(e.path()).unwrap()).unwrap())
        .expect("the session's MCP servers");
    let ours = servers.as_array().unwrap().iter().find(|s| s["name"] == "arugula").cloned().unwrap();
    let token = ours["headers"][0]["value"].as_str().unwrap().trim_start_matches("Bearer ").to_owned();

    let mut config =
        StreamableHttpClientTransportConfig::with_uri(format!("http://127.0.0.1:{}/mcp", d.port)).auth_header(token);
    config
        .custom_headers
        .insert(HeaderName::from_static("x-arugula-pane"), HeaderValue::from_str(&other.to_string()).unwrap());
    let s = Client::named("claude-code").serve(StreamableHttpClientTransport::from_config(config)).await.unwrap();
    let l = call(&s, "list", json!({})).await;
    let you: Vec<u64> = l["panes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["you"] == true)
        .map(|p| p["pane"].as_u64().unwrap())
        .collect();
    assert_eq!(you, [a], "{l}");
    s.cancel().await.unwrap();
}

// ---------------------------------------------------------------- #234

const OWNER: &str = "me@example.com";
const FRIEND: &str = "friend@example.com";

/// A daemon on a tailnet (so tests can be someone else), its first pane
/// and session, and FRIEND an editor there.
fn shared_daemon(env: &[(&str, &str)]) -> (Daemon, u64, u64) {
    let args = ["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock", "--wisp-token-file", "/nonexistent"];
    let d = Daemon::child_env(&args, env);
    let p = &d.get("/api/panes")[0];
    let (pane, session) = (p["id"].as_u64().unwrap(), p["session"].as_u64().unwrap());
    d.post("/api/acl", json!({ "session": session, "principal": format!("tailnet:{FRIEND}"), "role": "editor" }));
    (d, pane, session)
}

/// A POST on the socket as the CLI sends it under Claude Code: the HTTP
/// status. Like the CLI, it sends no body to a route that takes none
/// (`Request::without_body`, as close's): the daemon answers those without
/// reading one, and hangs up on a body it left unread, so the write or the
/// read here would fail with a broken pipe or a reset (#910). The request
/// goes in one write, as the CLI's does.
fn as_agent(d: &Daemon, path: &str, body: Option<Value>) -> u16 {
    use std::io::{Read, Write};
    let mut s = std::os::unix::net::UnixStream::connect(d.sock()).unwrap();
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Type: application/json\r\nX-Arugula-Agent: 1\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out.split_whitespace().nth(1).unwrap().parse().unwrap()
}

fn invite_blocks(d: &Daemon) -> Vec<u64> {
    let panes = d.get("/api/panes");
    panes.as_array().unwrap().iter().filter(|p| p["type"] == "invite").map(|p| p["id"].as_u64().unwrap()).collect()
}

fn grant_of(d: &Daemon, principal: &str) -> Option<Value> {
    d.get("/api/acl")["grants"].as_array().unwrap().iter().find(|g| g["principal"] == principal).cloned()
}

/// The card on a block (or pane), if one is open.
fn card_on(d: &Daemon, id: u64) -> Value {
    let panes = d.get("/api/panes");
    panes.as_array().unwrap().iter().find(|p| p["id"] == id).map(|p| p["ask"].clone()).unwrap_or(Value::Null)
}

/// `read_invite` until it isn't waiting.
fn settled(m: &Mcp, draft: &str, pane: Option<u64>) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let r = m.call("read_invite", json!({ "draft": draft, "pane": pane })).unwrap();
        if r["status"] != "waiting" {
            return r;
        }
        assert!(std::time::Instant::now() < deadline, "still waiting: {r}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn an_invite_waits_for_the_owner_and_only_they_send_it() {
    let (d, pane, _) = shared_daemon(&[]);
    // A full client outside any pane (no $ARUGULA_PANE).
    let m = Mcp::bridge(&d, None);
    let note = "the flaky test needs their eyes";

    // Where is unsaid; whom nobody knows; never an owner: no card.
    let e = m.call("invite_person", json!({ "who": "tailnet:sam@example.com", "note": note })).unwrap_err();
    assert!(e.contains("pane:"), "{e}");
    let e = m.call("invite_person", json!({ "who": "sam", "pane": pane, "note": note })).unwrap_err();
    assert!(e.contains("share once from the web"), "{e}");
    let e = m.call("invite_person", json!({ "who": FRIEND, "pane": pane, "role": "owner", "note": note })).unwrap_err();
    assert!(e.contains("never makes an owner"), "{e}");
    let e = m.call("invite_person", json!({ "who": FRIEND, "pane": pane, "note": " " })).unwrap_err();
    assert!(e.contains("note"), "{e}");
    assert!(invite_blocks(&d).is_empty(), "no card for any of that");

    // A draft, at once; nothing shared.
    let sam = "tailnet:sam@example.com";
    let r = m.call("invite_person", json!({ "who": sam, "pane": pane, "role": "editor", "note": note })).unwrap();
    assert_eq!(r["status"], "waiting", "{r}");
    let draft = r["draft"].as_str().unwrap().to_owned();
    let block = r["block"].as_u64().unwrap();
    assert!(grant_of(&d, sam).is_none());
    assert_eq!(tab_of(&d, block), tab_of(&d, pane), "beside the pane");
    // The card goes up after invite_person answers (`draft` raises it on
    // its own task): read or answered before then, it isn't there.
    d.wait_for("its card", || card_on(&d, block)["id"] == draft.as_str());
    let card = card_on(&d, block);
    assert_eq!(card["source"], "invite", "{card}");
    let name = d.get("/api/panes")[0]["session_name"].as_str().unwrap().to_owned();
    assert_eq!(
        card["message"],
        format!(
            "claude-code (pane %{pane}) wants to bring sam@example.com (editor) into {name} at pane %{pane}: {note}"
        )
    );
    assert!(card_on(&d, pane).is_null(), "never on the pane itself");
    assert_eq!(m.call("read_invite", json!({ "draft": draft, "pane": pane })).unwrap()["status"], "waiting");

    // An editor may answer other cards, not this one: by the card, or the
    // push's action. Nor an agent, through agent_respond.
    let body = json!({ "id": draft, "content": { "role": "editor" } });
    let (status, text) = d.raw_as(FRIEND, "POST", &format!("/api/blocks/{block}/call/answer"), Some(body));
    assert_eq!(status, 403, "{text}");
    let act = json!({ "action": "answer", "pane": block, "content": { "role": "editor" } });
    let (status, text) = d.raw_as(FRIEND, "POST", "/api/attention/act", Some(act));
    assert_eq!(status, 403, "{text}");
    let (status, _) = d.raw_as(FRIEND, "POST", "/api/attention/act", Some(json!({ "action": "deny", "pane": block })));
    assert_eq!(status, 403);
    let e = m.call("agent_respond", json!({ "pane": block, "action": "answer", "answers": {} })).unwrap_err();
    assert!(e.contains("only the session's owner"), "{e}");
    // Nor Claude Code on the owner's own CLI (it says so).
    assert_eq!(as_agent(&d, &format!("/api/blocks/{block}/call/answer"), Some(json!({ "content": {} }))), 403);
    assert_eq!(
        as_agent(&d, "/api/attention/act", Some(json!({ "action": "answer", "pane": block, "content": {} }))),
        403
    );
    // Nor may it skip the card: invite on the CLI, or make a card of its
    // own (whose words needn't be what it does), or pin a team.
    let session = d.get("/api/panes")[0]["session"].clone();
    let direct = json!({ "session": session, "who": sam, "role": "editor", "drive_minutes": 60 });
    assert_eq!(as_agent(&d, "/api/invite", Some(direct)), 403);
    assert_eq!(as_agent(&d, "/api/team-pins", Some(json!({ "pins": {} }))), 403);
    let forged = json!({ "type": "invite", "config": { "drafter": "%1", "drafts": [] } });
    let (status, text) = d.raw("POST", "/api/blocks", Some(forged));
    assert_eq!(status, 403, "{text}");
    assert_eq!(invite_blocks(&d), [block], "no other invite block");
    assert!(grant_of(&d, sam).is_none(), "nothing granted");
    assert_eq!(card_on(&d, block)["id"], draft.as_str(), "the card waits still");

    // The owner sends it: #233's invite, as them.
    let content = json!({ "role": "editor", "note": "edited: the flaky test" });
    let (status, text) =
        d.raw("POST", &format!("/api/blocks/{block}/call/answer"), Some(json!({ "content": content })));
    assert_eq!(status, 200, "{text}");
    let r = settled(&m, &draft, Some(pane));
    assert_eq!(r["status"], "sent", "{r}");
    assert_eq!((r["delivery"].as_str(), r["grant"]["principal"].as_str()), (Some("unreachable"), Some(sam)), "{r}");
    assert!(r["delivery_reason"].is_string() && r["settled_by"].is_string(), "{r}");
    assert_eq!(r["note"], "edited: the flaky test");
    assert_eq!(grant_of(&d, sam).unwrap()["role"], "editor");
    let audit = d.get("/api/acl")["audit"].clone();
    let line = audit.as_array().unwrap().iter().find(|a| a["action"] == "invite").cloned().unwrap();
    assert_eq!(
        (line["by"].as_str(), line["approved_by"].as_str(), line["drafted_by"].as_str(), line["drafted_in"].as_u64()),
        (Some("owner"), r["settled_by"].as_str(), Some("mcp:claude-code"), Some(pane)),
        "{line}"
    );

    // Declined, with a reason the agent reads; and without one.
    let kim = "tailnet:kim@example.com";
    let r = m.call("invite_person", json!({ "who": kim, "pane": pane, "note": note })).unwrap();
    let draft = r["draft"].as_str().unwrap().to_owned();
    assert_eq!(r["block"].as_u64(), Some(block), "the same block for the same caller");
    d.wait_for("its card", || card_on(&d, block)["id"] == draft.as_str());
    let content = json!({ "decline": true, "reason": "not this week" });
    d.post(&format!("/api/blocks/{block}/call/answer"), json!({ "content": content }));
    let r = settled(&m, &draft, Some(pane));
    assert_eq!((r["status"].as_str(), r["reason"].as_str()), (Some("declined"), Some("not this week")), "{r}");
    assert!(r["summary"].as_str().unwrap().contains("not this week"));
    let r = m.call("invite_person", json!({ "who": kim, "pane": pane, "note": note })).unwrap();
    let draft = r["draft"].as_str().unwrap().to_owned();
    d.wait_for("its card", || card_on(&d, block)["id"] == draft.as_str());
    d.post(&format!("/api/blocks/{block}/call/decline"), json!({}));
    let r = settled(&m, &draft, Some(pane));
    assert_eq!((r["status"].as_str(), r["reason"].as_str()), (Some("declined"), None), "{r}");
    assert!(grant_of(&d, kim).is_none());

    // Five may wait; the sixth is refused.
    for _ in 0..5 {
        m.call("invite_person", json!({ "who": kim, "pane": pane, "note": note })).unwrap();
    }
    let e = m.call("invite_person", json!({ "who": kim, "pane": pane, "note": note })).unwrap_err();
    assert!(e.contains("wait for the user already"), "{e}");
    // Another caller reads none of them.
    let other = Mcp::bridge(&d, Some(block));
    let e = other.call("read_invite", json!({ "draft": draft })).unwrap_err();
    assert!(e.contains("no invite"), "{e}");
}

#[test]
fn an_unanswered_invite_is_dropped() {
    let (d, pane, _) = shared_daemon(&[("ARUGULA_INVITE_TTL_MS", "1500")]);
    let m = Mcp::bridge(&d, Some(pane));
    let r = m.call("invite_person", json!({ "who": "tailnet:sam@example.com", "note": "x" })).unwrap();
    assert_eq!(r["pane"].as_u64(), Some(pane), "the pane arugula mcp runs in, by default");
    let block = r["block"].as_u64().unwrap();
    let r = settled(&m, r["draft"].as_str().unwrap(), None);
    assert_eq!(r["status"], "dropped", "{r}");
    assert!(card_on(&d, block).is_null(), "its card went with it");
    assert!(grant_of(&d, "tailnet:sam@example.com").is_none());
}

#[test]
fn an_invite_card_leaves_the_panes_own_cards_alone() {
    let d = Daemon::child();
    let pane = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    // Claude Code in that terminal, asking a question through its hook.
    let question = json!({ "questions": [{ "question": "Which colour?", "header": "Colour",
        "options": [{ "label": "Blue" }, { "label": "Red" }], "multiSelect": false }] });
    std::thread::scope(|s| {
        let asked = s.spawn(|| d.raw("POST", &format!("/api/panes/{pane}/ask"), Some(question)));
        d.wait_for("its question", || card_on(&d, pane)["kind"] == "questions");
        let q = card_on(&d, pane)["id"].clone();
        let m = Mcp::bridge(&d, Some(pane));
        let r = m.call("invite_person", json!({ "who": "tailnet:sam@example.com", "note": "x" })).unwrap();
        let block = r["block"].as_u64().unwrap();
        // Its card goes up on its own task: decline it once it's there.
        d.wait_for("the invite card", || card_on(&d, block)["id"] == r["draft"]);
        assert_eq!(card_on(&d, pane)["id"], q, "its own card stays");
        // Settling the invite doesn't answer it...
        d.post(&format!("/api/blocks/{block}/call/decline"), json!({}));
        settled(&m, r["draft"].as_str().unwrap(), None);
        assert_eq!(card_on(&d, pane)["id"], q);
        // ...nor answering it the invite.
        let r = m.call("invite_person", json!({ "who": "tailnet:sam@example.com", "note": "y" })).unwrap();
        let draft = r["draft"].as_str().unwrap().to_owned();
        d.wait_for("the invite card", || card_on(&d, block)["id"] == draft.as_str());
        let r = d.post(
            "/api/attention/act",
            json!({ "action": "answer", "pane": pane, "content": { "question_0": "Blue" } }),
        );
        assert_eq!(r["results"][0]["ok"], true, "{r}");
        let (status, text) = asked.join().unwrap();
        assert_eq!(status, 200, "{text}");
        assert!(text.contains("Blue"), "{text}");
        assert_eq!(card_on(&d, block)["id"], draft.as_str(), "the invite still waits");

        // A permission card, the same.
        let permit = json!({ "tool_name": "Bash", "tool_input": { "command": "ls" }, "session_id": "s" });
        let permitted = s.spawn(|| d.raw("POST", &format!("/api/panes/{pane}/permit"), Some(permit)));
        d.wait_for("its permission card", || card_on(&d, pane)["kind"] == "permission");
        assert_eq!(card_on(&d, block)["id"], draft.as_str());
        d.post(&format!("/api/blocks/{block}/call/answer"), json!({ "content": {} }));
        assert_eq!(settled(&m, &draft, None)["status"], "sent");
        assert_eq!(card_on(&d, pane)["kind"], "permission", "sending the invite left it");
        d.post("/api/attention/act", json!({ "action": "allow", "pane": pane }));
        let (status, text) = permitted.join().unwrap();
        assert_eq!(status, 200, "{text}");
        assert!(text.contains("allow"), "{text}");
        // Nothing raises anything on the invite block but itself (a card
        // that got there would wait for an answer: give it a few seconds).
        let mut c = std::os::unix::net::UnixStream::connect(d.sock()).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let body = json!({ "questions": [{ "question": "?" }] }).to_string();
        use std::io::{Read, Write};
        write!(
            c,
            "POST /api/panes/{block}/ask HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut head = [0u8; 12];
        c.read_exact(&mut head).expect("a question went up on the invite block");
        assert_eq!(&head[9..12], b"403");
    });
}

#[test]
fn an_agent_block_drafts_beside_itself() {
    let d = Daemon::child();
    let a = d.open("hello");
    d.wait(a, "idle");
    let r =
        agent_mcp(&d, a, "invite_person", json!({ "who": "tailnet:sam@example.com", "note": "pair on it" })).unwrap();
    assert_eq!((r["status"].as_str(), r["pane"].as_u64()), (Some("waiting"), Some(a)), "{r}");
    let block = r["block"].as_u64().unwrap();
    assert_eq!(tab_of(&d, block), tab_of(&d, a));
    assert!(grant_of(&d, "tailnet:sam@example.com").is_none());
    let draft = r["draft"].as_str().unwrap().to_owned();
    // The card goes up after invite_person answers, on its own task.
    d.wait_for("its card", || card_on(&d, block)["id"] == draft.as_str());
    let card = card_on(&d, block);
    // Whose agent it is, and whom, by name (the login is the name here).
    let want = format!("fake-agent (pane %{a}, you started it) wants to bring sam@example.com (");
    assert!(card["message"].as_str().unwrap().starts_with(&want), "{card}");
    let r = agent_mcp(&d, a, "read_invite", json!({ "draft": draft })).unwrap();
    assert_eq!(r["status"], "waiting", "{r}");
    // It may not close or answer its own card.
    let e = agent_mcp(&d, a, "close", json!({ "pane": block })).unwrap_err();
    assert!(e.contains("wasn't started by this agent"), "{e}");
    d.post(&format!("/api/blocks/{block}/call/answer"), json!({ "content": {} }));
    d.wait_for("sent", || {
        agent_mcp(&d, a, "read_invite", json!({ "draft": draft })).is_ok_and(|r| r["status"] == "sent")
    });
    assert!(grant_of(&d, "tailnet:sam@example.com").is_some());
}

/// An intent over the WebSocket as FRIEND: the error it got, if any.
fn intent_as_friend(d: &Daemon, intent: Value) -> Option<String> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let mut req = format!("ws://127.0.0.1:{}/ws", d.port).into_client_request().unwrap();
        req.headers_mut().insert("Tailscale-User-Login", FRIEND.parse().unwrap());
        let (mut ws, _) = tokio_tungstenite::connect_async(req).await.expect("FRIEND connects");
        let msg = json!({ "type": "intent", "id": 7, "intent": intent });
        ws.send(Message::Text(msg.to_string().into())).await.unwrap();
        let wait = tokio::time::timeout(Duration::from_secs(3), async {
            while let Some(Ok(m)) = ws.next().await {
                if let Message::Text(t) = m
                    && let Ok(v) = serde_json::from_str::<Value>(&t)
                    && v["type"] == "error"
                    && v["id"] == 7
                {
                    return v["message"].as_str().map(str::to_owned);
                }
            }
            None
        });
        wait.await.unwrap_or(None)
    })
}

#[test]
fn closing_an_invite_block_is_the_owners_and_loses_nothing() {
    let (d, pane, _) = shared_daemon(&[]);
    let m = Mcp::bridge(&d, Some(pane));
    let invite = |who: &str| m.call("invite_person", json!({ "who": who, "note": "x" })).unwrap();
    let sent = invite("tailnet:sam@example.com");
    let block = sent["block"].as_u64().unwrap();
    let sent = sent["draft"].as_str().unwrap().to_owned();
    // The card goes up after invite_person answers (`draft` raises it on
    // its own task): answered before then, `answer` isn't a card's.
    d.wait_for("its card", || card_on(&d, block)["id"] == sent.as_str());
    d.post(&format!("/api/blocks/{block}/call/answer"), json!({ "content": {} }));
    assert_eq!(settled(&m, &sent, None)["status"], "sent");
    let waiting = invite("tailnet:kim@example.com")["draft"].as_str().unwrap().to_owned();
    d.wait_for("its card", || card_on(&d, block)["id"] == waiting.as_str());

    // An editor can't close it: by the API, the pane or its tab over the
    // WebSocket. Nor an agent, through MCP's close.
    let (status, text) = d.raw_as(FRIEND, "POST", &format!("/api/panes/{block}/close"), Some(json!({})));
    assert_eq!(status, 403, "{text}");
    let why = intent_as_friend(&d, json!({ "op": "close_pane", "pane": block }));
    assert!(why.as_deref().is_some_and(|w| w.contains("only the session's owner closes")), "{why:?}");
    let why = intent_as_friend(&d, json!({ "op": "close_tab", "tab": tab_of(&d, block) }));
    assert!(why.as_deref().is_some_and(|w| w.contains("only the session's owner closes")), "{why:?}");
    let e = m.call("close", json!({ "pane": block })).unwrap_err();
    assert!(e.contains("only the session's owner closes"), "{e}");
    assert_eq!(as_agent(&d, &format!("/api/panes/{block}/close"), None), 403);
    assert!(invite_blocks(&d).contains(&block), "still open");
    assert_eq!(m.call("read_invite", json!({ "draft": waiting })).unwrap()["status"], "waiting");

    // The owner closes it: what waited is dropped, and said so; what was
    // sent still says so.
    d.post(&format!("/api/panes/{block}/close"), json!({}));
    d.wait_for("it to close", || !invite_blocks(&d).contains(&block));
    let r = m.call("read_invite", json!({ "draft": waiting })).unwrap();
    assert_eq!(r["status"], "dropped", "{r}");
    assert!(r["reason"].as_str().unwrap().contains("closed"), "{r}");
    assert_eq!(m.call("read_invite", json!({ "draft": sent })).unwrap()["status"], "sent");
    assert!(grant_of(&d, "tailnet:kim@example.com").is_none());
}

/// #229: `capabilities` says what this daemon offers the caller: the blocks
/// `show` lists (a Labs flag adds its kind), its tools, whether control is
/// joined, and whether Claude Code's hooks reach the daemon from its pane.
#[tokio::test(flavor = "multi_thread")]
async fn capabilities_says_what_is_here_and_whether_hooks_are_live() {
    let d = Daemon::child();
    let mine = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    let s = bridge_in(&d, Client::named("claude-code"), Some(mine)).await;
    let c = call(&s, "capabilities", json!({})).await;
    let blocks: Vec<&str> = c["blocks"].as_array().unwrap().iter().map(|b| b["kind"].as_str().unwrap()).collect();
    for kind in ["terminal", "agent", "port", "changes", "file"] {
        assert!(blocks.contains(&kind), "{kind} isn't offered: {c}");
    }
    assert!(!blocks.contains(&"workspace"), "workspace without its flag: {c}");
    assert!(c["tools"].as_array().unwrap().iter().any(|t| t == "capabilities"), "{c}");
    assert_eq!(c["control"]["state"], "not_joined", "{c}");
    assert_eq!((c["hooks"]["pane"].as_u64(), c["hooks"]["live"].as_bool()), (Some(mine), Some(false)), "{c}");
    assert!(c["hooks"]["note"].as_str().unwrap().contains("arugula hooks install"), "{c}");

    // A hook from the pane (as `arugula hook` posts Claude Code's
    // PreToolUse): live.
    d.post(
        &format!("/api/panes/{mine}/hook"),
        json!({ "hook_event_name": "PreToolUse", "tool_name": "mcp__arugula__capabilities", "tool_input": {} }),
    );
    let c = call(&s, "capabilities", json!({})).await;
    assert_eq!(c["hooks"]["live"], true, "{c}");
    assert!(c["summary"].as_str().unwrap().contains("hooks live"), "{c}");

    #[cfg(feature = "labs")]
    {
        arugula_proto::flags::set(&d.state, "chant-workspaces", true).unwrap();
        let c = call(&s, "capabilities", json!({})).await;
        assert!(c["blocks"].as_array().unwrap().iter().any(|b| b["kind"] == "workspace"), "{c}");
        assert_eq!(c["flags"], json!(["chant-workspaces"]), "{c}");
    }
    s.cancel().await.unwrap();

    // With no pane, it can't tell.
    let s = bridge_in(&d, Client::named("claude-code"), None).await;
    let c = call(&s, "capabilities", json!({})).await;
    assert_eq!(c["hooks"]["live"], Value::Null, "{c}");
    s.cancel().await.unwrap();
}

/// A fake control's Cloud workspace routes (#1094): one workspace and one
/// tab, whose turn ends a few reads after a prompt (and reads idle once
/// right after it, as control may before its next read of Managoat), and
/// every request's signature header.
#[derive(Clone, Default)]
struct FakeCloud {
    signed: Arc<std::sync::Mutex<Vec<String>>>,
    bodies: Arc<std::sync::Mutex<Vec<Value>>>,
}

/// A fake control's Cloud workspaces (hosted ones, M184): one, `hw_0a`
/// in `prj_a`, its box still being made.
async fn fake_cloud(f: FakeCloud) -> String {
    use axum::{
        Json, Router,
        extract::{Request, State},
        middleware::{self, Next},
        routing::{get, post},
    };
    async fn note(State(f): State<FakeCloud>, req: Request, next: Next) -> axum::response::Response {
        if let Some(h) = req.headers().get("x-arugula-auth").and_then(|h| h.to_str().ok()) {
            f.signed.lock().unwrap().push(format!("{} {h}", req.uri().path()));
        }
        next.run(req).await
    }
    let workspace =
        json!({ "id": "hw_0a", "name": "App box", "project": "prj_a", "project_name": "App", "state": "starting" });
    let (listed, made) = (workspace.clone(), workspace);
    let app = Router::new()
        .route("/control.json", get(|| async { Json(json!({ "daemon_auth": 2 })) }))
        .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
        .route("/api/daemon/peers", get(|| async { Json(json!({})) }))
        .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
        .route("/api/daemon/access", post(|| async { Json(json!({})) }))
        .route(
            "/api/workspaces",
            get(move || {
                let w = listed.clone();
                async move { Json(json!({ "workspaces": [w] })) }
            }),
        )
        .route(
            "/api/projects/prj_a/workspaces",
            post(move |State(f): State<FakeCloud>, Json(body): Json<Value>| {
                let w = made.clone();
                async move {
                    f.bodies.lock().unwrap().push(body);
                    Json(w)
                }
            }),
        )
        .layer(middleware::from_fn_with_state(f.clone(), note))
        .with_state(f);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    at
}

/// #1094: an agent here drives the owner's Cloud workspaces through
/// control, signed as this machine; not joined, it's told so.
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_drives_cloud_workspaces_through_control() {
    let fake = FakeCloud::default();
    let url = fake_cloud(fake.clone()).await;
    let d = Daemon::child_with(&["--wisp-token-file", "/nonexistent"]);
    let s = bridge(&d, Client::named("claude-code")).await;

    let said = refused(&s, "list", json!({ "kind": "cloud_workspaces" })).await;
    assert!(said.contains("hasn't joined Arugula control"), "{said}");
    let c = call(&s, "capabilities", json!({})).await;
    assert_eq!(c["control"]["cloud"]["reachable"], false, "{c}");

    // Joined, as `join` leaves it; the running daemon notices.
    let keys = arugula_e2e::DeviceKeys::load_or_create(&d.state.join("daemon.key")).unwrap();
    let cert = arugula_e2e::Cert::new(&keys, "a1", arugula_e2e::Kind::Daemon, "test");
    let saved = json!({ "url": url, "trust": { "account": "a1", "root": keys.id() }, "cert": cert });
    std::fs::write(d.state.join("control.json"), saved.to_string()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let c = call(&s, "capabilities", json!({})).await;
        if c["control"]["cloud"]["reachable"] == true {
            assert!(c["summary"].as_str().unwrap().contains("Cloud workspaces: yes"), "{c}");
            break;
        }
        assert!(std::time::Instant::now() < deadline, "never reachable: {c}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // A Cloud workspace is a machine of a project.
    let create = json!({
        "kind": "create", "name": "App box", "apt": ["ffmpeg"], "npm": ["pnpm"],
        "repositories": [{ "url": "https://github.com/example/app" }],
    });
    let why = refused(&s, "cloud", create.clone()).await;
    assert!(why.contains("give project"), "{why}");
    let mut create = create;
    create["project"] = json!("prj_a");
    let made = call(&s, "cloud", create).await;
    assert_eq!(made["cloud_workspace"]["id"], "hw_0a", "{made}");
    {
        let bodies = fake.bodies.lock().unwrap();
        assert_eq!(bodies[0]["name"], "App box", "{bodies:?}");
        assert_eq!(bodies[0]["spec"]["repos"], json!(["https://github.com/example/app"]), "{bodies:?}");
        assert_eq!(bodies[0]["spec"]["packages"], json!(["ffmpeg", "pnpm"]), "{bodies:?}");
    }
    let listed = call(&s, "list", json!({ "kind": "cloud_workspaces" })).await;
    let row = &listed["cloud_workspaces"][0];
    assert_eq!((row["kind"].as_str(), row["name"].as_str()), (Some("hosted"), Some("App box")), "{listed}");
    let why = refused(&s, "read_cloud", json!({ "kind": "files", "cloud_workspace": "cw_0a" })).await;
    assert!(why.contains("no cloud workspace cw_0a"), "{why}");
    let why = refused(&s, "wait", json!({ "cloud_workspace": "hw_0a", "tab": "3", "until": "exit" })).await;
    assert!(why.contains("until idle or needs_input"), "{why}");

    // Every call was signed as this machine.
    let signed = fake.signed.lock().unwrap().clone();
    let cloud: Vec<&String> =
        signed.iter().filter(|s| s.starts_with("/api/workspaces") || s.starts_with("/api/projects/")).collect();
    assert!(cloud.len() >= 4, "{signed:?}");
    assert!(cloud.iter().all(|s| s.contains(&format!(" v2 {} ", keys.id()))), "{signed:?}");
    s.cancel().await.unwrap();
    drop(d);
}

/// A fake control's platform for M169's forge operations: one linked PR
/// (`acme/app#1`), drafts kept as rows, sends not built yet (as the
/// platform answers until M168), and every call as it came, with its
/// signature.
#[derive(Clone, Default)]
struct FakeForge {
    calls: Arc<std::sync::Mutex<Vec<(String, Value)>>>,
    drafts: Arc<std::sync::Mutex<Vec<Value>>>,
}

async fn fake_forge(f: FakeForge) -> String {
    use axum::{
        Json, Router,
        extract::State,
        http::{HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        routing::{get, post},
    };
    async fn ops(State(f): State<FakeForge>, h: HeaderMap, Json(b): Json<Value>) -> Response {
        let auth = h.get("x-arugula-auth").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
        f.calls.lock().unwrap().push((auth, b.clone()));
        let ok = |result: Value| Json(json!({ "op": b["op"], "v": b["v"], "result": result })).into_response();
        let no = |status: StatusCode, said: &str| (status, Json(json!({ "error": said }))).into_response();
        let linked = b["repo"] == "acme/app" && b["number"] == 1;
        let drafts = f.drafts.lock().unwrap().clone();
        match b["op"].as_str().unwrap_or_default() {
            "pr.get" if linked => ok(json!({
                "item": { "id": "fi_1", "repo": "acme/app", "number": 1, "kind": "pr", "state": "open",
                          "title": "Fix the thing", "labels": [], "author": "alice-gh", "assignees": [],
                          "reviewers": ["bob-gh"], "draft": false, "merge_state": null, "head_ref": "fix",
                          "head_sha": "abc1234def", "base_ref": "main", "created_at": 1, "updated_at": 2 },
                "events": [{ "id": "fev_1", "kind": "review", "actor": "bob-gh", "review_state": "commented", "at": 1 }],
                "drafts": drafts,
            })),
            "pr.get" | "issue.get" | "checks.get" => {
                no(StatusCode::NOT_FOUND, "that repo isn't linked to a project you can read it in")
            }
            "draft.create" => {
                let d = json!({
                    "id": format!("dft_{:032x}", drafts.len() + 1), "project": "prj_1", "repo": b["repo"],
                    "repo_id": 77, "number": b["number"], "kind": b["kind"], "event": b["event"],
                    "title": b["title"], "body": b["body"], "state": "waiting", "author": "a1",
                    "agent": b["agent"], "machine": "m1", "machine_name": "geek", "created_at": 1, "updated_at": 1,
                });
                f.drafts.lock().unwrap().push(d.clone());
                ok(json!({ "draft": d, "created": true }))
            }
            "draft.list" => ok(json!({ "drafts": drafts })),
            "draft.get" => match drafts.into_iter().find(|d| d["id"] == b["draft"]) {
                Some(d) => ok(d),
                None => no(StatusCode::NOT_FOUND, "no such draft"),
            },
            op => (
                StatusCode::NOT_IMPLEMENTED,
                Json(json!({ "error": format!("{op}: sending from Arugula control isn't available yet; a draft waits for it"),
                             "reason": "not_yet" })),
            )
                .into_response(),
        }
    }
    let app = Router::new()
        .route("/control.json", get(|| async { Json(json!({ "daemon_auth": 2 })) }))
        .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
        .route("/api/daemon/peers", get(|| async { Json(json!({})) }))
        .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
        .route("/api/daemon/access", post(|| async { Json(json!({})) }))
        .route("/api/forge/ops", post(ops))
        .route(
            "/api/forge/repos/{owner}/{name}",
            get(|| async { Json(json!({ "repo": "acme/app", "linked": true, "project": "prj_1", "repo_id": 77 })) }),
        )
        .with_state(f);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    at
}

/// #1215's done-when, up to the send (M168's): an agent with no PR block
/// open reads a linked PR and drafts a review through MCP, and a person
/// lists and reads the draft with the CLI, whose send says it isn't built
/// yet. MCP has no send.
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_drafts_on_a_linked_pr_with_no_block_and_a_person_sees_it() {
    let fake = FakeForge::default();
    let url = fake_forge(fake.clone()).await;
    // A `gh` with no login: a block here never reads with the person's own.
    let bin = std::env::temp_dir().join(format!("arugula-forge-ops-gh-{}", std::process::id()));
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("gh"), "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(bin.join("gh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let api = format!("{url}/github");
    let d = Daemon::child_env(
        &["--wisp-token-file", "/nonexistent"],
        &[("PATH", path.as_str()), ("ARUGULA_GITHUB_API", api.as_str())],
    );
    let s = bridge(&d, Client::named("claude-code")).await;

    // Reads and drafts, never a send.
    let tools = s.list_all_tools().await.unwrap();
    assert_eq!(
        kinds(&tools, "forge_read"),
        ["pr", "issue", "items", "checks", "inbox", "drafts", "draft", "native_issues"]
    );
    assert_eq!(kinds(&tools, "forge_draft"), ["comment", "review", "issue", "discard"]);
    assert!(tools.iter().all(|t| !t.name.contains("send") || t.name == "send_input"), "a send tool");

    let said = refused(&s, "forge_read", json!({ "kind": "pr", "item": "acme/app#1" })).await;
    assert!(said.contains("hasn't joined Arugula control"), "{said}");
    let keys = arugula_e2e::DeviceKeys::load_or_create(&d.state.join("daemon.key")).unwrap();
    let cert = arugula_e2e::Cert::new(&keys, "a1", arugula_e2e::Kind::Daemon, "test");
    let saved = json!({ "url": url, "trust": { "account": "a1", "root": keys.id() }, "cert": cert });
    std::fs::write(d.state.join("control.json"), saved.to_string()).unwrap();

    // No block anywhere: read the PR, as soon as the daemon sees it joined.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let pr = loop {
        let r = call_raw(&s, "forge_read", json!({ "kind": "pr", "item": "https://github.com/acme/app/pull/1" })).await;
        if r.is_error != Some(true) {
            break r.structured_content.unwrap();
        }
        assert!(std::time::Instant::now() < deadline, "never joined: {:?}", r.content);
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    assert!(pr["summary"].as_str().unwrap().contains("acme/app#1 [pr, open] Fix the thing"), "{pr}");
    assert_eq!(pr["item"]["id"], "fi_1", "{pr}");
    let panes = call(&s, "list", json!({ "kind": "panes" })).await;
    assert!(!panes.to_string().contains("\"forge\""), "no block opened: {panes}");

    let review = json!({ "kind": "review", "item": "acme/app#1", "event": "request_changes", "body": "split it" });
    let drafted = call(&s, "forge_draft", review).await;
    let id = drafted["draft"]["id"].as_str().unwrap().to_owned();
    assert!(drafted["summary"].as_str().unwrap().contains("waits for a person"), "{drafted}");
    let why = refused(&s, "forge_draft", json!({ "kind": "send", "draft": id })).await;
    assert!(why.contains("send"), "{why}");
    let read = call(&s, "forge_read", json!({ "kind": "drafts", "item": "acme/app#1" })).await;
    assert_eq!(read["drafts"][0]["id"], id.as_str(), "{read}");

    // The old read_forge, on a block of the linked repo: the platform's
    // item beside the block's own.
    let block = d.post(
        "/api/blocks",
        json!({ "type": "forge", "config": { "pr": "https://github.com/acme/app/pull/1" }, "local": true }),
    )["block"]
        .as_u64()
        .unwrap();
    let old = call(&s, "read_forge", json!({ "block": block })).await;
    assert_eq!(old["platform"]["item"]["id"], "fi_1", "{old}");
    assert_eq!(old["platform"]["drafts"][0]["id"], id.as_str(), "{old}");
    assert!(old["summary"].as_str().unwrap().contains("linked in Arugula control: fi_1"), "{old}");
    {
        let calls = fake.calls.lock().unwrap();
        let (auth, b) = calls.iter().find(|(_, b)| b["op"] == "draft.create").unwrap();
        assert_eq!(
            (b["v"].as_u64(), b["kind"].as_str(), b["event"].as_str(), b["agent"].as_str()),
            (Some(1), Some("review"), Some("REQUEST_CHANGES"), Some("mcp:claude-code"))
        );
        assert!(auth.starts_with(&format!("v2 {} ", keys.id())), "signed as this machine: {auth}");
    }
    s.cancel().await.unwrap();

    // A person, with the CLI logged in to the same control.
    let home = d.state.join("cli-home");
    let config = home.join("arugula");
    std::fs::create_dir_all(&config).unwrap();
    let cli_keys = arugula_e2e::DeviceKeys::load_or_create(&config.join("cli-key")).unwrap();
    let cli_cert = arugula_e2e::Cert::new(&cli_keys, "a1", arugula_e2e::Kind::Cli, "laptop");
    let saved = json!({ "url": url, "cert": cli_cert, "trust": { "account": "a1", "root": cli_keys.id() } });
    std::fs::write(config.join("cli-control.json"), saved.to_string()).unwrap();
    let bin = cli_bin();
    let cli = move |args: &[&str], agent: bool| {
        let mut c = arugula_testkit::command(&bin);
        c.args(args).env("XDG_CONFIG_HOME", &home).env("ARUGULA_STATE_DIR", home.join("state"));
        c.env_remove("CLAUDECODE").env_remove("AI_AGENT");
        if agent {
            c.env("CLAUDECODE", "1");
        }
        let out = c.output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
        (out.status.success(), text)
    };
    let run = |args: &'static [&'static str], agent: bool| {
        let cli = cli.clone();
        tokio::task::spawn_blocking(move || cli(args, agent))
    };
    let (ok, ls) = run(&["draft", "ls"], false).await.unwrap();
    assert!(ok, "{ls}");
    assert!(ls.contains(&id) && ls.contains("review (request_changes) on acme/app#1"), "{ls}");
    assert!(ls.contains("by mcp:claude-code on geek"), "{ls}");
    let cli2 = cli.clone();
    let shown_id = id.clone();
    let (ok, shown) = tokio::task::spawn_blocking(move || cli2(&["draft", "show", &shown_id], false)).await.unwrap();
    assert!(ok && shown.contains("split it"), "{shown}");
    let (ok, view) = run(&["pr", "view", "acme/app#1"], false).await.unwrap();
    assert!(ok && view.contains("Fix the thing") && view.contains("fix (abc1234) → main"), "{view}");
    assert!(view.contains(&id), "the draft on the PR: {view}");
    let cli3 = cli.clone();
    let send_id = id.clone();
    let (ok, sent) = tokio::task::spawn_blocking(move || cli3(&["draft", "send", &send_id], false)).await.unwrap();
    assert!(!ok && sent.contains("isn't available yet"), "M168's: {sent}");
    let cli4 = cli.clone();
    let (ok, agent) = tokio::task::spawn_blocking(move || cli4(&["draft", "send", &id], true)).await.unwrap();
    assert!(!ok && agent.contains("an agent doesn't send"), "{agent}");
    // A comment from the CLI: the person's sends (not built yet), an
    // agent's is a draft.
    let (ok, c) = run(&["pr", "comment", "acme/app#1", "lgtm"], true).await.unwrap();
    assert!(ok && c.contains("waits for a person"), "{c}");
    let (ok, c) = run(&["pr", "comment", "acme/app#1", "lgtm"], false).await.unwrap();
    assert!(!ok && c.contains("--draft"), "{c}");

    // Every CLI call was signed with its own key, at version 1.
    let calls = fake.calls.lock().unwrap();
    assert!(calls.iter().all(|(_, b)| b["v"] == 1), "{calls:?}");
    let by_cli = calls.iter().filter(|(a, _)| a.contains(&cli_keys.id())).count();
    assert!(by_cli >= 6, "{calls:?}");
    let comment = calls.iter().rev().find(|(_, b)| b["op"] == "draft.create").unwrap();
    assert_eq!((comment.1["kind"].as_str(), comment.1["agent"].as_str()), (Some("comment"), Some("cli")));
    assert_eq!(calls.last().unwrap().1["op"], "pr.comment");
    drop(d);
}

/// A fake control's platform for M198's thread routes: one channel the
/// machine's grants reach, a message that mentions its agents (`me`) and
/// one its owner's agent wrote, posts kept, search by `q`, a refusal for
/// a room it isn't granted, and every call as it came: its path and
/// query, its signature, its body.
#[derive(Clone, Default)]
struct FakeThreads {
    me: String,
    calls: Arc<std::sync::Mutex<Vec<(String, String, Value)>>>,
}

async fn fake_threads(f: FakeThreads) -> String {
    use axum::{
        Json, Router,
        body::Bytes,
        extract::{Path, Request, State},
        http::StatusCode,
        middleware::{self, Next},
        response::IntoResponse,
        routing::{get, post},
    };
    async fn note(State(f): State<FakeThreads>, req: Request, next: Next) -> axum::response::Response {
        let at = req.uri().to_string();
        if at.starts_with("/api/daemon/chat/") {
            let auth = req.headers().get("x-arugula-auth").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
            let (parts, body) = req.into_parts();
            let bytes: Bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
            f.calls.lock().unwrap().push((at, auth, serde_json::from_slice(&bytes).unwrap_or_default()));
            return next.run(Request::from_parts(parts, bytes.into())).await;
        }
        next.run(req).await
    }
    let room = json!({ "id": "ch_general", "kind": "channel", "team": "t1", "name": "general", "target": null,
                       "project": null, "with": null, "last": 2, "last_mention": 1,
                       "powers": ["threads.post", "threads.read"], "writable": true });
    let reach =
        json!([{ "kind": "project", "id": "prj_t1", "name": "acme", "powers": ["threads.post", "threads.read"] }]);
    let (r1, r2) = (room.clone(), room);
    let me = f.me.clone();
    let messages = move || {
        json!([
            { "seq": 1, "at": 1, "author": "a2", "name": "Sam", "text": "@geek can you look?", "quote": null,
              "pane": null, "mentions": [me.clone()], "edited": null, "deleted": null, "imported": null, "agent": null },
            { "seq": 2, "at": 2, "author": "a1", "name": "Jake", "text": "on it", "quote": null, "pane": null,
              "mentions": [], "edited": null, "deleted": null, "imported": null,
              "agent": { "machine": "m1", "machine_name": "geek" } },
        ])
    };
    let found = messages.clone();
    let app = Router::new()
        .route("/control.json", get(|| async { Json(json!({ "daemon_auth": 2 })) }))
        .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
        .route("/api/daemon/peers", get(|| async { Json(json!({})) }))
        .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
        .route("/api/daemon/access", post(|| async { Json(json!({})) }))
        .route(
            "/api/daemon/chat/rooms",
            get(move || {
                let (room, reach) = (r1.clone(), reach.clone());
                async move { Json(json!({ "rooms": [room], "reach": reach })) }
            }),
        )
        .route(
            "/api/daemon/chat/rooms/{id}/messages",
            get(move |Path(id): Path<String>| {
                let (room, ms) = (r2.clone(), messages());
                async move {
                    if id != "ch_general" {
                        return (StatusCode::NOT_FOUND, Json(json!({ "error": "no such room" }))).into_response();
                    }
                    Json(json!({ "room": room, "messages": [ms[0].clone()], "more": false })).into_response()
                }
            })
            .post(|Path(id): Path<String>| async move {
                if id == "dm_ab" {
                    let why = "geek has no threads.post grant over your DMs: its owner gives it with `arugula grants add geek threads.post --dms`";
                    return (StatusCode::FORBIDDEN, Json(json!({ "error": why, "reason": "agent_ungranted" })))
                        .into_response();
                }
                Json(json!({ "seq": 3, "mentions": ["a2"], "unreached": ["nobody"], "pushed": [] })).into_response()
            }),
        )
        .route(
            "/api/daemon/chat/search",
            get(move || {
                let ms = found();
                async move { Json(json!({ "found": [{ "room": "ch_general", "message": ms[1].clone() }] })) }
            }),
        )
        .layer(middleware::from_fn_with_state(f.clone(), note))
        .with_state(f);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    at
}

/// M198: an agent lists, reads (its mentions too), searches and posts in
/// the owner's conversations through control, signed as this machine, with
/// no thread kept here; capabilities says they're live. Not joined, it's
/// told so; a refusal says how to grant.
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_reads_and_posts_in_conversations_through_control() {
    let d = Daemon::child_with(&["--wisp-token-file", "/nonexistent"]);
    let s = bridge(&d, Client::named("claude-code")).await;
    let said = refused(&s, "read_thread", json!({ "room": "ch_general" })).await;
    assert!(said.contains("hasn't joined Arugula control"), "{said}");
    let c = call(&s, "capabilities", json!({})).await;
    assert_eq!(c["control"]["threads"]["live"], false, "{c}");

    let keys = arugula_e2e::DeviceKeys::load_or_create(&d.state.join("daemon.key")).unwrap();
    let fake = FakeThreads { me: format!("machine:{}", keys.id()), ..Default::default() };
    let url = fake_threads(fake.clone()).await;
    let cert = arugula_e2e::Cert::new(&keys, "a1", arugula_e2e::Kind::Daemon, "test");
    let saved = json!({ "url": url, "trust": { "account": "a1", "root": keys.id() }, "cert": cert });
    std::fs::write(d.state.join("control.json"), saved.to_string()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let c = loop {
        let c = call(&s, "capabilities", json!({})).await;
        if c["control"]["threads"]["live"] == true {
            break c;
        }
        assert!(std::time::Instant::now() < deadline, "never live: {c}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    assert!(c["summary"].as_str().unwrap().contains("conversations: yes"), "{c}");
    assert_eq!(c["control"]["threads"]["scopes"][0]["name"], "acme", "{c}");

    let listed = call(&s, "list", json!({ "kind": "threads" })).await;
    assert_eq!(listed["threads"][0]["id"], "ch_general", "{listed}");
    assert!(
        listed["summary"].as_str().unwrap().contains("#general (threads.post, threads.read), mentioned you at seq 1")
    );

    let read = call(&s, "read_thread", json!({ "room": "ch_general", "after_seq": 0, "mentions_only": true })).await;
    assert_eq!((&read["messages"][0]["seq"], &read["next_after_seq"]), (&json!(1), &json!(1)), "{read}");
    assert_eq!(read["messages"][0]["mentions_you"], true, "{read}");
    assert!(read["summary"].as_str().unwrap().contains("[1] Sam: @geek can you look?"), "{read}");
    let found = call(&s, "search_threads", json!({ "q": "on it*", "team": "t1" })).await;
    assert_eq!(found["found"][0]["message"]["by"], "Jake's agent on geek", "{found}");

    let posted = call(&s, "post_thread", json!({ "room": "iss_0123", "text": "looking, @sam" })).await;
    assert_eq!(posted["seq"], 3, "{posted}");
    let summary = posted["summary"].as_str().unwrap();
    assert!(
        summary.contains("labelled as your person's agent") && summary.contains("@nobody reached nobody"),
        "{summary}"
    );
    let why = refused(&s, "post_thread", json!({ "room": "dm_ab", "text": "hi" })).await;
    assert!(why.contains("arugula grants add geek threads.post --dms"), "{why}");
    let why = refused(&s, "read_thread", json!({ "room": "ch_nope" })).await;
    assert!(why.contains("no such room"), "{why}");

    let calls = fake.calls.lock().unwrap().clone();
    let paths: Vec<&str> = calls.iter().map(|(p, _, _)| p.as_str()).collect();
    assert!(paths.contains(&"/api/daemon/chat/rooms/ch_general/messages?after=0&mentions=true"), "{paths:?}");
    assert!(paths.contains(&"/api/daemon/chat/search?q=on+it*&team=t1"), "{paths:?}");
    let post = calls.iter().find(|(p, _, _)| p == "/api/daemon/chat/rooms/iss_0123/messages").unwrap();
    assert_eq!(post.2, json!({ "text": "looking, @sam" }));
    assert!(calls.iter().all(|(_, auth, _)| auth.starts_with(&format!("v2 {} ", keys.id()))), "{calls:?}");
    assert!(!d.state.join("threads").exists(), "no thread kept here");
    s.cancel().await.unwrap();
}

/// A fake control's platform for M200's native issues: one project
/// (`prj_1`) whose issues are kept, their threads' comments, the machine's
/// `issues.write` grant (off until `granted`), GitHub drafts through
/// `/api/forge/ops`, and every call as it came: method and path, its
/// signature, its body.
#[derive(Clone, Default)]
struct FakeNative {
    granted: Arc<std::sync::atomic::AtomicBool>,
    issues: Arc<std::sync::Mutex<Vec<Value>>>,
    comments: Arc<std::sync::Mutex<Vec<(String, String)>>>,
    calls: Arc<std::sync::Mutex<Vec<(String, String, Value)>>>,
}

async fn fake_native(f: FakeNative) -> String {
    use axum::{
        Json, Router,
        extract::{Request, State},
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::{get, post},
    };
    async fn any(State(f): State<FakeNative>, req: Request) -> Response {
        let (method, at) = (req.method().to_string(), req.uri().to_string());
        let auth = req.headers().get("x-arugula-auth").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
        let bytes = axum::body::to_bytes(req.into_body(), 1 << 20).await.unwrap();
        let b: Value = serde_json::from_slice(&bytes).unwrap_or_default();
        f.calls.lock().unwrap().push((format!("{method} {at}"), auth, b.clone()));
        let no = |status: StatusCode, said: &str| (status, Json(json!({ "error": said }))).into_response();
        let path = at.split('?').next().unwrap_or_default().to_owned();
        let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        let find = |id: &str| f.issues.lock().unwrap().iter().position(|i| i["id"] == id);
        match (method.as_str(), segs.as_slice()) {
            ("GET", ["api", "daemon", "chat", "rooms"]) => Json(json!({ "rooms": [], "reach": [
                { "kind": "project", "id": "prj_1", "name": "acme",
                  "powers": ["issues.write", "threads.post", "threads.read"] }] }))
            .into_response(),
            ("GET", ["api", "daemon", "issue-home"]) => Json(json!({
                "project": "prj_1", "home": "native", "chosen": false, "repos": [],
                "native_issues": f.issues.lock().unwrap().len(), "both": false,
            }))
            .into_response(),
            ("POST", ["api", "daemon", "projects", "prj_1", "issues"]) => {
                if !f.granted.load(Ordering::SeqCst) {
                    let why = "geek has no issues.write grant over acme: its owner gives it with `arugula grants add geek issues.write --project prj_1`, or turns on geek for acme under Settings, Agents in conversations, on the platform page";
                    return (StatusCode::FORBIDDEN, Json(json!({ "error": why, "reason": "agent_ungranted" })))
                        .into_response();
                }
                let mut issues = f.issues.lock().unwrap();
                let n = issues.len() + 1;
                let i = json!({
                    "id": format!("iss_{n:032x}"), "project": "prj_1", "number": n, "title": b["title"],
                    "body": b["body"], "state": "open", "close_reason": null, "author": "a1",
                    "agent": { "machine": "m1", "machine_name": "geek" }, "labels": b["labels"],
                    "assignees": b["assignees"], "github": null, "created_at": 1, "updated_at": 1, "closed_at": null,
                });
                issues.push(i.clone());
                Json(i).into_response()
            }
            ("GET", ["api", "daemon", "projects", "prj_1", "issues"]) => {
                Json(json!({ "project": "prj_1", "issues": f.issues.lock().unwrap().clone(), "more": false }))
                    .into_response()
            }
            ("GET", ["api", "daemon", "projects", "prj_1", "issues", n]) => {
                match f.issues.lock().unwrap().iter().find(|i| n.parse::<u64>().is_ok_and(|n| i["number"] == n)) {
                    Some(i) => Json(i.clone()).into_response(),
                    None => no(StatusCode::NOT_FOUND, "no such issue"),
                }
            }
            ("GET", ["api", "daemon", "issues", id]) => match find(id) {
                Some(at) => Json(f.issues.lock().unwrap()[at].clone()).into_response(),
                None => no(StatusCode::NOT_FOUND, "no such issue"),
            },
            ("GET", ["api", "daemon", "issues", id, "events"]) => Json(json!({ "issue": id, "events": [
                { "id": 1, "kind": "opened", "actor": "a1", "agent": { "machine": "m1", "machine_name": "geek" },
                  "at": 1, "detail": {} }] }))
            .into_response(),
            ("PATCH", ["api", "daemon", "issues", id]) => {
                if !f.granted.load(Ordering::SeqCst) {
                    let why = "geek has no issues.write grant over acme";
                    return (StatusCode::FORBIDDEN, Json(json!({ "error": why, "reason": "agent_ungranted" })))
                        .into_response();
                }
                let Some(at) = find(id) else { return no(StatusCode::NOT_FOUND, "no such issue") };
                let mut issues = f.issues.lock().unwrap();
                let i = &mut issues[at];
                let mut events = vec![];
                if let Some(s) = b["state"].as_str() {
                    i["state"] = json!(s);
                    i["close_reason"] =
                        if s == "closed" { json!(b["reason"].as_str().unwrap_or("completed")) } else { Value::Null };
                    events.push(
                        json!({ "id": 2, "kind": if s == "closed" { "closed" } else { "reopened" }, "actor": "a1",
                                        "agent": { "machine": "m1", "machine_name": "geek" }, "at": 2, "detail": {} }),
                    );
                }
                Json(json!({ "issue": i.clone(), "events": events })).into_response()
            }
            ("GET", ["api", "daemon", "chat", "rooms", id, "messages"]) => {
                let ms: Vec<Value> = f
                    .comments
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(on, _)| on == id)
                    .enumerate()
                    .map(|(n, (_, text))| {
                        json!({ "seq": n + 1, "at": 1, "author": "a1", "name": "Jake", "text": text,
                        "agent": { "machine": "m1", "machine_name": "geek" }, "mentions": [] })
                    })
                    .collect();
                Json(json!({ "room": { "id": "ob_1", "kind": "object", "target": id }, "messages": ms, "more": false }))
                    .into_response()
            }
            ("POST", ["api", "daemon", "chat", "rooms", id, "messages"]) => {
                let mut cs = f.comments.lock().unwrap();
                cs.push(((*id).to_owned(), b["text"].as_str().unwrap_or_default().to_owned()));
                let seq = cs.iter().filter(|(on, _)| on == id).count();
                Json(json!({ "seq": seq, "mentions": [], "unreached": [], "pushed": [] })).into_response()
            }
            ("POST", ["api", "forge", "ops"]) if b["op"] == "draft.create" => {
                let d = json!({
                    "id": "dft_00000000000000000000000000000001", "project": "prj_1", "repo": b["repo"],
                    "repo_id": 77, "number": b["number"], "kind": b["kind"], "event": null, "title": null,
                    "body": b["body"], "state": "waiting", "author": "a1", "agent": b["agent"], "machine": "m1",
                    "machine_name": "geek", "created_at": 1, "updated_at": 1,
                });
                Json(json!({ "op": b["op"], "v": b["v"], "result": { "draft": d, "created": true } })).into_response()
            }
            _ => no(StatusCode::NOT_FOUND, &format!("no route {method} {path}")),
        }
    }
    let app = Router::new()
        .route("/control.json", get(|| async { Json(json!({ "daemon_auth": 2 })) }))
        .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
        .route("/api/daemon/peers", get(|| async { Json(json!({})) }))
        .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
        .route("/api/daemon/access", post(|| async { Json(json!({})) }))
        .fallback(any)
        .with_state(f);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    at
}

/// #1271's done-when: an agent with a grant files a native issue, comments
/// on it and closes it over MCP, each landing at once, signed as this
/// machine; refused saying how to grant before it has one. The same agent
/// drafting on a GitHub issue still makes a draft, and a native issue is
/// never drafted on. `#N` means the current project: the one this pane's
/// clone's GitHub repo is linked to.
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_files_comments_on_and_closes_a_native_issue() {
    let d = Daemon::child_with(&["--wisp-token-file", "/nonexistent"]);
    let keys = arugula_e2e::DeviceKeys::load_or_create(&d.state.join("daemon.key")).unwrap();
    let fake = FakeNative::default();
    let url = fake_native(fake.clone()).await;
    let cert = arugula_e2e::Cert::new(&keys, "a1", arugula_e2e::Kind::Daemon, "test");
    let saved = json!({ "url": url, "trust": { "account": "a1", "root": keys.id() }, "cert": cert });
    std::fs::write(d.state.join("control.json"), saved.to_string()).unwrap();

    // The agent's pane is in a clone of acme/app.
    let dir = d.sessions.join("app");
    std::fs::create_dir_all(&dir).unwrap();
    for args in [&["init", "-q"][..], &["remote", "add", "origin", "git@github.com:acme/app.git"]] {
        assert!(Command::new("git").arg("-C").arg(&dir).args(args).status().unwrap().success());
    }
    let s0 = bridge(&d, Client::named("claude-code")).await;
    let pane = call(&s0, "run", json!({ "cwd": dir })).await["pane"].as_u64().unwrap();
    let s = bridge_in(&d, Client::named("claude-code"), Some(pane)).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let c = call(&s, "capabilities", json!({})).await;
        if c["control"]["threads"]["live"] == true {
            assert!(c["control"]["threads"]["note"].as_str().unwrap().contains("issue_write"), "{c}");
            break;
        }
        assert!(std::time::Instant::now() < deadline, "never live: {c}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let file = json!({ "kind": "create", "title": "The canary fails", "body": "since 9:00", "labels": ["bug"] });
    let why = refused(&s, "issue_write", file.clone()).await;
    assert!(why.contains("arugula grants add geek issues.write --project prj_1"), "{why}");
    fake.granted.store(true, Ordering::SeqCst);
    let filed = call(&s, "issue_write", file).await;
    let id = filed["issue"]["id"].as_str().unwrap().to_owned();
    assert!(filed["summary"].as_str().unwrap().contains("Filed #1 [open] The canary fails"), "{filed}");
    assert!(filed["summary"].as_str().unwrap().contains("now, as your person's agent"), "{filed}");

    let commented = call(&s, "issue_write", json!({ "kind": "comment", "issue": "#1", "text": "bisected it" })).await;
    assert_eq!(commented["comment"]["seq"], 1, "{commented}");
    // A refused close leaves no comment behind.
    fake.granted.store(false, Ordering::SeqCst);
    let close = json!({ "kind": "close", "issue": id, "reason": "completed", "comment": "fixed" });
    assert!(refused(&s, "issue_write", close).await.contains("issues.write"));
    assert_eq!(fake.comments.lock().unwrap().len(), 1, "no stray comment");
    fake.granted.store(true, Ordering::SeqCst);
    let closed =
        call(&s, "issue_write", json!({ "kind": "close", "issue": id, "reason": "completed", "comment": "fixed" }))
            .await;
    assert_eq!((&closed["issue"]["state"], &closed["comment"]["seq"]), (&json!("closed"), &json!(2)), "{closed}");

    // Read back: read_forge and forge_read take its number or id.
    let read = call(&s, "read_forge", json!({ "block": "#1" })).await;
    assert_eq!(read["native"]["issue"]["id"], json!(id), "{read}");
    assert!(read["text"].as_str().unwrap().contains("[2] Jake's agent on geek: fixed"), "{read}");
    assert!(read.get("block").is_none(), "{read}");
    let read = call(&s, "forge_read", json!({ "kind": "issue", "item": id })).await;
    assert_eq!(read["comments"].as_array().map(Vec::len), Some(2), "{read}");
    let listed = call(&s, "forge_read", json!({ "kind": "native_issues", "state": "all" })).await;
    assert!(listed["summary"].as_str().unwrap().contains("#1 [closed, completed] The canary fails"), "{listed}");

    // GitHub stays a draft a person sends; a native issue isn't drafted.
    let drafted = call(&s, "forge_draft", json!({ "kind": "comment", "item": "acme/app#7", "body": "lgtm" })).await;
    assert_eq!(drafted["draft"]["state"], "waiting", "{drafted}");
    assert!(drafted["summary"].as_str().unwrap().contains("waits for a person to send"), "{drafted}");
    for (tool, args) in [
        ("forge_draft", json!({ "kind": "comment", "item": "#1", "body": "lgtm" })),
        ("draft", json!({ "kind": "comment", "block": id, "body": "lgtm" })),
    ] {
        let why = refused(&s, tool, args).await;
        assert!(why.contains("issue_write writes to them at once"), "{tool}: {why}");
    }

    let calls = fake.calls.lock().unwrap().clone();
    let at: Vec<&str> = calls.iter().map(|(a, _, _)| a.as_str()).collect();
    assert!(at.contains(&"GET /api/daemon/issue-home?repo=acme%2Fapp"), "the pane's clone: {at:?}");
    assert!(at.contains(&"GET /api/daemon/projects/prj_1/issues/1"), "{at:?}");
    assert!(at.contains(&format!("POST /api/daemon/chat/rooms/{id}/messages").as_str()), "{at:?}");
    let close = calls.iter().rposition(|(a, _, _)| a.starts_with("PATCH ")).unwrap();
    assert_eq!(calls[close].2, json!({ "state": "closed", "reason": "completed" }), "{:?}", calls[close]);
    let comment = calls.iter().rposition(|(a, _, _)| a.ends_with("/messages") && a.starts_with("POST ")).unwrap();
    assert!(close < comment, "closed, then commented: {at:?}");
    let ops: Vec<&Value> =
        calls.iter().filter(|(a, _, _)| a == "POST /api/forge/ops").map(|(_, _, b)| &b["op"]).collect();
    assert_eq!(ops, [&json!("draft.create")], "one draft, no send");
    let mine: Vec<&(String, String, Value)> =
        calls.iter().filter(|(a, _, _)| a.contains("/api/daemon/issue") || a.contains("/projects/")).collect();
    assert!(mine.iter().all(|(_, auth, _)| auth.starts_with(&format!("v2 {} ", keys.id()))), "{mine:?}");
    s.cancel().await.unwrap();
    s0.cancel().await.unwrap();
}

/// A fake control's platform for M216's runs: project `prj_1` with one
/// hosted box (`hw_1`, machine `dev_1`), the machine's run grants (off
/// until `granted`), and the runs started, each of which runs on its own
/// a moment later (`running` in pane 3, then `succeeded`, exit 0) with a
/// kept log. Every call as it came: method and path, its signature, its
/// body.
#[derive(Clone, Default)]
struct FakeRuns {
    granted: Arc<std::sync::atomic::AtomicBool>,
    /// The first wait is refused for too many open (429).
    busy_once: Arc<std::sync::atomic::AtomicBool>,
    runs: Arc<std::sync::Mutex<Vec<Value>>>,
    calls: Arc<std::sync::Mutex<Vec<(String, String, Value)>>>,
}

const RUN_LOG: &str = "running 3 tests\ntest result: ok. 3 passed\n";

async fn fake_runs(f: FakeRuns) -> String {
    use axum::{
        Json, Router,
        extract::{Request, State},
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::{get, post},
    };
    fn event(seq: u64, to: &str, detail: Value) -> Value {
        json!({ "seq": seq, "from": null, "to": to, "at": seq, "detail": detail, "daemon": "dev_1" })
    }
    async fn any(State(f): State<FakeRuns>, req: Request) -> Response {
        let (method, at) = (req.method().to_string(), req.uri().to_string());
        let auth = req.headers().get("x-arugula-auth").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
        let bytes = axum::body::to_bytes(req.into_body(), 1 << 20).await.unwrap();
        let b: Value = serde_json::from_slice(&bytes).unwrap_or_default();
        f.calls.lock().unwrap().push((format!("{method} {at}"), auth, b.clone()));
        let (path, q) = at.split_once('?').unwrap_or((&at, ""));
        let q: std::collections::HashMap<String, String> =
            url::form_urlencoded::parse(q.as_bytes()).into_owned().collect();
        let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        let ungranted = |power: &str| {
            let why = format!(
                "geek-box has no {power} grant over acme: its owner gives it with `arugula grants add geek-box {power} --project prj_1`, or turns on geek-box for acme under Settings, Agents and runs, on the platform page"
            );
            (StatusCode::FORBIDDEN, Json(json!({ "error": why, "reason": "agent_ungranted" }))).into_response()
        };
        let granted = f.granted.load(Ordering::SeqCst);
        let find = |id: &str| f.runs.lock().unwrap().iter().find(|r| r["id"] == id).cloned();
        match (method.as_str(), segs.as_slice()) {
            ("GET", ["api", "daemon", "runs", "reach"]) => {
                let projects = if granted {
                    json!([{ "id": "prj_1", "name": "acme", "powers": ["runs.read", "runs.start"] }])
                } else {
                    json!([])
                };
                Json(json!({ "projects": projects })).into_response()
            }
            ("POST", ["api", "daemon", "projects", "prj_1", "runs"]) => {
                if !granted {
                    return ungranted(if b["spec"].is_null() { "runs.command" } else { "runs.start" });
                }
                let argv: Vec<String> = serde_json::from_value(b["argv"].clone()).unwrap_or_default();
                let op = arugula_e2e::run::Op { argv, ..Default::default() }.encode("dev_1");
                let id = format!("run_{:032x}", f.runs.lock().unwrap().len() + 1);
                let run = json!({
                    "id": id, "project": "prj_1", "workspace": "hw_1", "trigger": "manual",
                    "source": { "by": "a1", "agent": "m1" }, "op": op, "op_hash": "h", "daemon": "dev_1",
                    "state": "pending", "started_by": "a1", "started_by_agent": "m1", "grant": "pgr_1",
                    "last_seq": 1, "created_at": 1, "events": [event(1, "pending", json!({}))],
                });
                f.runs.lock().unwrap().push(run.clone());
                // It runs on its box a moment later, then succeeds.
                let runs = f.runs.clone();
                tokio::spawn(async move {
                    for (seq, to, detail) in
                        [(2, "running", json!({ "pane": 3 })), (3, "succeeded", json!({ "exit": 0 }))]
                    {
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                        let mut all = runs.lock().unwrap();
                        let r = all.iter_mut().find(|r| r["id"] == id).unwrap();
                        r["state"] = json!(to);
                        r["last_seq"] = json!(seq);
                        r["events"].as_array_mut().unwrap().push(event(seq, to, detail));
                        if to == "succeeded" {
                            r["exit_code"] = json!(0);
                            r["pane"] = json!(3);
                        }
                    }
                });
                (StatusCode::CREATED, Json(run)).into_response()
            }
            ("GET", ["api", "daemon", "projects", "prj_1", "runs"]) => {
                if !granted {
                    return ungranted("runs.read");
                }
                Json(json!({ "project": "prj_1", "runs": f.runs.lock().unwrap().clone() })).into_response()
            }
            ("GET", ["api", "daemon", "runs", id]) => {
                if !granted {
                    return ungranted("runs.read");
                }
                // Answered once it has an event after `since`, or ended,
                // or `wait` passed.
                let since: Option<u64> = q.get("since").and_then(|s| s.parse().ok());
                let wait = Duration::from_secs(q.get("wait").and_then(|s| s.parse().ok()).unwrap_or(0));
                if since.is_some() && f.busy_once.swap(false, Ordering::SeqCst) {
                    let why = "at most 8 waits on runs at once: try again when one ends";
                    return (StatusCode::TOO_MANY_REQUESTS, Json(json!({ "error": why }))).into_response();
                }
                let until = std::time::Instant::now() + wait;
                loop {
                    let Some(r) = find(id) else {
                        return (StatusCode::NOT_FOUND, Json(json!({ "error": "no such run" }))).into_response();
                    };
                    let moved = since.is_none_or(|s| r["last_seq"].as_u64().unwrap() > s) || r["state"] == "succeeded";
                    if moved || std::time::Instant::now() >= until {
                        return Json(r).into_response();
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
            ("GET", ["api", "daemon", "runs", id, "log"]) => {
                if !granted {
                    return ungranted("runs.read");
                }
                let Some(r) = find(id) else {
                    return (StatusCode::NOT_FOUND, Json(json!({ "error": "no such run" }))).into_response();
                };
                let max: usize = q.get("max").and_then(|s| s.parse().ok()).unwrap_or(65536);
                let ended = r["state"] == "succeeded";
                let kept = if ended { RUN_LOG } else { "" };
                let offset: usize = match q.get("tail").map(String::as_str) {
                    Some("true") => kept.len().saturating_sub(max),
                    _ => q.get("offset").and_then(|s| s.parse().ok()).unwrap_or(0),
                };
                let text = &kept[offset.min(kept.len())..(offset + max).min(kept.len())];
                Json(json!({ "run": id, "kept": true, "text": text, "offset": offset,
                    "next_offset": offset + text.len(), "end": ended && offset + text.len() == kept.len(), "note": null }))
                .into_response()
            }
            _ => (StatusCode::NOT_FOUND, Json(json!({ "error": format!("no route {method} {path}") }))).into_response(),
        }
    }
    let app = Router::new()
        .route("/control.json", get(|| async { Json(json!({ "daemon_auth": 2 })) }))
        .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
        .route("/api/daemon/peers", get(|| async { Json(json!({})) }))
        .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
        .route("/api/daemon/access", post(|| async { Json(json!({})) }))
        .fallback(any)
        .with_state(f);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    at
}

/// M216's done-when, against a fake platform: an agent starts a run on a
/// project's hosted box, waits for it to end, and reads its log, each call
/// signed as this machine. Before its owner grants it, it's refused saying
/// how, and capabilities says the run tools aren't live.
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_starts_a_run_waits_for_it_and_reads_its_log() {
    let d = Daemon::child_with(&["--wisp-token-file", "/nonexistent"]);
    let keys = arugula_e2e::DeviceKeys::load_or_create(&d.state.join("daemon.key")).unwrap();
    let fake = FakeRuns::default();
    let url = fake_runs(fake.clone()).await;
    let cert = arugula_e2e::Cert::new(&keys, "a1", arugula_e2e::Kind::Daemon, "test");
    let saved = json!({ "url": url, "trust": { "account": "a1", "root": keys.id() }, "cert": cert });
    std::fs::write(d.state.join("control.json"), saved.to_string()).unwrap();
    let s = bridge(&d, Client::named("claude-code")).await;

    let tools = s.list_all_tools().await.unwrap();
    for t in ["start_run", "read_run"] {
        assert!(tools.iter().any(|x| x.name == t), "{t} is listed");
    }
    // Once the daemon has read control.json: joined, with no run grant.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let c = call(&s, "capabilities", json!({})).await;
        assert_eq!(c["control"]["runs"]["live"], false, "{c}");
        if c["control"]["runs"]["note"].as_str().unwrap().contains("runs.start") {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "never joined: {c}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let start = json!({ "project": "prj_1", "command": ["just", "test"], "box": "dev" });
    let why = refused(&s, "start_run", start.clone()).await;
    assert!(why.contains("arugula grants add geek-box runs.command --project prj_1"), "{why}");
    let why = refused(&s, "start_run", json!({ "project": "prj_1", "spec": "nightly" })).await;
    assert!(why.contains("arugula grants add geek-box runs.start --project prj_1"), "{why}");
    assert!(refused(&s, "start_run", json!({ "project": "acme", "command": ["x"] })).await.contains("prj_"));
    let both = json!({ "project": "prj_1", "command": ["x"], "instruction": "y" });
    assert!(refused(&s, "start_run", both).await.contains("one of them"));

    fake.granted.store(true, Ordering::SeqCst);
    let c = call(&s, "capabilities", json!({})).await;
    assert_eq!(c["control"]["runs"]["live"], true, "{c}");
    assert_eq!(c["control"]["runs"]["projects"][0]["id"], "prj_1", "{c}");

    let started = call(&s, "start_run", start).await;
    let id = started["run"]["id"].as_str().unwrap().to_owned();
    let summary = started["summary"].as_str().unwrap();
    assert!(summary.contains(&format!("Started {id} [pending] `just test` on hw_1")), "{summary}");
    assert!(summary.contains("started by a1's agent on m1"), "{summary}");

    let why = refused(&s, "wait", json!({ "run": id, "until": "idle" })).await;
    assert!(why.contains("until it ends"), "{why}");
    // Too many waits open on the platform: it backs off and asks again.
    fake.busy_once.store(true, Ordering::SeqCst);
    let waited = call(&s, "wait", json!({ "run": id, "until": "command_end", "timeout": 30 })).await;
    assert_eq!(waited["state"], "succeeded", "{waited}");
    let summary = waited["summary"].as_str().unwrap();
    assert!(summary.contains("[succeeded, exit 0]") && summary.contains("3 passed"), "{summary}");

    let read = call(&s, "read_run", json!({ "run": id, "offset": 16 })).await;
    assert_eq!(read["log"]["text"], &RUN_LOG[16..], "paged by offset: {read}");
    let summary = read["summary"].as_str().unwrap();
    assert!(summary.contains("[2] running in pane %3") && summary.contains("[3] succeeded, exit 0"), "{summary}");
    let page = call(&s, "read_run", json!({ "run": id, "max_chars": 10 })).await;
    assert_eq!((&page["log"]["text"], &page["log"]["next_offset"]), (&json!("running 3 "), &json!(10)));
    let listed = call(&s, "read_run", json!({ "project": "prj_1" })).await;
    assert!(listed["summary"].as_str().unwrap().contains(&format!("{id} [succeeded, exit 0]")), "{listed}");

    let calls = fake.calls.lock().unwrap().clone();
    let post = calls.iter().find(|(a, _, _)| a == "POST /api/daemon/projects/prj_1/runs").unwrap();
    assert_eq!(post.2, json!({ "argv": ["just", "test"], "workspace": "dev" }));
    let at: Vec<&str> = calls.iter().map(|(a, _, _)| a.as_str()).collect();
    assert!(at.iter().any(|a| a.starts_with(&format!("GET /api/daemon/runs/{id}?since="))), "long-polled: {at:?}");
    assert!(at.contains(&format!("GET /api/daemon/runs/{id}/log?offset=16&max=16000").as_str()), "{at:?}");
    assert!(at.contains(&format!("GET /api/daemon/runs/{id}/log?tail=true&max=16000").as_str()), "the tail: {at:?}");
    assert!(!fake.busy_once.load(Ordering::SeqCst), "a wait was refused once, and asked again");
    let mine: Vec<_> = calls.iter().filter(|(a, _, _)| a.contains("/api/daemon/")).collect();
    assert!(mine.iter().all(|(_, auth, _)| auth.starts_with(&format!("v2 {} ", keys.id()))), "{mine:?}");
    s.cancel().await.unwrap();
}

/// M184: a tool call another machine sends for its agent (a hosted
/// workspace's machine, driven by a project person's laptop) runs here as
/// the owner's would, naming the caller as who started it; a read-only
/// caller there stays read only here; an agent block's turn is prompted
/// and interrupted.
#[tokio::test(flavor = "multi_thread")]
async fn a_machines_forwarded_call_runs_here() {
    let d = Daemon::child();
    let call_here = |tool: &str, arguments: Value, read_only: bool| {
        d.post(
            "/api/mcp/call",
            json!({ "tool": tool, "arguments": arguments, "client": "claude-code via laptop", "read_only": read_only }),
        )
    };
    let v = call_here("run", json!({ "command": "echo forwarded-$((6*7))", "wait": true }), false);
    assert_ne!(v["isError"], true, "{v}");
    let out = &v["structuredContent"];
    assert_eq!(out["exit"], 0, "{v}");
    assert!(out.to_string().contains("forwarded-42"), "{v}");
    let pane = out["pane"].as_u64().unwrap();
    let panes = d.get("/api/panes");
    assert!(panes.to_string().contains("mcp:claude-code via laptop"), "started by the caller: {panes}");

    let v = call_here("run", json!({ "command": "true" }), true);
    assert_eq!(v["isError"], true, "a read-only caller can't run: {v}");
    let v = call_here("read_output", json!({ "pane": pane }), true);
    assert_ne!(v["isError"], true, "{v}");

    let b = d.open("hello");
    d.wait(b, "idle");
    let v = call_here("prompt_agent", json!({ "pane": b, "text": "recall" }), false);
    assert_eq!(v["structuredContent"]["result"], "done", "{v}");
    let v = call_here("machine_read", json!({ "kind": "cancel", "pane": b }), false);
    assert_ne!(v["isError"], true, "{v}");
    let v = call_here("machine_read", json!({ "kind": "cancel", "pane": pane }), false);
    assert_eq!(v["isError"], true, "a terminal isn't an agent block: {v}");
}

/// A fake control's deploy routes for a machine's agents (M218's): one
/// project, refused until the owner gives `deploys.plan`, and every path
/// asked, so the test can tell nothing tried to approve.
#[derive(Clone, Default)]
struct FakeDeploys {
    granted: Arc<std::sync::atomic::AtomicBool>,
    asked: Arc<std::sync::Mutex<Vec<(String, Value)>>>,
}

const DEP: &str = "dep_0123456789abcdef0123456789abcdef";

fn fake_deploy(state: &str) -> Value {
    json!({ "id": DEP, "state": state, "environment": { "id": "env_1", "name": "production" },
        "source": { "kind": "commit", "ref": "ab".repeat(20), "where": "acme/app" }, "roll_back": null,
        "plan": "cd".repeat(32), "plan_text": "deploy fake to production\ncommit\n", "may_approve": false,
        "events": [{ "seq": 1, "kind": "planned", "by": "a1's agent on test" }], "approvals": [] })
}

async fn fake_deploys(f: FakeDeploys) -> String {
    use axum::{
        Json, Router,
        extract::{Request, State},
        http::StatusCode,
        response::IntoResponse,
        routing::{get, post},
    };
    async fn any(State(f): State<FakeDeploys>, req: Request) -> axum::response::Response {
        let path = format!("{} {}", req.method(), req.uri().path());
        let body: Value = axum::body::to_bytes(req.into_body(), 1 << 16)
            .await
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        f.asked.lock().unwrap().push((path.clone(), body));
        if !f.granted.load(std::sync::atomic::Ordering::SeqCst) {
            let why = "test has no deploys.plan grant over that project: its owner gives it with `arugula grants add test deploys.plan --project acme`";
            return (StatusCode::FORBIDDEN, Json(json!({ "error": why, "reason": "agent_ungranted" }))).into_response();
        }
        match path.as_str() {
            "POST /api/daemon/projects/prj_0a/deploys" => Json(fake_deploy("planned")).into_response(),
            p if p == format!("POST /api/daemon/deploys/{DEP}/request") => Json(fake_deploy("pending")).into_response(),
            p if p == format!("GET /api/daemon/deploys/{DEP}") => Json(fake_deploy("pending")).into_response(),
            _ => (StatusCode::NOT_FOUND, Json(json!({ "error": "no such route" }))).into_response(),
        }
    }
    let app = Router::new()
        .route("/control.json", get(|| async { Json(json!({ "daemon_auth": 2 })) }))
        .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
        .route("/api/daemon/peers", get(|| async { Json(json!({})) }))
        .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
        .route("/api/daemon/access", post(|| async { Json(json!({})) }))
        .fallback(any)
        .with_state(f);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    at
}

/// M252: an agent plans a deploy and asks for its approval through control,
/// signed as this machine, under its owner's `deploys.plan` grant; refused
/// without it, saying approving is the owner's. No tool approves, and
/// nothing it does asks control to.
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_plans_a_deploy_and_asks_and_never_approves() {
    let fake = FakeDeploys::default();
    let url = fake_deploys(fake.clone()).await;
    let d = Daemon::child_with(&["--wisp-token-file", "/nonexistent"]);
    let s = bridge(&d, Client::named("claude-code")).await;
    let tools = s.list_all_tools().await.unwrap();
    let plan = tools.iter().find(|t| t.name == "plan_deploy").expect("plan_deploy is listed");
    let says = plan.description.as_deref().unwrap_or_default();
    assert!(says.contains("never approve") && says.contains("an owner approves"), "{says}");
    assert!(!tools.iter().any(|t| t.name.contains("approve") || t.name.contains("deny")), "no tool approves");

    let args =
        json!({ "project": "prj_0a", "environment": "production", "commit": "AB".repeat(20), "repo": "acme/app" });
    let said = refused(&s, "plan_deploy", args.clone()).await;
    assert!(said.contains("hasn't joined Arugula control"), "{said}");
    let said = refused(
        &s,
        "plan_deploy",
        json!({ "project": "prj_0a", "environment": "production", "commit": "abc", "repo": "acme/app" }),
    )
    .await;
    assert!(said.contains("40-hex"), "{said}");

    let keys = arugula_e2e::DeviceKeys::load_or_create(&d.state.join("daemon.key")).unwrap();
    let cert = arugula_e2e::Cert::new(&keys, "a1", arugula_e2e::Kind::Daemon, "test");
    let saved = json!({ "url": url, "trust": { "account": "a1", "root": keys.id() }, "cert": cert });
    std::fs::write(d.state.join("control.json"), saved.to_string()).unwrap();
    // Until the owner gives deploys.plan: refused, saying so.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let said = loop {
        let said = refused(&s, "plan_deploy", args.clone()).await;
        if !said.contains("hasn't joined") {
            break said;
        }
        assert!(std::time::Instant::now() < deadline, "never joined: {said}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    assert!(said.contains("no deploys.plan grant") && said.contains("approving stays theirs"), "{said}");

    fake.granted.store(true, std::sync::atomic::Ordering::SeqCst);
    let p = call(&s, "plan_deploy", args).await;
    assert_eq!(p["deploy"]["state"], "pending", "planned and asked: {p}");
    let summary = p["summary"].as_str().unwrap();
    assert!(summary.contains("deploy fake to production") && summary.contains("you can't"), "{summary}");
    let r = call(&s, "read_deploy", json!({ "id": DEP })).await;
    assert!(r["summary"].as_str().unwrap().contains("waiting for an owner's approval"), "{r}");
    let why = refused(&s, "read_deploy", json!({ "id": "dep_1" })).await;
    assert!(why.contains("dep_ and 32 hex"), "{why}");

    let asked = fake.asked.lock().unwrap().clone();
    let paths: Vec<&str> = asked.iter().map(|(p, _)| p.as_str()).filter(|p| p.contains("deploy")).collect();
    let plan = "POST /api/daemon/projects/prj_0a/deploys";
    let request = format!("POST /api/daemon/deploys/{DEP}/request");
    let read = format!("GET /api/daemon/deploys/{DEP}");
    assert_eq!(paths, [plan, plan, request.as_str(), read.as_str()], "{asked:?}");
    let body = &asked.iter().rfind(|(p, _)| p == plan).unwrap().1;
    assert_eq!(
        body,
        &json!({ "environment": "production", "roll_back": false,
            "source": { "kind": "commit", "ref": "ab".repeat(20), "where": "acme/app" } })
    );
    assert!(!asked.iter().any(|(p, _)| p.contains("approve") || p.contains("deny")), "{asked:?}");
    s.cancel().await.unwrap();
    drop(d);
}
