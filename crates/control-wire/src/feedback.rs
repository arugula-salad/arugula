//! Feedback (Arugula #1213): a bug, something missing, an idea or praise,
//! from a person or their agent, sent to the hosted control's [`FEEDBACK`]
//! and kept in one inbox (the plan Doc "Feedback intake: agents and people,
//! one inbox", its section "One feedback record").
//!
//! Three ways in, as telemetry's: a daemon joined to the hosted control
//! signs its send (the daemon signature, `auth.rs` on control), a signed-in
//! account sends from control's page with its session, and anything else
//! sends unsigned under its random install id with a [`FeedbackStamp`]. Who
//! a signed send is from comes from its signature or session, never from a
//! field in it, so [`FeedbackWho`] holds only what an anonymous send has.
//!
//! What's in a send is what its writer typed ([`FeedbackSubmit`]'s kind,
//! title and body), what they said about being contacted, and a little
//! context: versions, the platform, where they were, and the last Arugula
//! tool call that failed. Nothing else is ever attached: no terminal
//! contents, no file names or paths, no prompts, no code. A failed call's
//! error text is ours and is cut to [`MAX_ERROR`].
//!
//! The title, the body, the agent's name and model and a failed call's
//! error are free text from outside: untrusted. Whatever reads them (a
//! person's inbox, an agent triaging, an issue made from one) shows them
//! fenced as quoted data, never as instructions or markup.
//!
//! A closed set's value an older or newer side doesn't know reads as
//! `Other`, never as an error, so either side can add one.
//!
//! ## The proof of work
//!
//! An anonymous send carries a [`FeedbackStamp`], so that sending many
//! costs the sender something (#1215 is control's guard). It is a nonce
//! for the send's [`FeedbackSubmit::digest`], the SHA-256 of
//! `arugula-feedback-v1`, then the install id, the title and the body, each
//! as its length (8 bytes, little-endian) and its bytes. The stamp holds if
//! SHA-256 of that digest and the nonce (8 bytes, little-endian) starts
//! with at least `difficulty` zero bits, and `difficulty` is at least what
//! control asks. [`FeedbackStamp::solve`] finds one; [`FeedbackStamp::holds`]
//! checks one. Both sides call these, so they can't disagree.
//!
//! The digest covers only the install id, the title and the body, not the
//! whole request: a field a newer side adds can't change it, and a stamp
//! can't be moved to other words. A sender starts at [`POW_DIFFICULTY`];
//! control refuses a send whose stamp is too easy with [`POW_BAD`] and the
//! difficulty it wants ([`FeedbackRefused`]), which a sender solves again
//! for, up to [`MAX_POW_DIFFICULTY`].

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::telemetry::{Arch, Os};

/// A send (`POST`), [`FeedbackSubmit`], answered [`FeedbackAck`] or
/// refused with [`FeedbackRefused`]. Signed by a daemon joined to this
/// control, with a signed-in account's session, or neither (anonymous,
/// with a stamp).
pub const FEEDBACK: &str = "/api/feedback";

/// The longest title, in characters.
pub const MAX_TITLE: usize = 200;
/// The longest body, in bytes (8 KB).
pub const MAX_BODY: usize = 8 * 1024;
/// The longest failed call's error text, in bytes. Longer is cut, not
/// refused ([`FailedCall::new`]).
pub const MAX_ERROR: usize = 1024;
/// The longest of the short fields: an agent's name, version and model, a
/// version, a route, a tool and its kind.
pub const MAX_SHORT: usize = 200;

/// The reasons control refuses a send, in [`FeedbackRefused::error`]. The
/// daemon and the web client turn each into words of their own.
///
/// Too many sends from this account, machine or install (429): try later.
pub const RATE_LIMITED: &str = "feedback_rate_limited";
/// Control takes no anonymous sends now (403): sign in, or join the
/// machine.
pub const ANONYMOUS_OFF: &str = "feedback_anonymous_off";
/// A title, body or field over its limit (413).
pub const TOO_LARGE: &str = "feedback_too_large";
/// An anonymous send without a stamp (403).
pub const POW_MISSING: &str = "feedback_pow_missing";
/// An anonymous send whose stamp doesn't hold, or is easier than control
/// asks (403). [`FeedbackRefused::difficulty`] says what it asks.
pub const POW_BAD: &str = "feedback_pow_bad";

/// The difficulty a sender solves for first.
pub const POW_DIFFICULTY: u8 = 20;
/// The most a sender solves for: control asking more is refused by the
/// sender rather than worked at.
pub const MAX_POW_DIFFICULTY: u8 = 28;

/// The start of a feedback id: `fbk_…`.
pub const ID_PREFIX: &str = "fbk_";

/// What someone sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FeedbackSubmit {
    pub kind: FeedbackKind,
    /// At most [`MAX_TITLE`] characters, not empty. Untrusted.
    pub title: String,
    /// At most [`MAX_BODY`] bytes; may be empty. Untrusted.
    #[serde(default)]
    pub body: String,
    /// Whether the sender may be contacted about it: a signed-in account
    /// by its email, a joined machine by its owner's.
    #[serde(default)]
    pub contact_ok: bool,
    /// Who wrote it.
    pub source: FeedbackSource,
    /// Where it was sent from.
    pub surface: FeedbackSurface,
    /// The agent that wrote it, when an agent did and says.
    #[serde(default)]
    pub agent: Option<FeedbackAgent>,
    #[serde(default)]
    pub context: FeedbackContext,
    #[serde(default)]
    pub who: FeedbackWho,
}

/// What it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(transform = crate::open_enum))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum FeedbackKind {
    /// Something doesn't work.
    Bug,
    /// Something isn't there.
    Missing,
    Idea,
    Praise,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(transform = crate::open_enum))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum FeedbackSource {
    /// An agent, through the MCP tool or the CLI.
    Agent,
    Person,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(transform = crate::open_enum))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum FeedbackSurface {
    Mcp,
    Cli,
    Web,
    Desktop,
    #[serde(other)]
    Other,
}

/// The agent that wrote it, as it says: each at most [`MAX_SHORT`].
/// Untrusted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FeedbackAgent {
    /// `claude-code`, `codex`.
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

/// Where the sender was. The daemon fills it in for the CLI and MCP, the
/// web client for itself; each field is left out when not known.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FeedbackContext {
    /// Arugula's version: the daemon's, or the web client's.
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub os: Option<Os>,
    #[serde(default)]
    pub arch: Option<Arch>,
    /// How far through getting started the sender is: a step's name
    /// (letters, digits, `-` and `_`).
    #[serde(default)]
    pub onboarding: Option<String>,
    /// The web client's route, without its query or fragment.
    #[serde(default)]
    pub route: Option<String>,
    /// The last Arugula tool call that failed for the sender, if one did.
    #[serde(default)]
    pub failed: Option<FailedCall>,
}

/// An Arugula tool call that failed: which, and what it said. Never its
/// arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FailedCall {
    /// The tool (`forge_read`) or CLI command (`arugula run`).
    pub tool: String,
    /// Its kind, for a tool that has kinds (`issue`).
    #[serde(default)]
    pub kind: Option<String>,
    /// Its error, at most [`MAX_ERROR`] bytes. Untrusted.
    pub error: String,
}

/// An anonymous send's: an install id and its stamp. Empty on a signed
/// send, whose signature or session says who it's from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FeedbackWho {
    /// Random, 16 lowercase hex digits, the install's own: telemetry's when
    /// it has one.
    #[serde(default)]
    pub install: Option<String>,
    #[serde(default)]
    pub stamp: Option<FeedbackStamp>,
}

/// The proof of work for an anonymous send (the module's doc says how).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FeedbackStamp {
    /// The leading zero bits it was solved for.
    pub difficulty: u8,
    pub nonce: u64,
}

/// A send taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FeedbackAck {
    /// `fbk_…` ([`ID_PREFIX`]).
    pub id: String,
    /// What people are shown for it: `F-` and a short form (`F-7K2Q9X`).
    pub shown: String,
}

/// A send refused (the status each reason has is beside it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FeedbackRefused {
    /// [`RATE_LIMITED`], [`ANONYMOUS_OFF`], [`TOO_LARGE`], [`POW_MISSING`]
    /// or [`POW_BAD`]; another is a reason this side doesn't know yet.
    pub error: String,
    /// The difficulty control asks, with [`POW_MISSING`] and [`POW_BAD`].
    #[serde(default)]
    pub difficulty: Option<u8>,
    /// When to try again (seconds), with [`RATE_LIMITED`].
    #[serde(default)]
    pub retry_after: Option<u64>,
}

impl FailedCall {
    /// A failed call, its error cut to [`MAX_ERROR`] bytes.
    pub fn new(tool: impl Into<String>, kind: Option<String>, error: &str) -> Self {
        let mut end = error.len().min(MAX_ERROR);
        while !error.is_char_boundary(end) {
            end -= 1;
        }
        Self { tool: tool.into(), kind, error: error[..end].to_owned() }
    }
}

impl FeedbackSubmit {
    /// Whether control takes it: within the limits ([`TOO_LARGE`] if not),
    /// a title, and a well-formed install id. Whether it's signed or
    /// stamped is control's to check.
    pub fn check(&self) -> Result<(), &'static str> {
        let short = |s: &str| s.len() <= MAX_SHORT;
        let short_opt = |s: &Option<String>| s.as_deref().is_none_or(short);
        if self.title.trim().is_empty() {
            return Err("title: say what it's about");
        }
        if self.title.chars().count() > MAX_TITLE || self.body.len() > MAX_BODY {
            return Err(TOO_LARGE);
        }
        if let Some(a) = &self.agent
            && !(short(&a.name) && short_opt(&a.version) && short_opt(&a.model))
        {
            return Err(TOO_LARGE);
        }
        let c = &self.context;
        if !(short_opt(&c.version) && short_opt(&c.onboarding) && short_opt(&c.route)) {
            return Err(TOO_LARGE);
        }
        if let Some(f) = &c.failed
            && !(short(&f.tool) && short_opt(&f.kind) && f.error.len() <= MAX_ERROR)
        {
            return Err(TOO_LARGE);
        }
        if let Some(o) = &c.onboarding
            && !o.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("onboarding: a step's name");
        }
        if let Some(i) = &self.who.install
            && !(i.len() == 16 && i.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        {
            return Err("install: 16 lowercase hex digits");
        }
        Ok(())
    }

    /// What its stamp is for: the module's doc says how it's made.
    pub fn digest(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"arugula-feedback-v1");
        for part in [self.who.install.as_deref().unwrap_or(""), &self.title, &self.body] {
            h.update((part.len() as u64).to_le_bytes());
            h.update(part.as_bytes());
        }
        h.finalize().into()
    }
}

impl FeedbackStamp {
    /// The first stamp for `digest` at `difficulty`, or `None` above
    /// [`MAX_POW_DIFFICULTY`]. Some 2^difficulty hashes on average.
    pub fn solve(digest: &[u8; 32], difficulty: u8) -> Option<Self> {
        if difficulty > MAX_POW_DIFFICULTY {
            return None;
        }
        (0..=u64::MAX).map(|nonce| Self { difficulty, nonce }).find(|s| zeros(digest, s.nonce) >= u32::from(difficulty))
    }

    /// Whether it holds for `digest` and is at least as hard as `asked`.
    pub fn holds(&self, digest: &[u8; 32], asked: u8) -> bool {
        self.difficulty >= asked && zeros(digest, self.nonce) >= u32::from(self.difficulty)
    }
}

/// The leading zero bits of SHA-256 of `digest` and `nonce`.
fn zeros(digest: &[u8; 32], nonce: u64) -> u32 {
    let mut h = Sha256::new();
    h.update(digest);
    h.update(nonce.to_le_bytes());
    let out = h.finalize();
    let mut n = 0;
    for b in out {
        n += b.leading_zeros();
        if b != 0 {
            break;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn send() -> FeedbackSubmit {
        serde_json::from_value(json!({
            "kind": "bug", "title": "Panes flicker", "body": "When I resize.", "contact_ok": true,
            "source": "agent", "surface": "mcp",
            "agent": { "name": "claude-code", "version": "2.1.0", "model": null },
            "context": { "version": "0.70.49", "os": "macos", "arch": "aarch64", "onboarding": "first-pane",
                         "route": null,
                         "failed": { "tool": "forge_read", "kind": "issue", "error": "not found" } },
            "who": { "install": "0123456789abcdef", "stamp": null },
        }))
        .unwrap()
    }

    #[test]
    fn unknown_values_read_as_other() {
        let f: FeedbackSubmit = serde_json::from_value(json!({
            "kind": "complaint", "title": "x", "source": "robot", "surface": "phone",
        }))
        .unwrap();
        assert_eq!((f.kind, f.source, f.surface), (FeedbackKind::Other, FeedbackSource::Other, FeedbackSurface::Other));
        assert_eq!(f.context, FeedbackContext::default());
        assert!(f.check().is_ok());
    }

    #[test]
    fn checks() {
        let mut f = send();
        assert!(f.check().is_ok());
        f.title = "é".repeat(MAX_TITLE);
        assert!(f.check().is_ok(), "the title's limit is in characters");
        f.title.push('é');
        assert_eq!(f.check(), Err(TOO_LARGE));
        f.title = " ".into();
        assert!(f.check().is_err());
        f.title = "x".into();
        f.body = "x".repeat(MAX_BODY + 1);
        assert_eq!(f.check(), Err(TOO_LARGE));
        f.body.clear();
        f.context.onboarding = Some("../etc".into());
        assert!(f.check().is_err());
        f.context.onboarding = None;
        f.who.install = Some("0123456789ABCDEF".into());
        assert!(f.check().is_err());
    }

    #[test]
    fn a_failed_calls_error_is_cut_on_a_character() {
        let long = format!("x{}", "é".repeat(MAX_ERROR));
        let c = FailedCall::new("run", None, &long);
        assert!(c.error.len() <= MAX_ERROR && long.starts_with(&c.error));
        assert_eq!(FailedCall::new("run", None, "short").error, "short");
    }

    #[test]
    fn a_stamp_holds_for_its_words_only() {
        let mut f = send();
        let s = FeedbackStamp::solve(&f.digest(), 12).unwrap();
        assert!(s.holds(&f.digest(), 12));
        assert!(s.holds(&f.digest(), 8), "harder than asked is fine");
        assert!(!s.holds(&f.digest(), 13), "easier than asked isn't");
        // Fields beside the words don't change what it's for.
        let digest = f.digest();
        f.kind = FeedbackKind::Idea;
        f.contact_ok = false;
        f.who.stamp = Some(s);
        assert_eq!(f.digest(), digest);
        // Other words, or another install, are another digest.
        f.body.push('!');
        assert_ne!(f.digest(), digest);
        f.body.pop();
        f.who.install = Some("fedcba9876543210".into());
        assert_ne!(f.digest(), digest);
        assert_eq!(FeedbackStamp::solve(&digest, MAX_POW_DIFFICULTY + 1), None);
    }

    const PINNED: u64 = 66;

    #[test]
    fn a_stamp_is_the_same_on_both_sides() {
        // Pinned, so a change to how it's made shows up here.
        let s = FeedbackStamp::solve(&send().digest(), 8).unwrap();
        assert_eq!(s, FeedbackStamp { difficulty: 8, nonce: PINNED });
        assert!(s.holds(&send().digest(), 8));
    }
}
