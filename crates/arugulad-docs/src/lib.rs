//! arugulad-docs: Docs (#963, #1493, #1066). The Doc block (`docs/`), its
//! library and its links, their operations (`ops.rs`: `docs.list`,
//! `doc.open`, `doc.delete`; `links.rs`: `doc_link.get`, `doc_link.delete`)
//! and the `doc` MCP tool (`tool.rs`). The binary calls [`register`]; the
//! daemon (`arugulad-base`) reaches it only through its `features` hooks and
//! `mcp::registry`, so an edit here rebuilds this crate and the binary, not
//! the daemon (#232). Who may share a Doc is said in the base
//! (`doc_shares`), whose API checks a Doc block's sharing methods.

#[path = "ops.rs"]
mod doc_ops;
mod docs;
mod links;
mod tool;

use std::{path::Path, sync::Arc};

use arugula_proto::{
    BlockType,
    api::SearchHit,
    op::ops::{DocDelete, DocLinkDelete, DocLinkGet, DocOpen, DocsList},
};
use axum::Router;
use futures_util::future::BoxFuture;

// What the code here names as `crate::…`: the mux's modules and the daemon's,
// as it named them when it was the daemon's.
use arugula_mux::{acl, block, mux, store};
use arugulad_base::{agent, control, mcp, ops, push, server, task};
// ...and what its tests name.
#[cfg(test)]
use arugula_mux::{pane, shellenv};
#[cfg(test)]
use arugulad_base::fs;

use crate::{ops::OpRoutes, server::App};

/// Whose the Docs' operations are (`impl Handle<Docs> for DocsList`): this
/// crate's, so it may handle `arugula-proto`'s operation types.
pub struct Docs;

/// Puts Docs in the daemon. Call it before `arugulad_base::main`.
pub fn register() {
    arugulad_base::features::register(Box::new(Docs));
    tool::register();
}

impl arugulad_base::features::Feature for Docs {
    fn prepare(&self, state_dir: &Path) {
        // The Docs of the old layout move into the library.
        docs::library::migrate(state_dir);
    }

    fn with_control(&self, control: &Arc<control::Control>) {
        // Doc blocks share through control (#1066).
        docs::link::init(control.clone());
    }

    fn kinds(&self, kinds: &mut block::BlockKinds, _app: &arugulad_base::invite::Hook) {
        kinds.add(BlockType::Doc, docs::DocKind);
    }

    fn routes(&self, r: Router<Arc<App>>) -> Router<Arc<App>> {
        r.op_of::<DocLinkGet, Docs>()
            .op_of::<DocsList, Docs>()
            .op_of::<DocOpen, Docs>()
            .op_of::<DocDelete, Docs>()
            .op_of::<DocLinkDelete, Docs>()
    }

    fn search(
        &self,
        app: Arc<App>,
        re: regex::Regex,
        since_ms: Option<u64>,
        limit: usize,
    ) -> BoxFuture<'static, Vec<SearchHit>> {
        Box::pin(async move { doc_ops::search_library(&app, re, since_ms, limit).await })
    }
}
