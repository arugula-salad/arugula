//! Agent sessions on a hosted machine (M255, thread = session, Arugula
//! #129): the box's half of `arugula_control_wire::work`'s sessions, as S42
//! (`spikes/s42-thread-sessions`) found they must run.
//!
//! - **Pulled with runs.** A box whose template has session users says it
//!   takes sessions ([`takes`]); each pull's [`SessionWork`]s are kept as
//!   the latest word on each ([`pulled`]). One this box runs that's no
//!   longer listed has ended (a person stopped it): its agent is stopped.
//! - **A user each, at most six; three running** ([`Slots`]). A session
//!   runs as one of the box's session users (`ses1` to `ses6`, the
//!   template's: [`MAX_HELD_SESSIONS_PER_BOX`]), which it keeps for life:
//!   its checkout and its agent's own state (`~/.claude`, the ACP session
//!   a restart loads) live in that user's home. At most
//!   [`MAX_SESSIONS_PER_BOX`] have their agent block open at once. With
//!   every user taken, or three running, a queued session waits, listed,
//!   until one ends or parks. Each pull says how many users it has
//!   (`session_users:N`), and control holds no more there. Runs keep their
//!   one at a time beside them.
//! - **Parked** ([`park`], Arugula #215). A done session control lists as
//!   `park` (idle 30 minutes, `SESSION_PARK_MS`) has its transcript
//!   archived (`hosted.archive`'s path, [`crate::hosted::archive::flush`]),
//!   then its agent block closed and what its user left killed: that frees
//!   the memory, and a running slot. Its user, checkout and `~/.claude`
//!   stay. When control lists prompts for it again (only while it has a
//!   running slot for it), the box opens a block on the same user and
//!   checkout with the agent's session id ([`unpark`]), which loads the
//!   conversation as a restart does, and its next turn names the new
//!   block.
//! - **Its checkout is a clone, not a worktree** ([`checkout`]): the
//!   daemon's user clones the workspace's clone of the session's repo,
//!   moves it to the repo's default branch as GitHub has it now (Arugula
//!   #263: the clone is as old as the box's setup), hands it to the session
//!   user, and everything the daemon does in it
//!   after that (switching to the session's branch, the turn's work) runs
//!   as the session user, never as `user` (who can't get in) or root
//!   (whose git would run the agent's planted repo config). A repo linked
//!   to the project that the box's setup didn't clone is cloned from
//!   GitHub instead, shallow, with a token control mints for that clone
//!   ([`push::clone_linked`], Fleet 6).
//! - **Its agent is an ACP agent block** of kind `claude` (so M185's turn
//!   gate and placeholder key hold), whose agent runs as the session user
//!   (`agent::Config::run_as`): `sudo -n -u sesN -- env -i …`, with nothing
//!   secret on its command line, and not `/mcp`: the daemon's port is
//!   closed to it. Its one MCP server is `mcp::session`'s, two tools for
//!   its project's native issues, on a listener of its own.
//! - **One prompt per turn, with its author** ([`hand`]). The box hands
//!   the block a queued prompt only when no turn is under way, oldest first,
//!   as its author's (the block's transcript says whose). The first also
//!   carries the repo's `CLAUDE.md` and `AGENTS.md` as text: Claude Code's
//!   `settingSources: ["project"]` would load the repo's hooks too.
//! - **Its block is followed, not only what it hands in** ([`step`]). A
//!   turn started in the block itself (someone typed there from the card's
//!   Open, or the agent woke itself) is reported as a turn's start
//!   (`in_block`, with who typed it when the block says, and the start of
//!   what they typed, for the thread's line), and goes to the
//!   same end as the box's own: the finish step, the push, the turn's
//!   report. A question it asks goes out as `needs_input`; one answered in
//!   the block moves it back with no action named. The thread's prompts
//!   wait for it.
//! - **Nothing asks permission** ([`agent_config`]): Claude Code runs in
//!   `bypassPermissions` and Codex in `full-access`, and the block allows
//!   what still asks. Questions (AskUserQuestion, Codex's elicitations)
//!   aren't permissions, and still go to the card.
//! - **At most once.** Before a prompt goes to the block, the ledger
//!   (`<state>/work/sessions/ID.json`, fsynced) says it's handed; after a
//!   restart between the two, the block's own log (its queue and turns)
//!   says whether it got there. A prompt is never handed twice, and one in
//!   the ledger is never handed again, whatever control lists.
//! - **Reported, signed** ([`Session::push`]): the moves it makes
//!   (`queued`/`done` to `working` with the turn's prompts, to
//!   `needs_input` with what it asks, back, to `done`), a heartbeat every
//!   [`HEARTBEAT`] while a turn is under way with the tool running now, the
//!   agent's errors, and each turn's result (`turnwork`'s: branch, commits,
//!   diffstat, and `pushed`, with the agent's last message as its `reply`,
//!   which control posts in the session's thread). Each is kept in the
//!   ledger before it's sent, and sent in order until control takes it. A
//!   detail stays within what control takes ([`SESSION_DETAIL_MAX`]): the
//!   reply is cut further when the rest needs the room.
//! - **The finish step** ([`finish_turn`], M261): when the workspace's spec
//!   has one (`just check`), it runs after each turn, before the push, as
//!   the session's user in its checkout, with that user's environment
//!   only, for at most its timeout. Its result goes on the turn's report.
//!   When it fails, the agent gets one more turn, within the same
//!   `working`, with its output as a prompt from the finish step; the
//!   push waits for that turn's end, when it runs again. Failing twice, the
//!   session is done, unpushed, and its card says why.
//! - **Pushed** ([`push::push_turn`], M259): a turn that left commits of
//!   its own pushes the session's branch, and only it, from a bundle its
//!   user writes, through a repo of the daemon's own, with a token control
//!   mints for that one push. A push that fails is the session's error,
//!   and the turn says it didn't push.
//! - **Its harness** (M264): Claude Code, or Codex when control asks for
//!   it and the box has it ([`has_harness`]); else Claude Code, and the
//!   first report says which ran (`harness`). A Try 3's three sessions are
//!   three sessions here, each in its slot.
//! - **A review** (M264, `@codex review`) is read-only: no finish step and
//!   no push. Its turn's reply is the review, which control posts in its
//!   thread.
//! - **A slot is left clean** ([`clean_slot`]): when a session ends (or
//!   fails), what its pushes left, its checkout and its user's `~/.claude`
//!   go, so the next session in that slot starts with nothing of it.
//! - **People's answers** ([`SessionAction`]: a question's answer, a
//!   permission's approval or denial, cancelling the turn) are done once
//!   each, as the person who asked, and named in the next report.
//! - **Restarts.** The agent doesn't outlive the daemon (a hosted box keeps
//!   no FD store, so its pipes go), and what it left running as its user is
//!   killed ([`resume`]). Its block comes back, starts it again as the same
//!   user, and loads its session: a turn the restart cut off ends there,
//!   and what was queued behind it goes next.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write as _,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use arugula_control_wire::work::{
    DEFAULT_MODEL, FINISH_TIMEOUT_S, FinishResult, FinishStep, MAX_HELD_SESSIONS_PER_BOX, MAX_SESSIONS_PER_BOX,
    MODEL_MAX, MODELS, REPLY_MAX, Reported, SESSION_DETAIL_MAX, SessionAction, SessionDetail, SessionPrompt,
    SessionReport, SessionWork, TYPED_MAX, WORK_SESSION_REPORT,
};
use arugula_e2e::{
    now_ms,
    session::{Event, Kind, State},
};
use arugula_proto::{BlockType, api::OpenRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::{agent::hosted::AsUser, mux::Api, server::App};

/// Sessions' ledgers, in the run ledger's directory.
pub const DIR: &str = "sessions";
/// The mux session sessions' blocks open in.
pub const BLOCKS: &str = "sessions";
/// How often a session's block is looked at.
const FOLLOW: Duration = Duration::from_millis(250);
/// How often a turn under way says it's alive, with nothing else to say
/// (the platform marks it unresponsive after `session::QUIET_MS`).
pub const HEARTBEAT: Duration = Duration::from_secs(15);
/// How long a session's block may be missing (a restart restores it)
/// before the session fails.
const BLOCK_GRACE: Duration = Duration::from_secs(30);
/// The most of one of the repo's notes (`CLAUDE.md`) the first prompt
/// carries.
const NOTES_MAX: usize = 32 * 1024;
/// The most a session's checkout waits for its repo's head from GitHub
/// ([`fresh_base`]) before it starts from the box's clone.
const FRESH_MAX: Duration = Duration::from_secs(60);
/// Free space under which a prompt says the disk is low ([`disk_note`]).
const DISK_LOW: u64 = 10 << 30;
/// The most of any one text a report's detail carries: the whole detail is
/// at most 16 KiB.
const TEXT_MAX: usize = 4 * 1024;
/// The most of a tool's title a heartbeat sends, in bytes: control keeps
/// 200 characters (an older control refused a longer one, 0.63.3).
const TOOL_MAX: usize = 196;
/// The most a session's clone of a linked repo no box has may take
/// ([`push::clone_linked`]).
const CLONE_MAX: Duration = Duration::from_secs(300);
/// What a session whose block went away fails with.
pub const BLOCK_GONE: &str = "its agent block is gone";
/// The most of a finish step's output a turn's report carries, and its fix
/// prompt: the last of it.
const FINISH_OUTPUT_MAX: usize = 4 * 1024;
/// Who a finish step's fix prompt is from, in the block's transcript.
pub const FINISH_STEP: &str = "the finish step";

/// The latest pull's sessions, by id: `None` before the first.
static LATEST: Mutex<Option<BTreeMap<String, SessionWork>>> = Mutex::new(None);
/// Sessions followed now.
static FOLLOWING: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
/// Sessions with a turn under way: the box is busy.
static BUSY: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
/// One writer at a time for the ledgers.
static LEDGER: Mutex<()> = Mutex::new(());
/// Each session block's session, as this daemon opened or found it: what
/// its agent's turns are asked of the platform as ([`session_of`]).
static BLOCK_SESSIONS: Mutex<BTreeMap<u32, String>> = Mutex::new(BTreeMap::new());

/// The session agent block `block` runs, if it's a session's: the daemon
/// says so, never the block's own config.
pub fn session_of(block: u32) -> Option<String> {
    BLOCK_SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).get(&block).cloned()
}

fn block_is(block: u32, session: &str) {
    BLOCK_SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).insert(block, session.to_owned());
}

fn block_gone(block: u32) {
    BLOCK_SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).remove(&block);
}

fn heartbeat_every() -> Duration {
    std::env::var("ARUGULA_SESSION_HEARTBEAT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(HEARTBEAT, Duration::from_millis)
}

/// Whether a session has a turn under way on this box.
pub fn busy() -> bool {
    !BUSY.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
}

/// The box's session users, one per slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slots {
    pub users: Vec<AsUser>,
    /// The template's real Unix users, not a test's stand-ins: only then
    /// are they kept off loopback, handed checkouts with `chown` and have
    /// what they left killed.
    pub real: bool,
}

/// The box's session users: `ARUGULA_HOSTED_SESSION_USERS` (`ses1` to
/// `ses6` by default), each the template's, at most
/// [`MAX_HELD_SESSIONS_PER_BOX`]. A box without them runs no sessions (an
/// older template). A box from before the six has three, and runs and
/// holds three: no session parks to make room there. Tests name a
/// directory of homes instead (`ARUGULA_HOSTED_SESSION_HOMES`): the users
/// aren't made, and `ARUGULA_HOSTED_SUDO` stands in for sudo.
pub fn slots() -> Option<Slots> {
    static SLOTS: OnceLock<Option<Slots>> = OnceLock::new();
    SLOTS
        .get_or_init(|| {
            let names = std::env::var("ARUGULA_HOSTED_SESSION_USERS").unwrap_or_else(|_| default_users().join(","));
            let names: Vec<String> =
                names.split(',').map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned).collect();
            if names.is_empty() || !names.iter().all(|n| AsUser::plain(n)) {
                return None;
            }
            let names = names.into_iter().take(MAX_HELD_SESSIONS_PER_BOX);
            if let Some(homes) = std::env::var_os("ARUGULA_HOSTED_SESSION_HOMES").filter(|h| !h.is_empty()) {
                let homes = PathBuf::from(homes);
                let users = names.map(|user| AsUser { home: homes.join(&user).display().to_string(), user }).collect();
                return Some(Slots { users, real: false });
            }
            let mut users = vec![];
            for user in names {
                let Some(home) = home_of(&user) else {
                    // A template from before the six has the first three.
                    if users.len() >= 3 {
                        info!(user, held = users.len(), "hosted: fewer session users here (an older template)");
                        break;
                    }
                    info!(user, "hosted: no session user here (an older template): this box takes no sessions");
                    return None;
                };
                users.push(AsUser { user, home });
            }
            Some(Slots { users, real: true })
        })
        .clone()
}

/// The template's session users: `ses1` to `ses6`.
pub fn default_users() -> Vec<String> {
    (1..=MAX_HELD_SESSIONS_PER_BOX).map(|n| format!("ses{n}")).collect()
}

fn home_of(user: &str) -> Option<String> {
    let out = std::process::Command::new("getent").args(["passwd", user]).output().ok()?;
    let line = String::from_utf8_lossy(&out.stdout);
    line.trim_end().split(':').nth(5).filter(|h| h.starts_with('/') && *h != "/").map(str::to_owned)
}

/// Whether `u` is one of this box's session users, home and all: the only
/// users an agent block runs its agent as (`agent::Config::run_as`).
pub fn is_slot(u: &AsUser) -> bool {
    slots().is_some_and(|s| s.users.contains(u))
}

/// Whether this box takes sessions: it has session users.
pub fn takes() -> bool {
    slots().is_some()
}

/// The session users kept off loopback ([`super::firewall`]): real ones
/// only.
pub fn fenced() -> Vec<String> {
    slots().filter(|s| s.real).map(|s| s.users.into_iter().map(|u| u.user).collect()).unwrap_or_default()
}

/// A report, as signed and sent, and whether control has answered it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    pub report: SessionReport,
    #[serde(default)]
    pub answered: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
}

/// The turn under way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    /// The prompts it started with.
    pub prompts: Vec<String>,
    /// The block's turns before it.
    pub base: u64,
    /// The block has its prompt (else a restart came between the ledger
    /// and the block).
    pub sent: bool,
    pub at: u64,
    /// The finish step's fix (M261): this turn is the agent's one try at
    /// what the finish step said, and this is its prompt. Its end runs the
    /// finish step for the last time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// Someone cancelled it: a failed finish step gets no fix after it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cancelled: bool,
}

/// A session this box took, in `<state>/work/sessions/ID.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub session: String,
    pub project: String,
    pub workspace: String,
    pub asker: String,
    #[serde(default)]
    pub repo: Option<String>,
    pub branch: String,
    /// Its slot's user, which it keeps for life.
    pub user: AsUser,
    pub checkout: String,
    #[serde(default)]
    pub block: Option<u32>,
    pub taken_at: u64,
    /// This box's latest report number in it.
    pub n: u64,
    /// Its state by this box's own reports.
    pub state: String,
    /// It ended, and why: this box stops following it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<String>,
    /// Every prompt handed to its agent: none is handed again.
    #[serde(default)]
    pub handed: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<Turn>,
    /// The repo's notes for the first prompt, until it's sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Every action done: none is done again.
    #[serde(default)]
    pub acted: Vec<String>,
    /// Actions done that no report has named yet.
    #[serde(default)]
    pub unnamed: Vec<String>,
    /// The agent's latest error, as reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// When it last reported (ms).
    #[serde(default)]
    pub last_at: u64,
    #[serde(default)]
    pub events: Vec<Kept>,
    /// The commit its last push put on GitHub (M259).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed_head: Option<String>,
    /// The block's turns when the box last followed one to its end: one
    /// past that with none under way here was started in the block itself
    /// ([`step`]). `None` in a ledger from before it was kept, until the
    /// next look.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns_seen: Option<u64>,
    /// The agent it runs (M264): `codex`, or Claude Code when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// A review (M264): no finish step and no push.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub review: bool,
    /// The model its block was opened with (Arugula #215, [`model_for`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The model its agent says it's on now, and the last a report said:
    /// a report says it again when the two differ ([`Session::push`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_now: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_said: Option<String>,
    /// Parked (Arugula #215): its block closed, its user, checkout and
    /// conversation kept, until control lists a prompt for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parked: Option<Parked>,
    /// Its block was opened again after it was parked: the next turn's
    /// start names it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reopened: bool,
}

/// What a parked session keeps of its closed block (Arugula #215).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parked {
    /// When it was parked (ms).
    pub at: u64,
    /// The block it was in.
    pub block: Option<u32>,
    /// Its agent's ACP session, which a block opened again loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<String>,
}

impl Session {
    fn now(&self) -> State {
        State::parse(&self.state).unwrap_or(State::Queued)
    }

    /// Sign its next report and keep it, to be sent: a move to `to` (for
    /// [`Kind::State`]; the others stay where it is), naming the actions
    /// done since the last.
    pub fn push(&mut self, keys: &arugula_e2e::DeviceKeys, me: &str, kind: Kind, to: State, mut d: SessionDetail) {
        let from = self.now();
        let to = if kind == Kind::State { to } else { from };
        if kind == Kind::State && !from.daemon_may(to) {
            warn!(session = self.session, %from, %to, "hosted: a session move this box may not make; not reported");
            return;
        }
        d.actions.append(&mut self.unnamed);
        // The model it's on (Arugula #215): when it changed, and on each
        // turn's report.
        if d.model.is_none() && (self.model_now != self.model_said || kind == Kind::Turn) {
            d.model = self.model_now.clone();
        }
        if let Some(m) = &mut d.model {
            cut(m, MODEL_MAX);
            self.model_said = self.model_now.clone();
        }
        for t in [&mut d.question, &mut d.error, &mut d.reason, &mut d.diffstat].into_iter().flatten() {
            cut(t, TEXT_MAX);
        }
        if let Some(t) = &mut d.tool {
            cut(t, TOOL_MAX);
        }
        let detail = fit(&mut d);
        self.n += 1;
        let at = now_ms();
        let ev = Event { session: &self.session, n: self.n, kind, from, to, at, detail: &detail, daemon: me };
        let sig = ev.sign(keys);
        let report = SessionReport {
            session: self.session.clone(),
            n: self.n,
            kind: kind.as_str().into(),
            from: from.as_str().into(),
            to: to.as_str().into(),
            at,
            detail,
            sig,
        };
        self.events.push(Kept { report, answered: false, refused: None });
        self.state = to.as_str().into();
        self.last_at = at;
    }
}

/// `s` cut to `max` bytes, on a character's edge, and marked.
/// `d` as the report's detail, within [`SESSION_DETAIL_MAX`]: a turn's
/// reply (whose quotes and newlines grow in JSON) is cut until it fits,
/// from its start, and left out when even none of it would.
fn fit(d: &mut SessionDetail) -> String {
    loop {
        let detail = serde_json::to_string(&*d).unwrap_or_else(|_| "{}".into());
        let over = detail.len().saturating_sub(SESSION_DETAIL_MAX);
        let Some(r) = d.reply.as_mut().filter(|_| over > 0) else { return detail };
        let keep = r.len().saturating_sub(over + "…".len());
        if keep == 0 {
            d.reply = None;
        } else {
            cut_start(r, keep);
        }
    }
}

/// The agent's last message of a turn, as its report's reply: trimmed,
/// at most [`REPLY_MAX`] bytes, and none when it said nothing. A long one
/// keeps its end, where a summary's caveats are (Arugula #215).
fn reply_of(r: Option<&arugula_proto::api::TurnResult>) -> Option<String> {
    let mut m = r?.message.trim().to_owned();
    if m.is_empty() {
        return None;
    }
    cut_start(&mut m, REPLY_MAX);
    Some(m)
}

/// `s` cut to its last `max` bytes, on a character's edge, and marked at
/// its start.
fn cut_start(s: &mut String, max: usize) {
    if s.len() > max {
        let mut at = s.len() - max;
        while !s.is_char_boundary(at) {
            at += 1;
        }
        *s = format!("…{}", &s[at..]);
    }
}

fn cut(s: &mut String, max: usize) {
    if s.len() > max {
        let mut at = max;
        while !s.is_char_boundary(at) {
            at -= 1;
        }
        s.truncate(at);
        s.push('…');
    }
}

/// Whether `id` is a session's (`ses_` and 32 hex): control's word goes in
/// a file name only once it's that.
pub fn good_id(id: &str) -> bool {
    id.strip_prefix("ses_").is_some_and(|h| h.len() == 32 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// Whether `branch` is session `id`'s own (`arugula/<the first 8 hex of
/// its id>-<slug>`, as the platform names it), and a ref git takes as it is.
pub fn good_branch(id: &str, branch: &str) -> bool {
    let Some(short) = id.strip_prefix("ses_").and_then(|h| h.get(..8)) else { return false };
    let Some(rest) = branch.strip_prefix("arugula/").and_then(|r| r.strip_prefix(short)) else { return false };
    let slug = rest.strip_prefix('-').unwrap_or(rest);
    (rest.is_empty() || (rest.starts_with('-') && !slug.is_empty()))
        && slug.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        && !slug.ends_with('-')
        && branch.len() <= 200
}

fn path(dir: &Path, id: &str) -> Option<PathBuf> {
    good_id(id).then(|| dir.join(super::DIR).join(DIR).join(format!("{id}.json")))
}

/// Session `id`'s ledger: `Ok(None)` only when there's none.
pub fn read(dir: &Path, id: &str) -> std::io::Result<Option<Session>> {
    let Some(p) = path(dir, id) else { return Err(std::io::Error::other("not a session's id")) };
    match std::fs::read(p) {
        Ok(b) => serde_json::from_slice(&b).map(Some).map_err(std::io::Error::other),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Make session `l.session`'s ledger, once: `true` only for the call that
/// made it (fsynced).
pub fn claim(dir: &Path, l: &Session) -> bool {
    let Some(p) = path(dir, &l.session) else { return false };
    let _held = LEDGER.lock().unwrap_or_else(|e| e.into_inner());
    let made = (|| -> std::io::Result<()> {
        crate::store::private_dir(&dir.join(super::DIR))?;
        crate::store::private_dir(&dir.join(super::DIR).join(DIR))?;
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&p)?;
        f.write_all(&serde_json::to_vec_pretty(l).map_err(std::io::Error::other)?)?;
        f.sync_all()?;
        #[cfg(unix)]
        std::fs::File::open(dir.join(super::DIR).join(DIR))?.sync_all()?;
        Ok(())
    })();
    match made {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => {
            warn!(session = l.session, error = %e, "hosted: can't make a session's ledger; it isn't taken");
            false
        }
    }
}

/// Change session `id`'s ledger with `f`, written and fsynced, if it's
/// there: one writer at a time.
pub fn update<T>(dir: &Path, id: &str, f: impl FnOnce(&mut Session) -> T) -> Option<(Session, T)> {
    let p = path(dir, id)?;
    let _held = LEDGER.lock().unwrap_or_else(|e| e.into_inner());
    let mut l = match read(dir, id) {
        Ok(Some(l)) => l,
        Ok(None) => return None,
        Err(e) => {
            warn!(session = id, error = %e, "hosted: a session's ledger doesn't read");
            return None;
        }
    };
    let t = f(&mut l);
    // Heartbeats control has taken are of no more use.
    l.events.retain(|k| !(k.answered && k.report.kind == Kind::Heartbeat.as_str()));
    match serde_json::to_vec_pretty(&l)
        .map_err(anyhow::Error::from)
        .and_then(|b| crate::store::write_atomic(&p, &b).map_err(anyhow::Error::from))
    {
        Ok(()) => Some((l, t)),
        Err(e) => {
            warn!(session = id, error = %format!("{e:#}"), "hosted: can't write a session's ledger");
            None
        }
    }
}

/// Every session in the ledger that reads.
pub fn all(dir: &Path) -> Vec<Session> {
    let Ok(entries) = std::fs::read_dir(dir.join(super::DIR).join(DIR)) else { return vec![] };
    let mut out: Vec<Session> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".json")?.to_owned();
            read(dir, &id).ok().flatten()
        })
        .collect();
    out.sort_by_key(|l| l.taken_at);
    out
}

/// The sessions this box holds now (taken, not ended), parked ones too.
pub fn live(dir: &Path) -> Vec<Session> {
    all(dir).into_iter().filter(|l| l.ended.is_none()).collect()
}

/// How many of them run now: not parked, their agent block open or about
/// to be.
fn running(dir: &Path) -> usize {
    live(dir).iter().filter(|l| l.parked.is_none()).count()
}

/// A pull's sessions: kept as the latest word on each; the ones this box
/// runs that aren't listed any more are stopped; queued ones are taken
/// while there's a slot; and each one it runs is followed.
pub async fn pulled(app: &Arc<App>, sessions: Vec<SessionWork>) {
    let dir = app.control.state_dir().to_owned();
    let listed: BTreeMap<String, SessionWork> = sessions.into_iter().map(|s| (s.session.clone(), s)).collect();
    *LATEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(listed.clone());
    for l in live(&dir) {
        if !listed.contains_key(&l.session) {
            end(app, &l.session, "control says it ended").await;
        }
    }
    for (id, w) in &listed {
        if w.state == State::Queued.as_str() && matches!(read(&dir, id), Ok(None)) {
            take(app, w).await;
        }
    }
    for l in live(&dir) {
        follow(app, &l.session);
    }
    send_all(app).await;
}

/// After a restart: follow every session this box runs, and send what's
/// unsent.
pub async fn resume(app: &Arc<App>) {
    let dir = app.control.state_dir().to_owned();
    // Its blocks' turns are their sessions' from the start, before a
    // restored block sends what it had queued.
    for l in live(&dir) {
        if let Some(b) = l.block {
            block_is(b, &l.session);
        }
    }
    let real = slots().is_some_and(|s| s.real);
    for l in live(&dir) {
        // Parked as the daemon stopped: the block it was closing, should
        // the restart have brought it back, goes.
        if let Some(b) = l.parked.as_ref().and_then(|p| p.block)
            && app.mux.api(|r| Api::Block(b, r)).await.flatten().is_some()
        {
            app.mux.api(|r| Api::Close(b, r)).await;
        }
        info!(session = l.session, "hosted: following a session from before the restart");
        // A restart stops only the daemon (M258's `KillMode=process`): what
        // the agent started (a build, a server) would run on as its user,
        // unwatched. Killed: the block starts its agent again, and a turn
        // the restart cut off ends there.
        if real {
            super::kill_user(&l.user.user).await;
        }
        follow(app, &l.session);
    }
    send_all(app).await;
}

/// Who this machine is: its keys and device id.
fn me(app: &App) -> Option<(Arc<arugula_e2e::DeviceKeys>, String, String, String)> {
    super::me(app)
}

/// Whether this box runs agent `h` for sessions (M264): Claude Code
/// always; another when `ARUGULA_HOSTED_SESSION_HARNESSES` (a comma list)
/// names it, or, without that, Codex when its adapter and `codex` are
/// installed for the daemon's user.
pub fn has_harness(h: &str) -> bool {
    if h == "claude" {
        return true;
    }
    if let Ok(list) = std::env::var("ARUGULA_HOSTED_SESSION_HARNESSES") {
        return list.split(',').any(|x| x.trim() == h);
    }
    if h != "codex" {
        return false;
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return false };
    let Some(a) = crate::agent::adapters::of(crate::agent::defs::Kind::Codex) else { return false };
    let path = std::env::var_os("PATH").unwrap_or_default();
    crate::agent::adapters::status(a, &home, &[]).ok()
        && (home.join(".local/bin/codex").is_file()
            || std::env::split_paths(&path).filter(|d| !d.as_os_str().is_empty()).any(|d| d.join("codex").is_file()))
}

/// The agent session `w` runs on here: the one asked for when the box
/// has it, else Claude Code (`None`).
fn harness_for(w: &SessionWork) -> Option<String> {
    w.harness.clone().filter(|h| h != "claude" && has_harness(h))
}

/// The model session `w`'s block opens with, on `harness` (`None`: Claude
/// Code), Arugula #215: the one control asked for, else the strongest,
/// said outright ([`DEFAULT_MODEL`]), since the adapter's own `default`
/// depends on the account. Codex picks its own: Claude's words aren't its
/// models.
fn model_for(w: &SessionWork, harness: Option<&str>) -> Option<String> {
    if harness.is_some_and(|h| h != "claude") {
        return None;
    }
    let asked = w.model.as_deref().map(str::trim).filter(|m| MODELS.contains(m));
    Some(asked.unwrap_or(DEFAULT_MODEL).to_owned())
}

/// A free slot: a session user no session this box runs holds.
fn free_slot(dir: &Path) -> Option<(AsUser, bool)> {
    let slots = slots()?;
    let held: BTreeSet<String> = live(dir).into_iter().map(|l| l.user.user).collect();
    slots.users.iter().find(|u| !held.contains(&u.user)).cloned().map(|u| (u, slots.real))
}

/// Take queued session `w` if there's a slot: its ledger, its checkout,
/// its block, then its first turn. One that can't be set up fails, saying
/// why.
async fn take(app: &Arc<App>, w: &SessionWork) {
    let dir = app.control.state_dir().to_owned();
    let Some((keys, me, project, workspace)) = me(app) else { return };
    if !good_id(&w.session) {
        warn!(session = w.session, "hosted: control sent a session that isn't a session's id");
        return;
    }
    if w.project != project || w.workspace != workspace {
        warn!(session = w.session, "hosted: control sent another workspace's session");
        return;
    }
    if w.prompts.is_empty() {
        return;
    }
    let Some((user, real)) = free_slot(&dir) else {
        // Every slot is taken: it waits, listed, until one ends.
        return;
    };
    if running(&dir) >= MAX_SESSIONS_PER_BOX {
        // Three run: it waits, listed, until one ends or parks.
        return;
    }
    let dest = Path::new(&user.home).join("work").join(&w.session).display().to_string();
    let mut l = Session {
        session: w.session.clone(),
        project: w.project.clone(),
        workspace: w.workspace.clone(),
        asker: w.asker.clone(),
        repo: w.repo.clone(),
        branch: w.branch.clone(),
        user,
        checkout: dest,
        block: None,
        taken_at: now_ms(),
        n: w.n,
        state: State::Queued.as_str().into(),
        ended: None,
        handed: vec![],
        turn: None,
        notes: None,
        acted: vec![],
        unnamed: vec![],
        error: None,
        last_at: 0,
        events: vec![],
        pushed_head: None,
        turns_seen: Some(0),
        harness: harness_for(w),
        review: w.review,
        model: None,
        model_now: None,
        model_said: None,
        parked: None,
        reopened: false,
    };
    l.model = model_for(w, l.harness.as_deref());
    if let Some(h) = w.harness.as_deref().filter(|h| *h != "claude" && l.harness.is_none()) {
        info!(session = l.session, harness = h, "hosted: this box hasn't that agent; the session runs on Claude Code");
    }
    let fail = |mut l: Session, why: String| {
        info!(session = l.session, why, "hosted: a session can't start here");
        l.push(
            &keys,
            &me,
            Kind::State,
            State::Failed,
            SessionDetail { reason: Some(why.clone()), ..Default::default() },
        );
        l.ended = Some(why);
        if !claim(&dir, &l) {
            let _ = update(&dir, &l.session, |now| *now = l.clone());
        }
    };
    if !good_branch(&w.session, &w.branch) {
        return fail(l, format!("its branch isn't the session's own: {}", super::printable(&w.branch)));
    }
    if real
        && let Err(super::Refused(_, why)) =
            super::firewall(&dir, &super::fenced(super::run_as().ok().flatten().as_ref()))
    {
        return fail(l, why);
    }
    if !claim(&dir, &l) {
        return;
    }
    info!(session = l.session, user = l.user.user, "hosted: took a session");
    // A repo the box's setup didn't clone is one linked to the project:
    // cloned for the session (Fleet 6).
    let src = match l.repo.as_deref() {
        Some(repo) => clone_of(&dir, repo).map_or(Source::Linked, Source::Box),
        None => Source::Empty,
    };
    if real {
        super::kill_user(&l.user.user).await;
    }
    match checkout(app, &dir, &l, src, real).await {
        Ok(notes) => l.notes = notes,
        Err(why) => return fail(l, format!("its checkout: {why}")),
    }
    let block = match open_block(app, &l, None).await {
        Ok(b) => b,
        Err(why) => return fail(l, format!("its agent block: {why}")),
    };
    l.block = Some(block);
    block_is(block, &l.session);
    let notes = l.notes.clone();
    if update(&dir, &l.session, |now| {
        now.block = Some(block);
        now.notes = notes;
    })
    .is_none()
    {
        return;
    }
    follow(app, &l.session);
}

/// The workspace's clone of `repo` (`owner/name`), as its setup made it.
fn clone_of(dir: &Path, repo: &str) -> Option<PathBuf> {
    let want = repo.trim().trim_end_matches(".git").to_ascii_lowercase();
    crate::hosted::clones(dir)
        .into_iter()
        .find(|(url, _)| {
            let url = url.trim_end_matches('/').trim_end_matches(".git").to_ascii_lowercase();
            url.ends_with(&format!("/{want}")) || url.ends_with(&format!(":{want}"))
        })
        .map(|(_, dir)| dir)
        .filter(|d| d.join(".git").exists())
}

/// Where a session's checkout comes from ([`checkout`]).
enum Source {
    /// The workspace's clone of its repo, as its setup made it.
    Box(PathBuf),
    /// Its repo on GitHub, shallow: linked to the project, and no box's
    /// setup cloned it.
    Linked,
    /// Nothing: the session works in no repo.
    Empty,
}

/// Run `argv` as `u` ([`AsUser::wrap`]): its output, or what it said.
async fn as_user(u: &AsUser, argv: &[&str]) -> Result<String, String> {
    let argv: Vec<String> = argv.iter().map(|s| (*s).to_owned()).collect();
    let out = u
        .command(&[], &argv)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("can't run {} as {}: {e}", argv[0], u.user))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().chars().take(400).collect())
    }
}

/// Run `argv` as this daemon's user, as root through sudo when `root`.
async fn here(argv: &[&str], root: bool) -> Result<String, String> {
    let mut c = if root {
        let mut c = tokio::process::Command::new("sudo");
        c.arg("-n").args(argv);
        c
    } else {
        let mut c = tokio::process::Command::new(argv[0]);
        c.args(&argv[1..]);
        c
    };
    crate::agent::hosted::unprotect_oom(&mut c);
    let out = c
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("can't run {}: {e}", argv[0]))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "{}: {}",
            argv.join(" "),
            String::from_utf8_lossy(&out.stderr).trim().chars().take(400).collect::<String>()
        ))
    }
}

/// The session's checkout, `~sesN/work/ID`: a clone of `src` (the
/// workspace's clone; its repo on GitHub, shallow, for a linked repo the
/// box has none of; an empty repo with none) made by this daemon's user
/// in a staging directory, its origin the workspace clone's, handed to the
/// session user (`chown`, then moved into its home), and switched to the
/// session's branch as the session user. Never a `git worktree`: its
/// metadata would stay in the workspace's clone, which the session user
/// can't write. The repo's `CLAUDE.md` and `AGENTS.md`, read before it's
/// handed over, are the first prompt's notes.
///
/// Its branch starts from the repo's default branch as GitHub has it now
/// ([`fresh_base`], Arugula #263), not as the workspace's clone had it at
/// setup, which is a box's age behind: sessions cut from it built on code
/// from before the last waves merged. When that fetch fails, it starts
/// from the clone, and the first prompt says so ([`stale_note`]).
async fn checkout(app: &App, dir: &Path, l: &Session, src: Source, real: bool) -> Result<Option<String>, String> {
    let stage = dir.join(super::DIR).join(DIR).join(format!("{}.stage", l.session));
    let dest = PathBuf::from(&l.checkout);
    let s = stage.display().to_string();
    let _ = std::fs::remove_dir_all(&stage);
    let mut stale = None;
    match src {
        Source::Box(src) => {
            let from = src.display().to_string();
            here(&["git", "clone", "-q", "--no-hardlinks", "--", &from, &s], false).await?;
            // Its origin is the workspace clone's (no credentials in it).
            if let Ok(url) = here(&["git", "-C", &from, "remote", "get-url", "origin"], false).await {
                here(&["git", "-C", &s, "remote", "set-url", "origin", url.trim()], false).await?;
            }
            if let Some(repo) = l.repo.as_deref() {
                let fresh = tokio::time::timeout(FRESH_MAX, fresh_base(&s, repo))
                    .await
                    .unwrap_or_else(|_| Err(format!("it took over {} s", FRESH_MAX.as_secs())));
                match fresh {
                    Ok(branch) => {
                        info!(session = l.session, repo, branch, "hosted: a session starts from its repo's head")
                    }
                    Err(why) => {
                        warn!(session = l.session, repo, error = why, "hosted: a session starts from the box's clone");
                        stale = Some(stale_note(repo, &why));
                    }
                }
            }
        }
        // GitHub's head now: no fresh base to fetch.
        Source::Linked => tokio::time::timeout(CLONE_MAX, push::clone_linked(app, l, &stage))
            .await
            .unwrap_or_else(|_| Err(format!("it took over {} s", CLONE_MAX.as_secs())))
            .map_err(|why| {
                let repo = l.repo.as_deref().unwrap_or_default();
                format!("no box's setup cloned {repo}, and cloning it failed: {why}")
            })?,
        Source::Empty => here(&["git", "init", "-q", &s], false).await.map(|_| ())?,
    }
    let notes = match (notes(&stage), stale) {
        (Some(n), Some(s)) => Some(format!("{s}{n}")),
        (n, s) => n.or(s),
    };
    let work = Path::new(&l.user.home).join("work");
    let w = work.display().to_string();
    let d = dest.display().to_string();
    let owner = format!("{u}:{u}", u = l.user.user);
    if real {
        here(&["mkdir", "-p", "--", &w], true).await?;
        here(&["chown", &owner, "--", &w], true).await?;
        here(&["chmod", "0700", "--", &w], true).await?;
        here(&["chown", "-R", &owner, "--", &s], true).await?;
        here(&["rm", "-rf", "--", &d], true).await?;
        here(&["mv", "--", &s, &d], true).await?;
    } else {
        std::fs::create_dir_all(&work).map_err(|e| format!("{w}: {e}"))?;
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::rename(&stage, &dest).map_err(|e| format!("{d}: {e}"))?;
    }
    // From here on, its own user's.
    as_user(&l.user, &["git", "-C", &d, "config", "user.name", "Arugula session"]).await?;
    let email = format!("{}@sessions.arugula.invalid", l.session);
    as_user(&l.user, &["git", "-C", &d, "config", "user.email", &email]).await?;
    as_user(&l.user, &["git", "-C", &d, "switch", "-q", "-c", &l.branch]).await?;
    Ok(notes)
}

/// Move the staging clone `stage` to `repo`'s default branch as it is on
/// GitHub now ([`push::remote_for`]: where its pushes go): fetched there
/// by this daemon's user, before the clone is handed over (nothing in it
/// is the agent's yet), into `origin/<default>`, which `origin/HEAD`
/// names (a push counts the session's commits from it), and checked out
/// as the local default branch the session's branch is cut from. With no
/// credentials: a private repo's fetch fails, and the clone stays as the
/// box's setup made it. The default branch's name.
async fn fresh_base(stage: &str, repo: &str) -> Result<String, String> {
    let remote =
        push::remote_for(repo).ok_or_else(|| format!("{} isn't a GitHub repo's name", super::printable(repo)))?;
    let heads = here(&["git", "ls-remote", "--symref", &remote, "HEAD"], false).await?;
    let branch = default_branch_of(&heads).ok_or("GitHub named no default branch")?;
    let tracking = format!("refs/remotes/origin/{branch}");
    let spec = format!("+refs/heads/{branch}:{tracking}");
    here(&["git", "-C", stage, "fetch", "-q", "--no-tags", &remote, &spec], false).await?;
    here(&["git", "-C", stage, "symbolic-ref", "refs/remotes/origin/HEAD", &tracking], false).await?;
    here(&["git", "-C", stage, "checkout", "-q", "-B", &branch, &tracking], false).await?;
    Ok(branch)
}

/// The default branch `git ls-remote --symref URL HEAD` names (`ref:
/// refs/heads/main\tHEAD`), if it's a plain branch name.
pub fn default_branch_of(listing: &str) -> Option<String> {
    let line = listing.lines().find_map(|l| l.strip_prefix("ref: "))?;
    let (target, name) = line.split_once('\t')?;
    let branch = target.strip_prefix("refs/heads/")?;
    let plain = !branch.is_empty()
        && !branch.starts_with(['-', '.', '/'])
        && !branch.contains("..")
        && branch.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b));
    (name.trim() == "HEAD" && plain).then(|| branch.to_owned())
}

/// What the first prompt says when its checkout couldn't start from the
/// repo's head ([`fresh_base`]).
pub fn stale_note(repo: &str, why: &str) -> String {
    let mut why = why.to_owned();
    cut(&mut why, 300);
    format!(
        "\n\n---\nThis checkout is the box's clone of {repo} as its setup made it: fetching the default branch's \
         head from GitHub failed ({why}), so your branch may start behind it. Say so in your answer if the base \
         matters to the work."
    )
}

/// What a prompt says when the disk the session's checkout is on is low:
/// `free` and `total` bytes. A Rust target is 8 to 40 GB, and the box's
/// sessions share its disk (Arugula #263).
pub fn disk_note(free: u64, total: u64) -> Option<String> {
    const GIB: u64 = 1 << 30;
    (free < DISK_LOW).then(|| {
        format!(
            "\n\n---\nThe box's disk is low: {:.1} GiB free of {:.0} GiB, shared with its other sessions. Remove what \
             you don't need of your own (a Rust target's `incremental` directories first) before a big build, and \
             say in your answer if a build ran out of space.",
            free as f64 / GIB as f64,
            total as f64 / GIB as f64
        )
    })
}

/// [`disk_note`] for the disk `path` is on, on a box with the template's
/// session users (a test's host disk is nobody's business).
#[cfg(unix)]
fn disk_now(path: &str) -> Option<String> {
    if !slots().is_some_and(|s| s.real) {
        return None;
    }
    let s = nix::sys::statvfs::statvfs(path).ok()?;
    let frag = s.fragment_size() as u64;
    disk_note(s.blocks_available() as u64 * frag, s.blocks() as u64 * frag)
}

#[cfg(not(unix))]
fn disk_now(_: &str) -> Option<String> {
    None
}

/// The repo's `CLAUDE.md` and `AGENTS.md` (at its top), as the first
/// prompt's text.
fn notes(repo: &Path) -> Option<String> {
    let mut out = String::new();
    for name in ["CLAUDE.md", "AGENTS.md"] {
        let Ok(mut text) = std::fs::read_to_string(repo.join(name)) else { continue };
        if text.trim().is_empty() {
            continue;
        }
        cut(&mut text, NOTES_MAX);
        out.push_str(&format!("\n\nThe repository's {name}, as text:\n\n{}", text.trim_end()));
    }
    (!out.is_empty()).then(|| {
        format!(
            "\n\n---\nWhat the repository says about working in it (only as text: its settings and hooks aren't \
             loaded).{out}"
        )
    })
}

/// The agent a session's block runs: Claude Code in `bypassPermissions` on
/// its model, or Codex, when it runs on it (M264), in `full-access`, unless a test
/// names a command (`ARUGULA_HOSTED_SESSION_AGENT`), which still runs as
/// that kind, under M185's gate.
fn agent_config(l: &Session) -> Value {
    let agent = l.harness.as_deref().unwrap_or("claude");
    let mut c = json!({
        "agent": agent,
        "cwd": l.checkout,
        "title": format!("session {}", l.session),
        "run_as": l.user,
        // Nothing asks before it runs (aru-115: the box, as its own user
        // behind the firewall, is the blast radius); a question still
        // does (an elicitation), and goes to the card. What still asks in
        // this mode the block allows itself (`agent`'s permissions). Each
        // agent has its own word for the mode: codex-acp's is
        // `full-access`, which a review runs in too, since what keeps a
        // review read-only is that its box pushes nothing for it.
        "permission_mode": mode_of(agent),
    });
    // Its model (Arugula #215), which the block switches to as the session
    // opens, before the mode.
    if let Some(m) = &l.model {
        c["model"] = json!(m);
    }
    if let Some(cmd) = std::env::var("ARUGULA_HOSTED_SESSION_AGENT").ok().filter(|c| !c.trim().is_empty()) {
        c["command"] = json!(crate::agent::defs::split_command(&cmd));
    }
    c
}

/// The permission mode in which agent `agent` asks nothing (aru-115).
fn mode_of(agent: &str) -> &'static str {
    match agent {
        "codex" => "full-access",
        _ => "bypassPermissions",
    }
}

/// Open session `l`'s agent block: a new conversation, or with
/// `agent_session`, the one it had (a parked session's), loaded.
async fn open_block(app: &App, l: &Session, agent_session: Option<&str>) -> Result<u32, String> {
    let mut config = agent_config(l);
    if let Some(sid) = agent_session {
        config["session_id"] = json!(sid);
    }
    let req =
        OpenRequest { kind: BlockType::Agent, config, session: Some(BLOCKS.into()), local: true, ..Default::default() };
    match app.mux.api(|r| Api::Open(req, None, r)).await {
        Some(Ok(b)) => Ok(b),
        Some(Err(e)) => Err(e),
        None => Err("the daemon is stopping".into()),
    }
}

/// Follow session `id` (once: a second call while it's followed does
/// nothing) until it ends.
fn follow(app: &Arc<App>, id: &str) {
    if !FOLLOWING.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_owned()) {
        return;
    }
    let app = app.clone();
    let id = id.to_owned();
    crate::task::spawn(async move {
        let mut missing: Option<std::time::Instant> = None;
        while step(&app, &id, &mut missing).await {
            tokio::time::sleep(FOLLOW).await;
        }
        BUSY.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        FOLLOWING.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        send(&app, &id).await;
    });
}

/// The latest pull's word on session `id`: `None` when it wasn't listed,
/// or before the first pull.
fn latest(id: &str) -> Option<SessionWork> {
    LATEST.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(id).cloned())
}

/// What the block's state says of its turn.
struct Seen {
    status: String,
    turns: u64,
    queued: usize,
    /// An open permission request or question: kind, id, what it asks,
    /// and a permission's options.
    open: Option<(&'static str, String, String, Vec<String>)>,
    tool: Option<String>,
    error: Option<String>,
    /// The model its agent is on, in words (Arugula #215).
    model: Option<String>,
    /// Whose each turn's prompt was, oldest first, of the last few
    /// (`None`: the block's owner's, or the agent's own).
    by: Vec<Option<String>>,
    /// Each of those turns' prompt, as far as the block keeps it (`None`:
    /// the agent woke itself).
    prompt: Vec<Option<String>>,
    /// The first prompt waiting in the block's queue.
    next: Option<String>,
}

impl Seen {
    /// Whose the block's turn number `n` (from 0) was, if it says.
    fn by_at(&self, n: u64) -> Option<String> {
        let from = self.turns.saturating_sub(self.by.len() as u64);
        n.checked_sub(from).and_then(|i| self.by.get(i as usize).cloned().flatten())
    }

    /// What was typed for the block's turn number `n` (from 0), or for the
    /// next when it hasn't started (its prompt is still queued): at most
    /// [`TYPED_MAX`] characters, and none for a turn the agent woke for.
    fn typed_at(&self, n: u64) -> Option<String> {
        let from = self.turns.saturating_sub(self.prompt.len() as u64);
        let p = match n.checked_sub(from) {
            Some(i) if n < self.turns => self.prompt.get(i as usize).cloned().flatten(),
            _ if n == self.turns => self.next.clone(),
            _ => None,
        }?;
        let p = p.trim();
        (!p.is_empty()).then(|| p.chars().take(TYPED_MAX).collect())
    }
}

fn seen(s: &Value) -> Seen {
    let perm = s["pending"].as_array().and_then(|p| p.first()).map(|p| {
        let options = p["options"].as_array().into_iter().flatten().filter_map(|o| o["id"].as_str()).map(str::to_owned);
        (
            "permission",
            p["id"].as_str().unwrap_or_default().to_owned(),
            p["title"].as_str().unwrap_or("a permission").to_owned(),
            options.collect(),
        )
    });
    let ask = || {
        s["asks"].as_array().into_iter().flatten().find(|a| a["accepted"] != true).map(|a| {
            let q = a["questions"][0]["question"].as_str().or(a["message"].as_str()).unwrap_or("a question");
            ("question", a["id"].as_str().unwrap_or_default().to_owned(), q.to_owned(), choices(a))
        })
    };
    Seen {
        status: s["status"].as_str().unwrap_or_default().to_owned(),
        turns: s["turns"].as_u64().unwrap_or(0),
        queued: s["queued"].as_array().map_or(0, Vec::len),
        open: perm.or_else(ask),
        tool: s["current_tool"]["title"].as_str().map(str::to_owned),
        error: s["error"].as_str().map(str::to_owned),
        model: s["model_name"].as_str().map(str::to_owned),
        // `recent_turns` is newest first.
        by: s["recent_turns"]
            .as_array()
            .map(|t| t.iter().rev().map(|t| t["by"].as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
        prompt: s["recent_turns"]
            .as_array()
            .map(|t| {
                let typed = |t: &Value| t["prompt"].as_str().filter(|p| *p != crate::agent::WOKEN).map(str::to_owned);
                t.iter().rev().map(typed).collect()
            })
            .unwrap_or_default(),
        next: s["queued"][0].as_str().map(str::to_owned),
    }
}

/// A question's choices, by their labels, for the card's buttons (Arugula
/// #214): only for one question that takes one answer, which a label
/// answers whole (`question_0`). Anything else is answered in words.
fn choices(ask: &Value) -> Vec<String> {
    let qs = ask["questions"].as_array().map_or(&[][..], Vec::as_slice);
    let [q] = qs else { return vec![] };
    if q["multiSelect"] == true {
        return vec![];
    }
    let labels = q["options"].as_array().into_iter().flatten().filter_map(|o| o["label"].as_str());
    labels.filter(|l| !l.trim().is_empty()).take(12).map(|l| l.chars().take(200).collect()).collect()
}

/// One look at session `id`'s block, and what it calls for. `false` once
/// it ended.
async fn step(app: &Arc<App>, id: &str, missing: &mut Option<std::time::Instant>) -> bool {
    let dir = app.control.state_dir().to_owned();
    let Ok(Some(l)) = read(&dir, id) else { return false };
    if l.ended.is_some() {
        return false;
    }
    let Some((keys, me, ..)) = me(app) else { return true };
    if l.parked.is_some() {
        unpark(app, &l, &keys, &me).await;
        send(app, id).await;
        return true;
    }
    let Some(block) = l.block else { return true };
    let Some(b) = app.mux.api(|r| Api::Block(block, r)).await.flatten() else {
        // A restart restores it: give it a while.
        let since = *missing.get_or_insert_with(std::time::Instant::now);
        if since.elapsed() > BLOCK_GRACE {
            fail(app, id, BLOCK_GONE).await;
            return false;
        }
        return true;
    };
    *missing = None;
    let work = latest(id);
    // People's answers, each once, as whoever asked.
    if let Some(w) = &work {
        for a in w.actions.iter().filter(|a| !l.acted.contains(&a.id)) {
            act(&b, a).await;
            let _ = update(&dir, id, |now| {
                now.acted.push(a.id.clone());
                now.unnamed.push(a.id.clone());
                if a.kind == "cancel"
                    && let Some(t) = now.turn.as_mut()
                {
                    t.cancelled = true;
                }
            });
        }
    }
    let s = seen(&b.state());
    if s.model.is_some() && s.model != l.model_now {
        let m = s.model.clone();
        update(&dir, id, |now| now.model_now = m);
    }
    let Ok(Some(l)) = read(&dir, id) else { return false };
    let say = |f: Box<dyn FnOnce(&mut Session) + Send>| {
        update(&dir, id, f);
    };
    match l.turn.clone() {
        // A restart came between the ledger and the block: the block's own
        // log says whether it got the prompt.
        Some(t) if !t.sent => {
            if s.turns > t.base || s.queued > 0 {
                let (k, m) = (keys.clone(), me.clone());
                say(Box::new(move |now| started(now, &k, &m)));
            } else if let Some(text) = t.fix.clone() {
                give_fix(app, &b, id, text, &keys, &me).await;
            } else if let Some(p) = work.as_ref().and_then(|w| w.prompts.iter().find(|p| t.prompts.contains(&p.id))) {
                give(app, &b, id, p.clone(), &keys, &me).await;
            } else {
                // Control no longer lists it: it went with a turn before.
                say(Box::new(|now| now.turn = None));
            }
        }
        Some(t) => {
            BUSY.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_owned());
            let idle = s.status != "working" && s.status != "starting" && s.queued == 0 && s.open.is_none();
            if let Some(e) = s.error.clone().filter(|e| l.error.as_ref() != Some(e)) {
                let (k, m) = (keys.clone(), me.clone());
                say(Box::new(move |now| {
                    now.push(
                        &k,
                        &m,
                        Kind::Error,
                        State::Working,
                        SessionDetail { error: Some(e.clone()), ..Default::default() },
                    );
                    now.error = Some(e);
                }));
            }
            if idle && (s.turns > t.base || s.error.is_some() || s.status == "exited" || s.status == "stopped") {
                // The turn ended (or never began: the gate said no, or the
                // agent went away).
                if l.now() == State::NeedsInput {
                    let (k, m) = (keys.clone(), me.clone());
                    say(Box::new(move |now| now.push(&k, &m, Kind::State, State::Working, SessionDetail::default())));
                }
                // The finish step, before the push: a stop meanwhile ends
                // it here. A review runs none (M264).
                let step = work.as_ref().and_then(|w| w.finish.as_ref()).filter(|_| !l.review);
                let finished = finish_turn(app, &l, step, &keys, &me).await;
                let Ok(Some(l)) = read(&dir, id) else { return false };
                if l.ended.is_some() {
                    return false;
                }
                // One fix, after a turn that ran and wasn't cancelled.
                let went = s.turns > t.base && s.error.is_none() && s.status != "exited" && s.status != "stopped";
                let fix = match &finished {
                    Finished::Failed(r) if t.fix.is_none() && !t.cancelled && went => Some(fix_prompt(r)),
                    _ => None,
                };
                let result = b.call("turn_result", json!({})).await.ok().filter(|v| !v.is_null());
                let r: Option<arugula_proto::api::TurnResult> = result.and_then(|v| serde_json::from_value(v).ok());
                // The push, only when the finish step passed (or there's
                // none) and no fix is owed: the commit now on GitHub, nothing
                // to push, or why it didn't.
                // A review pushes nothing (M264): its reply is what it made.
                // Every turn's reply goes in its report, for its thread.
                let may_push = fix.is_none() && finished.may_push() && !l.review;
                info!(session = id, may_push, fix = fix.is_some(), "hosted: a session's turn ended");
                let pushed = if may_push { push::push_turn(app, &l).await } else { Ok(None) };
                // Stopped while it pushed: what the push left goes too.
                if may_push && push::clean_if_ended(&dir, &l).await {
                    return false;
                }
                let reply = reply_of(r.as_ref());
                let branch = l.branch.clone();
                let (k, m, base, again) = (keys.clone(), me.clone(), s.turns, fix.clone());
                say(Box::new(move |now| {
                    now.turns_seen = Some(base);
                    let pushed = match pushed {
                        Ok(Some(head)) => {
                            now.pushed_head = Some(head);
                            true
                        }
                        Ok(None) => false,
                        Err(why) => {
                            let error = Some(format!("couldn't push {}: {why}", now.branch));
                            now.push(
                                &k,
                                &m,
                                Kind::Error,
                                State::Working,
                                SessionDetail { error, ..Default::default() },
                            );
                            false
                        }
                    };
                    let w = r.and_then(|r| r.work);
                    let d = SessionDetail {
                        branch: Some(w.as_ref().map_or(branch, |w| w.branch.clone())),
                        head: w.as_ref().map(|w| w.head.clone()).filter(|h| !h.is_empty()),
                        pushed: Some(pushed),
                        commits: w.as_ref().and_then(|w| w.commits.clone()).unwrap_or_default(),
                        diffstat: w.as_ref().and_then(|w| w.diffstat.clone()),
                        uncommitted: w.as_ref().map(|w| w.uncommitted_total),
                        finish: finished.result(),
                        reply,
                        ..Default::default()
                    };
                    now.push(&k, &m, Kind::Turn, State::Working, d);
                    match again {
                        // The fix is the next turn, in the same `working`.
                        Some(text) => {
                            now.turn = Some(Turn {
                                prompts: vec![],
                                base,
                                sent: false,
                                at: now_ms(),
                                fix: Some(text),
                                cancelled: false,
                            })
                        }
                        None => {
                            now.push(&k, &m, Kind::State, State::Done, SessionDetail::default());
                            now.turn = None;
                        }
                    }
                }));
                match fix {
                    Some(text) => give_fix(app, &b, id, text, &keys, &me).await,
                    None => {
                        BUSY.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
                    }
                }
            } else if let Some((ask, request, question, options)) = s.open.clone().filter(|_| l.now() == State::Working)
            {
                let (k, m) = (keys.clone(), me.clone());
                say(Box::new(move |now| {
                    let d = SessionDetail {
                        question: Some(question),
                        request: Some(request),
                        ask: Some(ask.into()),
                        options,
                        ..Default::default()
                    };
                    now.push(&k, &m, Kind::State, State::NeedsInput, d);
                }));
            } else if s.open.is_none() && l.now() == State::NeedsInput {
                let (k, m) = (keys.clone(), me.clone());
                say(Box::new(move |now| now.push(&k, &m, Kind::State, State::Working, SessionDetail::default())));
            } else if now_ms().saturating_sub(l.last_at) >= heartbeat_every().as_millis() as u64
                || !l.unnamed.is_empty()
                || l.model_now != l.model_said
            {
                let (k, m, tool) = (keys.clone(), me.clone(), s.tool.clone());
                say(Box::new(move |now| {
                    let to = now.now();
                    now.push(&k, &m, Kind::Heartbeat, to, SessionDetail { tool, ..Default::default() })
                }));
            }
        }
        None => {
            BUSY.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
            let next = work.as_ref().and_then(|w| w.prompts.iter().find(|p| !l.handed.contains(&p.id)).cloned());
            let Some(seen_turns) = l.turns_seen else {
                // A ledger from before it was kept: from now on.
                let turns = s.turns;
                say(Box::new(move |now| now.turns_seen = Some(turns)));
                return true;
            };
            if l.now() == State::Done && (s.turns > seen_turns || s.queued > 0) {
                // A turn started in the block itself (someone typed there,
                // or the agent woke itself): followed like the box's own,
                // to the same end (finish step, push, turn report). The
                // thread's prompts wait for it.
                let (by, typed) = (s.by_at(seen_turns), s.typed_at(seen_turns));
                let (k, m) = (keys.clone(), me.clone());
                info!(session = id, "hosted: a turn started in the session's block");
                say(Box::new(move |now| {
                    now.turn = Some(Turn {
                        prompts: vec![],
                        base: seen_turns,
                        sent: true,
                        at: now_ms(),
                        fix: None,
                        cancelled: false,
                    });
                    let d = SessionDetail { in_block: true, by, typed, ..Default::default() };
                    now.push(&k, &m, Kind::State, State::Working, d);
                }));
            } else if let Some(p) = next {
                hand(app, &b, id, p, &keys, &me, s.turns).await;
            } else if l.now() == State::Done
                && work.as_ref().is_some_and(|w| w.park)
                && l.unnamed.is_empty()
                && s.queued == 0
                && s.open.is_none()
                && !matches!(s.status.as_str(), "working" | "starting")
            {
                park(app, &l, block, &b).await;
                return true;
            } else if !l.unnamed.is_empty() {
                // An action with no turn to do it to (a cancel too late):
                // named, so it leaves control's list.
                let (k, m) = (keys.clone(), me.clone());
                say(Box::new(move |now| {
                    let to = now.now();
                    now.push(&k, &m, Kind::Heartbeat, to, SessionDetail::default())
                }));
            }
        }
    }
    send(app, id).await;
    true
}

/// Park done session `l` (Arugula #215): its transcript archived first,
/// as `hosted.archive` sends it, then its block closed and what its user
/// left killed, which frees the memory and a running slot. Its user,
/// checkout and `~/.claude` stay, and the ledger keeps its agent's
/// session id for [`unpark`].
pub async fn park(app: &Arc<App>, l: &Session, block: u32, b: &Arc<dyn crate::block::Block>) {
    let dir = app.control.state_dir().to_owned();
    let agent_session = b.state()["session_id"].as_str().map(str::to_owned);
    let archived = crate::hosted::archive::flush(Some(&[u64::from(block)])).await;
    info!(session = l.session, block, ?archived, "hosted: parking an idle session: its transcript archived");
    let parked = Parked { at: now_ms(), block: Some(block), agent_session };
    if update(&dir, &l.session, |now| {
        now.parked = Some(parked);
        now.block = None;
    })
    .is_none()
    {
        return;
    }
    block_gone(block);
    app.mux.api(|r| Api::Close(block, r)).await;
    if slots().is_some_and(|s| s.real) {
        super::kill_user(&l.user.user).await;
    }
    info!(session = l.session, user = l.user.user, "hosted: parked a session: its checkout and conversation kept");
}

/// A parked session's look: once control lists a prompt it hasn't handed
/// (it does only while there's a running slot), and the box has one, open
/// its block again on the same user and checkout, loading its agent's
/// conversation; the next look hands the prompt. Actions with no agent to
/// do them to (a late cancel) are named, so they leave control's list.
async fn unpark(app: &Arc<App>, l: &Session, keys: &Arc<arugula_e2e::DeviceKeys>, me: &str) {
    let dir = app.control.state_dir().to_owned();
    let Some(w) = latest(&l.session) else { return };
    let late: Vec<String> = w.actions.iter().filter(|a| !l.acted.contains(&a.id)).map(|a| a.id.clone()).collect();
    if !late.is_empty() {
        update(&dir, &l.session, |now| {
            now.acted.extend(late.iter().cloned());
            now.unnamed.extend(late);
            let to = now.now();
            now.push(keys, me, Kind::Heartbeat, to, SessionDetail::default());
        });
        return;
    }
    if !w.prompts.iter().any(|p| !l.handed.contains(&p.id)) || running(&dir) >= MAX_SESSIONS_PER_BOX {
        return;
    }
    let sid = l.parked.as_ref().and_then(|p| p.agent_session.clone());
    let block = match open_block(app, l, sid.as_deref()).await {
        Ok(b) => b,
        Err(why) => return fail(app, &l.session, &format!("its agent block, opened again: {why}")).await,
    };
    block_is(block, &l.session);
    let opened = update(&dir, &l.session, |now| {
        now.block = Some(block);
        now.parked = None;
        now.reopened = true;
        // A new block: its turns count from none.
        now.turns_seen = Some(0);
    });
    if opened.is_none() {
        app.mux.api(|r| Api::Close(block, r)).await;
        return;
    }
    info!(session = l.session, block, resumed = sid.is_some(), "hosted: opened a parked session's block again");
    follow(app, &l.session);
}

/// Start a turn with prompt `p`: the ledger says it's handed (fsynced)
/// before the block gets it, so a restart can't hand it twice.
async fn hand(
    app: &App,
    b: &Arc<dyn crate::block::Block>,
    id: &str,
    p: SessionPrompt,
    keys: &Arc<arugula_e2e::DeviceKeys>,
    me: &str,
    base: u64,
) {
    let dir = app.control.state_dir();
    let pid = p.id.clone();
    let Some(_) = update(dir, id, |now| {
        now.handed.push(pid.clone());
        now.turn = Some(Turn { prompts: vec![pid], base, sent: false, at: now_ms(), fix: None, cancelled: false });
    }) else {
        return;
    };
    give(app, b, id, p, keys, me).await;
}

/// Give the block prompt `p` (the turn's, handed), as its author's, and say
/// the turn started. The first prompt carries the repo's notes; any says
/// when the disk is low ([`disk_note`]).
async fn give(
    app: &App,
    b: &Arc<dyn crate::block::Block>,
    id: &str,
    p: SessionPrompt,
    keys: &Arc<arugula_e2e::DeviceKeys>,
    me: &str,
) {
    let dir = app.control.state_dir();
    let Ok(Some(l)) = read(dir, id) else { return };
    let mut text = p.for_agent();
    if let Some(n) = &l.notes {
        text.push_str(n);
    }
    // Arugula #263: what the turn builds with. A resume since the last turn
    // took resolv.conf's newline again; the disk is shared with the box's
    // other sessions.
    crate::hosted::fix_resolv().await;
    if let Some(n) = disk_now(&l.user.home) {
        text.push_str(&n);
    }
    match b.call_by("send", json!({ "text": text }), Some(p.who())).await {
        Ok(_) => {
            update(dir, id, |now| started(now, keys, me));
        }
        Err(e) => {
            warn!(session = id, error = e, "hosted: a session's agent didn't take its prompt");
            let why = format!("couldn't give the agent its prompt: {e}");
            update(dir, id, |now| {
                // Not handed after all: it goes again at the next look.
                now.handed.retain(|h| *h != p.id);
                now.turn = None;
                if now.error.as_deref() != Some(why.as_str()) {
                    now.push(
                        keys,
                        me,
                        Kind::Error,
                        State::Working,
                        SessionDetail { error: Some(why.clone()), ..Default::default() },
                    );
                    now.error = Some(why);
                }
            });
        }
    }
}

/// The block has the turn's prompt: say the turn started (with the agent's
/// block, on the first), once.
fn started(now: &mut Session, keys: &arugula_e2e::DeviceKeys, me: &str) {
    let Some(t) = now.turn.as_mut() else { return };
    t.sent = true;
    let prompts = t.prompts.clone();
    now.notes = None;
    now.error = None;
    if matches!(now.now(), State::Queued | State::Done) {
        let first = now.now() == State::Queued;
        // A parked one's block, opened again, is named too.
        let named = first || std::mem::take(&mut now.reopened);
        let d = SessionDetail {
            prompts,
            block: named.then(|| now.block.map(|b| b.to_string())).flatten(),
            // The agent it runs on (M264), said once.
            harness: first.then(|| now.harness.clone().unwrap_or_else(|| "claude".into())),
            ..Default::default()
        };
        now.push(keys, me, Kind::State, State::Working, d);
    }
}

/// What a turn's finish step did (M261).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finished {
    /// The workspace has no finish step.
    None,
    Passed(FinishResult),
    Failed(FinishResult),
}

impl Finished {
    /// Whether the turn may be pushed: no finish step, or it passed.
    pub fn may_push(&self) -> bool {
        !matches!(self, Finished::Failed(_))
    }

    /// What the turn's report says of it.
    pub fn result(&self) -> Option<FinishResult> {
        match self {
            Finished::None => None,
            Finished::Passed(r) | Finished::Failed(r) => Some(r.clone()),
        }
    }
}

/// Run the workspace's finish step (`step`, from the latest pull) for
/// session `l`'s turn that just ended, before the push: as the session's
/// user, in its checkout, with that user's environment only and nothing
/// secret on its command line, for at most its timeout (15 minutes unless
/// the spec says), the last 4 KiB of its output kept. While it runs the
/// session says it's alive, with the finish step as its tool.
pub async fn finish_turn(
    app: &Arc<App>,
    l: &Session,
    step: Option<&FinishStep>,
    keys: &Arc<arugula_e2e::DeviceKeys>,
    me: &str,
) -> Finished {
    let Some(step) = step.filter(|s| !s.command.trim().is_empty()) else { return Finished::None };
    let dir = app.control.state_dir().to_owned();
    info!(session = l.session, "hosted: running the finish step");
    let run = run_finish(&l.user, &l.checkout, step);
    tokio::pin!(run);
    let mut beat = tokio::time::interval(heartbeat_every());
    beat.tick().await;
    let (ok, output_tail) = loop {
        tokio::select! {
            r = &mut run => break r,
            _ = beat.tick() => {
                let tool = Some(format!("finish: {}", step.command));
                update(&dir, &l.session, |now| {
                    if now.ended.is_none() {
                        let to = now.now();
                        now.push(keys, me, Kind::Heartbeat, to, SessionDetail { tool, ..Default::default() });
                    }
                });
                send(app, &l.session).await;
            }
        }
    };
    info!(session = l.session, ok, "hosted: the finish step ended");
    let r =
        FinishResult { ok, command: step.command.clone(), output_tail: if ok { String::new() } else { output_tail } };
    if ok { Finished::Passed(r) } else { Finished::Failed(r) }
}

/// `step` as user `u` in `checkout`: whether it exited 0 in time, and the
/// last of what it printed. `timeout` (coreutils, on every hosted box)
/// kills it and what it started when its time is up; where there's none
/// (macOS), the daemon's own limit a little after is the only one.
async fn run_finish(u: &AsUser, checkout: &str, step: &FinishStep) -> (bool, String) {
    use tokio::io::AsyncReadExt as _;
    let secs = if step.timeout_s == 0 { FINISH_TIMEOUT_S } else { step.timeout_s };
    // The checkout and the command are arguments, never in the script.
    let script = "cd -- \"$1\" || exit 125\nexec 2>&1\nexec sh -c \"$2\"";
    let mut argv: Vec<String> = match ["/usr/bin/timeout", "/bin/timeout"].into_iter().find(|t| Path::new(t).exists()) {
        Some(t) => vec![t.to_owned(), "-k".into(), "10".into(), secs.to_string()],
        None => vec![],
    };
    argv.extend(["sh", "-c", script, "sh", checkout, &step.command].into_iter().map(str::to_owned));
    let run = async {
        let mut child = u
            .command(&[], &argv)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("can't run the finish step as {}: {e}", u.user))?;
        let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
        let mut tail = Vec::new();
        let mut buf = vec![0u8; 16 * 1024];
        while let Some(o) = out.as_mut() {
            match o.read(&mut buf).await {
                Ok(0) | Err(_) => out = None,
                Ok(n) => {
                    tail.extend_from_slice(&buf[..n]);
                    if tail.len() > 2 * FINISH_OUTPUT_MAX {
                        tail.drain(..tail.len() - FINISH_OUTPUT_MAX);
                    }
                }
            }
        }
        // What sudo or `timeout` said, if anything: after the command's.
        let mut said = Vec::new();
        if let Some(e) = err.as_mut() {
            let _ = e.take(FINISH_OUTPUT_MAX as u64).read_to_end(&mut said).await;
        }
        tail.extend_from_slice(&said);
        let status = child.wait().await.map_err(|e| format!("the finish step: {e}"))?;
        Ok::<_, String>((status.code(), tail))
    };
    let (code, tail) = match tokio::time::timeout(Duration::from_secs(secs + 30), run).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return (false, e),
        Err(_) => (Some(124), vec![]),
    };
    let mut text = last(&String::from_utf8_lossy(&tail), FINISH_OUTPUT_MAX).trim_end().to_owned();
    match code {
        Some(0) => {}
        Some(124) | Some(137) => text.push_str(&format!("\n(the finish step ran out of time after {secs} s)")),
        Some(125) => text.push_str("\n(the session's checkout isn't there)"),
        Some(c) => text.push_str(&format!("\n(exit {c})")),
        None => text.push_str("\n(killed)"),
    }
    (code == Some(0), last(text.trim_start(), FINISH_OUTPUT_MAX))
}

/// The last `max` bytes of `s`, on a character's edge.
fn last(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut at = s.len() - max;
    while !s.is_char_boundary(at) {
        at += 1;
    }
    s[at..].to_owned()
}

/// The prompt that gives the agent its one try at a failed finish step:
/// what it ran and the last of its output, fenced so the output can't
/// close it.
pub fn fix_prompt(r: &FinishResult) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in r.output_tail.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat((longest + 1).max(3));
    format!(
        "The finish step failed after your turn. It ran `{}` in your checkout, and the last of its output \
         was:\n{fence}text\n{}\n{fence}\nFix what it reports, and commit. When this turn ends the finish step \
         runs once more; the branch is pushed only if it passes.",
        r.command,
        r.output_tail.trim_end()
    )
}

/// Give the block the finish step's fix prompt, as from the finish step:
/// the fix turn under way. One the agent doesn't take ends the turn,
/// unpushed.
async fn give_fix(
    app: &App,
    b: &Arc<dyn crate::block::Block>,
    id: &str,
    text: String,
    keys: &Arc<arugula_e2e::DeviceKeys>,
    me: &str,
) {
    let dir = app.control.state_dir();
    match b.call_by("send", json!({ "text": text }), Some(FINISH_STEP)).await {
        Ok(_) => {
            update(dir, id, |now| started(now, keys, me));
        }
        Err(e) => {
            warn!(session = id, error = e, "hosted: a session's agent didn't take the finish step's fix");
            let why = format!("couldn't give the agent the finish step's fix: {e}");
            update(dir, id, |now| {
                now.push(
                    keys,
                    me,
                    Kind::Error,
                    State::Working,
                    SessionDetail { error: Some(why), ..Default::default() },
                );
                now.push(keys, me, Kind::State, State::Done, SessionDetail::default());
                now.turn = None;
            });
            BUSY.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
        }
    }
}

/// Do action `a` to the block, as the person who asked (by login, for the
/// transcript's note, else by account id).
async fn act(b: &Arc<dyn crate::block::Block>, a: &SessionAction) {
    let id = a.request.clone().map_or(Value::Null, Value::String);
    let (method, args) = match a.kind.as_str() {
        "answer" => ("answer", json!({ "id": id, "content": a.answer.clone().unwrap_or_else(|| json!({})) })),
        "approve" => ("approve", json!({ "id": id, "option": a.option.as_deref().unwrap_or("allow-once") })),
        "deny" => ("deny", json!({ "id": id, "reason": a.reason.as_deref().unwrap_or("") })),
        "cancel" => ("cancel", json!({})),
        other => {
            warn!(action = a.id, kind = other, "hosted: a session action this box doesn't know; skipped");
            return;
        }
    };
    if let Err(e) = b.call_by(method, args, Some(a.who())).await {
        // Answered meanwhile, or nothing to cancel: it's done all the same.
        info!(action = a.id, method, error = e, "hosted: a session action didn't apply");
    }
}

/// Session `id` ended without this box (control stopped it): stop its
/// agent, close its block, kill what its user left, and free its slot.
async fn end(app: &Arc<App>, id: &str, why: &str) {
    let dir = app.control.state_dir().to_owned();
    let Some((l, _)) = update(&dir, id, |now| now.ended = Some(why.to_owned())) else { return };
    info!(session = id, why, "hosted: a session ended; stopping its agent");
    stop_agent(app, &l).await;
    clean_slot(&dir, &l).await;
}

/// Session `id` fails here, saying `why`, and its agent is stopped.
async fn fail(app: &Arc<App>, id: &str, why: &str) {
    let dir = app.control.state_dir().to_owned();
    let Some((keys, me, ..)) = me(app) else { return };
    let Some((l, _)) = update(&dir, id, |now| {
        now.push(
            &keys,
            &me,
            Kind::State,
            State::Failed,
            SessionDetail { reason: Some(why.to_owned()), ..Default::default() },
        );
        now.ended = Some(why.to_owned());
    }) else {
        return;
    };
    warn!(session = id, why, "hosted: a session failed");
    stop_agent(app, &l).await;
    clean_slot(&dir, &l).await;
    send(app, id).await;
}

async fn stop_agent(app: &App, l: &Session) {
    BUSY.lock().unwrap_or_else(|e| e.into_inner()).remove(&l.session);
    if let Some(block) = l.block {
        if let Some(b) = app.mux.api(|r| Api::Block(block, r)).await.flatten() {
            let _ = b.call("cancel", json!({})).await;
        }
        app.mux.api(|r| Api::Close(block, r)).await;
    }
    if slots().is_some_and(|s| s.real) {
        super::kill_user(&l.user.user).await;
    }
}

/// Leave an ended session's slot clean for the next session in it: what
/// its pushes left ([`push::clean`]: the daemon's repo and bundle under
/// `dir`), then its checkout and its user's `~/.claude` (its agent's
/// conversations). The slot's are removed as the session's user, who owns
/// them: never as root, through paths the agent could have made links of.
/// `end` and `fail` call it, once each session.
pub async fn clean_slot(dir: &Path, l: &Session) {
    push::clean(dir, l).await;
    let claude = Path::new(&l.user.home).join(".claude").display().to_string();
    match as_user(&l.user, &["rm", "-rf", "--", &l.checkout, &claude]).await {
        Ok(_) => info!(session = l.session, user = l.user.user, "hosted: cleaned an ended session's slot"),
        Err(e) => {
            warn!(session = l.session, user = l.user.user, error = e, "hosted: can't clean an ended session's slot")
        }
    }
}

/// Send session `id`'s reports control hasn't answered, in order, until
/// one doesn't get through.
async fn send(app: &Arc<App>, id: &str) {
    static SENDING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _one = SENDING.lock().await;
    let dir = app.control.state_dir().to_owned();
    loop {
        let Ok(Some(l)) = read(&dir, id) else { return };
        let Some(i) = l.events.iter().position(|k| !k.answered) else { return };
        let r = l.events[i].report.clone();
        let got: anyhow::Result<Reported> = app.control.post_signed(WORK_SESSION_REPORT, &r).await;
        let (refused, ended) = match got {
            Ok(Reported::Taken | Reported::Already) => (None, None),
            Ok(Reported::Refused { why, now, next }) => {
                warn!(session = id, n = r.n, kind = r.kind, %why, ?next, "hosted: control refused a session's report");
                // It moved on without this box (a person stopped it).
                let ended = now.as_deref().and_then(State::parse).filter(|s| s.terminal());
                (Some(why), ended)
            }
            Err(e) => {
                info!(session = id, error = %format!("{e:#}"), "hosted: a session's report didn't get through; trying again");
                return;
            }
        };
        update(&dir, id, |now| {
            if let Some(k) = now.events.iter_mut().find(|k| k.report.n == r.n) {
                k.answered = true;
                k.refused = refused.clone();
            }
        });
        if let Some(s) = ended
            && l.ended.is_none()
        {
            let app = app.clone();
            let id = id.to_owned();
            // Not under the sender's lock: stopping sends nothing.
            crate::task::spawn(async move { end(&app, &id, &format!("control says it's {s}")).await });
            return;
        }
    }
}

/// Send every session's unsent reports.
async fn send_all(app: &Arc<App>) {
    let dir = app.control.state_dir().to_owned();
    for l in all(&dir).into_iter().filter(|l| l.events.iter().any(|k| !k.answered)) {
        send(app, &l.session).await;
    }
}

#[path = "work_sessions_push.rs"]
pub mod push;

#[cfg(test)]
#[path = "work_sessions_tests.rs"]
mod tests;
