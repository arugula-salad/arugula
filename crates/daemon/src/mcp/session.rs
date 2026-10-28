//! A hosted session's MCP (aru-115, amended): the tools a session's agent
//! has for its project's native issues and Docs (aru-118), on a listener of
//! their own, which `arugulad-mcp`'s `session` serves. Here: their names, and
//! the entry a session's agent is given ([`entry`]).

use std::sync::OnceLock;

use serde_json::{Value, json};

pub const READ: &str = "read_session_issues";
pub const COMMENT: &str = "comment_session_issue";
pub const CREATE: &str = "create_session_issue";
pub const CREATE_DOC: &str = "create_session_doc";
pub const FILL_DOC: &str = "fill_session_doc";
pub const READ_DOC: &str = "read_project_doc";

static URL: OnceLock<String> = OnceLock::new();

/// Where a session's agent reaches its MCP, if this box serves one.
pub fn url() -> Option<&'static str> {
    URL.get().map(String::as_str)
}

/// The session server is up at `url` (once).
pub fn serving(url: String) {
    let _ = URL.set(url);
}

/// The server entry a session's agent is given: its MCP over HTTP, if the
/// agent takes that (`http`) and this box serves one.
pub fn entry(http: bool) -> Option<Value> {
    entry_at(http, url())
}

fn entry_at(http: bool, url: Option<&str>) -> Option<Value> {
    let url = url.filter(|_| http)?;
    Some(json!({ "type": "http", "name": super::SERVER_NAME, "url": url, "headers": [] }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Over HTTP and only when there is one; and nothing in it to leak.
    #[test]
    fn a_sessions_agent_gets_the_session_server_over_http_only() {
        let e = entry_at(true, Some("http://127.0.0.1:41000/mcp")).unwrap();
        assert_eq!(e["url"], "http://127.0.0.1:41000/mcp");
        assert_eq!(
            (e["type"].as_str(), e["name"].as_str(), e["headers"].as_array().map(Vec::len)),
            (Some("http"), Some("arugula"), Some(0))
        );
        assert!(
            entry_at(false, Some("http://127.0.0.1:41000/mcp")).is_none(),
            "an agent without HTTP gets none (no stdio bridge to the daemon)"
        );
        assert!(entry_at(true, None).is_none(), "a box with no session server");
    }
}
