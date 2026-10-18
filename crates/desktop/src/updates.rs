//! App updates (M46): the Tauri updater, against a manifest
//! (`latest.json`) on the release page, signed with the release's updater
//! key.
//!
//! A build without an updater key (`plugins.updater.pubkey` in
//! tauri.conf.json) never checks. One with a key checks a little after
//! launch and then every six hours. A newer release is downloaded,
//! checked against the key and put in place of this app; it runs from the
//! next start, which the tray offers (*Restart to update*). The daemon
//! keeps the panes through that restart.
//!
//! Until that start the app runs from a bundle that's gone, and macOS
//! services it reaches through the bundle stop answering (the file picker,
//! #556), so it doesn't wait for the tray (#561): once the update is in
//! place it restarts by itself when nobody's using it (`wait_for`): no
//! window of ours focused for five minutes, no typing or mouse anywhere for
//! as long (where the system says: macOS), and no dialog or file picker
//! open. It comes back quietly (`Quiet`): the windows it had, on
//! the pages they showed, without taking focus from another app.
//!
//! The app never updates the daemon:
//! the daemon updates itself (#391, #392). When the daemon here speaks a
//! newer protocol than this app knows, the setup page offers the update
//! straight away and restarts into it (`compat.rs`).
//!
//! The manifest is the newest app release's, on the rolling `app-latest`
//! release (#393): app releases (`app-v*`) are apart from the daemon's.
//!
//! Where it can update: the macOS app, the Linux AppImage and the Windows
//! installer. A .deb or .rpm belongs to the package manager. On Windows the
//! installer closes the app as it runs, so the background check only finds
//! the update; installing waits for the tray's *Update*.
//!
//! For tests: `ARUGULA_UPDATE_URL` is the manifest to read,
//! `ARUGULA_UPDATE_RESTART=1` restarts as soon as the update is in, and
//! `ARUGULA_UPDATE_IDLE=SECS` is how long unused counts as idle.

use std::{
    path::PathBuf,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime},
};

use tauri::{AppHandle, Manager, WebviewUrl, menu::MenuItem};
use tauri_plugin_updater::UpdaterExt;

/// The tray's update item, once the app has one.
pub struct Item(pub MenuItem<tauri::Wry>);

/// This build has a key and this install can replace itself (`init`).
static ON: AtomicBool = AtomicBool::new(false);

/// The version put in place, waiting for a restart.
static READY: Mutex<Option<String>> = Mutex::new(None);

/// One check or download at a time: the background's or the tray's.
static BUSY: LazyLock<tauri::async_runtime::Mutex<()>> = LazyLock::new(|| tauri::async_runtime::Mutex::new(()));

/// This build has an updater key.
pub fn configured(config: &tauri::Config) -> bool {
    config
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .is_some_and(|k| !k.trim().is_empty())
}

/// This install can replace itself.
pub fn can_update() -> bool {
    cfg!(target_os = "macos") || cfg!(windows) || std::env::var_os("APPIMAGE").is_some()
}

/// At launch, before any window: whether this app updates itself.
pub fn init(configured: bool) {
    ON.store(configured && can_update(), Ordering::Relaxed);
}

/// The tray offers the update.
pub fn enabled() -> bool {
    ON.load(Ordering::Relaxed)
}

pub fn start(app: AppHandle) {
    if !can_update() {
        eprintln!("arugula: updates come from the package manager here");
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(5));
        loop {
            // A test that restarts at once installs on Windows too.
            let install = !cfg!(windows) || std::env::var_os("ARUGULA_UPDATE_RESTART").is_some();
            match tauri::async_runtime::block_on(fetch(&app, install)) {
                Ok(Some(v)) => {
                    ready(&app, &v);
                    return;
                }
                Ok(None) => {}
                Err(e) => eprintln!("arugula: checking for an update: {e}"),
            }
            std::thread::sleep(Duration::from_secs(6 * 3600));
        }
    });
}

/// A newer release, put in place when `install` (else only found); its
/// version. Already in place: that version, without asking again. Also
/// when the daemon here is newer than this app knows (`compat.rs`).
pub async fn fetch(app: &AppHandle, install: bool) -> Result<Option<String>, String> {
    let _one = BUSY.lock().await;
    if let Some(v) = READY.lock().unwrap().clone() {
        return Ok(Some(v));
    }
    let mut b = app.updater_builder();
    if let Some(u) = std::env::var("ARUGULA_UPDATE_URL").ok().filter(|u| !u.is_empty()) {
        b = b.endpoints(vec![u.parse().map_err(|e| format!("ARUGULA_UPDATE_URL: {e}"))?]).map_err(|e| e.to_string())?;
    }
    let Some(update) = b.build().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    if !install {
        eprintln!("arugula: {} is out; it installs from the tray or the page", update.version);
        return Ok(Some(update.version));
    }
    eprintln!("arugula: updating the app from {} to {}", update.current_version, update.version);
    // Windows: the installer closes this app here and opens the new one.
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    eprintln!("arugula: {} is in place; it runs from the next start", update.version);
    *READY.lock().unwrap() = Some(update.version.clone());
    Ok(Some(update.version))
}

fn ready(app: &AppHandle, version: &str) {
    if std::env::var_os("ARUGULA_UPDATE_RESTART").is_some() && READY.lock().unwrap().is_some() {
        app.restart();
    }
    if READY.lock().unwrap().is_some() {
        restart_when_idle(app);
    }
    if let Some(item) = app.try_state::<Item>() {
        let verb = if READY.lock().unwrap().is_some() { "Restart to update" } else { "Update" };
        let _ = item.0.set_text(format!("{verb} to {version}"));
        let _ = item.0.set_enabled(true);
    }
}

/// The tray's item: restart into the update, installing it first if the
/// background check only found it (Windows).
pub fn from_tray(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match fetch(&app, true).await {
            Ok(Some(_)) => app.restart(),
            Ok(None) => {}
            Err(e) => eprintln!("arugula: updating the app: {e}"),
        }
    });
}

/// *Check for Updates…* (the macOS app menu, and the tray; #419): check
/// now and say what was found, since by the time the tray item's text
/// changes its menu has closed.
pub fn check_now(app: &AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let app = app.clone();
    let current = app.package_info().version.to_string();
    if !enabled() {
        let why = if can_update() {
            "This build of Arugula doesn't check for updates."
        } else {
            "This copy of Arugula updates with its package manager."
        };
        app.dialog().message(why).title(format!("Arugula {current}")).show(|_| {});
        return;
    }
    tauri::async_runtime::spawn(async move {
        // Windows: find only; the installer closes the app, so it waits
        // for a yes.
        let found = fetch(&app, !cfg!(windows)).await;
        let dialog = app.dialog().clone();
        match found {
            Ok(None) => {
                dialog.message(format!("Arugula {current} is the latest.")).title("No update").show(|_| {});
            }
            Ok(Some(v)) => {
                ready(&app, &v);
                let in_place = READY.lock().unwrap().is_some();
                let (text, yes) = if in_place {
                    (format!("Arugula {v} is ready. Restart now to use it?"), "Restart Now")
                } else {
                    (format!("Arugula {v} is out. Install it now? The app closes while it installs."), "Update Now")
                };
                let app = app.clone();
                dialog
                    .message(text)
                    .title("Update")
                    .buttons(MessageDialogButtons::OkCancelCustom(yes.into(), "Later".into()))
                    .show(move |yes| {
                        if yes {
                            from_tray(&app);
                        }
                    });
            }
            Err(e) => {
                dialog
                    .message(format!("Couldn't check for updates: {e}"))
                    .title("Update")
                    .kind(MessageDialogKind::Error)
                    .show(|_| {});
            }
        }
    });
}

// ---- restarting when idle (#561)

/// How long the app goes unused before it restarts into an update.
const IDLE: Duration = Duration::from_secs(5 * 60);
/// How often it looks.
const EVERY: Duration = Duration::from_secs(30);
/// How long a quiet start stays quiet, unless a window takes focus first.
const QUIET_FOR: Duration = Duration::from_secs(60);

/// When this app started, and when a window of ours last gained or lost
/// focus.
static STARTED: LazyLock<Instant> = LazyLock::new(Instant::now);
static LAST_FOCUS: Mutex<Option<Instant>> = Mutex::new(None);
/// One watcher, however many times the update is found ready.
static WATCHING: AtomicBool = AtomicBool::new(false);
/// This start is a restart into an update: windows don't take focus.
static QUIET: AtomicBool = AtomicBool::new(false);

/// What the idle check sees.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Seen {
    /// Since a window of ours last had focus; zero while one has it.
    pub unfocused: Duration,
    /// Since the last key press or mouse move anywhere, where the system
    /// says (macOS).
    pub input: Option<Duration>,
    /// A dialog, an alert or a file picker is open.
    pub dialog: bool,
}

/// Why the app shouldn't restart yet, or `None`: it's been idle `idle`.
pub fn wait_for(s: &Seen, idle: Duration) -> Option<&'static str> {
    if s.dialog {
        Some("a dialog is open")
    } else if s.unfocused < idle {
        Some("a window is in use")
    } else if s.input.is_some_and(|i| i < idle) {
        Some("someone's at the keyboard")
    } else {
        None
    }
}

/// At launch, so idle counts from here.
pub fn started() {
    LazyLock::force(&STARTED);
}

/// A window of ours gained or lost focus.
pub fn focus_changed(focused: bool) {
    *LAST_FOCUS.lock().unwrap() = Some(Instant::now());
    if focused {
        QUIET.store(false, Ordering::Relaxed);
    }
}

/// Windows opened now don't take focus: the app restarted into an update
/// on its own, and someone may be typing in another app.
pub fn quiet() -> bool {
    QUIET.load(Ordering::Relaxed) && STARTED.elapsed() < QUIET_FOR
}

fn idle_after() -> Duration {
    std::env::var("ARUGULA_UPDATE_IDLE")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(IDLE)
}

/// Watch for the app going idle, then restart into the update.
fn restart_when_idle(app: &AppHandle) {
    if WATCHING.swap(true, Ordering::Relaxed) {
        return;
    }
    let app = app.clone();
    let idle = idle_after();
    let spawned = std::thread::Builder::new().name("update-idle".into()).spawn(move || {
        let mut said = None;
        loop {
            std::thread::sleep(EVERY.min(idle).max(Duration::from_secs(1)));
            let why = wait_for(&look(&app), idle);
            if why.is_none() {
                break;
            }
            if why != said {
                eprintln!("arugula: the update waits to restart: {}", why.unwrap_or_default());
                said = why;
            }
        }
        let v = READY.lock().unwrap().clone().unwrap_or_default();
        eprintln!("arugula: restarting into {v} on its own: unused for {}s, no dialog (#561)", idle.as_secs());
        Quiet::save(&app);
        app.restart();
    });
    if let Err(e) = spawned {
        eprintln!("arugula: watching for idle: {e}");
    }
}

/// Look at the app and the machine, for `wait_for`.
fn look(app: &AppHandle) -> Seen {
    let windows = app.webview_windows();
    let focused = windows.values().any(|w| w.is_focused().unwrap_or(false));
    let unfocused = if focused { Duration::ZERO } else { LAST_FOCUS.lock().unwrap().unwrap_or(*STARTED).elapsed() };
    let (tx, rx) = mpsc::channel();
    let _ = app.run_on_main_thread(move || {
        let _ = tx.send(dialog_open());
    });
    let dialog = rx.recv_timeout(Duration::from_secs(5)).unwrap_or(true);
    Seen { unfocused, input: input_idle(), dialog }
}

/// A sheet, an alert or a file picker is up. On the main thread.
#[cfg(target_os = "macos")]
fn dialog_open() -> bool {
    let Some(mtm) = objc2::MainThreadMarker::new() else { return true };
    let ns = objc2_app_kit::NSApplication::sharedApplication(mtm);
    ns.modalWindow().is_some() || ns.windows().iter().any(|w| w.attachedSheet().is_some())
}

/// A GTK dialog is up: an alert, or WebKitGTK's file picker. On the main
/// thread.
#[cfg(target_os = "linux")]
fn dialog_open() -> bool {
    use gtk::prelude::*;
    gtk::Window::list_toplevels().iter().any(|w| w.is_visible() && w.is::<gtk::Dialog>())
}

/// Windows installs in place only from the tray, which restarts at once.
#[cfg(windows)]
fn dialog_open() -> bool {
    false
}

/// Since the last input event anywhere in this login session.
#[cfg(target_os = "macos")]
fn input_idle() -> Option<Duration> {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event: u32) -> f64;
    }
    // kCGEventSourceStateCombinedSessionState, kCGAnyInputEventType.
    // SAFETY: a plain C call with no pointers.
    let s = unsafe { CGEventSourceSecondsSinceLastEventType(0, u32::MAX) };
    (s.is_finite() && s >= 0.0).then(|| Duration::from_secs_f64(s))
}

#[cfg(not(target_os = "macos"))]
fn input_idle() -> Option<Duration> {
    None
}

/// A restart into an update, written down for the next start: the windows
/// the app had, on the pages they showed.
#[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Quiet {
    pub urls: Vec<String>,
}

impl Quiet {
    /// Where it's kept: the app's cache directory (Tauri's
    /// `app_cache_dir`), found before there's an app.
    fn path(identifier: &str) -> Option<PathBuf> {
        let home = || std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
        let base = if cfg!(target_os = "macos") {
            home()?.join("Library/Caches")
        } else if cfg!(windows) {
            return None;
        } else {
            match std::env::var_os("XDG_CACHE_HOME").filter(|d| !d.is_empty()) {
                Some(d) => PathBuf::from(d),
                None => home()?.join(".cache"),
            }
        };
        Some(base.join(identifier).join("restarted-for-update.json"))
    }

    fn save(app: &AppHandle) {
        let mut windows: Vec<_> = app.webview_windows().into_iter().collect();
        // w0, w1, … w10: in the order they opened.
        windows.sort_by_key(|(l, _)| (l.len(), l.clone()));
        let urls = windows.iter().filter_map(|(_, w)| w.url().ok()).map(|u| u.to_string()).collect();
        let Some(path) = Self::path(&app.config().identifier) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, serde_json::to_vec(&Quiet { urls }).unwrap_or_default()) {
            eprintln!("arugula: {}: {e}", path.display());
        }
    }

    /// At launch: this start is a restart into an update (written in the
    /// last two minutes), and from now on it's forgotten.
    pub fn take(identifier: &str) -> Option<Quiet> {
        Self::take_from(&Self::path(identifier)?)
    }

    fn take_from(path: &std::path::Path) -> Option<Quiet> {
        let fresh = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .is_ok_and(|t| SystemTime::now().duration_since(t).is_ok_and(|d| d < Duration::from_secs(120)));
        let read = std::fs::read(path).ok()?;
        let _ = std::fs::remove_file(path);
        let q = serde_json::from_slice(&read).ok().filter(|_| fresh)?;
        started();
        QUIET.store(true, Ordering::Relaxed);
        eprintln!("arugula: restarted into an update; the windows come back without taking focus");
        Some(q)
    }

    /// Where each window opens again: its page when that's the daemon's or
    /// control's (`ours`), else where a new window starts (`target`), which
    /// is also where they all start when that's the setup page.
    pub fn windows(&self, target: &WebviewUrl, ours: impl Fn(&tauri::Url) -> bool) -> Vec<WebviewUrl> {
        self.urls
            .iter()
            .map(|u| match (target, u.parse::<tauri::Url>()) {
                (WebviewUrl::External(_), Ok(u)) if matches!(u.scheme(), "http" | "https") && ours(&u) => {
                    WebviewUrl::External(u)
                }
                _ => target.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    fn idle() -> Seen {
        Seen { unfocused: 10 * MIN, input: Some(10 * MIN), dialog: false }
    }

    #[test]
    fn restarts_once_nobody_is_using_it() {
        assert_eq!(wait_for(&idle(), 5 * MIN), None);
        // The threshold itself counts.
        assert_eq!(wait_for(&Seen { unfocused: 5 * MIN, input: Some(5 * MIN), ..idle() }, 5 * MIN), None);
        // No word from the system on input (Linux): focus decides.
        assert_eq!(wait_for(&Seen { input: None, ..idle() }, 5 * MIN), None);
    }

    #[test]
    fn waits_while_a_window_is_in_use() {
        let s = Seen { unfocused: Duration::ZERO, ..idle() };
        assert_eq!(wait_for(&s, 5 * MIN), Some("a window is in use"));
        let s = Seen { unfocused: 4 * MIN, ..idle() };
        assert_eq!(wait_for(&s, 5 * MIN), Some("a window is in use"));
    }

    #[test]
    fn waits_while_someone_types_elsewhere() {
        // Another app has focus and someone's typing there: a window
        // coming back now would be in their way.
        let s = Seen { input: Some(Duration::from_secs(3)), ..idle() };
        assert_eq!(wait_for(&s, 5 * MIN), Some("someone's at the keyboard"));
    }

    #[test]
    fn waits_through_a_dialog() {
        assert_eq!(wait_for(&Seen { dialog: true, ..idle() }, 5 * MIN), Some("a dialog is open"));
        let all = Seen { unfocused: Duration::ZERO, input: Some(Duration::ZERO), dialog: true };
        assert_eq!(wait_for(&all, 5 * MIN), Some("a dialog is open"));
    }

    #[test]
    fn windows_come_back_on_their_pages() {
        let q = Quiet {
            urls: vec![
                "http://127.0.0.1:7600/#pane=3".into(),
                "https://elsewhere.example/".into(),
                "tauri://localhost/index.html".into(),
            ],
        };
        let home = WebviewUrl::External("http://127.0.0.1:7600/".parse().unwrap());
        let ours = |u: &tauri::Url| u.port() == Some(7600);
        let got: Vec<String> = q.windows(&home, ours).iter().map(|w| w.to_string()).collect();
        assert_eq!(got, ["http://127.0.0.1:7600/#pane=3", "http://127.0.0.1:7600/", "http://127.0.0.1:7600/"]);
        // The daemon's gone or doesn't match: every window starts on the
        // setup page instead.
        let setup = WebviewUrl::App("index.html".into());
        let got: Vec<String> = q.windows(&setup, ours).iter().map(|w| w.to_string()).collect();
        assert_eq!(got, ["index.html"; 3]);
    }

    #[test]
    fn a_restart_is_written_down_where_the_next_start_reads_it() {
        let dir = std::env::temp_dir().join(format!("arugula-quiet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("restarted-for-update.json");
        assert_eq!(Quiet::take_from(&path), None);
        assert!(!quiet());
        std::fs::write(&path, r#"{"urls":["http://127.0.0.1:7600/"]}"#).unwrap();
        assert_eq!(Quiet::take_from(&path), Some(Quiet { urls: vec!["http://127.0.0.1:7600/".into()] }));
        assert!(quiet());
        // Read once: the start after this one is an ordinary start.
        assert!(!path.exists());
        assert_eq!(Quiet::take_from(&path), None);
        // A window taking focus ends it.
        focus_changed(true);
        assert!(!quiet());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
