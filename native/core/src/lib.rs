//! What Arugula's native clients share under their native screens: signing a
//! request, the encrypted channel to a daemon, a pane's stream and the
//! terminal that draws it (aru-132). Screens don't belong here.
//!
//! - [`control`]: pairing by code, and requests to control, signed.
//! - [`link`]: a Noise channel to a machine through control's relay, its
//!   requests and its panes.
//! - [`Pane`]: one pane's terminal.
//!
//! A client's keys stay in the platform's keychain, so nothing here reads or
//! writes a key file: [`device`] takes them as bytes (aru-131).

pub mod control;
pub mod ffi;
pub mod link;
mod net;

use anyhow::Result;
use ed25519_dalek::SigningKey;
use libghostty_vt::{
    Terminal,
    key::{self, Key, Mods},
};

pub use arugula_control_wire as wire;
pub use arugula_e2e::{
    self as e2e,
    cert::request_auth,
    channel::{Channel, Initiator, Msg, prologue},
    keys::DeviceKeys,
};
pub use arugula_proto as proto;
pub use libghostty_vt;

/// A device's keys from the bytes the keychain holds.
pub fn device(noise_private: [u8; 32], noise_public: [u8; 32], sign_seed: [u8; 32]) -> DeviceKeys {
    DeviceKeys { noise_private, noise_public, sign: SigningKey::from_bytes(&sign_seed) }
}

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
