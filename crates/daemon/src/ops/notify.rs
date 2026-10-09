//! `push` and `notify`: a browser's subscription to Web Push, and which
//! "needs you" notifications someone is told (M29). HTTP only. Anyone here
//! may call them, so each handler answers for the caller alone.

use arugula_proto::{
    api::{Empty, NotifyPref, NotifyRequest, PushKey, PushSubscriptions, Subscription},
    op::ops::{NotifyGet, NotifySet, PushKeyGet, PushSubscribe, PushTest},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError};
use crate::api::bad;

fn push_off() -> OpError {
    OpError::Status(StatusCode::NOT_FOUND, "push is off".into())
}

impl Handle for PushKeyGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<PushKey, OpError> {
        let push = cx.app.push.as_ref().ok_or_else(push_off)?;
        Ok(PushKey { key: push.public_key() })
    }
}

impl Handle for PushSubscribe {
    async fn handle(cx: &Cx<'_>, _: (), mut sub: Subscription) -> Result<PushSubscriptions, OpError> {
        let push = cx.app.push.as_ref().ok_or_else(push_off)?;
        // Anyone with access here may subscribe (M29); a subscription is its
        // subscriber's, never someone else's.
        let who = cx.principal()?;
        if !who.is_owner() && !cx.app.acl.knows(&who) {
            return Err(OpError::Forbidden("you have no access here".into()));
        }
        sub.who = (!who.is_owner()).then(|| who.id().to_owned());
        push.subscribe(sub).map_err(|e| OpError::Failed(e.to_string()))?;
        Ok(PushSubscriptions { subscriptions: push.subscriptions() })
    }
}

impl Handle for PushTest {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<PushSubscriptions, OpError> {
        let push = cx.app.push.as_ref().ok_or_else(push_off)?;
        let me = cx.principal()?.id().to_owned();
        push.send_to(0, "Arugula", "Notifications work.", None, |w| w == me);
        Ok(PushSubscriptions { subscriptions: push.subscriptions() })
    }
}

impl Handle for NotifyGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<NotifyPref, OpError> {
        let who = cx.principal()?;
        if who.is_owner() {
            return Ok(NotifyPref { all: true, ..Default::default() });
        }
        Ok(cx.app.acl.notify_pref(who.id()))
    }
}

impl Handle for NotifySet {
    async fn handle(cx: &Cx<'_>, _: (), req: NotifyRequest) -> Result<NotifyPref, OpError> {
        let acl = &cx.app.acl;
        let who = cx.principal()?;
        if who.is_owner() {
            return Err(bad("the owner is always told").into());
        }
        match req.session {
            Some(s) if acl.role(&who, s).is_none_or(|r| r < arugula_core::Role::Editor) => {
                return Err(OpError::Forbidden("only people who may answer are told".into()));
            }
            None if !acl.knows(&who) => return Err(OpError::Forbidden("you have no access here".into())),
            _ => {}
        }
        acl.set_notify(who.id(), req.session, req.on).map_err(|e| OpError::Failed(e.to_string()))
    }
}
