//! Telemetry from Arugula on people's machines (the telemetry track,
//! M100–M106): what an install whose owner said yes sends to the hosted
//! control's [`TELEMETRY`], which passes it to product analytics.
//!
//! Opt-in only: nothing is sent until the owner answers yes (the daemon's
//! `telemetry.json`, M101). A machine joined to the hosted control signs
//! its report (the daemon signature, `auth.rs` on control) and it counts
//! for its account; any other install sends it unsigned, under its random
//! install id alone, which is all control knows of it.
//!
//! Every field is a number, a closed set or our own code's location. No
//! free text: a name, a path, a command, a title, a prompt or an error
//! message can't be put in one. A value an older or newer side doesn't
//! know reads as `Other`, never as an error, so either side can add one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// An install's report (`POST`), [`Report`], answered [`Ack`](crate::Ack).
/// Signed by a daemon joined to this control, or not signed at all.
pub const TELEMETRY: &str = "/api/telemetry";

/// The signed-in account's answer for control's own page (M104): `GET`
/// [`PageConsent`], `POST` [`SetPageConsent`]. An account's, not a
/// machine's: the page isn't any one machine's.
pub const ME_TELEMETRY: &str = "/api/me/telemetry";

/// Control's page's counts (`POST`), [`PageReport`], with the session.
/// Refused (403, [`NOT_OPTED_IN`]) unless the account said yes.
pub const PAGE: &str = "/api/telemetry/page";

/// The reason [`PAGE`] gives for a 403: the account hasn't said yes.
pub const NOT_OPTED_IN: &str = "telemetry_off";

/// The most any one count in a [`PageReport`] is taken as (a minute's is
/// far less).
pub const MAX_PAGE_COUNT: u32 = 10_000;

/// Whether the account lets control's page send usage counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageConsent {
    /// `None`: not asked yet, which counts as no.
    pub on: Option<bool>,
    /// When it was answered, in ms.
    pub at: Option<u64>,
    /// Whether the page should ask: this control forwards reports. One
    /// without analytics would send them nowhere, so it doesn't ask.
    pub ask: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetPageConsent {
    pub on: bool,
}

/// What control's page counted since its last report: views opened, and
/// how many errors our code hit (a count, never what they said).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageReport {
    #[serde(default)]
    pub views: BTreeMap<View, u32>,
    #[serde(default)]
    pub web_errors: u32,
}

/// The most events and crashes one report may carry.
pub const MAX_EVENTS: usize = 50;
pub const MAX_CRASHES: usize = 10;
/// The most frames a crash keeps.
pub const MAX_FRAMES: usize = 32;

/// What an install sends: who it is, and what happened since its last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// Random, 16 hex digits, made when its owner said yes and dropped
    /// when they say no (a new one if they say yes again).
    pub install: String,
    /// Its `arugulad` version.
    pub version: String,
    pub os: Os,
    pub arch: Arch,
    /// How it was installed.
    #[serde(default)]
    pub channel: Channel,
    /// Which control it's joined to, if any.
    #[serde(default)]
    pub control: ControlKind,
    #[serde(default)]
    pub events: Vec<Event>,
    #[serde(default)]
    pub crashes: Vec<Crash>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Linux,
    Macos,
    Windows,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    X86_64,
    Aarch64,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Channel {
    /// install.sh or install.ps1.
    Script,
    Homebrew,
    /// The desktop app installed it.
    Desktop,
    /// Built from source (`cargo install`, a checkout).
    Source,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlKind {
    #[default]
    None,
    /// control.arugula.io.
    Hosted,
    /// Someone else's control.
    SelfHosted,
    #[serde(other)]
    Other,
}

/// Something that happened on the install.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    /// The owner said yes (the first report after it).
    OptedIn {
        /// Where they were asked.
        via: Surface,
    },
    /// The daemon started.
    Started {
        /// It runs as a service (systemd, launchd, a Windows service).
        service: bool,
    },
    /// The daemon's day: counts since the last one (at most one a day).
    Daily(Counts),
    /// Unknown to this side.
    #[serde(other)]
    Other,
}

/// Where a person answered, or used something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Surface {
    Web,
    Desktop,
    Cli,
    Tui,
    Phone,
    Mcp,
    #[serde(other)]
    Other,
}

/// A day's counts. Keys are closed sets; a count is how many times.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub panes_opened: u32,
    /// The most panes open at once.
    pub panes_peak: u32,
    #[serde(default)]
    pub agents_started: BTreeMap<AgentRuntime, u32>,
    #[serde(default)]
    pub blocks_opened: BTreeMap<BlockKind, u32>,
    /// Clients that attached, by where.
    #[serde(default)]
    pub clients: BTreeMap<Surface, u32>,
    /// Views and features people opened in the web client and the app
    /// (M103).
    #[serde(default)]
    pub views: BTreeMap<View, u32>,
    /// Labs flags on at the end of the day.
    #[serde(default)]
    pub flags_on: Vec<LabsFlag>,
    /// Errors the web client caught (M102), by where in our bundle.
    #[serde(default)]
    pub web_errors: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentRuntime {
    Claude,
    Codex,
    Gemini,
    Cloud,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlockKind {
    Terminal,
    Agent,
    Browser,
    Editor,
    Review,
    Files,
    Pr,
    Issue,
    Vm,
    Workspace,
    Studio,
    Conversation,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum View {
    Swarm,
    Chat,
    Conversations,
    Agents,
    Palette,
    Settings,
    GettingStarted,
    Share,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LabsFlag {
    Chat,
    Vms,
    Studio,
    Workspaces,
    GuestSsh,
    SwarmThemes,
    Forges,
    Agents,
    #[serde(other)]
    Other,
}

/// A panic in one of our programs (M102). Only where: never its message,
/// which can hold a path or someone's text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crash {
    pub process: Process,
    /// The version that crashed (it may have been updated since).
    pub version: String,
    /// `crates/…/file.rs:line`, in our own source. A panic elsewhere (a
    /// dependency) is the first of our frames that called it.
    pub location: String,
    /// Our own functions on the stack, innermost first, at most
    /// [`MAX_FRAMES`]: `arugula_mux::pane::Pane::resize`. Nothing outside
    /// our crates is kept.
    #[serde(default)]
    pub frames: Vec<String>,
    /// When, in ms.
    pub at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Process {
    Daemon,
    Cli,
    Desktop,
    #[serde(other)]
    Other,
}

impl Report {
    /// Whether control takes it: a well-formed install id, versions and
    /// locations that look like ours, and no more than the limits.
    pub fn check(&self) -> Result<(), &'static str> {
        let hex16 = |s: &str| s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
        let version = |s: &str| {
            !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
        };
        let ours = |s: &str, prefix: &str| {
            s.starts_with(prefix)
                && s.len() <= 200
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_:/.-<>".contains(&b))
        };
        if !hex16(&self.install) {
            return Err("install: 16 lowercase hex digits");
        }
        if !version(&self.version) {
            return Err("version: like 0.42.0");
        }
        if self.events.len() > MAX_EVENTS || self.crashes.len() > MAX_CRASHES {
            return Err("too many events or crashes in one report");
        }
        for c in &self.crashes {
            if !version(&c.version)
                || !ours(&c.location, "crates/")
                || c.frames.len() > MAX_FRAMES
                || !c.frames.iter().all(|f| ours(f, "arugula"))
            {
                return Err("a crash is a version, a location and our own frames");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_values_read_as_other() {
        let r: Report = serde_json::from_value(json!({
            "install": "0123456789abcdef", "version": "0.42.0", "os": "plan9", "arch": "riscv64",
            "channel": "nix", "control": "hosted",
            "events": [
                { "event": "started", "service": true },
                { "event": "something-newer", "x": 1 },
                { "event": "daily", "panes_opened": 3, "panes_peak": 2,
                  "agents_started": { "claude": 2, "aider": 1 }, "views": { "swarm": 4 } },
            ],
        }))
        .unwrap();
        assert_eq!((r.os, r.arch, r.channel, r.control), (Os::Other, Arch::Other, Channel::Other, ControlKind::Hosted));
        assert_eq!(r.events[1], Event::Other);
        let Event::Daily(c) = &r.events[2] else { panic!() };
        assert_eq!(c.agents_started[&AgentRuntime::Other], 1);
        assert_eq!(c.views[&View::Swarm], 4);
        assert!(r.check().is_ok());
    }

    #[test]
    fn checks() {
        let mut r = Report {
            install: "0123456789abcdef".into(),
            version: "0.42.0".into(),
            os: Os::Linux,
            arch: Arch::X86_64,
            channel: Channel::Script,
            control: ControlKind::None,
            events: vec![],
            crashes: vec![Crash {
                process: Process::Daemon,
                version: "0.42.0".into(),
                location: "crates/mux/src/pane.rs:812".into(),
                frames: vec!["arugula_mux::pane::Pane::resize".into()],
                at: 1,
            }],
        };
        assert!(r.check().is_ok());
        r.crashes[0].location = "/home/someone/project/x.rs:1".into();
        assert!(r.check().is_err(), "a path outside our crates isn't ours");
        r.crashes[0].location = "crates/mux/src/pane.rs:812".into();
        r.crashes[0].frames.push("serde_json::de::from_str".into());
        assert!(r.check().is_err(), "a frame outside our crates isn't kept");
        r.crashes.clear();
        r.install = "not-hex".into();
        assert!(r.check().is_err());
    }
}
