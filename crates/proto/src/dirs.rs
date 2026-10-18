//! Where Arugula keeps its files on this machine: the state directory
//! (`~/.local/state/arugula`, Windows `%LOCALAPPDATA%\arugula\state`) and
//! the config directory (`~/.config/arugula`). The daemon, the CLI and the
//! desktop app all ask here, so they agree.
//!
//! A machine set up as illogical had them under the old name. The first
//! time anything asks, each is moved to the new name and a link is left
//! under the old one (#509): running shims, pane records and agents hold
//! paths under it, and they go on working through the link. Where it
//! can't be moved (a file in it is open on Windows, no link can be made),
//! it's used where it is, with a link under the new name beside it.

use std::path::{Path, PathBuf};

use crate::rename::OLD;

const NEW: &str = "arugula";

/// `new`, moved there from `old` if only `old` exists (with a link
/// `old → new` left behind). If it can't be moved: `old`, as it is, with a
/// link `new → old` made beside it (Unix) so it's found under the new name.
pub fn renamed(new: &Path, old: &Path) -> PathBuf {
    let is_link = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink());
    if let Ok(m) = std::fs::symlink_metadata(new) {
        if !m.file_type().is_symlink() {
            return new.to_owned();
        }
        // The link an older daemon made (or one that leads to the same
        // place): the directory is still under the old name, so it moves.
        let same = |a: &Path, b: &Path| a.canonicalize().ok().is_some_and(|a| b.canonicalize().ok() == Some(a));
        if !same(new, old) || is_link(old) {
            // A link to the old directory that's gone (it was deleted to
            // start again): out of the way, so the new one can be made.
            if !new.exists()
                && std::fs::read_link(new).is_ok_and(|t| t == old || Some(t.as_os_str()) == old.file_name())
            {
                let _ = std::fs::remove_file(new);
            }
            return new.to_owned();
        }
    } else if !old.is_dir() {
        return new.to_owned();
    }
    if moved(new, old) {
        return new.to_owned();
    }
    #[cfg(unix)]
    if let Some(name) = old.file_name().filter(|_| old.parent() == new.parent()) {
        // Relative, so it still holds if the home directory moves. Two at
        // once is fine: one makes it, the other finds it made.
        let _ = std::os::unix::fs::symlink(name, new);
    }
    old.to_owned()
}

/// Move `old` to `new` and leave a link to it at `old`. False if it's still
/// at `old`; true if it's at `new`, by this or by another process at once.
fn moved(new: &Path, old: &Path) -> bool {
    let real = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir());
    if let Some(parent) = new.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Our own link `new → old`, in the way.
    if std::fs::symlink_metadata(new).is_ok() && std::fs::remove_file(new).is_err() {
        return real(new);
    }
    if std::fs::rename(old, new).is_err() {
        return real(new);
    }
    if link_back(new, old).is_err() {
        // Nothing may lose a path it holds: back where it was.
        return std::fs::rename(new, old).is_err();
    }
    true
}

#[cfg(unix)]
fn link_back(new: &Path, old: &Path) -> std::io::Result<()> {
    let target = match new.file_name().filter(|_| old.parent() == new.parent()) {
        Some(name) => PathBuf::from(name),
        None => new.to_owned(),
    };
    std::os::unix::fs::symlink(target, old)
}

/// A symbolic link if this account may make one, else a junction, which
/// any account may.
#[cfg(windows)]
fn link_back(new: &Path, old: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    if std::os::windows::fs::symlink_dir(new, old).is_ok() {
        return Ok(());
    }
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let ok = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(old)
        .arg(new)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()?
        .success();
    if ok { Ok(()) } else { Err(std::io::Error::other("mklink /J failed")) }
}

/// Every directory made under the old name, moved to the new one: the
/// state, config, data, cache and lib directories and a sandbox's state.
/// The daemon does this as it starts; anything else that asks for one of
/// them first moves it then.
pub fn migrate() {
    let _ = default_state_dir();
    let _ = config_dir();
    for (var, rel) in [("XDG_DATA_HOME", ".local/share"), ("XDG_CACHE_HOME", ".cache")] {
        if let Some(base) = xdg(var, rel) {
            named_in(&base, NEW, OLD);
        }
    }
    if let Some(home) = home() {
        named_in(&home.join(".local/lib"), NEW, OLD);
    }
    let _ = state_home("arugula-sandbox", "illogical-sandbox");
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// `$VAR`, else `~/REL`.
fn xdg(var: &str, rel: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| Some(home()?.join(rel)))
}

/// `base/<new>`, moved there from `base/<old>` where only that exists.
pub fn named_in(base: &Path, new: &str, old: &str) -> PathBuf {
    renamed(&base.join(new), &base.join(old))
}

/// The daemon's state directory when nothing says otherwise:
/// `%LOCALAPPDATA%\arugula\state` on Windows, else
/// `$XDG_STATE_HOME/arugula` or `~/.local/state/arugula` (moved there from
/// the `illogical` one a machine already has).
pub fn default_state_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()).map(PathBuf::from) {
        // `%LOCALAPPDATA%\arugula` is also where the desktop app installs
        // itself, so it's the `state` folders that are compared.
        return Some(renamed(&local.join(NEW).join("state"), &local.join(OLD).join("state")));
    }
    state_home(NEW, OLD)
}

/// `ARUGULA_STATE_DIR`, else [`default_state_dir`].
pub fn state_dir() -> Option<PathBuf> {
    std::env::var_os("ARUGULA_STATE_DIR").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(default_state_dir)
}

/// `$XDG_STATE_HOME/<new>` (or `~/.local/state/<new>`), moved there from
/// `<old>` where only that exists: the sandbox's own directory, say.
pub fn state_home(new: &str, old: &str) -> Option<PathBuf> {
    Some(named_in(&xdg("XDG_STATE_HOME", ".local/state")?, new, old))
}

/// `$XDG_CONFIG_HOME/arugula` or `~/.config/arugula` (moved there from the
/// `illogical` one a machine already has): keys, tokens, the CLI's login.
pub fn config_dir() -> Option<PathBuf> {
    Some(named_in(&xdg("XDG_CONFIG_HOME", ".config")?, NEW, OLD))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arugula-dirs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(unix)]
    #[test]
    fn only_the_old_one_is_moved_and_linked_back() {
        let d = scratch("old");
        std::fs::create_dir(d.join("illogical")).unwrap();
        std::fs::write(d.join("illogical/layout.json"), "{}").unwrap();
        // A program that opened a file before the move still has it.
        let open = std::fs::File::open(d.join("illogical/layout.json")).unwrap();
        assert_eq!(named_in(&d, "arugula", "illogical"), d.join("arugula"));
        assert!(std::fs::symlink_metadata(d.join("arugula")).unwrap().is_dir());
        assert!(d.join("arugula/layout.json").is_file());
        // The old path still leads there, for what holds it.
        assert_eq!(std::fs::read_link(d.join("illogical")).unwrap(), Path::new("arugula"));
        assert!(d.join("illogical/layout.json").is_file());
        assert!(open.metadata().unwrap().is_file());
        // Asked again: the same, nothing moved twice.
        assert_eq!(named_in(&d, "arugula", "illogical"), d.join("arugula"));
        assert_eq!(std::fs::read_link(d.join("illogical")).unwrap(), Path::new("arugula"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// What a daemon from before this made: the old directory, with a link
    /// to it under the new name. It's turned around.
    #[cfg(unix)]
    #[test]
    fn the_link_an_older_daemon_made_is_turned_around() {
        let d = scratch("flip");
        std::fs::create_dir(d.join("illogical")).unwrap();
        std::fs::write(d.join("illogical/sock"), "").unwrap();
        std::os::unix::fs::symlink("illogical", d.join("arugula")).unwrap();
        assert_eq!(named_in(&d, "arugula", "illogical"), d.join("arugula"));
        assert!(std::fs::symlink_metadata(d.join("arugula")).unwrap().is_dir());
        assert_eq!(std::fs::read_link(d.join("illogical")).unwrap(), Path::new("arugula"));
        assert!(d.join("illogical/sock").is_file());
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// Not in the same directory (Windows' `illogical\state` and
    /// `arugula\state`): the new parent is made, the link is absolute (on
    /// Windows a symbolic link, or else a junction).
    #[test]
    fn moved_across_parents() {
        let d = scratch("parents");
        let (new, old) = (d.join("arugula").join("state"), d.join("illogical").join("state"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("listen"), "x").unwrap();
        assert_eq!(renamed(&new, &old), new);
        assert!(std::fs::symlink_metadata(&new).unwrap().is_dir());
        assert!(!std::fs::symlink_metadata(&old).unwrap().is_dir(), "a link now");
        assert_eq!(std::fs::read_to_string(old.join("listen")).unwrap(), "x");
        assert_eq!(renamed(&new, &old), new, "and again");
        // A junction goes with remove_dir, not remove_file.
        #[cfg(windows)]
        let _ = std::fs::remove_dir(&old);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn only_the_new_one() {
        let d = scratch("new");
        std::fs::create_dir(d.join("arugula")).unwrap();
        assert_eq!(named_in(&d, "arugula", "illogical"), d.join("arugula"));
        assert!(!d.join("illogical").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn both_is_the_new_one() {
        let d = scratch("both");
        std::fs::create_dir(d.join("arugula")).unwrap();
        std::fs::create_dir(d.join("illogical")).unwrap();
        assert_eq!(named_in(&d, "arugula", "illogical"), d.join("arugula"));
        assert!(!std::fs::symlink_metadata(d.join("arugula")).unwrap().file_type().is_symlink());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn neither_is_the_new_one_and_nothing_is_made() {
        let d = scratch("neither");
        assert_eq!(named_in(&d, "arugula", "illogical"), d.join("arugula"));
        assert!(!d.join("arugula").exists() && !d.join("illogical").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_left_after_the_old_one_went_is_cleared() {
        let d = scratch("dangling");
        std::os::unix::fs::symlink("illogical", d.join("arugula")).unwrap();
        assert_eq!(named_in(&d, "arugula", "illogical"), d.join("arugula"));
        assert!(std::fs::symlink_metadata(d.join("arugula")).is_err(), "the dangling link stays");
        std::fs::create_dir_all(d.join("arugula")).unwrap();
        // Someone's own link elsewhere is theirs: used, not touched.
        let mine = scratch("mine");
        let d2 = scratch("linked");
        std::os::unix::fs::symlink(&mine, d2.join("arugula")).unwrap();
        assert_eq!(named_in(&d2, "arugula", "illogical"), d2.join("arugula"));
        assert!(d2.join("arugula").exists());
        for x in [d, mine, d2] {
            std::fs::remove_dir_all(x).unwrap();
        }
    }
}
