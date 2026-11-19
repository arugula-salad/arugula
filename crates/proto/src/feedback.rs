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

use serde::{Deserialize, Serialize};

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

/// What `feedback.send` answers: control's `FeedbackAck`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FeedbackSent {
    /// `fbk_…`.
    pub id: String,
    /// What people are shown for it: `F-…`.
    pub shown: String,
}
