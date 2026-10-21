//! `arugula issue`: an issue as a block, and the writes to it.

use super::{Ctx, forge_ops, issue_native as native};
use crate::http::{self, request};
use crate::util::{Pane, absolute, env_pane, loaded, open_block, print_json, split_of};
use anyhow::{Context, bail};
use arugula_proto::{BlockType, api::OpenRequest};
use serde_json::json;

/// Issues.
#[derive(clap::Subcommand)]
pub enum IssueCmd {
    /// Open a new issue: the current project's, or a GitHub repo's (--repo).
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
        /// A native issue's: labels, assignees, and where it goes.
        #[command(flatten)]
        native: native::NewArgs,
    },
    /// Comment on an issue.
    ///
    /// `#N`, `N` or `iss_…`: a native issue's (its thread). `%N` (or `N` on
    /// a GitHub home): an issue block's. `OWNER/REPO#N` on a linked repo:
    /// through Arugula control, no block. `-`: read it from stdin. Run by
    /// an agent, or with `--draft`, a GitHub comment is a draft that waits
    /// for a person to send it.
    Comment {
        target: String,
        body: String,
        #[arg(long)]
        draft: bool,
        #[command(flatten)]
        home: native::Home,
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
    /// Start an agent on it.
    ///
    /// A worktree and branch `iN-<slug>`, the agent there with the issue as
    /// its prompt, and the two in a tab. On a native issue (`#N`, `N` or
    /// `iss_…`), the agent's prompt is the issue and its thread.
    Agent {
        target: String,
        /// claude (the default), codex or acp.
        #[arg(long)]
        agent: Option<String>,
        /// More for its prompt.
        #[arg(long)]
        prompt: Option<String>,
        /// The clone to work in (default: where the issue was opened, or
        /// here for a native issue).
        #[arg(long)]
        dir: Option<String>,
        #[command(flatten)]
        home: native::Home,
    },
    /// A project's own issues on Arugula control: show, list,
    /// close, reopen, edit, export.
    #[command(flatten)]
    Native(native::Cmd),
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
    #[command(flatten)]
    home: native::Home,
}

/// What's the platform's, before any daemon is asked (M199): a native
/// issue's commands. `None`: a block's (GitHub's), or `agent`.
pub fn on_platform(args: &mut Args, json_out: bool) -> Option<anyhow::Result<i32>> {
    let native_here = |home: &native::Home, r: Option<&native::Ref>| match native::decide(home, r) {
        Ok(native::Decided::Native(h)) => Ok(Some(h)),
        Ok(native::Decided::GitHub { .. }) => Ok(None),
        Err(e) => Err(e),
    };
    match &mut args.cmd {
        Some(IssueCmd::Native(cmd)) => native::run(cmd, json_out),
        Some(IssueCmd::New { title, body, repo: None, native: n, .. }) => match native::decide(&n.home, None) {
            Ok(native::Decided::Native(h)) => Some(native::new(&h, title.clone(), body.clone(), n, json_out)),
            // GitHub's: the project's linked repo, if it has one.
            Ok(native::Decided::GitHub { repo: linked }) => {
                if let Some(IssueCmd::New { repo, .. }) = &mut args.cmd {
                    *repo = linked;
                }
                None
            }
            Err(e) => Some(Err(e)),
        },
        Some(IssueCmd::Comment { target, body, home, .. }) => {
            let r = native::Ref::parse(target);
            match native_here(home, Some(&r)) {
                Ok(Some(h)) => Some(native::comment(&h, &r, body, json_out)),
                Ok(None) if matches!(r, native::Ref::Number(_)) => Some(Err(anyhow::anyhow!(
                    "this project's issues are on GitHub: `arugula issue N` opens one's block, and \
                     `arugula issue comment %B` comments there"
                ))),
                Ok(None) => None,
                Err(e) => Some(Err(e)),
            }
        }
        // A GitHub issue's, by `--repo`; and `agent`, which needs the daemon.
        Some(_) => None,
        // `issue TARGET` is `show`.
        None => native::run(&native::Cmd::Show { target: args.target.clone()?, home: args.home.clone() }, json_out),
    }
}

/// What's Arugula control's for a linked repo (M169), before any daemon
/// is asked: `create`, `view`, and a comment on `OWNER/REPO#N`. `None`: a
/// native issue's (`on_platform`) or a block's. `list` goes by its target
/// (`issue_native::run`).
pub fn on_control(args: &Args, json_out: bool) -> Option<anyhow::Result<i32>> {
    Some(match args.cmd.as_ref()? {
        IssueCmd::Create { repo, title, body, draft } => {
            forge_ops::create_issue(repo.trim().to_owned(), title.clone(), body.clone(), *draft, json_out)
        }
        IssueCmd::View { target } => forge_ops::item(target)
            .context("which issue? OWNER/REPO#N, or its link")
            .and_then(|a| forge_ops::view(a, true, json_out)),
        IssueCmd::Comment { target, body, draft, .. } => {
            let a = forge_ops::item(target)?;
            forge_ops::comment(a, body.clone(), true, *draft, json_out)
        }
        IssueCmd::New { .. } | IssueCmd::Agent { .. } | IssueCmd::Native(_) => return None,
    })
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    // GitHub's issue, shown as its block.
    let args = match args {
        Args { cmd: Some(IssueCmd::Native(native::Cmd::Show { target, .. })), split, session, home, .. } => {
            Args { cmd: None, target: Some(target), split, session, home }
        }
        a => a,
    };
    match args {
        Args { cmd: Some(IssueCmd::Native(_) | IssueCmd::Create { .. } | IssueCmd::View { .. }), .. } => {
            unreachable!("Arugula control's (on_control, on_platform)")
        }
        Args { cmd: Some(IssueCmd::Comment { target, body, .. }), .. } => {
            let block: Pane = target.parse().map_err(|e| anyhow::anyhow!("{e}, #N, iss_… or OWNER/REPO#N"))?;
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
        Args { cmd: Some(IssueCmd::Agent { target, agent, prompt, dir, home }), .. } => {
            let r = native::Ref::parse(&target);
            if !r.github()
                && let native::Decided::Native(h) = native::decide(&home, Some(&r))?
            {
                return native::agent(&h, &r, &sock, agent, prompt, dir, json_out);
            }
            let block: Pane = target.parse().map_err(|e| anyhow::anyhow!("{e}, #N or iss_…"))?;
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
        Args { cmd: Some(IssueCmd::New { title, body, repo, split, session, .. }), .. } => {
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
        Args { cmd: None, target, split, session, .. } => {
            let target = target.context("which issue? a URL, OWNER/REPO#N, or N in this repository")?;
            let target = target.trim_start_matches('#').to_owned();
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
