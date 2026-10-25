//! The git worktree an agent block owns (AR-2, #1476): `start_agent`'s
//! `worktree` and *Agent on this* (#89) make it with one script
//! (`arugula_core::worktree::SCRIPT`), and closing the block removes it
//! only when nothing would be lost (`CLOSE`).

use arugula_proto::api::{WorktreeFate, WorktreeOutcome};
use serde::{Deserialize, Serialize};

use crate::review::Runner;

/// A worktree a block made, kept in its config (`layout.json`) so `close`
/// finds it after a restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owned {
    /// The clone it belongs to.
    pub repo: String,
    /// The worktree, absolute.
    pub path: String,
    pub branch: String,
    /// What the branch was made from.
    pub base: String,
}

/// A runner on this host, with the environment an agent block gets (the
/// user's shell environment over the daemon's).
pub async fn local(app: &crate::server::App) -> Result<Runner, String> {
    let (home, env) = crate::api::agent_env(app).await.map_err(|e| e.1)?;
    Ok(Runner::Local { env, home })
}

/// What the script made: the clone, the worktree and the base.
pub struct Made {
    pub repo: String,
    pub path: String,
    pub base: String,
}

/// Makes the worktree for `branch` in the clone that holds `dir`, from
/// `base` (empty: the remote's HEAD). `fresh`: refuse a branch or worktree
/// that's there already.
pub async fn make(
    runner: &Runner,
    dir: &str,
    branch: &str,
    base: &str,
    repo: &str,
    fresh: bool,
) -> Result<Made, String> {
    let args = [dir, branch, base, repo, if fresh { "new" } else { "" }].map(str::to_owned);
    let (out, _) = runner.sh(arugula_core::worktree::SCRIPT, &args).await?;
    let out = String::from_utf8_lossy(&out);
    let top = out.lines().find_map(|l| l.strip_prefix("top ")).unwrap_or_default().to_owned();
    let last = out.lines().last().unwrap_or_default();
    let Some((path, base)) = last.strip_prefix("ok ").and_then(|r| r.rsplit_once(' ')) else {
        return Err(last.strip_prefix("err ").unwrap_or(last).to_owned());
    };
    Ok(Made { repo: top, path: path.to_owned(), base: base.to_owned() })
}

/// What closing a block does to its worktree: removed if clean, pushed and
/// merged into its base; else kept, and why. Never forces.
pub async fn settle(runner: &Runner, w: &Owned) -> WorktreeOutcome {
    let args = [w.repo.clone(), w.path.clone(), w.branch.clone(), w.base.clone()];
    let (outcome, why) = match runner.sh(arugula_core::worktree::CLOSE, &args).await {
        Ok((out, _)) => {
            let out = String::from_utf8_lossy(&out);
            let last = out.lines().last().unwrap_or_default().trim();
            match last {
                "removed" => (WorktreeFate::Removed, None),
                "gone" => (WorktreeFate::Removed, Some("it was gone already".to_owned())),
                _ if last.starts_with("removed ") => (WorktreeFate::Removed, Some(last["removed ".len()..].to_owned())),
                _ => match last.strip_prefix("kept ") {
                    Some(why) => (WorktreeFate::Kept, Some(why.to_owned())),
                    None => (WorktreeFate::Kept, Some(format!("couldn't check it: {last}"))),
                },
            }
        }
        Err(e) => (WorktreeFate::Kept, Some(format!("couldn't check it: {e}"))),
    };
    WorktreeOutcome { path: w.path.clone(), branch: w.branch.clone(), outcome, why }
}

/// A worktree left as it is, for `why`.
pub fn kept(w: &Owned, why: String) -> WorktreeOutcome {
    WorktreeOutcome { path: w.path.clone(), branch: w.branch.clone(), outcome: WorktreeFate::Kept, why: Some(why) }
}

/// The sentence for a closed block's summary: "kept worktree P: WHY".
pub fn sentence(o: &WorktreeOutcome) -> String {
    let what = match o.outcome {
        WorktreeFate::Removed => "removed",
        WorktreeFate::Kept => "kept",
    };
    match &o.why {
        Some(why) => format!("{what} worktree {} ({}): {why}", o.path, o.branch),
        None => format!("{what} worktree {} ({})", o.path, o.branch),
    }
}
