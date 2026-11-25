//! Ladder starts (aru-143): a chief of staff or a board's lead starts an
//! agent on one of its person's machines, for a board, under the board's
//! standing grant.
//!
//! The calling daemon asks the hub for a [`StartTicket`]
//! ([`start_ticket_path`], [`StartTicketAsk`]): control checks the board's
//! grant (its machines, its cap, its lapse, a revoke) and that the target is
//! online, and signs [`StartClaims`] with its issuer keys, as it signs a
//! machine token, but with `use: "start"` so neither passes for the other.
//! The caller sends the ticket with a [`LadderStart`] to the target's
//! [`START_PATH`] over the peer channel it already has (an `agents` machine
//! token). The target checks the ticket itself (the hub's signature, that
//! it's for this machine, of its own account, from the channel's caller,
//! unused), and that the start matches it, then opens the agent block and
//! answers [`LadderStarted`].
//!
//! A lead stops an issue agent on another machine with a [`LadderStop`] to
//! that machine's [`STOP_PATH`], over the same channel: no ticket, since a
//! stop only frees what a start took. The target closes only a block a
//! ladder start opened there, and only for the machine that started it or
//! the board's lead, then tells control at once so the grant's slot is free
//! for the next start.

use serde::{Deserialize, Serialize};

/// The `use` claim every start ticket carries: never
/// [`machine_token::USE`](crate::machine_token::USE), so a ticket can't open
/// a channel and a machine token can't start an agent.
pub const START_USE: &str = "start";

/// How long the hub makes a start ticket last, in seconds: one start, now.
pub const START_TICKET_SECS: u64 = 60;

/// The target's route for a ladder start, under `/api/a2a/` so an `agents`
/// machine token reaches it.
pub const START_PATH: &str = "/api/a2a/start";

/// The target's route for stopping an agent a ladder start opened there.
pub const STOP_PATH: &str = "/api/a2a/stop";

/// The reason of a 409 that refuses a ticket because the target is offline:
/// the sentence names it, and the grant's other machines that are online.
pub const OFFLINE: &str = "offline";

/// `POST` here, signed as a daemon, with a [`StartTicketAsk`], for a ticket
/// to start an agent for board `board`: answered [`StartTicket`].
pub fn start_ticket_path(board: &str) -> String {
    format!("/api/daemon/boards/{board}/start-ticket")
}

/// Which rung of the ladder the agent is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Rung {
    /// The board's lead: it takes the board's lead, so the board's thread
    /// reaches it.
    Lead,
    /// An issue agent: it's attached to its issue's row and holds it, so the
    /// issue's thread reaches it.
    Issue,
}

impl Rung {
    pub fn as_str(self) -> &'static str {
        match self {
            Rung::Lead => "lead",
            Rung::Issue => "issue",
        }
    }
}

/// What a daemon asks the hub a start ticket for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StartTicketAsk {
    /// The target machine's id: the asking machine's own, or another of its
    /// account's.
    pub to: String,
    pub rung: Rung,
    /// The row's issue, as the caller writes it (`#N`, `owner/repo#N`,
    /// `IDENT-N` or an `iss_`/`fi_` id), for [`Rung::Issue`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
    /// The recipe the agent wears; none for plain Claude Code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe: Option<String>,
}

/// The hub's answer to a [`StartTicketAsk`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StartTicket {
    pub ticket: String,
    /// When it runs out, in seconds since the epoch.
    pub expires: u64,
}

/// A start ticket's claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StartClaims {
    /// The hub's public URL.
    pub iss: String,
    /// The person's account: the target refuses another account's (across
    /// accounts it's a task and its card, aru-050).
    pub sub: String,
    /// The target machine's id.
    pub aud: String,
    /// Always [`START_USE`].
    #[serde(rename = "use")]
    pub use_: String,
    /// The calling machine's id: the target refuses it from a channel whose
    /// caller is another machine.
    pub from: String,
    /// The board, and its project: where the block is attached.
    pub board: String,
    pub project: String,
    /// The board's grant the start counts against; none for a lead the
    /// person's own approval starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant: Option<String>,
    pub rung: Rung,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe: Option<String>,
    pub iat: u64,
    pub exp: u64,
    /// Unique to this ticket: the target takes it once, and names it when it
    /// attaches the block, so control counts the start.
    pub jti: String,
}

/// Who started a ladder agent: the calling session, on its machine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StartedBy {
    /// The calling machine's name, filled in by the target from the
    /// channel's caller (what the caller sends here is ignored).
    #[serde(default)]
    pub machine: String,
    /// The calling session's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// `POST` [`START_PATH`]: start an agent here under a ticket. The board, the
/// rung, the issue and the recipe must be the ticket's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LadderStart {
    pub ticket: String,
    pub board: String,
    pub rung: Rung,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe: Option<String>,
    /// The repository it works in (`owner/name`): the target finds its own
    /// clone and opens the agent in a new worktree there. None: the target's
    /// home.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// What to ask it.
    pub prompt: String,
    /// A name for its session there; default the rung and the issue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default)]
    pub by: StartedBy,
}

/// The target's answer to a [`LadderStart`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LadderStarted {
    /// The target's name and id.
    pub machine: String,
    pub id: String,
    /// The agent block, there.
    pub block: u32,
    /// Its session there.
    pub session: String,
    /// Where it works, there.
    pub cwd: String,
    /// What the target says of its row or its lead: the boards it's on, or
    /// why it isn't.
    pub said: String,
}

/// `POST` [`STOP_PATH`]: close an agent block a ladder start opened on the
/// target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LadderStop {
    /// The agent block, there ([`LadderStarted::block`]).
    pub block: u32,
}

/// The target's answer to a [`LadderStop`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LadderStopped {
    /// The target's name and id.
    pub machine: String,
    pub id: String,
    pub block: u32,
    /// What became of its worktree, and whether control was told it closed
    /// (its grant's slot is free once it was).
    pub said: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ticket_is_never_a_machine_token() {
        assert_ne!(START_USE, crate::machine_token::USE);
        assert_ne!(START_USE, crate::machine_token::BLOCK_USE);
        assert!(START_PATH.starts_with("/api/a2a/"), "an agents token reaches it");
        assert!(STOP_PATH.starts_with("/api/a2a/"), "an agents token reaches it");
        assert_eq!(start_ticket_path("brd_1"), "/api/daemon/boards/brd_1/start-ticket");
    }

    #[test]
    fn claims_spell_use_and_rung() {
        let c = StartClaims {
            iss: "https://control.arugula.io".into(),
            sub: "acct".into(),
            aud: "m2".into(),
            use_: START_USE.into(),
            from: "m1".into(),
            board: "brd_1".into(),
            project: "prj_1".into(),
            grant: None,
            rung: Rung::Lead,
            issue: None,
            recipe: None,
            iat: 1,
            exp: 61,
            jti: "j".into(),
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["use"], "start");
        assert_eq!(v["rung"], "lead");
        assert!(v.get("grant").is_none() && v.get("issue").is_none());
        assert_eq!(serde_json::from_value::<StartClaims>(v).unwrap(), c);
    }
}
