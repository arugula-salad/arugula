//! `pane.nvim_open` (#1731): Open in editor when the Editor preference is
//! nvim. The file goes to the nvim already running in the pane, else to
//! one in another pane of its tab (over nvim's own RPC socket, #1730's
//! `editors::open_in_pane`), else to a new nvim editor block beside it
//! (#1734, `editors::nvim_block`: `nvim +LINE FILE` in the file's
//! directory, on the pane's machine, with arugula.nvim and the page's
//! colors). A build without the editor runs plain nvim there instead
//! (`run --split --join`). HTTP only; the page shows the pane it answers.

use std::path::{Path, PathBuf};

use arugula_proto::{
    BlockType, PaneId, PaneInfo,
    api::{NvimOpenRequest, NvimOpened, NvimWhere, PaneSummary, RunRequest},
    op::ops::PaneNvimOpen,
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError};
use crate::mux::Api;

impl Handle for PaneNvimOpen {
    async fn handle(cx: &Cx<'_>, id: PaneId, req: NvimOpenRequest) -> Result<NvimOpened, OpError> {
        let mux = &cx.app.mux;
        let all = mux.api(Api::Panes).await.unwrap_or_default();
        let me = all.iter().find(|p| p.info.id == id).ok_or(OpError::NoPane(id))?;
        let file = match req.file.as_deref().filter(|f| !f.is_empty()) {
            Some(f) => Some(whole(f, me.info.cwd.as_deref()).ok_or_else(|| {
                OpError::Status(StatusCode::BAD_REQUEST, format!("{f} isn't a whole path, and %{id} has no directory"))
            })?),
            None => None,
        };
        // The pane first, then the rest of its tab's terminals, in order.
        // A pane on a machine isn't one this daemon can reach the nvim of.
        let tab = all.iter().filter(|p| p.tab == me.tab && p.info.id != id && local_terminal(&p.info));
        let places = std::iter::once((me, NvimWhere::Pane))
            .filter(|(p, _)| local_terminal(&p.info))
            .chain(tab.map(|p| (p, NvimWhere::Tab)));
        for (p, how) in places {
            let pane = p.info.id;
            let found = match &file {
                Some(f) => crate::editors::open_in_pane(mux, pane, f, req.line).await.is_ok(),
                // Nothing to open: the nvim that's there.
                None => runs_nvim(&p.info),
            };
            if found {
                return Ok(NvimOpened { pane, how });
            }
        }
        // None running: an nvim editor block beside it (#1734), with
        // arugula.nvim and the page's colors. A build without the editor
        // runs plain nvim there instead.
        if crate::editors::joinable() {
            let at = crate::editors::NvimBlockAt {
                from: id,
                file: file.as_ref().map(|f| f.to_string_lossy().into_owned()),
                line: req.line,
                ..Default::default()
            };
            return match crate::editors::nvim_block(mux, at).await {
                Ok(pane) => Ok(NvimOpened { pane, how: NvimWhere::New }),
                Err(e) => Err(OpError::Status(StatusCode::BAD_REQUEST, e)),
            };
        }
        let run = new_pane(me, file.as_deref(), req.line);
        match mux.api(|r| Api::Run(run, r)).await {
            Some(Ok(pane)) => Ok(NvimOpened { pane, how: NvimWhere::New }),
            Some(Err(e)) => Err(OpError::Status(StatusCode::BAD_REQUEST, e)),
            None => Err(OpError::Status(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())),
        }
    }
}

/// `file` whole: as it is, or under `cwd`.
fn whole(file: &str, cwd: Option<&str>) -> Option<PathBuf> {
    let f = Path::new(file);
    if f.is_absolute() {
        return Some(f.to_owned());
    }
    Some(Path::new(cwd?).join(f))
}

/// A terminal on this host, whose processes are ours to read.
fn local_terminal(p: &PaneInfo) -> bool {
    p.kind == BlockType::Terminal && p.host.is_none()
}

/// What the daemon says runs in the pane is nvim: the editor it found there
/// (#1730), or the foreground command.
fn runs_nvim(p: &PaneInfo) -> bool {
    p.editor.as_ref().is_some_and(|e| e.app == "nvim")
        || p.command
            .as_deref()
            .and_then(|c| c.split_whitespace().next())
            .is_some_and(|c| c.rsplit('/').next() == Some("nvim"))
}

/// A new pane beside `me`, on its machine, running nvim at `file` and
/// `line` in the file's directory (or in `me`'s, with no file).
fn new_pane(me: &PaneSummary, file: Option<&Path>, line: Option<u32>) -> RunRequest {
    let (dir, args) = match file {
        Some(f) => {
            let dir = f.parent().map(|d| d.to_string_lossy().into_owned());
            let at = line.map(|l| format!("+{l} ")).unwrap_or_default();
            (dir, format!("{at}-- {}", quote(&f.to_string_lossy())))
        }
        None => (me.info.cwd.clone(), ".".to_owned()),
    };
    // On a machine its shell starts where it likes: go there first.
    let command = match (&dir, me.info.host) {
        (Some(d), Some(_)) => format!("cd {} && exec nvim {args}", quote(d)),
        _ => format!("exec nvim {args}"),
    };
    RunRequest {
        command: Some(command),
        split: Some(me.info.id),
        join: true,
        cwd: dir,
        from_pane: Some(me.info.id),
        ..RunRequest::default()
    }
}

/// `s` as one word for a POSIX shell (and fish).
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A terminal pane, %3.
    fn info() -> PaneInfo {
        serde_json::from_value(serde_json::json!({
            "id": 3, "epoch": 1, "cwd": null, "command": null, "running": true, "policy": { "kind": "shell" },
        }))
        .unwrap()
    }

    fn pane(cwd: Option<&str>, host: Option<u32>) -> PaneSummary {
        let mut info = info();
        (info.cwd, info.host) = (cwd.map(Into::into), host);
        PaneSummary { session: 1, session_name: "s".into(), tab: 1, tab_name: None, info }
    }

    #[cfg(unix)]
    #[test]
    fn a_relative_file_is_under_the_panes_directory() {
        assert_eq!(whole("src/a.rs", Some("/p")), Some(PathBuf::from("/p/src/a.rs")));
        assert_eq!(whole("/q/b.rs", None), Some(PathBuf::from("/q/b.rs")));
        assert_eq!(whole("b.rs", None), None);
    }

    #[test]
    fn a_new_pane_runs_nvim_at_the_line_in_the_files_directory() {
        let r = new_pane(&pane(Some("/p"), None), Some(Path::new("/p/src/it's.rs")), Some(12));
        assert_eq!(r.command.as_deref(), Some(r"exec nvim +12 -- '/p/src/it'\''s.rs'"));
        assert_eq!(r.cwd.as_deref(), Some("/p/src"));
        assert_eq!((r.split, r.join, r.from_pane), (Some(3), true, Some(3)));
        // No file: the pane's directory.
        let r = new_pane(&pane(Some("/p"), None), None, None);
        assert_eq!(r.command.as_deref(), Some("exec nvim ."));
        assert_eq!(r.cwd.as_deref(), Some("/p"));
        // On a machine, it goes there itself.
        let r = new_pane(&pane(Some("/home/u"), Some(2)), Some(Path::new("/home/u/a.txt")), None);
        assert_eq!(r.command.as_deref(), Some("cd '/home/u' && exec nvim -- '/home/u/a.txt'"));
    }

    #[test]
    fn nvim_is_the_editor_found_or_the_foreground_command() {
        let mut p = info();
        assert!(!runs_nvim(&p));
        p.command = Some("/usr/local/bin/nvim +3 a.txt".into());
        assert!(runs_nvim(&p));
        p.command = Some("vim a.txt".into());
        assert!(!runs_nvim(&p));
        p.editor = Some(arugula_proto::EditorInfo { app: "nvim".into(), ..Default::default() });
        assert!(runs_nvim(&p));
    }
}
