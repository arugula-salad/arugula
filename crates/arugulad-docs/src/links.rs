//! `doc_link.get` and `doc_link.delete` (#1066): the account's list of
//! shared Docs copies a link or unshares one through the machine that made
//! it. The owner's, and never an agent's (the key would be read back, or a
//! link taken down), as the Doc block's own methods are.

use std::sync::Arc;

use arugula_proto::{
    BlockType,
    api::{DocLinkUrl, DocShareRequest, Empty},
    op::ops::{DocLinkDelete, DocLinkGet, DocShare},
};
use axum::http::StatusCode;

use crate::{
    Docs,
    docs::{
        self, library,
        link::{self, AGENT_ASKS, OWNER_ONLY, Remote},
    },
    mux::Api,
    ops::{Cx, Handle, OpError},
};

fn id_of(id: &str) -> Result<(), OpError> {
    if link::is_id(id) {
        Ok(())
    } else {
        Err(OpError::Status(StatusCode::BAD_REQUEST, "a link id is 32 lower-case hex digits".into()))
    }
}

impl Handle<Docs> for DocLinkGet {
    async fn handle(cx: &Cx<'_>, id: String, _: Empty) -> Result<DocLinkUrl, OpError> {
        if cx.agent_or_guest() {
            return Err(OpError::Forbidden(AGENT_ASKS.into()));
        }
        if cx.co_owner() {
            return Err(OpError::Forbidden(OWNER_ONLY.into()));
        }
        id_of(&id)?;
        match link::url_of(cx.app.control.state_dir(), &id) {
            Ok(Some(url)) => Ok(DocLinkUrl { url }),
            Ok(None) => Err(OpError::Status(StatusCode::NOT_FOUND, "this machine didn't share that Doc".into())),
            Err(e) => Err(OpError::Failed(e)),
        }
    }
}

impl Handle<Docs> for DocLinkDelete {
    async fn handle(cx: &Cx<'_>, id: String, _: Empty) -> Result<Empty, OpError> {
        if cx.agent_or_guest() {
            return Err(OpError::Forbidden(AGENT_ASKS.into()));
        }
        if cx.co_owner() {
            return Err(OpError::Forbidden(OWNER_ONLY.into()));
        }
        id_of(&id)?;
        let dir = cx.app.control.state_dir();
        let why = |e: String| OpError::Status(StatusCode::BAD_GATEWAY, e);
        let Some(entry) = link::get(dir, &id).map_err(OpError::Failed)? else {
            return Err(OpError::Status(StatusCode::NOT_FOUND, "this machine didn't share that Doc".into()));
        };
        // The block that shows its Doc, if one is open and still holds this
        // link, does it all and clears its `shared`; otherwise only the key
        // goes.
        let block = match &entry.doc {
            Some(doc) => crate::doc_ops::open_blocks(cx.app).await.get(doc).copied(),
            None => None,
        };
        if let Some(b) = match block {
            Some(b) => cx.app.mux.api(|r| Api::Block(b, r)).await.flatten(),
            None => None,
        } && b.kind() == BlockType::Doc
        {
            let done = b.call("doc_unshare", serde_json::json!({ "id": id })).await.map_err(why)?;
            if done["other"] != true {
                return Ok(Empty {});
            }
        }
        let remote: Arc<dyn Remote> = cx.app.control.clone();
        link::unshare_id(dir, &remote, &id).await.map_err(why)?;
        Ok(Empty {})
    }
}

impl Handle<Docs> for DocShare {
    /// Share a Doc, or update its link, with no pane opened (Arugula #819).
    /// A block that shows it does the work, so its `shared` is right at
    /// once; otherwise the library does, by the same `share`.
    async fn handle(cx: &Cx<'_>, id: String, req: DocShareRequest) -> Result<DocLinkUrl, OpError> {
        if cx.agent_or_guest() {
            return Err(OpError::Forbidden(AGENT_ASKS.into()));
        }
        if cx.co_owner() {
            return Err(OpError::Forbidden(OWNER_ONLY.into()));
        }
        let dir = cx.app.control.state_dir();
        let meta = library::dir(dir, &id)
            .map_err(|e| OpError::Status(StatusCode::BAD_REQUEST, e))
            .map(|d| library::read_meta(&d))?;
        if meta.is_none() {
            return Err(OpError::Status(StatusCode::NOT_FOUND, format!("no Doc {id} in the library")));
        }
        // One at a time, so two shares of a closed Doc can't both find no
        // link and make two.
        static ONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _one = ONE.lock().await;
        let why = |e: String| OpError::Status(StatusCode::BAD_GATEWAY, e);
        let block = crate::doc_ops::open_blocks(cx.app).await.get(&id).copied();
        let open = match block {
            Some(b) => cx.app.mux.api(|r| Api::Block(b, r)).await.flatten().filter(|b| b.kind() == BlockType::Doc),
            None => None,
        };
        let got = match open {
            Some(b) => b.call(if req.update { "doc_share_update" } else { "doc_share" }, serde_json::json!({})).await,
            None => docs::share_kept(dir.to_owned(), id, req.update).await,
        };
        let v = got.map_err(why)?;
        let url = v["url"].as_str().ok_or_else(|| OpError::Failed("the share gave no link".into()))?;
        Ok(DocLinkUrl { url: url.to_owned() })
    }
}
