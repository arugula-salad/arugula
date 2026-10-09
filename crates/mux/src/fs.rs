//! Files on a host (M7): the types behind `list`, `stat`, `read` and `watch`,
//! read-only: [`Scope`] (this host's files), [`Machine`] (a machine's, through
//! its provider), [`Target`] (either) and [`FsError`]. The routes and handlers
//! that serve them are the daemon's `fs.rs`.
//!
//! **Where a call goes.** Without `pane`/`machine`, or for a pane on this
//! host, this daemon answers from its own filesystem. Every host with a
//! daemon (another peer, a resident sandbox, a dial-out host) answers the
//! same way for itself; the client reaches it like any other API call
//! (straight, or through `/h/NAME` and `/tunnel/NAME`). A block on a
//! machine with no daemon of ours (a VM pane or tab, a shell on a sandbox)
//! is answered through the machine's provider ([`Provider::fs_list`]).
//!
//! **Scoped to the host's user** means:
//!
//! - On a daemon's host, the daemon reads as the user it runs as, so the
//!   operating system's permissions are the limit: nothing the user
//!   couldn't `cat` themselves. On top of that it refuses `/proc`, `/sys`
//!   and `/dev` (other processes' environments and memory, kernel knobs,
//!   devices that block or never end), its own state directory, and the
//!   secrets it knows of (the provider token, agent credentials). Paths are
//!   resolved (symlinks followed) before that check, and an opened file or
//!   directory is checked again by what the kernel says it is
//!   (`/proc/self/fd`), so a link, or one swapped in between the check and
//!   the open, can't reach a refused place. Only regular files are read.
//! - On a machine, the provider's agent reads as the sandbox's root, but the
//!   sandbox is the user's own (nobody else's files are in it). The same
//!   places are refused by path, and a read refuses any path with a symlink
//!   in it (the agent can't say where one points), so a link planted by
//!   code in the sandbox can't lead a read into `/proc`.
//!
//! **Caps.** A listing holds at most [`LIST_MAX`] entries, a read at most
//! [`READ_MAX`] bytes (read more in ranges), and a watch polls (every
//! second here, every 3s on a machine, which keeps it awake while
//! watched) until the caller hangs up.

use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use arugula_proto::fs::{FsEntry, FsKind, FsList, LIST_MAX, READ_MAX};

use crate::provider::Provider;

/// Why a call failed, by what the caller can do about it.
#[derive(Debug)]
pub enum FsError {
    NotFound(String),
    /// Refused: by us, or by the filesystem's permissions.
    Denied(String),
    /// Not something this call takes (a directory to `read`, say).
    Bad(String),
    /// The machine or its provider can't be asked.
    Unavailable(String),
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsError::NotFound(s) | FsError::Denied(s) | FsError::Bad(s) | FsError::Unavailable(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for FsError {}

impl FsError {
    fn io(path: &Path, e: std::io::Error) -> Self {
        let p = path.display();
        match e.kind() {
            std::io::ErrorKind::NotFound => FsError::NotFound(format!("{p}: no such file or directory")),
            std::io::ErrorKind::PermissionDenied => FsError::Denied(format!("{p}: permission denied")),
            std::io::ErrorKind::NotADirectory => FsError::Bad(format!("{p}: not a directory")),
            _ => FsError::Bad(format!("{p}: {e}")),
        }
    }

    /// A provider's error: ours if it said why, else "unavailable".
    fn from_any(e: anyhow::Error) -> Self {
        match e.downcast::<FsError>() {
            Ok(f) => f,
            Err(e) => FsError::Unavailable(format!("{e:#}")),
        }
    }
}

/// What the calls in this file return.
pub type Res<T> = Result<T, FsError>;

/// Kernel and device trees, refused everywhere.
const SYSTEM: &[&str] = &["/proc", "/sys", "/dev"];

fn system(p: &Path) -> bool {
    SYSTEM.iter().any(|s| p.starts_with(s))
}

/// This host's files, as its user, minus what's refused.
#[derive(Debug, Clone)]
pub struct Scope {
    home: PathBuf,
    /// The daemon's state directory and the secret files it knows of.
    private: Vec<PathBuf>,
}

impl Scope {
    pub fn new(home: PathBuf, private: Vec<PathBuf>) -> Self {
        // Compare like with like: resolved paths.
        let private = private.into_iter().map(|p| p.canonicalize().unwrap_or(p)).collect();
        Self { home, private }
    }

    fn refused(&self, p: &Path) -> Option<FsError> {
        if system(p) {
            return Some(FsError::Denied(format!("{}: /proc, /sys and /dev aren't served", p.display())));
        }
        self.private
            .iter()
            .any(|x| p.starts_with(x))
            .then(|| FsError::Denied(format!("{}: Arugula's own state and secrets aren't served", p.display())))
    }

    /// `~`, `~/x` and relative paths are the home directory's.
    fn absolute(&self, path: &str) -> PathBuf {
        match path.strip_prefix('~') {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => self.home.join(rest.trim_start_matches('/')),
            _ => self.home.join(path),
        }
    }

    /// Symlinks resolved, and allowed.
    pub fn resolve(&self, path: &str) -> Res<PathBuf> {
        let abs = self.absolute(path);
        if let Some(e) = self.refused(&lexical(&abs)) {
            return Err(e);
        }
        let real = abs.canonicalize().map_err(|e| FsError::io(&abs, e))?;
        match self.refused(&real) {
            Some(e) => Err(e),
            None => Ok(real),
        }
    }

    /// Open what `real` names without following a last-moment symlink, and
    /// check that what was opened is still that, allowed.
    #[cfg(unix)]
    fn open(&self, real: &Path, dir: bool) -> Res<File> {
        use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
        let mut flags = nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_NONBLOCK;
        if dir {
            flags |= nix::fcntl::OFlag::O_DIRECTORY;
        }
        let f =
            OpenOptions::new().read(true).custom_flags(flags.bits()).open(real).map_err(|e| FsError::io(real, e))?;
        let opened = crate::procinfo::fd_path(f.as_raw_fd()).map_err(|e| FsError::io(real, e))?;
        if opened != real {
            return Err(FsError::Denied(format!("{}: changed while it was opened", real.display())));
        }
        Ok(f)
    }

    /// Windows: open the link itself rather than its target (a reparse
    /// point), directories included, then check the path still resolves to
    /// `real`. (The handle's own final path is M56's, #219.)
    #[cfg(windows)]
    fn open(&self, real: &Path, dir: bool) -> Res<File> {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        let mut flags = FILE_FLAG_OPEN_REPARSE_POINT;
        if dir {
            flags |= FILE_FLAG_BACKUP_SEMANTICS;
        }
        let f = OpenOptions::new().read(true).custom_flags(flags).open(real).map_err(|e| FsError::io(real, e))?;
        if f.metadata().map_err(|e| FsError::io(real, e))?.is_dir() != dir
            || real.canonicalize().map_err(|e| FsError::io(real, e))? != real
        {
            return Err(FsError::Denied(format!("{}: changed while it was opened", real.display())));
        }
        Ok(f)
    }

    pub fn list(&self, path: &str, dirs_only: bool) -> Res<FsList> {
        let real = self.resolve(path)?;
        let dir = self.open(&real, true)?;
        let entries = crate::procinfo::list_dir(&dir, &real).map_err(|e| FsError::io(&real, e))?;
        let mut out = Vec::new();
        let mut truncated = false;
        for (name, meta) in entries {
            let name = name.to_string_lossy().into_owned();
            let entry = entry_of(&real.join(&name), &name, &meta);
            if dirs_only && !entry.is_dir() {
                continue;
            }
            if out.len() == LIST_MAX {
                truncated = true;
                break;
            }
            out.push(entry);
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        let parent = real.parent().map(|p| p.display().to_string());
        Ok(FsList { path: real.display().to_string(), parent, entries: out, truncated })
    }

    pub fn stat(&self, path: &str) -> Res<FsEntry> {
        let real = self.resolve(path)?;
        let meta = std::fs::symlink_metadata(&real).map_err(|e| FsError::io(&real, e))?;
        let name = real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "/".into());
        Ok(entry_of(&real, &name, &meta))
    }

    /// Up to `len` bytes (at most [`READ_MAX`]) from `offset`, and the size.
    pub fn read(&self, path: &str, offset: u64, len: u64) -> Res<(Vec<u8>, u64)> {
        let real = self.resolve(path)?;
        let mut f = self.open(&real, false)?;
        let meta = f.metadata().map_err(|e| FsError::io(&real, e))?;
        if !meta.is_file() {
            return Err(FsError::Bad(format!("{}: not a file", real.display())));
        }
        let size = meta.len();
        f.seek(SeekFrom::Start(offset.min(size))).map_err(|e| FsError::io(&real, e))?;
        let mut out = Vec::new();
        f.take(len.min(READ_MAX)).read_to_end(&mut out).map_err(|e| FsError::io(&real, e))?;
        Ok((out, size))
    }

    /// A directory's listing, or a one-entry one for a file (for watching).
    pub fn snapshot(&self, path: &str) -> Res<FsList> {
        let e = self.stat(path)?;
        if e.kind == FsKind::Directory {
            return self.list(path, false);
        }
        let parent = Path::new(&e.path).parent().map(|p| p.display().to_string());
        Ok(FsList { path: e.path.clone(), parent, entries: vec![e], truncated: false })
    }
}

fn entry_of(path: &Path, name: &str, meta: &std::fs::Metadata) -> FsEntry {
    let kind = kind_of(meta.file_type());
    let target =
        (kind == FsKind::Symlink).then(|| std::fs::metadata(path).ok().map(|m| kind_of(m.file_type()))).flatten();
    FsEntry {
        name: name.to_owned(),
        path: path.display().to_string(),
        kind,
        size: meta.len(),
        mode: crate::perm::mode(meta) & 0o7777,
        mtime_ms: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as u64),
        target,
    }
}

fn kind_of(t: std::fs::FileType) -> FsKind {
    if t.is_symlink() {
        FsKind::Symlink
    } else if t.is_dir() {
        FsKind::Directory
    } else if t.is_file() {
        FsKind::File
    } else {
        FsKind::Other
    }
}

/// `.` and `..` worked out without asking the filesystem.
pub fn lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::from("/");
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(n) => out.push(n),
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------- machines

/// A machine's files, through its provider.
#[derive(Clone)]
pub struct Machine {
    provider: Arc<dyn Provider>,
    sprite: String,
}

impl Machine {
    /// Refused by path. A relative path (from the sandbox user's home) is
    /// checked once the provider has said where it is.
    pub fn refused(path: &str) -> Res<()> {
        let p = Path::new(path);
        if p.is_absolute() && system(&lexical(p)) {
            return Err(FsError::Denied(format!("{path}: /proc, /sys and /dev aren't served")));
        }
        Ok(())
    }

    async fn raw(&self, path: &str) -> Res<FsList> {
        Self::refused(path)?;
        let list = self.provider.fs_list(&self.sprite, path).await.map_err(FsError::from_any)?;
        Self::refused(&list.path)?;
        Ok(list)
    }

    /// The list is the thing itself, not a directory's entries.
    fn is_itself(list: &FsList) -> Option<&FsEntry> {
        match list.entries.as_slice() {
            [e] if e.path == list.path && e.kind != FsKind::Directory => Some(e),
            _ => None,
        }
    }

    pub async fn list(&self, path: &str, dirs_only: bool) -> Res<FsList> {
        let mut list = self.raw(path).await?;
        match Self::is_itself(&list) {
            Some(e) if e.kind == FsKind::Symlink => {
                return Err(FsError::Bad(format!("{}: a symlink (not followed on a machine)", list.path)));
            }
            Some(_) => return Err(FsError::Bad(format!("{}: not a directory", list.path))),
            None => {}
        }
        if dirs_only {
            list.entries.retain(FsEntry::is_dir);
        }
        Ok(list)
    }

    pub async fn stat(&self, path: &str) -> Res<FsEntry> {
        let list = self.raw(path).await?;
        if let Some(e) = Self::is_itself(&list) {
            return Ok(e.clone());
        }
        // A directory: its entry is in its parent's listing.
        let here = Path::new(&list.path);
        let name = here.file_name().map(|n| n.to_string_lossy().into_owned());
        if let (Some(parent), Some(name)) = (here.parent(), name.clone())
            && let Ok(up) = self.raw(&parent.display().to_string()).await
            && let Some(e) = up.entries.into_iter().find(|e| e.name == name)
        {
            return Ok(e);
        }
        Ok(FsEntry {
            name: name.unwrap_or_else(|| "/".into()),
            path: list.path,
            kind: FsKind::Directory,
            size: 0,
            mode: 0,
            mtime_ms: 0,
            target: None,
        })
    }

    pub async fn read(&self, path: &str, offset: u64, len: u64) -> Res<(Vec<u8>, u64)> {
        let list = self.raw(path).await?;
        let abs = match Self::is_itself(&list) {
            Some(e) if e.kind == FsKind::File => e.path.clone(),
            Some(e) if e.kind == FsKind::Symlink => {
                return Err(FsError::Denied(format!("{}: a symlink (not followed on a machine)", e.path)));
            }
            _ => return Err(FsError::Bad(format!("{}: not a file", list.path))),
        };
        // No directory on the way may be a link either.
        let mut prefix = PathBuf::from("/");
        let parts: Vec<_> = Path::new(&abs).components().skip(1).collect();
        for c in &parts[..parts.len().saturating_sub(1)] {
            prefix.push(c);
            let up = self.raw(&prefix.display().to_string()).await?;
            if Self::is_itself(&up).is_some() {
                return Err(FsError::Denied(format!("{}: a symlink on the way (not followed on a machine)", up.path)));
            }
        }
        self.provider.fs_read(&self.sprite, &abs, offset, len.min(READ_MAX)).await.map_err(FsError::from_any)
    }

    pub async fn snapshot(&self, path: &str) -> Res<FsList> {
        let list = self.raw(path).await?;
        Ok(list)
    }
}

/// Where files are read: this host, or a machine through its provider.
/// M11's file and diff blocks keep one.
#[derive(Clone)]
pub enum Target {
    Local(Arc<Scope>),
    Machine(Machine),
}

impl Target {
    /// A machine's, through `provider`.
    pub fn machine(provider: Arc<dyn Provider>, sprite: String) -> Self {
        Target::Machine(Machine { provider, sprite })
    }

    /// Up to `len` bytes (at most [`READ_MAX`]) from `offset`, and the size.
    pub async fn read(&self, path: &str, offset: u64, len: u64) -> Res<(Vec<u8>, u64)> {
        let len = len.min(READ_MAX);
        match self {
            Target::Local(s) => {
                let (s, path) = (s.clone(), path.to_owned());
                blocking(move || s.read(&path, offset, len)).await
            }
            Target::Machine(m) => m.read(path, offset, len).await,
        }
    }

    pub async fn stat(&self, path: &str) -> Res<FsEntry> {
        match self {
            Target::Local(s) => {
                let (s, path) = (s.clone(), path.to_owned());
                blocking(move || s.stat(&path)).await
            }
            Target::Machine(m) => m.stat(path).await,
        }
    }

    pub async fn list(self, path: String, dirs_only: bool) -> Res<FsList> {
        match self {
            Target::Local(s) => blocking(move || s.list(&path, dirs_only)).await,
            Target::Machine(m) => m.list(&path, dirs_only).await,
        }
    }

    pub async fn snapshot(&self, path: String) -> Res<FsList> {
        match self {
            Target::Local(s) => {
                let s = s.clone();
                blocking(move || s.snapshot(&path)).await
            }
            Target::Machine(m) => m.snapshot(&path).await,
        }
    }
}

pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| FsError::Unavailable(e.to_string()))?
}

/// A command line typed again: end of line, erase it, the command, Enter.
pub fn rerun_line(command: &str) -> Option<String> {
    let c = command.trim();
    if c.is_empty() || c.chars().any(|ch| ch.is_control() && ch != '\t') {
        return None;
    }
    Some(format!("\x05\x15{c}\r"))
}

// Unix: they make symlinks, which Windows only allows in developer mode.
#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ilg-fs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    #[test]
    fn lists_reads_and_stats_with_caps() {
        let home = tmp("home");
        std::fs::create_dir(home.join("src")).unwrap();
        std::fs::write(home.join("a.txt"), "hello world").unwrap();
        symlink(home.join("src"), home.join("link")).unwrap();
        let s = Scope::new(home.clone(), vec![]);
        let l = s.list("~", false).unwrap();
        let names: Vec<_> = l.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a.txt", "link", "src"]);
        assert_eq!(l.path, home.display().to_string());
        let link = &l.entries[1];
        assert_eq!((link.kind, link.target), (FsKind::Symlink, Some(FsKind::Directory)));
        let dirs = s.list("", true).unwrap();
        assert_eq!(dirs.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["link", "src"]);
        // Through the link: the real directory.
        assert_eq!(s.list("~/link", false).unwrap().path, home.join("src").display().to_string());
        let (bytes, size) = s.read("a.txt", 6, 100).unwrap();
        assert_eq!((bytes.as_slice(), size), (&b"world"[..], 11));
        assert!(matches!(s.read("src", 0, 10), Err(FsError::Bad(_))));
        assert!(matches!(s.list("a.txt", false), Err(FsError::Bad(_))));
        assert!(matches!(s.stat("nope"), Err(FsError::NotFound(_))));
        assert_eq!(s.stat("~/a.txt").unwrap().size, 11);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn refuses_proc_secrets_and_links_into_them() {
        let home = tmp("refuse");
        let state = home.join("state");
        std::fs::create_dir(&state).unwrap();
        std::fs::write(state.join("key"), "secret").unwrap();
        symlink("/proc/self/environ", home.join("env")).unwrap();
        symlink("/proc/self", home.join("me")).unwrap();
        symlink(&state, home.join("st")).unwrap();
        let s = Scope::new(home.clone(), vec![state.clone()]);
        // macOS has no /proc (or /sys) for the links to lead into.
        let (reads, lists): (&[&str], &[&str]) = if cfg!(any(target_os = "linux", target_os = "android")) {
            (
                &["/proc/self/environ", "/proc/1/environ", "~/env", "~/me/environ", "/sys/kernel", "/dev/zero"],
                &["/proc", "~/me", "~/st", "~/state", "/tmp/../proc/self"],
            )
        } else {
            (&["/proc/self/environ", "/dev/zero"], &["~/st", "~/state"])
        };
        for p in reads {
            assert!(matches!(s.read(p, 0, 10), Err(FsError::Denied(_))), "{p}");
        }
        for p in lists {
            assert!(matches!(s.list(p, false), Err(FsError::Denied(_))), "{p}");
        }
        assert!(matches!(s.read("~/st/key", 0, 10), Err(FsError::Denied(_))));
        // The link itself is listed (by name), but not followed.
        assert!(s.list("~", false).unwrap().entries.iter().any(|e| e.name == "env"));
        std::fs::remove_dir_all(home).unwrap();
    }
}
