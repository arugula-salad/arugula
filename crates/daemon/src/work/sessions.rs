//! The sessions this box holds and the users they run as (M255), as the
//! rest of the daemon reads them: the agent blocks (which users an agent
//! may run as, and which block is a session's), the hosted box and a
//! session's MCP. The engine that takes, follows and reports them is
//! `work_engine::sessions`, which writes what these read.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use arugula_control_wire::work::{MAX_HELD_SESSIONS_PER_BOX, SessionReport};
use arugula_e2e::session::State;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::agent::hosted::AsUser;

/// Sessions' ledgers, in the run ledger's directory.
pub const DIR: &str = "sessions";

/// Each session block's session, as this daemon opened or found it: what
/// its agent's turns are asked of the platform as ([`session_of`]).
static BLOCK_SESSIONS: Mutex<BTreeMap<u32, String>> = Mutex::new(BTreeMap::new());

/// The session agent block `block` runs, if it's a session's: the daemon
/// says so, never the block's own config.
pub fn session_of(block: u32) -> Option<String> {
    BLOCK_SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).get(&block).cloned()
}

/// Notes that session agent block `block` runs `session`.
pub fn block_is(block: u32, session: &str) {
    BLOCK_SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).insert(block, session.to_owned());
}

/// Notes that block `block` runs no session now.
pub fn block_gone(block: u32) {
    BLOCK_SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).remove(&block);
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
    /// Where it is, by this box's own reports.
    pub fn now(&self) -> State {
        State::parse(&self.state).unwrap_or(State::Queued)
    }
}

/// Whether `id` is a session's (`ses_` and 32 hex): control's word goes in
/// a file name only once it's that.
pub fn good_id(id: &str) -> bool {
    id.strip_prefix("ses_").is_some_and(|h| h.len() == 32 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// Session `id`'s ledger's path, if `id` is a session's.
pub fn path(dir: &Path, id: &str) -> Option<PathBuf> {
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
