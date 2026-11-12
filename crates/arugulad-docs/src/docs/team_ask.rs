//! An agent asks to share a team Doc, and the owner taps (Arugula #871): the
//! library Doc's card (`ask.rs`, #1067) for a Doc in a team's project. The
//! `doc` tool's `share` raises it on the block and returns at once; the public
//! link is made only when the owner taps *Share*, then as `doc_share`. An
//! agent never publishes a team Doc's content on its machine's grant alone.
//! Taking a link down stays direct (`doc_unshare`): it only takes content
//! down. The ask is kept in memory, so a restart drops a waiting card (the
//! agent asks again); it lapses after the library's day.

use std::{sync::atomic::Ordering, time::Duration};

use arugula_proto::Driver;
use serde_json::{Value, json};
use tracing::{info, warn};

use super::{
    super::ask::{Asked, Outcome, card, card_id, ttl},
    Team,
};
use crate::{mux::AskReply, store::now_ms};

impl Team {
    fn closed(&self) -> bool {
        self.card.closed.load(Ordering::SeqCst)
    }

    /// The block closed: the card goes with it.
    pub(super) fn drop_card(&self) {
        self.card.closed.store(true, Ordering::SeqCst);
        self.withdraw_card();
    }

    fn withdraw_card(&self) {
        if let (Some((id, token)), Some(ctx)) = (self.card.token.lock().unwrap().take(), self.card.ctx.get()) {
            ctx.withdraw(&id, token);
        }
    }

    /// `doc_share_ask`: raise the owner's card, or say where it stands. It
    /// never makes a link itself.
    pub(super) async fn share_ask(&self, by: Option<String>) -> Result<Value, String> {
        let by =
            by.filter(|b| b.starts_with("mcp:")).ok_or("an agent asks to share a Doc with the doc tool's share")?;
        let ctx = self.card.ctx.get().ok_or("this Doc can't ask here")?;
        let _ = self.refresh().await;
        let page = self.link();
        if let Some(p) = self.public().filter(|p| p.ver > 0) {
            // Shared (by the card, or by the owner's own tap).
            self.settle(None);
            let viewer = self.remote()?.viewer().await?;
            return Ok(json!({ "status": "shared", "url": Self::url(&viewer, &p), "page": page }));
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
            (self.changed)();
            let mut out = json!({ "status": outcome.word() });
            if let Some(why) = why {
                out["why"] = json!(why);
            }
            return Ok(out);
        }
        // This control can't keep a public link: nothing to ask.
        if let Err(why) = self.remote()?.viewer().await {
            return Ok(json!({ "status": "team", "url": page, "why": why }));
        }
        let at = now_ms();
        info!(pane = ctx.id, by, "an agent asks to share a team Doc");
        *self.card.asked.lock().unwrap() = Some(Asked { at_ms: at, by, outcome: Outcome::Waiting, why: None });
        (self.changed)();
        if let Some(me) = self.me.upgrade() {
            crate::task::spawn_on(&ctx.rt, async move {
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
            let Some(ctx) = self.card.ctx.get() else { return };
            if self.card.token.lock().unwrap().is_some() || self.closed() || now_ms() >= at + ttl().as_millis() as u64 {
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
                    crate::task::spawn_on(&ctx.rt, async move {
                        let (reply, by) = rx.await.unwrap_or((AskReply::Withdrawn, None));
                        me.answered(at, token, reply, by).await;
                    });
                }
                Err(e) => warn!(pane = ctx.id, error = e, "can't show the team Doc's share card"),
            }
        })
    }

    /// Someone answered the card: only the owner's Share makes the link.
    async fn answered(&self, at: u64, token: u64, reply: AskReply, by: Option<Driver>) {
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
        let owner = by.as_ref().is_some_and(|b| b.who == "owner");
        match reply {
            AskReply::Answer(_) if owner => {
                self.card_share(at).await;
                return;
            }
            AskReply::Decline if owner => {
                self.settle(Some((at, Outcome::Declined, None)));
                return;
            }
            AskReply::Withdrawn | AskReply::Terminal => {}
            _ if !owner => warn!(by = ?by, "a team Doc's share card answered by someone not the owner"),
            _ => {}
        }
        if !self.closed() {
            self.raise(at).await;
        }
    }

    /// The owner tapped Share: `doc_share`, as them.
    async fn card_share(&self, at: u64) {
        let done = if self.public().is_some_and(|p| p.ver > 0) { Ok(()) } else { self.share(false).await.map(|_| ()) };
        match done {
            Ok(()) => self.settle(None),
            Err(why) => {
                warn!(error = why, "a team Doc's share card: it failed");
                self.settle(Some((at, Outcome::Failed, Some(why))));
            }
        }
    }

    /// The ask made at `at` has an outcome (or, `None`, is over).
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
        (self.changed)();
    }

    /// Lapse the ask made at `at` once its day is up, while the block lives.
    async fn expire(&self, at: u64) {
        let left = (at + ttl().as_millis() as u64).saturating_sub(now_ms());
        let me = self.me.clone();
        tokio::time::sleep(Duration::from_millis(left)).await;
        let Some(me) = me.upgrade() else { return };
        if !me.closed() {
            me.settle(Some((at, Outcome::Lapsed, None)));
        }
    }
}
