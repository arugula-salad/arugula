//! Delegating (M78, #400), this machine as the caller: a task for an agent
//! another machine offers, over A2A 1.0 JSON-RPC through control's relay
//! (`peer.rs`), signed with this daemon's own key, so the other machine
//! knows which account asks.
//!
//! **An agent this machine offers** is reached the same way without the
//! relay: the same JSON-RPC, answered here as this machine's own account
//! (#403's follow-up). The machine can be left out: [`locate`] finds the
//! one that offers the agent, this one first.
//!
//! **Delivered as a pull request** (`pr`): the receiver works on a branch
//! off its origin's default branch and opens the pull request itself
//! (`tasks.rs`); the reply and a `pr` artifact carry its link.
//!
//! An agent here uses it through MCP's `delegate` tool: `send` (and wait),
//! `get` (and wait), `answer` (a question the task asked) and `cancel`.
//! Each ended task's result (its reply, and its patch) is kept in
//! `<state>/a2a/results/TASK.json`, since the other machine cleans up once
//! it's been read.
//!
//! **Its work comes back (M80, #402)** as that patch, against the commit
//! the task started from. `review` applies it to a scratch worktree of the
//! caller's repository at its `HEAD` (`~/.cache/arugula/review/TASK`: not
//! the state dir, which no block shows), for a
//! diff block to show before anything changes; `apply` applies it to the
//! checkout itself with `git apply --3way`, and, if that can't take all of
//! it, with `--reject`, saying which hunks didn't apply. Either way the
//! new binary files the task left out of the patch are named: nothing is
//! dropped silently.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use serde_json::{Value, json};

use super::tasks::{CANCELED, COMPLETED, FAILED, INPUT_REQUIRED, REJECTED};
use crate::{server::App, store::now_ms};

const HEADERS: [(&str, &str); 1] = [("A2A-Version", super::A2A_VERSION)];

/// Whether `machine` names this one: its name, its id in control, or
/// `here`.
pub fn is_here(app: &App, machine: &str) -> bool {
    let id = app.control.enrolled().map(|e| e.saved.cert.device.clone());
    machine == "here" || machine == app.hosts.name() || Some(machine) == id.as_deref()
}

/// The machine that offers `agent`: `machine` if it's given, else this one
/// if it offers it, else the one machine in the team's catalog that does.
pub async fn locate(app: &Arc<App>, machine: Option<&str>, agent: &str) -> Result<String, String> {
    if let Some(m) = machine.map(str::trim).filter(|m| !m.is_empty()) {
        return Ok(m.to_owned());
    }
    if super::offers(app.control.state_dir()).iter().any(|o| o.agent == agent) {
        return Ok(app.hosts.name().to_owned());
    }
    let c = super::catalog::get(app, super::catalog::FRESH_MS).await;
    let mut on: Vec<&super::catalog::Shelf> =
        c.machines.iter().filter(|s| !s.here && s.agents.iter().any(|a| a["name"] == agent)).collect();
    // Two machines with one name: the ids tell them apart.
    let named = |s: &super::catalog::Shelf| {
        if c.machines.iter().filter(|o| o.name == s.name).count() > 1 { s.machine.clone() } else { s.name.clone() }
    };
    match on.len() {
        0 => Err(format!("no machine you reach offers {agent} (list kind agents shows what's offered)")),
        1 => Ok(named(on.remove(0))),
        _ => Err(format!(
            "{agent} is offered on {}: say which machine",
            on.iter().map(|s| named(s)).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// One JSON-RPC call to `agent` on `machine`: its result, or its error.
async fn call(app: &Arc<App>, machine: &str, agent: &str, method: &str, params: Value) -> Result<Value, String> {
    let body = json!({ "jsonrpc": "2.0", "id": now_ms(), "method": method, "params": params });
    if is_here(app, machine) {
        return local(app, agent, body).await;
    }
    let m = crate::peer::find(&app.control, machine).await.map_err(|e| e.to_string())?;
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

/// A call on an agent this machine offers, answered here as this machine's
/// own account (the route's handler, without the channel).
async fn local(app: &Arc<App>, agent: &str, req: Value) -> Result<Value, String> {
    let Some(o) = super::offers(app.control.state_dir()).into_iter().find(|o| o.agent == agent) else {
        return Err(format!("{agent} isn't offered on this machine (arugula agents offer {agent})"));
    };
    super::find(std::path::Path::new(&o.dir), &crate::home(), agent)
        .map_err(|_| format!("{agent} is offered here but its recipe is gone"))?;
    let claims = req["params"]["message"]["metadata"]["arugula"].clone();
    let caller = super::tasks::asker(app, None, claims);
    let v = super::tasks::rpc(app.clone(), agent, &o.dir, caller, req).await;
    if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
        return Err(format!("{agent}: {}", e["message"].as_str().unwrap_or("an error")));
    }
    Ok(v["result"].clone())
}

/// Send `text` to `agent` on `machine`: a new task, or (with `task`) the
/// next message on one (an answer to its question, or a follow-up). A new
/// one with `pr` is delivered as a pull request.
pub async fn send(
    app: &Arc<App>,
    machine: &str,
    agent: &str,
    text: &str,
    task: Option<&str>,
    mut claims: Value,
    pr: bool,
) -> Result<Value, String> {
    if pr && task.is_none() {
        if !claims.is_object() {
            claims = json!({});
        }
        claims["deliver"] = json!("pr");
    }
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

/// A kept result: `(machine, agent, task)`.
pub fn kept(state_dir: &Path, task: &str) -> Result<(String, String, Value), String> {
    let file =
        state_dir.join("a2a").join("results").join(format!("{}.json", super::super::fountain::wear::component(task)));
    let b =
        std::fs::read(&file).map_err(|_| format!("no result kept here for task {task} (get it once it has ended)"))?;
    let v: Value = serde_json::from_slice(&b).map_err(|e| format!("its result: {e}"))?;
    Ok((
        v["machine"].as_str().unwrap_or_default().to_owned(),
        v["agent"].as_str().unwrap_or_default().to_owned(),
        v["task"].clone(),
    ))
}

/// The task's patch: its text, its base, and the files left out of it.
pub struct Patch {
    pub text: String,
    pub base: String,
    pub left_out: Vec<String>,
}

pub fn patch_of(t: &Value) -> Option<Patch> {
    let a = t["artifacts"].as_array()?.iter().find(|a| a["name"] == "patch")?;
    let m = &a["metadata"]["arugula"];
    Some(Patch {
        text: a["parts"][0]["text"].as_str()?.to_owned(),
        base: m["base"].as_str().unwrap_or_default().to_owned(),
        left_out: m["left_out"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect(),
    })
}

/// What applying a patch did.
#[derive(Debug, Default, serde::Serialize)]
pub struct Applied {
    /// Where.
    pub dir: String,
    /// The files it changed.
    pub files: Vec<String>,
    /// All of it applied (three-way where it had to).
    pub clean: bool,
    /// Files merged with conflict markers to resolve (three-way).
    pub conflicts: Vec<String>,
    /// Hunks that didn't apply, each in a `.rej` beside its file.
    pub rejected: Vec<String>,
    /// What git said when it didn't apply cleanly.
    pub said: Option<String>,
    /// New binary files the task made that the patch leaves out.
    pub left_out: Vec<String>,
    /// The commit the task started from isn't in this checkout (another
    /// clone, or not fetched): three-way merging had less to go on.
    pub base_missing: Option<String>,
}

impl Applied {
    pub fn text(&self, task: &str) -> String {
        let mut out = if self.clean {
            format!("Task {task}'s patch applied in {}: {}.\n", self.dir, self.files.join(", "))
        } else {
            format!("Task {task}'s patch applied in {} only in part: {}.\n", self.dir, self.files.join(", "))
        };
        if !self.conflicts.is_empty() {
            out.push_str(&format!("Merged with conflicts to resolve: {}\n", self.conflicts.join(", ")));
        }
        if !self.rejected.is_empty() {
            out.push_str(&format!("Hunks that didn't apply (see the .rej files): {}\n", self.rejected.join(", ")));
        }
        if let Some(s) = &self.said {
            out.push_str(&format!("git said: {s}\n"));
        }
        if let Some(b) = &self.base_missing {
            out.push_str(&format!(
                "This checkout doesn't have {b}, the commit the task started from: fetch it for a better merge.\n"
            ));
        }
        if !self.left_out.is_empty() {
            out.push_str(&format!(
                "Not in the patch (new binary files on the other machine): {}\n",
                self.left_out.join(", ")
            ));
        }
        out
    }
}

/// The files a patch touches.
fn files_of(patch: &str) -> Vec<String> {
    let mut f: Vec<String> = patch
        .lines()
        .filter_map(|l| l.strip_prefix("+++ b/").or_else(|| l.strip_prefix("--- a/")))
        .map(str::to_owned)
        .collect();
    f.sort();
    f.dedup();
    f
}

/// Apply `p` in `dir` (a git checkout): three-way, else what applies with
/// the rest rejected.
async fn apply_in(dir: &Path, p: &Patch, state_dir: &Path, task: &str) -> Result<Applied, String> {
    use super::tasks::git;
    let top = git(dir, &["rev-parse", "--show-toplevel"])
        .await
        .map_err(|_| format!("{} isn't in a git checkout", dir.display()))?;
    let top = PathBuf::from(top.trim());
    let file =
        state_dir.join("a2a").join("results").join(format!("{}.patch", super::super::fountain::wear::component(task)));
    std::fs::write(&file, &p.text).map_err(|e| format!("can't write the patch: {e}"))?;
    let f = file.display().to_string();
    let mut out = Applied {
        dir: top.display().to_string(),
        files: files_of(&p.text),
        left_out: p.left_out.clone(),
        ..Default::default()
    };
    if !p.base.is_empty() && git(&top, &["cat-file", "-e", &format!("{}^{{commit}}", p.base)]).await.is_err() {
        out.base_missing = Some(p.base.clone());
    }
    match git(&top, &["apply", "--3way", "--whitespace=nowarn", &f]).await {
        Ok(_) => out.clean = true,
        Err(e) => {
            out.said = Some(e.lines().take(6).collect::<Vec<_>>().join(" / "));
            // Merged, with conflict markers in these (`U path`): they're
            // for someone to resolve, and nothing else is tried over them.
            out.conflicts = e.lines().filter_map(|l| l.strip_prefix("U ")).map(str::to_owned).collect();
            if out.conflicts.is_empty() {
                // Nothing went in: what applies does, the rest as .rej files.
                let _ = git(&top, &["apply", "--reject", "--whitespace=nowarn", &f]).await;
                out.rejected =
                    files_of(&p.text).into_iter().filter(|x| top.join(format!("{x}.rej")).exists()).collect();
            }
        }
    }
    Ok(out)
}

/// Where review worktrees go: the user's cache.
pub fn review_root() -> PathBuf {
    super::super::fountain::wear::cache_root(&[], &crate::home()).with_file_name("review")
}

/// Apply task `task`'s kept patch in the checkout at `dir` (and drop its
/// review worktree, under `scratch`, if there is one).
pub async fn apply(state_dir: &Path, scratch: &Path, task: &str, dir: &Path) -> Result<Applied, String> {
    let (_, _, t) = kept(state_dir, task)?;
    let p = patch_of(&t).ok_or_else(|| format!("task {task} changed nothing: no patch to apply"))?;
    let out = apply_in(dir, &p, state_dir, task).await?;
    // Its review worktree, if any, has done its job.
    let review = review_dir(scratch, task);
    if review.exists() {
        let _ = super::tasks::git(dir, &["worktree", "remove", "--force", &review.display().to_string()]).await;
    }
    Ok(out)
}

fn review_dir(scratch: &Path, task: &str) -> PathBuf {
    scratch.join(super::super::fountain::wear::component(task))
}

/// Task `task`'s patch in a scratch worktree of `dir`'s repository at its
/// `HEAD`, for a diff block to show: where, and how it applied.
pub async fn review(state_dir: &Path, scratch: &Path, task: &str, dir: &Path) -> Result<(PathBuf, Applied), String> {
    use super::tasks::git;
    let (_, _, t) = kept(state_dir, task)?;
    let p = patch_of(&t).ok_or_else(|| format!("task {task} changed nothing: no patch to review"))?;
    let wt = review_dir(scratch, task);
    if wt.exists() {
        let _ = git(dir, &["worktree", "remove", "--force", &wt.display().to_string()]).await;
    }
    let _ = std::fs::create_dir_all(scratch);
    git(dir, &["worktree", "add", "-q", "--detach", &wt.display().to_string(), "HEAD"])
        .await
        .map_err(|e| format!("{} isn't in a git checkout ({e})", dir.display()))?;
    let applied = apply_in(&wt, &p, state_dir, task).await?;
    Ok((wt, applied))
}

/// Run `agent` on `machine` for a person to talk to: an agent block wearing
/// the recipe in the project it's offered from, there (the own account's
/// machines only; `POST /api/a2a/agents/NAME/run` there). Answers the
/// machine's name and id (none: this one) and the block.
pub async fn run_there(app: &Arc<App>, machine: &str, agent: &str, prompt: Option<&str>) -> Result<Value, String> {
    let body = json!({ "prompt": prompt });
    if is_here(app, machine) {
        let block = super::run_offered(app, agent, prompt).await?;
        return Ok(json!({ "machine": app.hosts.name(), "id": null, "block": block }));
    }
    let m = crate::peer::find(&app.control, machine).await.map_err(|e| e.to_string())?;
    let path = format!("/api/a2a/agents/{agent}/run");
    let (status, bytes) = crate::peer::request(&app.control, &m, "POST", &path, Some(&body), &HEADERS)
        .await
        .map_err(|e| format!("{machine}: {e:#}"))?;
    let v: Value = serde_json::from_slice(&bytes).unwrap_or_default();
    if status != 200 {
        let why =
            v["error"].as_str().map(str::to_owned).unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned());
        return Err(format!("{} said {status}: {why}", m.name));
    }
    Ok(json!({ "machine": m.name, "id": m.id, "block": v["block"] }))
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
            Some("pr") => {
                let r = a["parts"][0]["text"].as_str().unwrap_or_default();
                out.push_str(&format!("Its pull request: {r}\n"));
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
    let has = |name: &str| t["artifacts"].as_array().into_iter().flatten().any(|a| a["name"] == name);
    if state == "completed" && has("patch") && !has("pr") {
        out.push_str(
            "delegate {kind: review} shows the patch in a diff block on your checkout; {kind: apply} applies it.\n",
        );
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

    #[tokio::test]
    async fn a_patch_reviews_in_a_worktree_then_applies_saying_what_it_left_out() {
        use super::super::tasks::git;
        let root = std::env::temp_dir().join(format!("arugula-apply-{}-{}", std::process::id(), now_ms()));
        let (repo, state) = (root.join("repo"), root.join("state"));
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(state.join("a2a/results")).unwrap();
        let g = |args: &'static [&'static str]| {
            let repo = repo.clone();
            async move { git(&repo, &[&["-c", "user.name=t", "-c", "user.email=t@e"][..], args].concat()).await.unwrap() }
        };
        std::fs::write(repo.join("calc.py"), "a = 1\nx\ny\nz\nb = 2\n").unwrap();
        g(&["init", "-q"]).await;
        g(&["add", "-A"]).await;
        g(&["commit", "-q", "-m", "c"]).await;
        let base = git(&repo, &["rev-parse", "HEAD"]).await.unwrap().trim().to_owned();
        // The patch as a task makes it (tasks.rs): `git diff --binary` of
        // the work against its base, new files included.
        std::fs::write(repo.join("calc.py"), "a = 1\nx\ny\nz\nb = 3\n").unwrap();
        std::fs::write(repo.join("notes.txt"), "hi\n").unwrap();
        g(&["add", "-N", "notes.txt"]).await;
        let patch = git(&repo, &["diff", "--binary", &base]).await.unwrap();
        g(&["reset", "-q", "--hard"]).await;
        let _ = std::fs::remove_file(repo.join("notes.txt"));
        let task = json!({ "id": "t9", "status": { "state": "TASK_STATE_COMPLETED" }, "artifacts": [
            { "name": "patch", "parts": [{ "text": patch }], "metadata": { "arugula": { "base": base, "left_out": ["x.pyc"] } } },
        ] });
        std::fs::write(
            state.join("a2a/results/t9.json"),
            json!({ "machine": "m", "agent": "a", "task": task }).to_string(),
        )
        .unwrap();

        // A file's text, its line endings as written (a Windows checkout's
        // are CRLF).
        let read = |p: PathBuf| std::fs::read_to_string(p).unwrap().replace("\r\n", "\n");
        // Review: the checkout is untouched; the worktree has it.
        let scratch = root.join("review");
        let (wt, r) = review(&state, &scratch, "t9", &repo).await.unwrap();
        assert!(r.clean, "{r:?}");
        assert_eq!(read(wt.join("calc.py")), "a = 1\nx\ny\nz\nb = 3\n");
        assert_eq!(read(repo.join("calc.py")), "a = 1\nx\ny\nz\nb = 2\n");
        // Apply, after a change of ours elsewhere in the file: three-way.
        std::fs::write(repo.join("calc.py"), "a = 10\nx\ny\nz\nb = 2\n").unwrap();
        g(&["commit", "-q", "-am", "ours"]).await;
        let a = apply(&state, &scratch, "t9", &repo).await.unwrap();
        assert!(a.clean, "{a:?}");
        assert_eq!(a.files, ["calc.py", "notes.txt"]);
        assert_eq!(read(repo.join("calc.py")), "a = 10\nx\ny\nz\nb = 3\n");
        assert!(repo.join("notes.txt").is_file());
        assert!(a.text("t9").contains("x.pyc"), "the left-out file is named");
        assert!(!wt.exists(), "the review worktree went");
        // On a line we changed too: merged with conflicts, said so.
        g(&["checkout", "-q", "--", "."]).await;
        let _ = std::fs::remove_file(repo.join("notes.txt"));
        std::fs::write(repo.join("calc.py"), "a = 10\nx\ny\nz\nb = 4\n").unwrap();
        g(&["commit", "-q", "-am", "ours again"]).await;
        let again = apply(&state, &scratch, "t9", &repo).await.unwrap();
        assert!(!again.clean, "{again:?}");
        assert_eq!(again.conflicts, ["calc.py"], "{again:?}");
        assert!(again.text("t9").contains("conflicts to resolve: calc.py"));
        let _ = std::fs::remove_dir_all(&root);
    }

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
