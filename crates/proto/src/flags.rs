//! Named flags in the daemon's config (#464, #665): what a machine has
//! turned on that a stranger doesn't get, one flag to a Labs feature.
//!
//! They live in one file in the state dir, `flags.json`:
//!
//! ```json
//! { "flags": { "chat": true } }
//! ```
//!
//! - **The registry is [`FLAGS`].** A flag is a row there: its name, a
//!   sentence about it and its default. A name that isn't a row is an error
//!   to [`set`] and off to [`get`]. A feature of #347 adds a row and gates
//!   itself on `get`; nothing here changes.
//! - **Reads are never cached.** [`get`] reads the file on every call, so a
//!   flag flips with no restart, whoever changed it (the daemon for the web's
//!   switch, `arugulad flags` from another process). The file is small and
//!   the callers are a request, not a loop.
//! - **A write keeps what it doesn't know.** [`set`] reads the file as it is
//!   now, changes one flag and renames a new file over it, so keys and flags
//!   an older or newer daemon wrote survive. Two writers at once can lose the
//!   one that renames first; they are a person at a switch and a person at a
//!   terminal, and the loser sees the flag as it now is.
//! - **A file that doesn't parse is not an emergency.** It is logged once and
//!   read as "every flag at its default"; only an explicit [`set`] replaces
//!   it, and keeps the old one beside it as `flags.json.bad`.
//! - **`labs` is every flag.** Before each feature had a flag there was one
//!   switch: an empty `labs` file in the state dir (#385), then `labs` in
//!   `flags.json` (#464). Either still means "on" for every flag the file
//!   doesn't itself name, so a machine that had Labs keeps all of it, and
//!   turning one feature off there leaves the rest. The daemon moves the old
//!   file into `flags.json` when it starts ([`migrate_legacy`]). Nothing
//!   offers `labs` as a flag any more.
//! - **The file's presence is the owner asking.** A machine with a
//!   `flags.json` (or the old file) has asked for Developer settings
//!   ([`unlocked`]), and its menus offer them; a new install's don't.

use std::{
    fs, io,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The file in the state dir that holds the flags.
pub const FILE: &str = "flags.json";

/// What #385 used before this: an empty file named `labs`. Read as a
/// fallback and migrated ([`migrate_legacy`]); nothing writes it any more.
pub const LEGACY_LABS_FILE: &str = "labs";

/// The key that meant all of Labs before each feature had a flag (#464):
/// in the file it still stands for every flag the file doesn't name. Not a
/// flag in [`FLAGS`].
pub const LABS: &str = "labs";

/// Threads on panes and sessions, and the chat page.
pub const CHAT: &str = "chat";
/// Voice calls on a session.
pub const HUDDLES: &str = "huddles";
/// VM tabs and panes, machines and the sandboxes page.
pub const VMS: &str = "vms";
/// Fountain agents and the runner view.
pub const FOUNTAIN: &str = "fountain";
/// Studio app blocks.
pub const STUDIO: &str = "studio";
/// Chant workspace blocks.
pub const WORKSPACES: &str = "workspaces";
/// Inviting a guest to a pane over plain ssh.
pub const GUEST_SSH: &str = "guest-ssh";
/// The swarm's views beside the default.
pub const SWARM_THEMES: &str = "swarm-themes";
/// Forgejo and GitLab blocks, beside GitHub's.
pub const FORGES: &str = "forges";
/// Agent recipes offered from this machine, and the team's agent catalog.
pub const AGENTS: &str = "agents";

/// One flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flag {
    /// What `arugulad flags` and the file call it.
    pub name: &'static str,
    /// What Developer settings calls it.
    pub title: &'static str,
    /// One sentence, for the list in `arugulad flags` and Settings.
    pub about: &'static str,
    /// What it is until someone sets it.
    pub default: bool,
}

const fn row(name: &'static str, title: &'static str, about: &'static str) -> Flag {
    Flag { name, title, about, default: false }
}

/// Every flag there is. A new one is a row here.
pub const FLAGS: &[Flag] = &[
    row(CHAT, "Chat", "Threads on panes and sessions, and the chat page."),
    row(HUDDLES, "Huddles", "Voice calls on a session."),
    row(VMS, "VMs and sandboxes", "VM tabs and panes, machines and the sandboxes page. Needs a sandbox provider."),
    row(FOUNTAIN, "Fountain", "Fountain agents and the runner view. Needs a Fountain login."),
    row(STUDIO, "Studio apps", "Studio app blocks. Needs a linked studio."),
    row(WORKSPACES, "Chant workspaces", "Chant workspace blocks."),
    row(GUEST_SSH, "Guest ssh", "Invite a guest to a pane over plain ssh."),
    row(SWARM_THEMES, "Extra swarm views", "The swarm's other views beside the default."),
    row(FORGES, "Forgejo and GitLab", "PR and issue blocks for Forgejo and GitLab, beside GitHub's."),
    row(AGENTS, "Agent catalog", "Offer Claude Code subagents from this machine, and see the ones your team offers."),
];

/// The flags that are on, from one read: what a listing or a `--help` is
/// built from, where [`get`] would read the file once per flag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct On(u32);

impl On {
    pub fn none() -> On {
        On(0)
    }

    pub fn all() -> On {
        On((1 << FLAGS.len()) - 1)
    }

    /// Whether `name` is on; a name that isn't in [`FLAGS`] is off.
    pub fn has(self, name: &str) -> bool {
        FLAGS.iter().position(|f| f.name == name).is_some_and(|i| self.0 & (1 << i) != 0)
    }

    pub fn any(self) -> bool {
        self.0 != 0
    }

    /// Their names, in [`FLAGS`]' order.
    pub fn names(self) -> impl Iterator<Item = &'static str> {
        FLAGS.iter().enumerate().filter(move |(i, _)| self.0 & (1 << i) != 0).map(|(_, f)| f.name)
    }
}

/// The flags that are on in `state_dir`. Read on every call.
pub fn on(state_dir: &Path) -> On {
    let doc = read(state_dir);
    let bits = FLAGS.iter().enumerate().filter(|(_, f)| doc.resolve(state_dir, f)).fold(0, |b, (i, _)| b | (1 << i));
    On(bits)
}

/// Whether `state_dir`'s owner has asked for Developer settings: it has a
/// flags file, or the `labs` file from before.
pub fn unlocked(state_dir: &Path) -> bool {
    state_dir.join(FILE).exists() || state_dir.join(LEGACY_LABS_FILE).exists()
}

fn flag(name: &str) -> Option<&'static Flag> {
    FLAGS.iter().find(|f| f.name == name)
}

/// `GET /api/flags`: one row per flag (the `flags.list` operation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FlagInfo {
    pub name: String,
    /// What Developer settings calls it. Absent from a daemon before #665.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(as = "Option<String>", optional))]
    pub title: String,
    pub about: String,
    /// Whether it is on here now.
    pub on: bool,
    pub default: bool,
    /// Whether this build has what the flag turns on. A build without Labs
    /// can set a flag, and nothing follows; Settings says so. Absent from
    /// daemons that don't report it, which have it.
    #[serde(default = "yes")]
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    pub built: bool,
}

fn yes() -> bool {
    true
}

/// `PUT /api/flags/{name}`: the body (the `flag.set` operation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FlagSetRequest {
    pub on: bool,
}

/// The file as the last read found it.
enum Doc {
    /// No file, or an empty one.
    Missing,
    Ok(Map<String, Value>),
    /// Not JSON, or not an object, or `flags` isn't one.
    Malformed,
}

fn read(dir: &Path) -> Doc {
    let text = match fs::read_to_string(dir.join(FILE)) {
        Ok(t) if !t.trim().is_empty() => t,
        Ok(_) | Err(_) => return Doc::Missing,
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(m)) if m.get("flags").is_none_or(Value::is_object) => Doc::Ok(m),
        _ => {
            // Once per process: a request loop would say it every time.
            static SAID: AtomicBool = AtomicBool::new(false);
            if !SAID.swap(true, Ordering::Relaxed) {
                tracing::warn!(file = %dir.join(FILE).display(), "flags.json isn't a flags file: every flag is at its default");
            }
            Doc::Malformed
        }
    }
}

impl Doc {
    /// What the file says about `name`, if it says.
    fn says(&self, name: &str) -> Option<bool> {
        match self {
            Doc::Ok(m) => m.get("flags")?.get(name)?.as_bool(),
            Doc::Missing | Doc::Malformed => None,
        }
    }

    /// A flag's state: the file's word on it, else its word on `labs`
    /// (which was all of them), else the old marker, else its default.
    fn resolve(&self, dir: &Path, f: &Flag) -> bool {
        self.says(f.name)
            .or_else(|| self.says(LABS))
            .unwrap_or_else(|| dir.join(LEGACY_LABS_FILE).exists() || f.default)
    }
}

/// Whether the flag `name` is on in `state_dir`. Read on every call; a
/// name that isn't in [`FLAGS`] is off.
pub fn get(state_dir: &Path, name: &str) -> bool {
    flag(name).is_some_and(|f| read(state_dir).resolve(state_dir, f))
}

/// Every flag with its state, from one read of the file.
pub fn all(state_dir: &Path) -> Vec<(Flag, bool)> {
    let doc = read(state_dir);
    FLAGS.iter().map(|f| (*f, doc.resolve(state_dir, f))).collect()
}

/// Turns the flag `name` on or off, keeping everything else in the file.
/// An unknown name is `InvalidInput`.
pub fn set(state_dir: &Path, name: &str, on: bool) -> io::Result<()> {
    if flag(name).is_none() {
        let known: Vec<_> = FLAGS.iter().map(|f| f.name).collect();
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("no flag named {name} (the flags are {})", known.join(", ")),
        ));
    }
    write(state_dir, Some((name, on)))
}

/// Makes the flags file if there is none, with nothing in it: the owner
/// asking for Developer settings ([`unlocked`]) without turning anything on.
pub fn unlock(state_dir: &Path) -> io::Result<()> {
    if state_dir.join(FILE).exists() { Ok(()) } else { write(state_dir, None) }
}

/// The file written again with `key` set, if one is given.
fn write(state_dir: &Path, key: Option<(&str, bool)>) -> io::Result<()> {
    let mut doc = match read(state_dir) {
        Doc::Ok(m) => m,
        Doc::Missing => Map::new(),
        Doc::Malformed => {
            // An explicit set replaces it, with the old one kept.
            let _ = fs::copy(state_dir.join(FILE), state_dir.join(format!("{FILE}.bad")));
            Map::new()
        }
    };
    let flags = doc.entry("flags").or_insert_with(|| Value::Object(Map::new()));
    if let (Value::Object(flags), Some((name, on))) = (flags, key) {
        flags.insert(name.to_owned(), Value::Bool(on));
    }
    let mut text = serde_json::to_string_pretty(&doc).expect("a JSON object serializes");
    text.push('\n');
    write_atomic(&state_dir.join(FILE), text.as_bytes())
}

/// The old `labs` marker file, moved into `flags.json`: `labs` on there
/// (every flag the file doesn't name) unless the file already says one way
/// or the other, and the marker removed.
/// `true` if there was a marker. Run by the daemon at startup.
pub fn migrate_legacy(state_dir: &Path) -> io::Result<bool> {
    let marker = state_dir.join(LEGACY_LABS_FILE);
    if !marker.exists() {
        return Ok(false);
    }
    if read(state_dir).says(LABS).is_none() {
        write(state_dir, Some((LABS, true)))?;
    }
    fs::remove_file(&marker)?;
    Ok(true)
}

/// A new file renamed over the old, so a reader sees all of one or all of
/// the other. Private to the owner, as the state dir's other files are. The
/// temporary file's name has our pid, so two writers don't share one.
/// (`rename` replaces an existing file on Windows too.)
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use io::Write;
    if let Some(dir) = path.parent() {
        private_dir(dir)?;
    }
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new();
    file.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut file, 0o600);
    let written = (|| {
        let mut f = file.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

fn private_dir(dir: &Path) -> io::Result<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("arugula-flags-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn names(d: &Path) -> Vec<&'static str> {
        on(d).names().collect()
    }

    #[test]
    fn a_flag_is_at_its_default_until_set_and_then_follows_the_file() {
        let d = dir("set");
        assert!(!get(&d, CHAT) && !on(&d).any() && !unlocked(&d));
        set(&d, CHAT, true).unwrap();
        assert!(get(&d, CHAT) && unlocked(&d));
        assert_eq!(
            fs::read_to_string(d.join(FILE)).unwrap().replace(char::is_whitespace, ""),
            r#"{"flags":{"chat":true}}"#
        );
        // One flag turns on alone.
        assert_eq!(names(&d), [CHAT]);
        set(&d, GUEST_SSH, true).unwrap();
        set(&d, CHAT, false).unwrap();
        assert_eq!(names(&d), [GUEST_SSH]);
        assert_eq!(all(&d).iter().filter(|(_, on)| *on).map(|(f, _)| f.name).collect::<Vec<_>>(), [GUEST_SSH]);
        assert_eq!(all(&d).len(), FLAGS.len());
        // All off is still an answer: the file stays, and so does the asking.
        set(&d, GUEST_SSH, false).unwrap();
        assert!(!on(&d).any() && unlocked(&d));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn asking_makes_an_empty_file_and_leaves_a_file_alone() {
        let d = dir("unlock");
        unlock(&d).unwrap();
        assert!(unlocked(&d) && !on(&d).any());
        set(&d, CHAT, true).unwrap();
        let before = fs::read_to_string(d.join(FILE)).unwrap();
        unlock(&d).unwrap();
        assert_eq!(fs::read_to_string(d.join(FILE)).unwrap(), before);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_set_of_flags_knows_all_none_and_each_name() {
        assert_eq!(On::all().names().count(), FLAGS.len());
        assert!(On::all().any() && !On::none().any());
        for f in FLAGS {
            assert!(On::all().has(f.name) && !On::none().has(f.name), "{}", f.name);
            assert!(!f.default, "{} is on for a stranger", f.name);
        }
        assert!(!On::all().has(LABS) && !On::all().has("nope"));
    }

    #[test]
    fn an_unknown_flag_is_off_and_an_error_to_set() {
        let d = dir("unknown");
        assert!(!get(&d, "nope"));
        let e = set(&d, "nope", true).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
        assert!(e.to_string().contains("chat"), "{e}");
        assert!(!d.join(FILE).exists());
        // `labs` was a flag once (#464); it isn't one to set now.
        assert_eq!(set(&d, LABS, true).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        // Even if the file names it.
        fs::write(d.join(FILE), r#"{"flags":{"nope":true}}"#).unwrap();
        assert!(!get(&d, "nope"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_write_keeps_the_keys_and_flags_it_doesnt_know() {
        let d = dir("keeps");
        fs::write(d.join(FILE), r#"{"other":{"a":1},"flags":{"future":true,"chat":false}}"#).unwrap();
        set(&d, CHAT, true).unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(d.join(FILE)).unwrap()).unwrap();
        assert_eq!(v, serde_json::json!({"other":{"a":1},"flags":{"future":true,"chat":true}}));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_malformed_file_reads_as_defaults_and_only_a_set_replaces_it() {
        let d = dir("bad");
        for bad in ["{not json", "[]", r#"{"flags":3}"#] {
            fs::write(d.join(FILE), bad).unwrap();
            assert!(!get(&d, CHAT), "{bad}");
            assert_eq!(fs::read_to_string(d.join(FILE)).unwrap(), bad, "a read left it alone");
        }
        fs::write(d.join(FILE), "{not json").unwrap();
        set(&d, CHAT, true).unwrap();
        assert!(get(&d, CHAT));
        assert_eq!(fs::read_to_string(d.join(format!("{FILE}.bad"))).unwrap(), "{not json");
        fs::remove_dir_all(&d).unwrap();
    }

    /// What was one switch is every flag: the old `labs` file, and `labs`
    /// in the flags file. A flag the file names for itself follows that.
    #[test]
    fn labs_from_before_is_every_flag_the_file_doesnt_name() {
        let d = dir("legacy");
        fs::write(d.join(LEGACY_LABS_FILE), "").unwrap();
        assert_eq!(on(&d), On::all());
        assert!(unlocked(&d));
        // One turned off there leaves the rest.
        set(&d, FOUNTAIN, false).unwrap();
        assert!(!get(&d, FOUNTAIN) && get(&d, CHAT));
        assert_eq!(names(&d).len(), FLAGS.len() - 1);
        fs::remove_file(d.join(LEGACY_LABS_FILE)).unwrap();
        assert!(!on(&d).any());

        // `labs` in the file, as #464 wrote it: on is all of them, off none.
        fs::write(d.join(FILE), r#"{"flags":{"labs":true,"huddles":false}}"#).unwrap();
        assert!(get(&d, CHAT) && !get(&d, HUDDLES));
        fs::write(d.join(FILE), r#"{"flags":{"labs":false,"chat":true}}"#).unwrap();
        assert_eq!(names(&d), [CHAT]);
        // And off there wins over an old file still lying about.
        fs::write(d.join(LEGACY_LABS_FILE), "").unwrap();
        assert_eq!(names(&d), [CHAT]);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn migrating_moves_the_marker_into_the_flags_file() {
        let d = dir("migrate");
        assert!(!migrate_legacy(&d).unwrap());
        fs::write(d.join(LEGACY_LABS_FILE), "anything").unwrap();
        assert!(migrate_legacy(&d).unwrap());
        assert!(!d.join(LEGACY_LABS_FILE).exists());
        assert_eq!(on(&d), On::all());
        assert!(!migrate_legacy(&d).unwrap());

        // A file that already says off wins over the marker, which still goes.
        fs::write(d.join(FILE), r#"{"flags":{"labs":false}}"#).unwrap();
        fs::write(d.join(LEGACY_LABS_FILE), "").unwrap();
        assert!(migrate_legacy(&d).unwrap());
        assert!(!d.join(LEGACY_LABS_FILE).exists());
        assert!(!on(&d).any());
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_private_to_the_owner() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("mode");
        set(&d, CHAT, true).unwrap();
        assert_eq!(fs::metadata(d.join(FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        let tmps = fs::read_dir(&d)
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(tmps, 0);
        fs::remove_dir_all(&d).unwrap();
    }
}
