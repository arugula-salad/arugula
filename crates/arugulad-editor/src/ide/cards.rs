//! The edits Claude Code proposes through its IDE connections, as the cards
//! that make a terminal wait (#699): what the relay tells the mux task, the
//! diff for each card, and the answers Claude Code reads. The mux holds
//! these as its `PaneCards` and lends them to each call on its task, so
//! what they do comes in the order everything else there does.

use std::{any::Any, collections::HashMap};

use arugula_proto::{Action, DiffInfo, Driver, PaneId, Reason, ReasonKind, ask::Answered};
use serde_json::{Value, json};
use tracing::info;

use super::{Event, Ide, NAME, diff, no_diagnostics, rejected, saved};
use crate::{
    mux::{CardsCx, PaneCards},
    store::now_ms,
};

/// An edit Claude Code proposes through its IDE connection (M28).
#[derive(Debug)]
struct PendingDiff {
    /// Its pane, once its Claude Code's process is found in one.
    pane: Option<PaneId>,
    /// The relay's connection, and the call's id there.
    conn: u64,
    call: Value,
    tab: String,
    /// The file as it is, and as it would be.
    old: String,
    new: String,
    info: DiffInfo,
}

/// How much of a file a diff card keeps.
const MAX_DIFF_FILE: u64 = 4 << 20;
/// ...and of its diff.
const MAX_DIFF_TEXT: usize = 16 << 10;

/// "Claude Code wants to edit src/main.rs (+3 −1)".
fn diff_headline(d: &DiffInfo) -> String {
    let verb = if d.new { "create" } else { "edit" };
    format!("Claude Code wants to {verb} {} (+{} −{})", d.file, d.added, d.removed)
}

/// An `openDiff` call as a diff card: the file as it is now, and as it
/// would be.
fn new_diff(pane: Option<PaneId>, conn: u64, call: Value, args: &Value, cwd: Option<&str>) -> PendingDiff {
    use std::io::Read;
    let path = args["new_file_path"].as_str().or(args["old_file_path"].as_str()).unwrap_or("").to_owned();
    let old_path = args["old_file_path"].as_str().unwrap_or(&path).to_owned();
    let mut old = String::new();
    let exists = std::fs::File::open(&old_path).and_then(|f| f.take(MAX_DIFF_FILE).read_to_string(&mut old)).is_ok();
    let new: String = args["new_file_contents"].as_str().unwrap_or("").to_owned();
    let (added, removed, text) = diff::unified(&old, &new, MAX_DIFF_TEXT);
    let file = match cwd {
        Some(c) => crate::paths::relative(c, &path),
        None => path.clone(),
    };
    let id = format!("d{conn}-{}", call.to_string().trim_matches('"'));
    PendingDiff {
        pane,
        conn,
        call,
        tab: args["tab_name"].as_str().unwrap_or("").to_owned(),
        info: DiffInfo { id, file, added, removed, text, new: !exists, at_ms: now_ms(), ide: NAME.into() },
        old,
        new,
    }
}

/// The waiting edits, and the pane each of Claude Code's IDE connections
/// runs in.
#[derive(Debug)]
pub struct Cards {
    ide: std::sync::Arc<Ide>,
    diffs: Vec<PendingDiff>,
    conns: HashMap<u64, (Option<u32>, Option<PaneId>)>,
}

impl Cards {
    pub fn new(ide: std::sync::Arc<Ide>) -> Self {
        Self { ide, diffs: Vec::new(), conns: HashMap::new() }
    }

    /// The cards a call was given, if they are these.
    pub fn of(cards: &mut dyn PaneCards) -> Option<&mut Cards> {
        cards.as_any().downcast_mut()
    }

    /// An edit waiting as a diff in a pane (M28): it comes before the
    /// hook's permission card for the same edit.
    fn card(&self, pane: PaneId) -> Option<(Reason, DiffInfo)> {
        let d = &self.diffs.iter().find(|d| d.pane == Some(pane))?.info;
        let reason = Reason {
            kind: ReasonKind::Diff,
            since_ms: d.at_ms,
            headline: diff_headline(d),
            command: None,
            exit: None,
            duration_ms: None,
            bundle: None,
            ask: None,
            gate: None,
            // #887: a card away from the pane shows it from the reason alone.
            diff: Some(Box::new(d.clone())),
            actions: vec![Action::Accept, Action::Reject, Action::Dismiss],
        };
        Some((reason, d.clone()))
    }

    /// A pane's diffs changed: its card and its attention follow.
    fn changed(&self, cx: &mut dyn CardsCx, pane: PaneId) {
        cx.card(pane, self.card(pane));
    }

    /// The edit a pane's diff card shows: before and after.
    pub fn diff_of(&self, pane: PaneId) -> Option<(DiffInfo, String, String)> {
        let d = self.diffs.iter().find(|d| d.pane == Some(pane))?;
        Some((d.info.clone(), d.old.clone(), d.new.clone()))
    }

    /// The IDE connections of Claude Code in a pane (M28).
    pub fn conns_of(&self, pane: PaneId) -> Vec<u64> {
        let mut v: Vec<u64> = self.conns.iter().filter(|(_, c)| c.1 == Some(pane)).map(|(n, _)| *n).collect();
        v.sort();
        v
    }

    /// What Claude Code's IDE connections did (M28).
    pub fn event(&mut self, cx: &mut dyn CardsCx, ev: Event) {
        let ide = self.ide.clone();
        match ev {
            Event::Hello => {
                // The relay says again what's open; start from nothing.
                let panes: Vec<PaneId> = self.diffs.drain(..).filter_map(|d| d.pane).collect();
                self.conns.clear();
                for p in panes {
                    self.changed(cx, p);
                }
            }
            Event::Conn { conn, pid } => {
                let pane = pid.and_then(|p| cx.pane_of_pid(p));
                info!(conn, ?pid, ?pane, "Claude Code connected to its IDE");
                self.conns.insert(conn, (pid, pane));
                self.rehome(cx, conn, pane);
                if let Some(p) = pane {
                    cx.touch(p);
                }
            }
            Event::Gone { conn } => {
                if let Some((_, Some(p))) = self.conns.remove(&conn) {
                    cx.touch(p);
                }
                let gone: Vec<PaneId> = self.diffs.iter().filter(|d| d.conn == conn).filter_map(|d| d.pane).collect();
                self.diffs.retain(|d| d.conn != conn);
                for p in gone {
                    self.changed(cx, p);
                }
            }
            Event::Call { conn, id, tool, args } => match tool.as_str() {
                "openDiff" | "openDiff/here" => {
                    // Another IDE gets diffs, unless passing it on failed.
                    if tool == "openDiff" && ide.forward_diff(conn, &id, &args, cx.sender()) {
                        return;
                    }
                    let pane = self.conns.get(&conn).and_then(|c| c.1);
                    let cwd = pane.and_then(|p| cx.pane_cwd(p));
                    let d = new_diff(pane, conn, id, &args, cwd.as_deref());
                    info!(conn, ?pane, file = d.info.file, "an edit waits as a diff");
                    self.diffs.retain(|x| !(x.conn == d.conn && x.call == d.call));
                    self.diffs.push(d);
                    if let Some(p) = pane {
                        self.changed(cx, p);
                    }
                }
                "getDiagnostics" => ide.reply(conn, &id, no_diagnostics()),
                _ => ide.reply(conn, &id, json!({ "content": [] })),
            },
            Event::Closed { conn, ids, .. } => {
                ide.closed(conn, &ids);
                let (gone, keep): (Vec<PendingDiff>, Vec<PendingDiff>) =
                    self.diffs.drain(..).partition(|d| d.conn == conn && ids.contains(&d.call));
                self.diffs = keep;
                for d in gone {
                    let Some(p) = d.pane else { continue };
                    // The terminal answered it (M29's "Allowed, 14:02").
                    cx.answered(
                        p,
                        Answered {
                            id: d.info.id.clone(),
                            how: "answered".into(),
                            who: "terminal".into(),
                            name: "the terminal".into(),
                            at_ms: now_ms(),
                            headline: diff_headline(&d.info),
                        },
                    );
                    self.changed(cx, p);
                }
            }
        }
    }

    /// The pane an IDE connection's diffs belong to is known now.
    fn rehome(&mut self, cx: &mut dyn CardsCx, conn: u64, pane: Option<PaneId>) {
        let Some(p) = pane else { return };
        let mut found = false;
        for d in self.diffs.iter_mut().filter(|d| d.conn == conn && d.pane.is_none()) {
            d.pane = Some(p);
            found = true;
        }
        if found {
            self.changed(cx, p);
        }
    }

    /// Someone answered a diff card (M28): accepted (`accept`) or rejected,
    /// by its id or whichever waits; accepting may change it first.
    pub fn answer(
        &mut self,
        cx: &mut dyn CardsCx,
        pane: PaneId,
        id: Option<String>,
        accept: bool,
        text: Option<String>,
        by: Driver,
    ) -> Result<(), String> {
        let at = self
            .diffs
            .iter()
            .position(|d| d.pane == Some(pane) && id.as_ref().is_none_or(|i| *i == d.info.id))
            .ok_or_else(|| format!("%{pane} has no edit waiting (the terminal answered it, or it was closed)"))?;
        let d = self.diffs.remove(at);
        let changed = text.as_ref().is_some_and(|t| *t != d.new);
        let result = if accept { saved(text.as_deref().unwrap_or(&d.new)) } else { rejected(&d.tab) };
        self.ide.reply(d.conn, &d.call, result);
        let how = match (accept, changed) {
            (true, false) => "accepted",
            (true, true) => "accepted with changes",
            (false, _) => "rejected",
        };
        cx.record_answer(pane, &by, &d.info.id, how, &diff_headline(&d.info));
        self.changed(cx, pane);
        Ok(())
    }
}

impl PaneCards for Cards {
    fn linked(&self, pane: PaneId) -> bool {
        self.conns.values().any(|c| c.1 == Some(pane))
    }

    /// IDE connections whose Claude Code wasn't found in a pane yet (the
    /// panes come back after a restart a moment after the relay speaks):
    /// look again.
    fn tick(&mut self, cx: &mut dyn CardsCx) {
        let lost: Vec<(u64, u32)> = self
            .conns
            .iter()
            .filter(|(_, (_, p))| p.is_none())
            .filter_map(|(c, (pid, _))| Some((*c, (*pid)?)))
            .collect();
        for (conn, pid) in lost {
            if let Some(pane) = cx.pane_of_pid(pid) {
                info!(conn, pid, pane, "found Claude Code's pane");
                self.conns.insert(conn, (Some(pid), Some(pane)));
                self.rehome(cx, conn, Some(pane));
                cx.touch(pane);
            }
        }
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}
