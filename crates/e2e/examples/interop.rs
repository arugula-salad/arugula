//! Rust's half of the browser interop check (`web/e2e-interop.ts`):
//!
//! - `fixtures`: certificates, a revocation and what Rust makes of them, as
//!   JSON, for the TS code to evaluate the same way;
//! - `check`: read a certificate chain the TS code signed from stdin (and
//!   revocations, if any), and say which devices Rust trusts;
//! - `roster`: read `{pin, prev, roster, certs}` from stdin and say whether
//!   the roster follows (a team's next version, a presigned invite's too);
//! - `grant`: read `{grant, daemon, key, now, signer: {id, sign}}` from
//!   stdin (a block grant the TS code minted, M50) and print what
//!   `Grant::check` makes of it: `ok`, or the refusal's code;
//! - `responder ADDR`: a WebSocket daemon stand-in that answers the Noise
//!   handshake (with its clock in message 2, as the daemon does) and echoes
//!   text and binary messages, and answers requests with their method,
//!   path and body.

use std::io::Read;

use arugula_e2e::{
    Cert, DeviceKeys, Kind, Revocation, Trust,
    block::{Expect, Grant},
    cert::join_code,
    channel::{Msg, Responder, ResponseHead, Welcome, prologue},
    keys::hex32,
    team::{AccountCerts, Roster, TeamPin},
};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

fn signed(keys: &DeviceKeys, by: &DeviceKeys, account: &str, kind: Kind, name: &str, created: u64) -> Cert {
    let mut c = Cert::new(keys, account, kind, name);
    c.created = created;
    c.sign_with(by);
    c
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("fixtures") => {
            let (r, p, d, f) =
                (DeviceKeys::generate(), DeviceKeys::generate(), DeviceKeys::generate(), DeviceKeys::generate());
            let root = signed(&r, &r, "acct", Kind::Browser, "laptop ✓", 1);
            let phone = signed(&p, &r, "acct", Kind::Browser, "phone", 2);
            let daemon = signed(&d, &p, "acct", Kind::Daemon, "geek", 3);
            let late = signed(&DeviceKeys::generate(), &p, "acct", Kind::Daemon, "late", 50);
            let fake = signed(&f, &f, "acct", Kind::Browser, "control's", 4);
            let mut rev = Revocation::new("acct", &phone.device, &r);
            rev.at = 10;
            rev.sig = hex::encode(r.signature(rev.body().as_bytes()));
            let certs = vec![root.clone(), phone, daemon.clone(), late, fake];
            let trust = Trust { account: "acct".into(), root: root.device.clone() };
            let mut trusted: Vec<String> =
                trust.evaluate(&certs, std::slice::from_ref(&rev)).devices.into_keys().collect();
            trusted.sort();
            println!(
                "{}",
                serde_json::json!({
                    "trust": trust, "certs": certs, "revocations": [rev],
                    "trusted": trusted, "joinCode": join_code(&daemon), "daemon": daemon,
                })
            );
        }
        Some("check") => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            let v: serde_json::Value = serde_json::from_str(&s)?;
            let trust: Trust = serde_json::from_value(v["trust"].clone())?;
            let certs: Vec<Cert> = serde_json::from_value(v["certs"].clone())?;
            let revocations: Vec<Revocation> = serde_json::from_value(v["revocations"].clone()).unwrap_or_default();
            let mut ids: Vec<String> = trust.evaluate(&certs, &revocations).devices.into_keys().collect();
            ids.sort();
            println!("{}", serde_json::to_string(&ids)?);
        }
        Some("roster") => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            let v: serde_json::Value = serde_json::from_str(&s)?;
            let pin: TeamPin = serde_json::from_value(v["pin"].clone())?;
            let prev: Option<Roster> = serde_json::from_value(v["prev"].clone())?;
            // One that doesn't even parse doesn't follow.
            let roster: Option<Roster> = serde_json::from_value(v["roster"].clone()).ok();
            let certs: AccountCerts = serde_json::from_value(v["certs"].clone())?;
            println!("{}", roster.is_some_and(|r| r.follows(prev.as_ref(), &pin, &certs)));
        }
        Some("refresh") => {
            // `checkpoint::refresh_account`: what's kept of an account after
            // a fresh look, as signatures, and the stamps.
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            let v: serde_json::Value = serde_json::from_str(&s)?;
            let trust: arugula_e2e::Trust = serde_json::from_value(v["trust"].clone())?;
            let part = |k: &str| -> anyhow::Result<(Vec<arugula_e2e::Cert>, Vec<arugula_e2e::Revocation>)> {
                Ok((
                    serde_json::from_value(v[k]["certs"].clone())?,
                    serde_json::from_value(v[k]["revocations"].clone())?,
                ))
            };
            let (fresh, old) = (part("fresh")?, part("old")?);
            let mut seen: arugula_e2e::checkpoint::Seen = serde_json::from_value(v["seen"].clone())?;
            let now = v["now"].as_u64().unwrap_or_default();
            let (certs, revs, refused) =
                arugula_e2e::checkpoint::refresh_account(&trust, fresh, (&old.0, &old.1), &mut seen, now);
            let sigs = |x: Vec<String>| {
                let mut x = x;
                x.sort();
                x
            };
            println!(
                "{}",
                serde_json::json!({
                    "certs": sigs(certs.into_iter().map(|c| c.sig).collect()),
                    "revocations": sigs(revs.into_iter().map(|r| r.sig).collect()),
                    "refused": refused,
                    "seen": seen,
                })
            );
        }
        Some("lock") => {
            // `TeamLock::signed_in`.
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            let v: serde_json::Value = serde_json::from_str(&s)?;
            let lock: Option<arugula_e2e::team::TeamLock> = serde_json::from_value(v["lock"].clone()).ok();
            let roster: Roster = serde_json::from_value(v["roster"].clone())?;
            let certs: AccountCerts = serde_json::from_value(v["certs"].clone())?;
            println!("{}", lock.is_some_and(|l| l.signed_in(&roster, &certs)));
        }
        Some("advance") => {
            // `checkpoint::advance`: where a roster stands after a history,
            // from the one held, and the version that stopped it.
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            let v: serde_json::Value = serde_json::from_str(&s)?;
            let pin: TeamPin = serde_json::from_value(v["pin"].clone())?;
            let held: Option<Roster> = serde_json::from_value(v["held"].clone())?;
            let history: Vec<Roster> = serde_json::from_value(v["history"].clone())?;
            let certs: AccountCerts = serde_json::from_value(v["certs"].clone())?;
            let (cur, stopped) = arugula_e2e::checkpoint::advance(held.as_ref(), history, &pin, &certs);
            println!("{}", serde_json::json!({ "version": cur.map(|r| r.version), "stopped": stopped }));
        }
        Some("grant") => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            let v: serde_json::Value = serde_json::from_str(&s)?;
            let grant: Grant = serde_json::from_value(v["grant"].clone())?;
            let remote = hex32(v["key"].as_str().unwrap_or_default())?;
            let now = v["now"].as_u64().unwrap_or_default();
            let want = Expect { daemon: v["daemon"].as_str().unwrap_or_default(), remote: &remote, now };
            let (id, sign) = (v["signer"]["id"].as_str().unwrap_or_default(), v["signer"]["sign"].as_str());
            let signer = |by: &str| (by == id).then(|| sign.and_then(|k| hex32(k).ok())).flatten();
            let block = v["block"].as_str().unwrap_or_default();
            match grant.check(&want, signer, |b| b == block) {
                Ok(()) => println!("ok"),
                Err(r) => println!("{}", r.code()),
            }
        }
        Some("responder") => {
            let keys = DeviceKeys::generate();
            let l = tokio::net::TcpListener::bind(args.get(1).map(String::as_str).unwrap_or("127.0.0.1:0")).await?;
            println!("{} {} {}", l.local_addr()?, keys.id(), hex::encode(keys.noise_public));
            loop {
                let (sock, _) = l.accept().await?;
                let keys = DeviceKeys {
                    noise_private: keys.noise_private,
                    noise_public: keys.noise_public,
                    sign: keys.sign.clone(),
                };
                tokio::spawn(async move {
                    let mut ws = tokio_tungstenite::accept_async(sock).await?;
                    let Some(Ok(Message::Binary(m1))) = ws.next().await else { anyhow::bail!("no handshake") };
                    let (r, _who, _) = Responder::read(&keys, &prologue(&keys.id()), &m1)?;
                    let (m2, ch) = r.finish(&Welcome { now: arugula_e2e::now_ms() }.encode())?;
                    ws.send(Message::Binary(m2.into())).await?;
                    while let Some(Ok(Message::Binary(w))) = ws.next().await {
                        let Some(m) = ch.open(&w)? else { continue };
                        let reply = match m {
                            Msg::Request { id, head, body } => Msg::Response {
                                id,
                                head: ResponseHead {
                                    status: 201,
                                    content_type: Some("application/json".into()),
                                    more: false,
                                },
                                body: serde_json::to_vec(&serde_json::json!({
                                    "method": head.method, "path": head.path, "len": body.len(),
                                }))?,
                            },
                            other => other,
                        };
                        for w in ch.seal(&reply)? {
                            ws.send(Message::Binary(w.into())).await?;
                        }
                    }
                    anyhow::Ok(())
                });
            }
        }
        _ => anyhow::bail!("usage: interop fixtures|check|roster|grant|responder [ADDR]"),
    }
    Ok(())
}
