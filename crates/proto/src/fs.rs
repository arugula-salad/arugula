//! The filesystem methods every host answers (M7): list, stat, read and
//! watch, read-only. A daemon answers for its own host; a block on a
//! machine (a VM pane, a shell on a sandbox) is answered through the
//! machine's provider. The picker and M11's file and diff blocks use them.
//!
//! | method | path | query | answer |
//! |---|---|---|---|
//! | GET | `/api/fs/list` | `path=`, `pane=N` or `machine=N`, `dirs=1` | `FsList` |
//! | GET | `/api/fs/stat` | `path=`, `pane=`/`machine=` | `FsEntry` |
//! | GET | `/api/fs/read` | `path=`, `pane=`/`machine=`, `offset=`, `len=` | bytes; `x-arugula-size`: the file's size |
//! | GET | `/api/fs/watch` | `path=`, `pane=`/`machine=` | NDJSON `FsChange`s, until the caller hangs up |
//! | GET | `/api/fs/recent` | `pane=`/`machine=` | `[String]`: directories used lately on that host, newest first |
//! | POST | `/api/panes/N/cd` | `CdRequest` | `{}`; refused unless the shell is idle at its prompt |
//!
//! Without `pane` or `machine` it's this daemon's host. `path` may start
//! with `~` (the host's user's home) and is relative to it otherwise;
//! answers carry absolute paths.

use serde::{Deserialize, Serialize};

use crate::{MachineId, PaneId};

/// The most entries one listing returns (`truncated` says there were more).
pub const LIST_MAX: usize = 5000;
/// The most bytes one read returns; read a longer file in ranges.
pub const READ_MAX: u64 = 1 << 20;
/// What a read returns without `len`.
pub const READ_DEFAULT: u64 = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FsKind {
    File,
    Directory,
    Symlink,
    /// A socket, device or pipe: listed, never read.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsEntry {
    #[serde(default)]
    pub name: String,
    /// Absolute.
    #[serde(default)]
    pub path: String,
    #[serde(rename = "type")]
    pub kind: FsKind,
    #[serde(default)]
    pub size: u64,
    /// Permission bits (`0o644`).
    #[serde(default)]
    pub mode: u32,
    pub mtime_ms: u64,
    /// For a symlink: what it points at, when known (`directory` means the
    /// picker can go into it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<FsKind>,
}

impl FsEntry {
    /// A directory, or a link to one.
    pub fn is_dir(&self) -> bool {
        self.kind == FsKind::Directory || self.target == Some(FsKind::Directory)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsList {
    /// The directory, absolute (symlinks resolved, on a daemon's host).
    pub path: String,
    /// Its parent, if it has one.
    pub parent: Option<String>,
    /// By name.
    pub entries: Vec<FsEntry>,
    /// There were more than [`LIST_MAX`].
    #[serde(default)]
    pub truncated: bool,
}

/// What changed in a watched directory (or file).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FsChange {
    /// First: what's there now.
    Listing {
        list: FsList,
    },
    Created {
        entry: FsEntry,
    },
    Modified {
        entry: FsEntry,
    },
    Removed {
        path: String,
    },
    /// It can't be read any more (deleted, say); the watch ends.
    Error {
        error: String,
    },
}

/// `POST /api/panes/N/cd`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CdRequest {
    pub path: String,
}

/// The query of `list`, `stat`, `read`, `watch` and `recent`. As a request
/// its fields go out in this order (the host first), a `None` left out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsQuery {
    /// On the host this block runs on.
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// On this machine.
    #[serde(default)]
    pub machine: Option<MachineId>,
    #[serde(default)]
    pub path: Option<String>,
    /// Only directories (and links to them): `1`.
    #[serde(default)]
    pub dirs: Option<u8>,
    #[serde(default)]
    pub offset: Option<u64>,
    #[serde(default)]
    pub len: Option<u64>,
}
