//! What a tool answers with (#448) where more than MCP's server names it:
//! one type per result, serialized to the JSON agents see. `tools.rs` adds the
//! `summary` sentence (`done`). The rest, which only the server's own tools
//! answer with, are in `arugulad-mcp`'s `results` (#232).
//!
//! They live here, not in proto, because only agents read them: no Rust or
//! TypeScript client deserializes them, so they derive `Serialize`, and
//! `JsonSchema` where a tool lists them as its `outputSchema` (#580: the
//! tools that always answer with one of them; see `tools::output_schema`).
//! The JSON of each is what the old `json!` literal wrote: a field is skipped
//! only where the old code left the key out, and an absent value is `null`,
//! as before. Each is tested against that literal.

use arugula_proto::{Attention, BlockType, PaneId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A closed pane, and what became of the worktree its block owned.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ClosedPane {
    pub pane: PaneId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<arugula_proto::api::WorktreeOutcome>,
}

/// The last command in a [`PaneEntry`].
#[derive(Debug, Serialize)]
pub struct LastCommand {
    pub command: Option<String>,
    pub exit: Option<i32>,
}

/// A pane or block in `list` (and in the `arugula://block/N` resource).
#[derive(Debug, Serialize)]
pub struct PaneEntry {
    pub pane: PaneId,
    #[serde(rename = "type")]
    pub kind: BlockType,
    pub session: String,
    pub tab: u32,
    pub tab_name: Option<String>,
    pub machine: Option<String>,
    pub cwd: Option<String>,
    pub command: Option<String>,
    pub title: Option<String>,
    pub running: bool,
    pub attention: Attention,
    pub why: Option<String>,
    pub current: Option<String>,
    pub last: Option<LastCommand>,
    pub started_by: Option<String>,
    /// Set on the caller's own pane only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub you: Option<bool>,
}

/// `list` kind panes.
#[derive(Debug, Serialize)]
pub struct Panes {
    pub panes: Vec<PaneEntry>,
}

/// A studio app opened in a block.
#[cfg(feature = "labs")]
#[derive(Debug, Serialize)]
pub struct AppOpened {
    pub block: PaneId,
    pub app: String,
}

/// A forge, and whether this machine has the user's login for it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ForgeSignIn {
    /// `github`, `forgejo` or `gitlab`.
    pub forge: String,
    pub host: String,
    pub signed_in: bool,
    /// The login's user, where its CLI says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// What signs in, or what's missing.
    pub note: String,
}

/// The projects in control whose runs agents here reach (M216): through
/// grants this machine's owner gave it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RunsReach {
    /// This machine is joined to control and holds a live run grant
    /// (runs.read or runs.start) over a project its owner reads.
    pub live: bool,
    /// What its grants reach.
    pub projects: Vec<RunsProject>,
    /// What to use, or why not.
    pub note: String,
}

/// A project a machine's run grants reach.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RunsProject {
    pub id: String,
    pub name: String,
    /// runs.read, runs.start.
    pub powers: Vec<String>,
}

/// The projects in control whose connected Plane projects agents here may
/// write to (#359): through `trackers.write` grants this machine's owner gave it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TrackersReach {
    /// This machine is joined to control and holds a live `trackers.write`
    /// grant over a project its owner reads.
    pub live: bool,
    /// What its grants reach.
    pub projects: Vec<TrackerReachProject>,
    /// What to use, or why not.
    pub note: String,
}

/// A project a machine's `trackers.write` reaches.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TrackerReachProject {
    pub id: String,
    pub name: String,
}

/// The projects in control whose boards agents here reach: through grants
/// this machine's owner gave it.
#[derive(Debug, Default)]
pub struct BoardsReach {
    /// What its grants reach.
    pub projects: Vec<BoardsProject>,
    /// Why not, when control couldn't say.
    pub note: String,
}

/// A project a machine's board grants reach.
#[derive(Debug, Deserialize)]
pub struct BoardsProject {
    pub id: String,
    pub name: String,
    /// boards.read, boards.write.
    pub powers: Vec<String>,
    /// How many boards it has.
    #[serde(default)]
    pub boards: u64,
}

/// The conversations in control that agents here reach (M198): through
/// grants this machine's owner gave it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ThreadsReach {
    /// This machine is joined to control and holds a live thread grant
    /// (threads.read or threads.post) over something its owner reads.
    pub live: bool,
    /// What its grants reach.
    pub scopes: Vec<ThreadScope>,
    /// What to use, or why not.
    pub note: String,
}

/// A scope a machine's agent grants reach.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ThreadScope {
    /// `project` (its channels, issues and their threads) or `account`
    /// (the owner's DMs).
    pub kind: String,
    pub id: String,
    pub name: String,
    /// threads.read, threads.post, issues.write.
    pub powers: Vec<String>,
}

/// The owner's Cloud workspaces in control, as agents here reach them
/// (#1094).
#[derive(Debug, Serialize, JsonSchema)]
pub struct CloudReach {
    /// This machine is joined to control, and control has Cloud workspaces
    /// for its owner.
    pub reachable: bool,
    /// What to use, or why not.
    pub note: String,
}

/// An invite drafted on the user's card.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InviteDrafted {
    pub draft: String,
    pub status: &'static str,
    pub block: PaneId,
    pub who: String,
    pub name: String,
    /// `viewer` or `editor`.
    #[schemars(with = "String")]
    pub role: arugula_core::Role,
    pub session: arugula_core::SessionId,
    pub pane: PaneId,
}

/// `read_invite`: the draft as its card has it (the block's state while it's
/// open, the kept card once it was closed), with its `draft` id and `block`
/// set over it.
#[derive(Debug, Serialize)]
pub struct InviteRead<D: Serialize> {
    #[serde(flatten)]
    pub card: D,
    pub draft: String,
    pub block: PaneId,
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn same(got: impl Serialize, want: Value) {
        assert_eq!(serde_json::to_value(got).unwrap(), want);
    }

    /// The same, and the JSON fits the `outputSchema` of the tool that
    /// answers with it, `summary` added as `done` does (#580): a client may
    /// check `structuredContent` against the schema.
    fn shaped<T: Serialize + JsonSchema + 'static>(got: T, want: Value) {
        let mut sent = serde_json::to_value(&got).unwrap();
        assert_eq!(sent, want);
        sent["summary"] = Value::String("said".into());
        let schema = Value::Object((*super::super::tools::output_schema::<T>()).clone());
        if let Err(e) = arugula_testkit::schema::conforms(&sent, &schema) {
            panic!("{sent} doesn't fit its schema: {e}\n{schema:#}");
        }
    }

    #[test]
    fn a_closed_pane_and_an_app() {
        same(ClosedPane { pane: 7, worktree: None }, json!({ "pane": 7 }));
        #[cfg(feature = "labs")]
        same(AppOpened { block: 9, app: "notes".into() }, json!({ "block": 9, "app": "notes" }));
    }

    fn entry() -> PaneEntry {
        PaneEntry {
            pane: 3,
            kind: BlockType::Terminal,
            session: "main".into(),
            tab: 1,
            tab_name: None,
            machine: None,
            cwd: None,
            command: None,
            title: None,
            running: false,
            attention: Attention::Done,
            why: None,
            current: None,
            last: None,
            started_by: None,
            you: None,
        }
    }

    #[test]
    fn a_pane_in_list_leaves_out_you_unless_it_is_the_callers() {
        // Every key is there, null when empty, but `you`.
        same(
            Panes { panes: vec![entry()] },
            json!({ "panes": [{
                "pane": 3, "type": BlockType::Terminal, "session": "main", "tab": 1, "tab_name": null,
                "machine": null, "cwd": null, "command": null, "title": null, "running": false,
                "attention": Attention::Done, "why": null, "current": null, "last": null, "started_by": null,
            }] }),
        );
        let mine = PaneEntry {
            tab_name: Some("work".into()),
            machine: Some("m2".into()),
            last: Some(LastCommand { command: Some("ls".into()), exit: Some(0) }),
            you: Some(true),
            ..entry()
        };
        let v = serde_json::to_value(&mine).unwrap();
        assert_eq!(v["you"], true);
        assert_eq!(v["machine"], "m2");
        assert_eq!(v["last"], json!({ "command": "ls", "exit": 0 }));
    }

    #[test]
    fn invite_results() {
        shaped(
            InviteDrafted {
                draft: "d1".into(),
                status: "waiting",
                block: 6,
                who: "person".into(),
                name: "Pat".into(),
                role: arugula_core::Role::Viewer,
                session: 1,
                pane: 2,
            },
            json!({ "draft": "d1", "status": "waiting", "block": 6, "who": "person", "name": "Pat",
                "role": arugula_core::Role::Viewer, "session": 1, "pane": 2 }),
        );
        // The card's own `draft` and `block` keys, if it had any, lose.
        same(
            InviteRead {
                card: json!({ "id": "d1", "status": "sent", "block": 1, "draft": "x" }),
                draft: "d1".into(),
                block: 6,
            },
            json!({ "id": "d1", "status": "sent", "block": 6, "draft": "d1" }),
        );
    }
}
