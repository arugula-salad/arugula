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
//! - **The thread is done** ([`Db::done_threads`]) when the newest of the
//!   thread's sessions merged: its own, or a Try 3's picked sibling (M264),
//!   not the other tries or a review. A done thread leaves the Threads view; `@arugula` again
//!   starts a new session there, and the thread is going again.
//! - **A project's numbers** ([`Db::session_numbers`]): of the sessions
//!   asked for since a time, how many led to a PR, how many merged, and how
//!   long each took from its `@` to its PR. A Try 3 is one ask, whose PR is
//!   its picked sibling's; a review isn't one.

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

/// Of `agent_sessions s`, the thread's own sessions (M264): its own, and
/// a Try 3's picked sibling, as `Db::session_on_thread` has them.
const THREADS: &str = "(s.parent_session IS NULL OR (s.role = 'try' AND EXISTS (
    SELECT 1 FROM agent_sessions p WHERE p.id = s.parent_session AND p.picked = s.id)))";

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
            // The thread's sessions only (M264): its own, and a Try 3's
            // picked sibling, not the other tries or a review.
            let rows: Vec<(String, u64, Option<u64>)> = c.query_map(
                &format!(
                    "SELECT room_id, parent_seq, merged_at FROM agent_sessions s WHERE room_id = ?1 AND {THREADS}
                 ORDER BY created_at, id"
                ),
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
        // One row per PR from its branch: the first one counts. A review
        // asks for nothing (M264), and a Try 3's sibling is its parent's
        // ask: a picked sibling's PR is the ask's, at the ask's time.
        let rows: Vec<(Counted, Option<String>, Option<String>)> = self.c().query_map(
            "SELECT s.id, s.created_at, d.sent_at, i.created_at, s.merged_at, s.parent_session, s.picked
             FROM agent_sessions s
             LEFT JOIN pr_drafts d ON d.session_id = s.id AND d.state = 'sent'
             LEFT JOIN linked_repos l ON l.project_id = s.project_id AND lower(l.full_name) = lower(s.repo)
             LEFT JOIN forge_items i ON i.repo_id = l.repo_id AND i.kind = 'pr' AND i.head_ref = s.branch
             WHERE s.project_id = ?1 AND s.created_at >= ?2 AND s.role <> 'review'
             ORDER BY s.created_at, s.id",
            params![project, since],
            |r| {
                let (sent, item): (Option<u64>, Option<u64>) = (r.get(2)?, r.get(3)?);
                let c = Counted {
                    session: r.get(0)?,
                    asked_at: r.get(1)?,
                    pr_at: [sent, item].into_iter().flatten().min(),
                    merged_at: r.get(4)?,
                };
                Ok((c, r.get(5)?, r.get(6)?))
            },
        )?;
        let mut all: Vec<(Counted, Option<String>, Option<String>)> = vec![];
        for (c, parent, picked) in rows {
            match all.last_mut().filter(|(o, ..)| o.session == c.session) {
                Some((o, ..)) => o.pr_at = [o.pr_at, c.pr_at].into_iter().flatten().min(),
                None => all.push((c, parent, picked)),
            }
        }
        let of = |id: &str| all.iter().find(|(c, ..)| c.session == id).map(|(c, ..)| c.clone());
        Ok(all
            .iter()
            .filter(|(_, parent, _)| parent.is_none())
            .map(|(c, _, picked)| match picked.as_deref().filter(|k| *k != c.session).and_then(of) {
                Some(k) => Counted { pr_at: k.pr_at, merged_at: k.merged_at, ..c.clone() },
                None => c.clone(),
            })
            .collect())
    }
}
