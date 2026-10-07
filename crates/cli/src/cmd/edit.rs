//! `arugula edit`: VS Code on a folder or a file.

use super::Ctx;
use crate::util::{Pane, REMOTE, absolute, env_pane, file_line, here, open_block, print_json};
use anyhow::Context;
use arugula_proto::{BlockType, api::OpenRequest};
use serde_json::json;

#[derive(clap::Args)]
pub struct Args {
    /// A folder or file, optionally `FILE:LINE` [default: here].
    path: Option<String>,
    /// The line to show.
    #[arg(long)]
    line: Option<u32>,
    /// Split a block instead of opening a tab: `right` for the one this
    /// runs in, or `%N`.
    #[arg(long)]
    split: Option<String>,
    /// The machine it runs on: `mN`, or `local` for this host [default:
    /// this pane's machine]. (`--host` is another daemon.)
    #[arg(long, hide = true)]
    machine: Option<String>,
    #[arg(long)]
    session: Option<String>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let Args { path, line, split, machine, session } = args;
    let split = match split.as_deref() {
        None => None,
        Some("right") => Some(here(None)?),
        Some(p) => Some(p.parse::<Pane>().map_err(anyhow::Error::msg)?.0),
    };
    let local = machine.as_deref() == Some("local");
    let machine = match machine.filter(|_| !local) {
        Some(m) => Some(m.trim_start_matches('m').parse::<u32>().with_context(|| format!("not a machine: {m}"))?),
        None => None,
    };
    // On this host (not another daemon's, not a VM's), a path is
    // this directory's.
    let mine = machine.is_none() && !REMOTE.load(std::sync::atomic::Ordering::Relaxed);
    let (path, at) = match path {
        Some(p) => {
            let (p, at) = file_line(&p, mine);
            (Some(p), at)
        }
        None => (None, None),
    };
    let path = match path {
        Some(p) if mine => Some(absolute(&p)?),
        Some(p) => Some(p),
        None if mine => Some(std::env::current_dir()?.display().to_string()),
        None => None,
    };
    let body = OpenRequest {
        kind: BlockType::Editor,
        config: json!({ "path": path, "line": line.or(at) }),
        split,
        host: machine,
        local,
        session,
        from_pane: env_pane(),
        ..Default::default()
    };
    let (opened, v) = open_block(&sock, &body)?;
    if json_out {
        print_json(&v);
    } else {
        println!("%{}", opened.block);
    }
    Ok(0)
}
