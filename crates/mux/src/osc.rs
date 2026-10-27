//! Picks the structure out of a pane's output: where prompts and commands
//! begin and end (OSC 133, and VS Code's OSC 633 with the command line), the
//! working directory (OSC 7), notifications (OSC 9, OSC 777, OSC 99), the
//! bell, program status reports (OSC 7501, #720), a full reset (RIS), and a
//! link to a pull request or an issue printed in it (plain, or OSC 8). The
//! bytes themselves pass through untouched; this only watches.
//!
//! It is a small state machine that survives sequences split across reads,
//! and it ignores BEL inside strings (an OSC's terminator, or DCS/APC data).

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Signal {
    /// A prompt is being drawn (133;A).
    Prompt,
    /// The command line, as the shell integration reports it (633;E).
    CommandLine {
        text: String,
    },
    /// The command started running (133;C).
    CommandStart,
    /// The command finished (133;D[;exit]).
    CommandEnd {
        exit: Option<i32>,
    },
    /// The working directory (OSC 7 `file://host/path`).
    Cwd {
        path: String,
    },
    /// A program asked for attention (OSC 9, OSC 777 notify, OSC 99).
    Notify {
        title: String,
        body: String,
    },
    Bell,
    /// A program says what it's doing (OSC 7501, #720).
    Status(Status),
    /// A full reset (RIS, `ESC c`): it clears every status record.
    Reset,
    /// A link shaped like a pull request's or an issue's was printed
    /// (`gh pr create`'s): see [`forge_link`].
    Link {
        url: String,
    },
}

/// What a program reports in OSC 7501 (#720, the spec:
/// <https://www.superlogical.com/rex/docs/build/program-status>). Every
/// field came from the program and is untrusted: `msg` and `title` have no
/// control characters (a report with one is dropped) and no bidi overrides
/// or invisible formatting characters (removed), and are plain text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    /// The record's path (`deploy/us-east`); `None` is the root.
    pub id: Option<String>,
    pub state: StatusState,
    /// Only with `blocked`.
    pub kind: Option<StatusKind>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub msg: Option<String>,
    /// 0 to 100, only with `working` or `blocked`.
    pub progress: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
    /// Not a state: removes the record and its children (with no `id`,
    /// every record).
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    Permission,
    Question,
    Auth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    Osc,
    OscEsc,
    /// DCS, SOS, PM or APC data: skipped until ST.
    Str,
    StrEsc,
}

/// OSC payloads longer than this are dropped (they're images or clipboard
/// data, not anything we read).
const MAX_OSC: usize = 64 * 1024;

pub struct Scanner {
    state: State,
    buf: Vec<u8>,
    overflow: bool,
    /// The printable run so far outside any sequence, while it may hold a
    /// link: a word ends at a space, a control byte or a sequence.
    word: Vec<u8>,
}

/// A word longer than this isn't a link we read.
const MAX_WORD: usize = 2048;

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner {
    pub fn new() -> Self {
        Self { state: State::Ground, buf: Vec::new(), overflow: false, word: Vec::new() }
    }

    /// Feed output that starts at stream offset `base`; returns each signal
    /// with the offset just past the byte that completed it.
    pub fn feed(&mut self, data: &[u8], base: u64) -> Vec<(u64, Signal)> {
        let mut out = Vec::new();
        for (i, &b) in data.iter().enumerate() {
            let at = base + i as u64 + 1;
            if self.state == State::Ground {
                if b > b' ' && b != 0x7f {
                    if self.word.len() < MAX_WORD {
                        self.word.push(b);
                    }
                } else if !self.word.is_empty() {
                    // Just before the byte that ended it.
                    self.word_ends(at - 1, &mut out);
                }
            }
            self.state = match (self.state, b) {
                (State::Ground, 0x1b) => State::Esc,
                (State::Ground, 0x07) => {
                    out.push((at, Signal::Bell));
                    State::Ground
                }
                (State::Ground, _) => State::Ground,
                (State::Esc, b']') => {
                    self.buf.clear();
                    self.overflow = false;
                    State::Osc
                }
                (State::Esc, b'P' | b'X' | b'^' | b'_') => State::Str,
                (State::Esc, b'c') => {
                    out.push((at, Signal::Reset));
                    State::Ground
                }
                (State::Esc, 0x1b) => State::Esc,
                (State::Esc, _) => State::Ground,
                (State::Osc, 0x07) => {
                    self.finish(at, 1, &mut out);
                    State::Ground
                }
                (State::Osc, 0x1b) => State::OscEsc,
                (State::Osc, 0x18 | 0x1a) => State::Ground, // CAN/SUB abort
                (State::Osc, b) => {
                    if self.buf.len() < MAX_OSC {
                        self.buf.push(b);
                    } else {
                        self.overflow = true;
                    }
                    State::Osc
                }
                (State::OscEsc, b'\\') => {
                    self.finish(at, 2, &mut out);
                    State::Ground
                }
                (State::OscEsc, b']') => {
                    // ESC ] inside an OSC: a new one begins.
                    self.buf.clear();
                    self.overflow = false;
                    State::Osc
                }
                (State::OscEsc, _) => State::Ground,
                (State::Str, 0x1b) => State::StrEsc,
                (State::Str, 0x18 | 0x1a) => State::Ground,
                (State::Str, _) => State::Str,
                (State::StrEsc, b'\\') => State::Ground,
                (State::StrEsc, 0x1b) => State::StrEsc,
                (State::StrEsc, _) => State::Str,
            };
        }
        out
    }

    /// A printed word ended: a link to a PR or an issue in it.
    fn word_ends(&mut self, at: u64, out: &mut Vec<(u64, Signal)>) {
        let word = std::mem::take(&mut self.word);
        if word.len() >= MAX_WORD {
            return;
        }
        let text = String::from_utf8_lossy(&word);
        let Some(from) = text.find("https://").or_else(|| text.find("http://")) else { return };
        let url = text[from..].trim_end_matches(['.', ',', ';', ':', ')', ']', '>', '\'', '"', '!', '?']);
        if forge_link(url) {
            out.push((at, Signal::Link { url: url.to_owned() }));
        }
    }

    /// An OSC ended, with a terminator `term` bytes long.
    fn finish(&mut self, at: u64, term: usize, out: &mut Vec<(u64, Signal)>) {
        if self.overflow {
            return;
        }
        if let Some(body) = self.buf.strip_prefix(b"7501;") {
            // ESC ], the payload, the terminator.
            if 2 + self.buf.len() + term <= status::MAX_SEQUENCE
                && let Some(s) = status::parse(body)
            {
                out.push((at, Signal::Status(s)));
            }
            return;
        }
        if let Some(s) = parse_osc(&self.buf) {
            out.push((at, s));
        }
    }
}

fn parse_osc(payload: &[u8]) -> Option<Signal> {
    let text = String::from_utf8_lossy(payload);
    let (code, rest) = text.split_once(';').unwrap_or((&text, ""));
    match code {
        "133" | "633" => {
            let (kind, args) = rest.split_once(';').unwrap_or((rest, ""));
            match kind {
                "A" => Some(Signal::Prompt),
                "C" => Some(Signal::CommandStart),
                "D" => Some(Signal::CommandEnd { exit: args.split(';').next().and_then(|c| c.parse().ok()) }),
                // 633;E;<command line>[;<nonce>]: the line is escaped so that
                // ';' never appears raw.
                "E" if code == "633" => {
                    Some(Signal::CommandLine { text: unescape_633(args.split(';').next().unwrap_or("")) })
                }
                _ => None,
            }
        }
        // OSC 8 ; params ; URI: a hyperlink's target, when it's a PR's or
        // an issue's.
        "8" => {
            let (_, uri) = rest.split_once(';')?;
            forge_link(uri).then(|| Signal::Link { url: uri.to_owned() })
        }
        "7" => {
            let url = rest.strip_prefix("file://")?;
            let path = &url[url.find('/')?..];
            Some(Signal::Cwd { path: local_path(percent_decode(path)) })
        }
        // OSC 9;<message>; but 9;4;… is ConEmu/Ghostty progress, not a
        // notification.
        "9" if !rest.starts_with("4;") && !rest.is_empty() => {
            Some(Signal::Notify { title: String::new(), body: rest.to_owned() })
        }
        "777" => {
            let mut parts = rest.splitn(3, ';');
            (parts.next()? == "notify").then(|| Signal::Notify {
                title: parts.next().unwrap_or("").to_owned(),
                body: parts.next().unwrap_or("").to_owned(),
            })
        }
        // Kitty's OSC 99: metadata ; payload. Only the plain, unencoded
        // payload is read; chunked or base64 notifications show as a title.
        "99" => {
            let (meta, body) = rest.split_once(';')?;
            let is_title = meta.split(':').any(|kv| kv == "p=title");
            let encoded = meta.split(':').any(|kv| kv == "e=1");
            let body = if encoded { String::new() } else { body.to_owned() };
            Some(if is_title {
                Signal::Notify { title: body, body: String::new() }
            } else {
                Signal::Notify { title: String::new(), body }
            })
        }
        _ => None,
    }
}

/// Whether `url` is shaped like a link to a pull request or an issue: the
/// web client's `forgePr` and `forgeIssue` (web/src/shell/client.ts), the
/// shapes its links open as blocks. GitHub's `OWNER/REPO/pull/N`, Forgejo's
/// `…/OWNER/REPO/pulls/N`, a GitLab merge request's `…/-/merge_requests/N`,
/// and `…/OWNER/REPO/issues/N`. Only the shape: the client says which of
/// these it opens (`Client.forgeLink`, which knows the Labs flags).
pub fn forge_link(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")) else { return false };
    let path = match rest.find('/') {
        Some(i) if i > 0 => &rest[i + 1..],
        _ => return false,
    };
    let path = path.split(['?', '#']).next().unwrap_or("");
    let segs: Vec<&str> = path.split('/').collect();
    let number = |i: usize| segs.get(i).is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
    let named = |range: std::ops::Range<usize>| range.into_iter().all(|i| segs.get(i).is_some_and(|s| !s.is_empty()));
    segs.iter().enumerate().any(|(i, s)| match *s {
        // OWNER/REPO/pull/N: right after the host.
        "pull" => i == 2 && named(0..2) && number(3),
        "pulls" | "issues" => i >= 2 && named(0..i) && number(i + 1),
        // GROUP/[SUB/…]PROJECT/-/merge_requests/N
        "merge_requests" => i >= 3 && segs[i - 1] == "-" && named(0..i - 1) && number(i + 1),
        _ => false,
    })
}

/// OSC 7501's body, by the spec's parsing rules.
mod status {
    use super::{Status, StatusKind, StatusState};
    use base64::Engine;

    /// The whole sequence, from ESC to the terminator.
    pub(super) const MAX_SEQUENCE: usize = 4096;
    const MAX_KEY: usize = 16;
    /// `msg` and `title`: encoded, decoded.
    const MAX_MSG: (usize, usize) = (2732, 2048);
    const MAX_TITLE: (usize, usize) = (256, 192);
    const MAX_APP: usize = 32;
    const MAX_ID: usize = 128;
    const MAX_SEGMENT: usize = 32;
    const MAX_DEPTH: usize = 8;

    /// The body of a report (after `7501;`), or `None` when it's the
    /// feature query (the VT layer answers that), or a report to throw away.
    pub(super) fn parse(body: &[u8]) -> Option<Status> {
        let body = std::str::from_utf8(body).ok()?;
        if body.trim() == "?" {
            return None;
        }
        let (mut state, mut id, mut kind, mut app, mut title, mut msg, mut progress) =
            (None, None, None, None, None, None, None);
        for pair in body.split(':') {
            // A malformed pair is skipped; the rest still count.
            let Some((k, v)) = pair.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            if k.is_empty() || !k.bytes().all(|b| b.is_ascii_lowercase()) {
                continue;
            }
            if k.len() > MAX_KEY {
                return None;
            }
            // The last value of a key wins; unknown keys are ignored.
            match k {
                "state" => state = Some(v),
                "id" => id = Some(v),
                "kind" => kind = Some(v),
                "app" => app = Some(v),
                "title" => title = Some(v),
                "msg" => msg = Some(v),
                "progress" => progress = Some(v),
                _ => {}
            }
        }
        let state = match state? {
            "idle" => StatusState::Idle,
            "working" => StatusState::Working,
            "done" => StatusState::Done,
            "blocked" => StatusState::Blocked,
            "error" => StatusState::Error,
            "clear" => StatusState::Clear,
            _ => return None,
        };
        // A bad id doesn't fall back to the root: the report goes.
        let id = match id {
            Some(id) if valid_id(id) => Some(id.to_owned()),
            Some(_) => return None,
            None => None,
        };
        let kind = match kind.filter(|_| state == StatusState::Blocked) {
            Some("permission") => Some(StatusKind::Permission),
            Some("question") => Some(StatusKind::Question),
            Some("auth") => Some(StatusKind::Auth),
            _ => None,
        };
        let progress = progress
            .filter(|_| matches!(state, StatusState::Working | StatusState::Blocked))
            .filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|p| p.parse::<u8>().ok())
            .filter(|p| *p <= 100);
        let app = match app.filter(|a| !a.is_empty()) {
            Some(a) if a.len() > MAX_APP || a.chars().any(forbidden) => return None,
            Some(a) => Some(visible(a)).filter(|a| !a.is_empty()),
            None => None,
        };
        let title = text(title, MAX_TITLE)?;
        let msg = text(msg, MAX_MSG)?;
        Some(Status { id, state, kind, app, title, msg, progress })
    }

    /// `seg/seg/…`: 1 to 8 segments of 1 to 32 of `[A-Za-z0-9_.+-]`, 128
    /// bytes in all.
    fn valid_id(id: &str) -> bool {
        id.len() <= MAX_ID
            && id.split('/').count() <= MAX_DEPTH
            && id.split('/').all(|s| {
                (1..=MAX_SEGMENT).contains(&s.len())
                    && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-'))
            })
    }

    /// A base64 text value: `Some(None)` when absent or empty, `None` when
    /// it breaks a rule (and the whole report goes).
    fn text(v: Option<&str>, (encoded, decoded): (usize, usize)) -> Option<Option<String>> {
        let Some(v) = v.filter(|v| !v.is_empty()) else { return Some(None) };
        if v.len() > encoded {
            return None;
        }
        let bytes = base64::engine::general_purpose::STANDARD_PAD_INDIFFERENT.decode(v).ok()?;
        if bytes.len() > decoded {
            return None;
        }
        let s = String::from_utf8(bytes).ok()?;
        if s.chars().any(forbidden) {
            return None;
        }
        Some(Some(visible(&s)).filter(|s| !s.trim().is_empty()))
    }

    /// Control characters (C0, DEL, C1: newline and tab too) and the line
    /// and paragraph separators: a report with one is thrown away.
    fn forbidden(c: char) -> bool {
        c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')
    }

    /// The text without bidi overrides and isolates, or invisible
    /// formatting characters, which could make a card say something other
    /// than what it seems to (the zero-width joiner and variation selectors
    /// stay, for emoji).
    fn visible(s: &str) -> String {
        s.chars()
            .filter(|c| {
                !matches!(*c as u32,
                    0x00ad | 0x034f | 0x061c | 0x115f | 0x1160 | 0x17b4 | 0x17b5 | 0x180b..=0x180f
                    | 0x200b | 0x200c | 0x200e | 0x200f | 0x202a..=0x202e | 0x2060..=0x206f
                    | 0x3164 | 0xfeff | 0xffa0 | 0xfff9..=0xfffb
                    | 0x1d173..=0x1d17a | 0xe0000..=0xe0fff)
            })
            .collect()
    }
}

/// VS Code's 633 escaping: `\\` for a backslash, `\xHH` for anything else
/// that would break the sequence (`;`, control characters).
fn unescape_633(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            if b[i + 1] == b'\\' {
                out.push(b'\\');
                i += 2;
                continue;
            }
            if b[i + 1] == b'x'
                && i + 3 < b.len()
                && let Ok(v) = u8::from_str_radix(&s[i + 2..i + 4], 16)
            {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A file URL's path as this system writes it: `/C:/Users/x` is `C:\\Users\\x`
/// on Windows.
fn local_path(path: String) -> String {
    let b = path.as_bytes();
    let drive = b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':';
    if cfg!(windows) && drive {
        let p = path[1..].replace('/', "\\");
        // `C:` alone is the drive's current directory, not its root.
        if p.len() == 2 { p + "\\" } else { p }
    } else {
        path
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(Ok(v)) = s.get(i + 1..i + 3).map(|h| u8::from_str_radix(h, 16))
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Output with escape sequences and control characters removed (for search
/// and `--text`): what a reader would see, line by line.
pub fn strip(data: &[u8]) -> String {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if b == 0x1b {
            i += 1;
            match data.get(i) {
                Some(b'[') => {
                    i += 1;
                    while i < data.len() && !(0x40..=0x7e).contains(&data[i]) {
                        i += 1;
                    }
                    i += 1;
                }
                Some(b']' | b'P' | b'X' | b'^' | b'_') => {
                    while i < data.len() && data[i] != 0x07 && !(data[i] == 0x1b && data.get(i + 1) == Some(&b'\\')) {
                        i += 1;
                    }
                    i += if data.get(i) == Some(&0x07) { 1 } else { 2 };
                }
                Some(_) => i += 1,
                None => {}
            }
            continue;
        }
        match b {
            b'\n' | b'\t' => out.push(b),
            b'\r' => {
                if let Some(used) = eol_mark(&mut out, &data[i + 1..]) {
                    i += 1 + used;
                    continue;
                }
                // CR LF is a newline; a bare CR rewrites the line, which for
                // reading purposes we treat as a newline too (but not right
                // after one, where it moves nothing; at the start of `data`
                // it may end a line from the read before). CRs in a row are
                // one: a macOS pty sometimes writes CR CR LF for a LF.
                if out.last() != Some(&b'\n') && !matches!(data.get(i + 1), Some(b'\n' | b'\r')) {
                    out.push(b'\n');
                }
            }
            0x08 => {
                out.pop();
            }
            0x00..=0x1f | 0x7f => {}
            _ => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// zsh's PROMPT_SP (#348): before each prompt it writes PROMPT_EOL_MARK
/// (a reverse-video `%`, or `#` for root), pads the line to the screen's
/// width, then `\r`, as many spaces as the mark is wide, and `\r` again.
/// If the output ended with a newline that blanks the mark; if not, the
/// padding wraps and the mark stays after the output. It isn't the
/// program's output either way.
///
/// Called at the first `\r` with `out` stripped so far and `rest` the bytes
/// after it: when they're `<w spaces>\r` and `out`'s line ends with a mark
/// `w` columns wide and its padding, drop both and say how many bytes of
/// `rest` that used. A `%` the program wrote stays: only the `w` columns
/// right before the padding are the mark.
fn eol_mark(out: &mut Vec<u8>, rest: &[u8]) -> Option<usize> {
    /// The fewest padding spaces taken for PROMPT_SP's (a pane narrower
    /// than this plus the mark gets its marker kept).
    const MIN_PAD: usize = 8;
    let w = rest.iter().take_while(|b| **b == b' ').count();
    if w > 8 || rest.get(w) != Some(&b'\r') {
        return None;
    }
    let line_start = out.iter().rposition(|b| *b == b'\n').map_or(0, |n| n + 1);
    let line = &out[line_start..];
    let pad = line.iter().rev().take_while(|b| **b == b' ').count();
    if pad < MIN_PAD {
        return None;
    }
    let before = std::str::from_utf8(&line[..line.len() - pad]).ok()?;
    let mut cut = before.len();
    let mut cols = 0;
    for (at, c) in before.char_indices().rev() {
        if cols >= w {
            break;
        }
        cols += if wide(c) { 2 } else { 1 };
        cut = at;
    }
    if cols != w {
        return None;
    }
    out.truncate(line_start + cut);
    // Output with no newline: the padding wrapped, so the prompt is on the
    // next line.
    if cut > 0 {
        out.push(b'\n');
    }
    Some(w + 1)
}

/// A character two columns wide (East Asian wide and emoji), near enough
/// for an end-of-line mark.
fn wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115f | 0x2e80..=0xa4cf | 0xac00..=0xd7a3 | 0xf900..=0xfaff
        | 0xfe30..=0xfe4f | 0xff00..=0xff60 | 0xffe0..=0xffe6 | 0x1f300..=0x1faff | 0x20000..=0x3fffd)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(chunks: &[&[u8]]) -> Vec<Signal> {
        let mut s = Scanner::new();
        let mut base = 0;
        let mut out = vec![];
        for c in chunks {
            out.extend(s.feed(c, base).into_iter().map(|(_, sig)| sig));
            base += c.len() as u64;
        }
        out
    }

    #[test]
    fn links_to_prs_and_issues() {
        let link = |u: &str| Signal::Link { url: u.into() };
        // gh pr create's last line, split across reads, then colored ones.
        let s = scan(&[b"Creating pull request\r\nhttps://github.com/o/r/pu", b"ll/12\r\n"]);
        assert_eq!(s, vec![link("https://github.com/o/r/pull/12")]);
        let s = scan(&[b"see \x1b[4mhttps://forgejo.example/o/r/issues/3\x1b[0m.\n(https://gitlab.com/g/s/p/-/merge_requests/9).\n"]);
        assert_eq!(
            s,
            vec![link("https://forgejo.example/o/r/issues/3"), link("https://gitlab.com/g/s/p/-/merge_requests/9")]
        );
        // OSC 8's target, as a link.
        let s = scan(&[b"\x1b]8;;https://codeberg.org/o/r/pulls/4\x1b\\#4\x1b]8;;\x1b\\ "]);
        assert_eq!(s, vec![link("https://codeberg.org/o/r/pulls/4")]);
        // Other links, and words that only look like one, aren't.
        let s = scan(&[b"https://github.com/o/r https://github.com/o/pull/1 https://x.io/pull/1/2 github.com/o/r/pull/1 ftp://a/o/r/pull/1\n"]);
        assert_eq!(s, vec![]);
        assert!(forge_link("https://ghe.example.com/o/r/pull/3/files?x=1"));
        assert!(forge_link("http://127.0.0.1:3000/o/r/issues/7#c"));
        assert!(!forge_link("https://github.com/o/r/pull/x"));
        assert!(!forge_link("https://gitlab.com/p/-/merge_requests/3"));
    }

    #[test]
    fn prompts_commands_and_exit_codes() {
        let s = scan(&[b"\x1b]133;A\x07$ \x1b]633;E;echo hi\\x3b ls\x07\x1b]133;C\x07hi\r\n\x1b]133;D;3\x07"]);
        assert_eq!(
            s,
            vec![
                Signal::Prompt,
                Signal::CommandLine { text: "echo hi; ls".into() },
                Signal::CommandStart,
                Signal::CommandEnd { exit: Some(3) }
            ]
        );
    }

    #[test]
    fn sequences_split_across_reads() {
        let s = scan(&[b"\x1b]13", b"3;D;", b"0\x1b", b"\\", b"\x1b]7;file://geek/tmp/a%20b", b"\x07"]);
        assert_eq!(s, vec![Signal::CommandEnd { exit: Some(0) }, Signal::Cwd { path: "/tmp/a b".into() }]);
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_directory_from_powershell() {
        let s = scan(&[b"\x1b]7;file://WIN/C:/Program%20Files\x07\x1b]7;file://WIN/D:/\x07"]);
        assert_eq!(s, vec![Signal::Cwd { path: r"C:\Program Files".into() }, Signal::Cwd { path: r"D:\".into() }]);
    }

    #[test]
    fn notifications_and_bells() {
        let s = scan(&[b"\x1b]9;build done\x07\x1b]9;4;1;50\x07\x07\x1b]777;notify;Claude;needs you\x1b\\"]);
        assert_eq!(
            s,
            vec![
                Signal::Notify { title: "".into(), body: "build done".into() },
                Signal::Bell,
                Signal::Notify { title: "Claude".into(), body: "needs you".into() },
            ]
        );
    }

    fn status(id: Option<&str>, state: StatusState) -> Status {
        Status { id: id.map(str::to_owned), state, kind: None, app: None, title: None, msg: None, progress: None }
    }

    /// What each body says, sent as `OSC 7501 ; body BEL`.
    fn reports(bodies: &[&str]) -> Vec<Signal> {
        let seqs: Vec<Vec<u8>> = bodies.iter().map(|b| format!("\x1b]7501;{b}\x07").into_bytes()).collect();
        scan(&seqs.iter().map(|s| &s[..]).collect::<Vec<_>>())
    }

    #[test]
    fn program_status_with_either_terminator() {
        let s = scan(&[
            b"\x1b]7501;state=blocked:kind=question:app=deploy:msg=U2hpcCBpdCB0byBwcm9kPw==:progress=40\x07",
            b"\x1b]7501;state=working:id=deploy/us-east:title=VGVzdHMgcGFzc2Vk\x1b\\",
        ]);
        assert_eq!(
            s,
            vec![
                Signal::Status(Status {
                    kind: Some(StatusKind::Question),
                    app: Some("deploy".into()),
                    msg: Some("Ship it to prod?".into()),
                    progress: Some(40),
                    ..status(None, StatusState::Blocked)
                }),
                Signal::Status(Status {
                    title: Some("Tests passed".into()),
                    ..status(Some("deploy/us-east"), StatusState::Working)
                }),
            ]
        );
    }

    #[test]
    fn program_status_split_across_reads() {
        let s = scan(&[b"\x1b]75", b"01;state=do", b"ne:msg=VGVzdHM", b"gcGFzc2Vk\x1b", b"\\"]);
        assert_eq!(
            s,
            vec![Signal::Status(Status { msg: Some("Tests passed".into()), ..status(None, StatusState::Done) })]
        );
    }

    #[test]
    fn program_status_last_value_wins_and_the_rest_is_lenient() {
        let s = reports(&[
            // Repeated keys, whitespace, a malformed pair, an unknown key,
            // padding left off, a kind and progress without blocked.
            " state = working : app=a :junk: state=error :app=cargo:x=1:msg=eOKArnk:kind=auth:progress=7",
            "state=working:progress=101",
            "state=blocked:kind=telepathy:progress=+5",
            "state=clear:id=a/b",
        ]);
        assert_eq!(
            s,
            vec![
                // The bidi override is gone from `x\u{202e}y`.
                Signal::Status(Status {
                    app: Some("cargo".into()),
                    msg: Some("xy".into()),
                    ..status(None, StatusState::Error)
                }),
                Signal::Status(status(None, StatusState::Working)),
                Signal::Status(status(None, StatusState::Blocked)),
                Signal::Status(status(Some("a/b"), StatusState::Clear)),
            ]
        );
    }

    #[test]
    fn program_status_reports_thrown_away() {
        let long_msg = format!("state=done:msg={}", "A".repeat(2733));
        // 2049 bytes decoded, from 2732 encoded.
        let big_msg = format!("state=done:msg={}", "YWFh".repeat(683));
        let long_title = format!("state=done:title={}", "A".repeat(260));
        let deep = format!("state=done:id={}", ["a"; 9].join("/"));
        let seg = format!("state=done:id={}", "a".repeat(33));
        let long_id = format!("state=done:id={}", std::iter::repeat_n("a".repeat(30), 5).collect::<Vec<_>>().join("/"));
        let huge = format!("state=working:pad={}", "x".repeat(4096));
        let bad = [
            // No state, or one nobody knows.
            "app=cargo",
            "state=sleeping",
            // Bad base64, text with a control character (a newline), text
            // that isn't UTF-8.
            "state=done:msg=not*base64",
            "state=done:msg=YQpi",
            "state=done:title=//4=",
            // Limits: msg encoded and decoded, title, app, a key.
            &long_msg,
            &big_msg,
            &long_title,
            "state=done:app=abcdefghijklmnopqrstuvwxyz0123456789",
            "state=done:abcdefghijklmnopq=1",
            // Bad ids: too deep, a long segment, too long, an empty segment,
            // a character it can't have. None falls back to the root.
            &deep,
            &seg,
            &long_id,
            "state=done:id=a//b",
            "state=done:id=",
            "state=done:id=a b",
            // The whole sequence over 4096 bytes.
            &huge,
            // The feature query is the VT layer's to answer.
            "?",
        ];
        for body in bad {
            assert_eq!(reports(&[body]), vec![], "{body}");
        }
        // A sequence of exactly 4096 bytes is fine: ESC ], `7501;`, the
        // body, BEL.
        let fits = format!("state=idle:pad={}", "x".repeat(4096 - 2 - 5 - 15 - 1));
        assert_eq!(reports(&[&fits]), vec![Signal::Status(status(None, StatusState::Idle))]);
    }

    #[test]
    fn a_full_reset() {
        assert_eq!(scan(&[b"x\x1bc\x1b[0m"]), vec![Signal::Reset]);
    }

    #[test]
    fn bel_inside_strings_is_not_a_bell() {
        assert_eq!(scan(&[b"\x1bPq\x07data\x1b\\\x1b]2;title\x07"]), vec![]);
    }

    #[test]
    fn offsets_point_past_the_sequence() {
        let mut s = Scanner::new();
        let got = s.feed(b"ab\x1b]133;A\x07cd", 100);
        assert_eq!(got, vec![(110, Signal::Prompt)]);
    }

    #[test]
    fn strip_leaves_what_a_reader_sees() {
        let raw = b"\x1b[1;31mred\x1b[0m plain\r\nnext\x1b]133;A\x07 line\rover\x08x\n";
        assert_eq!(strip(raw), "red plain\nnext line\novex\n");
        assert_eq!(strip(b"491\r\r\n492\r\r493\r\n"), "491\n492\n493\n");
    }

    /// Bytes from zsh 5.9 in a 40-column pty (`zsh -f -i`, PS1='$ '): its
    /// PROMPT_SP mark and padding, after output with and without a newline.
    #[test]
    fn strip_drops_zshs_end_of_line_mark() {
        const MARK: &[u8] = b"\x1b[1m\x1b[7m%\x1b[27m\x1b[1m\x1b[0m                                       \r \r";
        let cat = |parts: &[&[u8]]| parts.concat();
        // A command's range, C to D: the mark comes before precmd's D.
        let ran = cat(&[b"\x1b]133;C\x07hi\r\n", MARK, b"\x1b]133;D;0\x07"]);
        assert_eq!(strip(&ran), "hi\n");
        // `printf abc`: no newline, so the mark followed the output.
        let abc = cat(&[b"abc", MARK, b"\r\x1b[0m\x1b[27m\x1b[24m\x1b[J$ \x1b[K"]);
        assert_eq!(strip(&abc), "abc\n$ ");
        // `printf '100%'`: the program's % stays.
        let pct = cat(&[b"100%", MARK, b"\r$ "]);
        assert_eq!(strip(&pct), "100%\n$ ");
        // A line of just %, from the program, then zsh's mark.
        let only = cat(&[b"%\r\n", MARK, b"\r$ "]);
        assert_eq!(strip(&only), "%\n$ ");
        // Root's #, and PROMPT_EOL_MARK set to '<-' and to ''.
        let root = b"ok\r\n\x1b[1m\x1b[7m#\x1b[27m\x1b[1m\x1b[0m                                       \r \r\r# ";
        assert_eq!(strip(root), "ok\n# ");
        assert_eq!(strip(b"abc<-                                      \r  \r\r$ "), "abc\n$ ");
        assert_eq!(strip(b"abc                                        \r\r\r$ "), "abc\n$ ");
        // A read that starts at the mark, the prompt's CR after a newline,
        // and a mark split from its last \r (a read that ends there), which
        // stays as it was.
        assert_eq!(strip(&cat(&[MARK, b"$ "])), "$ ");
        assert_eq!(strip(&cat(&[b"\n", MARK, b"\r$ "])), "\n$ ");
        assert_eq!(
            strip(b"hi\r\n%                                       \r "),
            "hi\n%                                       \n "
        );
        // A line cleared with CR, spaces and CR isn't a mark.
        assert_eq!(strip(b"50%\r          \r60%\n"), "50%\n          \n60%\n");
        assert_eq!(strip(b"\r60%"), "\n60%");
    }
}
