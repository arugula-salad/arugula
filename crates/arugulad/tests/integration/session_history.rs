//! M112a (#1027): `/api/history` and `/api/events` for one session's panes,
//! with an `id` on each entry that's the same on every read, and a guest's
//! read of them kept to the panes they may read.

// Over the daemon's Unix socket.
#![cfg(unix)]

use std::collections::BTreeSet;

use arugula_testkit::{Daemon, arugulad};
use serde_json::{Value, json};

const OWNER: &str = "me@example.com";
const FRIEND: &str = "friend@example.com";

fn start() -> Daemon {
    arugulad!("session-history").no_tailscale().args(["--owner", OWNER]).env("PS1", "$ ").wait_secs(10).start()
}

/// Run a command in a session (made by name), wait for it to end, and say
/// its pane and the pane's session.
fn run_in(d: &Daemon, session: &str, command: &str) -> (u64, u64) {
    let pane = d.post("/api/run", json!({"command": command, "session": session}))["pane"].as_u64().unwrap();
    d.get(&format!("/api/panes/{pane}/wait?until=exit&timeout=10"));
    let panes = d.get("/api/panes");
    let p = panes.as_array().unwrap().iter().find(|p| p["id"] == pane).unwrap();
    (pane, p["session"].as_u64().unwrap())
}

fn panes_of(v: &Value) -> BTreeSet<u64> {
    v.as_array().unwrap().iter().map(|e| e["pane"].as_u64().unwrap()).collect()
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap().to_owned()).collect()
}

fn ndjson(text: &str) -> Value {
    Value::Array(text.lines().filter(|l| !l.is_empty()).map(|l| serde_json::from_str(l).unwrap()).collect())
}

/// A GET over TCP as the friend: status and body.
fn as_friend(d: &Daemon, path: &str) -> (u16, String) {
    let (status, _, body) = d.tcp("GET", path, &[("tailscale-user-login", FRIEND)], None);
    (status, body)
}

#[test]
fn history_and_events_by_session_with_stable_ids_and_a_guest_sees_only_theirs() {
    let d = start();
    let (a, sa) = run_in(&d, "alpha", "echo in-alpha");
    let (b, sb) = run_in(&d, "beta", "echo in-beta");
    assert_ne!(sa, sb);
    d.wait_for("both commands in history", || {
        let all = panes_of(&d.get("/api/history"));
        all.contains(&a) && all.contains(&b)
    });

    // The session filter: only that session's panes.
    let ha = d.get(&format!("/api/history?session={sa}"));
    assert_eq!(panes_of(&ha), BTreeSet::from([a]), "{ha}");
    assert!(ha.as_array().unwrap().iter().any(|e| e["text"] == "echo in-alpha"), "{ha}");
    assert_eq!(panes_of(&d.get(&format!("/api/history?session={sb}"))), BTreeSet::from([b]));
    assert_eq!(d.raw("GET", "/api/history?session=999", None).0, 404);

    // Ids: the same on a second read, and unique.
    let all = d.get("/api/history");
    let first = ids(&all);
    assert!(first.iter().all(|i| !i.is_empty()), "{all}");
    assert_eq!(first, ids(&d.get("/api/history")));
    assert_eq!(first.iter().collect::<BTreeSet<_>>().len(), first.len(), "{all}");
    // A filtered read gives an entry the id it has unfiltered.
    for e in ha.as_array().unwrap() {
        assert!(first.contains(&e["id"].as_str().unwrap().to_owned()), "{e}");
    }

    // Events: the same filter, and ids that hold across reads.
    let events = |path: &str| ndjson(&d.raw("GET", path, None).1);
    let ea = events(&format!("/api/events?session={sa}"));
    assert!(!ea.as_array().unwrap().is_empty(), "no events for alpha");
    assert_eq!(panes_of(&ea), BTreeSet::from([a]), "{ea}");
    let eids = ids(&ea);
    assert!(eids.iter().all(|i| !i.is_empty()), "{ea}");
    assert_eq!(eids, ids(&events(&format!("/api/events?session={sa}"))));
    assert_eq!(eids.iter().collect::<BTreeSet<_>>().len(), eids.len(), "{ea}");
    assert_eq!(d.raw("GET", "/api/events?session=999", None).0, 404);

    // A guest: nothing before a share, then only the shared session's panes.
    assert_eq!(as_friend(&d, "/api/history").0, 403);
    d.post(
        "/api/acl",
        json!({ "session": sa, "principal": format!("tailnet:{FRIEND}"), "role": "viewer", "history": true }),
    );
    let (s, body) = as_friend(&d, "/api/history");
    assert_eq!(s, 200, "{body}");
    let theirs: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(panes_of(&theirs), BTreeSet::from([a]), "{theirs}");
    assert_eq!(ids(&theirs), ids(&ha), "a guest's ids are the owner's");
    let (s, body) = as_friend(&d, &format!("/api/history?session={sa}"));
    assert_eq!((s, panes_of(&serde_json::from_str(&body).unwrap())), (200, BTreeSet::from([a])));
    // Another session is no more theirs by naming it, nor another host's.
    assert_eq!(as_friend(&d, &format!("/api/history?session={sb}")).0, 404);
    assert_eq!(as_friend(&d, "/api/history?host=*").0, 403);
    let (s, body) = as_friend(&d, "/api/events");
    assert_eq!(s, 200, "{body}");
    assert_eq!(panes_of(&ndjson(&body)), BTreeSet::from([a]), "{body}");
    assert_eq!(as_friend(&d, &format!("/api/events?session={sb}")).0, 404);
}

/// Follow `/api/events` (over the socket) until a line has `until` in it:
/// the events it streamed.
fn follow(d: &Daemon, query: &str, until: String) -> std::thread::JoinHandle<Vec<Value>> {
    use std::io::{Read, Write};
    let mut s = std::os::unix::net::UnixStream::connect(d.sock()).unwrap();
    write!(s, "GET /api/events?follow=1&{query} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(30))).unwrap();
    std::thread::spawn(move || {
        let mut seen = String::new();
        let mut buf = [0u8; 4096];
        while !seen.lines().any(|l| l.starts_with('{') && l.contains(&until)) {
            match s.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => seen.push_str(&String::from_utf8_lossy(&buf[..n])),
            }
        }
        // Chunked: the JSON lines are the events.
        seen.lines().filter(|l| l.starts_with('{')).filter_map(|l| serde_json::from_str(l).ok()).collect()
    })
}

fn of(v: &[Value], pane: u64, ty: &str) -> Vec<Value> {
    v.iter().filter(|e| e["pane"] == pane && e["type"] == ty).cloned().collect()
}

/// #1035: panes opening, exiting and closing are kept, so a read without
/// `follow` has them, with the ids a follow stream gave them, and a session
/// still has a closed pane's.
#[test]
fn lifecycle_events_replay_with_the_follow_streams_ids_and_a_closed_panes_session() {
    let mut d = start();
    let (keep, sa) = run_in(&d, "alpha", "echo stays");
    const TYPES: &str = "type=exit,opened,closed,machine";
    let pane = d.post("/api/run", json!({"command": "echo goes", "session": "alpha"}))["pane"].as_u64().unwrap();
    let live = follow(&d, TYPES, format!(r#""pane":{pane},"type":"closed""#));
    d.get(&format!("/api/panes/{pane}/wait?until=exit&timeout=10"));
    let (other, sb) = run_in(&d, "beta", "echo elsewhere");
    assert_ne!(sa, sb);
    assert_eq!(d.raw("POST", &format!("/api/panes/{pane}/close"), Some(json!({}))).0, 200);
    let live = live.join().unwrap();
    let streamed_close = of(&live, pane, "closed");
    assert_eq!(streamed_close.len(), 1, "the stream saw it close: {live:?}");

    let events = |path: &str| -> Vec<Value> {
        let (status, body) = d.raw("GET", path, None);
        assert_eq!(status, 200, "{path}: {body}");
        ndjson(&body).as_array().unwrap().clone()
    };
    let all = events(&format!("/api/events?{TYPES}"));
    for ty in ["opened", "exit", "closed"] {
        assert_eq!(of(&all, pane, ty).len(), 1, "{ty} replays without follow: {all:?}");
    }
    assert_eq!(of(&all, keep, "exit").len(), 1, "{all:?}");
    // The same ids on every read, unique, and the stream's.
    let first = ids(&Value::Array(all.clone()));
    assert!(first.iter().all(|i| !i.is_empty()), "{all:?}");
    assert_eq!(first, ids(&Value::Array(events(&format!("/api/events?{TYPES}")))));
    assert_eq!(first.iter().collect::<BTreeSet<_>>().len(), first.len(), "{all:?}");
    for e in &live {
        let p = e["pane"].as_u64().unwrap_or(0);
        if p == pane || p == keep {
            assert!(all.contains(e), "streamed {e} isn't replayed the same: {all:?}");
        }
    }
    assert_eq!(of(&all, pane, "closed"), streamed_close);
    // A type filter gives an event the id it has unfiltered.
    let closed_only = events("/api/events?type=closed");
    assert!(closed_only.iter().all(|e| all.contains(e)), "{closed_only:?}");

    // The session it was in still has it, closed; the other session doesn't.
    let ea = events(&format!("/api/events?session={sa}&{TYPES}"));
    assert_eq!(of(&ea, pane, "closed"), streamed_close, "{ea:?}");
    assert_eq!(of(&ea, pane, "opened").len(), 1, "{ea:?}");
    assert_eq!(of(&ea, keep, "exit").len(), 1, "{ea:?}");
    assert!(of(&ea, other, "exit").is_empty(), "{ea:?}");
    let eb = events(&format!("/api/events?session={sb}&{TYPES}"));
    assert!(of(&eb, pane, "closed").is_empty() && of(&eb, other, "exit").len() == 1, "{eb:?}");
    // A guest of that session reads it too.
    d.post(
        "/api/acl",
        json!({ "session": sa, "principal": format!("tailnet:{FRIEND}"), "role": "viewer", "history": true }),
    );
    let (s, body) = as_friend(&d, &format!("/api/events?session={sa}&{TYPES}"));
    assert_eq!(s, 200, "{body}");
    let theirs = ndjson(&body);
    assert_eq!(of(theirs.as_array().unwrap(), pane, "closed"), streamed_close, "{theirs}");
    assert!(of(theirs.as_array().unwrap(), other, "exit").is_empty(), "{theirs}");

    // And a restart keeps them.
    d.stop();
    d.start();
    let again = ndjson(&d.raw("GET", &format!("/api/events?{TYPES}"), None).1);
    assert_eq!(of(again.as_array().unwrap(), pane, "closed"), streamed_close, "{again}");
}

/// Follow a pane's tail over TCP as the friend.
fn follow_as_friend(d: &Daemon, pane: u64) -> std::net::TcpStream {
    use std::io::Write;
    let mut s = std::net::TcpStream::connect(("127.0.0.1", d.port)).unwrap();
    write!(
        s,
        "GET /api/panes/{pane}/tail?follow=1 HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\ntailscale-user-login: {FRIEND}\r\n\r\n",
        d.port
    )
    .unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_millis(200))).unwrap();
    s
}

/// Read from a follow for up to `secs`: what came, and whether the daemon
/// ended it (the last chunk, or the connection closed) in that time.
fn read_follow(s: &mut std::net::TcpStream, secs: u64, mut enough: impl FnMut(&str) -> bool) -> (String, bool) {
    use std::io::Read;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    let mut seen = String::new();
    let mut buf = [0u8; 4096];
    while std::time::Instant::now() < deadline {
        match s.read(&mut buf) {
            Ok(0) => return (seen, true),
            Ok(n) => seen.push_str(&String::from_utf8_lossy(&buf[..n])),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => return (seen, true),
        }
        if seen.ends_with("\r\n0\r\n\r\n") {
            return (seen, true);
        }
        if enough(&seen) {
            return (seen, false);
        }
    }
    (seen, false)
}

/// The owner makes `pane` private (or not), as the pane menu does.
fn set_private(d: &Daemon, pane: u64, on: bool) {
    use tokio_tungstenite::tungstenite::{self, Message};
    let stream = std::net::TcpStream::connect(("127.0.0.1", d.port)).unwrap();
    let (mut ws, _) = tungstenite::client(d.ws("/ws"), stream).expect("the WebSocket");
    let op = json!({ "type": "pane", "pane": pane, "op": { "op": "set_private", "on": on } });
    ws.send(Message::Text(op.to_string().into())).unwrap();
    let _ = ws.close(None);
    while ws.read().is_ok() {}
}

/// A guest's follow of a pane lasts as long as they may read it: making the
/// pane private ends it, and so does taking their grant away. A tail of
/// another host's synced history is the owner's, as `/api/history` is,
/// whichever local pane it names.
#[test]
fn a_guests_follow_ends_when_they_may_no_longer_read_the_pane() {
    let d = start();
    let ticker =
        d.post("/api/run", json!({"command": "while :; do echo tick; sleep 0.2; done", "session": "alpha"}))["pane"]
            .as_u64()
            .unwrap();
    let panes = d.get("/api/panes");
    let session = panes.as_array().unwrap().iter().find(|p| p["id"] == ticker).unwrap()["session"].as_u64().unwrap();
    let grant = |role: Value| {
        d.post(
            "/api/acl",
            json!({ "session": session, "principal": format!("tailnet:{FRIEND}"), "role": role, "history": true }),
        );
    };
    grant(json!("viewer"));
    let ticks = |s: &str| s.matches("tick").count() >= 3;

    // Another host's history, by way of a pane they may read here.
    let (s, body) = as_friend(&d, &format!("/api/panes/{ticker}/tail?host=elsewhere"));
    assert_eq!(s, 403, "{body}");
    assert_ne!(d.raw("GET", &format!("/api/panes/{ticker}/tail?host=elsewhere"), None).0, 403, "the owner's");

    // Made private: the follow ends.
    let mut f = follow_as_friend(&d, ticker);
    let (seen, ended) = read_follow(&mut f, 10, ticks);
    assert!(!ended && ticks(&seen), "the follow streams: {seen}");
    set_private(&d, ticker, true);
    let (seen, ended) = read_follow(&mut f, 10, |_| false);
    assert!(ended, "still streaming after the pane went private: {seen}");
    assert_eq!(as_friend(&d, &format!("/api/panes/{ticker}/tail")).0, 404);
    set_private(&d, ticker, false);

    // Their grant taken away: the follow ends.
    let mut f = follow_as_friend(&d, ticker);
    let (seen, ended) = read_follow(&mut f, 10, ticks);
    assert!(!ended && ticks(&seen), "the follow streams again: {seen}");
    grant(Value::Null);
    let (seen, ended) = read_follow(&mut f, 10, |_| false);
    assert!(ended, "still streaming after the grant went: {seen}");
}

/// Access that changes between the check at the door and the follow's start
/// still counts: a grant taken away while the request waits there (held by
/// `ARUGULA_TAIL_START_PAUSE_MS`) means no follow at all.
#[test]
fn a_grant_taken_away_while_a_follow_starts_still_ends_it() {
    let d = arugulad!("session-history-start")
        .no_tailscale()
        .args(["--owner", OWNER])
        .env("PS1", "$ ")
        .env("ARUGULA_TAIL_START_PAUSE_MS", "1500")
        .wait_secs(10)
        .start();
    let ticker =
        d.post("/api/run", json!({"command": "while :; do echo tick; sleep 0.2; done", "session": "alpha"}))["pane"]
            .as_u64()
            .unwrap();
    let panes = d.get("/api/panes");
    let session = panes.as_array().unwrap().iter().find(|p| p["id"] == ticker).unwrap()["session"].as_u64().unwrap();
    let grant = |role: Value| {
        d.post(
            "/api/acl",
            json!({ "session": session, "principal": format!("tailnet:{FRIEND}"), "role": role, "history": true }),
        );
    };
    grant(json!("viewer"));
    d.wait_for("ticks", || d.raw("GET", &format!("/api/panes/{ticker}/tail"), None).1.contains("tick"));

    // Past the door, then the grant goes while the handler hasn't started.
    let mut f = follow_as_friend(&d, ticker);
    std::thread::sleep(std::time::Duration::from_millis(500));
    grant(Value::Null);
    let (seen, ended) = read_follow(&mut f, 10, |s| s.contains("tick"));
    assert!(ended && !seen.contains("tick"), "the follow went on after the grant went: {seen}");
}
