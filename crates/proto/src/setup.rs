//! Getting started's buttons (`/api/setup/…`): what they send and what a
//! button that did something answers. The daemon runs them
//! (`crates/daemon/src/setup.rs`); the status they sit beside is a JSON
//! object whose keys depend on `?part=`, so it is read as a value.

use serde::{Deserialize, Serialize};

/// What a button did: done, or why not and what fixes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    #[serde(default)]
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// A command for the person to run once (it needs sudo, say).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// A page for the person to open (Tailscale's admin console, say).
    /// (Boxed: an `Err` of its own in the daemon's `install` and `add_mcp`.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Box<Link>>,
    /// What changed, a line each (#335).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub done: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub label: String,
    pub url: String,
}

impl Outcome {
    pub fn ok() -> Self {
        Self { ok: true, ..Default::default() }
    }
    pub fn err(e: impl Into<String>) -> Self {
        Self { error: Some(e.into()), ..Default::default() }
    }
    pub fn fix(mut self, cmd: impl Into<String>) -> Self {
        self.fix = Some(cmd.into());
        self
    }
    pub fn link(mut self, label: &str, url: impl Into<String>) -> Self {
        self.link = Some(Box::new(Link { label: label.into(), url: url.into() }));
        self
    }
}

/// `GET /api/setup`'s query.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SetupQuery {
    /// `control`: only that (cheap, for polling while a join waits);
    /// `agents`: Claude Code's MCP server and the adapters (#335). The
    /// whole runs `tailscale`, `claude` and `node`.
    pub part: Option<String>,
}

/// `POST /api/setup/control`: the body may be left off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ControlJoinRequest {
    /// Control's address (Arugula's cloud by default).
    pub url: Option<String>,
    /// A team to put it in ahead of time (its id).
    pub team: Option<String>,
}

/// `POST /api/setup/control/confirm`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlConfirmRequest {
    /// The fingerprint here is the one the approving device shows.
    pub same: bool,
}
