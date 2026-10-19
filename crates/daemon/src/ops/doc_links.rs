//! `doc_link.get` and `doc_link.delete` (#1066): the account's list of
//! shared Docs copies a link or unshares one through the machine that made
//! it. The owner's, and never an agent's (the key would be read back, or a
//! link taken down), as the Doc block's own methods are.

use std::sync::Arc;

use arugula_proto::{
    BlockType,
    api::{DocLinkUrl, Empty},
    op::ops::{DocLinkDelete, DocLinkGet},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError};
use crate::{
    docs::link::{self, AGENT_ASKS, Remote},
    mux::Api,
};

fn id_of(id: &str) -> Result<(), OpError> {
    if link::is_id(id) {
        Ok(())
    } else {
        Err(OpError::Status(StatusCode::BAD_REQUEST, "a link id is 32 lower-case hex digits".into()))
    }
}

impl Handle for DocLinkGet {
    async fn handle(cx: &Cx<'_>, id: String, _: Empty) -> Result<DocLinkUrl, OpError> {
        if cx.agent_or_guest() {
            return Err(OpError::Forbidden(AGENT_ASKS.into()));
        }
        id_of(&id)?;
        match link::url_of(cx.app.control.state_dir(), &id) {
            Ok(Some(url)) => Ok(DocLinkUrl { url }),
            Ok(None) => Err(OpError::Status(StatusCode::NOT_FOUND, "this machine didn't share that Doc".into())),
            Err(e) => Err(OpError::Failed(e)),
        }
    }
}

impl Handle for DocLinkDelete {
    async fn handle(cx: &Cx<'_>, id: String, _: Empty) -> Result<Empty, OpError> {
        if cx.agent_or_guest() {
            return Err(OpError::Forbidden(AGENT_ASKS.into()));
        }
        id_of(&id)?;
        let dir = cx.app.control.state_dir();
        let why = |e: String| OpError::Status(StatusCode::BAD_GATEWAY, e);
        let Some(entry) = link::get(dir, &id).map_err(OpError::Failed)? else {
            return Err(OpError::Status(StatusCode::NOT_FOUND, "this machine didn't share that Doc".into()));
        };
        // Its block, if it is open and still holds this link, does it all
        // and clears its `shared`; otherwise only the key goes.
        if let Some(b) = cx.app.mux.api(|r| Api::Block(entry.block, r)).await.flatten()
            && b.kind() == BlockType::Doc
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
