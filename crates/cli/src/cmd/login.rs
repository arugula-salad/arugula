//! `arugula login`: this CLI signed in to Arugula cloud from a browser
//! (#1151), or with `--device` as one of the account's devices.

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
    /// Sign in from a browser: the default. Click Allow there, and the CLI
    /// holds a CLI's session of your account, with no key. With it, an
    /// older control that can't tell a CLI's session from a browser's is
    /// refused rather than joined as a device.
    #[arg(long, conflicts_with_all = ["device", "account"])]
    browser: bool,
    /// Join as one of the account's devices instead, with a key and a code
    /// another device approves: for an older control, or a device you
    /// already have that holds a key.
    #[arg(long)]
    device: bool,
    /// With `--device`: the account's fingerprint, as the approving device
    /// shows it (Devices and machines…), checked too. Only needed with an
    /// older control or daemon, whose link has no secret to answer.
    #[arg(long, value_name = "FINGERPRINT")]
    account: Option<String>,
}

pub fn run(args: &Args, cli: &Cli) -> anyhow::Result<i32> {
    let Args { url, name, browser, device, account } = args;
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
    // `--account` is the device flow's alone.
    if *device || account.is_some() {
        control::login_device(&url, &name, account.as_deref())?;
        return Ok(0);
    }
    match control::login(&url, &name) {
        Err(e) if !*browser && e.downcast_ref::<control::OlderControl>().is_some() => {
            println!();
            println!("  {e}: joining this terminal as one of your devices instead (`arugula login --device`).");
            control::login_device(&url, &name, None)?;
        }
        Err(e) if e.downcast_ref::<control::OlderControl>().is_some() => {
            anyhow::bail!("{e}; sign in with `arugula login --device` until it's updated");
        }
        r => r?,
    }
    Ok(0)
}
