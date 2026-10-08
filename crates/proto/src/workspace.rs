//! A workspace block's records (M34), as the web client sees them: what
//! chant's `records --current` says of each, kept field by field so a
//! decision can be drawn where it's asked about (#616). The daemon
//! (`workspace/model.rs`) builds them; the web client draws them with the
//! TypeScript made from [`Record`] (`WorkspaceRecord`, since TypeScript has
//! a `Record` of its own).

use serde::{Deserialize, Serialize};

/// One current record of a kind the declaration names: a decision, a work
/// item, a session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "WorkspaceRecord"))]
pub struct Record {
    /// The kind's name (`decision`, `work`).
    pub kind: String,
    pub id: String,
    pub title: Option<String>,
    pub state: Option<String>,
    /// A work item: whether nothing blocks it.
    pub ready: Option<bool>,
    pub blocked_by: Vec<String>,
    /// PinState drift and the like.
    pub warnings: Vec<String>,
    pub valid: bool,
    /// What a decision answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub question: Option<String>,
    /// What it constrains, as the record writes it: `member:<name>`,
    /// `path:<path>`.
    #[serde(default)]
    pub constrains: Vec<String>,
    /// The option chosen, and why. None while proposed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub choice: Option<RecordChoice>,
    /// The options turned down, each with why.
    #[serde(default)]
    pub rejected: Vec<RecordChoice>,
    /// The records this one replaces, and the one that replaced it (chant
    /// derives that from the other's link).
    #[serde(default)]
    pub supersedes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub superseded_by: Option<String>,
    /// Records that fix this one's consequences without replacing it.
    #[serde(default)]
    pub remediated_by: Vec<String>,
    /// A work item: the decisions it carries out.
    #[serde(default)]
    pub implements: Vec<String>,
    /// Who decided, and when (a date); who proposed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub decided_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub decided_on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub proposed_by: Option<String>,
    /// What it cites: links, and workspace files pinned by hash.
    #[serde(default)]
    pub evidence: Vec<RecordEvidence>,
    /// Whether its author's seal verifies: none when nothing here can say
    /// (no seal and no signers file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub attested: Option<bool>,
    /// How far the commit behind it is trusted: `attested`,
    /// `attested-unverifiable-here`, `adopted`, `unattested`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub provenance: Option<String>,
    /// The record's file, from the repository root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub path: Option<String>,
}

/// An option chosen or turned down.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RecordChoice {
    pub option: String,
    /// The option's label, from the record's options.
    pub label: Option<String>,
    /// Why it was chosen, or turned down.
    pub why: Option<String>,
}

/// One entry of a record's evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RecordEvidence {
    pub title: Option<String>,
    /// A link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub url: Option<String>,
    /// A workspace file pinned by the hash of its bytes, and how it stands
    /// now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub pin: Option<PinState>,
}

/// A pinned file against the tree read (chant's asset `state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PinState {
    /// The hashes match.
    Pinned,
    /// The file changed since it was pinned.
    Drifted,
    /// The file isn't there.
    Missing,
    /// It matches, and a record this one supersedes pinned the same bytes.
    Stale,
}

impl PinState {
    pub fn parse(s: &str) -> Option<PinState> {
        Some(match s {
            "pinned" => PinState::Pinned,
            "drifted" => PinState::Drifted,
            "missing" => PinState::Missing,
            "stale" => PinState::Stale,
            _ => return None,
        })
    }
}

/// A decision covering a region, as chant's intent read ranks it
/// (`graph --intent <dir>`'s `why.decisions`, #617): chant does the
/// `member:` and `path:` covering and follows supersession.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DecisionRef {
    pub id: String,
    pub title: Option<String>,
    pub state: Option<String>,
    /// How it covers the region: `carried`, `path`, `contract`, `issue`,
    /// `member`, or `related` (only through supersession or a commit).
    pub relevance: String,
    /// Not superseded.
    pub current: bool,
    /// In a state its kind closes, such as ratified.
    pub closed: bool,
}

impl DecisionRef {
    /// Whether it governs the region itself: current, decided (not
    /// proposed or withdrawn), and covering it by more than a link.
    pub fn governs(&self) -> bool {
        self.current && self.relevance != "related" && !matches!(self.state.as_deref(), Some("proposed" | "withdrawn"))
    }
}

/// What someone about to approve a gate wants to know (#617): the
/// decisions it enforces, the plan it binds and the member's last
/// release.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GateWhy {
    /// The decisions covering the gate's member, most relevant first. None
    /// until chant has been asked.
    pub decisions: Option<Vec<DecisionRef>>,
    /// Why they couldn't be read.
    pub note: Option<String>,
    /// The plan the gate was reached for (`status`'s `planDigest`).
    pub plan_digest: Option<String>,
    /// The member's latest release in the env watched.
    pub last_release: Option<GateRelease>,
}

/// A release, from `status`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GateRelease {
    pub component: String,
    /// When (RFC 3339), who and from which commit.
    pub at: String,
    pub actor: Option<String>,
    pub git_sha: Option<String>,
}

/// A work lease, from `status`'s `leases` (#618): who holds a work item.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Lease {
    /// The work item's id.
    pub item: String,
    pub holder: String,
    /// `active`, or `expired` (it can be claimed again).
    pub state: String,
    pub expires_at: Option<String>,
    /// The member whose ledger holds it; none for the workspace's own.
    pub member: Option<String>,
    /// The fencing token, which a run under the lease names.
    pub token: Option<String>,
    /// The Arugula agent pane holding it, when a run of one names its token
    /// (or, running, its agent session is the holder).
    pub pane: Option<u32>,
}

/// An agent run, from `chant workspace runs` (#618). `WorkspaceRun` in
/// TypeScript, which has a `RunRef` already.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "WorkspaceRun"))]
pub struct RunRef {
    /// The `Chant-Run` trailer's value.
    pub id: String,
    /// `running` or `ended`.
    pub state: Option<String>,
    /// The agent session it ran as, and who it worked for.
    pub agent: Option<String>,
    pub by: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    /// How it ended: done, not_done, failed, cancelled.
    pub outcome: Option<String>,
    /// The work item it worked on.
    pub unit: Option<String>,
    /// The lease token it worked under.
    pub lease: Option<String>,
    /// `<kind>/<id>` of each decision it carried out.
    pub decisions: Vec<String>,
    /// The Arugula agent pane that ran it (an Arugula run's id names its
    /// block: `arugula-<pane>-<ms>`). The client links it while that pane
    /// is open.
    pub pane: Option<u32>,
}

impl RunRef {
    /// The pane an Arugula run's id names (`arugula-<pane>-<ms>`).
    pub fn pane_of(id: &str) -> Option<u32> {
        let mut it = id.strip_prefix("arugula-")?.split('-');
        let pane = it.next()?.parse().ok()?;
        it.next()?.parse::<u64>().ok()?;
        it.next().is_none().then_some(pane)
    }
}

/// Why a member is the way it is, and who works on it (#618): read when
/// its card is opened, kept until the fingerprint moves.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MemberWhy {
    /// The decisions covering it, most relevant first (the gate card's
    /// read). None while reading, or when it couldn't be read.
    pub decisions: Option<Vec<DecisionRef>>,
    /// Commits that changed it while no decision constrained it.
    pub undecided: Option<u64>,
    /// Why the decisions couldn't be read.
    pub note: Option<String>,
    /// Its recent agent runs, newest first: those of the agent sessions the
    /// declaration binds to it, and those that made commits in it.
    pub runs: Vec<RunRef>,
    /// Why the runs couldn't be read.
    pub runs_note: Option<String>,
    /// How long chant's intent read took.
    pub ms: Option<u64>,
}
