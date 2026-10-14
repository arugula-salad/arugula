//! `arugula attention`: what wants you, or tell Arugula a pane needs you.

use super::Ctx;
use crate::http::{request, request_op};
use crate::util::{Pane, here, print_json, snake};
use arugula_proto::{
    api::{AttentionItem, Empty},
    op::ops::PaneAttention,
};
use serde_json::{Value, json};

/// The longest turn summary sent, in characters.
const SUMMARY_MAX: usize = 300;

/// What a hook says, when its JSON is on stdin: a `Notification`'s
/// `message` ("Claude needs your permission to use Bash"), or for `Stop`
/// (#857) the turn's last assistant message, as a summary of it: Claude
/// Code's `last_assistant_message`, or else the last one in its
/// `transcript_path`.
fn hook_message() -> Option<String> {
    use std::io::{IsTerminal, Read};
    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut input = String::new();
    std::io::stdin().take(1 << 20).read_to_string(&mut input).ok()?;
    let v: Value = serde_json::from_str(&input).ok()?;
    if v["hook_event_name"] == "Stop" {
        return stop_summary(&v);
    }
    v["message"].as_str().map(str::to_owned).filter(|m| !m.is_empty())
}

/// A `Stop` hook's turn summary: its last assistant message, on one line.
fn stop_summary(hook: &Value) -> Option<String> {
    let text = match hook["last_assistant_message"].as_str().filter(|m| !m.trim().is_empty()) {
        Some(m) => m.to_owned(),
        None => last_in_transcript(std::path::Path::new(hook["transcript_path"].as_str()?))?,
    };
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= SUMMARY_MAX {
        return Some(one);
    }
    Some(one.chars().take(SUMMARY_MAX - 1).collect::<String>() + "\u{2026}")
}

/// The text of the last assistant message in a Claude Code transcript
/// (JSONL), read from its last megabyte.
fn last_in_transcript(path: &std::path::Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(1 << 20))).ok()?;
    let mut tail = Vec::new();
    f.read_to_end(&mut tail).ok()?;
    last_assistant_text(&String::from_utf8_lossy(&tail))
}

fn last_assistant_text(jsonl: &str) -> Option<String> {
    jsonl.lines().rev().find_map(|l| {
        let v: Value = serde_json::from_str(l).ok()?;
        if v["type"] != "assistant" {
            return None;
        }
        let text: Vec<&str> = match &v["message"]["content"] {
            Value::String(s) => vec![s.as_str()],
            Value::Array(parts) => {
                parts.iter().filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str()).collect()
            }
            _ => Vec::new(),
        };
        let text = text.join("\n");
        (!text.trim().is_empty()).then_some(text)
    })
}

#[derive(clap::Args)]
pub struct Args {
    /// needs-input, done, working or idle; none lists what wants you.
    state: Option<String>,
    #[arg(long)]
    pane: Option<Pane>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    match args {
        Args { state: None, .. } => {
            let (items, v) = request(&sock, "GET", "/api/attention", None)?.parse_raw::<Vec<AttentionItem>>()?;
            if json_out {
                print_json(&v);
                return Ok(0);
            }
            for i in &items {
                let r = &i.reason;
                let bundle = r.bundle.as_ref().map(|b| format!("  [{b}]")).unwrap_or_default();
                println!("%{} {:<7} {}{bundle}", i.pane, snake(&r.kind), r.headline);
            }
        }
        Args { state: Some(state), pane } => {
            // The plugin's copy of a hook settings.json has too (#229).
            if pane.is_none() && crate::hooks::plugin_copy_stands_aside(&format!("arugula attention {state}"), None) {
                let _ = hook_message();
                return Ok(0);
            }
            let state = state.replace('-', "_");
            // Hooks (Claude Code's, say) run this in every terminal; outside an
            // Arugula pane there's nobody to tell, and that's fine.
            let Ok(pane) = here(pane) else { return Ok(0) };
            // The state stays the string typed here: the daemon names the
            // ones it doesn't know, as it always did.
            request_op::<PaneAttention>(&sock, &pane, &json!({"state": state, "why": hook_message()}))?
                .parse::<Empty>()?;
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stop_hook_s_summary_is_its_last_assistant_message() {
        // Claude Code 2.1's Stop input carries it.
        let hook = json!({ "hook_event_name": "Stop", "stop_hook_active": false,
            "last_assistant_message": "Fixed `mean()`.\n\nAll 4 tests pass.", "transcript_path": "/nonexistent" });
        assert_eq!(stop_summary(&hook).as_deref(), Some("Fixed `mean()`. All 4 tests pass."));
        // Else the transcript's last one with text (a tool call has none).
        let jsonl = [
            json!({ "type": "user", "message": { "role": "user", "content": "fix it" } }),
            json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "Fixed it." }] } }),
            json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Bash" }] } }),
            json!({ "type": "system", "subtype": "stop_hook_summary" }),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
        assert_eq!(last_assistant_text(&jsonl).as_deref(), Some("Fixed it."));
        let long = json!({ "hook_event_name": "Stop", "last_assistant_message": "x".repeat(1000) });
        assert_eq!(stop_summary(&long).unwrap().chars().count(), SUMMARY_MAX);
        assert_eq!(stop_summary(&json!({ "hook_event_name": "Stop", "transcript_path": "/nonexistent" })), None);
    }
}
