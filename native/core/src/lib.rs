//! What Arugula's native clients share under their native screens: signing
//! in, the encrypted channel to a daemon, a pane's stream and the terminal
//! that draws it (aru-132). Screens don't belong here.
//!
//! - [`control`]: signing in with the account (#999), and requests to
//!   control with the session.
//! - [`chat_live`]: chat's live stream, what to fetch again as it happens.
//! - [`link`]: a Noise channel to a machine through control's relay, opened
//!   with a machine token, its requests and its panes.
//! - [`shown`]: which sessions show in the current project, and the
//!   machines in order, as the web's (#1355).
//! - [`Pane`]: one pane's terminal.
//!
//! Swift reaches it through [`ffi`]'s C functions, and Kotlin through
//! `kotlin`'s JNI over the same functions (the `jni` feature).
//!
//! The session stays in the platform's keychain, so nothing here reads or
//! writes a file: [`control::Session`] crosses as JSON.

pub mod chat_live;
pub mod chief;
pub mod control;
pub mod ffi;
#[cfg(feature = "jni")]
pub mod kotlin;
pub mod link;
mod net;
pub mod shown;

use anyhow::Result;
use libghostty_vt::{
    Terminal,
    key::{self, Key, Mods},
};

pub use arugula_control_wire as wire;
pub use arugula_proto as proto;
pub use libghostty_vt;

/// One pane's terminal on the client, fed its snapshot and output and read by
/// a renderer. It never answers terminal queries: the daemon's terminal does.
pub struct Pane {
    pub term: Terminal<'static, 'static>,
    encoder: key::Encoder<'static>,
    event: key::Event<'static>,
    buf: Vec<u8>,
}

impl Pane {
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        let mut term = Terminal::new(cols, rows)?;
        term.set_scrollback_max_lines(Some(10_000))?;
        Ok(Self { term, encoder: key::Encoder::new()?, event: key::Event::new()?, buf: Vec::with_capacity(64) })
    }

    /// Start over from the daemon's snapshot of the pane.
    pub fn snapshot(&mut self, bytes: &[u8]) {
        self.term.reset();
        self.term.vt_write(bytes);
    }

    pub fn output(&mut self, bytes: &[u8]) {
        self.term.vt_write(bytes);
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell_w: u32, cell_h: u32) -> Result<()> {
        self.term.resize(cols, rows, cell_w, cell_h)?;
        Ok(())
    }

    /// The bytes for a key press or release under the terminal's current
    /// modes, or none. `text` is what the keyboard produced.
    pub fn key(&mut self, k: Key, press: bool, mods: Mods, text: Option<&str>) -> Result<&[u8]> {
        self.buf.clear();
        let unshifted =
            text.and_then(|t| t.chars().next()).filter(|c| !c.is_control()).map_or('\0', |c| c.to_ascii_lowercase());
        let consumed = if unshifted != '\0' && mods.contains(Mods::SHIFT) { Mods::SHIFT } else { Mods::empty() };
        self.event
            .set_action(if press { key::Action::Press } else { key::Action::Release })
            .set_key(k)
            .set_mods(mods)
            .set_consumed_mods(consumed)
            .set_unshifted_codepoint(unshifted)
            .set_utf8(text.filter(|t| !t.chars().any(char::is_control)));
        self.encoder.set_options_from_terminal(&self.term).encode_to_vec(&self.event, &mut self.buf)?;
        Ok(&self.buf)
    }
}

#[cfg(test)]
mod tests {
    fn pin(manifest: &str) -> &str {
        let line = manifest.lines().find(|l| l.starts_with("libghostty-vt =")).expect("a libghostty-vt line");
        line.split("rev = \"").nth(1).and_then(|r| r.split('"').next()).expect("a rev")
    }

    #[test]
    fn the_terminal_is_the_daemons_engine() {
        assert_eq!(pin(include_str!("../Cargo.toml")), pin(include_str!("../../../crates/vt/Cargo.toml")));
    }
}
