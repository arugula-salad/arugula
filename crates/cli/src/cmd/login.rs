//! `arugula login`: this CLI signed in to Arugula cloud: as one of the
//! account's devices, or with `--browser` from a browser's session.

use crate::http::{self};
use crate::{Cli, control, socket, ssh, util};

#[derive(clap::Args)]
pub struct Args {
    /// The control [default: the one this machine's daemon joined, else
    /// https://control.arugula.io].
    url: Option<String>,
    /// What the account calls this terminal [default: Arugula CLI on
    /// <hostname>].
    #[arg(long)]
    name: Option<String>,
    /// Sign in from a browser instead: click Allow there, and the CLI holds
    /// a session of your account with no key. Opt-in until control tells a
    /// CLI's session from a page's (agents on this machine can read it).
    #[arg(long, conflicts_with = "account")]
    browser: bool,
    /// Join as one of the account's devices, with a key and a code another
    /// device approves: the default for now.
    #[arg(long, hide = true)]
    device: bool,
    /// The account's fingerprint, as the approving device
    /// shows it (Devices and machines…), checked too. Only needed with an
    /// older control or daemon, whose link has no secret to answer.
    #[arg(long, value_name = "FINGERPRINT")]
    account: Option<String>,
}

pub fn run(args: &Args, cli: &Cli) -> anyhow::Result<i32> {
    let Args { url, name, browser, device: _, account } = args;
    let url = match url {
        Some(u) => u.clone(),
        None => http::send_op::<arugula_proto::op::ops::HostGet>(
            &http::Target::Socket(socket(cli)),
            &(),
            &arugula_proto::api::Empty {},
        )
        .and_then(|r| r.json())
        .ok()
        .and_then(|v| v["control"].as_str().map(String::from))
        .unwrap_or_else(|| ssh::CONTROL.to_owned()),
    };
    let name = name.clone().unwrap_or_else(|| {
        let h = util::hostname().unwrap_or_default();
        if h.is_empty() { "Arugula CLI".into() } else { format!("Arugula CLI on {h}") }
    });
    if *browser {
        control::login(&url, &name)?;
    } else {
        control::login_device(&url, &name, account.as_deref())?;
    }
    Ok(0)
}
