//! `docs.list`, `doc.open` and `doc.delete` (#1493): the Doc library (see
//! [`crate::docs::library`]) listed, a Doc opened in a block, and a Doc
//! deleted. The owner's: `serve` checks that before the handler runs. MCP's
//! `doc` kind `list` is `docs.list`; its kind `open` is `doc.open` with the
//! caller recorded as the block's starter (`mcp/tools.rs`), so the agent can
//! `fill` what it opened. An agent can't delete.
//!
//! **One block per Doc.** The blocks that show a Doc are found by a scan: a
//! Doc block's config names its Doc (`open_blocks`). `doc.open` holds one
//! lock while it looks and while it opens, so two opens of one Doc make one
//! block.

use std::{collections::HashMap, sync::Arc};

use arugula_core::PaneId;
use arugula_proto::{
    BlockType,
    api::{DocOpenRequest, DocOpened, DocRow, DocShared, Empty, OpenRequest},
    op::ops::{DocDelete, DocOpen, DocsList},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError, Via};
use crate::{
    docs::{
        library,
        link::{self, Remote},
    },
    mcp::{
        ops::{McpOp, Surface},
        tools::{Call, ListArgs, Out, done},
    },
    mux::Api,
    server::App,
};

/// Said to an agent and to a team's other owner who asks to delete a Doc.
pub const DELETE_OWNER_ONLY: &str = "only the owner can delete a Doc";

/// The blocks that show a Doc, by the Doc's id.
pub(crate) async fn open_blocks(app: &App) -> HashMap<String, PaneId> {
    let mut out = HashMap::new();
    for p in app.mux.api(Api::Panes).await.unwrap_or_default() {
        if p.info.kind != BlockType::Doc {
            continue;
        }
        if let Some(b) = app.mux.api(|r| Api::Block(p.info.id, r)).await.flatten()
            && let Some(doc) = b.config()["doc"].as_str()
        {
            out.insert(doc.to_owned(), p.info.id);
        }
    }
    out
}

/// `search`'s lines of the Docs in the library (a Doc no block shows too),
/// when `docs` is on.
pub(crate) async fn search_library(
    app: &App,
    re: regex::Regex,
    since_ms: Option<u64>,
    limit: usize,
) -> Vec<arugula_proto::api::SearchHit> {
    let state = app.control.state_dir().to_owned();
    if limit == 0 || crate::labs::flag(&state, arugula_proto::flags::DOCS).is_err() {
        return vec![];
    }
    let open = open_blocks(app).await;
    tokio::task::spawn_blocking(move || library::search(&state, &open, &re, since_ms, limit)).await.unwrap_or_default()
}

fn not_found(id: &str) -> OpError {
    OpError::Status(StatusCode::NOT_FOUND, format!("no Doc {id} in the library (docs list has them)"))
}

fn checked(app: &App, id: &str) -> Result<library::Meta, OpError> {
    let dir = library::dir(app.control.state_dir(), id).map_err(|e| OpError::Status(StatusCode::BAD_REQUEST, e))?;
    library::read_meta(&dir).ok_or_else(|| not_found(id))
}

impl Handle for DocsList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Vec<DocRow>, OpError> {
        let open = open_blocks(cx.app).await;
        let state = cx.app.control.state_dir().to_owned();
        let (metas, links) = tokio::task::spawn_blocking(move || (library::list(&state), link::by_doc(&state)))
            .await
            .map_err(|e| OpError::Failed(e.to_string()))?;
        Ok(metas
            .into_iter()
            .map(|m| DocRow {
                open: open.get(&m.id).copied(),
                shared: links.get(&m.id).map(|(_, e)| DocShared { made: e.made, updated: e.updated }),
                id: m.id,
                title: m.title,
                made: m.made,
                updated: m.updated,
                by: m.by,
            })
            .collect())
    }
}

impl McpOp for DocsList {
    const SURFACE: Surface = Surface::Kind {
        tool: "doc",
        kind: "list",
        description: "The Docs kept on this machine, newest first: id, title, made, updated, who made it, the block that shows it now (open) and whether it is shared. Any Doc an earlier session wrote is here, open or not; doc open shows one in a block, which you can then fill.",
    };
    type Args = ListArgs;
    type Answer = serde_json::Value;

    async fn args(_: &Call<'_>, _: &ListArgs) -> Result<((), Empty), String> {
        Ok(((), Empty {}))
    }

    fn answer(_: &Call<'_>, _: &ListArgs, _: &(), docs: Vec<DocRow>) -> Out {
        let n = docs.len();
        done(format!("{n} Docs; doc open shows one in a block"), serde_json::json!({ "docs": docs }))
    }
}

/// Open a Doc in a block, or find the block that has it: the block, and
/// whether it was made now. `who` is the person who asks, over HTTP.
pub(crate) async fn open_doc(
    app: &App,
    who: Option<crate::acl::Principal>,
    req: DocOpenRequest,
) -> Result<(PaneId, bool), OpError> {
    // One at a time, so two opens of one Doc can't both find none.
    static ONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _one = ONE.lock().await;
    let meta = checked(app, &req.id)?;
    if let Some(block) = open_blocks(app).await.get(&meta.id) {
        return Ok((*block, false));
    }
    let open = OpenRequest {
        kind: BlockType::Doc,
        config: serde_json::json!({ "doc": meta.id, "title": meta.title }),
        session: req.session,
        split: req.split,
        from_pane: req.split,
        vm: false,
        image: None,
        host: None,
        local: true,
    };
    match app.mux.api(|r| Api::Open(open, who, r)).await {
        Some(Ok(b)) => Ok((b, true)),
        Some(Err(e)) => Err(OpError::Status(StatusCode::BAD_REQUEST, e)),
        None => Err(OpError::Status(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())),
    }
}

impl Handle for DocOpen {
    async fn handle(cx: &Cx<'_>, _: (), req: DocOpenRequest) -> Result<DocOpened, OpError> {
        let who = match &cx.via {
            Via::Http { who, .. } => Some(who.clone()),
            Via::Mcp(_) => None,
        };
        Ok(DocOpened { pane: open_doc(cx.app, who, req).await?.0 })
    }
}

impl Handle for DocDelete {
    async fn handle(cx: &Cx<'_>, id: String, _: Empty) -> Result<Empty, OpError> {
        // The link's key is the machine's own account's, and a Doc that is
        // gone can't be got back: not an agent's, nor a team's other owner's.
        if cx.agent_or_guest() || cx.co_owner() {
            return Err(OpError::Forbidden(DELETE_OWNER_ONLY.into()));
        }
        let meta = checked(cx.app, &id)?;
        let state = cx.app.control.state_dir().to_owned();
        let block = open_blocks(cx.app).await.get(&meta.id).copied();
        // Unshared first. A key must never be left behind a live link, so
        // with control out of reach everything is kept.
        if let Some((link_id, _)) = link::of_doc(&state, &meta.id) {
            let refuse = |e: String| {
                OpError::Status(
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("can't unshare it at control, so the Doc is kept: {e}"),
                )
            };
            // The open block, if it holds this link, does it all and clears
            // its `shared`; otherwise only the key goes.
            let mut done = false;
            if let Some(b) = match block {
                Some(b) => cx.app.mux.api(|r| Api::Block(b, r)).await.flatten(),
                None => None,
            } {
                let got = b.call("doc_unshare", serde_json::json!({ "id": link_id })).await.map_err(refuse)?;
                done = got["other"] != true;
            }
            if !done {
                let remote: Arc<dyn Remote> = cx.app.control.clone();
                link::unshare_id(&state, &remote, &link_id).await.map_err(refuse)?;
            }
        }
        // The block first, so nothing writes into the folder as it goes.
        if let Some(b) = block {
            cx.app.mux.api(|r| Api::Close(b, r)).await;
        }
        let id = meta.id;
        tokio::task::spawn_blocking(move || library::remove(&state, &id))
            .await
            .map_err(|e| OpError::Failed(e.to_string()))?
            .map_err(OpError::Failed)?;
        Ok(Empty {})
    }
}
