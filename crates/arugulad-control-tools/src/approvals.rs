//! `ask_approval` for an agent on a machine (Arugula #1417): the chief of
//! staff, an agent block on one of its person's machines, asks them to
//! approve what only they may do: start an agent on one of the project's
//! issues, or close agent blocks, here. It's the hosted session's card
//! (Arugula #1278): control posts it in their staff room (`via`
//! `ask:prp_…`), under Needs you and on their phone, checked as this
//! agent's posts there are (`threads.post`). **Approve** sends this machine
//! `agents.approved` (`arugulad_base::approvals`), which does it as them;
//! `ask.answered` tells the agent, by its watch.
//!
//! The agent does nothing itself. A start on another of the person's
//! machines is a ladder start's (aru-143, #1415), not a card's yet.

use arugula_control_wire::sessions::{
    MachinePropose, MachineProposed, PROPOSAL_CLOSE_MAX, PROPOSAL_PROMPT_MAX, PROPOSAL_WHY_MAX, Proposal,
    ProposalSession, agent_proposals,
};
use arugula_proto::{BlockType, PaneId};
use reqwest::Method;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::{
    control::CloudRefused,
    mcp::tools::{Call, Out, PaneArg, done},
    mux::Api,
};

pub(crate) const TOOL: &str = "ask_approval";

pub(crate) const DESCRIPTION: &str = "Ask your person to approve something only they may do, on this machine: start an agent on one of the project's issues (action start: issue #N, prompt), or close agent blocks here (action close: blocks, by pane). Say why. It puts a card with Approve and Deny in their staff room, under Needs you and on their phone; Approve does it as them, here, and Deny drops it. You never do it yourself, and nothing said in chat approves it. Call it from your agent block. Returns the card's id (prp_…): watch event ask.answered with match {\"ask\": id}, and end your turn.";

#[derive(Deserialize, JsonSchema)]
pub struct AskApprovalArgs {
    /// start (an agent on an issue, on this machine) or close (agent
    /// blocks on this machine).
    pub action: String,
    /// start: the issue, as #N or N, in the project.
    #[serde(default)]
    pub issue: Option<String>,
    /// start: what the new agent is told, at most 4000 characters.
    /// Without it: to work on the issue.
    #[serde(default)]
    pub prompt: Option<String>,
    /// close: the agent blocks to close, by pane (7 or "%7"), at most 10.
    #[serde(default)]
    pub blocks: Vec<PaneArg>,
    /// Why, in a line or two (at most 500 characters): the card shows it.
    pub why: String,
    /// The project (prj_…). Default: the current project, whose staff room
    /// the card goes in.
    #[serde(default)]
    pub project: Option<String>,
}

/// What it proposes, checked here as control checks it again.
fn proposal(a: &AskApprovalArgs, blocks: Vec<PaneId>) -> Result<Proposal, String> {
    let why = a.why.trim().to_owned();
    if why.is_empty() {
        return Err("say why: the card shows it".into());
    }
    if why.chars().count() > PROPOSAL_WHY_MAX {
        return Err(format!("why: at most {PROPOSAL_WHY_MAX} characters"));
    }
    match a.action.trim() {
        "start" => {
            if !blocks.is_empty() {
                return Err("blocks go with close".into());
            }
            let issue = a.issue.as_deref().ok_or("start needs issue: #N")?;
            let number =
                issue.trim().trim_start_matches('#').parse().map_err(|_| format!("an issue is #N, not {issue:?}"))?;
            let prompt = a.prompt.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned);
            if prompt.as_ref().is_some_and(|p| p.chars().count() > PROPOSAL_PROMPT_MAX) {
                return Err(format!("prompt: at most {PROPOSAL_PROMPT_MAX} characters"));
            }
            Ok(Proposal::Start { number, prompt, repo: None, why })
        }
        "close" => {
            if a.issue.is_some() || a.prompt.is_some() {
                return Err("issue and prompt go with start".into());
            }
            match blocks.len() {
                0 => Err("close needs blocks: the agent blocks to close, by pane".into()),
                n if n > PROPOSAL_CLOSE_MAX => Err(format!("at most {PROPOSAL_CLOSE_MAX} blocks on one card")),
                _ => Ok(Proposal::Close { sessions: vec![], blocks, why }),
            }
        }
        a => Err(format!("action {a}: start or close")),
    }
}

/// Each of `blocks` as the card shows it, once it's checked to be an agent
/// block here that isn't the caller's own.
async fn cards_of(c: &Call<'_>, me: PaneId, blocks: &[PaneId]) -> Result<Vec<ProposalSession>, String> {
    let panes = c.app().mux.api(Api::Panes).await.unwrap_or_default();
    let mut out = vec![];
    for &id in blocks {
        if id == me {
            return Err(format!("%{id} is your own block: your person closes it themselves"));
        }
        let p = panes.iter().find(|p| p.info.id == id).ok_or_else(|| format!("no block %{id} on this machine"))?;
        if p.info.kind != BlockType::Agent || p.info.host.is_some() {
            return Err(format!("%{id} isn't an agent block on this machine"));
        }
        let state = match c.app().mux.api(|r| Api::Block(id, r)).await.flatten() {
            Some(b) => b.state(),
            None => serde_json::Value::Null,
        };
        let title = match arugulad_base::agent_mentions::session_name(c.app(), id).await {
            Some(name) => name,
            None => p.tab_name.clone().unwrap_or_else(|| p.session_name.clone()),
        };
        out.push(ProposalSession {
            id: format!("%{id}"),
            title,
            state: state["state"].as_str().unwrap_or_default().to_owned(),
        });
    }
    Ok(out)
}

pub(crate) async fn ask_approval(c: &Call<'_>, a: AskApprovalArgs) -> Out {
    let me = c.own_pane().ok_or("ask_approval is an agent block's: call it from your own block")?;
    let mut blocks = vec![];
    for b in &a.blocks {
        let id = b.id()?;
        if !blocks.contains(&id) {
            blocks.push(id);
        }
    }
    let proposal = proposal(&a, blocks.clone())?;
    let session = arugulad_base::agent_mentions::session_name(c.app(), me)
        .await
        .ok_or("your block has no session name for the card: give its tab a name")?;
    let cards = cards_of(c, me, &blocks).await?;
    let project = c.current_project(a.project.as_deref()).await?;
    let body = MachinePropose { proposal, session, pane: me, blocks: cards };
    let body = serde_json::to_value(&body).map_err(|e| e.to_string())?;
    let v = match c.app().control.cloud(Method::POST, &agent_proposals(&project), Some(&body)).await {
        Ok(v) => v,
        Err(CloudRefused::NotJoined) => {
            return Err(
                "this machine hasn't joined Arugula cloud, where the card goes: ask your person to join it".into()
            );
        }
        Err(CloudRefused::OldControl | CloudRefused::Said { status: 404 | 405, .. }) => {
            return Err(
                "Arugula cloud doesn't take a machine's approval cards yet: tell your person where to do it themselves"
                    .into(),
            );
        }
        Err(e) => return Err(e.to_string()),
    };
    let p: MachineProposed = serde_json::from_value(v).map_err(|e| format!("Arugula cloud answered oddly: {e}"))?;
    done(
        format!(
            "Asked for approval: card {} (message {} in room {}). Watch event ask.answered with match {{\"ask\": \"{}\"}}, then end your turn.",
            p.ask, p.seq, p.room, p.ask
        ),
        json!({ "ask": p.ask, "room": p.room, "seq": p.seq, "project": project }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: serde_json::Value) -> AskApprovalArgs {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_machines_card_proposes_one_thing_with_a_why() {
        let a = args(json!({ "action": "start", "issue": "#12", "why": "it's next" }));
        assert_eq!(
            proposal(&a, vec![]).unwrap(),
            Proposal::Start { number: 12, prompt: None, repo: None, why: "it's next".into() }
        );
        let a = args(json!({ "action": "close", "blocks": [4, "%9"], "why": "done" }));
        let blocks: Vec<PaneId> = a.blocks.iter().map(|b| b.id().unwrap()).collect();
        assert_eq!(
            proposal(&a, blocks).unwrap(),
            Proposal::Close { sessions: vec![], blocks: vec![4, 9], why: "done".into() }
        );
        let refused = |v| proposal(&args(v), vec![]).unwrap_err();
        assert!(refused(json!({ "action": "start", "issue": "#12", "why": " " })).contains("say why"));
        assert!(refused(json!({ "action": "start", "why": "x" })).contains("needs issue"));
        assert!(refused(json!({ "action": "close", "why": "x" })).contains("needs blocks"));
        assert!(refused(json!({ "action": "close", "issue": "#1", "why": "x" })).contains("go with start"));
        assert!(refused(json!({ "action": "run_board", "why": "x" })).contains("start or close"));
        assert!(proposal(&args(json!({ "action": "start", "issue": "#1", "why": "x" })), vec![3]).is_err());
    }
}
