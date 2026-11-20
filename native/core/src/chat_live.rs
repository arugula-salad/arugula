//! Chat's live stream (`GET /api/chat/live`), as the web's `EventSource` reads
//! it: server-sent events, each one JSON object (`arugula_control_wire`'s
//! `ChatLive`, tagged by `type`). Its first, `hello`, names the stream for
//! `POST /api/chat/live/open`; the rest say what to fetch again.
//!
//! A dropped stream isn't resumed here: it ends with [`Event::Closed`], and the
//! app opens another, whose `hello` is its cue to load what it missed (the web
//! does the same). So the device's keys stay with [`Control`].

use std::{
    io::{BufRead, Read},
    net::{Shutdown, TcpStream},
    sync::{
        Mutex,
        mpsc::{self, Receiver, Sender},
    },
};

use anyhow::Result;
use arugula_control_wire as wire;
use arugula_e2e::cert::request_auth;
use serde_json::Value;

use crate::control::{Control, Refused};

/// The stream's path.
pub const PATH: &str = "/api/chat/live";

/// What the stream said.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// One event's JSON, as control sent it.
    Data(String),
    /// The stream ended, and why.
    Closed(String),
}

/// An open stream. Dropping it shuts the socket.
pub struct ChatLive {
    events: Mutex<Receiver<Event>>,
    shut: TcpStream,
}

impl ChatLive {
    /// Open the stream, calling `notify` from its thread each time an event is
    /// queued: a UI's cue to take them with [`ChatLive::next`].
    pub fn open(c: &Control, notify: impl Fn() + Send + 'static) -> Result<Self> {
        let auth = request_auth(&c.keys, "GET", PATH, b"");
        let headers = [
            ("x-arugula-auth", auth.as_str()),
            (wire::FEATURES_HEADER, wire::APP_DEVICES),
            ("Accept", "text/event-stream"),
            ("Cache-Control", "no-cache"),
        ];
        let (status, body, shut) = c.site.stream(PATH, &headers)?;
        if !(200..300).contains(&status) {
            let mut said = String::new();
            let _ = body.take(4096).read_to_string(&mut said);
            let v: Value = serde_json::from_str(&said).unwrap_or(Value::Null);
            let error = v["error"].as_str().map_or(said.clone(), str::to_owned);
            let reason = v["reason"].as_str().map(str::to_owned);
            return Err(Refused { status, error, reason }.into());
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let why = read_events(body, &tx, &notify);
            let _ = tx.send(Event::Closed(why));
            notify();
        });
        Ok(Self { events: Mutex::new(rx), shut })
    }

    /// The next event queued, if there is one.
    pub fn next(&self) -> Option<Event> {
        self.events.lock().ok()?.try_recv().ok()
    }
}

impl Drop for ChatLive {
    fn drop(&mut self) {
        let _ = self.shut.shutdown(Shutdown::Both);
    }
}

/// Read server-sent events until the stream ends: each event's `data:` lines,
/// joined, go to `tx`. Comments (keep-alives), `id:` and `event:` lines are
/// passed over. Answers why it ended.
fn read_events(mut body: impl BufRead, tx: &Sender<Event>, notify: &impl Fn()) -> String {
    let mut line = String::new();
    let mut data = String::new();
    loop {
        line.clear();
        match body.read_line(&mut line) {
            Ok(0) => return "the stream ended".to_owned(),
            Ok(_) => {}
            Err(e) => return format!("the stream broke: {e}"),
        }
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            if !data.is_empty() {
                if tx.send(Event::Data(std::mem::take(&mut data))).is_err() {
                    return "nobody's listening".to_owned();
                }
                notify();
            }
            continue;
        }
        if let Some(d) = l.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(d.strip_prefix(' ').unwrap_or(d));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_their_data_lines_and_nothing_else() {
        let stream =
            b": keep-alive\n\ndata: {\"type\":\"hello\",\"stream\":\"s1\"}\n\nid: 7\ndata: {\"type\":\"message\",\n\
              data: \"room\":\"ch_1\",\"seq\":3}\n\n"
                .as_slice();
        let (tx, rx) = mpsc::channel();
        let why = read_events(stream, &tx, &|| {});
        assert_eq!(why, "the stream ended");
        let got: Vec<Event> = rx.try_iter().collect();
        assert_eq!(
            got,
            vec![
                Event::Data(r#"{"type":"hello","stream":"s1"}"#.to_owned()),
                Event::Data("{\"type\":\"message\",\n\"room\":\"ch_1\",\"seq\":3}".to_owned()),
            ]
        );
    }
}
