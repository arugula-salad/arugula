//! A place's layout (Arugula #1496; the design lift's §3.12): the tabs and
//! panes a person and their agents arranged on a place, one record per
//! person per place, kept on the platform so it follows them across
//! machines. A place is a board (`board:brd_…`) or a project's Overview
//! (`overview:prj_…`).
//!
//! - **Tabs and panes.** The first tab, [`HOME_TAB`], is the place's own:
//!   its default panes (a board's lane groups, the Overview's Boards and
//!   Running) are the client's to draw, and the record keeps only what
//!   differs from them: the panes opened beside them, the defaults closed
//!   ([`PlaceLayout::closed_defaults`]) and sized. Every other tab was
//!   opened by someone.
//! - **Who owns what.** An agent only rearranges a pane an agent opened and
//!   the person hasn't touched (moved, resized, maximised). At most
//!   [`AGENT_PANES`] untouched agent panes a tab: a fourth closes the oldest
//!   ([`LayoutChanged::closed`]). The limit binds agents, not the person. A
//!   default pane the person closed stays closed until Reset; an agent may
//!   show a pane in place of an open one for a while
//!   ([`Pane::in_place_of`]), and closing that pane puts it back.
//! - **Beside or for you.** [`Intent::Beside`] puts a pane in the home tab,
//!   part of the conversation now. [`Intent::ForYou`] opens a tab for
//!   something waiting on the person (a prompt, a Doc to review, a PR to
//!   approve), and the Inbox gets a row pointing at it
//!   ([`crate::projects::InboxSubject::tab`]). The record has no current
//!   tab: which tab is showing is the page's, so nothing an agent does
//!   switches the person there.
//! - **Changed.** While the home tab differs from the default,
//!   [`PlaceLayout::changed`] says who changed it last and when (the
//!   header's "changed by … · Reset"); Reset puts the defaults back and
//!   sends what was opened there to [`PlaceLayout::closed`].
//! - **Live.** Each change is [`crate::chat::ChatLive::Layout`], to that
//!   person only.
//! - **Offline.** A pane whose source is on a machine ([`Pane::machine`])
//!   keeps the state it was last shown with; a read marks it
//!   [`Pane::offline`] while the machine isn't connected.
//! - **A prompt** is a blocked agent's pending permission or question on a
//!   machine (`ref` its pane, `%N`), answered in place from the machine.
//!   Once that pane no longer waits, its prompt pane closes, and its tab
//!   with it when that was all the tab held.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `GET`: the reader's layout of `place` ([`PlaceLayout`]; a place they
/// never changed answers its default, `rev` 0). `POST` [`LayoutChange`]:
/// change it, answered [`LayoutChanged`].
pub const LAYOUT: &str = "/api/layouts/{place}";
/// The same, for a machine's agent, on its owner's layout. A change needs
/// the machine's `boards.write` grant over the place's project.
pub const AGENT_LAYOUT: &str = "/api/daemon/layouts/{place}";

/// [`AGENT_LAYOUT`] for `place`.
pub fn agent_layout(place: &str) -> String {
    format!("/api/daemon/layouts/{place}")
}

/// The place's own tab, always first.
pub const HOME_TAB: &str = "home";
/// The most untouched agent panes a tab holds.
pub const AGENT_PANES: usize = 3;
/// The most tabs a layout holds, the home tab with them.
pub const TABS_MAX: usize = 8;
/// The most panes a tab holds.
pub const PANES_MAX: usize = 12;
/// The most panes [`PlaceLayout::closed`] keeps.
pub const CLOSED_MAX: usize = 20;
/// The longest title or state line, in characters.
pub const TITLE_MAX: usize = 200;
/// The longest `ref`, in characters.
pub const REF_MAX: usize = 400;

/// What a place is, as its path names it: `board:brd_…` or
/// `overview:prj_…`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    Board(String),
    Overview(String),
}

impl Place {
    /// `board:brd_…` or `overview:prj_…`; `None` for anything else.
    pub fn parse(s: &str) -> Option<Place> {
        let (kind, id) = s.split_once(':')?;
        let ok = |prefix: &str| {
            id.strip_prefix(prefix).is_some_and(|r| {
                !r.is_empty() && r.len() <= 60 && r.bytes().all(|b| b.is_ascii_alphanumeric())
            })
        };
        match kind {
            "board" if ok("brd_") => Some(Place::Board(id.into())),
            "overview" if ok("prj_") => Some(Place::Overview(id.into())),
            _ => None,
        }
    }
}

impl std::fmt::Display for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Place::Board(id) => write!(f, "board:{id}"),
            Place::Overview(id) => write!(f, "overview:{id}"),
        }
    }
}

/// What a pane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "LayoutPaneKind"))]
#[cfg_attr(feature = "schema", schemars(transform = crate::open_enum))]
#[serde(rename_all = "snake_case")]
pub enum PaneKind {
    /// A Doc (`ref` its id).
    Doc,
    /// A pull request (`ref` `owner/repo#N`).
    Pr,
    /// A preview of something served (`ref` its URL or port).
    Preview,
    /// A run (`ref` `run_…`).
    Run,
    /// A terminal or agent pane on a machine (`ref` `%N`).
    Terminal,
    /// An issue's card (`ref` `iss_…`).
    Card,
    /// A blocked agent's permission or question (`ref` its pane, `%N`).
    Prompt,
    /// A kind this side doesn't know yet.
    #[serde(other)]
    Other,
}

/// Where a shown pane goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "LayoutIntent"))]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    /// Beside the place's own panes: part of the conversation now.
    #[default]
    Beside,
    /// A tab of its own, waiting on the person, with an Inbox row pointing
    /// at it. Never switched to.
    ForYou,
}

/// Who opened or changed something: a person, or (with `machine`) an agent
/// on that machine acting for them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "LayoutBy"))]
pub struct By {
    pub account: String,
    /// The agent's machine; `None` for the person.
    #[serde(default)]
    pub machine: Option<String>,
}

impl By {
    pub fn is_agent(&self) -> bool {
        self.machine.is_some()
    }
}

/// One pane of a tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "LayoutPane"))]
pub struct Pane {
    /// `pan_…`.
    pub id: String,
    pub kind: PaneKind,
    /// What it shows, by its kind ([`PaneKind`]).
    #[serde(rename = "ref")]
    #[cfg_attr(feature = "ts", ts(rename = "ref"))]
    pub target: String,
    pub title: String,
    /// The machine its source is on (a terminal, a preview, a prompt).
    #[serde(default)]
    pub machine: Option<String>,
    /// Its last state, one line, as it was last shown: what a page shows
    /// while `machine` is offline.
    #[serde(default)]
    pub state: Option<String>,
    pub opened_by: By,
    /// When it was opened (ms).
    pub opened_at: u64,
    /// The person moved, resized or maximised it: it's theirs now, and no
    /// agent moves or closes it.
    #[serde(default)]
    pub touched: bool,
    /// The default pane it stands in place of (its name), for a while.
    #[serde(default)]
    pub in_place_of: Option<String>,
    /// Its share of the tab's width, in thousandths; `None` for an even
    /// share.
    #[serde(default)]
    pub size: Option<u32>,
    /// Answered by a read, never kept: `machine` isn't connected, so this
    /// is its last state.
    #[serde(default)]
    pub offline: bool,
}

/// One tab of a place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "LayoutTab"))]
pub struct Tab {
    /// [`HOME_TAB`], or `tab_…`.
    pub id: String,
    /// Empty for the home tab, which is the place's.
    pub title: String,
    /// Who opened it; the person for the home tab.
    pub opened_by: By,
    pub opened_at: u64,
    /// It waits on the person ([`Intent::ForYou`]): the Inbox has a row
    /// pointing at it until it closes.
    #[serde(default)]
    pub for_you: bool,
    /// Its opened panes, in order. The home tab's defaults aren't here.
    pub panes: Vec<Pane>,
}

/// Who changed the home tab last, and when (ms).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "LayoutChangedBy"))]
pub struct Changed {
    pub by: By,
    pub at: u64,
}

/// A person's layout of a place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PlaceLayout {
    pub place: String,
    /// Counts changes: a page drops a [`crate::chat::ChatLive::Layout`] at
    /// or below the rev it has.
    pub rev: u64,
    /// The home tab first.
    pub tabs: Vec<Tab>,
    /// The home tab's default panes the person closed, by name, until
    /// Reset.
    #[serde(default)]
    pub closed_defaults: Vec<String>,
    /// The home tab's default panes the person sized, by name, in
    /// thousandths.
    #[serde(default)]
    pub default_sizes: BTreeMap<String, u32>,
    /// What was closed, newest first, at most [`CLOSED_MAX`]: the closed
    /// list, which Reopen takes from.
    #[serde(default)]
    pub closed: Vec<Pane>,
    /// While the home tab differs from the default: who changed it last.
    #[serde(default)]
    pub changed: Option<Changed>,
}

/// A pane to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "LayoutNewPane"))]
pub struct NewPane {
    pub kind: PaneKind,
    #[serde(rename = "ref")]
    #[cfg_attr(feature = "ts", ts(rename = "ref"))]
    pub target: String,
    pub title: String,
    #[serde(default)]
    pub machine: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

/// `POST` [`LAYOUT`] (or [`AGENT_LAYOUT`]): one change. An agent may only
/// show, and close or move a pane an agent opened that the person hasn't
/// touched; the rest is the person's (403).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum LayoutChange {
    /// Show a pane: beside the place's panes, or a tab for the person. A
    /// pane of the same kind and `ref` that's open already is shown again
    /// with the new title and state (and, for `for_you`, moved to a tab of
    /// its own if an agent's and untouched). `tab`: the person's choice of
    /// tab (`+ pane`); `in_place_of`: a default pane's name.
    Show {
        pane: NewPane,
        #[serde(default)]
        intent: Intent,
        #[serde(default)]
        tab: Option<String>,
        #[serde(default)]
        in_place_of: Option<String>,
    },
    /// Move a pane to `tab` (`None`: a new tab of its own) at `index`
    /// (`None`: last).
    Move {
        pane: String,
        #[serde(default)]
        tab: Option<String>,
        #[serde(default)]
        index: Option<u32>,
    },
    /// Size a pane, or a default pane by name, in thousandths.
    Resize { pane: String, size: u32 },
    /// The person maximised a pane: it's theirs now.
    Maximise { pane: String },
    /// Close a pane, or a default pane by name.
    Close { pane: String },
    /// Close a tab and its panes; never the home tab.
    CloseTab { tab: String },
    /// Open a pane from the closed list again, where it was.
    Reopen { pane: String },
    /// Put the home tab back to its default: its closed defaults open,
    /// their sizes even, and what was opened there to the closed list.
    Reset {},
}

/// What a change answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LayoutChanged {
    pub layout: PlaceLayout,
    /// The pane shown or reopened, and the tab it's in.
    #[serde(default)]
    pub pane: Option<String>,
    #[serde(default)]
    pub tab: Option<String>,
    /// Panes the change closed to make room: the oldest untouched agent
    /// pane past [`AGENT_PANES`].
    #[serde(default)]
    pub closed: Vec<Pane>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_is_a_board_or_an_overview() {
        assert_eq!(Place::parse("board:brd_abc1"), Some(Place::Board("brd_abc1".into())));
        assert_eq!(Place::parse("overview:prj_x9"), Some(Place::Overview("prj_x9".into())));
        assert_eq!(Place::parse("board:brd_abc1").unwrap().to_string(), "board:brd_abc1");
        for bad in ["board:prj_x", "board:brd_", "overview:brd_x", "desk:brd_x", "brd_x", "board:brd_a/b"] {
            assert_eq!(Place::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_change_reads_as_its_op() {
        let c: LayoutChange = serde_json::from_value(serde_json::json!({
            "op": "show", "pane": { "kind": "prompt", "ref": "%3", "title": "git clone", "machine": "dmn_1" },
            "intent": "for_you",
        }))
        .unwrap();
        let LayoutChange::Show { pane, intent, tab, in_place_of } = c else { panic!("{c:?}") };
        assert_eq!((pane.kind, intent, tab, in_place_of), (PaneKind::Prompt, Intent::ForYou, None, None));
        let c: LayoutChange = serde_json::from_value(serde_json::json!({ "op": "reset" })).unwrap();
        assert_eq!(c, LayoutChange::Reset {});
        let k: PaneKind = serde_json::from_value(serde_json::json!("hologram")).unwrap();
        assert_eq!(k, PaneKind::Other);
    }
}
