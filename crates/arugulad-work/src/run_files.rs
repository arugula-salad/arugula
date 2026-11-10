//! What a run left, read by this daemon (#1985 review; run logs' artifacts,
//! M213): a file only when it is a regular file with one name that the run
//! user owns, not a link, at most a given size, streamed.
//!
//! On a box this daemon runs as its own user (`user`), and the run's home
//! is the run user's, 0700: this daemon can't open it. So the run user hands
//! the bytes over (`run_file_read.sh`, run with `sudo -n -u <run user>`),
//! checking the file first. With no run user (tests), this daemon opens the
//! file itself, without following a link, and checks the same.

use std::path::Path;

use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite};

use crate::RunAs;

/// What the run user runs to hand a file or a directory's names over.
pub const READ_SCRIPT: &str = include_str!("run_file_read.sh");

/// Why a run's file wasn't read.
#[derive(Debug, PartialEq, Eq)]
pub enum NotRead {
    /// There's no such file.
    Missing,
    /// A link, not a regular file, more than one name, or not the run
    /// user's own.
    NotOwn,
    /// Larger than it may be.
    TooLarge,
    /// The read itself failed.
    Failed(String),
}

impl std::fmt::Display for NotRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotRead::Missing => write!(f, "isn't there"),
            NotRead::NotOwn => write!(f, "is a link, not a regular file of the run's own, or has another name"),
            NotRead::TooLarge => write!(f, "is too large"),
            NotRead::Failed(why) => write!(f, "didn't read ({why})"),
        }
    }
}

/// The run user's file `path`, at most `max` bytes, copied into `to`: how
/// many bytes.
pub async fn copy_own<W: AsyncWrite + Unpin>(
    as_: Option<&RunAs>,
    path: &Path,
    max: u64,
    to: &mut W,
) -> Result<u64, NotRead> {
    match as_ {
        Some(a) => copy_as(a, path, max, to).await,
        None => copy_here(path, max, to).await,
    }
}

/// The names in the run user's directory `dir` (none when it isn't there).
pub async fn list_own(as_: Option<&RunAs>, dir: &Path) -> Vec<String> {
    match as_ {
        Some(a) => {
            let out = tokio::process::Command::new("sudo")
                .args(["-n", "-u", &a.user, "--", "/bin/sh", "-c", READ_SCRIPT, "sh", "-l"])
                .arg(dir)
                .stdin(std::process::Stdio::null())
                .output()
                .await;
            match out {
                Ok(o) if o.status.success() => {
                    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.is_empty()).map(str::to_owned).collect()
                }
                _ => vec![],
            }
        }
        None => match std::fs::read_dir(dir) {
            Ok(entries) => entries.flatten().filter_map(|e| e.file_name().into_string().ok()).collect(),
            Err(_) => vec![],
        },
    }
}

/// Copy at most `max + 1` bytes of `from` into `to`: how many.
async fn capped<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(from: R, max: u64, to: &mut W) -> Result<u64, NotRead> {
    let mut from = from.take(max + 1);
    tokio::io::copy(&mut from, to).await.map_err(|e| NotRead::Failed(e.to_string()))
}

async fn copy_as<W: AsyncWrite + Unpin>(a: &RunAs, path: &Path, max: u64, to: &mut W) -> Result<u64, NotRead> {
    let mut child = tokio::process::Command::new("sudo")
        .args(["-n", "-u", &a.user, "--", "/bin/sh", "-c", READ_SCRIPT, "sh"])
        .arg(path)
        .arg(max.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| NotRead::Failed(format!("sudo: {e}")))?;
    let stdout = child.stdout.take().ok_or_else(|| NotRead::Failed("no output".into()))?;
    let copied = capped(stdout, max, to).await;
    let out = child.wait_with_output().await.map_err(|e| NotRead::Failed(e.to_string()))?;
    match out.status.code() {
        Some(0) => {
            let n = copied?;
            if n > max { Err(NotRead::TooLarge) } else { Ok(n) }
        }
        Some(2) => Err(NotRead::Missing),
        Some(3) => Err(NotRead::NotOwn),
        Some(4) => Err(NotRead::TooLarge),
        _ => Err(NotRead::Failed(String::from_utf8_lossy(&out.stderr).trim().to_owned())),
    }
}

#[cfg(unix)]
async fn copy_here<W: AsyncWrite + Unpin>(path: &Path, max: u64, to: &mut W) -> Result<u64, NotRead> {
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
    let f = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(NotRead::Missing),
        // A link (ELOOP), or anything else that won't open as a file.
        Err(_) => return Err(NotRead::NotOwn),
    };
    let m = f.metadata().map_err(|e| NotRead::Failed(e.to_string()))?;
    if !m.is_file() || m.uid() != nix::unistd::getuid().as_raw() || m.nlink() != 1 {
        return Err(NotRead::NotOwn);
    }
    if m.len() > max {
        return Err(NotRead::TooLarge);
    }
    let n = capped(tokio::fs::File::from_std(f), max, to).await?;
    if n > max { Err(NotRead::TooLarge) } else { Ok(n) }
}

#[cfg(not(unix))]
async fn copy_here<W: AsyncWrite + Unpin>(_: &Path, _: u64, _: &mut W) -> Result<u64, NotRead> {
    Err(NotRead::Failed("a run's files are read on a Linux box only".into()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// With no run user: a regular file of this user's, whole; not a link,
    /// a second name, a directory or one too large.
    #[tokio::test]
    async fn only_a_runs_own_regular_file_is_read() {
        let dir = std::env::temp_dir().join(format!("arugula-run-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("ok"), b"hello").unwrap();
        std::os::unix::fs::symlink(dir.join("ok"), dir.join("link")).unwrap();
        std::fs::write(dir.join("two"), b"x").unwrap();
        std::fs::hard_link(dir.join("two"), dir.join("two-again")).unwrap();
        let read = |name: &str, max: u64| {
            let p = dir.join(name);
            async move {
                let mut out = Vec::new();
                copy_own(None, &p, max, &mut out).await.map(|_| out)
            }
        };
        assert_eq!(read("ok", 5).await.unwrap(), b"hello");
        assert_eq!(read("ok", 4).await.unwrap_err(), NotRead::TooLarge);
        assert_eq!(read("link", 5).await.unwrap_err(), NotRead::NotOwn);
        assert_eq!(read("two", 5).await.unwrap_err(), NotRead::NotOwn);
        assert_eq!(read("sub", 5).await.unwrap_err(), NotRead::NotOwn);
        assert_eq!(read("none", 5).await.unwrap_err(), NotRead::Missing);
        let mut names = list_own(None, &dir).await;
        names.sort();
        assert_eq!(names, ["link", "ok", "sub", "two", "two-again"]);
    }

    /// The run user's script, here as this user (`sh`, as `sudo -u` runs
    /// it): the same answers, by its exits.
    #[tokio::test]
    async fn the_run_users_script_answers_the_same() {
        let dir = std::env::temp_dir().join(format!("arugula-run-files-sh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ok"), b"hello").unwrap();
        std::os::unix::fs::symlink(dir.join("ok"), dir.join("link")).unwrap();
        // GNU stat (a box's): macOS's stat takes no -c, so this runs on Linux.
        if !std::process::Command::new("stat").args(["-c", "%h", "/"]).output().is_ok_and(|o| o.status.success()) {
            eprintln!("skipped: no GNU stat here");
            return;
        }
        let run = |args: &[&str]| {
            std::process::Command::new("/bin/sh").arg("-c").arg(READ_SCRIPT).arg("sh").args(args).output().unwrap()
        };
        let p = |n: &str| dir.join(n).to_string_lossy().into_owned();
        let o = run(&[&p("ok"), "5"]);
        assert_eq!((o.status.code(), o.stdout.as_slice()), (Some(0), b"hello".as_slice()));
        assert_eq!(run(&[&p("ok"), "4"]).status.code(), Some(4));
        assert_eq!(run(&[&p("link"), "5"]).status.code(), Some(3));
        assert_eq!(run(&[&p("none"), "5"]).status.code(), Some(2));
    }
}
