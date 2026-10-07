//! `arugula synced`: the history other hosts synced here.

use super::Ctx;
use crate::http::{enc, request};
use crate::util::{print_json, time};
use arugula_proto::api::{Empty, SyncedHost};

#[derive(clap::Subcommand)]
pub enum SyncedCmd {
    /// Forget a host's synced history.
    Rm { name: String },
    /// Re-encrypt all synced history under a new key, and drop the old one.
    RotateKey,
}

pub fn run(cmd: Option<SyncedCmd>, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let (hosts, v) = match cmd {
        None => request(&sock, "GET", "/api/synced", None)?.parse_raw::<Vec<SyncedHost>>()?,
        Some(SyncedCmd::Rm { name }) => {
            let (_, v) =
                request(&sock, "DELETE", &format!("/api/synced/{}", enc(&name)), None)?.parse_raw::<Empty>()?;
            print_json(&v);
            return Ok(0);
        }
        // `{"key": id}`: no type in proto for it.
        Some(SyncedCmd::RotateKey) => {
            print_json(&request(&sock, "POST", "/api/synced/rotate-key", None)?.json()?);
            return Ok(0);
        }
    };
    if json_out {
        print_json(&v);
        return Ok(0);
    }
    for h in &hosts {
        let bytes: u64 = h.panes.values().map(|p| p.bytes).sum();
        let last = h.panes.values().map(|p| p.last_push_ms).max().unwrap_or(0);
        println!(
            "{:<20} {} panes, {} KB, last pushed {}",
            if h.name.is_empty() { "?" } else { &h.name },
            h.panes.len(),
            bytes / 1024,
            time(last)
        );
    }
    Ok(0)
}
