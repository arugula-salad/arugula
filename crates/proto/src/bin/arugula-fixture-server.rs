//! `arugula-fixture-server FIXTURE [--listen ADDR] [--timeout SECS]`:
//! serve a client fixture's daemon side (#200), so a client can be tested
//! with no daemon. FIXTURE is a `.jsonl` file, or the name of one in
//! `crates/proto/fixtures`. It prints its URL, plays the fixture as the
//! client goes, and exits once the fixture has played out and the client
//! has gone: 0 if it all happened, 1 on anything the fixture didn't
//! expect (printed), 2 if steps were still left at the timeout (60 s).

use std::{io::Write, path::PathBuf, process::ExitCode, time::Duration};

use arugula_proto::fixture::{self, Fixture, server::Server};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (mut name, mut listen, mut timeout) = (None, "127.0.0.1:0".to_owned(), 60);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--listen" => listen = args.next().unwrap_or_default(),
            "--timeout" => timeout = args.next().and_then(|t| t.parse().ok()).unwrap_or(timeout),
            "-h" | "--help" => name = None,
            _ => name = Some(a),
        }
    }
    let Some(name) = name else {
        eprintln!("usage: arugula-fixture-server FIXTURE [--listen ADDR] [--timeout SECS]");
        return ExitCode::from(64);
    };
    let path = PathBuf::from(&name);
    let path = if path.exists() { path } else { fixture::dir().join(format!("{name}.jsonl")) };
    let f = match Fixture::load(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("arugula-fixture-server: {e}");
            return ExitCode::from(66);
        }
    };
    let server = match Server::start(&f, &listen) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("arugula-fixture-server: {listen}: {e}");
            return ExitCode::from(71);
        }
    };
    println!("{}", server.url());
    let _ = std::io::stdout().flush();
    let r = server.wait(Duration::from_secs(timeout));
    for u in &r.unexpected {
        eprintln!("unexpected: {u}");
    }
    for l in &r.left {
        eprintln!("left: {l}");
    }
    eprintln!("{}: {} steps played, {} left", f.header.fixture, r.done, r.left.len());
    if !r.ok() {
        ExitCode::from(1)
    } else if !r.left.is_empty() {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}
