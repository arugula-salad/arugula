//! `conversations`: the Claude Code conversations on this machine, listed
//! and opened as agent blocks (M33, `arugula claude`). HTTP only, and the
//! owner's (`serve` checks that before the handler runs). MCP's `list` kind
//! conversations and `show` kind conversation reach the same two functions
//! in `api.rs`.

use arugula_proto::{
    api::{ConversationList, ConversationsQuery, OpenConversationRequest, OpenConversationResponse},
    op::ops::{ConversationOpen, ConversationsList},
};

use super::{Cx, Handle, OpError};
use crate::api::{bad, list_conversations, open_conversation_as};

impl Handle for ConversationsList {
    async fn handle(cx: &Cx<'_>, _: (), q: ConversationsQuery) -> Result<ConversationList, OpError> {
        Ok(list_conversations(cx.app, q).await.map_err(bad)?)
    }
}

impl Handle for ConversationOpen {
    async fn handle(cx: &Cx<'_>, _: (), req: OpenConversationRequest) -> Result<OpenConversationResponse, OpError> {
        let who = cx.principal()?;
        Ok(open_conversation_as(cx.app, Some(who), req).await?)
    }
}
