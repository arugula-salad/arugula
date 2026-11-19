//! Feedback through this daemon (Arugula #1213): what the CLI, an agent's
//! MCP tool and a web client with no account signed in ask the daemon to
//! send (the `feedback.send` operation). The daemon fills in the context
//! and, unless it's joined to the hosted control and signs, the install id
//! and the proof of work, and sends an
//! `arugula_control_wire::feedback::FeedbackSubmit` (#1216).
//!
//! Only what the writer gives is here. Nothing else is ever attached: no
//! terminal contents, file names, prompts or code. The title and body are
//! untrusted text, shown fenced as quoted data wherever they're read.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// `ARUGULA_FEEDBACK=off` (or `0`, `false`, `no`): this machine's agents
/// may not send feedback, whatever `sharing.json` says (#1216).
pub const ENV: &str = "ARUGULA_FEEDBACK";

/// What an agent is told when it may not send: the owner turned it off.
pub const AGENTS_OFF: &str = "Feedback from agents is turned off on this machine: its owner can still send it with `arugula feedback`, and turns it back on in the machine's sharing settings (Agents may send feedback to Arugula) or with `arugulad sharing agent-feedback on`.";

/// Whether this machine's agents may send feedback: the machine setting
/// (`sharing::agent_feedback`, on by default) and [`ENV`] not off. Read on
/// every call. Telemetry's consent has no say in it.
pub fn agents_may(state_dir: &Path) -> bool {
    agents_may_in(state_dir, &|k| std::env::var(k).ok())
}

/// [`agents_may`], with the environment `env` looks up (a test's own).
pub fn agents_may_in(state_dir: &Path, env: &dyn Fn(&str) -> Option<String>) -> bool {
    !env_off(env) && crate::sharing::agent_feedback(state_dir)
}

/// Whether [`ENV`] says off in `env` (the CLI asks of its own too).
pub fn env_off(env: &dyn Fn(&str) -> Option<String>) -> bool {
    env(ENV).is_some_and(|v| ["0", "off", "false", "no"].contains(&v.trim().to_lowercase().as_str()))
}

/// `POST /api/feedback` (the `feedback.send` operation): what the person or
/// agent wrote. Names are `arugula_control_wire::feedback`'s: `kind` is
/// one of `FeedbackKind`'s, `source` of `FeedbackSource`'s and `surface`
/// of `FeedbackSurface`'s. The daemon says `source` and `surface` when the
/// caller doesn't (an MCP call is an agent's, the CLI a person's).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FeedbackSendRequest {
    #[cfg_attr(feature = "ts", ts(type = "FeedbackKind"))]
    pub kind: String,
    /// At most 200 characters (`MAX_TITLE`).
    pub title: String,
    /// At most 8 KB (`MAX_BODY`).
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub contact_ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "FeedbackSource"))]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "FeedbackSurface"))]
    pub surface: Option<String>,
    /// The agent writing it, when it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub agent: Option<FeedbackAgentInfo>,
}

/// The agent writing it, as it says: `FeedbackAgent`'s fields.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FeedbackAgentInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub model: Option<String>,
}

/// What `feedback.send` answers: control's `FeedbackAck`, or, where
/// control couldn't be reached, that it's queued to go later (#1216).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FeedbackSent {
    /// `fbk_…`. Empty while it's queued.
    pub id: String,
    /// What people are shown for it: `F-…`. Empty while it's queued.
    pub shown: String,
    /// Not sent yet: control couldn't be reached, and the daemon sends it
    /// when it can.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    pub queued: bool,
}

impl FeedbackSent {
    /// The sentence the CLI and the MCP tool give for it.
    pub fn sentence(&self) -> String {
        match self.queued {
            true => "Arugula's cloud couldn't be reached, so your feedback is queued on this machine: it will send when it can. Thanks.".into(),
            false => format!("Sent to the Arugula team as feedback {}. Thanks.", self.shown),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_may_unless_the_setting_or_the_env_says_off() {
        let d = std::env::temp_dir().join(format!("arugula-feedback-may-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let none = |_: &str| None;
        assert!(agents_may_in(&d, &none));
        for off in ["off", "0", "FALSE", " no "] {
            let env = |k: &str| (k == ENV).then(|| off.to_owned());
            assert!(!agents_may_in(&d, &env), "{off:?}");
        }
        let on = |k: &str| (k == ENV).then(|| "on".to_owned());
        assert!(agents_may_in(&d, &on));
        crate::sharing::set_agent_feedback(&d, false).unwrap();
        assert!(!agents_may_in(&d, &none) && !agents_may_in(&d, &on));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
