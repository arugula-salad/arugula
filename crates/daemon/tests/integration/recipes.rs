//! M76 (#398): agent recipes, Claude Code subagent files worn by an agent
//! block, and the A2A cards of the ones a machine offers. The fake ACP
//! agent stands in for Claude Code's adapter and records what its
//! `session/new` got.
//!
//! What's checked: a recipe's prompt, model, tool lists, permission mode,
//! skills (as a local plugin) and MCP servers (inline, and named from
//! `.mcp.json`) reach the session, with a credential by reference only;
//! what can't come along is named with why; a recipe needs the `agents`
//! flag; offering is the owner's and checks the recipe; the cards and the
//! routes go with the flag.
//!
//! And, after #403: a task for an agent this machine offers runs here,
//! without control or a machine named, and is cleaned up once read; one
//! delivered as a pull request works on a branch off the origin's default
//! branch and is told so; *Run* opens one in its project, in a session of
//! its name.

// Over the daemon's Unix socket.
#![cfg(unix)]

use crate::agentd::*;

use std::{os::unix::fs::PermissionsExt, path::Path};

use serde_json::{Value, json};

const TOKEN: &str = "fake-recipe-token-7601";

fn script(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A project with the recipe `fixer`, a skill, and a server in its
/// `.mcp.json`; Claude Code's adapter is the fake.
struct Setup {
    proj: std::path::PathBuf,
    env: Vec<(&'static str, String)>,
}

fn setup(dir: &Path) -> Setup {
    let bin = dir.join("agents/claude/node_modules/.bin");
    std::fs::create_dir_all(&bin).unwrap();
    script(&bin.join("claude-agent-acp"), &format!("#!/bin/sh\nexec python3 {} \"$@\"\n", fake()));
    let proj = dir.join("proj");
    std::fs::create_dir_all(proj.join(".claude/agents")).unwrap();
    std::fs::create_dir_all(proj.join(".claude/skills/testing")).unwrap();
    std::fs::write(
        proj.join(".claude/skills/testing/SKILL.md"),
        "---\nname: testing\ndescription: tests\n---\nRun them.\n",
    )
    .unwrap();
    std::fs::write(
        proj.join(".claude/agents/fixer.md"),
        "---\nname: fixer\ndescription: Fixes failing tests\ntools: Read, Edit, Bash\nmodel: haiku\npermissionMode: acceptEdits\nskills: [testing, absent-skill]\nmcpServers:\n  - docs\n  - nowhere\n  - tracker:\n      type: http\n      url: http://127.0.0.1:9/mcp\n      headers:\n        Authorization: Bearer ${RECIPE_TOKEN}\n---\n\nYou fix tests.\n",
    )
    .unwrap();
    std::fs::write(proj.join(".mcp.json"), r#"{"mcpServers":{"docs":{"command":"python3","args":["-c","pass"]}}}"#)
        .unwrap();
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let env = vec![
        ("HOME", home.display().to_string()),
        ("ARUGULA_AGENTS_DIR", dir.join("agents").display().to_string()),
        ("XDG_CACHE_HOME", dir.join("cache").display().to_string()),
        ("RECIPE_TOKEN", TOKEN.to_owned()),
        ("GITHUB_TOKEN", String::new()),
        ("GH_TOKEN", String::new()),
    ];
    Setup { proj, env }
}

fn daemon(s: &Setup, agents: bool) -> Daemon {
    let env: Vec<(&str, &str)> = s.env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    Daemon::child_in(&[], &env, |state| {
        arugula_proto::flags::set(state, arugula_proto::flags::AGENTS, agents).unwrap();
    })
}

fn session(d: &Daemon, block: u64) -> Value {
    let sid = d.state(block)["session_id"].as_str().unwrap().to_owned();
    serde_json::from_str(&std::fs::read_to_string(d.sessions.join(format!("{sid}.json"))).unwrap()).unwrap()
}

#[test]
fn a_recipe_runs_with_its_skills_and_servers_or_says_what_it_left_out() {
    let dir = Scratch::new("recipe-wear");
    let s = setup(&dir);
    let d = daemon(&s, true);
    // As `arugula agent --as fixer` opens it.
    let config = json!({ "agent": "claude", "recipe": "fixer", "cwd": s.proj, "prompt": "hello" });
    let id = d.open_with(json!({ "type": "agent", "config": config }));
    assert_eq!(d.wait(id, "idle"), "done", "{}", d.state(id));
    let st = d.state(id);
    assert_eq!(st["recipe"], "fixer");
    let w = &st["worn"];
    assert_eq!(w["agent"], "fixer", "{st}");
    assert_eq!(w["recipe"], s.proj.join(".claude/agents/fixer.md").display().to_string());
    assert_eq!(w["skills"], json!(["testing"]));
    assert!(w["skills_missing"][0].as_str().unwrap().starts_with("absent-skill"), "{w}");
    let servers: Vec<&str> = w["servers"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
    assert_eq!(servers, ["docs", "tracker"], "{w}");
    let left: Vec<&str> = w["left_out"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
    assert_eq!(left, ["nowhere"], "{w}");
    assert!(w["left_out"][0]["why"].as_str().unwrap().contains("isn't configured"), "{w}");

    // What the session got.
    let sess = session(&d, id);
    let meta = &sess["meta"];
    let system = meta["systemPrompt"]["append"].as_str().unwrap();
    assert!(system.contains("You fix tests.") && system.ends_with("Run them."), "the skill preloaded: {system}");
    assert_eq!(meta["claudeCode"]["options"]["extraArgs"], json!({ "strict-mcp-config": "" }));
    let o = &meta["claudeCode"]["options"];
    assert_eq!(o["settingSources"], json!([]));
    assert_eq!(o["model"], "haiku");
    assert_eq!(o["tools"], json!(["Read", "Edit", "Bash"]));
    let plugin = o["plugins"][0]["path"].as_str().unwrap();
    assert!(Path::new(plugin).join("skills/testing/SKILL.md").is_file(), "{plugin}");
    assert_eq!(sess["mode"], "acceptEdits", "the recipe's permission mode");
    let mcp = sess["mcp"].as_array().unwrap();
    assert!(mcp.iter().any(|m| m["name"] == "docs"), "{mcp:?}");
    let tracker = mcp.iter().find(|m| m["name"] == "tracker").unwrap();
    // The credential by reference: its value never in what's sent.
    assert!(!tracker.to_string().contains(TOKEN), "{tracker}");
    assert!(tracker["headers"][0]["value"].as_str().unwrap().starts_with("${ARUGULA_FTN_TRACKER_H_"), "{tracker}");
    // Only the name in the saved layout.
    let layout = || std::fs::read_to_string(d.state.join("layout.json")).unwrap_or_default();
    d.wait_for("the config saved", || layout().contains("\"recipe\": \"fixer\""));
    assert!(!layout().contains(TOKEN), "{}", layout());
}

#[test]
fn recipes_and_cards_need_the_agents_flag() {
    let dir = Scratch::new("recipe-off");
    let s = setup(&dir);
    let d = daemon(&s, false);
    let config = json!({ "agent": "claude", "recipe": "fixer", "cwd": s.proj, "prompt": "hello" });
    let id = d.open_with(json!({ "type": "agent", "config": config }));
    d.wait_for("it says why", || d.state(id)["error"].as_str().is_some_and(|e| e.contains("agents on")));
    assert!(d.state(id)["session_id"].is_null(), "no session opened");
    for path in ["/api/a2a/agents", "/api/a2a/offers", "/api/a2a/agents/fixer/card"] {
        assert_eq!(d.raw("GET", path, None).0, 404, "{path}");
    }
    // Turned on, read on the next request.
    arugula_proto::flags::set(&d.state, arugula_proto::flags::AGENTS, true).unwrap();
    assert_eq!(d.get("/api/a2a/agents")["agents"], json!([]));
}

#[test]
fn the_owner_offers_a_recipe_and_its_card_is_a2a() {
    let dir = Scratch::new("recipe-offer");
    let s = setup(&dir);
    let d = daemon(&s, true);
    // Canonical: a Mac's temp dir is behind a symlink.
    let proj = s.proj.canonicalize().unwrap().display().to_string();
    // The recipes a project has, none offered.
    let rs = d.get(&format!("/api/a2a/recipes?dir={proj}"));
    assert_eq!(rs[0]["name"], "fixer", "{rs}");
    assert!(rs[0]["offered_from"].is_null());
    // An offer names a recipe the project has.
    let (status, body) = d.raw("POST", "/api/a2a/offers", Some(json!({ "agent": "nobody", "dir": proj })));
    assert_eq!(status, 400, "{body}");
    let list = d.post("/api/a2a/offers", json!({ "agent": "fixer", "dir": proj }));
    assert_eq!(list, json!([{ "agent": "fixer", "dir": proj }]));
    assert_eq!(d.get(&format!("/api/a2a/recipes?dir={proj}"))[0]["offered_from"], proj);

    let all = d.get("/api/a2a/agents");
    let card = &all["agents"][0];
    assert_eq!(card["name"], "fixer");
    assert_eq!(card["description"], "Fixes failing tests");
    let iface = &card["supportedInterfaces"][0];
    assert_eq!(iface["protocolBinding"], "JSONRPC");
    assert_eq!(iface["protocolVersion"], "1.0");
    assert!(iface["url"].as_str().unwrap().starts_with("arugula://"), "{iface}");
    assert!(iface["url"].as_str().unwrap().ends_with("/api/a2a/agents/fixer"), "{iface}");
    assert_eq!(card["capabilities"]["extensions"][0]["required"], true);
    assert_eq!(&d.get("/api/a2a/agents/fixer/card"), card);
    // Its interface speaks A2A 1.0 (M78): a call without the version is
    // refused as A2A says, and a message with no text is a bad request.
    let r =
        d.post("/api/a2a/agents/fixer", json!({ "jsonrpc": "2.0", "id": 7, "method": "SendMessage", "params": {} }));
    assert_eq!((r["id"].clone(), r["error"]["code"].clone()), (json!(7), json!(-32009)), "{r}");
    let r = d.post(
        "/api/a2a/agents/fixer?a2a-version=1.0",
        json!({ "jsonrpc": "2.0", "id": 8, "method": "SendMessage", "params": { "message": { "parts": [] } } }),
    );
    assert_eq!(r["error"]["code"], -32602, "{r}");

    // A recipe gone from disk: no card, and the owner sees why.
    std::fs::remove_file(s.proj.join(".claude/agents/fixer.md")).unwrap();
    let all = d.get("/api/a2a/agents");
    assert_eq!(all["agents"], json!([]));
    assert_eq!(all["broken"][0]["agent"], "fixer");
    assert_eq!(d.raw("GET", "/api/a2a/agents/fixer/card", None).0, 404);
    // Stopped.
    assert_eq!(d.post("/api/a2a/offers", json!({ "agent": "fixer" })), json!([]));
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@e", "-c", "init.defaultBranch=main"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// The project as a clone of an origin that has moved on since: its
/// `HEAD` is behind `origin/main`, once fetched.
fn with_origin(dir: &Path, proj: &Path) -> String {
    let origin = dir.join("origin.git");
    git(dir, &["init", "-q", "--bare", origin.to_str().unwrap()]);
    git(proj, &["init", "-q"]);
    git(proj, &["add", "-A"]);
    git(proj, &["commit", "-q", "-m", "first"]);
    git(proj, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git(proj, &["push", "-q", "origin", "HEAD:main"]);
    git(proj, &["fetch", "-q", "origin"]);
    git(proj, &["remote", "set-head", "origin", "main"]);
    let other = dir.join("other");
    git(dir, &["clone", "-q", origin.to_str().unwrap(), other.to_str().unwrap()]);
    std::fs::write(other.join("upstream.txt"), "newer\n").unwrap();
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "HEAD:main"]);
    git(&other, &["rev-parse", "HEAD"])
}

fn task_of(r: &Value) -> &Value {
    &r["task"]
}

fn artifact<'a>(t: &'a Value, name: &str) -> &'a Value {
    t["artifacts"].as_array().unwrap().iter().find(|a| a["name"] == name).unwrap_or(&Value::Null)
}

#[test]
fn a_task_for_an_agent_offered_here_runs_here_and_cleans_up_once_read() {
    let dir = Scratch::new("recipe-task-here");
    let s = setup(&dir);
    with_origin(&dir, &s.proj);
    let d = daemon(&s, true);
    let proj = s.proj.canonicalize().unwrap();
    let head = git(&proj, &["rev-parse", "HEAD"]);
    d.post("/api/a2a/offers", json!({ "agent": "fixer", "dir": proj }));

    // No machine named: this one offers it. No control needed.
    let r = d.post(
        "/api/a2a/delegate",
        json!({ "kind": "send", "agent": "fixer", "text": "write notes.txt hi", "wait": 60 }),
    );
    let t = task_of(&r);
    assert_eq!(t["status"]["state"], "TASK_STATE_COMPLETED", "{r}");
    assert_eq!(t["metadata"]["arugula"]["deliver"], "patch", "{t}");
    let patch = artifact(t, "patch");
    assert_eq!(patch["metadata"]["arugula"]["base"], head.as_str(), "from the project's HEAD: {t}");
    assert!(patch["parts"][0]["text"].as_str().unwrap().contains("+++ b/notes.txt"), "{patch}");
    assert!(artifact(t, "pr").is_null(), "{t}");
    assert!(r["summary"].as_str().unwrap().contains("{kind: review}"), "{r}");
    // Read by its caller once it ended: its worktree and block go.
    let id = t["id"].as_str().unwrap();
    let wt = d.state.join("a2a/work").join(id);
    d.wait_for("its worktree removed", || !wt.exists());
    // A machine named as this one works too.
    let r = d.post(
        "/api/a2a/delegate",
        json!({ "kind": "send", "machine": "here", "agent": "fixer", "text": "hello", "wait": 60 }),
    );
    assert_eq!(task_of(&r)["status"]["state"], "TASK_STATE_COMPLETED", "{r}");
    // One that isn't offered here, with no machine to ask: said so.
    let (status, body) =
        d.raw("POST", "/api/a2a/delegate", Some(json!({ "kind": "send", "agent": "nobody", "text": "hi" })));
    assert_eq!(status, 404, "{body}");
    assert!(body.contains("no machine you reach offers nobody"), "{body}");
}

#[test]
fn a_task_delivered_as_a_pull_request_works_on_a_branch_off_the_origin() {
    let dir = Scratch::new("recipe-task-pr");
    let s = setup(&dir);
    let upstream = with_origin(&dir, &s.proj);
    let d = daemon(&s, true);
    let proj = s.proj.canonicalize().unwrap();
    d.post("/api/a2a/offers", json!({ "agent": "fixer", "dir": proj }));

    // `meta`: the fake says what its session was told.
    let r = d
        .post("/api/a2a/delegate", json!({ "kind": "send", "agent": "fixer", "text": "meta", "pr": true, "wait": 60 }));
    let t = task_of(&r);
    assert_eq!(t["status"]["state"], "TASK_STATE_COMPLETED", "{r}");
    let id = t["id"].as_str().unwrap();
    let branch = format!("a2a/fixer-{id}");
    assert_eq!(t["metadata"]["arugula"]["deliver"], "pr", "{t}");
    assert_eq!(t["metadata"]["arugula"]["branch"], branch.as_str(), "{t}");
    // Off origin's main as fetched, not the project's own HEAD.
    assert_eq!(t["metadata"]["arugula"]["base"], upstream.as_str(), "{t}");
    // Told after its own prompt.
    let reply = artifact(t, "reply")["parts"][0]["text"].as_str().unwrap();
    assert!(reply.contains("You fix tests."), "{reply}");
    assert!(reply.contains("Deliver this task as a pull request") && reply.contains(&branch), "{reply}");
    assert!(reply.contains("against main"), "{reply}");
    // No link in its reply, nothing pushed: said so.
    let pr = artifact(t, "pr");
    let m = &pr["metadata"]["arugula"];
    assert_eq!(
        (m["url"].clone(), m["pushed"].clone(), m["from"].clone()),
        (Value::Null, json!(false), json!("origin/main"))
    );
    assert!(r["summary"].as_str().unwrap().contains("Its pull request: No pull request link"), "{r}");
    assert!(!r["summary"].as_str().unwrap().contains("{kind: review}"), "{r}");
    // Cleaned up once read; the branch stays, being unpushed work.
    let wt = d.state.join("a2a/work").join(id);
    d.wait_for("its worktree removed", || !wt.exists());
    assert_eq!(git(&proj, &["rev-parse", &branch]), upstream);
}

#[test]
fn run_opens_an_offered_agent_in_its_project_in_a_session_of_its_name() {
    let dir = Scratch::new("recipe-run");
    let s = setup(&dir);
    let d = daemon(&s, true);
    let proj = s.proj.canonicalize().unwrap();
    let (status, body) = d.raw("POST", "/api/a2a/run", Some(json!({ "agent": "fixer" })));
    assert_eq!(status, 404, "not offered anywhere: {body}");
    d.post("/api/a2a/offers", json!({ "agent": "fixer", "dir": proj }));
    let r = d.post("/api/a2a/run", json!({ "agent": "fixer", "prompt": "hello" }));
    let block = r["block"].as_u64().unwrap_or_else(|| panic!("{r}"));
    assert!(r["id"].is_null(), "this machine: {r}");
    assert_eq!(d.wait(block, "idle"), "done", "{}", d.state(block));
    assert_eq!(d.state(block)["recipe"], "fixer");
    let panes = d.get("/api/panes");
    let p = panes.as_array().unwrap().iter().find(|p| p["id"] == block).unwrap();
    assert_eq!(p["session_name"], "fixer", "{p}");
    assert_eq!(session(&d, block)["cwd"], proj.display().to_string(), "in its project");
    // The route another machine of the account calls: the same.
    let again = d.post("/api/a2a/agents/fixer/run", json!({}));
    assert!(again["block"].as_u64().is_some(), "{again}");
}
