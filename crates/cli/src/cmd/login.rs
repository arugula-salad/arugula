//! `arugula login`: this CLI signed in to Arugula cloud, from a browser.

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
    /// Join as one of the account's devices instead, with a key and a code
    /// another device approves, as before machine tokens. Only for a
    /// control or machines that don't take tokens yet; it goes away with
    /// device keys.
    #[arg(long)]
    device: bool,
    /// With --device: the account's fingerprint, as the approving device
    /// shows it (Devices and machines…), checked too. Only needed with an
    /// older control or daemon, whose link has no secret to answer. Implies
    /// --device.
    #[arg(long, value_name = "FINGERPRINT")]
    account: Option<String>,
}

pub fn run(args: &Args, cli: &Cli) -> anyhow::Result<i32> {
    let Args { url, name, device, account } = args;
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
    if *device || account.is_some() {
        control::login_device(&url, &name, account.as_deref())?;
    } else {
        control::login(&url, &name)?;
    }
    Ok(0)
}
