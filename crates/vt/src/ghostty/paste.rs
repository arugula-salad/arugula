//! Pasted text, encoded for the modes the pane's program has set
//! (bracketed paste).

use libghostty_vt::{
    paste,
    terminal::{Mode, ModeKind},
};

use super::GhosttyEngine;

impl GhosttyEngine {
    /// Pasted text: bracketed if the program asked for that, otherwise with
    /// newlines as Enter; control characters that could end a bracket are
    /// made harmless either way.
    pub fn encode_paste(&self, text: &str) -> Vec<u8> {
        let bracketed = self.term.mode(Mode::new(2004, ModeKind::Dec)).unwrap_or(false);
        let mut data = text.as_bytes().to_vec();
        let mut buf = vec![0u8; data.len() + 16];
        loop {
            match paste::encode(&mut data, bracketed, &mut buf) {
                Ok(n) => {
                    buf.truncate(n);
                    return buf;
                }
                Err(libghostty_vt::Error::OutOfSpace { required }) => buf.resize(required, 0),
                Err(_) => return Vec::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VtEngine;

    #[test]
    fn paste_is_bracketed_when_asked() {
        let mut e = GhosttyEngine::new(80, 24);
        assert_eq!(e.encode_paste("a\nb"), b"a\rb");
        e.feed(b"\x1b[?2004h");
        assert_eq!(e.encode_paste("a\nb"), b"\x1b[200~a\nb\x1b[201~");
    }
}
