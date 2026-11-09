//! `arugula chant`: what chant needs from Arugula (#1985).
//!
//! `trust-entry` prints your passkeys as chant's `.chant/trust.json` lists
//! a passkey that may sign a gate (chant#4646): each live passkey in your
//! account's published passkey log (`GET /api/accounts/{id}/passkeys/log`,
//! which anyone may read), as `arugula:<your account>`, its public key as
//! DER SubjectPublicKeyInfo, its RP, and the origins an approval may come
//! from (control's site and the related origins control lists, the platform
//! page among them). A chant-fly deploy's approval is your passkey's
//! signature over chant's own gate statement, which the deploy's box hands
//! chant, and chant counts it only for a passkey trust.json lists at base.
//!
//! It only prints: pasting the entries into the repository's trust.json is
//! a change to its trust config, a protected write in chant, and a person's
//! to make.

use anyhow::bail;
use serde_json::Value;

use crate::control::Enrolled;

#[derive(clap::Subcommand)]
pub enum ChantCmd {
    /// Print your passkeys as entries for chant's `.chant/trust.json`.
    ///
    /// Each live passkey of your account, as `arugula:<account>`, for the
    /// `passkeys` list: a chant-fly deploy's approval is your passkey's
    /// signature over chant's gate statement, and chant counts it only for
    /// a passkey listed there. Paste them into the repository's
    /// `.chant/trust.json` yourself; this changes nothing.
    TrustEntry,
}

pub fn run(cmd: ChantCmd, _json_out: bool) -> anyhow::Result<i32> {
    let Some(e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula control: `arugula login` first");
    };
    match cmd {
        ChantCmd::TrustEntry => trust_entry(&e),
    }
}

fn trust_entry(e: &Enrolled) -> anyhow::Result<i32> {
    let account = e.account().to_owned();
    let log: arugula_e2e::passkey::Log =
        serde_json::from_value(e.api("GET", &format!("/api/accounts/{account}/passkeys/log"), None)?)?;
    if log.account != account {
        bail!("control answered another account's passkey log");
    }
    // The related origins control lists for its passkeys (the platform
    // page's): none when it lists none.
    let related: Vec<String> = e
        .api("GET", "/.well-known/webauthn", None)
        .ok()
        .and_then(|v| v["origins"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|o| o.as_str().map(str::to_owned))
        .collect();
    // Only from a log whose chain holds: one control rewrote (a key swapped
    // in, a removal dropped) prints nothing to trust.
    let entries = arugula_e2e::deploy::chant::trust_entries(&log, e.control_url(), &related).map_err(|r| {
        anyhow::anyhow!(
            "your account's passkey log from control doesn't hold together ({r:?}): nothing printed, since chant would \
             trust what it says"
        )
    })?;
    if entries.is_empty() {
        bail!(
            "your account has no passkey: add one on Control (the account menu, Sign-in and account), then run this again"
        );
    }
    println!("{}", serde_json::to_string_pretty(&Value::Array(entries.clone()))?);
    eprintln!(
        "arugula: {} passkey entr{} for arugula:{account}. Add {} to the \"passkeys\" list of the repository's \
         .chant/trust.json (chant 0.124.64 or newer reads it), and keep a signers file there, even an empty one.",
        entries.len(),
        if entries.len() == 1 { "y" } else { "ies" },
        if entries.len() == 1 { "it" } else { "them" },
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A control that answers the account's passkey log with `log`, and
    /// lists no related origins.
    fn control(log: Value) -> Enrolled {
        use std::io::{BufRead, BufReader, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            for sock in l.incoming() {
                let Ok(sock) = sock else { break };
                let mut r = BufReader::new(sock.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap_or_default().to_owned();
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap() == 0 || h.trim().is_empty() {
                        break;
                    }
                }
                let answer = if path == "/api/accounts/a/passkeys/log" { log.to_string() } else { "{}".to_owned() };
                let mut sock = sock;
                write!(sock, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{answer}", answer.len()).unwrap();
            }
        });
        Enrolled::for_tests(url)
    }

    /// `trust-entry` prints the live passkeys of a log whose chain holds,
    /// and refuses one control rewrote: a key swapped in, its hash left as
    /// it was.
    #[test]
    fn trust_entry_prints_only_from_a_log_that_holds_together() {
        use arugula_e2e::passkey::{Change, Draft, EDDSA, Log, NO_AAGUID};
        // Credential ids, base64url of 16 bytes each.
        let ids = ["AQEBAQEBAQEBAQEBAQEBAQ", "AgICAgICAgICAgICAgICAg"];
        let draft = |n: u8, at: u64| Draft {
            account: "a".into(),
            event: Change::Added,
            credential: ids[usize::from(n) - 1].into(),
            rp: "127.0.0.1".into(),
            alg: EDDSA,
            key: vec![n; 32],
            aaguid: NO_AAGUID.into(),
            at,
            by: "signup".into(),
        };
        let mut log = Log { account: "a".into(), ..Default::default() };
        log.events.push(draft(1, 1).after(None));
        log.events.push(draft(2, 2).after(log.events.last()));
        assert_eq!(trust_entry(&control(serde_json::to_value(&log).unwrap())).unwrap(), 0);

        let mut swapped = log.clone();
        // Another key (base64url of 32 bytes), its event's hash left.
        swapped.events[1].key = "CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQk".into();
        let err = trust_entry(&control(serde_json::to_value(&swapped).unwrap())).unwrap_err();
        assert!(err.to_string().contains("doesn't hold together"), "{err:#}");
    }
}
