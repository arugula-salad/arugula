//! An agent session's pull request (M259, thread = session, Arugula #129),
//! as its asker drafts it and sends it: one per session, made by
//! `0053_pr_drafts`. "Agents draft, people send" (aru-041): the session's
//! box pushes its branch, and only the asker opens the PR, as themselves
//! (`sessions::pr`). Drafts of other kinds are `drafts`' (M170), whose
//! kinds this doesn't widen.
//!
//! - **Saved** ([`Db::save_pr_draft`]) while it's waiting or failed, and
//!   no send holds it: a sent one is the PR's record.
//! - **Sent** as `drafts` are: a send holds it by setting `sent_at`
//!   ([`SENDING_MS`]), and ends once ([`Db::end_pr_send`]): sent (the
//!   PR's number and URL), failed (GitHub refused it), or waiting again
//!   (no way to send it now).
//! - **Noted** ([`Db::note_pr`]): once the mirror has the PR's item, its
//!   thread gets one system line, `session_pr`, by the asker, whose text is
//!   the session's id: the page links it back to the session's thread.

use super::{
    Db,
    chat::NewMessage,
    forge::SENDING_MS,
    sql::{Optional as _, Row, Sql as _, params},
};

/// A session's PR draft, with its repo's name and, once the mirror has the
/// PR, its item's id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrDraft {
    /// `pdr_` and 32 hex characters.
    pub id: String,
    pub session: String,
    pub project: String,
    /// The session's asker: the one person who sends it.
    pub author: String,
    pub repo_id: u64,
    /// `owner/name`, as the repo is linked.
    pub full_name: String,
    /// The session's branch.
    pub head: String,
    pub base: String,
    pub title: String,
    pub body: String,
    /// Opened as a draft PR on GitHub.
    pub draft: bool,
    /// `waiting`, `sent` or `failed`.
    pub state: String,
    pub sent_at: Option<u64>,
    pub sent_url: Option<String>,
    pub number: Option<u64>,
    pub error: Option<String>,
    pub noted_at: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
    /// The mirrored PR's item (`fi_…`), once the mirror has it.
    pub item: Option<String>,
}

impl PrDraft {
    /// Whether a send holds it now ([`SENDING_MS`]).
    pub fn sending(&self, now: u64) -> bool {
        self.state != "sent" && self.sent_at.is_some_and(|at| at + SENDING_MS > now)
    }
}

const COLUMNS: &str = "p.id, p.session_id, p.project_id, p.author, p.repo_id, l.full_name, p.head, p.base, p.title, \
     p.body, p.draft, p.state, p.sent_at, p.sent_url, p.number, p.error, p.noted_at, p.created_at, p.updated_at, i.id";

const FROM: &str = "pr_drafts p JOIN linked_repos l ON l.repo_id = p.repo_id \
     LEFT JOIN forge_items i ON i.repo_id = p.repo_id AND i.number = p.number";

fn row(r: &Row) -> anyhow::Result<PrDraft> {
    Ok(PrDraft {
        id: r.get(0)?,
        session: r.get(1)?,
        project: r.get(2)?,
        author: r.get(3)?,
        repo_id: r.get(4)?,
        full_name: r.get(5)?,
        head: r.get(6)?,
        base: r.get(7)?,
        title: r.get(8)?,
        body: r.get(9)?,
        draft: r.get(10)?,
        state: r.get(11)?,
        sent_at: r.get(12)?,
        sent_url: r.get(13)?,
        number: r.get(14)?,
        error: r.get(15)?,
        noted_at: r.get(16)?,
        created_at: r.get(17)?,
        updated_at: r.get(18)?,
        item: r.get(19)?,
    })
}

/// What a session's PR says, as its asker wrote it.
#[derive(Debug, Clone, Copy)]
pub struct NewPrDraft<'a> {
    pub session: &'a str,
    pub project: &'a str,
    pub author: &'a str,
    pub repo_id: u64,
    pub head: &'a str,
    pub base: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub draft: bool,
}

/// Why a PR draft wasn't saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotSaved {
    /// It was sent: the PR is open.
    Sent,
    /// A send holds it.
    Sending,
}

/// How a send ended.
#[derive(Debug, Clone, Copy)]
pub enum PrEnd<'a> {
    /// Opened: the PR's number and URL.
    Sent { number: Option<u64>, url: Option<&'a str> },
    /// GitHub refused it: to send again.
    Failed(&'a str),
    /// No way to send it now: it waits, saying why.
    Waits(&'a str),
}

impl Db {
    fn pr_draft_where(&self, cond: &str, p: &str) -> anyhow::Result<Option<PrDraft>> {
        self.c().query_row(&format!("SELECT {COLUMNS} FROM {FROM} WHERE {cond}"), params![p], row).optional()
    }

    /// Session `session`'s PR draft, if it has one.
    pub fn pr_draft_of(&self, session: &str) -> anyhow::Result<Option<PrDraft>> {
        self.pr_draft_where("p.session_id = ?1", session)
    }

    /// PR draft `id`.
    pub fn pr_draft(&self, id: &str) -> anyhow::Result<Option<PrDraft>> {
        self.pr_draft_where("p.id = ?1", id)
    }

    /// Keep what a session's PR says (made the first time), unless it was
    /// sent or a send holds it.
    pub fn save_pr_draft(&self, n: &NewPrDraft, now: u64) -> anyhow::Result<Result<PrDraft, NotSaved>> {
        let id = format!("pdr_{}", hex::encode(arugula_e2e::random::<16>()));
        let held = now.saturating_sub(SENDING_MS);
        let mut c = self.c();
        let tx = c.immediate()?;
        let changed = tx.execute(
            "INSERT INTO pr_drafts (id, session_id, project_id, author, repo_id, head, base, title, body, draft,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
             ON CONFLICT (session_id) DO UPDATE SET author = excluded.author, repo_id = excluded.repo_id,
                 head = excluded.head, base = excluded.base, title = excluded.title, body = excluded.body,
                 draft = excluded.draft, updated_at = excluded.updated_at
             WHERE pr_drafts.state <> 'sent' AND (pr_drafts.sent_at IS NULL OR pr_drafts.sent_at < ?12)",
            params![id, n.session, n.project, n.author, n.repo_id, n.head, n.base, n.title, n.body, n.draft, now, held],
        )?;
        let got = tx
            .query_row(&format!("SELECT {COLUMNS} FROM {FROM} WHERE p.session_id = ?1"), params![n.session], row)
            .optional()?;
        tx.commit()?;
        let Some(d) = got else { anyhow::bail!("session {}'s repo isn't linked", n.session) };
        Ok(match (changed, d.state.as_str()) {
            (1, _) => Ok(d),
            (_, "sent") => Err(NotSaved::Sent),
            _ => Err(NotSaved::Sending),
        })
    }

    /// Hold `author`'s waiting (or failed) PR draft `id` for a send: the
    /// draft, held since its `sent_at`. `None` when it isn't theirs, was
    /// sent, or another send holds it.
    pub fn hold_pr_draft(&self, id: &str, author: &str, now: u64) -> anyhow::Result<Option<PrDraft>> {
        let held = now.saturating_sub(SENDING_MS);
        let mut c = self.c();
        let tx = c.immediate()?;
        let n = tx.execute(
            "UPDATE pr_drafts SET sent_at = ?1, error = NULL, updated_at = ?1
             WHERE id = ?2 AND author = ?3 AND author <> '' AND state IN ('waiting', 'failed')
                 AND (sent_at IS NULL OR sent_at < ?4)",
            params![now, id, author, held],
        )?;
        let got = if n == 1 {
            Some(tx.query_row(&format!("SELECT {COLUMNS} FROM {FROM} WHERE p.id = ?1"), params![id], row)?)
        } else {
            None
        };
        tx.commit()?;
        Ok(got)
    }

    /// End the send that holds PR draft `id` since `held`: the draft after,
    /// or `None` when that hold isn't its any more.
    pub fn end_pr_send(&self, id: &str, held: u64, end: PrEnd<'_>, now: u64) -> anyhow::Result<Option<PrDraft>> {
        let (state, sent_at, number, url, error) = match end {
            PrEnd::Sent { number, url } => ("sent", Some(now), number, url, None),
            PrEnd::Failed(e) => ("failed", None, None, None, Some(e)),
            PrEnd::Waits(e) => ("waiting", None, None, None, Some(e)),
        };
        let mut c = self.c();
        let tx = c.immediate()?;
        let n = tx.execute(
            "UPDATE pr_drafts SET state = ?1, sent_at = ?2, number = ?3, sent_url = ?4, error = ?5, updated_at = ?6
             WHERE id = ?7 AND sent_at = ?8 AND state IN ('waiting', 'failed')",
            params![state, sent_at, number, url, error, now, id, held],
        )?;
        let got = if n == 1 {
            Some(tx.query_row(&format!("SELECT {COLUMNS} FROM {FROM} WHERE p.id = ?1"), params![id], row)?)
        } else {
            None
        };
        tx.commit()?;
        Ok(got)
    }

    /// The sent PR drafts on `repo_id` whose PR the mirror has now and
    /// whose thread hasn't had its line.
    pub fn pr_drafts_to_note(&self, repo_id: u64) -> anyhow::Result<Vec<PrDraft>> {
        self.c().query_map(
            &format!(
                "SELECT {COLUMNS} FROM {FROM}
                 WHERE p.repo_id = ?1 AND p.state = 'sent' AND p.noted_at IS NULL AND i.id IS NOT NULL"
            ),
            params![repo_id],
            row,
        )
    }

    /// Say in `room` (the PR's thread) that PR draft `d` came from its
    /// session, once: the line's number, or `None` when it was said.
    pub fn note_pr(&self, d: &PrDraft, room: &str, now: u64) -> anyhow::Result<Option<i64>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let n = tx.execute(
            "UPDATE pr_drafts SET noted_at = ?2 WHERE id = ?1 AND noted_at IS NULL AND state = 'sent'",
            params![d.id, now],
        )?;
        if n != 1 {
            return Ok(None);
        }
        let m = NewMessage {
            author: &d.author,
            at: now,
            text: &d.session,
            quote: None,
            ref_machine: None,
            ref_pane: None,
            mentions: &[],
            agent: None,
            via: None,
            parent: None,
            also: false,
        };
        let seq = super::chat::add_message_in(&tx, room, &m, Some(("session_pr", None)))?;
        tx.commit()?;
        Ok(Some(seq))
    }
}
