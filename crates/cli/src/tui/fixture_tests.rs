//! The TUI against a client fixture (#200), with no daemon or PTY: the
//! fixture server plays the daemon's side of `attach.jsonl`, and fails on
//! anything the TUI sends that the recording doesn't have. The TUI draws
//! into ratatui's test backend, sized so its pane area is the size the
//! recording viewed, and is typed into with key events.

use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use arugula_proto::fixture::{self, Fixture, What, server::Server};
use arugula_vt::VtEngine;
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
};

use super::{
    app::App,
    conn::{Conn, Wake},
    draw,
};
use crate::http::{Target, Url};

/// The sizes the recording's client viewed, in order.
fn views(f: &Fixture) -> Vec<(u16, u16)> {
    f.events
        .iter()
        .filter_map(|e| match &e.what {
            What::Msg(m) if m["type"] == "view" => Some((m["cols"].as_u64()? as u16, m["rows"].as_u64()? as u16)),
            _ => None,
        })
        .collect()
}

/// A screen whose pane area (beside the sidebar, above the status line)
/// is `cols`x`rows`.
fn screen_for(app: &App, (cols, rows): (u16, u16)) -> (u16, u16) {
    let w = (cols..cols + 64)
        .find(|w| {
            let sw = draw::sidebar_width(*w, app.sidebar);
            w - if sw > 0 { sw + 1 } else { 0 } == cols
        })
        .expect("a width");
    (w, rows + 1)
}

fn text(term: &Terminal<TestBackend>) -> String {
    let buf = term.backend().buffer();
    let mut s = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            s.push_str(buf[(x, y)].symbol());
        }
        s.push('\n');
    }
    s
}

/// Read what the server sent, draw, and send what the TUI queued, until
/// `done` holds; panics after 15 s, showing the screen and the server's
/// report.
fn run_until(
    app: &mut App,
    term: &mut Terminal<TestBackend>,
    server: &Server,
    what: &str,
    mut done: impl FnMut(&App, &str) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let mut got = Vec::new();
        app.conn.read(|m| got.push(m)).unwrap();
        for m in got {
            app.take(m);
        }
        term.draw(|f| {
            draw::draw(app, f);
        })
        .unwrap();
        app.conn.flush().unwrap();
        let screen = text(term);
        let r = server.report();
        assert!(r.ok(), "the TUI sent what the fixture doesn't have: {:?}\n{screen}", r.unexpected);
        if done(app, &screen) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}:\n{screen}\nleft: {:#?}", r.left);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_tui_plays_the_attach_fixture() {
    let f = Fixture::load(&fixture::dir().join("attach.jsonl")).unwrap();
    let sizes = views(&f);
    assert_eq!(sizes.len(), 2, "attach.jsonl views a size, then resizes");
    let server = Server::start(&f, "127.0.0.1:0").unwrap();
    let target = Target::Url(Url::parse(&server.url()).unwrap());
    let (_wake, waker) = Wake::pair().unwrap();
    let (err_tx, err_rx) = mpsc::channel();
    let (conn, hello) = Conn::open(&target, err_tx, waker).unwrap();
    let mut app = App::new(conn, hello, err_rx, None);
    let (w, h) = screen_for(&app, sizes[0]);
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();

    // Attached: the shell's prompt from the snapshot.
    run_until(&mut app, &mut term, &server, "the prompt", |app, s| {
        app.panes.get(&1).is_some_and(|p| p.engine.size() == sizes[0]) && s.contains('$')
    });

    // Typed a key at a time; the command and its output come back.
    for c in "echo fixture-attach".chars() {
        app.event(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
    }
    app.event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    run_until(&mut app, &mut term, &server, "the output", |_, s| s.matches("fixture-attach").count() >= 2);

    // A bigger terminal: the TUI views the new size, and the pane follows.
    let (w, h) = screen_for(&app, sizes[1]);
    term.backend_mut().resize(w, h);
    run_until(&mut app, &mut term, &server, "the resize", |app, _| {
        app.panes.get(&1).is_some_and(|p| p.engine.size() == sizes[1])
    });

    // All the TUI does is played: what's left is the recording's detach
    // and close (the TUI detaches by closing, when it quits).
    let r = server.report();
    assert!(r.ok(), "{:?}", r.unexpected);
    assert!(r.left.first().is_some_and(|l| l.contains(r#""type":"detach""#)), "left: {:#?}", r.left);
}
