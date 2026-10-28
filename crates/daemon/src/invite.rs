//! Inviting a person into a session (#233, #234) is the crate
//! `arugulad-invite`'s (#232): its routes, its card and MCP's
//! `invite_person`. What the base keeps is what its own API checks, and the
//! [`Hook`] the card runs an approved invite through.

use std::sync::{Arc, OnceLock, Weak};

use axum::http::HeaderMap;

use crate::server::App;

/// What runs an approved invite card (#234): blocks are made before the
/// server is, so it's set once it is.
pub type Hook = Arc<OnceLock<Weak<App>>>;

pub use crate::mux::{CLOSE_OWNER_ONLY, OWNER_ONLY};

/// Whether the owner's CLI says an agent runs it (as for a forge's drafts).
pub fn agent(headers: &HeaderMap) -> bool {
    headers.contains_key("x-arugula-agent")
}
