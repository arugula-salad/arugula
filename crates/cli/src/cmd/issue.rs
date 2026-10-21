//! `arugula issue`: an issue as a block, and the writes to it.

use super::{
    Ctx,
    forge_ops::{self, ListArgs},
};
use crate::http::{self, request};
use crate::util::{Pane, absolute, env_pane, loaded, open_block, print_json, split_of};
use anyhow::{Context, bail};
use arugula_proto::{BlockType, api::OpenRequest};
use serde_json::json;

/// Issues.
#[derive(clap::Subcommand)]
pub enum IssueCmd {
    /// Open a new issue in this directory's repository (or --repo).
    New {
        #[arg(long, short)]
        title: String,
        #[arg(long, short)]
        body: Option<String>,
        /// OWNER/REPO.
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        split: Option<String>,
        #[arg(long)]
        session: Option<String>,
    },
    /// Open an issue on a linked repo through Arugula control: no block.
    ///
    /// Run by an agent, or with `--draft`, it's a draft that waits for a
    /// person to send it.
    Create {
        /// OWNER/REPO.
        repo: String,
        #[arg(long, short)]
        title: String,
        #[arg(long, short)]
        body: Option<String>,
        #[arg(long)]
        draft: bool,
    },
    /// Show a linked repo's issue, from Arugula control.
    ///
    /// `OWNER/REPO#N`, or its link. No block opens.
    View { target: String },
    /// A linked repo's issues on Arugula control, or a project's.
    List {
        #[command(flatten)]
        list: ListArgs,
    },
    /// Comment on an issue.
    ///
    /// `%N`: an issue block's. `OWNER/REPO#N` on a linked repo: through
    /// Arugula control, no block. Run by an agent, or with `--draft`, it's a
    /// draft that waits for a person to send it.
    Comment {
        target: String,
        body: String,
        #[arg(long)]
        draft: bool,
    },
    /// Start an agent on it.
    ///
    /// A worktree and branch `iN-<slug>`, the agent there with the issue as
    /// its prompt, and the two in a tab.
    Agent {
        block: Pane,
        /// claude (the default), codex or acp.
        #[arg(long)]
        agent: Option<String>,
        /// More for its prompt.
        #[arg(long)]
        prompt: Option<String>,
        /// The clone to work in (default: where the issue was opened).
        #[arg(long)]
        dir: Option<String>,
    },
}

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    cmd: Option<IssueCmd>,
    /// The issue.
    target: Option<String>,
    /// Split a block instead of opening a tab: `right` for the one this
    /// runs in, or `%N`.
    #[arg(long)]
    split: Option<String>,
    #[arg(long)]
    session: Option<String>,
}

/// What's Arugula control's, before any daemon is asked: `create`,
/// `view`, `list`, and a comment on `OWNER/REPO#N`. `None`: a block's.
pub fn on_control(args: &Args, json_out: bool) -> Option<anyhow::Result<i32>> {
    Some(match args.cmd.as_ref()? {
        IssueCmd::Create { repo, title, body, draft } => {
            forge_ops::create_issue(repo.trim().to_owned(), title.clone(), body.clone(), *draft, json_out)
        }
        IssueCmd::View { target } => forge_ops::item(target)
            .context("which issue? OWNER/REPO#N, or its link")
            .and_then(|a| forge_ops::view(a, true, json_out)),
        IssueCmd::List { list } => forge_ops::list(list.clone(), "issue", json_out),
        IssueCmd::Comment { target, body, draft } => {
            let a = forge_ops::item(target)?;
            forge_ops::comment(a, body.clone(), true, *draft, json_out)
        }
        IssueCmd::New { .. } | IssueCmd::Agent { .. } => return None,
    })
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    match args {
        Args { cmd: Some(IssueCmd::Create { .. } | IssueCmd::View { .. } | IssueCmd::List { .. }), .. } => {
            unreachable!("Arugula control's (on_control)")
        }
        Args { cmd: Some(IssueCmd::Comment { target, body, .. }), .. } => {
            let block: Pane = target.parse().map_err(|e| anyhow::anyhow!("{e}, or OWNER/REPO#N"))?;
            let mut args = json!({ "body": body });
            if http::agent() {
                args["agent"] = json!(true);
            }
            let v = request(&sock, "POST", &format!("/api/blocks/{}/call/comment", block.0), Some(&args))?.json()?;
            if json_out {
                print_json(&v);
            } else if let Some(d) = v["draft"].as_str() {
                println!("draft {d}: waits for a person to send it on %{}", block.0);
            } else {
                println!("{}", v["url"].as_str().or(v["said"].as_str()).unwrap_or("sent"));
            }
        }
        Args { cmd: Some(IssueCmd::Agent { block, agent, prompt, dir }), .. } => {
            let dir = dir.map(|d| absolute(&d)).transpose()?;
            let args = json!({ "agent": agent, "prompt_extra": prompt, "dir": dir });
            let v = request(&sock, "POST", &format!("/api/blocks/{}/call/agent", block.0), Some(&args))?.json()?;
            if json_out {
                print_json(&v);
            } else {
                println!(
                    "%{} on branch {} in {}",
                    v["agent"],
                    v["branch"].as_str().unwrap_or("?"),
                    v["worktree"].as_str().unwrap_or("?")
                );
            }
        }
        Args { cmd: Some(IssueCmd::New { title, body, repo, split, session }), .. } => {
            let body = OpenRequest {
                kind: BlockType::Forge,
                config: json!({ "issue": "new", "title": title, "body": body.unwrap_or_default(), "repo": repo, "dir": absolute(".")? }),
                split: split_of(split.as_deref())?,
                session,
                from_pane: env_pane(),
                ..Default::default()
            };
            let block = open_block(&sock, &body)?.0.block;
            // A person's goes out now; an agent's waits as a draft.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let v = loop {
                let v = request(&sock, "GET", &format!("/api/blocks/{block}"), None)?.json()?;
                let st = &v["state"];
                let settled = st["number"].as_u64().is_some_and(|n| n > 0)
                    || st["new"]["agent"] == true
                    || !st["error"].is_null()
                    || !st["new"]["error"].is_null();
                if settled || std::time::Instant::now() > deadline {
                    break v;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            };
            if json_out {
                print_json(&v);
                return Ok(0);
            }
            let st = &v["state"];
            if let Some(e) = st["error"].as_str().or(st["new"]["error"].as_str()) {
                bail!("{e}");
            }
            match st["number"].as_u64().filter(|n| *n > 0) {
                Some(n) => println!(
                    "%{block} {}#{n} {}",
                    st["repo"].as_str().unwrap_or(""),
                    st["new"]["url"].as_str().unwrap_or("")
                ),
                None => println!("%{block}: a draft that waits for a person to send it"),
            }
        }
        Args { cmd: None, target, split, session } => {
            let target = target.context("which issue? a URL, OWNER/REPO#N, or N in this repository")?;
            let body = OpenRequest {
                kind: BlockType::Forge,
                config: json!({ "issue": target, "dir": absolute(".")? }),
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
