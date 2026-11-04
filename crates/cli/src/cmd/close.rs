//! `arugula close`: panes, and what runs in them.

use super::Ctx;
use crate::http::call;
use crate::util::Pane;
use arugula_proto::{
    api::{Empty, WorktreeFate},
    op::ops::ClosePane,
};

#[derive(clap::Args)]
pub struct Args {
    #[arg(required = true)]
    panes: Vec<Pane>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, .. } = ctx;
    let Args { panes } = args;
    for p in panes {
        let closed = call::<ClosePane>(&sock, &p.0, &Empty {})?;
        // The block's worktree (AR-2): removed, or kept and why.
        if let Some(w) = closed.worktree {
            let why = w.why.map(|w| format!(": {w}")).unwrap_or_default();
            eprintln!(
                "arugula: {} worktree {} ({}){why}",
                if w.outcome == WorktreeFate::Kept { "kept" } else { "removed" },
                w.path,
                w.branch
            );
        }
    }
    Ok(0)
}
