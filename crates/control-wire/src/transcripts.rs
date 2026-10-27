//! Agent transcripts from hosted workspaces (M184, #1245). An agent block's
//! transcript lives on its box's disk while the block is open. When the
//! block closes, the hosted daemon sends it to [`HOSTED_TRANSCRIPTS`]
//! (signed as its hosted machine), and the platform keeps it read-only
//! under the workspace's project, for the project's people to read after
//! the box is gone.
//!
//! What's sent is the block's text (`capture --text`: its prompts, the
//! agent's messages and tool calls), cut to [`MAX_TEXT`] bytes by the
//! daemon: the start and the end kept, the middle dropped and said so.

use serde::{Deserialize, Serialize};

/// A hosted machine archives a closed agent block's transcript (`POST`,
/// signed as the machine), [`TranscriptPost`], answered
/// [`TranscriptArchived`].
pub const HOSTED_TRANSCRIPTS: &str = "/api/hosted/transcripts";

/// The most text one transcript keeps, in bytes: under the hub's limit on a
/// signed body (1 MiB) with room for the rest.
pub const MAX_TEXT: usize = 768 * 1024;

/// What a hosted machine sends as an agent block closes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptPost {
    /// The block's id on the machine.
    pub block: u64,
    /// `claude`, `codex` or `acp`.
    #[serde(default)]
    pub agent: String,
    /// What the block was called.
    #[serde(default)]
    pub title: String,
    /// Where it worked, on the box.
    #[serde(default)]
    pub cwd: Option<String>,
    /// When the block was made and closed (the machine's clock, ms).
    #[serde(default)]
    pub started_at: u64,
    #[serde(default)]
    pub closed_at: u64,
    /// The middle of the text was dropped to fit [`MAX_TEXT`].
    #[serde(default)]
    pub cut: bool,
    pub text: String,
}

/// `hosted.archive` (Arugula #209), control to a hosted machine through
/// its relay: send every open agent block's transcript now, as closing it
/// would, and answer once each send is done. The platform asks just before
/// it rebuilds the box, so a session that moves to the new box starts
/// from its conversation, not without it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveOpen {}

/// `hosted.archive_blocks` (Arugula #214): [`ArchiveOpen`] for these agent
/// blocks only. The platform asks before Move to… takes a session off a
/// box that stays up: its conversation goes with it, and the box's other
/// blocks are archived when they close, as they end (one transcript per
/// block is kept). A daemon from before it answers that it doesn't know
/// the op.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveBlocks {
    pub blocks: Vec<u64>,
}

/// The platform kept it: its id (`htx_…`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptArchived {
    pub id: String,
}

/// `text` cut to at most `max` bytes (on char boundaries): its first
/// sixteenth and its end, with a line saying how much was dropped between.
/// Whether it was cut.
pub fn fit(text: &str, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text.to_owned(), false);
    }
    let floor = |mut i: usize| {
        while !text.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    let note_room = 64;
    let head = floor(max / 16);
    let tail_len = max.saturating_sub(head + note_room);
    let mut tail = text.len() - tail_len;
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    let dropped = tail - head;
    (format!("{}\n\n[… {dropped} bytes cut …]\n\n{}", &text[..head], &text[tail..]), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_transcript_keeps_its_start_and_end() {
        assert_eq!(fit("short", 100), ("short".to_owned(), false));
        let text = format!("PROMPT {}é END", "x".repeat(10_000));
        let (cut, was) = fit(&text, 1000);
        assert!(was && cut.len() <= 1000, "{}", cut.len());
        assert!(cut.starts_with("PROMPT") && cut.ends_with("é END"));
        assert!(cut.contains("bytes cut"));
    }

    #[test]
    fn a_post_reads_leniently() {
        let p: TranscriptPost = serde_json::from_value(serde_json::json!({ "block": 3, "text": "hi" })).unwrap();
        assert_eq!((p.block, p.agent.as_str(), p.cut), (3, "", false));
    }

    /// `hosted.archive` is its name alone on the relay.
    #[test]
    fn the_archive_op_on_the_wire() {
        use crate::forge::{DaemonOp, OpAsk};
        let ask = OpAsk::new("1".into(), DaemonOp::HostedArchive(ArchiveOpen::default()));
        let v = serde_json::to_value(&ask).unwrap();
        assert_eq!(v, serde_json::json!({ "t": "op", "id": "1", "op": { "op": "hosted.archive" } }));
        assert_eq!(serde_json::from_value::<OpAsk>(v).unwrap(), ask);
        assert!(crate::forge::Ops::ours().ops.contains(&ask.op.name().to_owned()));
    }
}
