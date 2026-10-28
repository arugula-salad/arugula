//! Feature crates (`arugulad-<area>`, #232): what each adds to the daemon,
//! registered by the binary before `main`. The base calls these hooks
//! where it used to name the feature; a feature that isn't registered
//! adds nothing. (The editor's surface is `editors.rs`, older than this.)
//! A feature's operations are `Handle<Mine>`, routed in its [`routes`]
//! with `op_of`, and its MCP tools are `mcp::registry`'s.

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use arugula_proto::api::SearchHit;
use axum::Router;
use futures_util::future::BoxFuture;

use crate::{block::BlockKinds, control::Control, invite::Hook, server::App};

/// What a feature crate adds.
pub trait Feature: Send + Sync + 'static {
    /// Before the layout is restored: what it keeps in the state directory,
    /// moved to where it lives now.
    fn prepare(&self, _state_dir: &Path) {}
    /// Once control is there (before any block is made).
    fn with_control(&self, _control: &Arc<Control>) {}
    /// Its block types. `app` is the app once it's made (blocks are made
    /// before the server is).
    fn kinds(&self, _kinds: &mut BlockKinds, _app: &Hook) {}
    /// Its routes and operations, beside the API's.
    fn routes(&self, r: Router<Arc<App>>) -> Router<Arc<App>> {
        r
    }
    /// `search`'s lines of what it keeps outside panes, after the panes'.
    fn search(
        &self,
        _app: Arc<App>,
        _re: regex::Regex,
        _since_ms: Option<u64>,
        _limit: usize,
    ) -> BoxFuture<'static, Vec<SearchHit>> {
        Box::pin(async { vec![] })
    }
}

static ALL: Mutex<Vec<&'static dyn Feature>> = Mutex::new(Vec::new());

/// Puts a feature in this daemon: the binary calls it before `main`.
pub fn register(f: Box<dyn Feature>) {
    ALL.lock().unwrap().push(Box::leak(f));
}

fn all() -> Vec<&'static dyn Feature> {
    ALL.lock().unwrap().clone()
}

pub(crate) fn prepare(state_dir: &Path) {
    for f in all() {
        f.prepare(state_dir);
    }
}

pub(crate) fn with_control(control: &Arc<Control>) {
    for f in all() {
        f.with_control(control);
    }
}

pub(crate) fn kinds(kinds: &mut BlockKinds, app: &Hook) {
    for f in all() {
        f.kinds(kinds, app);
    }
}

pub(crate) fn routes(mut r: Router<Arc<App>>) -> Router<Arc<App>> {
    for f in all() {
        r = f.routes(r);
    }
    r
}

/// Each feature's `search` lines, in turn, up to `limit` in all.
pub(crate) async fn search(app: &Arc<App>, re: regex::Regex, since_ms: Option<u64>, limit: usize) -> Vec<SearchHit> {
    let mut out = Vec::new();
    for f in all() {
        if out.len() >= limit {
            break;
        }
        out.extend(f.search(app.clone(), re.clone(), since_ms, limit - out.len()).await);
    }
    out
}
