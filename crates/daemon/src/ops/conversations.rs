//! `conversations`: the Claude Code conversations on this machine, listed
//! and opened as agent blocks (M33, `arugula claude`), the owner's (`serve`
//! checks that before the handler runs). MCP's `list` kind
//! conversations and `show` kind conversation are the same two operations: an
//! agent's token is confined to its machine and its tab (`McpOp::args`), and
//! opens one as no one in particular.

use arugula_proto::{
    api::{ConversationList, ConversationsQuery, OpenConversationRequest, OpenConversationResponse},
    op::ops::{ConversationOpen, ConversationsList},
};

use super::{Cx, Handle, OpError, Via};
use crate::{
    api::{bad, list_conversations, open_conversation_as},
    mcp::{
        ops::{McpOp, Surface},
        tools::{Call, ConversationThen, ListConversationsArgs, OpenConversationArgs, Out, PaneArg, confine, done},
    },
};

impl Handle for ConversationsList {
    async fn handle(cx: &Cx<'_>, _: (), q: ConversationsQuery) -> Result<ConversationList, OpError> {
        Ok(list_conversations(cx.app, q).await.map_err(bad)?)
    }
}

impl Handle for ConversationOpen {
    async fn handle(cx: &Cx<'_>, _: (), req: OpenConversationRequest) -> Result<OpenConversationResponse, OpError> {
        let who = match &cx.via {
            Via::Http { who, .. } => Some(who.clone()),
            Via::Mcp(_) => None,
        };
        Ok(open_conversation_as(cx.app, who, req).await?)
    }
}

impl McpOp for ConversationsList {
    const SURFACE: Surface = Surface::Kind {
        tool: "list",
        kind: "conversations",
        description: "Claude Code conversations on this machine, from a terminal or the desktop app's Code tab, newest first: id, title, folder, first and last prompt, where it's open now, and the agent block that has it. show kind conversation opens one.",
    };
    type Args = ListConversationsArgs;

    async fn args(_: &Call<'_>, a: &ListConversationsArgs) -> Result<((), ConversationsQuery), String> {
        let q = ConversationsQuery {
            all: a.all,
            q: a.query.clone(),
            cwd: a.cwd.clone(),
            live: a.live,
            limit: Some(a.limit.unwrap_or(30)),
        };
        Ok(((), q))
    }

    fn answer(_: &Call<'_>, _: &ListConversationsArgs, _: &(), res: ConversationList) -> Out {
        let mut v = serde_json::to_value(res).unwrap_or_default();
        // The transcript's path is the daemon's business.
        for c in v["conversations"].as_array_mut().into_iter().flatten() {
            if let Some(o) = c.as_object_mut() {
                o.remove("path");
            }
        }
        let n = v["conversations"].as_array().map_or(0, Vec::len);
        done(format!("{n} conversations; show (kind conversation) shows one as a block"), v)
    }
}

impl McpOp for ConversationOpen {
    const SURFACE: Surface = Surface::Kind {
        tool: "show",
        kind: "conversation",
        description: "A Claude Code conversation (list kind conversations) as an agent block, stopped, with its transcript; then: continue (refused while it's open somewhere else) or fork (a new session with its history; the original is left alone). send_input to the block goes on.",
    };
    type Args = OpenConversationArgs;

    async fn args(call: &Call<'_>, a: &OpenConversationArgs) -> Result<((), OpenConversationRequest), String> {
        // Conversations are this host's, and continue as a Claude Code here.
        confine(call.on_machine().await?, false)?;
        let beside = match (a.beside.as_ref().map(PaneArg::id).transpose()?, call.own_pane()) {
            (Some(b), _) => Some(b),
            (None, own) => own,
        };
        if let Some(b) = beside {
            call.readable(b).await?;
        }
        let req = OpenConversationRequest {
            id: a.id.clone(),
            then: a.then.map(|t| match t {
                ConversationThen::Continue => "continue".into(),
                ConversationThen::Fork => "fork".into(),
            }),
            session: None,
            split: beside,
            from_pane: beside,
        };
        Ok(((), req))
    }

    fn answer(_: &Call<'_>, _: &OpenConversationArgs, _: &(), v: OpenConversationResponse) -> Out {
        let block = v.block;
        let summary = match v.error.as_deref() {
            Some(e) => format!("Opened it in %{block}, but: {e}"),
            None => format!("It's in %{block}; send_input to it to go on, then wait and read_output"),
        };
        done(summary, serde_json::to_value(&v).unwrap_or_default())
    }
}
