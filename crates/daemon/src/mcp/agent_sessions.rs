//! A person's agent and agent sessions (Fleet 3, aru-121): `session_start`
//! and `session_reply`. With its owner's grants over a project, this
//! machine's agents start an agent session there as the owner
//! (`sessions.start`) and answer a session's question or prompt it
//! (`sessions.reply`), without a browser.
//!
//! Like the thread tools, they live on control's platform, never here: each
//! is a `/api/daemon/sessions…` route signed with this machine's key, and
//! the platform asks the grant every time
//! (`crates/platform/src/chat/agent_sessions.rs`). The ask or the reply is
//! a message in the session's thread, labelled "<owner>'s agent on
//! <machine>"; the owner is the session's asker, and its bill is theirs.
//! Neither approves anything: a session's permission waits on a person.
//!
//! An agent a guest stands behind may not use them: they act as the owner.

use arugula_control_wire::sessions::{AGENT_SESSIONS, AgentSessionStart, agent_session_reply};
use reqwest::Method;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    issues::Native,
    tools::{Call, Out, done},
};
use crate::{control::CloudRefused, mux::Api};

pub(crate) const START_TOOL: &str = "session_start";
pub(crate) const REPLY_TOOL: &str = "session_reply";

pub(crate) const START_DESCRIPTION: &str = "Start an agent session in Arugula control as your person: @arugula on a native issue (issue: #N in the current project, or iss_…) or in a project's channel (room, and thread: a message's seq to ask in its thread). It runs on the project's cloud workspace, as your person (the asker, who pays for it), and its card says it was started by their agent on this machine. Needs their sessions.start grant over the project. prompt is what to ask (it may name the repo, owner/name, and a model); on an issue it defaults to working on it. Returns the session (ses_…), or what the platform said instead: why not, or which repo it asks (ask again there with the repo, owner/name, in prompt). session_reply answers it or prompts it after.";

pub(crate) const REPLY_DESCRIPTION: &str = "Reply to an agent session in Arugula control as your person (ses_…, from session_start or the thread's card), in its thread: when it waits on a question, your text answers it; otherwise it's the session's next prompt (queued while a turn runs). Never an approval: a permission waits on a person, and your text is queued for after it. Needs your person's sessions.reply grant over its project. Says which it did.";

#[derive(Deserialize, JsonSchema)]
pub struct SessionStartArgs {
    /// A native issue: "#N" (in the current project) or "iss_…".
    #[serde(default)]
    pub issue: Option<String>,
    /// Or a project's channel: its room id (list kind threads).
    #[serde(default)]
    pub room: Option<String>,
    /// In the room, the thread of this message (its seq); without it, the
    /// ask is a message of its own, starting its thread.
    #[serde(default)]
    pub thread: Option<i64>,
    /// What to ask @arugula: plain words, which may name the repo
    /// (owner/name) and a model. On an issue, by default, to work on it.
    #[serde(default)]
    pub prompt: Option<String>,
    /// "#N"'s project: its id (prj_…) or personal. Default: the one this
    /// clone's GitHub repo is linked to, else your person's personal one.
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SessionReplyArgs {
    /// The session: its id (ses_…).
    pub session: String,
    /// The answer or the prompt, in markdown.
    pub text: String,
}

/// What a refusal says to an agent.
fn said(e: CloudRefused) -> String {
    match e {
        CloudRefused::NotJoined => "this machine hasn't joined Arugula control, where sessions live: ask your person to join it (Getting started on the web page, or `arugulad join`)".into(),
        CloudRefused::OldControl | CloudRefused::Said { status: 405, .. } => {
            "this Arugula control can't start sessions for agents yet".into()
        }
        e => e.to_string(),
    }
}

/// The body for [`AGENT_SESSIONS`], from the tool's arguments: `issue`'s
/// number is sent with its project, which the caller resolved.
pub(crate) fn start_body(a: &SessionStartArgs, project: Option<String>) -> Result<AgentSessionStart, String> {
    let room = a.room.as_deref().map(str::trim).filter(|r| !r.is_empty());
    let prompt = a.prompt.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned);
    match (a.issue.as_deref().map(str::trim).filter(|i| !i.is_empty()), room) {
        (Some(_), Some(_)) => Err("say where: an issue or a room, not both".into()),
        (None, None) => Err("say where: issue (#N or iss_…), or room (a project's channel)".into()),
        (Some(i), None) => {
            if a.thread.is_some() {
                return Err("an issue's thread is the issue: leave thread out".into());
            }
            match Native::parse(i) {
                Some(Native::Id(id)) => Ok(AgentSessionStart { issue: Some(id), prompt, ..Default::default() }),
                Some(Native::Number(n)) => {
                    Ok(AgentSessionStart { number: Some(n), project, prompt, ..Default::default() })
                }
                None => Err(format!("issue: #N (in the current project) or iss_…, not {i:?}")),
            }
        }
        (None, Some(r)) => {
            if prompt.is_none() {
                return Err("say what to ask @arugula: prompt".into());
            }
            Ok(AgentSessionStart { room: Some(r.to_owned()), thread: a.thread, prompt, ..Default::default() })
        }
    }
}

impl Call<'_> {
    /// They act as this machine's owner: only the owner's own agents may.
    async fn owners_sessions(&self, tool: &str) -> Result<(), String> {
        if let Some(me) = self.me()
            && self.app().mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some()
        {
            return Err(format!("{tool} acts as this machine's owner: only their own agents may"));
        }
        Ok(())
    }

    async fn sessions_call(&self, path: &str, body: Value) -> Result<Value, String> {
        self.app().control.cloud(Method::POST, path, Some(&body)).await.map_err(said)
    }

    /// `session_start`.
    pub(crate) async fn session_start(&self, a: SessionStartArgs) -> Out {
        self.owners_sessions(START_TOOL).await?;
        let number = a.issue.as_deref().and_then(Native::parse).is_some_and(|n| matches!(n, Native::Number(_)));
        let project = match number {
            true => Some(self.current_project(a.project.as_deref()).await?),
            false => None,
        };
        let body = serde_json::to_value(start_body(&a, project)?).map_err(|e| e.to_string())?;
        let v = self.sessions_call(AGENT_SESSIONS, body).await?;
        let at = format!("seq {} in {}", v["seq"], v["room"].as_str().unwrap_or("?"));
        let s = match v["session"].as_str() {
            Some(ses) => format!(
                "Started session {ses} as your person, asked by their agent on this machine ({at}). Its answers come in that thread (read_thread); session_reply answers or prompts it."
            ),
            None => {
                let said: Vec<&str> = v["said"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                match said.as_slice() {
                    [] => format!("Asked @arugula ({at}), but no session started and the platform said nothing."),
                    lines => format!("Asked @arugula ({at}); no session yet. The platform said:\n{}", lines.join("\n")),
                }
            }
        };
        done(s, v)
    }

    /// `session_reply`.
    pub(crate) async fn session_reply(&self, a: SessionReplyArgs) -> Out {
        self.owners_sessions(REPLY_TOOL).await?;
        let ses = a.session.trim();
        if !ses.starts_with("ses_") {
            return Err(format!("session: its id, ses_…, not {ses:?}"));
        }
        let v = self.sessions_call(&agent_session_reply(ses), json!({ "text": a.text })).await?;
        let s = match v["did"].as_str() {
            Some("answered") => format!("Answered session {ses}'s question, as your person's agent on this machine."),
            Some("queued") if v["permission"] == true => format!(
                "Session {ses} waits on a permission, which a person allows or denies: your reply is queued as its prompt for after."
            ),
            Some("queued") => {
                format!("Queued as session {ses}'s next prompt (it starts when the turn under way ends).")
            }
            Some("refused") => format!(
                "Posted in session {ses}'s thread, but it took no prompt: {}",
                v["why"].as_str().unwrap_or("it said no")
            ),
            _ => {
                format!("Posted in session {ses}'s thread, but your person only views its project: it took no prompt.")
            }
        };
        done(s, v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: Value) -> SessionStartArgs {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_start_is_an_issue_or_a_room() {
        let id = format!("iss_{}", "a".repeat(32));
        let b = start_body(&args(json!({ "issue": id })), None).unwrap();
        assert_eq!((b.issue.as_deref(), b.number, b.prompt), (Some(id.as_str()), None, None));
        let b = start_body(&args(json!({ "issue": "#12", "prompt": " fix it " })), Some("prj_1".into())).unwrap();
        assert_eq!((b.number, b.project.as_deref(), b.prompt.as_deref()), (Some(12), Some("prj_1"), Some("fix it")));
        let b = start_body(&args(json!({ "room": "ch_1", "thread": 4, "prompt": "go" })), None).unwrap();
        assert_eq!((b.room.as_deref(), b.thread, b.issue), (Some("ch_1"), Some(4), None));
        assert!(start_body(&args(json!({})), None).unwrap_err().contains("say where"));
        assert!(start_body(&args(json!({ "issue": "#1", "room": "ch_1" })), None).unwrap_err().contains("not both"));
        assert!(start_body(&args(json!({ "room": "ch_1" })), None).unwrap_err().contains("prompt"));
        assert!(start_body(&args(json!({ "issue": "#1", "thread": 2 })), None).unwrap_err().contains("thread"));
        assert!(start_body(&args(json!({ "issue": "o/r#1" })), None).unwrap_err().contains("#N"));
    }
}
