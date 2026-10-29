//! nvim editor blocks (#1734): "New editor block" with nvim, beside VS
//! Code's.
//!
//! An nvim block is a terminal typed as an editor (the mux keeps its
//! `app`, `nvim`, and lists it as an editor whatever runs in it), running
//! `nvim` in a folder or `nvim +LINE -- FILE`, on the machine of the pane
//! it was opened from.
//!
//! - **arugula.nvim, whatever the config.** The daemon writes its own copy
//!   of the plugin (the one `build.rs` embeds, #1733) into
//!   `<state>/editor/nvim/v/<stamp>/`, points `<state>/editor/nvim/arugula.nvim`
//!   at it, and starts nvim with `--cmd "lua dofile(<block.lua>)"`, which
//!   puts that copy first on the runtimepath before the user's config runs
//!   (`set rtp^=`), and puts it back at `VimEnter` if a plugin manager
//!   reset the runtimepath (lazy.nvim does), sourcing its `plugin/` if
//!   nothing loaded it. The user's own config still loads. The plugin then
//!   joins as the pane (`$ARUGULA_PANE`), so the block is followable from
//!   the start. The link is stable, so a restart's command still finds the
//!   plugin after the daemon is upgraded.
//! - **The page's colors** (`NvimTheme`, made by the page from its palette
//!   as it makes VS Code's, `web/src/shell/editor-theme.ts`) are kept in
//!   `<state>/editor/nvim/theme.json` and sent to each block's plugin when
//!   it joins, and to every block when the page's palette changes: the
//!   plugin applies them as the `arugula` colorscheme (`theme.lua`), unless
//!   `vim.g.arugula = { theme = false }` or the user picked another
//!   colorscheme in that nvim.
//! - **A restart.** Across a daemon restart the pane's nvim keeps running
//!   (the shim holds its terminal) and its plugin joins again. For a
//!   reboot, the block's restart policy is a command that opens nvim again
//!   on its file and line: [`watch`] keeps it up to date from what the
//!   pane's editor reports, and leaves a policy someone picked alone.
//!
//! On a VM tab the block runs nvim on the VM, without the plugin (its copy
//! is on this host) and without the colors: there's no daemon there for
//! it to join.

use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use arugula_proto::{
    BlockType, PaneId, PaneInfo, Policy,
    api::{NvimBlockOpened, NvimBlockRequest, NvimTheme, NvimThemed, PaneSummary, RunRequest},
    op::ops::{NvimBlockOpen, NvimBlockTheme},
};
use arugulad_base::ops::{Cx, Handle, OpError};
use axum::http::StatusCode;
use serde_json::json;
use tracing::{info, warn};

use super::{link::Link, nvim_plugin};
use crate::{
    Editor,
    mux::{Api, MuxHandle},
};

/// What an nvim block's `app` says.
pub const APP: &str = "nvim";
/// How often [`watch`] brings the blocks' restart commands up to date.
const LOOK: Duration = Duration::from_secs(2);
const MAX_GROUPS: usize = 500;

/// `<state>/editor/nvim`, once the editor is installed.
fn home() -> Option<PathBuf> {
    super::server::settings().map(|s| s.dir.join("nvim"))
}

// ---- the plugin's copy, and how nvim loads it

/// The bootstrap nvim sources with `--cmd`: arugula.nvim from `dir` first
/// on the runtimepath, even after a plugin manager resets it.
fn bootstrap(dir: &Path) -> String {
    format!(
        r#"-- Arugula's nvim editor block (#1734), written by its daemon: the daemon's
-- arugula.nvim first on the runtimepath, whatever the config does to it.
local dir = [==[{}]==]
vim.opt.runtimepath:prepend(dir)
vim.api.nvim_create_autocmd("VimEnter", {{
  once = true,
  callback = function()
    -- A plugin manager may have reset the runtimepath (lazy.nvim does).
    if not vim.tbl_contains(vim.opt.runtimepath:get(), dir) then vim.opt.runtimepath:prepend(dir) end
    if not vim.g.loaded_arugula then vim.cmd.source(vim.fn.fnameescape(dir .. "/plugin/arugula.lua")) end
  end,
}})
"#,
        dir.display()
    )
}

/// Writes this daemon's arugula.nvim under `home` (once per version), the
/// stable link to it and the bootstrap. The bootstrap's path. The copy is
/// the one an install writes (`nvim_plugin::write`): the embedded files and
/// their `doc/tags`, so `:help arugula` works in a block.
pub fn runtime_at(home: &Path) -> io::Result<PathBuf> {
    let stamp = nvim_plugin::stamp();
    let versions = home.join("v");
    let mine = versions.join(&stamp);
    if !mine.join("plugin/arugula.lua").is_file() {
        let new = versions.join(format!(".new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&new);
        for (rel, bytes) in nvim_plugin::FILES {
            let p = new.join(rel);
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(p, bytes)?;
        }
        let tags = nvim_plugin::help_tags();
        if !tags.is_empty() {
            std::fs::write(new.join("doc/tags"), tags)?;
        }
        let _ = std::fs::remove_dir_all(&mine);
        std::fs::rename(&new, &mine)?;
    }
    // The link moves in one rename: a running nvim never finds it missing.
    let link = home.join("arugula.nvim");
    let target = Path::new("v").join(&stamp);
    if std::fs::read_link(&link).ok().as_deref() != Some(target.as_path()) {
        let tmp = home.join(format!(".arugula.nvim-{}", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &tmp)?;
        #[cfg(not(unix))]
        return Err(io::Error::other("nvim blocks need macOS or Linux"));
        std::fs::rename(&tmp, &link)?;
    }
    // Other versions: the link no longer names them.
    if let Ok(dir) = std::fs::read_dir(&versions) {
        for e in dir.flatten() {
            if e.file_name().to_string_lossy() != stamp.as_str() {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let boot = home.join("block.lua");
    let text = bootstrap(&link);
    if std::fs::read_to_string(&boot).ok().as_deref() != Some(text.as_str()) {
        crate::store::write_atomic(&boot, text.as_bytes())?;
    }
    Ok(boot)
}

/// [`runtime_at`] in this daemon's state.
async fn runtime() -> Result<PathBuf, String> {
    let home = home().ok_or("nvim blocks are off here")?;
    tokio::task::spawn_blocking(move || runtime_at(&home))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("couldn't write arugula.nvim for the block: {e}"))
}

// ---- the command

/// `s` as one word for a POSIX shell (and fish).
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// nvim, with the bootstrap (`boot`) if there is one, at `file` and `line`
/// or on the folder it starts in.
pub fn nvim(boot: Option<&Path>, file: Option<&Path>, line: Option<u32>) -> String {
    let mut c = String::from("exec nvim");
    if let Some(b) = boot {
        c += &format!(" --cmd {}", quote(&format!("lua dofile([==[{}]==])", b.display())));
    }
    match file {
        Some(f) => {
            if let Some(l) = line {
                c += &format!(" +{l}");
            }
            c += &format!(" -- {}", quote(&f.to_string_lossy()));
        }
        None => c += " .",
    }
    c
}

/// The restart command: nvim again in `dir`, at `file` and `line`.
pub fn restart(dir: &str, boot: Option<&Path>, file: Option<&Path>, line: Option<u32>) -> String {
    format!("cd {} && {}", quote(dir), nvim(boot, file, line))
}

/// A restart command [`restart`] made: one [`watch`] may bring up to date.
fn ours(p: &Policy) -> Option<&str> {
    match p {
        Policy::Hook { command } if command.starts_with("cd ") && command.contains(" && exec nvim ") => Some(command),
        _ => None,
    }
}

/// `file` whole: as it is, or under `dir`.
fn whole(file: &str, dir: Option<&str>) -> Option<PathBuf> {
    let f = Path::new(file);
    if f.is_absolute() {
        return Some(f.to_owned());
    }
    Some(Path::new(dir?).join(f))
}

// ---- making one

/// Where a new nvim block goes and what it opens.
#[derive(Debug, Clone, Default)]
pub struct At {
    /// Beside this pane, on its machine.
    pub from: PaneId,
    /// The folder (none: the file's, else the pane's).
    pub dir: Option<String>,
    /// A file, whole or relative to the folder, and its line.
    pub file: Option<String>,
    pub line: Option<u32>,
    /// On this host even if the pane's tab is a VM's.
    pub local: bool,
}

/// What [`open`] runs, from the pane it's beside.
fn plan(me: &PaneSummary, at: &At, boot: Option<&Path>) -> Result<RunRequest, String> {
    let on_machine = me.info.host.is_some() && !at.local;
    // The pane's directory means something only where the pane runs.
    let pane_dir = (me.info.host.is_none() || on_machine).then(|| me.info.cwd.clone()).flatten();
    let file = match at.file.as_deref().filter(|f| !f.is_empty()) {
        Some(f) => Some(
            whole(f, at.dir.as_deref().or(pane_dir.as_deref()))
                .ok_or_else(|| format!("{f} isn't a whole path, and %{} has no directory", me.info.id))?,
        ),
        None => None,
    };
    let dir = at
        .dir
        .clone()
        .filter(|d| !d.is_empty())
        .or_else(|| file.as_deref().and_then(Path::parent).map(|d| d.to_string_lossy().into_owned()))
        .or(pane_dir);
    let boot = if on_machine { None } else { boot };
    let run = nvim(boot, file.as_deref(), at.line);
    // On a machine its shell starts where it likes: go there first.
    let command = match (&dir, on_machine) {
        (Some(d), true) => format!("cd {} && {run}", quote(d)),
        _ => run,
    };
    let policy = Policy::Hook { command: restart(dir.as_deref().unwrap_or("."), boot, file.as_deref(), at.line) };
    Ok(RunRequest {
        command: Some(command),
        split: Some(me.info.id),
        join: !at.local,
        cwd: dir,
        policy: Some(policy),
        from_pane: Some(me.info.id),
        ..RunRequest::default()
    })
}

/// Makes an nvim block beside `at.from`: a terminal running nvim, typed as
/// an editor. Its pane.
pub async fn open(mux: &MuxHandle, at: At) -> Result<PaneId, String> {
    let all = mux.api(Api::Panes).await.ok_or("daemon is shutting down")?;
    let me = all.iter().find(|p| p.info.id == at.from).ok_or_else(|| format!("no pane %{}", at.from))?;
    let here = me.info.host.is_none() || at.local;
    let boot = if here { Some(runtime().await?) } else { None };
    let run = plan(me, &at, boot.as_deref())?;
    let pane = match mux.api(|r| Api::Run(run, r)).await {
        Some(r) => r?,
        None => return Err("daemon is shutting down".into()),
    };
    mux.send(crate::mux::Cmd::Api(Api::EditorApp(pane, APP.into())));
    info!(pane, from = at.from, file = at.file.as_deref().unwrap_or(""), "nvim block");
    Ok(pane)
}

// ---- keeping it

/// Each nvim block's restart command follows its file and line.
pub fn watch(mux: MuxHandle) {
    crate::task::spawn(async move {
        loop {
            tokio::time::sleep(LOOK).await;
            let Some(all) = mux.api(Api::Panes).await else { return };
            for p in all.iter().filter(|p| is_block(&p.info) && p.info.host.is_none()) {
                let Some(was) = ours(&p.info.policy) else { continue };
                let Some(want) = wanted(&p.info, was) else { continue };
                if want != was {
                    let (old, new) = (p.info.policy.clone(), Policy::Hook { command: want });
                    mux.api(|r| Api::SwapPolicy(p.info.id, old, new, r)).await;
                }
            }
        }
    });
}

/// An nvim block on this daemon.
fn is_block(i: &PaneInfo) -> bool {
    i.kind == BlockType::Terminal && i.app.as_deref() == Some(APP)
}

/// The restart command for what the block's nvim shows now; `None` until
/// its editor has said.
fn wanted(i: &PaneInfo, was: &str) -> Option<String> {
    let editor = i.editor.as_ref()?;
    let dir = i.cwd.as_deref()?;
    let file = i.file.as_deref().and_then(|f| whole(f, Some(dir)));
    // The bootstrap it started with, kept.
    let boot = was
        .split_once("--cmd 'lua dofile([==[")
        .and_then(|(_, rest)| rest.split_once("]==])'"))
        .map(|(b, _)| PathBuf::from(b));
    Some(restart(dir, boot.as_deref(), file.as_deref(), editor.line.filter(|_| file.is_some())))
}

// ---- colors

/// The colors last sent, and the blocks' plugins to send them to.
struct Colors {
    theme: Option<NvimTheme>,
    loaded: bool,
    links: BTreeMap<PaneId, Weak<Link>>,
}

static COLORS: Mutex<Colors> = Mutex::new(Colors { theme: None, loaded: false, links: BTreeMap::new() });

fn theme_file() -> Option<PathBuf> {
    home().map(|h| h.join("theme.json"))
}

fn current_theme() -> Option<NvimTheme> {
    let mut c = COLORS.lock().unwrap();
    if !c.loaded {
        c.loaded = true;
        c.theme = theme_file().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok());
    }
    c.theme.clone()
}

fn send_theme(link: &Link, t: &NvimTheme) {
    link.send(json!({ "t": "theme", "theme": t }));
}

/// arugula.nvim joined as terminal `pane`: if it's an nvim block, it gets
/// the page's colors, and every change to them from now on.
pub fn joined(mux: &MuxHandle, pane: PaneId, link: &Arc<Link>) {
    let (mux, weak) = (mux.clone(), Arc::downgrade(link));
    crate::task::spawn(async move {
        let all = mux.api(Api::Panes).await.unwrap_or_default();
        if !all.iter().any(|p| p.info.id == pane && is_block(&p.info)) {
            return;
        }
        let Some(link) = weak.upgrade() else { return };
        COLORS.lock().unwrap().links.insert(pane, weak);
        if let Some(t) = tokio::task::spawn_blocking(current_theme).await.ok().flatten() {
            send_theme(&link, &t);
        }
    });
}

/// ...and left.
pub fn left(pane: PaneId, link: &Arc<Link>) {
    let mut c = COLORS.lock().unwrap();
    if c.links.get(&pane).is_some_and(|w| w.as_ptr() == Arc::as_ptr(link)) {
        c.links.remove(&pane);
    }
}

fn color(s: &str) -> bool {
    let hex = s.strip_prefix('#').unwrap_or("");
    hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

fn group_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_@.".contains(&b))
}

/// Colors only, as `#rrggbb`, under highlight groups' names.
pub fn check(t: &NvimTheme) -> Result<(), String> {
    if t.groups.len() > MAX_GROUPS || t.terminal.len() > 16 {
        return Err("theme: too many colors".into());
    }
    for (name, h) in &t.groups {
        let colors = [&h.fg, &h.bg, &h.sp].into_iter().flatten().all(|c| color(c));
        if !group_name(name) || !colors || h.link.as_deref().is_some_and(|l| !group_name(l)) {
            return Err(format!("theme: {name}"));
        }
    }
    if !t.terminal.iter().all(|c| color(c)) {
        return Err("theme: a terminal color".into());
    }
    Ok(())
}

/// Keeps `t` and sends it to every block whose plugin is here. How many.
fn set_theme(t: NvimTheme) -> u32 {
    if let Some(f) = theme_file()
        && let Err(e) = serde_json::to_vec(&t).map_err(io::Error::other).and_then(|b| {
            if let Some(d) = f.parent() {
                std::fs::create_dir_all(d)?;
            }
            crate::store::write_atomic(&f, &b)
        })
    {
        warn!(error = %e, "couldn't keep the nvim blocks' colors");
    }
    let mut c = COLORS.lock().unwrap();
    (c.theme, c.loaded) = (Some(t.clone()), true);
    c.links.retain(|_, w| w.strong_count() > 0);
    let links: Vec<Arc<Link>> = c.links.values().filter_map(Weak::upgrade).collect();
    drop(c);
    for l in &links {
        send_theme(l, &t);
    }
    links.len() as u32
}

// ---- the operations (owner only: `Access::Owner`, checked before these run)

impl Handle<Editor> for NvimBlockOpen {
    async fn handle(cx: &Cx<'_>, from: PaneId, req: NvimBlockRequest) -> Result<NvimBlockOpened, OpError> {
        let at = At { from, dir: req.dir, file: req.file, line: req.line, local: req.local.unwrap_or(false) };
        match open(&cx.app.mux, at).await {
            Ok(pane) => Ok(NvimBlockOpened { pane }),
            Err(e) if e.starts_with("no pane") => Err(OpError::NoPane(from)),
            Err(e) => Err(OpError::Status(StatusCode::BAD_REQUEST, e)),
        }
    }
}

impl Handle<Editor> for NvimBlockTheme {
    async fn handle(_: &Cx<'_>, _: (), t: NvimTheme) -> Result<NvimThemed, OpError> {
        check(&t).map_err(|e| OpError::Status(StatusCode::BAD_REQUEST, e))?;
        let blocks =
            tokio::task::spawn_blocking(move || set_theme(t)).await.map_err(|e| OpError::Failed(e.to_string()))?;
        Ok(NvimThemed { blocks })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arugula_proto::api::NvimHl;

    fn pane(cwd: Option<&str>, host: Option<u32>) -> PaneSummary {
        let mut info: PaneInfo = serde_json::from_value(json!({
            "id": 3, "epoch": 1, "cwd": null, "command": null, "running": true, "policy": { "kind": "shell" },
        }))
        .unwrap();
        (info.cwd, info.host) = (cwd.map(Into::into), host);
        PaneSummary { session: 1, session_name: "s".into(), tab: 1, tab_name: None, info }
    }

    #[test]
    fn a_block_runs_nvim_with_the_plugin_at_the_file_and_line() {
        let boot = Path::new("/st/editor/nvim/block.lua");
        let at = At { from: 3, file: Some("src/it's.rs".into()), line: Some(12), ..At::default() };
        let r = plan(&pane(Some("/p"), None), &at, Some(boot)).unwrap();
        assert_eq!(
            r.command.as_deref(),
            Some(r"exec nvim --cmd 'lua dofile([==[/st/editor/nvim/block.lua]==])' +12 -- '/p/src/it'\''s.rs'")
        );
        assert_eq!(r.cwd.as_deref(), Some("/p/src"));
        assert_eq!((r.split, r.join, r.from_pane), (Some(3), true, Some(3)));
        let Some(Policy::Hook { command }) = &r.policy else { panic!("{:?}", r.policy) };
        assert_eq!(command, &format!("cd '/p/src' && {}", r.command.as_deref().unwrap()));
        assert!(ours(r.policy.as_ref().unwrap()).is_some());
        // A folder: nvim on it, there.
        let at = At { from: 3, dir: Some("/q".into()), ..At::default() };
        let r = plan(&pane(Some("/p"), None), &at, Some(boot)).unwrap();
        assert_eq!(r.command.as_deref(), Some("exec nvim --cmd 'lua dofile([==[/st/editor/nvim/block.lua]==])' ."));
        assert_eq!(r.cwd.as_deref(), Some("/q"));
        // Nothing: the pane's directory.
        let r = plan(&pane(Some("/p"), None), &At { from: 3, ..At::default() }, Some(boot)).unwrap();
        assert_eq!(r.cwd.as_deref(), Some("/p"));
        assert!(plan(&pane(None, None), &At { from: 3, file: Some("a.rs".into()), ..At::default() }, None).is_err());
    }

    #[test]
    fn on_a_vm_it_goes_there_without_the_plugin_unless_asked_here() {
        let boot = Path::new("/st/block.lua");
        let at = At { from: 3, file: Some("/home/u/a.txt".into()), ..At::default() };
        let r = plan(&pane(Some("/home/u"), Some(2)), &at, Some(boot)).unwrap();
        assert_eq!(r.command.as_deref(), Some("cd '/home/u' && exec nvim -- '/home/u/a.txt'"));
        assert!(r.join);
        // Here, from a VM tab: this host, the pane's directory not used.
        let r =
            plan(&pane(Some("/home/u"), Some(2)), &At { from: 3, local: true, ..At::default() }, Some(boot)).unwrap();
        assert_eq!(r.command.as_deref(), Some("exec nvim --cmd 'lua dofile([==[/st/block.lua]==])' ."));
        assert_eq!((r.join, r.cwd.as_deref()), (false, None));
    }

    #[test]
    fn the_restart_follows_the_file_and_line_and_keeps_its_bootstrap() {
        let was = restart("/p", Some(Path::new("/st/block.lua")), None, None);
        let mut i = pane(Some("/p"), None).info;
        assert_eq!(wanted(&i, &was), None, "not until its editor says");
        i.editor = Some(arugula_proto::EditorInfo { app: "nvim".into(), line: Some(40), ..Default::default() });
        i.file = Some("src/b.rs".into());
        assert_eq!(
            wanted(&i, &was).as_deref(),
            Some("cd '/p' && exec nvim --cmd 'lua dofile([==[/st/block.lua]==])' +40 -- '/p/src/b.rs'")
        );
        // Someone picked another policy: not ours to change.
        assert!(ours(&Policy::Hook { command: "make watch".into() }).is_none());
        assert!(ours(&Policy::Shell).is_none());
    }

    #[test]
    fn colors_are_checked() {
        let mut t = NvimTheme { light: false, groups: Default::default(), terminal: vec!["#000000".into()] };
        t.groups.insert(
            "Normal".into(),
            NvimHl { fg: Some("#cdd6f4".into()), bg: Some("#1e1e2e".into()), ..Default::default() },
        );
        t.groups.insert("@string".into(), NvimHl { link: Some("String".into()), ..Default::default() });
        assert_eq!(check(&t), Ok(()));
        let mut bad = t.clone();
        bad.groups.insert("Normal".into(), NvimHl { fg: Some("red".into()), ..Default::default() });
        assert!(check(&bad).is_err());
        let mut bad = t.clone();
        bad.groups.insert("x]]; os.exit()".into(), NvimHl::default());
        assert!(check(&bad).is_err());
        let mut bad = t;
        bad.terminal = vec!["#12".into()];
        assert!(check(&bad).is_err());
    }

    /// nvim, headless, with the block's bootstrap and `init` as its config;
    /// it runs `lua` once it has started (at `VimEnter`, after the
    /// bootstrap's: when the plugin's colors can first come) and prints
    /// what that returns.
    fn headless(boot: &Path, init: &str, lua: &str) -> Option<String> {
        let dir = boot.parent().unwrap().join("cfg");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("init.lua"), init).unwrap();
        std::fs::write(
            dir.join("probe.lua"),
            format!(
                "vim.api.nvim_create_autocmd('VimEnter', {{ once = true, nested = true, callback = function()\n\
                 local ok, v = pcall(function() {lua} end)\n\
                 io.stdout:write(vim.json.encode(ok and v or {{ 'error', tostring(v) }}))\n\
                 vim.cmd('qa!')\nend }})\n"
            ),
        )
        .unwrap();
        let out = std::process::Command::new("nvim")
            .args(["--headless", "-u"])
            .arg(dir.join("init.lua"))
            .arg("--cmd")
            .arg(format!("lua dofile([==[{}]==])", boot.display()))
            .arg("-c")
            .arg(format!("luafile {}", dir.join("probe.lua").display()))
            .env_remove("ARUGULA_PANE")
            .env("XDG_DATA_HOME", dir.join("data"))
            .env("XDG_STATE_HOME", dir.join("state"))
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    #[test]
    fn a_real_nvim_loads_the_plugin_whatever_its_config_and_takes_the_colors_unless_kept() {
        let v = std::process::Command::new("nvim").arg("--version").output();
        let Some(v) = v.ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()) else {
            eprintln!("SKIPPED: no nvim here");
            return;
        };
        if v.starts_with("NVIM v0.")
            && v[8..].split('.').next().and_then(|m| m.parse::<u32>().ok()).is_some_and(|m| m < 10)
        {
            eprintln!("SKIPPED: {} is older than 0.10", v.lines().next().unwrap_or(""));
            return;
        }
        let home = std::env::temp_dir().join(format!("ilg-nvt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let boot = runtime_at(&home).unwrap();
        let theme = r##"{ light = true, groups = { Normal = { fg = "#4c4f69", bg = "#eff1f5" }, ["@string"] = { link = "String" } }, terminal = {} }"##;
        let apply = format!(
            "local r = require('arugula.theme').apply({theme}); return {{ r, vim.g.colors_name or '', vim.o.background, string.format('#%06x', vim.api.nvim_get_hl(0, {{ name = 'Normal' }}).bg or 0), vim.g.loaded_arugula == true }}"
        );
        // A config that resets the runtimepath, as lazy.nvim does: the
        // plugin is loaded anyway, and the colors are applied.
        let reset = "vim.opt.runtimepath = { vim.env.VIMRUNTIME }\n";
        let out = headless(&boot, reset, &apply).unwrap();
        assert_eq!(out, r##"["applied","arugula","light","#eff1f5",true]"##, "{out}");
        // `vim.g.arugula = { theme = false }`: the user's colors stay.
        let out = headless(&boot, "vim.g.arugula = { theme = false }\n", &apply).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap_or_else(|_| panic!("{out}"));
        assert_eq!((&v[0], &v[1], &v[4]), (&json!("kept"), &json!(""), &json!(true)), "{out}");
        // A colorscheme picked after ours is theirs from then on.
        let after = format!(
            "local t = {theme}; local th = require('arugula.theme'); th.apply(t); vim.cmd.colorscheme('default'); return {{ th.apply(t), vim.g.colors_name }}"
        );
        assert_eq!(headless(&boot, "", &after).unwrap(), r#"["left","default"]"#);
        // Its help is found (doc/tags), as an installed copy's is.
        let help = "vim.cmd('help arugula'); return { vim.bo.filetype, vim.fn.expand('%:t') }";
        assert_eq!(headless(&boot, "", help).unwrap(), r#"["help","arugula.txt"]"#);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_plugin_is_written_once_per_version_behind_a_stable_link() {
        let home = std::env::temp_dir().join(format!("ilg-nvb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("v/0.0.1+old/plugin")).unwrap();
        let boot = runtime_at(&home).unwrap();
        assert_eq!(boot, home.join("block.lua"));
        let link = home.join("arugula.nvim");
        assert!(link.join("plugin/arugula.lua").is_file());
        assert!(link.join("lua/arugula/theme.lua").is_file());
        assert_eq!(std::fs::read_to_string(link.join("doc/tags")).unwrap(), nvim_plugin::help_tags());
        assert_eq!(std::fs::read_link(&link).unwrap(), Path::new("v").join(nvim_plugin::stamp()));
        assert!(!home.join("v/0.0.1+old").exists(), "an older version goes");
        assert!(std::fs::read_to_string(&boot).unwrap().contains(&format!("[==[{}]==]", link.display())));
        // Again: nothing to do, and the same.
        assert_eq!(runtime_at(&home).unwrap(), boot);
        let _ = std::fs::remove_dir_all(&home);
    }
}
