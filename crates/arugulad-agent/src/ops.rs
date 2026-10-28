//! `conversations`: the Claude Code conversations on this machine, listed
//! and opened as agent blocks (M33, `arugula claude`), the owner's (`serve`
//! checks that before the handler runs). MCP's `list` kind
//! conversations and `show` kind conversation are the same two operations: an
//! agent's token is confined to its machine and its tab (`McpOp::args`), and
//! opens one as no one in particular. They're the agent block's
//! (`Handle<AgentBlock>`, routed with `op_of`), and MCP lists them through
//! `mcp::registry` where they were.

use std::collections::HashMap;

use arugula_proto::{
    PaneId,
    api::{ConversationList, ConversationRow, ConversationsQuery, OpenConversationRequest, OpenConversationResponse},
    op::ops::{ConversationOpen, ConversationsList},
};
use axum::http::StatusCode;
use futures_util::future::BoxFuture;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use arugulad_base::ops::{Cx, Handle, OpError, Via};

use super::AgentBlock;
use crate::{
    api::{ApiError, bad},
    mcp::{
        ops::{McpOp, Surface, kind_of},
        tools::{Call, Kind, Out, PaneArg, confine, done},
    },
    mux::Api,
    server::App,
};

#[derive(Deserialize, JsonSchema)]
pub struct ListConversationsArgs {
    /// Words in the title, prompts or folder.
    #[serde(default)]
    pub query: Option<String>,
    /// Only ones under this folder.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Only ones open somewhere now.
    #[serde(default)]
    pub live: bool,
    /// Everything: `claude -p` and SDK runs, archived ones, ones whose folder is gone.
    #[serde(default)]
    pub all: bool,
    /// How many [default 30].
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OpenConversationArgs {
    /// The conversation's id, or the start of it (list kind conversations).
    pub id: String,
    /// Then continue it, or fork it (for one open somewhere else) and go on in the fork.
    #[serde(default)]
    pub then: Option<ConversationThen>,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ConversationThen {
    Continue,
    Fork,
}

/// `list` kind conversations, after panes, and `show` kind conversation,
/// after issue: where they were.
pub(super) fn register() {
    use crate::mcp::{ops::call_of, registry};
    static LIST: Kind = kind_of::<ConversationsList, AgentBlock>();
    static SHOW: Kind = kind_of::<ConversationOpen, AgentBlock>();
    fn list<'a>(c: &'a Call<'a>, _: Option<&'static str>, args: Value) -> BoxFuture<'a, Out> {
        Box::pin(call_of::<ConversationsList, AgentBlock>(c, args))
    }
    fn show<'a>(c: &'a Call<'a>, _: Option<&'static str>, args: Value) -> BoxFuture<'a, Out> {
        Box::pin(call_of::<ConversationOpen, AgentBlock>(c, args))
    }
    registry::kind(registry::ToolKind { tool: "list", after: Some("panes"), kind: &LIST, run: list });
    registry::kind(registry::ToolKind { tool: "show", after: Some("issue"), kind: &SHOW, run: show });
}

impl Handle<AgentBlock> for ConversationsList {
    async fn handle(cx: &Cx<'_>, _: (), q: ConversationsQuery) -> Result<ConversationList, OpError> {
        Ok(list_conversations(cx.app, q).await.map_err(bad)?)
    }
}

impl Handle<AgentBlock> for ConversationOpen {
    async fn handle(cx: &Cx<'_>, _: (), req: OpenConversationRequest) -> Result<OpenConversationResponse, OpError> {
        let who = match &cx.via {
            Via::Http { who, .. } => Some(who.clone()),
            Via::Mcp(_) => None,
        };
        Ok(open_conversation_as(cx.app, who, req).await?)
    }
}

impl McpOp<AgentBlock> for ConversationsList {
    const SURFACE: Surface = Surface::Kind {
        tool: "list",
        kind: "conversations",
        description: "Claude Code conversations on this machine, from a terminal or the desktop app's Code tab, newest first: id, title, folder, first and last prompt, where it's open now, and the agent block that has it. show kind conversation opens one.",
    };
    type Args = ListConversationsArgs;
    type Answer = serde_json::Value;

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

impl McpOp<AgentBlock> for ConversationOpen {
    const SURFACE: Surface = Surface::Kind {
        tool: "show",
        kind: "conversation",
        description: "A Claude Code conversation (list kind conversations) as an agent block, stopped, with its transcript; then: continue (refused while it's open somewhere else) or fork (a new session with its history; the original is left alone). send_input to the block goes on.",
    };
    type Args = OpenConversationArgs;
    type Answer = serde_json::Value;

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

/// Agent blocks and chats by the Claude Code session they have.
async fn blocks_by_session(app: &App) -> HashMap<String, PaneId> {
    let mut out = HashMap::new();
    for p in app.mux.api(Api::Panes).await.unwrap_or_default() {
        if !p.info.kind.prompted() {
            continue;
        }
        if let Some(b) = app.mux.api(|r| Api::Block(p.info.id, r)).await.flatten()
            && let Some(sid) = b.config()["session_id"].as_str()
        {
            out.insert(sid.to_owned(), p.info.id);
        }
    }
    out
}

/// Our terminals' and agent blocks' processes on this host (#81): a
/// Claude Code under one of them runs there.
async fn our_pids(app: &App) -> crate::conversations::Ours {
    let mut ours = crate::conversations::Ours::default();
    for p in app.mux.api(Api::Panes).await.unwrap_or_default() {
        let id = p.info.id;
        match p.info.kind {
            arugula_proto::BlockType::Terminal => {
                if let Some(pid) = app.mux.api(|r| Api::Pane(id, r)).await.flatten().and_then(|h| h.pid_now()) {
                    ours.panes.insert(pid, id);
                }
            }
            _ => {
                if let Some(pid) = app.mux.api(|r| Api::Block(id, r)).await.flatten().and_then(|b| b.pid()) {
                    ours.blocks.insert(pid, id);
                }
            }
        }
    }
    ours
}

async fn list_conversations(app: &App, q: ConversationsQuery) -> Result<ConversationList, String> {
    let blocks = blocks_by_session(app).await;
    let ours: std::collections::HashSet<PaneId> =
        app.mux.api(Api::Panes).await.unwrap_or_default().into_iter().map(|p| p.info.id).collect();
    let pids = our_pids(app).await;
    let list = crate::task::spawn_blocking(move || {
        let mut ix = crate::conversations::Index::global().lock().unwrap();
        ix.set_ours(pids);
        ix.scan()
    })
    .await
    .map_err(|e| e.to_string())?;
    let words: Vec<String> = q.q.as_deref().unwrap_or("").split_whitespace().map(str::to_lowercase).collect();
    let total = list.len();
    let out: Vec<ConversationRow> = list
        .into_iter()
        .filter(|c| crate::conversations::shown(c, q.all))
        .filter(|c| !q.live || c.live.is_some())
        .filter(|c| q.cwd.as_deref().is_none_or(|d| crate::paths::is_under(&c.cwd, d)))
        .filter(|c| {
            let hay = format!(
                "{} {} {} {}",
                c.title,
                c.first_prompt.as_deref().unwrap_or(""),
                c.last_prompt.as_deref().unwrap_or(""),
                c.cwd
            )
            .to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .take(q.limit.unwrap_or(500))
        .map(|mut c| {
            let block = blocks.get(&c.id).copied();
            c.live = c.live.take().map(|l| l.ours(|p| ours.contains(&p)));
            // The adapter of the block that has it, when its scope didn't
            // say (no systemd scopes).
            if let (Some(b), Some(l)) = (block, c.live.as_mut())
                && l.block.is_none()
                && l.entrypoint == "sdk-ts"
            {
                l.block = Some(b);
                l.pane = None;
                l.place = l.place();
            }
            ConversationRow { conversation: serde_json::to_value(&c).unwrap_or_default(), block }
        })
        .collect();
    Ok(ConversationList { conversations: out, total })
}

async fn open_conversation_as(
    app: &App,
    who: Option<crate::acl::Principal>,
    req: OpenConversationRequest,
) -> Result<OpenConversationResponse, ApiError> {
    let id = req.id.clone();
    let c = crate::task::spawn_blocking(move || crate::conversations::Index::global().lock().unwrap().find(&id))
        .await
        .map_err(|e| bad(e.to_string()))?
        .map_err(|e| ApiError(StatusCode::NOT_FOUND, e))?;
    let existing = blocks_by_session(app).await.get(&c.id).copied();
    let (block, opened) = match existing {
        Some(b) => (b, false),
        None => {
            let source = serde_json::to_value(c.source).unwrap_or_default();
            let config = serde_json::json!({
                "agent": "claude",
                "cwd": c.cwd,
                "session_id": c.id,
                "import": { "path": c.path, "source": source, "title": c.title, "model": c.model },
            });
            let open = arugula_proto::api::OpenRequest {
                kind: arugula_proto::BlockType::Agent,
                config,
                session: req.session,
                split: req.split,
                from_pane: req.from_pane,
                vm: false,
                image: None,
                host: None,
                local: true,
            };
            match app.mux.api(|r| Api::Open(open, who, r)).await {
                Some(Ok(b)) => (b, true),
                Some(Err(e)) => return Err(bad(e)),
                None => return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())),
            }
        }
    };
    let mut out = OpenConversationResponse { block, opened, conversation: c.id, error: None };
    if let Some(then) = req.then.as_deref() {
        let method = match then {
            "continue" | "fork" => then,
            t => return Err(bad(format!("then: {t}? (continue or fork)"))),
        };
        let b = app.mux.api(|r| Api::Block(block, r)).await.flatten().ok_or_else(|| bad("the block went away"))?;
        if let Err(e) = b.call(method, serde_json::json!({})).await {
            out.error = Some(e);
        }
    }
    Ok(out)
}
