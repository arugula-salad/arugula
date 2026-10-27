//! What comes back from GitHub to an agent session (M260, thread =
//! session, Arugula #129), made by `0055_session_feedback`: its CI rounds
//! and its reviews, each gathered from the webhooks that bring it, then
//! sent to the session as one prompt (`sessions::feedback` says when and
//! what it says).
//!
//! - **One batch per thing** ([`Db::gather_feedback`]): a CI round is the
//!   failed checks of one head commit of the session's branch, a review is
//!   one review's text and its inline comments. A batch is one per session,
//!   kind and key, and each item in it is one per GitHub id, so a
//!   redelivered webhook, or the same event seen twice, adds nothing.
//! - **Gathering, then settled once** ([`Db::settle_feedback`]): one
//!   transaction takes a due batch and the session, and sends it (a
//!   prompt queued, and a CI round counted), refuses it (the session had
//!   its [`MAX_CI_ROUNDS`]), or drops it (the session ended, or a review
//!   said nothing). What comes for a batch after that is late, and kept
//!   out.
//! - **Not required** (Arugula #210, `0060_ci_rounds_required`): a CI
//!   batch none of whose failed checks the base branch requires is dropped
//!   `not_required`, and spends no round. Unlike the rest it isn't
//!   settled for good: a check that fails on its commit later opens it
//!   again, gathering, so a required check that fails after an outside one
//!   still makes a round of both.
//! - **GitHub's text** stays GitHub's: the prompt is
//!   [`Source::Github`], by `github:<login>`, with its
//!   `author_association`, so the agent gets it fenced as quoted data.

use anyhow::{Context as _, Result};

use super::{
    Db,
    sessions::{COLS, NewPrompt, Session, Source, queue_in, session_for_update, session_row},
    sql::{Optional as _, Row, Sql, params},
};

/// The most CI rounds a session gets: after that a person takes over.
pub const MAX_CI_ROUNDS: u32 = 2;

/// Which kind of feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackKind {
    /// The failed checks of one head commit.
    Ci,
    /// One review: its text and inline comments.
    Review,
}

impl FeedbackKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FeedbackKind::Ci => "ci",
            FeedbackKind::Review => "review",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [FeedbackKind::Ci, FeedbackKind::Review].into_iter().find(|k| k.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackState {
    Gathering,
    Sent,
    /// The session had its CI rounds.
    Refused,
    /// The session had ended, or a review said nothing.
    Dropped,
}

impl FeedbackState {
    pub fn as_str(self) -> &'static str {
        match self {
            FeedbackState::Gathering => "gathering",
            FeedbackState::Sent => "sent",
            FeedbackState::Refused => "refused",
            FeedbackState::Dropped => "dropped",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [FeedbackState::Gathering, FeedbackState::Sent, FeedbackState::Refused, FeedbackState::Dropped]
            .into_iter()
            .find(|k| k.as_str() == s)
    }
}

/// A batch of a session's feedback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feedback {
    /// `sfb_` and 32 hex characters.
    pub id: String,
    pub session: String,
    pub kind: FeedbackKind,
    /// A CI round's head sha, or GitHub's id for the review.
    pub batch: String,
    /// The PR's number, when GitHub said.
    pub number: Option<u64>,
    /// A review's author, as GitHub said.
    pub login: Option<String>,
    pub github_id: Option<u64>,
    pub association: Option<String>,
    /// `approved`, `changes_requested` or `commented`.
    pub review_state: Option<String>,
    pub state: FeedbackState,
    pub due_at: u64,
    /// A sent CI round's number, from 1.
    pub round: Option<u32>,
    pub prompt: Option<String>,
    pub created_at: u64,
    pub done_at: Option<u64>,
    /// A CI batch dropped because none of its checks is required: it
    /// opens again when another check fails.
    pub not_required: bool,
}

/// One thing a batch gathered.
#[derive(Clone, PartialEq, Eq)]
pub struct Item {
    /// `check_run:<id>`, `status:<id>`, `review:<id>` or `comment:<id>`.
    pub item: String,
    /// A check's name, or an inline comment's file; empty for a review's
    /// own text.
    pub name: String,
    /// An inline comment's line.
    pub line: Option<u64>,
    pub conclusion: Option<String>,
    pub url: Option<String>,
    /// A check's output's end, or a review's or comment's text. GitHub's.
    pub text: String,
    pub at: u64,
    /// The GitHub App whose check run it is (`github-actions` for an
    /// Actions job), as its webhook said.
    pub app: Option<String>,
}

/// Everything but the text, which is never printed.
impl std::fmt::Debug for Item {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Item")
            .field("item", &self.item)
            .field("line", &self.line)
            .field("conclusion", &self.conclusion)
            .field("app", &self.app)
            .field("text", &format_args!("<{} bytes>", self.text.len()))
            .finish_non_exhaustive()
    }
}

/// The batch an item goes in, as its webhook says it.
#[derive(Debug, Clone, Copy)]
pub struct NewFeedback<'a> {
    pub session: &'a str,
    pub kind: FeedbackKind,
    pub batch: &'a str,
    pub number: Option<u64>,
    pub login: Option<&'a str>,
    pub github_id: Option<u64>,
    pub association: Option<&'a str>,
    pub review_state: Option<&'a str>,
    /// When it's sent, if this item starts it.
    pub due_at: u64,
}

/// What came of gathering an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gathered {
    /// In its batch now.
    Added(Feedback),
    /// It was there already: a redelivery.
    Already(Feedback),
    /// Its batch was settled already: kept out.
    Late(Feedback),
}

/// How a due batch was settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settled {
    /// Sent, as this prompt (a CI round numbered `round`).
    Sent { prompt: String, round: Option<u32> },
    /// The session had its CI rounds: not sent.
    Refused,
    /// The session had ended: not sent.
    Ended,
    /// A review that said nothing to act on.
    Nothing,
    /// A CI batch none of whose checks the base branch requires: not sent,
    /// no round spent.
    NotRequired,
}

/// What a due batch comes to, as `sessions::feedback` reads it.
pub enum Answer {
    /// Send it as this prompt (refused instead when it's a CI round and
    /// the session had its rounds).
    Send(Prompt),
    /// Nothing in it to act on.
    Nothing,
    /// A CI batch with no required check in it.
    NotRequired,
}

/// What a due batch's prompt is: its text, and who it's by
/// (`github:<login>`) with their association.
pub struct Prompt {
    pub author: String,
    pub association: String,
    pub text: String,
}

fn new_id() -> String {
    format!("sfb_{}", hex::encode(arugula_e2e::random::<16>()))
}

const FB_COLS: &str = "id, session_id, kind, batch, number, login, github_id, association, review_state, state, \
                       due_at, round, prompt_id, created_at, done_at, not_required";
const ITEM_COLS: &str = "item, name, line, conclusion, url, text, at, app";

fn feedback_row(r: &Row) -> Result<Feedback> {
    let (kind, state): (String, String) = (r.get(2)?, r.get(9)?);
    Ok(Feedback {
        id: r.get(0)?,
        session: r.get(1)?,
        kind: FeedbackKind::parse(&kind).with_context(|| format!("feedback of kind {kind:?}"))?,
        batch: r.get(3)?,
        number: r.get(4)?,
        login: r.get(5)?,
        github_id: r.get(6)?,
        association: r.get(7)?,
        review_state: r.get(8)?,
        state: FeedbackState::parse(&state).with_context(|| format!("feedback in state {state:?}"))?,
        due_at: r.get(10)?,
        round: r.get(11)?,
        prompt: r.get(12)?,
        created_at: r.get(13)?,
        done_at: r.get(14)?,
        not_required: r.get(15)?,
    })
}

fn item_row(r: &Row) -> Result<Item> {
    Ok(Item {
        item: r.get(0)?,
        name: r.get(1)?,
        line: r.get(2)?,
        conclusion: r.get(3)?,
        url: r.get(4)?,
        text: r.get(5)?,
        at: r.get(6)?,
        app: r.get(7)?,
    })
}

fn items_in(c: &impl Sql, id: &str) -> Result<Vec<Item>> {
    c.query_map(
        &format!("SELECT {ITEM_COLS} FROM session_feedback_items WHERE feedback_id = ?1 ORDER BY at, item"),
        params![id],
        item_row,
    )
}

/// Whether batch `id` gathered `item`.
fn has_item(c: &impl Sql, id: &str, item: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT 1 FROM session_feedback_items WHERE feedback_id = ?1 AND item = ?2",
        params![id, item],
        |_| Ok(()),
    )
    .optional()?
    .is_some())
}

/// Session `session`'s batch of `kind` and key `batch`, locked for the
/// caller's transaction.
fn batch_for_update(c: &impl Sql, session: &str, kind: FeedbackKind, batch: &str) -> Result<Option<Feedback>> {
    c.query_row(
        &format!(
            "SELECT {FB_COLS} FROM session_feedback WHERE session_id = ?1 AND kind = ?2 AND batch = ?3{}",
            c.for_update()
        ),
        params![session, kind.as_str(), batch],
        feedback_row,
    )
    .optional()
}

/// Make the batch `new` names, if it isn't there, in `state`; and fill
/// what a review's later webhook says that its first didn't.
fn make(c: &impl Sql, new: &NewFeedback, state: FeedbackState, now: u64) -> Result<Feedback> {
    let done = (state != FeedbackState::Gathering).then_some(now);
    c.execute(
        "INSERT INTO session_feedback (id, session_id, kind, batch, number, login, github_id, association,
             review_state, state, due_at, created_at, done_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT (session_id, kind, batch) DO NOTHING",
        params![
            new_id(),
            new.session,
            new.kind.as_str(),
            new.batch,
            new.number,
            new.login,
            new.github_id,
            new.association,
            new.review_state,
            state.as_str(),
            new.due_at,
            now,
            done
        ],
    )?;
    c.execute(
        "UPDATE session_feedback SET number = COALESCE(number, ?4), login = COALESCE(login, ?5),
             github_id = COALESCE(github_id, ?6), association = COALESCE(association, ?7),
             review_state = COALESCE(?8, review_state)
         WHERE session_id = ?1 AND kind = ?2 AND batch = ?3",
        params![
            new.session,
            new.kind.as_str(),
            new.batch,
            new.number,
            new.login,
            new.github_id,
            new.association,
            new.review_state
        ],
    )?;
    batch_for_update(c, new.session, new.kind, new.batch)?.context("the batch it made")
}

impl Db {
    /// The newest session in a project that links repo `repo_id` (as
    /// `owner/name`, `repo`) whose branch is `branch`: a session's branch
    /// is one per project.
    pub fn session_on_branch(&self, repo_id: u64, repo: &str, branch: &str) -> Result<Option<Session>> {
        self.c()
            .query_row(
                &format!(
                    "SELECT {COLS} FROM agent_sessions
                     WHERE branch = ?2 AND lower(repo) = lower(?3) AND project_id IN (
                         SELECT project_id FROM linked_repos WHERE repo_id = ?1 AND unlinked_at IS NULL)
                     ORDER BY created_at DESC, id DESC LIMIT 1"
                ),
                params![repo_id, branch, repo],
                session_row,
            )
            .optional()
    }

    /// Put `item` in the batch `new` names, made `gathering` (due at
    /// `new.due_at`) if it isn't there.
    pub fn gather_feedback(&self, new: &NewFeedback, item: &Item, now: u64) -> Result<Gathered> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let mut fb = make(&tx, new, FeedbackState::Gathering, now)?;
        if fb.not_required && fb.state == FeedbackState::Dropped && !has_item(&tx, &fb.id, &item.item)? {
            // A check failed after the batch was dropped as not required:
            // it gathers again, with what it had.
            tx.execute(
                "UPDATE session_feedback SET state = 'gathering', not_required = FALSE, due_at = ?2, done_at = NULL
                 WHERE id = ?1",
                params![fb.id, new.due_at],
            )?;
            fb = batch_for_update(&tx, new.session, new.kind, new.batch)?.context("the batch it opened")?;
        }
        if fb.state != FeedbackState::Gathering {
            return Ok(Gathered::Late(fb));
        }
        let added = tx.execute(
            &format!(
                "INSERT INTO session_feedback_items (feedback_id, {ITEM_COLS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (feedback_id, item) DO NOTHING"
            ),
            params![fb.id, item.item, item.name, item.line, item.conclusion, item.url, item.text, item.at, item.app],
        )?;
        tx.commit()?;
        Ok(if added == 1 { Gathered::Added(fb) } else { Gathered::Already(fb) })
    }

    /// The batch `new` names for a session that ended: made `dropped` if it
    /// isn't there. `true` when this made it (say so once).
    pub fn drop_feedback(&self, new: &NewFeedback, now: u64) -> Result<bool> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let there = batch_for_update(&tx, new.session, new.kind, new.batch)?.is_some();
        if !there {
            make(&tx, new, FeedbackState::Dropped, now)?;
        }
        tx.commit()?;
        Ok(!there)
    }

    /// Make a gathering batch due at `now` (its check suite completed):
    /// `false` when there's none.
    pub fn feedback_due(&self, session: &str, kind: FeedbackKind, batch: &str, now: u64) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE session_feedback SET due_at = ?4
             WHERE session_id = ?1 AND kind = ?2 AND batch = ?3 AND state = 'gathering' AND due_at > ?4",
            params![session, kind.as_str(), batch, now],
        )? == 1)
    }

    /// A session's feedback, oldest first.
    pub fn session_feedback(&self, session: &str) -> Result<Vec<Feedback>> {
        self.c().query_map(
            &format!("SELECT {FB_COLS} FROM session_feedback WHERE session_id = ?1 ORDER BY created_at, id"),
            params![session],
            feedback_row,
        )
    }

    /// What a batch gathered, oldest first.
    pub fn feedback_items(&self, id: &str) -> Result<Vec<Item>> {
        items_in(&self.c(), id)
    }

    /// When a session's next gathering batch is due, if it has one.
    pub fn next_feedback_due(&self, session: &str) -> Result<Option<u64>> {
        let due: Option<Option<u64>> = self
            .c()
            .query_row(
                "SELECT min(due_at) FROM session_feedback WHERE session_id = ?1 AND state = 'gathering'",
                params![session],
                |r| r.get(0),
            )
            .optional()?;
        Ok(due.flatten())
    }

    /// The ids of a session's gathering batches due by `now`, oldest first.
    pub fn due_feedback(&self, session: &str, now: u64) -> Result<Vec<String>> {
        self.c().query_map(
            "SELECT id FROM session_feedback WHERE session_id = ?1 AND state = 'gathering' AND due_at <= ?2
             ORDER BY due_at, id",
            params![session, now],
            |r| r.get(0),
        )
    }

    /// Settle batch `id`, if it's gathering and due by `due_by` (module
    /// docs), at `now`, in one transaction: `answer` says what it comes to
    /// ([`Answer`]).
    /// The batch, as it is after, and how; `None` when it wasn't there to
    /// settle.
    pub fn settle_feedback(
        &self,
        id: &str,
        due_by: u64,
        now: u64,
        answer: impl FnOnce(&Feedback, &[Item], &Session) -> Answer,
    ) -> Result<Option<(Feedback, Vec<Item>, Settled)>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let fb = tx
            .query_row(
                &format!(
                    "SELECT {FB_COLS} FROM session_feedback WHERE id = ?1 AND state = 'gathering' AND due_at <= ?2{}",
                    tx.for_update()
                ),
                params![id, due_by],
                feedback_row,
            )
            .optional()?;
        let Some(fb) = fb else { return Ok(None) };
        let s = session_for_update(&tx, &fb.session)?.context("a batch's session")?;
        let items = items_in(&tx, &fb.id)?;
        let end = |state: FeedbackState, prompt: Option<&str>, round: Option<u32>| {
            tx.execute(
                "UPDATE session_feedback SET state = ?2, prompt_id = ?3, round = ?4, done_at = ?5 WHERE id = ?1",
                params![fb.id, state.as_str(), prompt, round, now],
            )
        };
        let settled = if s.state.terminal() {
            end(FeedbackState::Dropped, None, None)?;
            Settled::Ended
        } else {
            match answer(&fb, &items, &s) {
                Answer::Nothing => {
                    end(FeedbackState::Dropped, None, None)?;
                    Settled::Nothing
                }
                Answer::NotRequired => {
                    end(FeedbackState::Dropped, None, None)?;
                    tx.execute("UPDATE session_feedback SET not_required = TRUE WHERE id = ?1", params![fb.id])?;
                    Settled::NotRequired
                }
                Answer::Send(_) if fb.kind == FeedbackKind::Ci && s.ci_rounds >= MAX_CI_ROUNDS => {
                    end(FeedbackState::Refused, None, None)?;
                    Settled::Refused
                }
                Answer::Send(p) => {
                    let new = NewPrompt {
                        author: &p.author,
                        source: Source::Github,
                        association: Some(&p.association),
                        text: &p.text,
                    };
                    let queued = match queue_in(&tx, &s, &new, now)? {
                        Ok(q) => q,
                        Err(why) => anyhow::bail!("queueing a session's feedback: {why}"),
                    };
                    let round = match fb.kind {
                        FeedbackKind::Ci => Some(tx.query_row(
                            "UPDATE agent_sessions SET ci_rounds = ci_rounds + 1, updated_at = ?2 WHERE id = ?1
                             RETURNING ci_rounds",
                            params![s.id, now],
                            |r| r.get::<u32>(0),
                        )?),
                        FeedbackKind::Review => None,
                    };
                    end(FeedbackState::Sent, Some(&queued.id), round)?;
                    Settled::Sent { prompt: queued.id, round }
                }
            }
        };
        let after = tx.query_row(
            &format!("SELECT {FB_COLS} FROM session_feedback WHERE id = ?1"),
            params![fb.id],
            feedback_row,
        )?;
        tx.commit()?;
        Ok(Some((after, items, settled)))
    }
}

impl Db {
    /// Whether any of `sha`'s check runs on repo `repo_id` is still going,
    /// as the mirror has them.
    pub fn checks_running(&self, repo_id: u64, sha: &str) -> Result<bool> {
        Ok(self
            .c()
            .query_row(
                "SELECT 1 FROM forge_checks WHERE repo_id = ?1 AND head_sha = ?2 AND kind = 'check_run'
                     AND status <> 'completed' LIMIT 1",
                params![repo_id, sha],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}
