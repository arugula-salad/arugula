//! `todo` (Arugula #1463): an agent hands its person work only they can do,
//! outside Arugula (an approval on GitHub, a script with their credentials,
//! a secret put somewhere), as a to-do (`Proposal::Todo`, Arugula #1462).
//! It's the machine's approval card (`approvals.rs`, #1417) with **Done**
//! and **Can't**: control posts it in the person's staff room, under Needs
//! you and on their phone, checked as this agent's posts there are
//! (`threads.post`).
//!
//! - **From the caller, never the arguments.** Whom it's for (this
//!   machine's owner: the route is signed as the machine), the session (its
//!   name here), the conversation (`{agent: claude|codex, id}`, so an answer
//!   can find it once its block is gone) and the card (the issue its block
//!   was started on, else its pane's own, when it's a native `#N` in the
//!   current project). The agent says what, why and how; nothing else.
//! - **`list` is this machine's.** Control has no route that lists a
//!   machine agent's to-dos, so the ones added here are kept in
//!   `todos.json` in the state dir, by conversation (by session name for a
//!   caller with none), until they're cancelled or answered: the answer
//!   wakes the agent with its note, and drops the to-do here as it arrives
//!   (Arugula #1465, `agent_mentions::on_answer`). One answered while this
//!   machine was away stays until its answer comes, or a cancel finds it
//!   settled.
//! - **`cancel`** is `agent_proposal_cancel`, as the session that asked;
//!   control refuses one that isn't open, and says how it stands.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use arugula_control_wire::sessions::{
    MachineCancel, MachinePropose, MachineProposed, PROPOSAL_WHY_MAX, Proposal, ProposalCancelled, TODO_HOW_MAX,
    TODO_WHAT_MAX, TodoConversation, agent_proposal_cancel, agent_proposals,
};
use arugula_proto::PaneId;
use reqwest::Method;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    control::CloudRefused,
    mcp::tools::{Call, Kind, Out, done, parse},
    mux::Api,
};

pub(crate) const TOOL: &str = "todo";

pub(crate) const DESCRIPTION: &str = "Hand your person a task you can't do yourself: approve something on GitHub, run a script with their credentials, put a secret somewhere. It waits under Needs you, on their phone and on your card until they mark it Done or Can't, with a note. It's for the person this machine belongs to, on the card you're on: you don't name either. Their answer wakes you here with their note, no watch needed: don't wait or poll, end your turn or go on with other work. Call it from your agent block, or a terminal with Arugula's hooks.";

/// `todo`'s kinds.
pub(crate) const KINDS: &[Kind] = &[
    Kind {
        name: "add",
        description: "Add one: what in the imperative, one line (at most 120 characters); why you can't do it yourself (at most 500); how as plain-text steps, commands verbatim (at most 4000), saying where a secret goes, never the secret. Returns its id (prp_…).",
        schema: schema_for_type::<AddArgs>,
        flag: None,
    },
    Kind {
        name: "list",
        description: "Your open to-dos, as this machine added them: id, what, its card and project. One leaves once its answer reaches you.",
        schema: schema_for_type::<ListArgs>,
        flag: None,
    },
    Kind {
        name: "cancel",
        description: "Cancel one you no longer need (you found another way), by id: it leaves Needs you.",
        schema: schema_for_type::<CancelArgs>,
        flag: None,
    },
];

#[derive(Deserialize, JsonSchema)]
pub struct AddArgs {
    /// What to do, in the imperative, one line: "Approve the deploy on
    /// GitHub". At most 120 characters.
    pub what: String,
    /// Why you can't do it yourself, in a sentence or two. At most 500
    /// characters.
    pub why: String,
    /// The steps, plain text shown as written: commands verbatim, and where
    /// a secret goes, never the secret itself. At most 4000 characters.
    #[serde(default)]
    pub how: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListArgs {}

#[derive(Deserialize, JsonSchema)]
pub struct CancelArgs {
    /// The to-do (prp_…), as add returned it.
    pub id: String,
}

/// What a control from before to-dos means for the agent.
const NOT_YET: &str = "Arugula cloud doesn't take a machine's to-dos yet: tell your person in chat instead";

pub(crate) async fn todo(c: &Call<'_>, kind: &str, args: Value) -> Out {
    match kind {
        "add" => add(c, parse(args)?).await,
        "list" => {
            let _: ListArgs = parse(args)?;
            list(c).await
        }
        "cancel" => cancel(c, parse(args)?).await,
        k => Err(format!("{TOOL} kind {k} isn't wired up")),
    }
}

/// The caller: its pane, its session's name, and its conversation when it
/// has one an answer could find.
struct Caller {
    pane: PaneId,
    session: String,
    conversation: Option<TodoConversation>,
}

async fn caller(c: &Call<'_>) -> Result<Caller, String> {
    let pane = c.own_pane().ok_or("todo is an agent's: call it from your agent block, or a pane in Arugula")?;
    let session = arugulad_base::agent_mentions::session_name(c.app(), pane)
        .await
        .ok_or("your pane has no session name for the to-do: give its tab a name")?;
    // None for another ACP agent: control would refuse its harness, and
    // nothing answers to it (Arugula #1465).
    let conversation = arugulad_base::agent_mentions::asking(c.app(), pane).await;
    Ok(Caller { pane, session, conversation })
}

/// The card's number, when the caller's card is a native `#N` of
/// `project`.
async fn card_in(c: &Call<'_>, project: &str) -> Option<u64> {
    let (on, issue) = c.my_card().await?;
    (on == project).then(|| native_number(&issue)).flatten()
}

/// `#12` or `12`; not another forge's issue.
fn native_number(issue: &str) -> Option<u64> {
    issue.trim().trim_start_matches('#').parse().ok()
}

/// The to-do, checked here as control checks it again (it also refuses
/// text that looks like a secret).
fn proposal(a: &AddArgs, issue: Option<u64>, conversation: Option<TodoConversation>) -> Result<Proposal, String> {
    let what = a.what.trim();
    if what.is_empty() {
        return Err("say what: one line, in the imperative".into());
    }
    if what.contains(['\n', '\r']) {
        return Err("what is one line: the steps go in how".into());
    }
    if what.chars().count() > TODO_WHAT_MAX {
        return Err(format!("what: at most {TODO_WHAT_MAX} characters"));
    }
    let why = a.why.trim();
    if why.is_empty() {
        return Err("say why you can't do it yourself: the card shows it".into());
    }
    if why.chars().count() > PROPOSAL_WHY_MAX {
        return Err(format!("why: at most {PROPOSAL_WHY_MAX} characters"));
    }
    let how = a.how.trim_end();
    if how.chars().count() > TODO_HOW_MAX {
        return Err(format!("how: at most {TODO_HOW_MAX} characters"));
    }
    Ok(Proposal::Todo { what: what.into(), why: why.into(), how: how.into(), issue, conversation })
}

/// What control's refusal means for the agent: a control from before
/// to-dos (no such route, no words of its own) is [`NOT_YET`].
fn refused(e: CloudRefused) -> String {
    match e {
        CloudRefused::NotJoined => {
            "this machine hasn't joined Arugula cloud, where the to-do goes: ask your person to join it".into()
        }
        CloudRefused::Said { status: 404 | 405, said, .. } if said.starts_with("control answered") => NOT_YET.into(),
        CloudRefused::OldControl => NOT_YET.into(),
        e => e.to_string(),
    }
}

async fn add(c: &Call<'_>, a: AddArgs) -> Out {
    let me = caller(c).await?;
    let project = c.current_project(None).await?;
    let issue = card_in(c, &project).await;
    let proposal = proposal(&a, issue, me.conversation.clone())?;
    let Proposal::Todo { what, .. } = &proposal else { unreachable!("proposal makes a to-do") };
    let what = what.clone();
    let conversation = me.conversation.clone();
    let body = MachinePropose {
        proposal,
        session: me.session.clone(),
        pane: me.pane,
        blocks: vec![],
        conversation,
        supersedes: None,
    };
    let body = serde_json::to_value(&body).map_err(|e| e.to_string())?;
    let v = c.app().control.cloud(Method::POST, &agent_proposals(&project), Some(&body)).await.map_err(refused)?;
    let p: MachineProposed = serde_json::from_value(v).map_err(|e| format!("Arugula cloud answered oddly: {e}"))?;
    let kept = Kept {
        id: p.ask.clone(),
        project: project.clone(),
        session: me.session,
        conversation: me.conversation,
        what,
        issue,
        at: now_ms(),
    };
    // Its pane waits on it, under its what, until it's answered.
    c.app().mux.api(|r| Api::WaitsOn(me.pane, p.ask.clone(), kept.what.clone(), r)).await;
    let dir = c.app().control.state_dir().to_owned();
    let unkept = match keep(&dir, kept) {
        Ok(()) => String::new(),
        Err(e) => format!(" (list won't show it: {e})"),
    };
    let on = issue.map(|n| format!(" on #{n}")).unwrap_or_default();
    done(
        format!(
            "Added to-do {}{on} for your person: it waits under Needs you until they mark it Done or Can't, and their answer wakes you here with their note, no watch needed: end your turn or go on with other work.{unkept}",
            p.ask
        ),
        json!({ "todo": p.ask, "status": "open", "card": issue, "project": project, "room": p.room, "seq": p.seq }),
    )
}

async fn list(c: &Call<'_>) -> Out {
    let me = caller(c).await?;
    let dir = c.app().control.state_dir().to_owned();
    let mine: Vec<Kept> = read(&dir).into_iter().filter(|k| k.is_of(&me)).collect();
    if mine.is_empty() {
        return done("You have no open to-dos.", json!({ "todos": [] }));
    }
    let lines: Vec<String> = mine
        .iter()
        .map(|k| {
            let on = k.issue.map(|n| format!(" (#{n})")).unwrap_or_default();
            format!("- {}: {}{on}, in {}", k.id, k.what, k.project)
        })
        .collect();
    let todos: Vec<Value> = mine
        .iter()
        .map(|k| json!({ "id": k.id, "what": k.what, "card": k.issue, "project": k.project, "added_at": k.at }))
        .collect();
    done(format!("Your open to-dos, as added here:\n{}", lines.join("\n")), json!({ "todos": todos }))
}

async fn cancel(c: &Call<'_>, a: CancelArgs) -> Out {
    let id = a.id.trim();
    if id.is_empty() {
        return Err("cancel needs id: the to-do's prp_… id".into());
    }
    let me = caller(c).await?;
    let dir = c.app().control.state_dir().to_owned();
    let kept = read(&dir).into_iter().find(|k| k.id == id && k.is_of(&me));
    let project = match kept {
        Some(k) => k.project,
        None => c.current_project(None).await?,
    };
    let body = serde_json::to_value(MachineCancel { session: me.session }).map_err(|e| e.to_string())?;
    let v = match c.app().control.cloud(Method::POST, &agent_proposal_cancel(&project, id), Some(&body)).await {
        Ok(v) => v,
        // Answered, cancelled or gone: it's no longer open, so not listed.
        Err(CloudRefused::Said { status: 404 | 409, said, .. }) if !said.starts_with("control answered") => {
            drop_kept(&dir, id);
            c.app().mux.api(|r| Api::WaitsSettled(id.to_owned(), r)).await;
            return Err(format!("{id} isn't open: {said}"));
        }
        Err(e) => return Err(refused(e)),
    };
    let p: ProposalCancelled = serde_json::from_value(v).map_err(|e| format!("Arugula cloud answered oddly: {e}"))?;
    drop_kept(&dir, &p.ask);
    c.app().mux.api(|r| Api::WaitsSettled(p.ask.clone(), r)).await;
    done(format!("Cancelled to-do {}: it left Needs you.", p.ask), json!({ "todo": p.ask, "status": "cancelled" }))
}

// ---------------------------------------------------------------- kept here

/// A to-do added here, for `list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Kept {
    /// `prp_…`.
    id: String,
    project: String,
    session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    conversation: Option<TodoConversation>,
    what: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    issue: Option<u64>,
    /// When it was added, ms.
    at: u64,
}

impl Kept {
    /// The caller's: its conversation's, or with none, its session's.
    fn is_of(&self, me: &Caller) -> bool {
        match &me.conversation {
            Some(c) => self.conversation.as_ref() == Some(c),
            None => self.conversation.is_none() && self.session == me.session,
        }
    }
}

/// The most to-dos kept here: the oldest go first. Control keeps at most 5
/// open per agent, so this is many agents' worth.
const KEPT_MAX: usize = 200;

/// One writer at a time.
static FILE: Mutex<()> = Mutex::new(());

fn file(dir: &Path) -> PathBuf {
    dir.join("todos.json")
}

fn read(dir: &Path) -> Vec<Kept> {
    let _held = FILE.lock().unwrap_or_else(|e| e.into_inner());
    read_unlocked(dir)
}

fn read_unlocked(dir: &Path) -> Vec<Kept> {
    std::fs::read(file(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_unlocked(dir: &Path, all: &[Kept]) -> std::io::Result<()> {
    let tmp = dir.join("todos.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(all)?)?;
    std::fs::rename(tmp, file(dir))
}

fn keep(dir: &Path, k: Kept) -> std::io::Result<()> {
    let _held = FILE.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = read_unlocked(dir);
    all.retain(|o| o.id != k.id);
    all.push(k);
    let over = all.len().saturating_sub(KEPT_MAX);
    all.drain(..over);
    write_unlocked(dir, &all)
}

/// Also when its answer reaches this machine (Arugula #1465,
/// `agent_mentions::on_answer`): answered, it's no longer open.
pub(crate) fn drop_kept(dir: &Path, id: &str) {
    let _held = FILE.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = read_unlocked(dir);
    let n = all.len();
    all.retain(|k| k.id != id);
    if all.len() != n {
        // Not forgotten: list shows it until a cancel finds it settled.
        let _ = write_unlocked(dir, &all);
    }
}

fn now_ms() -> u64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: Value) -> AddArgs {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_todo_says_what_why_and_how_and_nothing_else() {
        let conv = TodoConversation { agent: "claude".into(), id: "c-1".into() };
        let a =
            args(json!({ "what": " Approve the deploy on GitHub ", "why": "it needs your login", "how": "Open it\n" }));
        assert_eq!(
            proposal(&a, Some(12), Some(conv.clone())).unwrap(),
            Proposal::Todo {
                what: "Approve the deploy on GitHub".into(),
                why: "it needs your login".into(),
                how: "Open it".into(),
                issue: Some(12),
                conversation: Some(conv),
            }
        );
        // Who it's for, the session and the card aren't arguments.
        for extra in ["person", "session", "issue", "conversation", "project"] {
            let mut v = json!({ "what": "x", "why": "y" });
            v[extra] = json!("someone");
            let a = args(v);
            assert!(matches!(
                proposal(&a, None, None).unwrap(),
                Proposal::Todo { issue: None, conversation: None, .. }
            ));
        }
        let refused = |v| proposal(&args(v), None, None).unwrap_err();
        assert!(refused(json!({ "what": " ", "why": "y" })).contains("say what"));
        assert!(refused(json!({ "what": "a\nb", "why": "y" })).contains("one line"));
        assert!(refused(json!({ "what": "x".repeat(TODO_WHAT_MAX + 1), "why": "y" })).contains("what: at most"));
        assert!(refused(json!({ "what": "x", "why": "" })).contains("say why"));
        assert!(refused(json!({ "what": "x", "why": "y".repeat(PROPOSAL_WHY_MAX + 1) })).contains("why: at most"));
        assert!(refused(json!({ "what": "x", "why": "y", "how": "z".repeat(TODO_HOW_MAX + 1) })).contains("how:"));
    }

    #[test]
    fn a_card_is_a_native_number() {
        assert_eq!(native_number("#12"), Some(12));
        assert_eq!(native_number("12"), Some(12));
        assert_eq!(native_number("acme/app#12"), None);
        assert_eq!(native_number("ALL-12"), None);
    }

    fn kept(id: &str, session: &str, conversation: Option<&str>) -> Kept {
        Kept {
            id: id.into(),
            project: "prj_1".into(),
            session: session.into(),
            conversation: conversation.map(|c| TodoConversation { agent: "claude".into(), id: c.into() }),
            what: "Do it".into(),
            issue: None,
            at: 1,
        }
    }

    #[test]
    fn list_is_the_callers_and_cancel_forgets_one() {
        let dir = std::env::temp_dir().join(format!("arugula-todos-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        keep(&dir, kept("prp_1", "lead", Some("c-1"))).unwrap();
        keep(&dir, kept("prp_2", "lead", Some("c-2"))).unwrap();
        keep(&dir, kept("prp_3", "term", None)).unwrap();
        let conv = |id: &str| Some(TodoConversation { agent: "claude".into(), id: id.into() });
        let me = Caller { pane: 1, session: "lead".into(), conversation: conv("c-1") };
        let mine: Vec<String> = read(&dir).into_iter().filter(|k| k.is_of(&me)).map(|k| k.id).collect();
        assert_eq!(mine, ["prp_1"], "another conversation of the same name isn't this one");
        let term = Caller { pane: 2, session: "term".into(), conversation: None };
        assert!(read(&dir).iter().any(|k| k.id == "prp_3" && k.is_of(&term)));
        drop_kept(&dir, "prp_1");
        assert!(!read(&dir).iter().any(|k| k.is_of(&me)));
        for n in 0..KEPT_MAX + 5 {
            keep(&dir, kept(&format!("prp_x{n}"), "lead", None)).unwrap();
        }
        let all = read(&dir);
        assert_eq!(all.len(), KEPT_MAX);
        assert_eq!(all.last().unwrap().id, format!("prp_x{}", KEPT_MAX + 4), "the oldest go first");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
