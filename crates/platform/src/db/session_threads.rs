//! What thread dispatch (M256) asks of agent sessions' tables beside
//! [`super::sessions`]: a thread's latest session, a room's project, the
//! nudge that makes a session's job look again, and the ends a session
//! comes to when what it ran for goes (its workspace, its asker's place in
//! the project). Every move still goes through
//! [`super::sessions::move_session_in`].

use anyhow::Result;

use super::{
    Db,
    jobs::{self, JobKind},
    sessions::{Move, Session, State, move_session_in},
    sql::{Optional as _, Sql, params},
};

/// Why a session stopped when its asker left the project.
pub const ASKER_LEFT: &str = "asker_left: its asker isn't in the project any more";
/// Why a session stopped when its workspace was deleted.
pub const WORKSPACE_GONE: &str = "workspace_gone: its hosted workspace was deleted";

/// Stop each of these sessions that hasn't ended, as control, saying
/// `reason`: the ones it stopped.
fn stop_in(c: &impl Sql, ids: Vec<(String, String)>, reason: &str, now: u64) -> Result<Vec<String>> {
    let detail = serde_json::json!({ "reason": reason }).to_string();
    let mut out = Vec::new();
    for (id, st) in ids {
        let Some(from) = State::parse(&st).filter(|s| !s.terminal()) else { continue };
        let m = Move::Control { from, to: State::Stopped, at: now, detail: &detail };
        if move_session_in(c, &id, &m)?.is_ok() {
            out.push(id);
        }
    }
    Ok(out)
}

/// In the caller's transaction, after a team's roster was rewritten
/// (`Db::project_roster`): stop every session going in the team's
/// projects whose asker isn't in the team now ([`ASKER_LEFT`]). The ones
/// it stopped.
pub fn stop_departed_in(c: &impl Sql, team: &str, now: u64) -> Result<Vec<String>> {
    let going: Vec<(String, String)> = c.query_map(
        "SELECT s.id, s.state FROM agent_sessions s JOIN projects p ON p.id = s.project_id
         WHERE p.team_id = ?1 AND s.state NOT IN ('failed', 'stopped')
           AND NOT EXISTS (SELECT 1 FROM team_members m WHERE m.team_id = ?1 AND m.account_id = s.asker)
         ORDER BY s.created_at, s.id",
        params![team],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    stop_in(c, going, ASKER_LEFT, now)
}

impl Db {
    /// The project a chat room belongs to: a team channel's (its team's
    /// default project) or an object thread's. `None` for DMs, pane threads
    /// and archives.
    pub fn chat_room_project(&self, room: &str) -> Result<Option<String>> {
        Ok(self
            .c()
            .query_row("SELECT project_id FROM chat_rooms WHERE id = ?1", params![room], |r| r.get::<Option<String>>(0))
            .optional()?
            .flatten())
    }

    /// The newest session made on a thread, ended or not.
    pub fn latest_thread_session(&self, room: &str, parent_seq: u64) -> Result<Option<Session>> {
        let id: Option<String> = self
            .c()
            .query_row(
                "SELECT id FROM agent_sessions WHERE room_id = ?1 AND parent_seq = ?2
                 ORDER BY created_at DESC, id DESC LIMIT 1",
                params![room, parent_seq],
                |r| r.get(0),
            )
            .optional()?;
        match id {
            Some(id) => self.session(&id),
            None => Ok(None),
        }
    }

    /// Make session `id`'s job look again now (a prompt was queued, say):
    /// queued again if it had ended, asked to look again if a step holds
    /// it ([`jobs::requeue`]).
    pub fn nudge_session(&self, id: &str, now: u64) -> Result<()> {
        jobs::requeue(&self.c(), id, JobKind::Session, &serde_json::json!({ "session": id }), now, now)
    }

    /// Stop every session on `workspace` that hasn't ended
    /// ([`WORKSPACE_GONE`]): the ones it stopped.
    pub fn stop_workspace_sessions(&self, workspace: &str, now: u64) -> Result<Vec<String>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let going: Vec<(String, String)> = tx.query_map(
            "SELECT id, state FROM agent_sessions WHERE workspace_id = ?1 AND state NOT IN ('failed', 'stopped')
             ORDER BY created_at, id",
            params![workspace],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let stopped = stop_in(&tx, going, WORKSPACE_GONE, now)?;
        tx.commit()?;
        Ok(stopped)
    }
}
