//! #1067: an agent asks to share a Doc, and the owner taps. The `doc` tool's
//! `share {block}` calls [`Doc::share_ask`], which raises an `Ask` on the Doc
//! block itself (an agent's pane holds only Claude Code's own card, as for
//! an invite: `invite/card.rs`) and returns at once. Nothing is uploaded
//! until the owner taps *Share*; then it is `doc_share`, as the owner.
//!
//! The card is the owner's alone, by every route that can answer one: the
//! mux refuses anyone whose answer isn't the owner's
//! ([`crate::mux::DOC_OWNER_ONLY`]), the HTTP routes say 403 first (and for
//! an agent on the owner's CLI, and a team's other owner, whom the mux can't
//! tell from the owner), and here it is checked again.
//!
//! Config `share_ask: {at_ms, by, outcome, why}` keeps it across a restart:
//! a waiting card is raised again on restore while it is within its day
//! (`ARUGULA_DOC_SHARE_TTL_MS` in a debug build's tests), and lapses after.
//! `share` called again says what became of it, once: `shared` with the
//! URL (the one place an agent is ever given a key: it asked, and the owner
//! chose to publish), `declined`, `lapsed` or `failed`; asking again after
//! that raises a new card.

use std::{
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use arugula_proto::ask::{Ask, AskKind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};

use super::{Doc, link::Fail};
use crate::{block::BlockCtx, mux::AskReply, store::now_ms};

/// What the card says under its question.
const DETAIL: &str =
    "Anyone with the link can read it, without an account. You can update or unshare it from the Doc's menu.";

/// How long a card waits before it lapses (a debug build's tests may say,
/// `ARUGULA_DOC_SHARE_TTL_MS`).
fn ttl() -> Duration {
    let day = Duration::from_secs(24 * 3600);
    if !cfg!(debug_assertions) {
        return day;
    }
    std::env::var("ARUGULA_DOC_SHARE_TTL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(day)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Outcome {
    #[default]
    Waiting,
    Declined,
    Lapsed,
    Failed,
}

impl Outcome {
    fn word(self) -> &'static str {
        match self {
            Outcome::Waiting => "waiting",
            Outcome::Declined => "declined",
            Outcome::Lapsed => "lapsed",
            Outcome::Failed => "failed",
        }
    }
}

/// The last ask: who made it, when, and what became of it until a repeat
/// call has said so.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Asked {
    pub at_ms: u64,
    pub by: String,
    #[serde(default)]
    pub outcome: Outcome,
    /// `failed`: the one line why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// A Doc's share card: its block's context to ask through, the ask kept in
/// the config, and the card on screen.
#[derive(Default)]
pub(super) struct Card {
    ctx: OnceLock<BlockCtx>,
    pub asked: Mutex<Option<Asked>>,
    /// The card on screen: its ask's id and token.
    token: Mutex<Option<(String, u64)>>,
    /// One card raised at a time.
    raising: tokio::sync::Mutex<()>,
    closed: AtomicBool,
}

impl Doc {
    /// The block's context, once it has one, and what it asked before a
    /// restart; the card is raised again if it is still within its day.
    pub(super) fn attach(&self, ctx: BlockCtx, asked: Option<Asked>) {
        let _ = self.card.ctx.set(ctx);
        let Some(asked) = asked else { return };
        let waiting = asked.outcome == Outcome::Waiting;
        let at = asked.at_ms;
        *self.card.asked.lock().unwrap() = Some(asked);
        if waiting && let Some(me) = self.me.upgrade() {
            self.ctx().expect("just set").rt.spawn(async move {
                me.raise(at).await;
                me.expire(at).await;
            });
        }
    }

    fn ctx(&self) -> Option<&BlockCtx> {
        self.card.ctx.get()
    }

    fn changed(&self) {
        (self.changed)();
    }

    fn closed(&self) -> bool {
        self.card.closed.load(Ordering::SeqCst)
    }

    /// The block closed: the card goes with it.
    pub(super) fn drop_card(&self) {
        self.card.closed.store(true, Ordering::SeqCst);
        self.withdraw_card();
    }

    fn withdraw_card(&self) {
        if let (Some((id, token)), Some(ctx)) = (self.card.token.lock().unwrap().take(), self.ctx()) {
            ctx.withdraw(&id, token);
        }
    }

    /// `doc_share_ask`, for the `doc` tool's `share` and nothing else: raises
    /// the card, or says where it stands. It never uploads.
    pub(super) async fn share_ask(&self, by: Option<String>) -> Result<Value, String> {
        let by =
            by.filter(|b| b.starts_with("mcp:")).ok_or("an agent asks to share a Doc with the doc tool's share")?;
        let ctx = self.ctx().ok_or("this Doc can't ask here")?;
        // Behind any share going on: the answer is then the outcome.
        let _one = self.shares.lock().await;
        match self.link.shares().await {
            Ok(Some(s)) => {
                // Shared (by the card, or by the owner's own tap): the
                // answer is the link, and no card is left.
                self.settle(None);
                self.set_shared(Some(super::SharedAt { made: s.made, updated: s.updated }));
                return Ok(json!({ "status": "shared", "url": s.url }));
            }
            Ok(None) => {}
            Err(Fail::Gone) => self.set_shared(None),
            Err(Fail::Said(e)) => return Err(e),
        }
        let said = {
            let mut asked = self.card.asked.lock().unwrap();
            match asked.as_ref().map(|a| (a.outcome, a.why.clone())) {
                Some((Outcome::Waiting, _)) => return Ok(json!({ "status": "waiting" })),
                // Said once; the next call asks again.
                Some((outcome, why)) => {
                    *asked = None;
                    Some((outcome, why))
                }
                None => None,
            }
        };
        if let Some((outcome, why)) = said {
            self.changed();
            let mut out = json!({ "status": outcome.word() });
            if let Some(why) = why {
                out["why"] = json!(why);
            }
            return Ok(out);
        }
        let at = now_ms();
        info!(pane = ctx.id, by, "an agent asks to share a Doc");
        *self.card.asked.lock().unwrap() = Some(Asked { at_ms: at, by, outcome: Outcome::Waiting, why: None });
        self.changed();
        if let Some(me) = self.me.upgrade() {
            ctx.rt.spawn(async move {
                me.raise(at).await;
                me.expire(at).await;
            });
        }
        Ok(json!({ "status": "waiting" }))
    }

    /// Put the card for the ask made at `at` on screen, if it is still
    /// waiting and none is.
    fn raise(&self, at: u64) -> futures_util::future::BoxFuture<'_, ()> {
        Box::pin(async move {
            let _one = self.card.raising.lock().await;
            let Some(ctx) = self.ctx() else { return };
            if self.card.token.lock().unwrap().is_some() || self.closed() {
                return;
            }
            // A card whose day is up (restored after a long stop) is never
            // shown: `expire` lapses it, with no notification for it.
            if now_ms() >= at + ttl().as_millis() as u64 {
                return;
            }
            let by = match &*self.card.asked.lock().unwrap() {
                Some(a) if a.at_ms == at && a.outcome == Outcome::Waiting => a.by.clone(),
                _ => return,
            };
            let title = self.state.lock().unwrap().title.clone();
            match ctx.ask(card(at, &title, &by)).await {
                Ok((token, rx)) => {
                    *self.card.token.lock().unwrap() = Some((card_id(at), token));
                    let Some(me) = self.me.upgrade() else { return };
                    ctx.rt.spawn(async move {
                        let (reply, by) = rx.await.unwrap_or((AskReply::Withdrawn, None));
                        me.answered(at, token, reply, by).await;
                    });
                }
                Err(e) => warn!(pane = ctx.id, error = e, "can't show the Doc's share card"),
            }
        })
    }

    /// Someone answered the card.
    async fn answered(&self, at: u64, token: u64, reply: AskReply, by: Option<arugula_proto::Driver>) {
        {
            let mut t = self.card.token.lock().unwrap();
            if t.as_ref().map(|(_, t)| *t) != Some(token) {
                return;
            }
            *t = None;
        }
        let waiting =
            matches!(&*self.card.asked.lock().unwrap(), Some(a) if a.at_ms == at && a.outcome == Outcome::Waiting);
        if !waiting {
            return;
        }
        // The owner's card: anyone else's answer (which the mux and the
        // routes refuse already) changes nothing, and it is asked again.
        let owner = by.as_ref().is_some_and(|b| b.who == "owner");
        match reply {
            // Share is the card's answer, Not now its decline.
            AskReply::Answer(_) if owner => {
                self.card_share(at).await;
                return;
            }
            AskReply::Decline if owner => {
                info!(pane = self.ctx().map(|c| c.id), "a Doc's share card: not now");
                self.settle(Some((at, Outcome::Declined, None)));
                return;
            }
            AskReply::Withdrawn | AskReply::Terminal => {}
            _ if !owner => warn!(by = ?by, "a Doc's share card answered by someone not the owner"),
            _ => {}
        }
        if !self.closed() {
            self.raise(at).await;
        }
    }

    /// The owner tapped Share: `doc_share`, as them. A failure settles the
    /// ask as `failed` with its one line.
    async fn card_share(&self, at: u64) {
        let _one = self.shares.lock().await;
        let done = if self.link.current().is_some() { Ok(()) } else { self.share_held(false).await.map(|_| ()) };
        match done {
            Ok(()) => {
                info!(pane = self.ctx().map(|c| c.id), "a Doc's share card: shared");
                self.settle(None);
            }
            Err(why) => {
                warn!(pane = self.ctx().map(|c| c.id), error = why, "a Doc's share card: it failed");
                self.settle(Some((at, Outcome::Failed, Some(why))));
            }
        }
    }

    /// The ask made at `at` has an outcome (or, `None`, is over with
    /// nothing to say: it was shared).
    fn settle(&self, to: Option<(u64, Outcome, Option<String>)>) {
        {
            let mut asked = self.card.asked.lock().unwrap();
            match to {
                None => *asked = None,
                Some((at, outcome, why)) => match asked.as_mut() {
                    Some(a) if a.at_ms == at && a.outcome == Outcome::Waiting => (a.outcome, a.why) = (outcome, why),
                    _ => return,
                },
            }
        }
        self.withdraw_card();
        self.changed();
    }

    /// Lapse the ask made at `at` once its day is up, while the block lives.
    async fn expire(&self, at: u64) {
        let left = (at + ttl().as_millis() as u64).saturating_sub(now_ms());
        let me = self.me.clone();
        tokio::time::sleep(Duration::from_millis(left)).await;
        let Some(me) = me.upgrade() else { return };
        if me.closed() {
            return;
        }
        let _one = me.shares.lock().await;
        info!(pane = me.ctx().map(|c| c.id), "a Doc's share card lapsed");
        me.settle(Some((at, Outcome::Lapsed, None)));
    }

    pub(super) fn asked_json(&self) -> Option<Value> {
        self.card.asked.lock().unwrap().as_ref().and_then(|a| serde_json::to_value(a).ok())
    }
}

fn card_id(at: u64) -> String {
    format!("share-{at}")
}

/// The owner's card: the question and what it means; the client's buttons
/// are Share (an answer) and Not now (a decline).
fn card(at: u64, title: &str, by: &str) -> Ask {
    let agent = by.strip_prefix("mcp:").unwrap_or(by).to_owned();
    let schema = json!({
        "type": "object",
        "description": DETAIL,
        "properties": {},
    });
    Ask {
        id: card_id(at),
        kind: AskKind::Form,
        message: format!("Share '{title}' with a link anyone can open?"),
        questions: None,
        schema: Some(schema),
        url: None,
        accepted: false,
        tool_call_id: None,
        source: "doc".into(),
        agent: Some(agent),
        at_ms: at,
        tool: None,
        input: None,
        suggestions: None,
        session: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_asks_by_title_and_says_what_sharing_means() {
        let a = card(5, "Q3 plan", "mcp:claude-code");
        assert_eq!(a.message, "Share 'Q3 plan' with a link anyone can open?");
        assert_eq!((a.kind, a.source.as_str(), a.agent.as_deref()), (AskKind::Form, "doc", Some("claude-code")));
        let s = a.schema.unwrap();
        assert_eq!(s["description"], DETAIL);
        assert!(!a.message.contains("#k="), "no key on the card");
    }
}
