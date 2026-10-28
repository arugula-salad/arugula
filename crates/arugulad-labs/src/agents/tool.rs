//! MCP's delegating (M78) and the team's agent catalog (M77). The tools'
//! rows are the base's (`mcp/tools.rs`); they call these through
//! `lab_hooks`.

use std::time::Duration;

use arugula_proto::{BlockType, api::OpenRequest, flags};
use serde_json::{Value, json};

use crate::{
    mcp::tools::{Call, DelegateArgs, ListTeamAgentsArgs, Out, done},
    mux::Api,
};

/// Delegating (M78): a task for another machine's agent.
pub async fn delegate(call: &Call<'_>, kind: &str, a: DelegateArgs) -> Out {
    use super::delegate as d;
    if !crate::lab_flags::on(call.app.control.state_dir(), flags::AGENT_DELEGATION) {
        return Err("delegating is in Labs: `arugulad flags agent-delegation on` turns it on here".into());
    }
    // The owner's agents': a guest's agent asks no one's agents.
    if let Some(me) = call.me()
        && call.app.mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some()
    {
        return Err("only the owner's own agents delegate to other machines".into());
    }
    let app = call.app.clone();
    // M80: the work comes back, from what's kept here.
    if kind == "review" || kind == "apply" {
        let task = a.task.as_deref().filter(|t| !t.is_empty()).ok_or_else(|| format!("{kind} needs task"))?;
        let dir = match a.dir.as_deref().filter(|d| !d.is_empty()) {
            Some(d) => d.to_owned(),
            None => match call.me() {
                Some(me) => call.readable(me).await?.info.cwd.ok_or("say where your checkout is: dir")?,
                None => return Err("say where your checkout is: dir".into()),
            },
        };
        let state = app.control.state_dir().to_owned();
        if kind == "apply" {
            let r = d::apply(&state, &d::review_root(), task, std::path::Path::new(&dir)).await?;
            return done(r.text(task), json!({ "task": task, "applied": r }));
        }
        let (wt, r) = d::review(&state, &d::review_root(), task, std::path::Path::new(&dir)).await?;
        let beside = call.own_pane();
        let req = OpenRequest {
            kind: BlockType::Diff,
            config: json!({ "repo": wt }),
            split: beside,
            from_pane: beside,
            local: true,
            ..Default::default()
        };
        let block = call.open(req).await?;
        let text = format!(
            "Task {task}'s patch is in diff block %{block}, on a copy of {dir}; nothing here changed yet. {}",
            r.text(task)
        );
        return done(text, json!({ "task": task, "block": block, "worktree": wt, "applied": r }));
    }
    let machine = d::locate(&app, a.machine.as_deref(), &a.agent).await?;
    let (machine, agent) = (machine.as_str(), a.agent.as_str());
    let limit = Duration::from_secs(a.wait.unwrap_or(300).min(1800));
    let task_of = || a.task.as_deref().filter(|t| !t.is_empty()).ok_or_else(|| format!("{kind} needs task"));
    let text_of = || a.text.as_deref().filter(|t| !t.trim().is_empty()).ok_or_else(|| format!("{kind} needs text"));
    let claims = json!({ "machine": app.hosts.name(), "pane": call.me() });
    let t = match kind {
        "send" => d::send(&app, machine, agent, text_of()?, None, claims, a.pr).await?,
        "answer" => d::send(&app, machine, agent, text_of()?, Some(task_of()?), claims, false).await?,
        "cancel" => d::cancel(&app, machine, agent, task_of()?).await?,
        _ => d::get(&app, machine, agent, task_of()?).await?,
    };
    let id = t["id"].as_str().unwrap_or_default().to_owned();
    let t = if kind != "cancel" && !limit.is_zero() && !d::settled(&t) && !id.is_empty() {
        let what = format!("{agent} on {machine}");
        match call.waiting(&what, limit, d::wait(&app, machine, agent, &id, limit)).await {
            Some(r) => r?,
            None => d::get(&app, machine, agent, &id).await?,
        }
    } else {
        t
    };
    done(d::summary(machine, agent, &t), json!({ "machine": machine, "agent": agent, "task": t }))
}

/// The team's agent catalog (M77): the cards each machine offers.
pub async fn team_agents(call: &Call<'_>, a: ListTeamAgentsArgs) -> Out {
    use super::catalog;
    if !crate::lab_flags::on(call.app.control.state_dir(), flags::AGENT_DELEGATION) {
        return Err("the agent catalog is in Labs: `arugulad flags agent-delegation on` turns it on here".into());
    }
    // The owner's agents': a guest's agent sees no catalog (#234's rule).
    if let Some(me) = call.me()
        && call.app.mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some()
    {
        return Err("only the owner's own agents read the team's agent catalog".into());
    }
    let c = catalog::get(call.app, if a.fresh { 0 } else { catalog::FRESH_MS }).await;
    let q = a.query.as_deref().map(str::to_lowercase).filter(|q| !q.trim().is_empty());
    let hit = |s: &catalog::Shelf, card: &Value| {
        q.as_ref().is_none_or(|q| {
            let hay = format!("{} {} {} {}", card["name"], card["description"], s.name, s.owner).to_lowercase();
            q.split_whitespace().all(|w| hay.contains(w))
        })
    };
    let rows: Vec<(&catalog::Shelf, &Value)> = c.agents().filter(|(s, card)| hit(s, card)).collect();
    let total = c.agents().count();
    let mut text = match rows.len() {
        0 if total == 0 => "No agents offered on any machine the user's team can reach.\n".to_owned(),
        n => format!("{n} of {total} agents on {} machines:\n", c.machines.len()),
    };
    for (s, card) in &rows {
        text.push_str(&catalog::Catalog::line(s, card));
        text.push('\n');
    }
    for s in c.machines.iter().filter(|s| s.note.is_some()) {
        text.push_str(&format!("{}: {}\n", s.name, s.note.as_deref().unwrap_or_default()));
    }
    if let Some(n) = &c.note {
        text.push_str(n);
        text.push('\n');
    }
    done(text, &c)
}
