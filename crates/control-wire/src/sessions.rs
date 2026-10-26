//! An agent session's status card (M257, thread = session, Arugula #129):
//! what control's page draws for a session, live, in the thread it was
//! asked in and on its own page (`/p/:project/sessions/:ses`), and what
//! people do from it.
//!
//! - **The card's message.** When a session is made the platform posts one
//!   message for it in its thread, with no author and `via`
//!   `session:<ses>` ([`card_via`]): the one card a chat timeline shows
//!   (aru-115). The message stays one row; the card is drawn from the
//!   session as it is now ([`SESSION`]), and read again whenever the
//!   session changes ([`crate::chat::ChatLive::Session`]).
//! - **Who:** any member of the session's project reads it; its owners and
//!   editors stop it, cancel its turn and answer what it asks
//!   ([`SessionView::may`]).
//!
//! | Route | Who |
//! |---|---|
//! | `GET` [`SESSION`]: [`SessionView`] | its project's members |
//! | `POST` [`SESSION_STOP`]: [`SessionView`] | owners and editors |
//! | `POST` [`SESSION_ACTIONS`] [`SessionAct`]: [`SessionView`] | owners and editors |
//! | `GET` [`SESSION_PR`]: [`SessionPr`] | its asker |
//! | `PUT` [`SESSION_PR`] [`SessionPrEdit`]: [`SessionPr`] | its asker |
//! | `POST` [`SESSION_PR_SEND`] [`SessionPrSend`]: [`SessionPrSent`] | its asker, as a person |
//! | `POST` [`SESSION_TRY`] [`SessionTryAsk`]: [`SessionView`] | owners and editors who may start one |
//! | `POST` [`SESSION_PICK`] [`SessionPick`]: [`SessionView`] | owners and editors |
//! | `GET` [`PROJECT_SESSION_STATS`]: [`SessionStats`] | the project's members |
//! | `POST` [`ISSUE_AGENT`]: [`IssueAgent`] | the project's owners and editors, as `@arugula` |
//!
//! **Try 3** (M264). From a session's card, two siblings of it start: new
//! sessions under it (`parent_session`), on the same thread, box and repo,
//! each re-running its first prompt in a slot of its own, on the harness
//! asked for (`claude`, `codex`). The card's compare view lists the three,
//! the session first ([`SessionCard::tries`]): each one's diffstat, how its
//! turn finished, and its page. **Pick** keeps one: the others stop, "not
//! picked". A picked sibling is the thread's session from then on, with
//! its own card in the thread.
//!
//! **Done when its PR merges** (M265). A PR from the session's branch
//! merging (whoever opened it) ends the session, `stopped` saying
//! `merged`, once a turn under way is done; its card says "merged ✓"
//! ([`SessionCard::merged_at`]), and its thread leaves the Threads view.
//!
//! **Agent on this** ([`ISSUE_AGENT`]) writes `@arugula` in a native
//! issue's thread as the person who pressed it: everything after is what
//! that mention does (which box, which repo, the session and its card).
//!
//! **Its pull request** (M259). Once a turn pushed the session's branch,
//! its asker drafts the PR from the card: the title and body come
//! prefilled from the repo's PR template and what the turns left (commits,
//! diffstat), and they edit both before sending. Only the asker sends it,
//! as themselves ("agents draft, people send", aru-041): with a one-use
//! user token through GitHub's authorization (`via: "browser"`, as a
//! forge draft's send), else through one of their own machines. A machine
//! never sends. Once sent, the session's card links the PR, and the PR's
//! thread gets a line that links back to the session.

use serde::{Deserialize, Serialize};

/// `GET`: a session's card and history.
pub const SESSION: &str = "/api/sessions/{ses}";
/// `POST`: stop it (control's `stopped`; its box stops the agent).
pub const SESSION_STOP: &str = "/api/sessions/{ses}/stop";
/// `POST` [`SessionAct`]: ask its box to answer, approve, deny or cancel
/// (`arugula_control_wire::work::SessionAction`).
pub const SESSION_ACTIONS: &str = "/api/sessions/{ses}/actions";
/// `GET`: its PR as its asker drafts it; `PUT` [`SessionPrEdit`]: keep it.
pub const SESSION_PR: &str = "/api/sessions/{ses}/pr";
/// `POST` [`SessionPrSend`]: send it, as its asker.
pub const SESSION_PR_SEND: &str = "/api/sessions/{ses}/pr/send";
/// `POST` [`SessionTryAsk`]: Try 3, two siblings beside it (M264).
pub const SESSION_TRY: &str = "/api/sessions/{ses}/try";
/// `POST` [`SessionPick`]: keep one of a Try 3's sessions; the others stop.
pub const SESSION_PICK: &str = "/api/sessions/{ses}/pick";

/// `GET`: a project's agent sessions in numbers ([`SessionStats`], M265).
pub const PROJECT_SESSION_STATS: &str = "/api/projects/{project}/sessions/stats";
/// `POST`: "Agent on this" on a native issue ([`IssueAgent`], M265).
pub const ISSUE_AGENT: &str = "/api/issues/{issue}/agent";

/// How many days [`SessionStats`] looks back.
pub const STATS_DAYS: u32 = 30;

/// What a card's message has for `via`, before the session's id.
pub const CARD_VIA: &str = "session:";

/// The `via` of session `ses`'s card message.
pub fn card_via(ses: &str) -> String {
    format!("{CARD_VIA}{ses}")
}

/// The session a message's `via` is the card of, if it's one.
pub fn card_of(via: &str) -> Option<&str> {
    via.strip_prefix(CARD_VIA).filter(|s| s.starts_with("ses_"))
}

/// A session as its card shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionCard {
    pub session: String,
    pub project: String,
    /// Its thread: the room and the message it hangs off.
    pub room: String,
    pub parent: u64,
    /// `queued`, `working`, `needs_input`, `done`, `failed`,
    /// `unresponsive` or `stopped`.
    pub state: String,
    /// An `unresponsive` one's state before its box went quiet.
    pub was: Option<String>,
    /// Who asked, and their name now ("" once they're gone).
    pub asker: String,
    pub asker_name: String,
    /// The hosted workspace it runs on, and its name ("" once it's gone).
    pub workspace: String,
    pub workspace_name: String,
    pub repo: Option<String>,
    pub branch: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// When the platform last heard from its box.
    pub heard_at: Option<u64>,
    /// When it last stopped running (done, failed, stopped): the clock
    /// stops there. `None` while a turn is under way or queued.
    pub ended_at: Option<u64>,
    pub last_tool: Option<String>,
    pub last_error: Option<String>,
    /// Why it failed or stopped, or why it's queued again (its box was
    /// rebuilt).
    pub reason: Option<String>,
    /// What its latest turn left.
    pub turn: Option<SessionTurn>,
    /// What it asks, while it waits on a person.
    pub open: Option<SessionOpen>,
    /// The answer to it that its box hasn't taken yet.
    pub answered: Option<SessionAnswered>,
    /// Someone asked its box to cancel the turn, and it hasn't yet.
    pub cancelling: bool,
    /// Its box and agent block there, once the box took it: Open goes there.
    pub machine: Option<String>,
    pub block: Option<String>,
    /// Its workspace's dev servers (Preview), the public ones with their
    /// URL.
    pub ports: Vec<SessionPort>,
    /// The PR its asker sent (M259): its URL.
    pub pr: Option<String>,
    /// When its PR merged (M265): the thread is done, and the session ends
    /// once a turn under way does. Its card says "merged ✓".
    #[serde(default)]
    pub merged_at: Option<u64>,
    /// A turn pushed its branch: its asker may draft its PR.
    pub pushed: bool,
    /// How many CI rounds went back to it (M260), of at most `ci_max`.
    #[serde(default)]
    pub ci_rounds: u32,
    #[serde(default)]
    pub ci_max: u32,
    /// CI failed again after its last round, and nobody in its thread has
    /// prompted it since: it needs a person.
    #[serde(default)]
    pub needs_person: bool,
    /// What it is (M264): `thread` (the thread's own, `@arugula`), `try` (a
    /// Try 3's sibling of `parent_session`) or `review` (`@codex review`
    /// of `parent_session`).
    pub role: String,
    pub parent_session: Option<String>,
    /// The agent asked for (`claude`, `codex`), and the one its box ran
    /// it on, once it took it (`claude` when the other isn't there).
    pub harness: String,
    pub ran: Option<String>,
    /// A Try 3's sessions, on its parent's card: the parent first, then its
    /// siblings, oldest first. Empty for every other session.
    pub tries: Vec<SessionTry>,
    /// The one of [`SessionCard::tries`] kept, once one is picked; on a
    /// sibling, its parent's pick.
    pub picked: Option<String>,
}

/// One of a Try 3's sessions, as its parent's compare view shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionTry {
    pub session: String,
    pub state: String,
    pub branch: String,
    pub harness: String,
    pub ran: Option<String>,
    /// What its latest turn left: its diffstat and how its finish step went.
    pub turn: Option<SessionTurn>,
    pub pushed: bool,
    /// Why it failed or stopped.
    pub reason: Option<String>,
    pub updated_at: u64,
    /// Queued with no slot on its box for it yet (the box holds three
    /// sessions, done ones too).
    pub no_slot: bool,
}

/// [`SESSION_TRY`]'s body: the harnesses of the two siblings, `claude`
/// or `codex` each (`["claude", "codex"]` when it's left out; a box
/// without Codex runs Claude Code in its place).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionTryAsk {
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub harnesses: Option<Vec<String>>,
}

/// [`SESSION_PICK`]'s body: the session kept, one of the Try 3's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionPick {
    pub session: String,
}

/// What a session's turn left, as its box reported it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionTurn {
    pub at: u64,
    pub pushed: bool,
    pub head: Option<String>,
    /// `"SHA subject"`, newest first.
    pub commits: Vec<String>,
    /// `git diff --stat`, shortened.
    pub diffstat: Option<String>,
    pub uncommitted: Option<u32>,
    /// What the workspace's finish step did after it (M261), when it has
    /// one.
    pub finish: Option<SessionFinish>,
}

/// A turn's finish step (M261): the command, whether it passed, and when
/// it failed, the last of its output. A failed one isn't pushed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionFinish {
    pub ok: bool,
    pub command: String,
    pub output_tail: String,
}

/// What a session waits on a person for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionOpen {
    /// The request's id, as its box named it.
    pub request: Option<String>,
    /// `permission` or `question` (`None`: the box didn't say).
    pub ask: Option<String>,
    /// The words shown.
    pub question: Option<String>,
    /// A permission's option ids.
    pub options: Vec<String>,
    pub at: u64,
}

/// An answer to what's open, on its way to the box.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionAnswered {
    pub by: String,
    pub name: String,
    /// `answer`, `approve` or `deny`.
    pub kind: String,
    pub at: u64,
}

/// One of a workspace's dev servers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionPort {
    pub port: u16,
    pub label: String,
    /// Its public preview's URL, while it's public.
    pub url: Option<String>,
}

/// One entry of a session's history, in words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionLine {
    pub seq: u64,
    /// `state`, `error` or `turn`.
    pub kind: String,
    pub from: Option<String>,
    pub to: String,
    pub at: u64,
    /// Its box reported it, signed (else control recorded it).
    pub signed: bool,
    /// What it says, in a line.
    pub text: String,
}

/// [`SESSION`]'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionView {
    pub card: SessionCard,
    /// The reader may stop it and answer it: an owner or editor of its
    /// project.
    pub may: bool,
    /// The reader asked for it: only they send its PR.
    pub asked: bool,
    /// The platform's clock when it answered, so the page can tell a box
    /// that went quiet whatever its own clock says.
    pub now: u64,
    /// Its history, oldest first.
    pub events: Vec<SessionLine>,
}

/// [`SESSION_ACTIONS`]'s body: what to ask of the session's box.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionAct {
    /// `answer`, `approve`, `deny` or `cancel`.
    pub kind: String,
    /// The open request it answers (the card's `open.request`).
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub request: Option<String>,
    /// `answer`'s: the question card's answer (`question_<n>`).
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional, type = "Record<string, unknown>"))]
    pub answer: Option<serde_json::Value>,
    /// `approve`'s: one of the permission's options.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub option: Option<String>,
    /// `deny`'s: why.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub reason: Option<String>,
}

/// A session's pull request, as its asker drafts it ([`SESSION_PR`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionPr {
    pub session: String,
    /// `pdr_…`, once it's kept.
    pub id: Option<String>,
    /// `owner/name`.
    pub repo: String,
    /// The session's branch, which the PR is from.
    pub head: String,
    /// The branch it goes into: the repo's default, unless its asker says.
    pub base: String,
    pub title: String,
    pub body: String,
    /// Opened as a draft PR on GitHub.
    pub draft: bool,
    /// `new` (not kept yet: prefilled), `waiting`, `sending`, `sent` or
    /// `failed`.
    pub state: String,
    /// Why the last send didn't get there.
    pub error: Option<String>,
    /// The PR, once sent.
    pub url: Option<String>,
    pub number: Option<u64>,
    /// The repo's PR template the body started from, by its path.
    pub template: Option<String>,
}

/// [`SESSION_PR`]'s `PUT` body: what its asker wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionPrEdit {
    pub title: String,
    pub body: String,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub draft: Option<bool>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub base: Option<String>,
}

/// [`SESSION_PR_SEND`]'s body: what to send (kept first, when given), and
/// how: `browser` (GitHub's authorization in this browser, coming back to
/// `back`, a path on the platform page) or `machine` (one of the asker's
/// own).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionPrSend {
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub edit: Option<SessionPrEdit>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub via: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub back: Option<String>,
}

/// [`SESSION_PR_SEND`]'s answer: the PR draft now, and GitHub's
/// authorization page to go to when the send goes through the browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionPrSent {
    pub pr: SessionPr,
    pub authorize: Option<String>,
    /// `token` or `machine`, once sent.
    pub via: Option<String>,
    /// The machine that sent it.
    pub machine: Option<String>,
}

/// [`PROJECT_SESSION_STATS`]'s answer: of the project's sessions asked for
/// in the last [`STATS_DAYS`] days, how many led to a PR (sent by its
/// asker, or opened from its branch by hand) and how many merged, and the
/// median time from the `@` to the PR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionStats {
    pub days: u32,
    pub sessions: u32,
    pub prs_opened: u32,
    pub prs_merged: u32,
    /// `None` until one led to a PR.
    pub median_to_pr_ms: Option<u64>,
}

/// [`ISSUE_AGENT`]'s answer: the issue's thread and the message that asked,
/// and the session it started, if it did (else the thread says why, or
/// asks which box or repo).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IssueAgent {
    pub room: String,
    pub seq: i64,
    pub session: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_message_names_its_session() {
        assert_eq!(card_via("ses_ab"), "session:ses_ab");
        assert_eq!(card_of("session:ses_ab"), Some("ses_ab"));
        for not in ["system", "hud box", "session:", "session:run_1"] {
            assert_eq!(card_of(not), None, "{not}");
        }
    }
}
