//! The mux's half of Claude Code's IDE (M28): what the relay tells it, the
//! diff for a card and the answers Claude Code reads. It names nothing of
//! the relay's side (the link, lock files, preferences).

use serde_json::{Value, json};

pub mod diff;

/// What we're called in Claude Code's `/ide` list.
pub const NAME: &str = "arugula";

/// What the relay tells the daemon.
#[derive(Debug, Clone)]
pub enum Event {
    /// The relay (again): what follows is everything open.
    Hello,
    /// A Claude Code connected; the process it is.
    Conn {
        conn: u64,
        pid: Option<u32>,
    },
    Gone {
        conn: u64,
    },
    /// A tool call that waits for an answer.
    Call {
        conn: u64,
        id: Value,
        tool: String,
        args: Value,
    },
    /// Claude Code closed these diffs (its terminal answered).
    Closed {
        conn: u64,
        ids: Vec<Value>,
    },
}

/// `openDiff`'s answers, as Claude Code reads them.
pub fn saved(contents: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": "FILE_SAVED" }, { "type": "text", "text": contents }] })
}

pub fn rejected(tab: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": "DIFF_REJECTED" }, { "type": "text", "text": tab }] })
}

/// `getDiagnostics` with nothing to say.
pub fn no_diagnostics() -> Value {
    json!({ "content": [{ "type": "text", "text": "[]" }] })
}
