//! What the daemon does about chat in a build without the `labs` feature:
//! nothing is kept, no thread is ever in a client's state, and every request
//! for one is refused with "isn't in this build". The real ones are
//! `thread_ops`; these are their twins for the methods the rest of the mux
//! calls.

use super::{ConvReq, Daemon, Posted, ThreadError, ThreadPost};
use crate::{acl::Principal, labs::not_built};
use arugula_core::Role;
use arugula_proto::{Conversation, SessionId, ThreadMsg, ThreadSummary, ThreadTarget};

impl Daemon {
    pub(super) fn thread_exists(&self, _target: ThreadTarget) -> bool {
        false
    }

    pub(super) fn convs_for(&self, _who: &Principal) -> Vec<Conversation> {
        Vec::new()
    }

    pub(super) fn conv(&mut self, _req: ConvReq, _who: &Principal) -> Result<Conversation, ThreadError> {
        Err(ThreadError(501, not_built("Chat")))
    }

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

    pub(super) fn threads_for(&self, _who: &Principal, _me: &Principal) -> Vec<ThreadSummary> {
        Vec::new()
    }
}
