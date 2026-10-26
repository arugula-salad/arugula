//! A thread is done when its PR merges (M265, thread = session, Arugula
//! #129): the `merged_at` that `0057_session_merged` adds to
//! `agent_sessions`, and what's read from it (a thread's done, a project's
//! numbers). Every move still goes through
//! [`super::sessions::move_session_in`].
//!
//! - **Which session** ([`Db::session_merged`]): the mirror saw a PR merged
//!   on a linked repo, from a branch that is a session's (`arugula/<8
//!   hex>-<slug>`, one per project) in that repo, whoever opened the PR.
//!   Once only: a session already merged is left as it is.
//! - **It ends.** It's `stopped` by control, saying [`MERGED`], which
//!   frees its slot on the box. A turn under way (`working`) finishes
//!   first: the session's job stops it once the turn is `done`
//!   (`sessions::job`). One that had ended (idle, say) only takes the time.
//! - **Its PR** is the one that merged, so the card links it: the asker's
//!   own from the card, or one someone opened from its branch by hand
//!   (which wins over an asker's PR that closed unmerged). Its draft can't
//!   be kept or sent after (`sessions::pr`).
//! - **The thread is done** ([`Db::done_threads`]) when the newest session
//!   on it merged. A done thread leaves the Threads view; `@arugula` again
//!   starts a new session there, and the thread is going again.
//! - **A project's numbers** ([`Db::session_numbers`]): of the sessions
//!   started since a time, how many led to a PR, how many merged, and how
//!   long each took from its `@` to its PR.

use std::collections::HashSet;

use anyhow::Result;

use super::{
    Db,
    sessions::{Move, State, move_session_in},
    sql::{Sql as _, params},
};

/// Why a session stopped when its PR merged, first the code (the card
/// knows it), then the words.
pub const MERGED: &str = "merged: its pull request merged";

/// One session started since a time, for a project's numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counted {
    pub session: String,
    /// When it was asked for: its `@`.
    pub asked_at: u64,
    /// When its PR opened: its asker's send, else the mirrored PR from its
    /// branch.
    pub pr_at: Option<u64>,
    pub merged_at: Option<u64>,
}

impl Db {
    /// The mirror saw PR `number` on linked repo `repo_id` merged, from
    /// `head` (module docs): the sessions it marked, and of them the ones
    /// it stopped.
    pub fn session_merged(
        &self,
        repo_id: u64,
        head: &str,
        number: u64,
        now: u64,
    ) -> Result<(Vec<String>, Vec<String>)> {
        if !head.starts_with("arugula/") {
            return Ok((vec![], vec![]));
        }
        let mut c = self.c();
        let tx = c.immediate()?;
        let found: Vec<(String, String, String)> = tx.query_map(
            "SELECT s.id, s.state, l.full_name FROM agent_sessions s
             JOIN linked_repos l ON l.project_id = s.project_id AND l.repo_id = ?1 AND l.unlinked_at IS NULL
             WHERE s.branch = ?2 AND s.merged_at IS NULL AND lower(s.repo) = lower(l.full_name)",
            params![repo_id, head],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let detail = serde_json::json!({ "reason": MERGED, "pr": number }).to_string();
        let (mut marked, mut stopped) = (vec![], vec![]);
        for (id, st, full) in found {
            let url = format!("https://github.com/{full}/pull/{number}");
            tx.execute(
                "UPDATE agent_sessions SET merged_at = ?2, pr = ?3, updated_at = ?2
                 WHERE id = ?1 AND merged_at IS NULL",
                params![id, now, url],
            )?;
            marked.push(id.clone());
            // A turn under way ends first: its job stops it once it's done.
            let Some(from) = State::parse(&st).filter(|s| !s.terminal() && *s != State::Working) else { continue };
            let m = Move::Control { from, to: State::Stopped, at: now, detail: &detail };
            if move_session_in(&tx, &id, &m)?.is_ok() {
                stopped.push(id);
            }
        }
        tx.commit()?;
        Ok((marked, stopped))
    }

    /// Of the threads `(room, parent)` that `rooms` hold, the ones that are
    /// done: the newest session on each merged.
    pub fn done_threads(&self, rooms: &[&str]) -> Result<HashSet<(String, u64)>> {
        let mut out = HashSet::new();
        let c = self.c();
        for room in rooms {
            let rows: Vec<(String, u64, Option<u64>)> = c.query_map(
                "SELECT room_id, parent_seq, merged_at FROM agent_sessions WHERE room_id = ?1
                 ORDER BY created_at, id",
                params![room],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            // Oldest first: the newest on each thread decides.
            for (room, parent, merged) in rows {
                if merged.is_some() {
                    out.insert((room, parent));
                } else {
                    out.remove(&(room, parent));
                }
            }
        }
        Ok(out)
    }

    /// Project `project`'s sessions asked for at `since` or later, with
    /// when each one's PR opened and merged.
    pub fn session_numbers(&self, project: &str, since: u64) -> Result<Vec<Counted>> {
        // One row per PR from its branch: the first one counts.
        let rows: Vec<Counted> = self.c().query_map(
            "SELECT s.id, s.created_at, d.sent_at, i.created_at, s.merged_at FROM agent_sessions s
             LEFT JOIN pr_drafts d ON d.session_id = s.id AND d.state = 'sent'
             LEFT JOIN linked_repos l ON l.project_id = s.project_id AND lower(l.full_name) = lower(s.repo)
             LEFT JOIN forge_items i ON i.repo_id = l.repo_id AND i.kind = 'pr' AND i.head_ref = s.branch
             WHERE s.project_id = ?1 AND s.created_at >= ?2
             ORDER BY s.created_at, s.id",
            params![project, since],
            |r| {
                let (sent, item): (Option<u64>, Option<u64>) = (r.get(2)?, r.get(3)?);
                Ok(Counted {
                    session: r.get(0)?,
                    asked_at: r.get(1)?,
                    pr_at: [sent, item].into_iter().flatten().min(),
                    merged_at: r.get(4)?,
                })
            },
        )?;
        let mut out: Vec<Counted> = vec![];
        for c in rows {
            match out.last_mut().filter(|o| o.session == c.session) {
                Some(o) => o.pr_at = [o.pr_at, c.pr_at].into_iter().flatten().min(),
                None => out.push(c),
            }
        }
        Ok(out)
    }
}
