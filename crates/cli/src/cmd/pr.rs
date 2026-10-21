//! `arugula pr`: a pull request as a block, and the writes to it.

use super::{
    Ctx,
    forge_ops::{self, ListArgs},
};
use crate::http::{self, request};
use crate::util::{Pane, absolute, env_pane, loaded, open_block, print_json, split_of};
use anyhow::{Context, bail};
use arugula_proto::{BlockType, api::OpenRequest};
use serde_json::json;

/// Writes to a PR block, and a linked repo's PRs on Arugula control.
#[derive(clap::Subcommand)]
pub enum PrCmd {
    /// Show a linked repo's PR, from Arugula control.
    ///
    /// `OWNER/REPO#N`, or its link. No block opens.
    View { target: String },
    /// A linked repo's PRs on Arugula control, or a project's.
    List {
        #[command(flatten)]
        list: ListArgs,
    },
    /// A linked repo's PR's checks, from Arugula control.
    Checks { target: String },
    /// Comment on it.
    ///
    /// `%N`: a PR block's. `OWNER/REPO#N` on a linked repo: through Arugula
    /// control, no block. Run by an agent, or with `--draft`, it's a draft
    /// that waits for a person to send it.
    Comment {
        target: String,
        body: String,
        #[arg(long)]
        draft: bool,
    },
    /// Review it: approve, request_changes or comment.
    ///
    /// `%N` or `OWNER/REPO#N`, as `comment`.
    Review {
        target: String,
        event: String,
        body: Option<String>,
        #[arg(long)]
        draft: bool,
    },
    /// Merge it.
    ///
    /// merge, rebase, rebase-merge, squash, fast-forward-only; GitHub: merge,
    /// squash or rebase.
    Merge {
        block: Pane,
        #[arg(long)]
        style: Option<String>,
    },
    /// Rerun its failed checks.
    ///
    /// GitHub: each red workflow run's failed jobs; Forgejo has no API for it.
    Rerun { block: Pane },
}

/// What's Arugula control's, before any daemon is asked: `view`, `list`,
/// `checks`, and a comment or review on `OWNER/REPO#N`. `None`: a block's.
pub fn on_control(args: &Args, json_out: bool) -> Option<anyhow::Result<i32>> {
    let at = |t: &str| forge_ops::item(t).context("which pull request? OWNER/REPO#N, or its link");
    Some(match args.cmd.as_ref()? {
        PrCmd::View { target } => at(target).and_then(|a| forge_ops::view(a, false, json_out)),
        PrCmd::List { list } => forge_ops::list(list.clone(), "pr", json_out),
        PrCmd::Checks { target } => at(target).and_then(|a| forge_ops::checks(a, json_out)),
        PrCmd::Comment { target, body, draft } => {
            let a = forge_ops::item(target)?;
            forge_ops::comment(a, body.clone(), false, *draft, json_out)
        }
        PrCmd::Review { target, event, body, draft } => {
            let a = forge_ops::item(target)?;
            forge_ops::review(a, event, body.clone(), *draft, json_out)
        }
        PrCmd::Merge { .. } | PrCmd::Rerun { .. } => return None,
    })
}

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    cmd: Option<PrCmd>,
    /// The pull request.
    target: Option<String>,
    /// Split a block instead of opening a tab: `right` for the one this
    /// runs in, or `%N`.
    #[arg(long)]
    split: Option<String>,
    #[arg(long)]
    session: Option<String>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    match args {
        Args { cmd: Some(cmd), .. } => {
            let pane = |t: &str| t.parse::<Pane>().map_err(|e| anyhow::anyhow!("{e}, or OWNER/REPO#N"));
            let (block, method, mut args) = match cmd {
                PrCmd::Comment { target, body, .. } => (pane(&target)?, "comment", json!({ "body": body })),
                PrCmd::Review { target, event, body, .. } => {
                    (pane(&target)?, "review", json!({ "event": event, "body": body }))
                }
                PrCmd::Merge { block, style } => (block, "merge", json!({ "style": style })),
                PrCmd::Rerun { block } => (block, "rerun_checks", json!({})),
                PrCmd::View { .. } | PrCmd::List { .. } | PrCmd::Checks { .. } => {
                    unreachable!("Arugula control's (on_control)")
                }
            };
            if http::agent() {
                args["agent"] = json!(true);
            }
            let v = request(&sock, "POST", &format!("/api/blocks/{}/call/{method}", block.0), Some(&args))?.json()?;
            if json_out {
                print_json(&v);
            } else if let Some(d) = v["draft"].as_str() {
                println!("draft {d}: waits for a person to send it on %{}", block.0);
            } else {
                println!("{}", v["url"].as_str().or(v["said"].as_str()).unwrap_or("sent"));
            }
        }
        Args { cmd: None, target, split, session } => {
            let target = target.context("which pull request? a URL, OWNER/REPO#N, or N in this repository")?;
            let body = OpenRequest {
                kind: BlockType::Forge,
                config: json!({ "pr": target, "dir": absolute(".")? }),
                split: split_of(split.as_deref())?,
                session,
                from_pane: env_pane(),
                ..Default::default()
            };
            let block = open_block(&sock, &body)?.0.block;
            let v = loaded(&sock, block)?;
            if json_out {
                print_json(&v);
                return Ok(0);
            }
            println!("%{block}");
            if let Some(e) = v["state"]["error"].as_str() {
                bail!("{e}");
            }
            print!("{}", request(&sock, "GET", &format!("/api/panes/{block}/capture"), None)?.text()?);
        }
    }
    Ok(0)
}
