//! `arugula share`: a read-only link to a pane, an ssh invite with `--guest`, and the lists of both.

use super::Ctx;
use crate::http::{call, call_raw};
use crate::util::{Pane, duration, here, now_ms, print_json, span, time};
use arugula_proto::{
    api::{Empty, GuestInviteRequest, ShareRequest},
    op::ops::{GuestMint, GuestRevoke, GuestsList, ShareMint, ShareRevoke, SharesList},
};

#[derive(clap::Subcommand)]
pub enum SharesCmd {
    /// End a share link now; anyone watching is cut off.
    Revoke { id: u32 },
}

#[derive(clap::Args)]
pub struct Args {
    pane: Option<Pane>,
    /// How long it works (e.g. 30m, 2h, 7d; a week at most).
    #[arg(long, default_value = "1h")]
    ttl: String,
    /// An ssh invite instead of a link. (`--ssh` is taken: it
    /// reaches a box over ssh, so `--ssh box share --guest` makes an
    /// invite there.)
    #[arg(long, hide = true)]
    guest: bool,
    /// They may type, when nobody else is driving the pane.
    #[arg(long, hide = true, requires = "guest")]
    rw: bool,
    /// Good for any number of logins until it ends.
    #[arg(long, hide = true, requires = "guest")]
    reusable: bool,
    /// What to call them on their input [default: guest].
    #[arg(long, hide = true, requires = "guest")]
    name: Option<String>,
    /// The address they should ssh to [default: the daemon's
    /// --guest-ssh-host, else its hostname]. Not through control.
    #[arg(long = "addr", hide = true, requires = "guest", conflicts_with = "relay")]
    addr: Option<String>,
    /// Through control's ssh jump host, or fail [default: when this
    /// machine is joined to control and no address is set].
    #[arg(long, hide = true, requires = "guest")]
    relay: bool,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    match args {
        Args { pane, ttl, guest: true, rw, reusable, name, addr, relay } => {
            let body = GuestInviteRequest {
                pane: here(pane)?,
                ttl_secs: Some(duration(&ttl)?),
                rw,
                reusable,
                label: name,
                host: addr,
                relay: relay.then_some(true),
            };
            let (g, v) = call_raw::<GuestMint>(&sock, &(), &body)?;
            if json_out {
                print_json(&v);
            } else {
                println!("{}", g.command.as_deref().unwrap_or_default());
                let left = g.expires_ms.saturating_sub(now_ms()) / 1000;
                if let Some(jump) = g.jump.as_deref() {
                    eprintln!(
                        "Through control's ssh jump host {jump} (ssh -J, written out so its key is pinned \
                         too): control carries the session and can't read it."
                    );
                }
                eprintln!(
                    "Invite {}: {}, {}, for {}. `arugula guests revoke {}` ends it.\n\
                     The host key is pinned in the command ({}). ssh older than 8.5 has no \
                     KnownHostsCommand: save this to a file and pass -o UserKnownHostsFile=<file>:\n{}",
                    g.id,
                    if rw { "read-write" } else { "read-only" },
                    if reusable { "reusable" } else { "one login" },
                    span(left),
                    g.id,
                    g.fingerprint.as_deref().unwrap_or_default(),
                    g.known_hosts.as_deref().unwrap_or_default(),
                );
            }
        }
        Args { pane, ttl, .. } => {
            let body = ShareRequest { pane: here(pane)?, ttl_secs: Some(duration(&ttl)?) };
            let (s, v) = call_raw::<ShareMint>(&sock, &(), &body)?;
            if json_out {
                print_json(&v);
            } else {
                println!("{}", s.url.as_deref().or(s.path.as_deref()).unwrap_or_default());
            }
        }
    }
    Ok(0)
}

pub fn guests(cmd: Option<SharesCmd>, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    match cmd {
        None => {
            let (list, v) = call_raw::<GuestsList>(&sock, &(), &Empty {})?;
            if json_out {
                print_json(&v);
                return Ok(0);
            }
            if list.is_empty() {
                println!("no ssh invites");
            }
            for g in list {
                let left = g.expires_ms.saturating_sub(now_ms()) / 1000;
                let kind = match (g.rw, g.reusable) {
                    (true, true) => "rw, reusable",
                    (true, _) => "rw",
                    (_, true) => "ro, reusable",
                    _ => "ro",
                };
                println!(
                    "{} %{} {:<12} {:<14} {} connected{}, expires in {}",
                    g.id,
                    g.pane,
                    g.label,
                    kind,
                    g.sessions,
                    if g.used && !g.reusable { ", spent" } else { "" },
                    span(left)
                );
            }
        }
        Some(SharesCmd::Revoke { id }) => {
            call::<GuestRevoke>(&sock, &id, &Empty {})?;
        }
    }
    Ok(0)
}

pub fn shares(cmd: Option<SharesCmd>, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    match cmd {
        None => {
            let (shares, v) = call_raw::<SharesList>(&sock, &(), &Empty {})?;
            if json_out {
                print_json(&v);
                return Ok(0);
            }
            for s in &shares {
                let left = s.expires_ms.saturating_sub(now_ms()) / 1000;
                println!("{} %{} made {:>8}, expires in {}", s.id, s.pane, time(s.created_ms), span(left));
            }
        }
        Some(SharesCmd::Revoke { id }) => {
            call::<ShareRevoke>(&sock, &id, &Empty {})?;
        }
    }
    Ok(0)
}
