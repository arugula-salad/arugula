//! OSC 7501's feature query (#720): a program asks `OSC 7501 ; ? ST`
//! whether the terminal reads program status, and gets the same `?` back.
//! That reply is the only thing the protocol ever writes to a program;
//! the reports themselves are read by the mux (`arugula_mux::osc`).
//!
//! libghostty doesn't know the sequence, so the engine finds the query in
//! the output itself and answers it in order with libghostty's own
//! replies: a program sends the query and then a primary device
//! attributes request, and reads a DA reply that comes first as "not
//! supported".

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    #[default]
    Ground,
    Esc,
    Osc,
    OscEsc,
}

/// The query's body, and the most of an OSC kept to compare with it.
const QUERY: &[u8] = b"7501;";
const KEEP: usize = 16;

/// Finds queries across reads.
#[derive(Debug, Default)]
pub(crate) struct StatusQuery {
    state: State,
    buf: Vec<u8>,
}

impl StatusQuery {
    /// Where in `data` each query ends (just past its terminator), with the
    /// reply, ended the way the query was.
    pub(crate) fn feed(&mut self, data: &[u8]) -> Vec<(usize, &'static [u8])> {
        let mut out = vec![];
        for (i, &b) in data.iter().enumerate() {
            self.state = match (self.state, b) {
                (State::Ground, 0x1b) => State::Esc,
                (State::Ground, _) => State::Ground,
                (State::Esc, b']') => {
                    self.buf.clear();
                    State::Osc
                }
                (State::Esc, 0x1b) => State::Esc,
                (State::Esc, _) => State::Ground,
                (State::Osc, 0x07) => {
                    if self.is_query() {
                        out.push((i + 1, &b"\x1b]7501;?\x07"[..]));
                    }
                    State::Ground
                }
                (State::Osc, 0x1b) => State::OscEsc,
                (State::Osc, 0x18 | 0x1a) => State::Ground,
                (State::Osc, b) => {
                    if self.buf.len() <= KEEP {
                        self.buf.push(b);
                    }
                    State::Osc
                }
                (State::OscEsc, b'\\') => {
                    if self.is_query() {
                        out.push((i + 1, &b"\x1b]7501;?\x1b\\"[..]));
                    }
                    State::Ground
                }
                (State::OscEsc, b']') => {
                    self.buf.clear();
                    State::Osc
                }
                (State::OscEsc, _) => State::Ground,
            };
        }
        out
    }

    fn is_query(&self) -> bool {
        self.buf.len() <= KEEP && self.buf.strip_prefix(QUERY).is_some_and(|rest| rest.trim_ascii() == b"?")
    }
}

#[cfg(test)]
mod tests {
    use super::StatusQuery;
    use crate::{GhosttyEngine, VtEngine};

    #[test]
    fn the_query_is_answered_with_either_terminator() {
        let mut q = StatusQuery::default();
        assert_eq!(q.feed(b"ab\x1b]7501;?\x07"), vec![(11, &b"\x1b]7501;?\x07"[..])]);
        assert_eq!(q.feed(b"\x1b]7501;?\x1b\\"), vec![(10, &b"\x1b]7501;?\x1b\\"[..])]);
        // Split across reads.
        assert_eq!(q.feed(b"\x1b]75"), vec![]);
        assert_eq!(q.feed(b"01;?\x1b"), vec![]);
        assert_eq!(q.feed(b"\\"), vec![(1, &b"\x1b]7501;?\x1b\\"[..])]);
        // Not queries.
        assert_eq!(q.feed(b"\x1b]7501;state=working\x07\x1b]7501;??\x07\x1b]17501;?\x07"), vec![]);
    }

    #[test]
    fn the_engine_answers_before_the_device_attributes_that_follow() {
        let mut e = GhosttyEngine::new(80, 24);
        e.feed(b"\x1b]7501;?\x1b\\\x1b[c");
        let replies = e.take_replies();
        assert!(replies.starts_with(b"\x1b]7501;?\x1b\\\x1b[?"), "{:?}", String::from_utf8_lossy(&replies));
    }

    #[test]
    fn nothing_from_a_report_is_written_back() {
        let mut e = GhosttyEngine::new(80, 24);
        e.feed(b"\x1b]7501;state=blocked:kind=question:msg=U2hpcCBpdCB0byBwcm9kPw==\x07");
        e.feed(b"\x1b]7501;state=working:id=a/b\x1b\\\x1b]7501;state=clear\x07");
        assert_eq!(e.take_replies(), b"");
    }
}
