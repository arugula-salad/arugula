//! Agents' build folders (Arugula #1410). AGENTS.md puts an agent's cargo
//! target dir at `~/.cache/arugula-<worktree>-target`, outside its
//! worktree; a hosted session user's is `~/.cache/arugula/cargo-target`, or
//! the `arugula-ses-<id>-target` an agent made following AGENTS.md. A few
//! GB each, and nothing removed them:
//!
//! - closing a worktree removes its own (`arugula_core::worktree::CLOSE`);
//! - a hosted session's end removes its user's ([`drop_user`]), keeping
//!   sccache and the rest of `~/.cache`;
//! - [`keep_sweeping`] clears what's left: this daemon's user's, and a
//!   hosted box's session users' that no session holds.
//!
//! A sweep removes a dir only when no live worktree owns it, nothing in it
//! was touched for [`KEEP`], and no build holds its `.cargo-lock`.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use arugula_mux::mux::Api;
use tracing::{info, warn};

use crate::agent::hosted::AsUser;

/// A dir untouched this long, and owned by no live worktree, is leftovers.
pub const KEEP: Duration = Duration::from_secs(3 * 24 * 3600);

const EVERY: Duration = Duration::from_secs(6 * 3600);
/// The first sweep, after the daemon has settled.
const FIRST: Duration = Duration::from_secs(60);

/// The worktree a `~/.cache` entry named `arugula-<name>-target` belongs
/// to: `<name>`, a plain one. `/`, `.` and `..` are never a worktree's
/// name, so nothing built from one reaches outside the cache dir.
pub fn owner(entry: &str) -> Option<&str> {
    let name = entry.strip_prefix("arugula-")?.strip_suffix("-target")?;
    (!name.is_empty() && name != "." && name != ".." && !name.contains('/')).then_some(name)
}

/// The newest mtime among `dir`, its profile dirs (`debug`, `release`, …)
/// and their `deps` and `.fingerprint` dirs, and `<profile>/.cargo-lock`:
/// what a build touches. Not what's inside them, which is thousands of
/// files.
fn touched(dir: &Path) -> Option<SystemTime> {
    let mut newest = fs::symlink_metadata(dir).and_then(|m| m.modified()).ok()?;
    let mut see = |p: &Path| {
        if let Ok(t) = fs::symlink_metadata(p).and_then(|m| m.modified()) {
            newest = newest.max(t);
        }
    };
    for profile in fs::read_dir(dir).ok()?.flatten() {
        if !profile.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let p = profile.path();
        see(&p);
        for sub in ["deps", ".fingerprint", ".cargo-lock"] {
            see(&p.join(sub));
        }
    }
    Some(newest)
}

/// Whether a build holds one of `dir`'s `<profile>/.cargo-lock`s: cargo
/// takes an exclusive `flock` on it while building. Taking it without
/// waiting and failing is the answer; one taken is let go again.
fn in_use(dir: &Path) -> bool {
    let Ok(profiles) = fs::read_dir(dir) else { return false };
    profiles.flatten().any(|profile| {
        let Ok(f) = fs::File::options().read(true).open(profile.path().join(".cargo-lock")) else { return false };
        matches!(f.try_lock(), Err(fs::TryLockError::WouldBlock))
    })
}

/// `p`'s size on disk as a sum of file lengths, links not followed.
fn size(p: &Path) -> u64 {
    let Ok(m) = fs::symlink_metadata(p) else { return 0 };
    if !m.is_dir() {
        return m.len();
    }
    fs::read_dir(p).map(|d| d.flatten().map(|e| size(&e.path())).sum()).unwrap_or(0)
}

/// What a sweep took out: each dir and its size.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Swept {
    pub removed: Vec<(PathBuf, u64)>,
}

impl Swept {
    pub fn bytes(&self) -> u64 {
        self.removed.iter().map(|(_, n)| n).sum()
    }
}

/// The `arugula-*-target` dirs in `cache` a sweep would remove at `now`:
/// not a live worktree's (`live`: their names), untouched for `keep`, and
/// not held by a build. With `remove`, removes them (a link is removed, not
/// what it points at). Only a direct child of `cache` is ever touched.
pub fn sweep(cache: &Path, live: &BTreeSet<String>, now: SystemTime, keep: Duration, remove: bool) -> Swept {
    let mut out = Swept::default();
    let Ok(entries) = fs::read_dir(cache) else { return out };
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(owner) = owner(name) else { continue };
        let path = cache.join(name);
        debug_assert_eq!(path.parent(), Some(cache), "{name}");
        if live.contains(owner) {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        let link = meta.file_type().is_symlink();
        if !link && !meta.is_dir() {
            continue;
        }
        let old = (if link { meta.modified().ok() } else { touched(&path) })
            .is_some_and(|t| now.duration_since(t).unwrap_or_default() >= keep);
        if !old || (!link && in_use(&path)) {
            continue;
        }
        let n = if link { 0 } else { size(&path) };
        if remove {
            let gone = if link { fs::remove_file(&path) } else { fs::remove_dir_all(&path) };
            if let Err(e) = gone {
                warn!(dir = %path.display(), error = %e, "can't remove an old build folder");
                continue;
            }
        }
        out.removed.push((path, n));
    }
    out
}

/// What session user `u`'s home drops: its `~/.cache/arugula/cargo-target`
/// and each `~/.cache/arugula-*-target`, as the user (who owns them).
/// `old`: only the ones untouched for [`KEEP`] and held by no build, for
/// the sweep of a slot no session holds; else all, for a session's end.
/// Not sccache, nor anything else in `~/.cache`.
const DROP: &str = r#"set -u
home=$1; days=$2
[ -L "$home/.cache" ] && exit 0
cd -- "$home/.cache" 2>/dev/null || exit 0
for d in arugula/cargo-target arugula-*-target; do
  [ -e "$d" ] || [ -L "$d" ] || continue
  if [ "$days" != all ] && [ ! -L "$d" ]; then
    [ -z "$(find "$d" -maxdepth 2 -mtime -"$days" -print -quit 2>/dev/null)" ] || continue
    busy=
    for f in "$d"/*/.cargo-lock; do
      [ -e "$f" ] && command -v flock >/dev/null 2>&1 && { flock -n "$f" true || busy=1; }
    done
    [ -z "$busy" ] || continue
  fi
  chmod -R u+rwX -- "$d" 2>/dev/null
  rm -rf -- "$d" && printf '%s\n' "$d"
done
"#;

/// [`DROP`] for session user `u`: the dirs it removed. `old` as there.
pub async fn drop_user(u: &AsUser, old: bool) -> Result<Vec<String>, String> {
    let days = if old { (KEEP.as_secs() / 86400).to_string() } else { "all".into() };
    let argv: Vec<String> = ["sh", "-c", DROP, "sh", &u.home, &days].map(str::to_owned).into();
    let out = u
        .command(&[], &argv)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("can't run sh as {}: {e}", u.user))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().chars().take(400).collect());
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).collect())
}

/// The names of the worktrees in each repo `cwds` are in (`git worktree
/// list`): what the daemon knows live. A cwd that isn't in a repo adds none.
fn worktrees_of(cwds: &BTreeSet<String>) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for cwd in cwds {
        let Ok(out) = std::process::Command::new("git")
            .args(["-C", cwd, "worktree", "list", "--porcelain"])
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(std::process::Stdio::null())
            .output()
        else {
            continue;
        };
        for p in String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.strip_prefix("worktree ")) {
            if seen.insert(p.to_owned())
                && let Some(n) = Path::new(p).file_name().and_then(|n| n.to_str())
            {
                names.insert(n.to_owned());
            }
        }
    }
    names
}

/// One sweep: this user's `~/.cache`, and on a hosted box the session users'
/// whose slot no session holds. Logs a line for each dir and a total.
async fn sweep_now(app: &crate::server::App) {
    let cwds: BTreeSet<String> = app
        .mux
        .api(Api::Panes)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| p.info.cwd)
        .chain(std::env::current_dir().ok().map(|d| d.display().to_string()))
        .collect();
    let mut live = crate::task::spawn_blocking(move || worktrees_of(&cwds)).await.unwrap_or_default();
    let dir = app.control.state_dir().to_owned();
    let sessions = crate::work::sessions::all(&dir);
    // A session's worktree is `ses-` and the first eight of its id's digits.
    live.extend(
        sessions.iter().filter(|l| l.ended.is_none()).filter_map(|l| Some(format!("ses-{}", l.session.get(4..12)?))),
    );
    let cache = arugula_mux::sys::home().join(".cache");
    let swept = crate::task::spawn_blocking(move || sweep(&cache, &live, SystemTime::now(), KEEP, true))
        .await
        .unwrap_or_default();
    let total = swept.bytes();
    for (p, n) in &swept.removed {
        info!(dir = %p.display(), bytes = n, "removed an old build folder");
    }
    let mut count = swept.removed.len();
    if let Some(slots) = crate::work::sessions::slots() {
        for u in &slots.users {
            let held = sessions.iter().any(|l| l.ended.is_none() && l.user.user == u.user);
            if held {
                continue;
            }
            match drop_user(u, true).await {
                Ok(gone) => {
                    for d in &gone {
                        info!(user = u.user, dir = d, "removed an old build folder");
                    }
                    count += gone.len();
                }
                Err(e) => warn!(user = u.user, error = e, "can't sweep a session user's build folders"),
            }
        }
    }
    if count > 0 {
        info!(dirs = count, bytes = total, "swept old build folders");
    }
}

/// The sweep, shortly after the daemon starts and every six hours.
pub async fn keep_sweeping(app: std::sync::Arc<crate::server::App>) {
    let mut every = tokio::time::interval_at(tokio::time::Instant::now() + FIRST, EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        every.tick().await;
        sweep_now(&app).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: Duration = Duration::from_secs(86400);

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("arugula-build-dirs-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn set_mtime(p: &Path, t: SystemTime) {
        // A dir opens read-only on Unix; `set_modified` needs only that on
        // Linux, and write access on macOS for a file.
        let f = fs::File::options().read(true).open(p).unwrap();
        f.set_modified(t).unwrap();
    }

    /// `cache/arugula-<name>-target/debug/{deps,.fingerprint,.cargo-lock}`,
    /// all `age` old.
    fn target(cache: &Path, name: &str, age: Duration, now: SystemTime) -> PathBuf {
        let dir = cache.join(format!("arugula-{name}-target"));
        let debug = dir.join("debug");
        fs::create_dir_all(debug.join("deps")).unwrap();
        fs::create_dir_all(debug.join(".fingerprint")).unwrap();
        fs::write(debug.join(".cargo-lock"), "").unwrap();
        fs::write(debug.join("deps/libx.rlib"), vec![0u8; 1000]).unwrap();
        let then = now - age;
        for p in [
            debug.join("deps/libx.rlib"),
            debug.join("deps"),
            debug.join(".fingerprint"),
            debug.join(".cargo-lock"),
            debug.clone(),
            dir.clone(),
        ] {
            set_mtime(&p, then);
        }
        dir
    }

    fn live(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn only_a_plain_worktree_name_owns_a_dir() {
        assert_eq!(owner("arugula-1410-fix-target"), Some("1410-fix"));
        assert_eq!(owner("arugula-ses-eaa1b571-target"), Some("ses-eaa1b571"));
        for no in
            ["arugula--target", "arugula-..-target", "arugula-.-target", "arugula-a/b-target", "arugula", "x-target"]
        {
            assert_eq!(owner(no), None, "{no}");
        }
    }

    /// An old, unowned, idle dir goes; a newer one stays.
    #[test]
    fn a_sweep_removes_old_dirs_and_keeps_new_ones() {
        let cache = temp("age");
        let now = SystemTime::now();
        let old = target(&cache, "old", 4 * DAY, now);
        let fresh = target(&cache, "fresh", 2 * DAY, now);
        let other = cache.join("sccache");
        fs::create_dir_all(&other).unwrap();
        set_mtime(&other, now - 30 * DAY);

        let would = sweep(&cache, &live(&[]), now, KEEP, false);
        assert_eq!(would.removed.len(), 1);
        assert!(old.exists(), "a dry run removes nothing");

        let swept = sweep(&cache, &live(&[]), now, KEEP, true);
        assert_eq!(swept.removed, [(old.clone(), 1000)]);
        assert!(!old.exists());
        assert!(fresh.exists(), "two days old: kept");
        assert!(other.exists(), "not a target dir");
        let _ = fs::remove_dir_all(&cache);
    }

    /// One new file deep in `deps` makes the dir new: the newest of what a
    /// build touches counts, not the top dir's own.
    #[test]
    fn a_dir_a_build_touched_lately_is_kept() {
        let cache = temp("touched");
        let now = SystemTime::now();
        for place in ["debug/deps", "debug/.fingerprint", "debug/.cargo-lock", "debug"] {
            let dir = target(&cache, "busy", 5 * DAY, now);
            set_mtime(&dir.join(place), now - DAY);
            let swept = sweep(&cache, &live(&[]), now, KEEP, true);
            assert!(swept.removed.is_empty() && dir.exists(), "{place}");
            fs::remove_dir_all(&dir).unwrap();
        }
        let _ = fs::remove_dir_all(&cache);
    }

    /// A worktree that's live keeps its dir however old.
    #[test]
    fn a_live_worktrees_dir_stays() {
        let cache = temp("live");
        let now = SystemTime::now();
        let mine = target(&cache, "1410-x", 30 * DAY, now);
        let gone = target(&cache, "1409-y", 30 * DAY, now);
        let swept = sweep(&cache, &live(&["1410-x"]), now, KEEP, true);
        assert_eq!(swept.removed.len(), 1);
        assert!(mine.exists() && !gone.exists());
        let _ = fs::remove_dir_all(&cache);
    }

    /// A build holds `.cargo-lock` with an exclusive flock: its dir is
    /// skipped, however old, and goes once the build ends. The build is a
    /// child process: a lock held here could be inherited for a moment by
    /// another test's fork, and look held after it's let go.
    #[test]
    fn a_dir_a_build_holds_is_skipped() {
        use std::io::{BufRead, BufReader};
        let cache = temp("lock");
        let now = SystemTime::now();
        let dir = target(&cache, "building", 10 * DAY, now);
        let mut build = std::process::Command::new("perl")
            .args([
                "-MFcntl=:flock",
                "-e",
                r#"open(F, "<", shift) or die; flock(F, LOCK_EX) or die; $| = 1; print "held\n"; <STDIN>"#,
            ])
            .arg(dir.join("debug/.cargo-lock"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut said = String::new();
        BufReader::new(build.stdout.take().unwrap()).read_line(&mut said).unwrap();
        assert_eq!(said, "held\n");
        assert!(sweep(&cache, &live(&[]), now, KEEP, true).removed.is_empty());
        assert!(dir.exists());
        drop(build.stdin.take());
        build.wait().unwrap();
        assert_eq!(sweep(&cache, &live(&[]), now, KEEP, true).removed.len(), 1);
        assert!(!dir.exists());
        let _ = fs::remove_dir_all(&cache);
    }

    /// A link in the cache is removed, never what it points at.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_dir_is_unlinked_not_followed() {
        let cache = temp("link");
        let elsewhere = temp("link-elsewhere");
        let now = SystemTime::now();
        fs::write(elsewhere.join("precious"), "x").unwrap();
        let link = cache.join("arugula-evil-target");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
        // A link's own mtime is what its age is; leave it new first.
        let swept = sweep(&cache, &live(&[]), now, KEEP, true);
        assert!(swept.removed.is_empty() && link.exists(), "a new link: kept");
        let later = now + 4 * DAY;
        let swept = sweep(&cache, &live(&[]), later, KEEP, true);
        assert_eq!(swept.removed, [(link.clone(), 0)]);
        assert!(fs::symlink_metadata(&link).is_err(), "the link is gone");
        assert!(elsewhere.join("precious").exists(), "what it pointed at isn't");
        let _ = fs::remove_dir_all(&cache);
        let _ = fs::remove_dir_all(&elsewhere);
    }

    /// A session user's home, as [`DROP`] runs in it.
    fn drop_in(home: &Path, days: &str) -> String {
        let out =
            std::process::Command::new("sh").args(["-c", DROP, "sh", home.to_str().unwrap(), days]).output().unwrap();
        assert!(out.status.success(), "{out:?}");
        let mut got: Vec<_> = String::from_utf8(out.stdout).unwrap().lines().map(str::to_owned).collect();
        got.sort();
        got.join(" ")
    }

    /// A session's end drops its build dirs, either shape, and leaves
    /// sccache and the rest.
    #[test]
    fn a_sessions_end_drops_its_build_dirs_but_not_sccache() {
        let home = temp("end");
        let now = SystemTime::now();
        let cache = home.join(".cache");
        fs::create_dir_all(cache.join("sccache")).unwrap();
        fs::write(cache.join("sccache/x"), "x").unwrap();
        fs::create_dir_all(cache.join("arugula/cargo-target/debug")).unwrap();
        fs::create_dir_all(cache.join("arugula/uploads")).unwrap();
        target(&cache, "ses-eaa1b571", DAY, now);
        let got = drop_in(&home, "all");
        assert_eq!(got, "arugula-ses-eaa1b571-target arugula/cargo-target");
        assert!(cache.join("sccache/x").exists() && cache.join("arugula/uploads").exists());
        assert!(!cache.join("arugula/cargo-target").exists() && !cache.join("arugula-ses-eaa1b571-target").exists());
        let _ = fs::remove_dir_all(&home);
    }

    /// The sweep of a slot no session holds drops only what's old, and not
    /// through a `.cache` that is a link.
    #[cfg(unix)]
    #[test]
    fn a_slots_sweep_drops_only_the_old() {
        let home = temp("slot");
        let now = SystemTime::now();
        let cache = home.join(".cache");
        fs::create_dir_all(&cache).unwrap();
        let old = target(&cache, "ses-old", 5 * DAY, now);
        let fresh = target(&cache, "ses-fresh", DAY, now);
        assert_eq!(drop_in(&home, "3"), "arugula-ses-old-target");
        assert!(!old.exists() && fresh.exists());

        let away = temp("slot-away");
        let held = target(&away, "ses-x", 9 * DAY, SystemTime::now());
        let home2 = temp("slot2");
        std::os::unix::fs::symlink(&away, home2.join(".cache")).unwrap();
        assert_eq!(drop_in(&home2, "all"), "");
        assert!(held.exists(), "a .cache that is a link is left");
        for d in [home, home2, away] {
            let _ = fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn worktrees_are_read_from_git() {
        let dir = temp("git");
        let sh = |s: &str| {
            let out = std::process::Command::new("sh").arg("-c").arg(s).current_dir(&dir).output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        };
        sh("git init -q -b main r && git -C r -c user.name=t -c user.email=t@t commit -q --allow-empty -m one \
            && git -C r worktree add -q -b w1 ../w-one");
        let cwds = BTreeSet::from([dir.join("r").display().to_string(), dir.join("nowhere").display().to_string()]);
        assert_eq!(worktrees_of(&cwds), live(&["r", "w-one"]));
        let _ = fs::remove_dir_all(&dir);
    }
}
