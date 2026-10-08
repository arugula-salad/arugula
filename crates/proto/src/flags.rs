//! Named flags in the daemon's config (#464, the first of #347's): what a
//! machine has turned on that a stranger doesn't get. Today that is Labs.
//!
//! They live in one file in the state dir, `flags.json`:
//!
//! ```json
//! { "flags": { "labs": true } }
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
//! - **The old marker file** (#385, an empty `labs` file) still turns Labs on
//!   while `flags.json` has no `labs` of its own, so a CLI that is newer than
//!   its daemon, or a daemon not yet restarted, agrees with the file the owner
//!   made. The daemon moves it into `flags.json` when it starts
//!   ([`migrate_legacy`]). The fallback goes in a later release.

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

/// The name of the flag that turns on what a stranger doesn't get: huddles,
/// chat, Fountain, studio, VMs, guest ssh, the swarm's extra views and the
/// Forgejo and GitLab forges.
pub const LABS: &str = "labs";

/// One flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flag {
    /// What `arugulad flags` and the file call it.
    pub name: &'static str,
    /// One sentence, for the list in `arugulad flags` and Settings.
    pub about: &'static str,
    /// What it is until someone sets it.
    pub default: bool,
}

/// Every flag there is. A new one is a row here.
pub const FLAGS: &[Flag] = &[Flag {
    name: LABS,
    about: "Turns on what a new install doesn't show: chat and threads, huddles, Fountain, studio apps, \
            workspaces, VMs and sandboxes, ssh invites for guests, GitLab and Forgejo blocks and the swarm's \
            extra views.",
    default: false,
}];

fn flag(name: &str) -> Option<&'static Flag> {
    FLAGS.iter().find(|f| f.name == name)
}

/// `GET /api/flags`: one row per flag (the `flags.list` operation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FlagInfo {
    pub name: String,
    pub about: String,
    /// Whether it is on here now.
    pub on: bool,
    pub default: bool,
    /// Whether this build has what the flag turns on. A build without Labs
    /// can set `labs`, and nothing follows; Settings says so. Absent from
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

    /// `name`'s state: the file's word, else the old marker (for `labs`),
    /// else its default.
    fn resolve(&self, dir: &Path, f: &Flag) -> bool {
        self.says(f.name).unwrap_or_else(|| (f.name == LABS && dir.join(LEGACY_LABS_FILE).exists()) || f.default)
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
    if let Value::Object(flags) = flags {
        flags.insert(name.to_owned(), Value::Bool(on));
    }
    let mut text = serde_json::to_string_pretty(&doc).expect("a JSON object serializes");
    text.push('\n');
    write_atomic(&state_dir.join(FILE), text.as_bytes())
}

/// The old `labs` marker file, moved into `flags.json`: Labs on there unless
/// the file already says one way or the other, and the marker removed.
/// `true` if there was a marker. Run by the daemon at startup.
pub fn migrate_legacy(state_dir: &Path) -> io::Result<bool> {
    let marker = state_dir.join(LEGACY_LABS_FILE);
    if !marker.exists() {
        return Ok(false);
    }
    if read(state_dir).says(LABS).is_none() {
        set(state_dir, LABS, true)?;
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

    #[test]
    fn a_flag_is_at_its_default_until_set_and_then_follows_the_file() {
        let d = dir("set");
        assert!(!get(&d, LABS));
        set(&d, LABS, true).unwrap();
        assert!(get(&d, LABS));
        assert_eq!(
            fs::read_to_string(d.join(FILE)).unwrap().replace(char::is_whitespace, ""),
            r#"{"flags":{"labs":true}}"#
        );
        set(&d, LABS, false).unwrap();
        assert!(!get(&d, LABS));
        assert_eq!(all(&d).iter().map(|(f, on)| (f.name, *on)).collect::<Vec<_>>(), [(LABS, false)]);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn an_unknown_flag_is_off_and_an_error_to_set() {
        let d = dir("unknown");
        assert!(!get(&d, "nope"));
        let e = set(&d, "nope", true).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
        assert!(e.to_string().contains("labs"), "{e}");
        assert!(!d.join(FILE).exists());
        // Even if the file names it.
        fs::write(d.join(FILE), r#"{"flags":{"nope":true}}"#).unwrap();
        assert!(!get(&d, "nope"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_write_keeps_the_keys_and_flags_it_doesnt_know() {
        let d = dir("keeps");
        fs::write(d.join(FILE), r#"{"other":{"a":1},"flags":{"future":true,"labs":false}}"#).unwrap();
        set(&d, LABS, true).unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(d.join(FILE)).unwrap()).unwrap();
        assert_eq!(v, serde_json::json!({"other":{"a":1},"flags":{"future":true,"labs":true}}));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_malformed_file_reads_as_defaults_and_only_a_set_replaces_it() {
        let d = dir("bad");
        for bad in ["{not json", "[]", r#"{"flags":3}"#] {
            fs::write(d.join(FILE), bad).unwrap();
            assert!(!get(&d, LABS), "{bad}");
            assert_eq!(fs::read_to_string(d.join(FILE)).unwrap(), bad, "a read left it alone");
        }
        fs::write(d.join(FILE), "{not json").unwrap();
        set(&d, LABS, true).unwrap();
        assert!(get(&d, LABS));
        assert_eq!(fs::read_to_string(d.join(format!("{FILE}.bad"))).unwrap(), "{not json");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_old_marker_file_turns_labs_on_until_the_flags_file_says_otherwise() {
        let d = dir("legacy");
        fs::write(d.join(LEGACY_LABS_FILE), "").unwrap();
        assert!(get(&d, LABS));
        // A flags file with other flags only: the marker still counts.
        fs::write(d.join(FILE), r#"{"flags":{"future":true}}"#).unwrap();
        assert!(get(&d, LABS));
        // Once it says `labs`, it decides, off included.
        set(&d, LABS, false).unwrap();
        assert!(!get(&d, LABS));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn migrating_moves_the_marker_into_the_flags_file() {
        let d = dir("migrate");
        assert!(!migrate_legacy(&d).unwrap());
        fs::write(d.join(LEGACY_LABS_FILE), "anything").unwrap();
        assert!(migrate_legacy(&d).unwrap());
        assert!(!d.join(LEGACY_LABS_FILE).exists());
        assert!(get(&d, LABS));
        assert!(!migrate_legacy(&d).unwrap());

        // A file that already says off wins over the marker, which still goes.
        fs::write(d.join(FILE), r#"{"flags":{"labs":false}}"#).unwrap();
        fs::write(d.join(LEGACY_LABS_FILE), "").unwrap();
        assert!(migrate_legacy(&d).unwrap());
        assert!(!d.join(LEGACY_LABS_FILE).exists());
        assert!(!get(&d, LABS));
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_private_to_the_owner() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("mode");
        set(&d, LABS, true).unwrap();
        assert_eq!(fs::metadata(d.join(FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        let tmps = fs::read_dir(&d)
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(tmps, 0);
        fs::remove_dir_all(&d).unwrap();
    }
}
