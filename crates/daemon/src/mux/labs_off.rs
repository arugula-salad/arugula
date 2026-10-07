//! What the daemon does about chat and huddles in a build without the `labs`
//! feature: nothing is kept, no thread or huddle is ever in a client's state,
//! and every request for either is refused with "isn't in this build". The
//! real ones are `thread_ops` and `call_ops`; these are their twins for the
//! methods the rest of the mux calls.

use super::{Daemon, Posted, ThreadError, ThreadPost};
use crate::{acl::Principal, labs::not_built, pane::Subscriber, pane::ToClient};
use arugula_core::Role;
use arugula_proto::{Call, ClientId, ServerMsg, SessionId, ThreadMsg, ThreadSummary, ThreadTarget};

impl Daemon {
    pub(super) fn thread_session(&self, _target: ThreadTarget) -> Option<SessionId> {
        None
    }

    pub(super) fn thread_role(&self, _who: &Principal, _target: ThreadTarget) -> Option<(Role, u64)> {
        None
    }

    pub(super) fn thread_place(
        &self,
        _target: ThreadTarget,
        _msg: Option<u64>,
    ) -> Result<(SessionId, Option<u64>), String> {
        Err(not_built("Chat"))
    }

    pub(super) fn thread_get(&self, _target: ThreadTarget, _who: &Principal) -> Result<Vec<ThreadMsg>, ThreadError> {
        Err(ThreadError(501, not_built("Chat")))
    }

    pub(super) fn thread_post(&mut self, _post: ThreadPost) -> Result<Posted, ThreadError> {
        Err(ThreadError(501, not_built("Chat")))
    }

    pub(super) fn threads_for(&self, _who: &Principal) -> Vec<ThreadSummary> {
        Vec::new()
    }

    pub(super) fn calls_for(&self, _who: &Principal) -> Vec<Call> {
        Vec::new()
    }

    pub(super) fn call_leave(&mut self, sub: &Subscriber, _session: SessionId) {
        refuse(sub);
    }

    pub(super) fn call_mute(&mut self, sub: &Subscriber, _session: SessionId, _muted: bool) {
        refuse(sub);
    }

    pub(super) fn call_join(&mut self, sub: &Subscriber, _session: SessionId) {
        refuse(sub);
    }

    pub(super) fn call_signal(
        &mut self,
        sub: &Subscriber,
        _session: SessionId,
        _to: ClientId,
        _signal: serde_json::Value,
    ) {
        refuse(sub);
    }
}

/// Tells the client its huddle message went nowhere.
fn refuse(sub: &Subscriber) {
    let _ = sub.ctrl.send(ToClient::Msg(ServerMsg::Error { id: None, message: not_built("Huddles") }));
}
