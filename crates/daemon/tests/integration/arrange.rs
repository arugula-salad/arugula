//! M109: layout presets and focus only move panes around. Every pane keeps
//! its process and its scrollback through each preset and a focus round
//! trip.

// Over the daemon's Unix socket, like attach.rs.
#![cfg(unix)]

use std::time::Duration;

use arugula_proto::{AttachPane, ClientMsg, Dir, Edge, Frame, FrameKind, Intent, Node, Preset, ServerMsg, State};
use arugula_testkit::{Daemon, arugulad};
use futures_util::{SinkExt, StreamExt};
use tokio::{net::TcpStream, time::timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[allow(clippy::large_enum_variant)]
enum In {
    Msg(ServerMsg),
    Frame(Frame),
}

async fn recv(ws: &mut Ws) -> In {
    loop {
        let msg = timeout(Duration::from_secs(10), ws.next()).await.expect("timed out").unwrap().unwrap();
        match msg {
            Message::Text(t) => return In::Msg(serde_json::from_str(&t).unwrap()),
            Message::Binary(b) => return In::Frame(Frame::decode(&b).unwrap()),
            _ => {}
        }
    }
}

async fn until<T>(ws: &mut Ws, mut f: impl FnMut(&In) -> Option<T>) -> T {
    loop {
        let m = recv(ws).await;
        if let In::Msg(ServerMsg::Error { message, .. }) = &m {
            panic!("refused: {message}");
        }
        if let Some(t) = f(&m) {
            return t;
        }
    }
}

async fn send(ws: &mut Ws, msg: ClientMsg) {
    ws.send(Message::Text(serde_json::to_string(&msg).unwrap().into())).await.unwrap();
}

async fn intent(ws: &mut Ws, intent: Intent) {
    send(ws, ClientMsg::Intent { id: Some(1), intent }).await;
}

/// A fresh `State`, followed through its deltas until `f` holds.
async fn state_where(ws: &mut Ws, f: impl Fn(&State) -> bool) -> State {
    send(ws, ClientMsg::Subscribe { summary: false }).await;
    let mut cur: Option<State> = None;
    until(ws, |m| {
        match m {
            In::Msg(ServerMsg::State { state }) => cur = Some(state.clone()),
            In::Msg(ServerMsg::Delta { delta }) => {
                if let Some(c) = &mut cur {
                    c.apply(delta);
                }
            }
            _ => return None,
        }
        cur.as_ref().filter(|s| f(s)).cloned()
    })
    .await
}

/// Attach `pane` alone; its snapshot, as text.
async fn attach_pane(ws: &mut Ws, pane: u32) -> String {
    let panes = vec![AttachPane::new(pane, None)];
    send(ws, ClientMsg::Attach { panes, zstd: false, acks: false, kitty_keys: false }).await;
    until(ws, |m| match m {
        In::Frame(f) if f.kind == FrameKind::Snapshot && f.pane == pane => {
            Some(String::from_utf8_lossy(&f.data).into_owned())
        }
        _ => None,
    })
    .await
}

async fn type_in(ws: &mut Ws, pane: u32, line: &str) {
    let f = Frame { kind: FrameKind::Input, pane, offset: 0, data: format!("{line}\r").into_bytes() };
    ws.send(Message::Binary(f.encode().into())).await.unwrap();
}

async fn read_pane_until(ws: &mut Ws, pane: u32, needle: &str) {
    let mut seen = String::new();
    until(ws, |m| {
        if let In::Frame(f) = m
            && f.pane == pane
            && f.kind == FrameKind::Output
        {
            seen.push_str(&String::from_utf8_lossy(&f.data));
        }
        seen.contains(needle).then_some(())
    })
    .await
}

/// What each pane printed: `mark-<pane>007`, spelled so the typed command
/// line doesn't contain it.
fn mark(pane: u32) -> String {
    format!("mark-{pane}007")
}

/// Pane ids, each one's process epoch, and that it runs.
fn panes_of(s: &State) -> Vec<(u32, u64, bool)> {
    let mut p: Vec<_> = s.panes.iter().map(|p| (p.id, p.epoch, p.running)).collect();
    p.sort();
    p
}

fn top(s: &State) -> (Option<Dir>, usize) {
    match &s.tabs[0].root {
        Node::Split { dir, children, .. } => (Some(*dir), children.len()),
        Node::Pane { .. } => (None, 1),
    }
}

#[tokio::test]
async fn arranging_and_focusing_keep_every_pane_and_its_scrollback() {
    let d: Daemon = arugulad!("arrange").env("PS1", "$ ").start();
    let (mut ws, _) = connect_async(d.ws("/ws")).await.unwrap();
    let In::Msg(ServerMsg::Hello { state, .. }) = recv(&mut ws).await else { panic!("expected hello") };
    let tab = state.tabs[0].id;

    // Four panes, split every which way.
    for (pane, edge) in [(1, Edge::Right), (2, Edge::Bottom), (1, Edge::Bottom)] {
        intent(&mut ws, Intent::Split { pane, edge, local: false, cwd: None }).await;
    }
    let s = state_where(&mut ws, |s| s.tabs[0].layout.panes.len() == 4 && s.panes.iter().all(|p| p.running)).await;
    let ids: Vec<u32> = s.tabs[0].root.panes();
    for &p in &ids {
        attach_pane(&mut ws, p).await;
        type_in(&mut ws, p, &format!("echo mark-$(({p}*1000+7))")).await;
        read_pane_until(&mut ws, p, &mark(p)).await;
    }
    let before = panes_of(&state_where(&mut ws, |_| true).await);
    assert_eq!(before.len(), 4);

    // Each preset, by its shape.
    let main = ids[2];
    intent(&mut ws, Intent::Arrange { tab, preset: Preset::Columns, main: None }).await;
    state_where(&mut ws, |s| top(s) == (Some(Dir::Row), 4)).await;
    intent(&mut ws, Intent::Arrange { tab, preset: Preset::Grid, main: None }).await;
    state_where(&mut ws, |s| top(s) == (Some(Dir::Column), 2)).await;
    intent(&mut ws, Intent::Arrange { tab, preset: Preset::MainStack, main: Some(main) }).await;
    let s = state_where(&mut ws, |s| top(s) == (Some(Dir::Row), 2) && s.tabs[0].root.panes()[0] == main).await;
    let arranged = s.tabs[0].layout.clone();

    // Focus one pane, then the whole tab again: the same layout.
    intent(&mut ws, Intent::Focus { tab, pane: Some(ids[1]) }).await;
    let s = state_where(&mut ws, |s| s.tabs[0].focus == Some(ids[1])).await;
    assert_eq!(s.tabs[0].layout.panes.len(), 1);
    assert_eq!(s.tabs[0].layout.panes[0].1.cols, s.tabs[0].cols, "the focused pane fills the tab");
    intent(&mut ws, Intent::Focus { tab, pane: None }).await;
    let s = state_where(&mut ws, |s| s.tabs[0].focus.is_none()).await;
    assert_eq!(s.tabs[0].layout, arranged);

    // The same panes and processes, each with its output and still alive.
    assert_eq!(panes_of(&s), before, "ids, epochs and running");
    for &p in &ids {
        let snap = attach_pane(&mut ws, p).await;
        assert!(snap.contains(&mark(p)), "%{p} lost its scrollback");
        type_in(&mut ws, p, &format!("echo again-$(({p}+40))")).await;
        read_pane_until(&mut ws, p, &format!("again-{}", p + 40)).await;
    }
}
