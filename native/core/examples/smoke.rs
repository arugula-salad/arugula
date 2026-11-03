//! Everything in the core running on the target, with no network: `just
//! native-ios` runs it on an iOS simulator, where a build alone wouldn't show
//! that the C and Zig libraries link and work. Prints "ok" or fails.

use anyhow::{Result, ensure};
use arugula_native_core::{DeviceKeys, Initiator, Msg, Pane, device, e2e, libghostty_vt::key, prologue, request_auth};

fn main() -> Result<()> {
    // aws-lc's own code, which rustls and RS256 passkeys reach.
    let sum = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, b"abc");
    ensure!(hex::encode(sum) == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", "aws-lc's SHA-256");

    // A device's keys as the keychain would give them back.
    let made = DeviceKeys::generate();
    let keys = device(made.noise_private, made.noise_public, made.sign.to_bytes());
    ensure!(keys.id() == made.id(), "the device's id changed through its bytes");

    // A signed request, in the form control reads.
    let auth = request_auth(&keys, "GET", "/api/me", b"");
    let fields: Vec<&str> = auth.split(' ').collect();
    ensure!(fields.len() == 5 && fields[0] == "v2" && fields[1] == keys.id(), "request_auth: {auth}");

    // A Noise channel to a daemon, both ends here, and a message each way.
    let daemon = DeviceKeys::generate();
    let pro = prologue(&daemon.id());
    let (initiator, msg1) = Initiator::start(&keys, &daemon.noise_public, &pro)?;
    let (responder, _, _) = e2e::channel::Responder::read(&daemon, &pro, &msg1)?;
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
