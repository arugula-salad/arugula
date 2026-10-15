//! Managoat F4 (#942): a recipe (a Claude Code subagent file) run on
//! Managoat, against a stub Managoat served here: its agents, environments
//! and vaults, `POST /api/apply`, secrets by key, and conversations with
//! their streams. Nothing reaches a real Managoat.
//!
//! What's checked (F4's "done when"): from the agents block, the preflight
//! names what doesn't carry over and the variables Managoat lacks, without
//! writing anything; a variable is copied to the vault only when asked, its
//! value never in an answer or the block; *Run on Managoat* upserts the
//! recipe as `recipe-fixer` (managed by Arugula, with its project as an
//! environment) in one apply, and the conversation it opens carries the
//! recipe's hash as a label and answers over the stream; a second run
//! applies nothing new; an agent of that name Arugula didn't make is never
//! written; MCP's `start_agent {on: fountain}` and the CLI's `agents run
//! --fountain` run it too.

// Over the daemon's Unix socket.
#![cfg(unix)]

use crate::agentd;

use std::{
    collections::BTreeMap,
    convert::Infallible,
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use agentd::*;
use axum::{
    Json, Router,
    body::Body,
    extract::{Path as UrlPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

const KEY: &str = "ftn_f4_test_key";
const TOKEN: &str = "s3cr3t-recipe-token-f4";

#[derive(Default)]
struct Inner {
    agents: Vec<Value>,
    envs: Vec<Value>,
    /// `(vault id, key)` of each secret.
    secrets: Vec<(String, String)>,
    /// `(holder id, key, value)` of each secret written.
    written: Vec<(String, String, String)>,
    applies: Vec<Value>,
    created: Vec<Value>,
    next: u64,
    events: Vec<(u64, String, Value)>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<Inner>>);

impl Fake {
    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        f(&mut self.0.lock().unwrap())
    }

    fn push(&self, conv: &str, p: Value) {
        self.with(|i| {
            i.next += 1;
            let id = i.next;
            i.events.push((id, conv.to_owned(), p));
        });
    }

    fn stage(&self, conv: &str, stage: &str, state: &str, meta: Value) {
        let turn = meta["turn_id"].clone();
        self.push(
            conv,
            json!({ "kind": "stage", "stream": "", "data": meta.to_string(), "stage": stage, "state": state, "turn_id": turn }),
        );
    }

    fn after(&self, conv: &str, after: u64) -> Vec<(u64, Value)> {
        self.with(|i| {
            i.events.iter().filter(|(id, c, _)| *id > after && c == conv).map(|(id, _, p)| (*id, p.clone())).collect()
        })
    }
}

fn authed(h: &HeaderMap) -> bool {
    h.get("authorization").and_then(|a| a.to_str().ok()) == Some(&format!("Bearer {KEY}"))
}

fn err(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({ "error": code }))).into_response()
}

async fn agents(State(f): State<Fake>, h: HeaderMap) -> Response {
    if !authed(&h) {
        return err(StatusCode::UNAUTHORIZED, "invalid_api_key");
    }
    Json(json!({ "data": f.with(|i| i.agents.clone()) })).into_response()
}

async fn environments(State(f): State<Fake>) -> Response {
    Json(json!({ "data": f.with(|i| i.envs.clone()) })).into_response()
}

async fn vaults() -> Response {
    Json(json!({ "data": [{ "id": "vault-1", "name": "v1" }] })).into_response()
}

async fn secrets(State(f): State<Fake>, UrlPath(id): UrlPath<String>) -> Response {
    let keys: Vec<Value> = f.with(|i| {
        i.secrets.iter().filter(|(h, _)| *h == id).map(|(_, k)| json!({ "id": format!("s-{k}"), "key": k })).collect()
    });
    Json(json!({ "data": keys })).into_response()
}

async fn set_secret(State(f): State<Fake>, UrlPath(id): UrlPath<String>, Json(b): Json<Value>) -> Response {
    let key = b["key"].as_str().unwrap_or_default().to_owned();
    f.with(|i| {
        i.written.push((id.clone(), key.clone(), b["value"].as_str().unwrap_or_default().to_owned()));
        if !i.secrets.iter().any(|(h, k)| *h == id && *k == key) {
            i.secrets.push((id.clone(), key.clone()));
        }
    });
    (StatusCode::CREATED, Json(json!({ "data": { "id": format!("s-{key}"), "key": key } }))).into_response()
}

/// Managoat's apply: environments, then agents; upsert by name; a row is
/// `unchanged` when the spec is what's there.
async fn apply(State(f): State<Fake>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if !authed(&h) {
        return err(StatusCode::UNAUTHORIZED, "invalid_api_key");
    }
    let results = f.with(|i| {
        i.applies.push(b.clone());
        let mut out = vec![];
        let mut docs: Vec<Value> = b["resources"].as_array().cloned().unwrap_or_default();
        docs.sort_by_key(|d| d["kind"] != "Environment");
        for d in docs {
            let (kind, name, spec) = (d["kind"].as_str().unwrap(), d["name"].as_str().unwrap(), d["spec"].clone());
            let (list, prefix) = if kind == "Agent" { (&mut i.agents, "agent") } else { (&mut i.envs, "env") };
            let mut record = spec.clone();
            record["name"] = json!(name);
            let action = match list.iter_mut().find(|a| a["name"] == name) {
                Some(a) => {
                    record["id"] = a["id"].clone();
                    // Absent keys stay as they were (the model, on inherit).
                    let mut merged = a.clone();
                    for (k, v) in record.as_object().unwrap() {
                        merged[k] = v.clone();
                    }
                    let same = merged == *a;
                    *a = merged;
                    if same { "unchanged" } else { "updated" }
                }
                None => {
                    record["id"] = json!(format!("{prefix}-{name}"));
                    list.push(record);
                    "created"
                }
            };
            out.push(json!({ "kind": kind, "name": name, "action": action, "errors": null, "secrets": [] }));
        }
        out
    });
    Json(json!({ "data": { "results": results } })).into_response()
}

fn conv_json(id: &str) -> Value {
    json!({ "id": id, "status": "idle", "agent_id": "agent-recipe-fixer", "sandbox_id": "sb-1",
            "sandbox": { "id": "sb-1", "status": "ready" }, "runtime": "claude" })
}

async fn create(State(f): State<Fake>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if !authed(&h) {
        return err(StatusCode::UNAUTHORIZED, "invalid_api_key");
    }
    let id = f.with(|i| {
        i.created.push(b.clone());
        format!("conv-{}", i.created.len())
    });
    f.stage(&id, "provision", "done", json!({}));
    (StatusCode::CREATED, Json(json!({ "data": conv_json(&id) }))).into_response()
}

async fn conversation(UrlPath(id): UrlPath<String>) -> Response {
    Json(json!({ "data": conv_json(&id) })).into_response()
}

async fn turns() -> Response {
    Json(json!({ "data": [] })).into_response()
}

/// A prompt: a turn that says `echo: TEXT` and ends.
async fn prompts(State(f): State<Fake>, UrlPath(id): UrlPath<String>, Json(b): Json<Value>) -> Response {
    let text = b["prompt"].as_str().unwrap_or_default().to_owned();
    let crid = b["client_request_id"].clone();
    let turn = format!("turn-{}", f.with(|i| i.next));
    f.stage(&id, "turn", "started", json!({ "turn_id": turn, "client_request_id": crid }));
    let line = json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "s",
        "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": format!("echo: {text}") } } } });
    f.push(&id, json!({ "kind": "output", "stream": "acp", "data": line.to_string(), "turn_id": turn }));
    f.stage(&id, "turn", "done", json!({ "turn_id": turn, "stop_reason": "end_turn" }));
    Json(json!({ "data": { "status": "accepted", "client_request_id": crid } })).into_response()
}

/// The conversation's SSE: what's after `Last-Event-ID`, then what comes,
/// until it's quiet a while (Managoat's idle close, short here).
async fn stream(
    State(f): State<Fake>,
    h: HeaderMap,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let after: u64 = h.get("last-event-id").and_then(|v| v.to_str().ok()?.parse().ok()).unwrap_or(0);
    let wait = q.get("wait").is_none_or(|w| w != "false");
    let frame = |n: u64, p: &Value| format!("id: {n}\nevent: {}\ndata: {p}\n\n", p["kind"].as_str().unwrap());
    let first: String = std::iter::once(": connected\n\n".to_owned())
        .chain(f.after(&id, after).iter().map(|(n, p)| frame(*n, p)))
        .collect();
    let last = f.after(&id, after).last().map_or(after, |e| e.0);
    if !wait {
        return Response::builder().header("content-type", "text/event-stream").body(Body::from(first)).unwrap();
    }
    let st = (f, id, last, Instant::now(), Some(first));
    let s = futures_util::stream::unfold(st, move |(f, id, last, quiet, first)| async move {
        if let Some(c) = first {
            return Some((Ok::<_, Infallible>(c), (f, id, last, Instant::now(), None)));
        }
        loop {
            tokio::time::sleep(Duration::from_millis(25)).await;
            let new = f.after(&id, last);
            if let Some(&(newest, _)) = new.last() {
                let chunk: String = new.iter().map(|(n, p)| frame(*n, p)).collect();
                return Some((Ok(chunk), (f, id, newest, Instant::now(), None)));
            }
            if quiet.elapsed() > Duration::from_millis(1500) {
                return None;
            }
        }
    });
    Response::builder().header("content-type", "text/event-stream").body(Body::from_stream(s)).unwrap()
}

struct Managoat {
    _rt: tokio::runtime::Runtime,
    f: Fake,
    origin: String,
}

impl Managoat {
    fn start() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let f = Fake::default();
        // chant's agent, which a recipe of the same name must never touch.
        f.with(|i| {
            i.agents.push(json!({ "id": "agent-chanted", "name": "recipe-chanted", "runtime": "claude",
                                  "metadata": { "managed-by": "chant" } }))
        });
        let origin = rt.block_on(async {
            let api = Router::new()
                .route("/api/agents", get(agents))
                .route("/api/environments", get(environments))
                .route("/api/vaults", get(vaults))
                .route("/api/vaults/{id}/secrets", get(secrets).post(set_secret))
                .route("/api/environments/{id}/secrets", get(secrets).post(set_secret))
                .route("/api/apply", post(apply))
                .route("/api/conversations", post(create))
                .route("/api/conversations/{id}", get(conversation))
                .route("/api/conversations/{id}/turns", get(turns))
                .route("/api/conversations/{id}/prompts", post(prompts))
                .route("/api/conversations/{id}/stream", get(stream))
                .with_state(f.clone());
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let at = format!("http://{}", l.local_addr().unwrap());
            tokio::spawn(async move { axum::serve(l, api).await.unwrap() });
            at
        });
        Self { _rt: rt, f, origin }
    }

    fn daemon(&self, dir: &Path) -> Daemon {
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let home = home.display().to_string();
        // Recipes are the `agents` flag's; Managoat needs none (#942, F5).
        Daemon::child_in(
            &["--wisp-token-file", "/nonexistent", "--tailscale-socket", "/nonexistent/sock"],
            &[
                ("HOME", home.as_str()),
                ("MANAGOAT_API_KEY", KEY),
                ("MANAGOAT_BASE_URL", self.origin.as_str()),
                ("ARUGULA_FOUNTAIN_STREAM_RETRY_MS", "50"),
                ("RECIPE_TOKEN", TOKEN),
                ("GITHUB_TOKEN", ""),
                ("GH_TOKEN", ""),
                ("ARUGULA_GH_BIN", "/nonexistent/gh"),
            ],
            |state| arugula_proto::flags::set(state, arugula_proto::flags::AGENTS, true).unwrap(),
        )
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success();
    assert!(ok, "git {args:?}");
}

/// A checkout of acme/widgets with the recipes `fixer` and `chanted`, and
/// work that isn't committed.
fn project(dir: &Path) -> std::path::PathBuf {
    let proj = dir.join("widgets");
    std::fs::create_dir_all(proj.join(".claude/agents")).unwrap();
    std::fs::create_dir_all(proj.join(".claude/skills/testing/scripts")).unwrap();
    std::fs::write(proj.join(".claude/skills/testing/SKILL.md"), "---\nname: testing\n---\nRun them.\n").unwrap();
    std::fs::write(proj.join(".claude/skills/testing/scripts/run.sh"), "make test\n").unwrap();
    std::fs::write(
        proj.join(".claude/agents/fixer.md"),
        "---\nname: fixer\ndescription: Fixes failing tests\ntools: Read, Edit, Grep\nmodel: haiku\nskills: [testing]\nmcpServers:\n  - tracker:\n      type: http\n      url: https://tracker.example.com/mcp\n      headers:\n        Authorization: Bearer ${RECIPE_TOKEN}\n---\n\nYou fix tests.\n",
    )
    .unwrap();
    std::fs::write(proj.join(".claude/agents/chanted.md"), "---\nname: chanted\n---\nChant.\n").unwrap();
    git(&proj, &["init", "-q", "-b", "main"]);
    git(&proj, &["remote", "add", "origin", "git@github.com:acme/widgets.git"]);
    std::fs::write(proj.join("wip.txt"), "not committed\n").unwrap();
    proj
}

fn texts(st: &Value) -> Vec<String> {
    entries(st).iter().filter(|e| e["type"] == "agent").filter_map(|e| e["text"].as_str().map(str::to_owned)).collect()
}

/// F4's "done when": the recipe that runs here runs on Managoat from the
/// same card, and its conversation carries the recipe's hash.
#[test]
fn a_recipe_runs_on_fountain_as_its_own_agent_with_its_hash() {
    let fz = Managoat::start();
    let dir = Scratch::new("fountain-recipes");
    let proj = project(&dir);
    let d = fz.daemon(&dir);
    let cwd = proj.display().to_string();
    let block = d.open_with(json!({ "type": "agents", "local": true, "config": { "dir": cwd } }));

    // The preflight: what doesn't carry over, the checkout, and what
    // Managoat lacks. Nothing is written.
    let p = d.call(block, "fountain_preflight", json!({ "agent": "fixer", "cwd": cwd, "vault": "v1" }));
    assert_eq!(p["agent"], "recipe-fixer", "{p}");
    assert_eq!(p["exists"], false);
    assert_eq!(p["refused"], Value::Null);
    assert_eq!(p["repo"]["url"], "https://github.com/acme/widgets.git");
    assert_eq!(p["environment"], "repo-acme-widgets");
    assert!(p["repo"]["stays"].as_array().unwrap().iter().any(|s| s == "uncommitted changes"), "{p}");
    assert_eq!(p["missing"], json!(["GITHUB_TOKEN", "RECIPE_TOKEN"]));
    assert_eq!(p["model"], "anthropic/claude-haiku-5-5");
    let left = p["left_out"].to_string();
    assert!(left.contains("SKILL.md only") && left.contains("scripts/run.sh"), "{left}");
    assert!(p["notes"].to_string().contains("no tool allowlist"), "{p}");
    assert!(p["summary"].as_str().unwrap().contains("2 variable(s) missing"), "{p}");
    let hash = p["hash"].as_str().unwrap().to_owned();
    assert!(fz.f.with(|i| i.applies.is_empty() && i.written.is_empty()));

    // A copy, as asked: once, to the vault; the value goes nowhere else.
    let (status, body) = d.raw(
        "POST",
        &format!("/api/blocks/{block}/call/fountain_copy"),
        Some(json!({ "agent": "fixer", "cwd": cwd, "var": "RECIPE_TOKEN", "vault": "v1" })),
    );
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("shell environment") && !body.contains(TOKEN), "{body}");
    assert_eq!(fz.f.with(|i| i.written.clone()), [("vault-1".to_owned(), "RECIPE_TOKEN".to_owned(), TOKEN.to_owned())]);
    assert!(!d.state(block).to_string().contains(TOKEN));
    // What isn't set here, and what the recipe doesn't use, aren't copied.
    let (status, body) = d.raw(
        "POST",
        &format!("/api/blocks/{block}/call/fountain_copy"),
        Some(json!({ "agent": "fixer", "cwd": cwd, "var": "GITHUB_TOKEN", "vault": "v1" })),
    );
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("isn't set on this machine"), "{body}");
    let (status, body) = d.raw(
        "POST",
        &format!("/api/blocks/{block}/call/fountain_copy"),
        Some(json!({ "agent": "fixer", "cwd": cwd, "var": "HOME", "vault": "v1" })),
    );
    assert_eq!(status, 400, "{body}");
    assert_eq!(fz.f.with(|i| i.written.len()), 1);
    let p = d.call(block, "fountain_preflight", json!({ "agent": "fixer", "cwd": cwd, "vault": "v1" }));
    assert_eq!(p["missing"], json!(["GITHUB_TOKEN"]));
    assert!(p["vars"].as_array().unwrap().contains(&json!({ "name": "RECIPE_TOKEN", "on": "vault" })), "{p}");

    // Run on Managoat: one apply, the environment and the agent.
    let r = d.call(block, "run_fountain", json!({ "agent": "fixer", "cwd": cwd, "vault": "v1", "prompt": "hello" }));
    let agent = r["block"].as_u64().unwrap();
    let applies = fz.f.with(|i| i.applies.clone());
    assert_eq!(applies.len(), 1);
    let docs = applies[0]["resources"].as_array().unwrap();
    assert_eq!(docs.len(), 2, "{docs:?}");
    let (env, spec) = (&docs[0], &docs[1]);
    assert_eq!((env["kind"].as_str(), env["name"].as_str()), (Some("Environment"), Some("repo-acme-widgets")));
    assert_eq!(
        env["spec"]["repositories"],
        json!([{ "url": "https://github.com/acme/widgets.git", "mount_path": "/work/widgets", "secret_key": "GITHUB_TOKEN" }])
    );
    assert_eq!((spec["kind"].as_str(), spec["name"].as_str()), (Some("Agent"), Some("recipe-fixer")));
    let s = &spec["spec"];
    assert_eq!(s["runtime"], "claude");
    assert_eq!(s["model"], "anthropic/claude-haiku-5-5");
    assert!(s["system"].as_str().unwrap().starts_with("You fix tests."), "{s}");
    assert_eq!(s["skills"], json!([{ "name": "testing", "content": "---\nname: testing\n---\nRun them.\n" }]));
    assert_eq!(s["mcp_servers"]["tracker"]["headers"]["Authorization"], "Bearer ${RECIPE_TOKEN}");
    assert_eq!(s["permission_policy"]["execute"], "auto_deny");
    assert_eq!(
        s["metadata"],
        json!({ "managed-by": "arugula", "source_path": proj.join(".claude/agents/fixer.md").display().to_string(), "recipe_hash": hash })
    );
    assert!(!applies[0].to_string().contains(TOKEN));

    // The conversation: the upserted agent, the project's environment, the
    // vault, the agent's own policy, and the labels.
    d.wait_for("the conversation", || fz.f.with(|i| i.created.len()) == 1);
    let c = fz.f.with(|i| i.created[0].clone());
    assert_eq!(c["agent_id"], "agent-recipe-fixer", "{c}");
    assert_eq!(c["environment_id"], "env-repo-acme-widgets");
    assert_eq!(c["vault_id"], "vault-1");
    assert_eq!(c["permission_policy"], s["permission_policy"]);
    assert_eq!(c["labels"]["arugula.recipe"], "fixer");
    assert_eq!(c["labels"]["arugula.recipe_hash"], hash.as_str());
    assert!(c["labels"]["arugula.machine"].as_str().is_some_and(|m| !m.is_empty()), "{c}");
    assert_eq!(r["labels"], c["labels"]);
    // It answers over the stream.
    let deadline = Instant::now() + Duration::from_secs(20);
    while !texts(&d.state(agent)).iter().any(|t| t == "echo: hello") {
        assert!(Instant::now() < deadline, "no answer: {}", d.state(agent));
        std::thread::sleep(Duration::from_millis(50));
    }
    let said = d.state(block)["said"].as_str().unwrap_or_default().to_owned();
    assert!(said.contains("recipe-fixer") && said.contains("SKILL.md only"), "{said}");

    // Again: nothing new to write.
    d.call(block, "run_fountain", json!({ "agent": "fixer", "cwd": cwd, "vault": "v1" }));
    let rows = fz.f.with(|i| i.applies.len());
    assert_eq!(rows, 2);
    let said = d.state(block)["said"].as_str().unwrap_or_default().to_owned();
    assert!(
        said.contains("Agent recipe-fixer: unchanged") && said.contains("Environment repo-acme-widgets: unchanged"),
        "{said}"
    );

    // An agent of the recipe's name that Arugula didn't make: left alone.
    let (status, body) = d.raw(
        "POST",
        &format!("/api/blocks/{block}/call/run_fountain"),
        Some(json!({ "agent": "chanted", "cwd": cwd })),
    );
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("didn't make"), "{body}");
    assert_eq!(fz.f.with(|i| i.applies.len()), 2, "nothing applied for it");
    assert_eq!(
        fz.f.with(|i| i.agents.iter().find(|a| a["name"] == "recipe-chanted").unwrap()["metadata"].clone()),
        json!({ "managed-by": "chant" })
    );

    // MCP: start_agent {recipe, on: fountain}.
    let m = Mcp::bridge(&d, None);
    let r = m
        .call(
            "start_agent",
            json!({ "recipe": "fixer", "on": "managoat", "cwd": cwd, "vault": "v1", "prompt": "via mcp" }),
        )
        .unwrap();
    assert!(r["block"].as_u64().is_some(), "{r}");
    d.wait_for("the MCP run's conversation", || fz.f.with(|i| i.created.len()) == 3);
    let created = fz.f.with(|i| i.created.clone());
    assert!(created.iter().all(|c| c["labels"]["arugula.recipe_hash"] == hash.as_str()), "{created:?}");
    assert!(m.call("start_agent", json!({ "on": "fountain", "prompt": "x" })).unwrap_err().contains("give recipe"));

    // The CLI: agents run --fountain.
    let cli = Path::new(env!("CARGO_BIN_EXE_arugulad")).with_file_name("arugula");
    let out = Command::new(cli)
        .arg("--socket")
        .arg(d.sock())
        .args(["agents", "run", "fixer", "--fountain", "--vault", "v1", "--cwd", &cwd, "from", "the", "cli"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("fixer runs on Managoat as recipe-fixer"), "{said}");
    d.wait_for("the CLI run's conversation", || fz.f.with(|i| i.created.len()) == 4);
    assert!(fz.f.with(|i| i.created.iter().all(|c| c["labels"]["arugula.recipe"] == "fixer")));
}
