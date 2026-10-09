//! Delegating (M78, #400), this machine as the caller: a task for an agent
//! another machine offers, over A2A 1.0 JSON-RPC through control's relay
//! (`peer.rs`), signed with this daemon's own key, so the other machine
//! knows which account asks.
//!
//! An agent here uses it through MCP's `delegate` tool: `send` (and wait),
//! `get` (and wait), `answer` (a question the task asked) and `cancel`.
//! Each ended task's result (its reply, and its patch) is kept in
//! `<state>/a2a/results/TASK.json`, since the other machine cleans up once
//! it's been read.

use std::{sync::Arc, time::Duration};

use serde_json::{Value, json};

use super::tasks::{CANCELED, COMPLETED, FAILED, INPUT_REQUIRED, REJECTED};
use crate::{server::App, store::now_ms};

const HEADERS: [(&str, &str); 1] = [("A2A-Version", super::A2A_VERSION)];

/// One JSON-RPC call to `agent` on `machine`: its result, or its error.
async fn call(app: &Arc<App>, machine: &str, agent: &str, method: &str, params: Value) -> Result<Value, String> {
    let m = crate::peer::find(&app.control, machine).await.map_err(|e| e.to_string())?;
    let body = json!({ "jsonrpc": "2.0", "id": now_ms(), "method": method, "params": params });
    let path = format!("/api/a2a/agents/{}", agent);
    let (status, bytes) = crate::peer::request(&app.control, &m, "POST", &path, Some(&body), &HEADERS)
        .await
        .map_err(|e| format!("{machine}: {e:#}"))?;
    let v: Value = serde_json::from_slice(&bytes).unwrap_or_default();
    if status != 200 {
        let why =
            v["error"].as_str().map(str::to_owned).unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned());
        return Err(format!("{machine} said {status}: {why}"));
    }
    if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
        return Err(format!("{machine}/{agent}: {}", e["message"].as_str().unwrap_or("an error")));
    }
    Ok(v["result"].clone())
}

/// Send `text` to `agent` on `machine`: a new task, or (with `task`) the
/// next message on one (an answer to its question, or a follow-up).
pub async fn send(
    app: &Arc<App>,
    machine: &str,
    agent: &str,
    text: &str,
    task: Option<&str>,
    claims: Value,
) -> Result<Value, String> {
    let mut message = json!({
        "messageId": format!("m{}", now_ms()),
        "role": "ROLE_USER",
        "parts": [{ "text": text }],
        "metadata": { "arugula": claims },
    });
    if let Some(t) = task {
        message["taskId"] = json!(t);
    }
    let r = call(
        app,
        machine,
        agent,
        "SendMessage",
        json!({ "message": message, "configuration": { "returnImmediately": true } }),
    )
    .await?;
    Ok(r["task"].clone())
}

pub async fn get(app: &Arc<App>, machine: &str, agent: &str, task: &str) -> Result<Value, String> {
    let t = call(app, machine, agent, "GetTask", json!({ "id": task, "historyLength": 0 })).await?;
    keep(app, machine, agent, &t);
    Ok(t)
}

pub async fn cancel(app: &Arc<App>, machine: &str, agent: &str, task: &str) -> Result<Value, String> {
    call(app, machine, agent, "CancelTask", json!({ "id": task })).await
}

/// Whether a task's state is one it stays in (or waits in for the caller).
pub fn settled(t: &Value) -> bool {
    let s = t["status"]["state"].as_str().unwrap_or_default();
    [COMPLETED, FAILED, CANCELED, REJECTED, INPUT_REQUIRED].contains(&s)
}

/// Read the task every couple of seconds until it settles, or `limit` has
/// passed: as it is then.
pub async fn wait(app: &Arc<App>, machine: &str, agent: &str, task: &str, limit: Duration) -> Result<Value, String> {
    let until = std::time::Instant::now() + limit;
    loop {
        let t = get(app, machine, agent, task).await?;
        if settled(&t) || std::time::Instant::now() >= until {
            return Ok(t);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// An ended task's result, kept here (the other machine cleans up).
fn keep(app: &App, machine: &str, agent: &str, t: &Value) {
    let s = t["status"]["state"].as_str().unwrap_or_default();
    let Some(id) = t["id"].as_str().filter(|_| [COMPLETED, FAILED, CANCELED, REJECTED].contains(&s)) else { return };
    let dir = app.control.state_dir().join("a2a").join("results");
    let _ = std::fs::create_dir_all(&dir);
    let v = json!({ "machine": machine, "agent": agent, "task": t, "kept_ms": now_ms() });
    if let Ok(b) = serde_json::to_vec_pretty(&v) {
        let _ =
            crate::store::write_atomic(&dir.join(format!("{}.json", super::super::fountain::wear::component(id))), &b);
    }
}

/// A task in a few lines for an agent: its state, what it says or waits
/// on, its reply, and its patch's stat (and what was left out of it).
pub fn summary(machine: &str, agent: &str, t: &Value) -> String {
    let id = t["id"].as_str().unwrap_or("?");
    let state = t["status"]["state"].as_str().unwrap_or("?").trim_start_matches("TASK_STATE_").to_lowercase();
    let mut out = format!("{agent} on {machine}, task {id}: {state}");
    if let Some(m) = t["status"]["message"]["parts"][0]["text"].as_str() {
        out.push_str(&format!(" ({m})"));
    }
    out.push('\n');
    for a in t["artifacts"].as_array().into_iter().flatten() {
        match a["name"].as_str() {
            Some("reply") => {
                let r = a["parts"][0]["text"].as_str().unwrap_or_default();
                if !r.is_empty() {
                    out.push_str(&format!("Its reply:\n{r}\n"));
                }
            }
            Some("patch") => {
                let m = &a["metadata"]["arugula"];
                out.push_str(&format!(
                    "A patch against {}:\n{}",
                    m["base"].as_str().unwrap_or("its base"),
                    m["stat"].as_str().unwrap_or_default()
                ));
                let left: Vec<&str> =
                    m["left_out"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                if !left.is_empty() {
                    out.push_str(&format!("Left out of it (new binary files): {}\n", left.join(", ")));
                }
            }
            _ => {}
        }
    }
    match state.as_str() {
        "input_required" => out.push_str("It asks you: answer with delegate {kind: answer, …, text}.\n"),
        "submitted" => out.push_str("It waits for that machine's owner to allow it.\n"),
        "working" => out.push_str("Still working: delegate {kind: get, …, wait} follows it.\n"),
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_says_state_reply_and_patch() {
        let t = json!({
            "id": "t1",
            "status": { "state": "TASK_STATE_COMPLETED" },
            "artifacts": [
                { "name": "reply", "parts": [{ "text": "Fixed both." }] },
                { "name": "patch", "parts": [{ "text": "diff…" }], "metadata": { "arugula": { "base": "abc", "stat": " calc.py | 2 +-\n", "left_out": ["x.pyc"] } } },
            ],
        });
        let s = summary("ale-box", "fixer", &t);
        assert!(s.starts_with("fixer on ale-box, task t1: completed"), "{s}");
        assert!(s.contains("Fixed both.") && s.contains("calc.py | 2 +-") && s.contains("x.pyc"), "{s}");
        assert!(settled(&t));
        let w = json!({ "id": "t2", "status": { "state": "TASK_STATE_SUBMITTED", "message": { "parts": [{ "text": "waiting for the owner" }] } } });
        assert!(!settled(&w));
        assert!(summary("m", "a", &w).contains("allow it"));
    }
}
