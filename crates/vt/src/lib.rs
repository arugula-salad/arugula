//! Server-side terminal state for a pane.
//!
//! [`VtEngine`] is what the daemon needs from a terminal emulator: feed it
//! PTY output, resize it, collect its replies to terminal queries, and take a
//! snapshot that reproduces the screen in a fresh terminal. The only
//! implementation is [`GhosttyEngine`] (libghostty-vt). See
//! `spikes/s1-ghostty/README.md` for why and for what the snapshot fixes up.

mod compat;
pub mod detect;
mod ghostty;
mod status_query;

pub use compat::Capabilities;
pub use ghostty::{CheckpointError, GhosttyEngine, engine_tag};

static XTVERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The release the terminals answer XTVERSION with ("arugula 0.45.0"). The
/// daemon sets it at start: this crate stays at version 0.0.0, so a release
/// bump doesn't rebuild it (arugula_core::version says why).
pub fn set_version(version: &str) {
    let _ = XTVERSION.set(format!("arugula {version}"));
}

fn xtversion() -> &'static str {
    XTVERSION.get().map_or("arugula 0.0.0", String::as_str)
}

pub trait VtEngine {
    /// Process PTY output.
    fn feed(&mut self, bytes: &[u8]);
    /// Change the terminal size (reflows the primary screen).
    fn resize(&mut self, cols: u16, rows: u16);
    fn size(&self) -> (u16, u16);
    /// Bytes the terminal wants written back to the PTY: answers to device
    /// attribute, status and color queries, limited to what the client
    /// renderer supports ([`Capabilities`]). Drained on each call.
    fn take_replies(&mut self) -> Vec<u8>;
    /// VT bytes that recreate the current state (both screens, scrollback,
    /// modes, cursor, title) when written to a fresh terminal of the same size.
    fn snapshot(&mut self) -> Vec<u8>;
    /// The same with at most `history` rows of scrollback, the newest
    /// (`None`: all of it), for a client that keeps no more than that.
    fn snapshot_history(&mut self, history: Option<usize>) -> Vec<u8>;
    /// The same without any scrollback: only what's on screen now (M13: a
    /// "from now" viewer never gets history from before they were let in).
    fn screen_snapshot(&mut self) -> Vec<u8>;
    /// Plain text of the active screen, including scrollback when the
    /// primary screen is active.
    fn plain_text(&self) -> String;
    /// The same with colors and styles as escape sequences (no modes or
    /// cursor), for `capture --ansi`.
    fn vt_text(&self) -> String;
    /// The same as HTML with inline styles.
    fn html(&self) -> String;
    fn title(&self) -> String;
    /// Whether a full-screen program's alternate screen is showing.
    fn alt_screen(&self) -> bool;
    /// Whether a DEC private mode (`CSI ? n h`) is set.
    fn dec_mode(&self, mode: u16) -> bool;
    fn pwd(&self) -> String;
    /// The screen's rows down to the last one with text (0 when blank): where
    /// output that mustn't overwrite anything can start.
    fn content_rows(&self) -> u16;
}
