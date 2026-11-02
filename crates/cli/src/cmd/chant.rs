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
    let entries = arugula_e2e::deploy::chant::trust_entries(&log, e.control_url(), &related);
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
