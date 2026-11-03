//! The machine's sharing settings (native #205): what its owner lets
//! people without a way in of their own do here. **Guest SSH
//! access**: the owner may invite a guest with only OpenSSH to a pane
//! (M65, *Invite over ssh…*, `arugula share --guest`). It was the
//! `guest-ssh` Labs flag; it is off until the owner turns it on.
//!
//! They live in one file in the state dir, `sharing.json`:
//!
//! ```json
//! { "guest_ssh": true }
//! ```
//!
//! - **Only the owner sets them.** The web client's *Guest SSH access…*
//!   in the machine's settings (the `sharing.set` operation, the owner in
//!   person) or `arugulad sharing guest-ssh on` on the machine. Control
//!   never writes this file, so nothing it pushes opens a way in, as the
//!   flag was local-only (#347).
//! - **Reads are never cached**, as the flags' aren't: a change counts at
//!   once, whoever made it.
//! - **A write keeps what it doesn't know**, as `flags.json`'s does.
//! - **A machine that had the flag keeps it.** Until this file names
//!   `guest_ssh`, the setting is what `flags.json` said of `guest-ssh` (by
//!   name, or `labs`, which was every flag); [`migrate`] writes that here
//!   once (at the daemon's start, and before `arugulad flags` changes the
//!   flags file), so a later change there doesn't move it.

use std::{fs, io, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::flags;

/// The file in the state dir that holds the sharing settings.
pub const FILE: &str = "sharing.json";

/// The Labs flag Guest SSH access was, read once to keep a machine's
/// choice ([`migrate`]).
pub const OLD_FLAG: &str = "guest-ssh";

/// What a request for a guest invite is told where Guest SSH access is
/// off: the machine is the daemon that answered.
pub const GUEST_SSH_OFF: &str = "Guest SSH access is off on this machine: its owner turns it on in the machine menu's Guest SSH access…, or with `arugulad sharing guest-ssh on` there";

/// `GET /api/sharing` (the `sharing.get` operation), and what `sharing.set`
/// answers: the machine's sharing settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Sharing {
    /// Guest SSH access: its owner may invite a guest to a pane over plain
    /// ssh. Off by default.
    #[serde(default)]
    pub guest_ssh: bool,
    /// Agent mentions (Arugula #394): teammates' messages in a thread
    /// reach this machine's agents (`@<machine>`, or a reply to one of
    /// them). On by default; the owner turns it off here.
    #[serde(default = "yes")]
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    pub agent_mentions: bool,
    /// What a teammate's agent may learn of this machine's sessions
    /// (#577). Names by default.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(as = "Option<AgentSessions>", optional))]
    pub agent_sessions: AgentSessions,
    /// Whether this build has guest ssh. A build without Labs keeps the
    /// setting, and nothing follows. Absent: it has.
    #[serde(default = "yes")]
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    pub built: bool,
}

fn yes() -> bool {
    true
}

/// What a teammate's agent may learn of this machine's sessions (#577),
/// when it asks which ones it can `@machine/name`. Only with agent mentions
/// on; off, it learns nothing whatever this says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum AgentSessions {
    /// No session is named to them.
    None,
    /// Each session's name, tab and how recently it was active.
    #[default]
    Names,
    /// Also what each works on (the issue it was started on).
    Work,
}

impl AgentSessions {
    /// The word `sharing.json` and `arugulad sharing` use.
    pub fn word(self) -> &'static str {
        match self {
            AgentSessions::None => "none",
            AgentSessions::Names => "names",
            AgentSessions::Work => "work",
        }
    }

    pub fn from_word(w: &str) -> Option<Self> {
        match w {
            "none" => Some(AgentSessions::None),
            "names" => Some(AgentSessions::Names),
            "work" => Some(AgentSessions::Work),
            _ => None,
        }
    }
}

/// `PUT /api/sharing` (the `sharing.set` operation): the settings to
/// change; one left out stays as it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SharingSetRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub guest_ssh: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub agent_mentions: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub agent_sessions: Option<AgentSessions>,
}

/// The file as it is: empty if there's none, or it doesn't parse (logged
/// by nobody: the next write replaces it).
fn doc(dir: &Path) -> Map<String, Value> {
    match fs::read_to_string(dir.join(FILE)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// Whether Guest SSH access is on in `state_dir`: the file's word, else
/// the old flag's, else off. Read on every call; it writes nothing.
pub fn guest_ssh(state_dir: &Path) -> bool {
    doc(state_dir)
        .get("guest_ssh")
        .and_then(Value::as_bool)
        .or_else(|| flags::said_here(state_dir, OLD_FLAG))
        .unwrap_or(false)
}

/// Whether teammates' messages in a thread reach this machine's agents
/// (Arugula #394): on unless the file says `agent_mentions: false`.
pub fn agent_mentions(state_dir: &Path) -> bool {
    doc(state_dir).get("agent_mentions").and_then(Value::as_bool).unwrap_or(true)
}

/// What a teammate's agent may learn of this machine's sessions (#577):
/// the file's word, else names. Unknown words are names too, the middle
/// way, so a typo neither opens nor closes it.
pub fn agent_sessions(state_dir: &Path) -> AgentSessions {
    doc(state_dir).get("agent_sessions").and_then(Value::as_str).and_then(AgentSessions::from_word).unwrap_or_default()
}

/// What agents may learn of a machine's sessions, with the owner's two
/// switches together (#576, #577): nothing when mentions are off.
pub fn sessions_shown(state_dir: &Path) -> AgentSessions {
    if agent_mentions(state_dir) { agent_sessions(state_dir) } else { AgentSessions::None }
}

/// Turns Guest SSH access on or off, keeping the rest of the file.
pub fn set_guest_ssh(state_dir: &Path, on: bool) -> io::Result<()> {
    set(state_dir, "guest_ssh", on)
}

/// Turns agent mentions on or off, keeping the rest of the file.
pub fn set_agent_mentions(state_dir: &Path, on: bool) -> io::Result<()> {
    set(state_dir, "agent_mentions", on)
}

/// Sets what agents may learn of the machine's sessions, keeping the rest
/// of the file (#577).
pub fn set_agent_sessions(state_dir: &Path, what: AgentSessions) -> io::Result<()> {
    write(state_dir, "agent_sessions", Value::String(what.word().into()))
}

fn set(state_dir: &Path, key: &str, on: bool) -> io::Result<()> {
    write(state_dir, key, Value::Bool(on))
}

fn write(state_dir: &Path, key: &str, v: Value) -> io::Result<()> {
    let mut m = doc(state_dir);
    m.insert(key.into(), v);
    let mut text = serde_json::to_string_pretty(&Value::Object(m)).expect("a JSON object serializes");
    text.push('\n');
    flags::write_atomic(&state_dir.join(FILE), text.as_bytes())
}

/// Keeps what the `guest-ssh` flag said here, once: where this file doesn't
/// name `guest_ssh` and `flags.json` said something of it, that is written
/// here. `true` if it wrote.
pub fn migrate(state_dir: &Path) -> io::Result<bool> {
    if doc(state_dir).contains_key("guest_ssh") {
        return Ok(false);
    }
    match flags::said_here(state_dir, OLD_FLAG) {
        Some(on) => set_guest_ssh(state_dir, on).map(|()| true),
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("arugula-sharing-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn guest_ssh_is_off_until_the_owner_turns_it_on() {
        let d = dir("default");
        assert!(!guest_ssh(&d));
        assert!(!migrate(&d).unwrap() && !d.join(FILE).exists());
        set_guest_ssh(&d, true).unwrap();
        assert!(guest_ssh(&d));
        set_guest_ssh(&d, false).unwrap();
        assert!(!guest_ssh(&d));
        fs::remove_dir_all(&d).unwrap();
    }

    /// A machine that had the `guest-ssh` flag on (by name, or `labs`)
    /// keeps it on; once written here, the flags file no longer counts.
    #[test]
    fn the_old_flag_is_read_once() {
        for (old, on) in [
            (r#"{"flags":{"guest-ssh":true}}"#, true),
            (r#"{"flags":{"labs":true}}"#, true),
            (r#"{"flags":{"labs":true,"guest-ssh":false}}"#, false),
        ] {
            let d = dir("migrate");
            fs::write(d.join(flags::FILE), old).unwrap();
            assert_eq!(guest_ssh(&d), on, "{old}");
            assert!(migrate(&d).unwrap() && !migrate(&d).unwrap());
            fs::write(d.join(flags::FILE), r#"{"flags":{}}"#).unwrap();
            assert_eq!(guest_ssh(&d), on, "{old}, after the flags file changed");
            fs::remove_dir_all(&d).unwrap();
        }
    }

    /// Control's word on the old flag never counts.
    #[test]
    fn control_has_no_say() {
        let d = dir("control");
        let pushed =
            flags::Pushed { account: [(OLD_FLAG.to_owned(), true)].into(), team: [(OLD_FLAG.to_owned(), true)].into() };
        flags::write_control(&d, &pushed).unwrap();
        assert!(!guest_ssh(&d) && !migrate(&d).unwrap());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn agent_sessions_are_names_until_the_owner_says_otherwise() {
        let d = dir("sessions");
        assert_eq!(agent_sessions(&d), AgentSessions::Names);
        for what in [AgentSessions::Work, AgentSessions::None, AgentSessions::Names] {
            set_agent_sessions(&d, what).unwrap();
            assert_eq!(agent_sessions(&d), what);
        }
        // Another key's write keeps it, and a word it doesn't know is the default.
        set_agent_sessions(&d, AgentSessions::Work).unwrap();
        set_agent_mentions(&d, false).unwrap();
        assert_eq!(agent_sessions(&d), AgentSessions::Work);
        assert_eq!(sessions_shown(&d), AgentSessions::None, "mentions off shows nothing");
        std::fs::write(d.join(FILE), r#"{"agent_sessions": "everything"}"#).unwrap();
        assert_eq!(agent_sessions(&d), AgentSessions::Names);
    }

    #[test]
    fn agent_mentions_are_on_until_the_owner_turns_them_off() {
        let d = dir("mentions");
        assert!(agent_mentions(&d));
        set_agent_mentions(&d, false).unwrap();
        assert!(!agent_mentions(&d) && !guest_ssh(&d));
        set_agent_mentions(&d, true).unwrap();
        assert!(agent_mentions(&d));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_write_keeps_what_it_doesnt_know() {
        let d = dir("keep");
        fs::write(d.join(FILE), r#"{"later":1}"#).unwrap();
        set_guest_ssh(&d, true).unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(d.join(FILE)).unwrap()).unwrap();
        assert_eq!(v["later"], 1);
        assert_eq!(v["guest_ssh"], true);
        fs::remove_dir_all(&d).unwrap();
    }
}
