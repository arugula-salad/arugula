//! Claude Code's proposed edits in nvim too (#1732). When arugula.nvim runs
//! in a terminal in the same session as the Claude Code that proposes an
//! edit (in the same tab, if one is there), the daemon sends it the
//! proposal (`diff`, see `editor/link.rs`), and the plugin opens a vimdiff
//! tab: the file as it is against the proposed text. Accepting (as
//! proposed or edited first) or rejecting there answers the same card the
//! rail and the pane show: the cards own the answer, so whichever answers
//! first (nvim, the card, or Claude Code's terminal) closes the others
//! (`diff_close`). Without such an nvim nothing changes. This isn't the
//! "another IDE" forward: the cards stay.
//!
//! The links are the cards' to hold, so what they do comes in the mux
//! task's order with everything else the cards do.

use std::sync::Arc;

use arugula_proto::{Driver, PaneId};
use serde_json::{Value, json};
use tracing::{debug, info};

use super::{Cards, cards::PendingDiff};
use crate::{
    editor::link::Link,
    mux::{Api, CardsCx, Cmd, MuxHandle},
};

/// arugula.nvim's link, as the cards hold it.
pub(super) struct Nvim(Arc<Link>);

impl std::fmt::Debug for Nvim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Nvim").field(&self.0.hello.pid).finish()
    }
}

/// Who answered, as the pane's history and the card say it.
fn by(nvim: PaneId) -> Driver {
    Driver { who: "nvim".into(), name: format!("nvim (pane %{nvim})") }
}

/// The proposal, as the plugin reads it.
fn proposal(d: &PendingDiff, claude: PaneId) -> Value {
    json!({
        "t": "diff",
        "id": d.info.id,
        "file": d.path,
        "name": d.info.file,
        "agent": "Claude Code",
        "pane": claude,
        "new_file": d.info.new,
        "old": d.old,
        "new": d.new,
    })
}

impl Cards {
    /// The nvim with arugula.nvim nearest Claude Code in `claude`: in its
    /// session, its own tab first, then the lowest pane.
    fn nearest(&self, cx: &dyn CardsCx, claude: PaneId) -> Option<PaneId> {
        let (session, tab) = cx.place(claude)?;
        self.nvims
            .keys()
            .filter(|p| **p != claude)
            .filter_map(|p| {
                let (s, t) = cx.place(*p)?;
                (s == session).then_some((t != tab, *p))
            })
            .min()
            .map(|(_, p)| p)
    }

    /// Show diff `i` in the nearest nvim too, if it isn't shown yet and one
    /// is there. Its card says so once the caller calls `changed`.
    pub(super) fn show(&mut self, cx: &dyn CardsCx, i: usize) {
        let Some(d) = self.diffs.get(i) else { return };
        let Some(claude) = d.pane.filter(|_| d.info.nvim.is_none()) else { return };
        let Some(nvim) = self.nearest(cx, claude) else { return };
        self.nvims[&nvim].0.send(proposal(d, claude));
        info!(pane = claude, nvim, file = d.info.file, "the diff shows in nvim too");
        self.diffs[i].info.nvim = Some(nvim);
    }

    /// A diff went (`how`: accepted, rejected, answered in the terminal,
    /// gone): its vimdiff tab closes.
    pub(super) fn unshow(&self, d: &PendingDiff, how: &str) {
        if let Some(link) = d.info.nvim.and_then(|n| self.nvims.get(&n)) {
            link.0.send(json!({ "t": "diff_close", "id": d.info.id, "how": how }));
        }
    }

    /// arugula.nvim attached to terminal `pane`: the diffs waiting near it
    /// show there, in the order they came.
    pub fn nvim_joined(&mut self, cx: &mut dyn CardsCx, pane: PaneId, link: Arc<Link>) {
        // Another link on the same terminal (the plugin reconnected): what
        // the old one showed, this one hasn't.
        if self.nvims.insert(pane, Nvim(link)).is_some() {
            for d in self.diffs.iter_mut().filter(|d| d.info.nvim == Some(pane)) {
                d.info.nvim = None;
            }
        }
        let mut touched = self.reshow(cx);
        touched.sort();
        touched.dedup();
        for p in touched {
            self.changed(cx, p);
        }
    }

    /// ...and it left: its diffs stay cards, and show in another nvim near
    /// them if there's one.
    pub fn nvim_left(&mut self, cx: &mut dyn CardsCx, pane: PaneId, link: &Arc<Link>) {
        if !self.nvims.get(&pane).is_some_and(|l| Arc::ptr_eq(&l.0, link)) {
            return;
        }
        self.nvims.remove(&pane);
        let mut touched = vec![];
        for d in self.diffs.iter_mut().filter(|d| d.info.nvim == Some(pane)) {
            d.info.nvim = None;
            touched.extend(d.pane);
        }
        touched.extend(self.reshow(cx));
        touched.sort();
        touched.dedup();
        for p in touched {
            self.changed(cx, p);
        }
    }

    /// Show every waiting diff that isn't shown, in the order they came;
    /// the panes whose cards that changes (for `changed`).
    fn reshow(&mut self, cx: &dyn CardsCx) -> Vec<PaneId> {
        let mut touched = vec![];
        for i in 0..self.diffs.len() {
            if self.diffs[i].info.nvim.is_none() {
                self.show(cx, i);
                if self.diffs[i].info.nvim.is_some() {
                    touched.extend(self.diffs[i].pane);
                }
            }
        }
        touched
    }

    /// nvim in terminal `nvim` answered diff `id`: accepted (with its text
    /// if it was edited) or rejected. A diff that's gone closes its tab.
    pub fn nvim_answer(
        &mut self,
        cx: &mut dyn CardsCx,
        nvim: PaneId,
        id: &str,
        accept: bool,
        text: Option<String>,
    ) -> Result<(), String> {
        let found = self.diffs.iter().find(|d| d.info.id == id && d.info.nvim == Some(nvim)).and_then(|d| d.pane);
        let Some(claude) = found else {
            if let Some(link) = self.nvims.get(&nvim) {
                link.0.send(json!({ "t": "diff_close", "id": id, "how": "gone" }));
            }
            return Err(format!("no edit {id} waits in nvim (pane %{nvim})"));
        };
        self.answer(cx, claude, Some(id.to_owned()), accept, text, by(nvim))
    }
}

/// A call on the cards from an editor's connection, on the mux task.
fn on_cards(mux: &MuxHandle, f: impl FnOnce(&mut Cards, &mut dyn CardsCx) + Send + 'static) {
    mux.send(Cmd::Api(Api::Cards(Box::new(move |cards, cx| {
        if let Some(c) = Cards::of(cards) {
            f(c, cx);
        }
    }))));
}

/// arugula.nvim attached to terminal `pane`, and shows diffs.
pub fn joined(mux: &MuxHandle, pane: PaneId, link: Arc<Link>) {
    on_cards(mux, move |c, cx| c.nvim_joined(cx, pane, link));
}

/// ...and it left.
pub fn left(mux: &MuxHandle, pane: PaneId, link: Arc<Link>) {
    on_cards(mux, move |c, cx| c.nvim_left(cx, pane, &link));
}

/// What the plugin in terminal `pane` said of a diff: `diff_accept` (with
/// `text` when edited first) or `diff_reject`.
pub fn answered(mux: &MuxHandle, pane: PaneId, msg: &Value) {
    let Some(id) = msg["id"].as_str().map(str::to_owned) else { return };
    let accept = msg["t"] == "diff_accept";
    let text = msg["text"].as_str().filter(|_| accept).map(str::to_owned);
    on_cards(mux, move |c, cx| {
        if let Err(e) = c.nvim_answer(cx, pane, &id, accept, text) {
            debug!(pane, error = e, "an answer from nvim for nothing");
        }
    });
}
