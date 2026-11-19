//! `feedback.send` (#1216): what a person or agent writes, with the context
//! the daemon fills in, sent to the hosted control (`crate::feedback`).
//! Over HTTP (`arugula feedback`) and as MCP's `feedback`.
//!
//! The context is this daemon's version, OS and architecture, and for an
//! MCP call the session's last Arugula tool call that failed (`Listed`).
//! The daemon knows no getting-started step, so `onboarding` is left out.
//! Nothing else is attached.
//!
//! An agent's send (every MCP call; over HTTP, the CLI an agent runs, or
//! one that says `source: agent`) is refused where the owner turned agents'
//! feedback off (`arugula_proto::feedback::agents_may`); a person's never
//! is. Telemetry's consent has no say.

use arugula_control_wire::{
    feedback::{
        FeedbackAgent, FeedbackContext, FeedbackKind, FeedbackSource, FeedbackSubmit, FeedbackSurface, MAX_SHORT,
    },
    telemetry::{Arch, Os},
};
use arugula_proto::{
    feedback::{self as said, FeedbackAgentInfo, FeedbackSendRequest, FeedbackSent},
    op::ops::FeedbackSend,
};
use axum::http::StatusCode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{Cx, Handle, OpError, Via};
use crate::{
    feedback::Sent,
    mcp::{
        ops::{McpOp, Surface},
        tools::{Call, Needs, Out, done, line},
    },
};

/// One of a closed set's names, as the wire spells it; `None` for one it
/// doesn't know.
fn word<T: DeserializeOwned>(s: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(s.trim().to_lowercase())).ok()
}

/// At most [`MAX_SHORT`] bytes, cut on a character.
fn short(s: &str) -> String {
    let mut end = s.len().min(MAX_SHORT);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

fn os() -> Os {
    match std::env::consts::OS {
        "linux" => Os::Linux,
        "macos" => Os::Macos,
        "windows" => Os::Windows,
        _ => Os::Other,
    }
}

fn arch() -> Arch {
    match std::env::consts::ARCH {
        "x86_64" => Arch::X86_64,
        "aarch64" => Arch::Aarch64,
        _ => Arch::Other,
    }
}

impl Handle for FeedbackSend {
    async fn handle(cx: &Cx<'_>, _: (), req: FeedbackSendRequest) -> Result<FeedbackSent, OpError> {
        let bad = |why: &str| OpError::Status(StatusCode::BAD_REQUEST, why.to_owned());
        let kind = word::<FeedbackKind>(&req.kind)
            .filter(|k| *k != FeedbackKind::Other)
            .ok_or_else(|| bad("kind: bug, missing, idea or praise"))?;
        let (agent_runs, failed, surface) = match &cx.via {
            Via::Mcp(call) => (true, call.listed.as_ref().and_then(|l| l.last_failed()), FeedbackSurface::Mcp),
            Via::Http { agent, .. } => (*agent, None, FeedbackSurface::Cli),
        };
        // An agent is an agent whatever it says; a person may say either.
        let source = match agent_runs {
            true => FeedbackSource::Agent,
            false => req.source.as_deref().and_then(word).unwrap_or(FeedbackSource::Person),
        };
        if source == FeedbackSource::Agent && !said::agents_may(cx.app.control.state_dir()) {
            return Err(OpError::Forbidden(said::AGENTS_OFF.into()));
        }
        let surface = req.surface.as_deref().and_then(word).unwrap_or(surface);
        let agent = req.agent.filter(|a| !a.name.trim().is_empty()).map(|a| FeedbackAgent {
            name: short(a.name.trim()),
            version: a.version.as_deref().map(short),
            model: a.model.as_deref().map(short),
        });
        // A page's path only: its query or fragment could carry a token.
        let route = req
            .route
            .as_deref()
            .map(|r| r.split(['?', '#']).next().unwrap_or_default().trim())
            .filter(|r| !r.is_empty())
            .map(short);
        let send = FeedbackSubmit {
            kind,
            title: req.title.trim().to_owned(),
            body: req.body,
            contact_ok: req.contact_ok,
            source,
            surface,
            agent,
            context: FeedbackContext {
                version: Some(arugula_core::version::get().into()),
                os: Some(os()),
                arch: Some(arch()),
                onboarding: None,
                route,
                failed,
            },
            who: Default::default(),
        };
        let dir = cx.app.control.state_dir();
        match crate::feedback::sender(dir, cx.app.control.clone()).send(send).await {
            Sent::Taken(ack) => Ok(FeedbackSent { id: ack.id, shown: ack.shown, queued: false }),
            Sent::Queued => Ok(FeedbackSent { id: String::new(), shown: String::new(), queued: true }),
            Sent::Refused(code, why) => Err(OpError::Status(code, why)),
        }
    }
}

/// What it is.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Something in Arugula doesn't work.
    Bug,
    /// Something you needed from Arugula isn't there.
    Missing,
    Idea,
    Praise,
}

#[derive(Deserialize, JsonSchema)]
pub struct FeedbackArgs {
    pub kind: Kind,
    /// One line: what it's about (at most 200 characters).
    pub title: String,
    /// What you were trying to do, what happened, and what you expected (at
    /// most 8 KB). No secrets, personal data or the user's code.
    #[serde(default)]
    pub body: String,
}

/// What came of it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeedbackAnswer {
    /// What the Arugula team knows it by (`F-…`); absent while it's queued.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
    /// Arugula's cloud couldn't be reached: it's kept here and sent later.
    pub queued: bool,
}

impl McpOp for FeedbackSend {
    const SURFACE: Surface = Surface::Tool {
        name: crate::feedback::TOOL,
        title: "Send feedback to Arugula",
        description: "Tell the Arugula team about Arugula itself: a bug (bug), something you needed that isn't there (missing), an idea, or praise. Use it when Arugula got in your way or helped, not for the user's own project; you needn't ask the user first. Say what you were trying to do, what happened and what you expected. Never include secrets, tokens, personal data, or the user's code, file contents or paths: describe them instead. Arugula's version, the OS and your last failed Arugula tool call (its name and error, never its arguments) go with it.",
        destructive: false,
        idempotent: false,
        open_world: true,
        human: line(
            "tell the Arugula team what broke, what's missing or what worked",
            "If Arugula gets in your way today, send them feedback.",
            Needs::Nothing,
        ),
    };
    type Args = FeedbackArgs;
    type Answer = FeedbackAnswer;

    async fn args(call: &Call<'_>, a: &FeedbackArgs) -> Result<((), FeedbackSendRequest), String> {
        let kind = match a.kind {
            Kind::Bug => "bug",
            Kind::Missing => "missing",
            Kind::Idea => "idea",
            Kind::Praise => "praise",
        };
        // The agent, as its MCP client says (`claude-code`, `codex`).
        let agent = call.ctx.as_ref().and_then(|c| c.client_info()).filter(|i| !i.name.trim().is_empty()).map(|i| {
            FeedbackAgentInfo { name: i.name, version: Some(i.version).filter(|v| !v.trim().is_empty()), model: None }
        });
        Ok((
            (),
            FeedbackSendRequest {
                kind: kind.into(),
                title: a.title.clone(),
                body: a.body.clone(),
                contact_ok: false,
                source: Some("agent".into()),
                surface: Some("mcp".into()),
                agent,
                route: None,
            },
        ))
    }

    fn answer(_: &Call<'_>, _: &FeedbackArgs, _: &(), sent: FeedbackSent) -> Out {
        let shown = (!sent.queued).then(|| sent.shown.clone());
        done(sent.sentence(), FeedbackAnswer { shown, queued: sent.queued })
    }
}
