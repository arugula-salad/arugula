//! Everything in the core running on the target, with no network: `just
//! native-ios` runs it on an iOS simulator, where a build alone wouldn't show
//! that the C and Zig libraries link and work. Prints "ok" or fails.

use anyhow::{Result, ensure};
use arugula_control_wire::machine_token::TokenHello;
use arugula_e2e::{
    channel::{Initiator, Msg, Responder, prologue},
    keys::DeviceKeys,
};
use arugula_native_core::{Pane, libghostty_vt::key};

fn main() -> Result<()> {
    // aws-lc's own code, which rustls and RS256 passkeys reach.
    let sum = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, b"abc");
    ensure!(hex::encode(sum) == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", "aws-lc's SHA-256");

    // A Noise channel to a daemon, both ends here, opened as the app opens
    // one: a key made for it alone and a machine token in its first message.
    let daemon = DeviceKeys::generate();
    let pro = prologue(&daemon.id());
    let hello = TokenHello { machine_token: "a.token.here".into() }.encode();
    let (initiator, msg1) = Initiator::start_with(&DeviceKeys::generate(), &daemon.noise_public, &pro, &hello)?;
    let (responder, _, said) = Responder::read(&daemon, &pro, &msg1)?;
    ensure!(TokenHello::decode(&said).is_some_and(|t| t.machine_token == "a.token.here"), "the token hello");
    let (msg2, theirs) = responder.finish(b"welcome")?;
    let (welcome, ours) = initiator.finish(&msg2)?;
    ensure!(welcome == b"welcome", "the daemon's welcome");
    for wire in ours.seal(&Msg::Text("hello".into()))? {
        ensure!(theirs.open(&wire)? == Some(Msg::Text("hello".into())), "client to daemon");
    }
    for wire in theirs.seal(&Msg::Binary(vec![1, 2, 3]))? {
        ensure!(ours.open(&wire)? == Some(Msg::Binary(vec![1, 2, 3])), "daemon to client");
    }

    // The terminal: a snapshot, output, and a key under the terminal's modes.
    let mut pane = Pane::new(80, 24)?;
    pane.snapshot(b"\x1b]0;a title\x07abc");
    pane.output(b"de");
    ensure!(pane.term.title()? == "a title", "the terminal's title");
    ensure!(pane.term.cursor_x()? == 5, "the terminal's cursor");
    ensure!(pane.key(key::Key::Enter, true, key::Mods::empty(), None)? == b"\r", "Enter");

    println!("ok");
    Ok(())
}
