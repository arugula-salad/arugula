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
