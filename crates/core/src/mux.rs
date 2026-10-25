//! Sessions, tabs and split trees, and the intents that change them.
//!
//! This is pure state. Applying an intent returns [`Effect`]s (start or stop
//! a pane's process) for the daemon to carry out; sizes come from
//! [`Mux::pane_rects`].

use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::{
    ClientId, NodeId, PaneId, SessionId, TabId,
    layout::{self, Layout, Rect},
    tree::{Edge, Node, normalize_weights},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub tabs: Vec<TabId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub id: TabId,
    pub name: Option<String>,
    pub root: Node,
    /// Size of the whole tab in cells, set by its size owner.
    pub cols: u16,
    pub rows: u16,
    /// The client whose window size the tab has (the last to claim it).
    pub owner: Option<ClientId>,
    /// While set, this pane fills the tab and the others keep their sizes
    /// (a phone showing one pane at a time).
    pub zoom: Option<PaneId>,
    /// While set, this pane fills the tab for everyone (desktop focus mode,
    /// [`Intent::Focus`]). Unlike `zoom`, which follows one client's view,
    /// it's shared, saved and survives a reload; it never touches `root`,
    /// so unfocusing gives back the same layout.
    #[serde(default)]
    pub focus: Option<PaneId>,
    /// The client presenting this tab (#1472): while set, only its window
    /// sets the tab's size. Never saved: client ids start over with the
    /// daemon, so a saved one could pin the tab to a stranger.
    #[serde(skip)]
    pub presenter: Option<ClientId>,
}

/// A whole-tab arrangement for [`Intent::Arrange`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Preset {
    /// One row of equal columns.
    Columns,
    /// ⌈√n⌉ columns, filled row by row; the last row's panes widen to fill
    /// it.
    Grid,
    /// `main` on the left (0.6), the rest stacked on the right.
    MainStack,
}

/// Where an option lives, as in tmux: the server, a session, a window (tab)
/// or a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum OptionScope {
    Global,
    Session(SessionId),
    Tab(TabId),
    Pane(PaneId),
}

pub type OptionMap = BTreeMap<String, String>;

/// Opaque named strings that clients keep with the layout (tmux's `@user`
/// options: iTerm2's tab grouping and attach guard, `@affinities`). Saved
/// with the layout; an entry goes when what it belongs to does. Only the
/// tmux front end set them, and it was removed (DECISIONS.md "The TUI and
/// tmux -CC are removed"); they stay in the protocol and the saved layout so
/// a layout and an older client still load, and nothing reads them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Options {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub global: OptionMap,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "by_id")]
    #[cfg_attr(feature = "ts", ts(as = "Vec<(SessionId, OptionMap)>"))]
    pub sessions: BTreeMap<SessionId, OptionMap>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "by_id")]
    #[cfg_attr(feature = "ts", ts(as = "Vec<(TabId, OptionMap)>"))]
    pub tabs: BTreeMap<TabId, OptionMap>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "by_id")]
    #[cfg_attr(feature = "ts", ts(as = "Vec<(PaneId, OptionMap)>"))]
    pub panes: BTreeMap<PaneId, OptionMap>,
}

/// A map keyed by id as `[[id, value], ...]`. JSON object keys are strings,
/// and serde can't turn them back into numbers inside an internally tagged
/// enum (`ServerMsg::State` carries these).
mod by_id {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer, V: Serialize>(m: &BTreeMap<u32, V>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(m.iter())
    }

    pub fn deserialize<'de, D: Deserializer<'de>, V: Deserialize<'de>>(d: D) -> Result<BTreeMap<u32, V>, D::Error> {
        Ok(Vec::<(u32, V)>::deserialize(d)?.into_iter().collect())
    }
}

impl Options {
    pub fn get(&self, scope: OptionScope) -> Option<&OptionMap> {
        match scope {
            OptionScope::Global => Some(&self.global),
            OptionScope::Session(s) => self.sessions.get(&s),
            OptionScope::Tab(t) => self.tabs.get(&t),
            OptionScope::Pane(p) => self.panes.get(&p),
        }
    }

    fn map_mut(&mut self, scope: OptionScope) -> &mut OptionMap {
        match scope {
            OptionScope::Global => &mut self.global,
            OptionScope::Session(s) => self.sessions.entry(s).or_default(),
            OptionScope::Tab(t) => self.tabs.entry(t).or_default(),
            OptionScope::Pane(p) => self.panes.entry(p).or_default(),
        }
    }
}

pub const DEFAULT_COLS: u16 = 80;
pub const DEFAULT_ROWS: u16 = 24;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Intent {
    /// A session with one tab and one pane. `from_pane` lends its working
    /// directory to the new pane.
    NewSession {
        name: Option<String>,
        from_pane: Option<PaneId>,
    },
    RenameSession {
        session: SessionId,
        name: String,
    },
    CloseSession {
        session: SessionId,
    },
    /// `cwd`, when given, is the new pane's directory instead of
    /// `from_pane`'s.
    NewTab {
        session: SessionId,
        from_pane: Option<PaneId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        cwd: Option<String>,
    },
    RenameTab {
        tab: TabId,
        name: Option<String>,
    },
    CloseTab {
        tab: TabId,
    },
    /// Reorder a tab, or move it to another session.
    MoveTab {
        tab: TabId,
        session: SessionId,
        index: usize,
    },
    /// A new pane beside `pane` on the given side. `local`: on this host
    /// even in a tab that has a machine (the daemon's business; the layout
    /// doesn't care). `cwd`: the new pane's directory [default: `pane`'s].
    Split {
        pane: PaneId,
        edge: Edge,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        local: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        cwd: Option<String>,
    },
    ClosePane {
        pane: PaneId,
    },
    /// Dock `pane` at an edge of `target` (or swap with it, for Center).
    MovePane {
        pane: PaneId,
        target: PaneId,
        edge: Edge,
    },
    /// Take `pane` out into a tab of its own.
    BreakPane {
        pane: PaneId,
        session: SessionId,
        index: Option<usize>,
    },
    /// Dock a whole tab's layout at an edge of `target`.
    DockTab {
        tab: TabId,
        target: PaneId,
        edge: Edge,
    },
    ResizeSplit {
        split: NodeId,
        weights: Vec<f64>,
    },
    /// Rebuild a tab's tree from its panes, in reading order, as `preset`.
    /// No pane is started or stopped. `main` is the big pane of
    /// [`Preset::MainStack`] [default: the first]; it must be in the tab.
    Arrange {
        tab: TabId,
        preset: Preset,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        main: Option<PaneId>,
    },
    /// Show one pane filling the tab, for everyone, or (`pane: None`) the
    /// whole tab again. See [`Tab::focus`].
    Focus {
        tab: TabId,
        pane: Option<PaneId>,
    },
    /// Set an option, or unset it (`value: None`). No client sends it
    /// since the tmux front end was removed; kept in the protocol (see
    /// [`Options`]).
    SetOption {
        scope: OptionScope,
        name: String,
        value: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Start a pane's process, in `cwd` if given, else in `cwd_from`'s
    /// directory.
    Spawn {
        pane: PaneId,
        cwd_from: Option<PaneId>,
        cwd: Option<String>,
    },
    Kill {
        pane: PaneId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    NoSession(SessionId),
    NoTab(TabId),
    NoPane(PaneId),
    NoSplit(NodeId),
    Invalid(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoSession(s) => write!(f, "no session ${s}"),
            Error::NoTab(t) => write!(f, "no tab @{t}"),
            Error::NoPane(p) => write!(f, "no pane %{p}"),
            Error::NoSplit(n) => write!(f, "no split {n}"),
            Error::Invalid(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Error {}

/// The whole multiplexer. IDs are never reused.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Mux {
    pub sessions: Vec<Session>,
    pub tabs: BTreeMap<TabId, Tab>,
    /// Bumped on every change.
    pub rev: u64,
    #[serde(default)]
    pub options: Options,
    next_session: SessionId,
    next_tab: TabId,
    next_pane: PaneId,
    next_node: NodeId,
}

impl Mux {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn session(&self, id: SessionId) -> Result<&Session, Error> {
        self.sessions.iter().find(|s| s.id == id).ok_or(Error::NoSession(id))
    }

    fn session_mut(&mut self, id: SessionId) -> Result<&mut Session, Error> {
        self.sessions.iter_mut().find(|s| s.id == id).ok_or(Error::NoSession(id))
    }

    pub fn tab(&self, id: TabId) -> Result<&Tab, Error> {
        self.tabs.get(&id).ok_or(Error::NoTab(id))
    }

    pub fn tab_of(&self, pane: PaneId) -> Result<TabId, Error> {
        self.tabs.values().find(|t| t.root.contains(pane)).map(|t| t.id).ok_or(Error::NoPane(pane))
    }

    pub fn session_of_tab(&self, tab: TabId) -> Result<SessionId, Error> {
        self.sessions.iter().find(|s| s.tabs.contains(&tab)).map(|s| s.id).ok_or(Error::NoTab(tab))
    }

    pub fn panes(&self) -> Vec<PaneId> {
        self.tabs.values().flat_map(|t| t.root.panes()).collect()
    }

    fn new_pane(&mut self) -> PaneId {
        self.next_pane += 1;
        self.next_pane
    }

    /// An id from the panes' space for something outside the layout (M28:
    /// an editor that joined the swarm), never reused.
    pub fn reserve_pane(&mut self) -> PaneId {
        self.new_pane()
    }

    fn node_ids(&mut self) -> impl FnMut() -> NodeId + '_ {
        || {
            self.next_node += 1;
            self.next_node
        }
    }

    /// A new tab holding `root`, inserted into `session` at `index`.
    fn add_tab(&mut self, session: SessionId, root: Node, index: Option<usize>) -> Result<TabId, Error> {
        self.session(session)?;
        self.next_tab += 1;
        let id = self.next_tab;
        self.tabs.insert(
            id,
            Tab {
                id,
                name: None,
                root,
                cols: DEFAULT_COLS,
                rows: DEFAULT_ROWS,
                owner: None,
                zoom: None,
                focus: None,
                presenter: None,
            },
        );
        let s = self.session_mut(session)?;
        let at = index.unwrap_or(s.tabs.len()).min(s.tabs.len());
        s.tabs.insert(at, id);
        Ok(id)
    }

    /// Drop a tab whose panes are gone or moved; drop its session if empty.
    fn remove_tab(&mut self, tab: TabId) {
        self.tabs.remove(&tab);
        for s in &mut self.sessions {
            s.tabs.retain(|t| *t != tab);
        }
        self.sessions.retain(|s| !s.tabs.is_empty());
    }

    /// Take a pane out of its tree, removing the tab if it was the last.
    fn detach_pane(&mut self, pane: PaneId) -> Result<(), Error> {
        let tab_id = self.tab_of(pane)?;
        let tab = self.tabs.get_mut(&tab_id).unwrap();
        let root = std::mem::replace(&mut tab.root, Node::pane(0));
        match root.remove(pane) {
            Some(rest) => {
                tab.root = rest;
                if tab.zoom == Some(pane) {
                    tab.zoom = None;
                }
                if tab.focus == Some(pane) {
                    tab.focus = None;
                }
            }
            None => self.remove_tab(tab_id),
        }
        Ok(())
    }

    pub fn apply(&mut self, intent: Intent) -> Result<Vec<Effect>, Error> {
        let effects = self.apply_inner(intent)?;
        self.fit_tabs();
        self.prune_options();
        self.rev += 1;
        debug_assert_eq!(self.validate(), Ok(()));
        Ok(effects)
    }

    fn apply_inner(&mut self, intent: Intent) -> Result<Vec<Effect>, Error> {
        use Intent::*;
        match intent {
            NewSession { name, from_pane } => {
                self.next_session += 1;
                let id = self.next_session;
                self.sessions.push(Session { id, name: name.unwrap_or_else(|| id.to_string()), tabs: vec![] });
                let pane = self.new_pane();
                self.add_tab(id, Node::pane(pane), None)?;
                Ok(vec![Effect::Spawn { pane, cwd_from: from_pane, cwd: None }])
            }
            RenameSession { session, name } => {
                self.session_mut(session)?.name = name;
                Ok(vec![])
            }
            CloseSession { session } => {
                let tabs = self.session(session)?.tabs.clone();
                let mut effects = vec![];
                for t in tabs {
                    effects.extend(self.tab(t)?.root.panes().into_iter().map(|pane| Effect::Kill { pane }));
                    self.remove_tab(t);
                }
                Ok(effects)
            }
            NewTab { session, from_pane, cwd } => {
                let pane = self.new_pane();
                self.add_tab(session, Node::pane(pane), None)?;
                Ok(vec![Effect::Spawn { pane, cwd_from: from_pane, cwd }])
            }
            RenameTab { tab, name } => {
                self.tabs.get_mut(&tab).ok_or(Error::NoTab(tab))?.name = name.filter(|n| !n.trim().is_empty());
                Ok(vec![])
            }
            CloseTab { tab } => {
                let panes = self.tab(tab)?.root.panes();
                self.remove_tab(tab);
                Ok(panes.into_iter().map(|pane| Effect::Kill { pane }).collect())
            }
            MoveTab { tab, session, index } => {
                self.tab(tab)?;
                self.session(session)?;
                for s in &mut self.sessions {
                    s.tabs.retain(|t| *t != tab);
                }
                let s = self.session_mut(session)?;
                let at = index.min(s.tabs.len());
                s.tabs.insert(at, tab);
                self.sessions.retain(|s| !s.tabs.is_empty());
                Ok(vec![])
            }
            Split { pane, edge, cwd, .. } => {
                if edge == Edge::Center {
                    return Err(Error::Invalid("split needs a side"));
                }
                let tab_id = self.tab_of(pane)?;
                let new = self.new_pane();
                let mut root = std::mem::replace(&mut self.tabs.get_mut(&tab_id).unwrap().root, Node::pane(0));
                root.insert(pane, edge, Node::pane(new), &mut self.node_ids());
                let tab = self.tabs.get_mut(&tab_id).unwrap();
                tab.root = root;
                tab.zoom = None;
                tab.focus = None;
                Ok(vec![Effect::Spawn { pane: new, cwd_from: Some(pane), cwd }])
            }
            ClosePane { pane } => {
                self.detach_pane(pane)?;
                Ok(vec![Effect::Kill { pane }])
            }
            MovePane { pane, target, edge } => {
                if pane == target {
                    return Err(Error::Invalid("a pane can't move onto itself"));
                }
                let from = self.tab_of(pane)?;
                let to = self.tab_of(target)?;
                if edge == Edge::Center {
                    // Swap, possibly across tabs.
                    const TEMP: PaneId = PaneId::MAX;
                    let tabs = &mut self.tabs;
                    tabs.get_mut(&from).unwrap().root.replace_pane(pane, TEMP);
                    tabs.get_mut(&to).unwrap().root.replace_pane(target, pane);
                    tabs.get_mut(&from).unwrap().root.replace_pane(TEMP, target);
                    // Across tabs, a zoom or focus on either pane would
                    // now point into the other tab.
                    for t in [from, to] {
                        let t = tabs.get_mut(&t).unwrap();
                        t.zoom = t.zoom.filter(|z| t.root.contains(*z));
                        t.focus = None;
                    }
                    return Ok(vec![]);
                }
                self.detach_pane(pane)?;
                let to = self.tab_of(target)?;
                let mut root = std::mem::replace(&mut self.tabs.get_mut(&to).unwrap().root, Node::pane(0));
                root.insert(target, edge, Node::pane(pane), &mut self.node_ids());
                let tab = self.tabs.get_mut(&to).unwrap();
                tab.root = root;
                tab.zoom = None;
                tab.focus = None;
                Ok(vec![])
            }
            BreakPane { pane, session, index } => {
                self.session(session)?;
                let from = self.tab_of(pane)?;
                if self.tab(from)?.root == Node::pane(pane) && self.session_of_tab(from)? == session {
                    return Err(Error::Invalid("pane already has its own tab"));
                }
                self.detach_pane(pane)?;
                // Detaching can remove the session if it held only this pane.
                let session = if self.session(session).is_ok() {
                    session
                } else {
                    return Err(Error::Invalid("target session went away"));
                };
                self.add_tab(session, Node::pane(pane), index)?;
                Ok(vec![])
            }
            DockTab { tab, target, edge } => {
                if edge == Edge::Center {
                    return Err(Error::Invalid("docking needs a side"));
                }
                let to = self.tab_of(target)?;
                if to == tab {
                    return Err(Error::Invalid("a tab can't dock into itself"));
                }
                let root = self.tab(tab)?.root.clone();
                // Removing the tab can't remove the target's session: the
                // target's tab is still in it.
                self.remove_tab(tab);
                let mut dest = std::mem::replace(&mut self.tabs.get_mut(&to).unwrap().root, Node::pane(0));
                dest.insert(target, edge, root, &mut self.node_ids());
                let t = self.tabs.get_mut(&to).unwrap();
                t.root = dest;
                t.zoom = None;
                t.focus = None;
                Ok(vec![])
            }
            ResizeSplit { split, weights } => {
                let tab = self.tabs.values_mut().find(|t| t.root.has_split(split)).ok_or(Error::NoSplit(split))?;
                let (_, children) = tab.root.find_split_mut(split).unwrap();
                if weights.len() != children.len() {
                    return Err(Error::Invalid("one weight per child"));
                }
                for (c, w) in children.iter_mut().zip(weights) {
                    c.weight = w;
                }
                normalize_weights(children);
                Ok(vec![])
            }
            Arrange { tab, preset, main } => {
                let t = self.tab(tab)?;
                let panes = t.root.panes();
                if let Some(m) = main
                    && !panes.contains(&m)
                {
                    return Err(Error::NoPane(m));
                }
                let (cols, rows) = (t.cols, t.rows);
                let root = arrange(&panes, preset, main, &mut self.node_ids());
                let (min_cols, min_rows) = root.min_size();
                if min_cols > cols || min_rows > rows {
                    return Err(Error::Invalid("the tab is too small for that layout"));
                }
                let t = self.tabs.get_mut(&tab).unwrap();
                t.root = root;
                // Every pane is still there, so a focus could stay; but
                // then the new arrangement wouldn't show until unfocus, and
                // asking for one means wanting to see it. So it goes, as it
                // does on a Split. A phone's zoom is its own view and stays.
                t.focus = None;
                Ok(vec![])
            }
            Focus { tab, pane } => {
                let t = self.tabs.get_mut(&tab).ok_or(Error::NoTab(tab))?;
                if let Some(p) = pane
                    && !t.root.contains(p)
                {
                    return Err(Error::NoPane(p));
                }
                t.focus = pane;
                Ok(vec![])
            }
            SetOption { scope, name, value } => {
                match scope {
                    OptionScope::Global => {}
                    OptionScope::Session(s) => _ = self.session(s)?,
                    OptionScope::Tab(t) => _ = self.tab(t)?,
                    OptionScope::Pane(p) => _ = self.tab_of(p)?,
                }
                let map = self.options.map_mut(scope);
                match value {
                    Some(v) => _ = map.insert(name, v),
                    None => _ = map.remove(&name),
                }
                Ok(vec![])
            }
        }
    }

    /// Grow any tab smaller than its tree's minimum size.
    fn fit_tabs(&mut self) {
        for t in self.tabs.values_mut() {
            let (cols, rows) = t.root.min_size();
            t.cols = t.cols.max(cols);
            t.rows = t.rows.max(rows);
        }
    }

    /// Drop options whose session, tab or pane has gone (and empty maps).
    fn prune_options(&mut self) {
        let panes: std::collections::HashSet<PaneId> = self.panes().into_iter().collect();
        let sessions: Vec<SessionId> = self.sessions.iter().map(|s| s.id).collect();
        let o = &mut self.options;
        o.sessions.retain(|s, m| sessions.contains(s) && !m.is_empty());
        o.tabs.retain(|t, m| self.tabs.contains_key(t) && !m.is_empty());
        o.panes.retain(|p, m| panes.contains(p) && !m.is_empty());
    }

    /// A client shows `tab` in a `cols`×`rows` cell area. With `claim`, its
    /// size becomes the tab's; otherwise it only does if the client already
    /// owns the tab or nobody does. Returns whether anything changed.
    pub fn view(
        &mut self,
        client: ClientId,
        tab: TabId,
        cols: u16,
        rows: u16,
        zoom: Option<PaneId>,
        claim: bool,
    ) -> Result<bool, Error> {
        let t = self.tabs.get_mut(&tab).ok_or(Error::NoTab(tab))?;
        // Whatever the claim says, a presenter's size holds (#1472).
        if t.presenter.is_some_and(|p| p != client) {
            return Ok(false);
        }
        if !(claim || t.owner.is_none() || t.owner == Some(client)) {
            return Ok(false);
        }
        let zoom = zoom.filter(|p| t.root.contains(*p) && t.root.panes().len() > 1);
        // Never smaller than the tree: a client too small for it sees a
        // bigger tab, as tmux does.
        let (min_cols, min_rows) = t.root.min_size();
        let next = (cols.max(2).max(min_cols), rows.max(1).max(min_rows), Some(client), zoom);
        if (t.cols, t.rows, t.owner, t.zoom) == next {
            return Ok(false);
        }
        (t.cols, t.rows, t.owner, t.zoom) = next;
        self.rev += 1;
        Ok(true)
    }

    /// `client` starts presenting `tab` (or takes over from the one who
    /// does), or, with `on` false, stops it. Starting claims the size at
    /// once. Stopping is for the presenter; who else may is the caller's
    /// call. Returns whether anything changed (#1472).
    pub fn present(
        &mut self,
        client: ClientId,
        tab: TabId,
        on: bool,
        (cols, rows): (u16, u16),
        zoom: Option<PaneId>,
    ) -> Result<bool, Error> {
        let t = self.tabs.get_mut(&tab).ok_or(Error::NoTab(tab))?;
        if !on {
            if t.presenter.is_none() {
                return Ok(false);
            }
            t.presenter = None;
            self.rev += 1;
            return Ok(true);
        }
        let was = t.presenter.take();
        // The old presenter no longer pins, so the claim is allowed.
        let sized = self.view(client, tab, cols, rows, zoom, true)?;
        self.tabs.get_mut(&tab).ok_or(Error::NoTab(tab))?.presenter = Some(client);
        if !sized && was != Some(client) {
            self.rev += 1;
        }
        Ok(sized || was != Some(client))
    }

    /// A client went away: its tabs keep their size until someone else
    /// views them, and it stops presenting.
    pub fn release(&mut self, client: ClientId) -> bool {
        let mut changed = false;
        for t in self.tabs.values_mut().filter(|t| t.presenter == Some(client)) {
            t.presenter = None;
            changed = true;
        }
        for t in self.tabs.values_mut().filter(|t| t.owner == Some(client)) {
            t.owner = None;
            t.zoom = None;
            changed = true;
        }
        if changed {
            self.rev += 1;
        }
        changed
    }

    pub fn layout(&self, tab: TabId) -> Result<Layout, Error> {
        let t = self.tab(tab)?;
        Ok(match t.focus.or(t.zoom) {
            Some(p) => Layout { panes: vec![(p, Rect { x: 0, y: 0, cols: t.cols, rows: t.rows })], splits: vec![] },
            None => layout::layout(&t.root, t.cols, t.rows),
        })
    }

    /// The size every visible pane should have. Panes hidden behind a focus
    /// or a zoom are left out, so they keep their last size.
    pub fn pane_rects(&self) -> BTreeMap<PaneId, Rect> {
        self.tabs.keys().flat_map(|t| self.layout(*t).unwrap().panes).collect()
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for t in self.tabs.values() {
            t.root.validate().map_err(|e| format!("tab {}: {e}", t.id))?;
            for p in t.root.panes() {
                if !seen.insert(p) {
                    return Err(format!("pane {p} is in two tabs"));
                }
            }
            if t.zoom.is_some_and(|z| !t.root.contains(z)) {
                return Err(format!("tab {} zooms a pane it doesn't have", t.id));
            }
            if t.focus.is_some_and(|f| !t.root.contains(f)) {
                return Err(format!("tab {} focuses a pane it doesn't have", t.id));
            }
            let homes = self.sessions.iter().filter(|s| s.tabs.contains(&t.id)).count();
            if homes != 1 {
                return Err(format!("tab {} is in {homes} sessions", t.id));
            }
        }
        for s in &self.sessions {
            if s.tabs.is_empty() {
                return Err(format!("session {} has no tabs", s.id));
            }
            if let Some(t) = s.tabs.iter().find(|t| !self.tabs.contains_key(t)) {
                return Err(format!("session {} lists missing tab {t}", s.id));
            }
        }
        Ok(())
    }
}

/// `panes` (in reading order) laid out as `preset`, normalized.
fn arrange(panes: &[PaneId], preset: Preset, main: Option<PaneId>, next_id: &mut impl FnMut() -> NodeId) -> Node {
    use crate::tree::{Child, Dir};
    let mut split = |dir, nodes: Vec<(f64, Node)>| -> Node {
        if nodes.len() == 1 {
            return nodes.into_iter().next().unwrap().1;
        }
        let children = nodes.into_iter().map(|(weight, node)| Child { weight, node }).collect();
        Node::Split { id: next_id(), dir, children }
    };
    let even = |ps: &[PaneId]| ps.iter().map(|p| (1.0, Node::pane(*p))).collect::<Vec<_>>();
    let mut root = match preset {
        Preset::Columns => split(Dir::Row, even(panes)),
        Preset::Grid => {
            let c = (panes.len() as f64).sqrt().ceil().max(1.0) as usize;
            let rows: Vec<(f64, Node)> = panes.chunks(c).map(|r| (1.0, split(Dir::Row, even(r)))).collect();
            split(Dir::Column, rows)
        }
        Preset::MainStack => {
            let main = main.or(panes.first().copied()).expect("a tab has a pane");
            let rest: Vec<PaneId> = panes.iter().copied().filter(|p| *p != main).collect();
            match rest.len() {
                0 => Node::pane(main),
                1 => split(Dir::Row, even(&[main, rest[0]])),
                _ => {
                    let stack = split(Dir::Column, even(&rest));
                    split(Dir::Row, vec![(0.6, Node::pane(main)), (0.4, stack)])
                }
            }
        }
    };
    root.normalize();
    root
}

/// How long a tab's size owner keeps the size against another client's
/// typing after it last typed there or took the size (#333).
pub const SIZE_HOLD: Duration = Duration::from_secs(3);

/// Why a client sends its size for a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// It only shows the tab: its size counts if it owns the tab or nobody
    /// does.
    No,
    /// It typed in the tab. It takes the size unless the owner typed there
    /// or took the size within [`SIZE_HOLD`]; until then it types into the
    /// owner's size, letterboxed or scaled.
    Typed,
    /// It opened or switched to the tab, or asked for it ("use this size").
    Yes,
}

/// Size arbitration between clients (DECISIONS.md, "Size arbitration"):
/// when each tab's size owner last did something there. Kept beside the
/// [`Mux`], not in it, as it's only for now: nothing to save or send.
#[derive(Debug, Default)]
pub struct SizeHold {
    active: HashMap<TabId, Instant>,
}

impl SizeHold {
    /// [`Mux::view`], with typing claims held off while the owner is busy.
    #[allow(clippy::too_many_arguments)]
    pub fn view(
        &mut self,
        mux: &mut Mux,
        client: ClientId,
        tab: TabId,
        (cols, rows): (u16, u16),
        zoom: Option<PaneId>,
        claim: Claim,
        now: Instant,
    ) -> Result<bool, Error> {
        let owner = mux.tab(tab)?.owner;
        let take = match claim {
            Claim::No => false,
            Claim::Yes => true,
            Claim::Typed => {
                owner.is_none_or(|o| o == client)
                    || self.active.get(&tab).is_none_or(|at| now.saturating_duration_since(*at) >= SIZE_HOLD)
            }
        };
        let changed = mux.view(client, tab, cols, rows, zoom, take)?;
        if take {
            self.active.insert(tab, now);
        }
        if self.active.len() > mux.tabs.len() {
            self.active.retain(|t, _| mux.tabs.contains_key(t));
        }
        Ok(changed)
    }

    /// `client` typed in `pane`: if its tab's size is theirs, they keep it
    /// a while longer.
    pub fn typed(&mut self, mux: &Mux, client: ClientId, pane: PaneId, now: Instant) {
        if let Ok(tab) = mux.tab_of(pane)
            && mux.tab(tab).is_ok_and(|t| t.owner == Some(client))
        {
            self.active.insert(tab, now);
        }
    }
}
