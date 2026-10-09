//! `arugula synced`: the history other hosts synced here.

use super::Ctx;
use crate::http::{call_raw, enc};
use crate::util::{print_json, time};
use arugula_proto::{
    api::Empty,
    op::ops::{SyncedForget, SyncedList, SyncedRotate},
};

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
        None => call_raw::<SyncedList>(&sock, &(), &Empty {})?,
        Some(SyncedCmd::Rm { name }) => {
            let (_, v) = // A path argument goes in as it is: the name is encoded here, as it was.
            call_raw::<SyncedForget>(&sock, &enc(&name), &Empty {})?;
            print_json(&v);
            return Ok(0);
        }
        Some(SyncedCmd::RotateKey) => {
            print_json(&call_raw::<SyncedRotate>(&sock, &(), &Empty {})?.1);
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
