//! The core with no phone, as testnet's `native` claim drives it: each command
//! does what the app would, against a real control and a real machine, and
//! exits non-zero when it doesn't hold. Its state file stands in for the
//! keychain. Not shipped.

use std::{
    fs,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use arugula_native_core::{
    DeviceKeys, Pane,
    control::{Control, Pairing},
    e2e::{Cert, Trust},
    link::{Event, Link},
    proto::api::{RunRequest, RunResponse},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, Deserialize)]
struct Saved {
    url: String,
    cert: Cert,
    trust: Trust,
}

fn load(state: &str) -> Result<Control> {
    let s: Saved = serde_json::from_slice(&fs::read(state).with_context(|| format!("reading {state}"))?)?;
    let keys = DeviceKeys::load(format!("{state}.key").as_ref())?;
    Control::new(&s.url, keys, s.cert, s.trust)
}

/// Print the link, and wait for a signed-in browser to approve it.
fn pair(url: &str, name: &str, state: &str) -> Result<()> {
    let mut p = Pairing::start(url, DeviceKeys::generate(), name)?;
    println!("#join={}", p.whole_code());
    let end = Instant::now() + Duration::from_secs(p.expires_in_secs.min(120));
    let c = loop {
        ensure!(Instant::now() < end, "nobody approved it");
        match p.poll()? {
            Ok(c) => break c,
            Err(still) => p = still,
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    c.keys.save(format!("{state}.key").as_ref())?;
    fs::write(state, serde_json::to_vec(&Saved { url: c.url.clone(), cert: c.cert.clone(), trust: c.trust.clone() })?)?;
    println!("paired {}", c.cert.device);
    Ok(())
}

/// Swap the device's key for a stranger's, for the claim's BREAK=1: everything
/// it signs after this has to be refused.
fn rekey(state: &str) -> Result<()> {
    DeviceKeys::generate().save(format!("{state}.key").as_ref())
}

/// The startup call: who it is, and what it has.
fn start(c: &Control) -> Result<()> {
    println!("{}", c.get("/api/projects")?);
    Ok(())
}

/// Post `marker` to `room`, and read it back.
fn chat(c: &Control, room: &str, marker: &str) -> Result<()> {
    c.post(&format!("/api/chat/rooms/{room}/messages"), &json!({ "text": marker }))?;
    let page = c.get(&format!("/api/chat/rooms/{room}/messages"))?;
    ensure!(page.to_string().contains(marker), "{marker} isn't in the room: {page}");
    println!("chat ok");
    Ok(())
}

/// Through the relay to the machine called `name`: a new pane, typed into, its
/// answer read back through this client's own terminal.
fn term(c: &Control, name: &str, marker: &str) -> Result<()> {
    let dir = c.get("/api/directory")?;
    let machine = dir["daemons"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|d| d["name"] == name)
        .with_context(|| format!("{name} isn't in the directory: {dir}"))?;
    let id = machine["id"].as_str().context("a machine's id")?;
    let link = Link::open(c, id)?;
    let wait = Duration::from_secs(30);
    // Its hello first: nothing's attached until it has said who's here.
    match link.events.recv_timeout(wait)? {
        Event::State(_) => {}
        other => bail!("the machine's first word was {other:?}"),
    }
    let ask = RunRequest { command: None, ..Default::default() };
    let (status, got) = link.request("POST", "/api/run", Some(&serde_json::to_value(&ask)?), wait)?;
    ensure!((200..300).contains(&status), "POST /api/run: {status} {got}");
    let pane = serde_json::from_value::<RunResponse>(got)?.pane;

    let end = Instant::now() + wait;
    let tab = loop {
        ensure!(Instant::now() < end, "pane %{pane} never appeared");
        if let Event::State(s) = link.events.recv_timeout(wait)?
            && let Some(t) = s.tabs.iter().find(|t| t.layout.panes.iter().any(|(p, _)| *p == pane))
        {
            break t.id;
        }
    };
    link.attach(tab, pane, 100, 30)?;
    link.input(pane, format!("echo {marker}-$((6*7))\r").as_bytes())?;

    let mut screen = Pane::new(100, 30)?;
    let (want, mut seen) = (format!("{marker}-42"), Vec::new());
    let end = Instant::now() + wait;
    while Instant::now() < end {
        match link.events.recv_timeout(end - Instant::now()) {
            Ok(Event::Snapshot { pane: p, data }) if p == pane => {
                screen.snapshot(&data);
                seen = data;
            }
            Ok(Event::Output { pane: p, data }) if p == pane => {
                screen.output(&data);
                seen.extend_from_slice(&data);
            }
            Ok(Event::Closed(why)) => bail!("the channel closed: {why}"),
            Ok(Event::Said(what)) => bail!("the machine said: {what}"),
            Ok(_) => {}
            Err(_) => break,
        }
        if String::from_utf8_lossy(&seen).contains(&want) {
            ensure!(screen.term.cursor_y()? > 0, "the terminal didn't move past the first line");
            println!("term ok");
            return Ok(());
        }
    }
    bail!("{want} never came back: {}", String::from_utf8_lossy(&seen))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["pair", url, name, state] => pair(url, name, state),
        ["rekey", state] => rekey(state),
        ["start", state] => start(&load(state)?),
        ["chat", state, room, marker] => chat(&load(state)?, room, marker),
        ["term", state, name, marker] => term(&load(state)?, name, marker),
        _ => bail!(
            "usage: pair URL NAME STATE | rekey STATE | start STATE | chat STATE ROOM MARKER | term STATE MACHINE MARKER"
        ),
    }
}
