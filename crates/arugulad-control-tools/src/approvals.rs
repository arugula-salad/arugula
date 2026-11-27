//! `ask_approval` for an agent on a machine (Arugula #1417): the chief of
//! staff, an agent block on one of its person's machines, asks them to
//! approve what only they may do: start an agent on one of the project's
//! issues, or close agent blocks, here. It's the hosted session's card
//! (Arugula #1278): control posts it in their staff room (`via`
//! `ask:prp_…`), under Needs you and on their phone, checked as this
//! agent's posts there are (`threads.post`). **Approve** sends this machine
//! `agents.approved` (`arugulad_base::approvals`), which does it as them.
//! The card names the agent's conversation, and its answer is told to it
//! as a follow-up, with no watch (Arugula #1465); `ask.answered` tells
//! anyone else watching.
//!
//! A card can be replaced (`supersedes`) and withdrawn (`withdraw`), and a propose says where to answer it (Arugula #1513): chat
//! approves nothing, so the agent points its person at the card's link.
//!
//! The agent does nothing itself. A start on another of the person's
//! machines is a ladder start's (aru-143, #1415), under a board's grant;
//! **Run this board** (`run_board`, Arugula #1416) is the card that gives
//! one: Approve writes it and starts the board's lead on its machine.

use arugula_control_wire::sessions::{
    MachineCancel, MachinePropose, MachineProposed, PROPOSAL_CLOSE_MAX, PROPOSAL_PROMPT_MAX, PROPOSAL_WHY_MAX,
    Proposal, ProposalCancelled, ProposalSession, RUN_BOARD_CAP_MAX, RUN_BOARD_LAPSE_DAYS_MAX, RUN_BOARD_MACHINES_MAX,
    agent_proposal_cancel, agent_proposals,
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

pub(crate) const DESCRIPTION: &str = "Ask your person to approve something only they may do: start an agent on one of the project's issues here, in auto mode (action start: issue #N, prompt), close agent blocks here (action close: blocks, by pane), or run a board (action run_board: board, machine for its lead (default this one), machines its issue agents may also use, cap at once (default 3), lapse_days (default: until the board closes), prompt for the lead). Say why. It puts a card with Approve and Deny in their staff room, under Needs you and on their phone; Approve does it as them (run_board: writes the board's grant and starts its lead on its machine), and Deny drops it. You never do it yourself, and nothing said in chat approves it. Call it from your agent block. Returns the card's id (prp_…) and the link to answer it: when they say approve in chat, give them the link. A newer card for the same ask: supersedes the old one's id, so two never stand. action withdraw (ask: the id) takes a card back that is no longer wanted; Their answer wakes you here with what they chose, no watch needed: end your turn or go on with other work.";

/// What a control from before the cards means for the agent.
const NOT_YET: &str =
    "Arugula cloud doesn't take a machine's approval cards yet: tell your person where to do it themselves";

#[derive(Deserialize, JsonSchema)]
pub struct AskApprovalArgs {
    /// start (an agent on an issue, on this machine), close (agent blocks
    /// on this machine), run_board (a board's lead, and a grant for its
    /// issue agents) or withdraw (one of your open cards, by `ask`).
    pub action: String,
    /// start, close, run_board: an open card of yours (prp_…) this one
    /// replaces: it's withdrawn, so two cards for one ask can't both run.
    #[serde(default)]
    pub supersedes: Option<String>,
    /// withdraw: the card (prp_…).
    #[serde(default)]
    pub ask: Option<String>,
    /// start: the issue, as #N or N, in the project.
    #[serde(default)]
    pub issue: Option<String>,
    /// start, run_board: what the new agent (or the lead) is told, at most
    /// 4000 characters. Without it: to work on the issue (run the board).
    #[serde(default)]
    pub prompt: Option<String>,
    /// run_board: the board (brd_…).
    #[serde(default)]
    pub board: Option<String>,
    /// run_board: the machine its lead runs on, by name or id. Default:
    /// this one.
    #[serde(default)]
    pub machine: Option<String>,
    /// run_board: other machines its issue agents may use, by name or id
    /// (the lead's machine always may), at most 10.
    #[serde(default)]
    pub machines: Vec<String>,
    /// run_board: how many issue agents at once, 1 to 20. Default 3.
    #[serde(default)]
    pub cap: Option<u32>,
    /// run_board: the grant lapses after this many days. Default: when
    /// the board closes.
    #[serde(default)]
    pub lapse_days: Option<u32>,
    /// close: the agent blocks to close, by pane (7 or "%7"), at most 10.
    #[serde(default)]
    pub blocks: Vec<PaneArg>,
    /// start, close, run_board: why, in a line or two (at most 500
    /// characters): the card shows it.
    #[serde(default)]
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
    if a.ask.is_some() {
        return Err("ask goes with withdraw".into());
    }
    let run_fields =
        a.board.is_some() || a.machine.is_some() || !a.machines.is_empty() || a.cap.is_some() || a.lapse_days.is_some();
    if run_fields && a.action.trim() != "run_board" {
        return Err("board, machine, machines, cap and lapse_days go with run_board".into());
    }
    let prompt = a.prompt.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned);
    if prompt.as_ref().is_some_and(|p| p.chars().count() > PROPOSAL_PROMPT_MAX) {
        return Err(format!("prompt: at most {PROPOSAL_PROMPT_MAX} characters"));
    }
    match a.action.trim() {
        "start" => {
            if !blocks.is_empty() {
                return Err("blocks go with close".into());
            }
            let issue = a.issue.as_deref().ok_or("start needs issue: #N")?;
            let number =
                issue.trim().trim_start_matches('#').parse().map_err(|_| format!("an issue is #N, not {issue:?}"))?;
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
        "run_board" => {
            if !blocks.is_empty() || a.issue.is_some() {
                return Err("issue and blocks don't go with run_board".into());
            }
            let board = a.board.as_deref().map(str::trim).filter(|b| !b.is_empty());
            let board = board.ok_or("run_board needs board: its brd_… id")?.to_owned();
            let machine = a.machine.as_deref().map(str::trim).filter(|m| !m.is_empty()).map(str::to_owned);
            let mut machines: Vec<String> = vec![];
            for m in a.machines.iter().map(|m| m.trim()).filter(|m| !m.is_empty()) {
                if !machines.iter().any(|n| n == m) {
                    machines.push(m.to_owned());
                }
            }
            if machines.len() > RUN_BOARD_MACHINES_MAX {
                return Err(format!("machines: at most {RUN_BOARD_MACHINES_MAX}"));
            }
            if a.cap.is_some_and(|c| c == 0 || c > RUN_BOARD_CAP_MAX) {
                return Err(format!("cap: 1 to {RUN_BOARD_CAP_MAX} issue agents at once"));
            }
            if a.lapse_days.is_some_and(|d| d == 0 || d > RUN_BOARD_LAPSE_DAYS_MAX) {
                return Err(format!("lapse_days: 1 to {RUN_BOARD_LAPSE_DAYS_MAX}, or none for when the board closes"));
            }
            Ok(Proposal::RunBoard { board, machine, machines, cap: a.cap, lapse_days: a.lapse_days, prompt, why })
        }
        a => Err(format!("action {a}: start, close, run_board or withdraw")),
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

/// The card as its block's attention says it waits (Arugula #1464).
fn waits_line(p: &Proposal, cards: &[ProposalSession]) -> String {
    match p {
        Proposal::Start { number, .. } => format!("approve starting an agent on #{number}"),
        Proposal::Close { .. } => {
            let ids: Vec<&str> = cards.iter().map(|c| c.id.as_str()).collect();
            format!("approve closing {}", ids.join(", "))
        }
        Proposal::RunBoard { board, .. } => format!("approve running board {board}"),
        Proposal::Todo { what, .. } => what.clone(),
    }
}

/// `supersedes`, or `ask` for withdraw, as a card id.
fn card_id(v: Option<&str>, field: &str) -> Result<Option<String>, String> {
    let Some(v) = v.map(str::trim).filter(|v| !v.is_empty()) else { return Ok(None) };
    if !v.starts_with("prp_") {
        return Err(format!("{field} is a card's prp_… id, not {v:?}"));
    }
    Ok(Some(v.to_owned()))
}

/// Where its person answers the card, and what it replaced.
fn asked_text(p: &MachineProposed, supersedes: Option<&str>, kept_old: bool) -> String {
    let mut t = if p.room.is_empty() {
        format!("Asked for approval: card {}, in their Inbox.", p.ask)
    } else {
        format!("Asked for approval: card {} (message {} in room {}).", p.ask, p.seq, p.room)
    };
    if let Some(l) = p.link.as_deref().filter(|l| !l.is_empty()) {
        t.push_str(&format!(
            " Approve or Deny it here: {l}. Chat can't approve it: give them that link if they say \"approve\"."
        ));
    }
    if !p.withdrew.is_empty() {
        t.push_str(&format!(" It replaced {}, withdrawn.", p.withdrew.join(", ")));
    } else if let (Some(old), true) = (supersedes, kept_old) {
        t.push_str(&format!(
            " Arugula cloud doesn't take supersedes yet, so {old} still stands: withdraw it, or tell your person."
        ));
    }
    t.push_str(" Their answer wakes you here, no watch needed: end your turn or go on with other work.");
    t
}

/// What control's answer says when a card route fails.
fn card_refused(e: CloudRefused) -> String {
    match e {
        CloudRefused::NotJoined => {
            "this machine hasn't joined Arugula cloud, where the card is: ask your person to join it".into()
        }
        CloudRefused::OldControl => NOT_YET.into(),
        CloudRefused::Said { status: 404 | 405, said, .. } if said.starts_with("control answered") => NOT_YET.into(),
        e => e.to_string(),
    }
}

/// The agent's own session name, and the project the card is in.
async fn mine(c: &Call<'_>, project: Option<&str>) -> Result<(String, String), String> {
    let me = c.own_pane().ok_or("ask_approval is an agent block's: call it from your own block")?;
    let session = arugulad_base::agent_mentions::session_name(c.app(), me)
        .await
        .ok_or("your block has no session name for the card: give its tab a name")?;
    Ok((session, c.current_project(project).await?))
}

/// `withdraw`: take back one of this agent's open cards.
async fn withdraw(c: &Call<'_>, a: &AskApprovalArgs) -> Out {
    let ask = card_id(a.ask.as_deref(), "ask")?.ok_or("withdraw needs ask: the card's prp_… id")?;
    let (session, project) = mine(c, a.project.as_deref()).await?;
    let body = serde_json::to_value(MachineCancel { session }).map_err(|e| e.to_string())?;
    let v = match c.app().control.cloud(Method::POST, &agent_proposal_cancel(&project, &ask), Some(&body)).await {
        Ok(v) => v,
        // Answered, withdrawn or gone: it no longer waits.
        Err(CloudRefused::Said { status: 404 | 409, said, .. }) if !said.starts_with("control answered") => {
            c.app().mux.api(|r| Api::WaitsSettled(ask.clone(), r)).await;
            return Err(format!("{ask} wasn't withdrawn: {said}"));
        }
        Err(e) => return Err(card_refused(e)),
    };
    let p: ProposalCancelled = serde_json::from_value(v).map_err(|e| format!("Arugula cloud answered oddly: {e}"))?;
    c.app().mux.api(|r| Api::WaitsSettled(p.ask.clone(), r)).await;
    done(
        format!("Withdrew card {}: it left your person's Needs you.", p.ask),
        json!({ "ask": p.ask, "state": "cancelled" }),
    )
}

pub(crate) async fn ask_approval(c: &Call<'_>, a: AskApprovalArgs) -> Out {
    if a.action.trim() == "withdraw" {
        return withdraw(c, &a).await;
    }
    let me = c.own_pane().ok_or("ask_approval is an agent block's: call it from your own block")?;
    let mut blocks = vec![];
    for b in &a.blocks {
        let id = b.id()?;
        if !blocks.contains(&id) {
            blocks.push(id);
        }
    }
    let supersedes = card_id(a.supersedes.as_deref(), "supersedes")?;
    let proposal = proposal(&a, blocks.clone())?;
    let session = arugulad_base::agent_mentions::session_name(c.app(), me)
        .await
        .ok_or("your block has no session name for the card: give its tab a name")?;
    let cards = cards_of(c, me, &blocks).await?;
    let project = c.current_project(a.project.as_deref()).await?;
    let conversation = arugulad_base::agent_mentions::asking(c.app(), me).await;
    let what = waits_line(&proposal, &cards);
    let mut body =
        MachinePropose { proposal, session, pane: me, blocks: cards, conversation, supersedes: supersedes.clone() };
    let mut kept_old = false;
    let v = loop {
        let sent = serde_json::to_value(&body).map_err(|e| e.to_string())?;
        match c.app().control.cloud(Method::POST, &agent_proposals(&project), Some(&sent)).await {
            Ok(v) => break v,
            // A control from before `supersedes` refuses the field: send the
            // card without it, and say the old one still stands.
            Err(CloudRefused::Said { status: 400 | 422, said, .. })
                if body.supersedes.is_some() && said.contains("unknown field `supersedes`") =>
            {
                body.supersedes = None;
                kept_old = true;
            }
            Err(CloudRefused::NotJoined) => {
                return Err(
                    "this machine hasn't joined Arugula cloud, where the card goes: ask your person to join it".into(),
                );
            }
            // No such route (no words of its own): a control from before the
            // cards. A 404 that says why (no such project) is passed on.
            Err(CloudRefused::Said { status: 404 | 405, said, .. }) if said.starts_with("control answered") => {
                return Err(NOT_YET.into());
            }
            Err(CloudRefused::OldControl) => return Err(NOT_YET.into()),
            Err(e) => return Err(e.to_string()),
        }
    };
    let p: MachineProposed = serde_json::from_value(v).map_err(|e| format!("Arugula cloud answered oddly: {e}"))?;
    // The cards it replaced no longer hold its block.
    for old in &p.withdrew {
        c.app().mux.api(|r| Api::WaitsSettled(old.clone(), r)).await;
    }
    // Its block waits on it until it's answered.
    c.app().mux.api(|r| Api::WaitsOn(me, p.ask.clone(), what, r)).await;
    done(
        asked_text(&p, supersedes.as_deref(), kept_old),
        json!({ "ask": p.ask, "room": p.room, "seq": p.seq, "project": project, "link": p.link, "withdrew": p.withdrew }),
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
        assert!(refused(json!({ "action": "launch", "why": "x" })).contains("start, close, run_board or withdraw"));
        assert!(refused(json!({ "action": "start", "issue": "#1", "ask": "prp_1", "why": "x" })).contains("withdraw"));
        assert!(refused(json!({ "action": "run_board", "why": "x" })).contains("needs board"));
        assert!(
            refused(json!({ "action": "start", "issue": "#1", "cap": 2, "why": "x" })).contains("go with run_board")
        );
        assert!(
            refused(json!({ "action": "run_board", "board": "brd_1", "cap": 0, "why": "x" })).contains("cap: 1 to")
        );
        assert!(
            refused(json!({ "action": "run_board", "board": "brd_1", "lapse_days": 0, "why": "x" }))
                .contains("lapse_days")
        );
        let a = args(
            json!({ "action": "run_board", "board": " brd_1 ", "machines": ["mini", " mini", ""], "why": "ready" }),
        );
        assert_eq!(
            proposal(&a, vec![]).unwrap(),
            Proposal::RunBoard {
                board: "brd_1".into(),
                machine: None,
                machines: vec!["mini".into()],
                cap: None,
                lapse_days: None,
                prompt: None,
                why: "ready".into()
            }
        );
        assert!(proposal(&args(json!({ "action": "start", "issue": "#1", "why": "x" })), vec![3]).is_err());
    }

    fn proposed(v: serde_json::Value) -> MachineProposed {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_propose_says_where_to_answer_it_and_what_it_replaced() {
        let p = proposed(json!({ "ask": "prp_2", "room": "dm_staff", "seq": 41,
            "link": "https://app.example/p/t/chat/dm_staff?msg=41", "withdrew": ["prp_1"] }));
        let t = asked_text(&p, Some("prp_1"), false);
        assert!(t.contains("card prp_2 (message 41 in room dm_staff)"), "{t}");
        assert!(t.contains("Approve or Deny it here: https://app.example/p/t/chat/dm_staff?msg=41."), "{t}");
        assert!(t.contains("replaced prp_1, withdrawn"), "{t}");
        // A personal project: no room, and the link is the Inbox.
        let p = proposed(json!({ "ask": "prp_3", "room": "", "seq": 0, "link": "https://app.example/inbox" }));
        let t = asked_text(&p, None, false);
        assert!(t.contains("card prp_3, in their Inbox") && t.contains("here: https://app.example/inbox."), "{t}");
        assert!(!t.contains("replaced"), "{t}");
        // An older control: no link; a refused supersedes leaves the old one standing.
        let p = proposed(json!({ "ask": "prp_4", "room": "r", "seq": 2 }));
        let t = asked_text(&p, Some("prp_1"), true);
        assert!(!t.contains("Approve or Deny it here"), "{t}");
        assert!(t.contains("prp_1 still stands"), "{t}");
    }

    #[test]
    fn a_card_id_is_a_prp_id() {
        assert_eq!(card_id(Some(" prp_1 "), "ask").unwrap().as_deref(), Some("prp_1"));
        assert_eq!(card_id(Some(" "), "ask").unwrap(), None);
        assert!(card_id(Some("12"), "supersedes").unwrap_err().contains("prp_"));
    }
}
