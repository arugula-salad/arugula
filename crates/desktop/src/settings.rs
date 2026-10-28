//! The app's own settings (`desktop.json` in its config directory), and
//! the global hotkey they turn on.
//!
//! The hotkey is off by default (M46). The tray's *Global hotkey* item
//! turns it on or off; `hotkey` in the file picks the keys
//! (`Ctrl+Alt+Space` unless set). Pressed, it brings Arugula to the
//! front, or hides it when one of its windows has focus.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+Space";

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Settings {
    /// The global hotkey is on.
    #[serde(default)]
    pub hotkey_on: bool,
    /// Its keys, in the global-shortcut plugin's syntax.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotkey: Option<String>,
}

impl Settings {
    pub fn keys(&self) -> &str {
        self.hotkey.as_deref().filter(|k| !k.trim().is_empty()).unwrap_or(DEFAULT_HOTKEY)
    }
}

fn file(app: &AppHandle) -> Option<PathBuf> {
    if let Some(f) = std::env::var_os("ARUGULA_DESKTOP_SETTINGS") {
        return Some(f.into());
    }
    Some(app.path().app_config_dir().ok()?.join("desktop.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    file(app)
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, s: &Settings) {
    let Some(f) = file(app) else { return };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&f, serde_json::to_string_pretty(s).unwrap()) {
        eprintln!("arugula: saving {}: {e}", f.display());
    }
}

/// #505: the app's identifier changed (`wtf.widgets.illogical` to
/// `io.arugula.desktop`), and its directories with it. The first launch
/// copies the settings from the old app's config directory (copies: the
/// old app may still be there). Not `cloud.json`: it says which controls
/// the window holds a session on, and those sessions are in the WebView's
/// storage, which starts fresh.
pub fn adopt_old(app: &AppHandle) {
    let Ok(dir) = app.path().app_config_dir() else { return };
    let old = dir.with_file_name(arugula_proto::service::OLD_APP_ID);
    for name in adopt(&old, &dir) {
        eprintln!("arugula: took {name} from {}", old.display());
    }
}

/// The settings files `old` has and `dir` doesn't, copied over.
fn adopt(old: &std::path::Path, dir: &std::path::Path) -> Vec<&'static str> {
    if old == dir {
        return Vec::new();
    }
    ["desktop.json", "daemon.json"]
        .into_iter()
        .filter(|name| {
            let (from, to) = (old.join(name), dir.join(name));
            from.is_file() && !to.exists() && std::fs::create_dir_all(dir).is_ok() && std::fs::copy(&from, &to).is_ok()
        })
        .collect()
}

/// The plugin, with what a press does.
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                toggle(app);
            }
        })
        .build()
}

/// Register the hotkey when it's on. Says why not when it can't be had.
pub fn apply(app: &AppHandle, s: &Settings) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if !s.hotkey_on {
        return Ok(());
    }
    gs.register(s.keys()).map_err(|e| format!("the global hotkey {}: {e}", s.keys()))?;
    eprintln!("arugula: global hotkey {}", s.keys());
    Ok(())
}

/// The tray's *Global hotkey* item, which `start` unchecks when the keys
/// can't be had.
pub struct Item(pub tauri::menu::CheckMenuItem<tauri::Wry>);

/// Register the hotkey once the app is up (`RunEvent::Ready`, after its
/// setup), not as its setup starts: a press then toggled an app whose
/// window was on screen but not yet among its windows (the setup still
/// building the tray, seconds on a loaded machine), so it opened a second
/// one (#295). "arugula: global hotkey …" says it's ready for a press.
pub fn start(app: &AppHandle) {
    if let Err(e) = apply(app, &load(app)) {
        eprintln!("arugula: {e}");
        if let Some(item) = app.try_state::<Item>() {
            let _ = item.0.set_checked(false);
        }
    }
}

/// Our windows' focus, as their `Focused` events tell it: which have it,
/// and when one last lost it.
///
/// Not GTK's own (`is_focused`) alone: on X11 the hotkey's grab takes focus
/// from the window as the key goes down, and the press reaches `toggle` on
/// the hotkey's own thread. GTK can have taken the focus away (so
/// `is_focused` says no) before its event reaches `focus_changed`; a check
/// of when focus was lost made before that saw nothing, and the press
/// brought the focused window forward instead of hiding it (#295: about 1
/// press in 20 on a loaded machine). Here a window keeps focus until its
/// event says it lost it, and then it lost it just now: either way it
/// counts.
#[derive(Default)]
struct Focus {
    windows: std::collections::BTreeSet<String>,
    lost: Option<Instant>,
}

/// How long a window that lost focus still counts as having it.
const JUST: Duration = Duration::from_millis(500);

impl Focus {
    fn changed(&mut self, window: &str, focused: bool, now: Instant) {
        if focused {
            self.windows.insert(window.to_owned());
        } else {
            self.windows.remove(window);
            self.lost = Some(now);
        }
    }

    /// One of our windows has focus, or had it a moment ago.
    fn had(&self, now: Instant) -> bool {
        !self.windows.is_empty() || self.lost.is_some_and(|t| now.saturating_duration_since(t) < JUST)
    }
}

static FOCUS: Mutex<Focus> = Mutex::new(Focus { windows: std::collections::BTreeSet::new(), lost: None });

/// Window `label` of ours gained focus (`focused`) or lost it.
pub fn focus_changed(label: &str, focused: bool) {
    FOCUS.lock().unwrap().changed(label, focused, Instant::now());
}

/// Window `label` is gone: it has no focus to lose.
pub fn window_gone(label: &str) {
    FOCUS.lock().unwrap().windows.remove(label);
}

/// Front, or away when a window of ours already has focus.
fn toggle(app: &AppHandle) {
    let windows = app.webview_windows();
    let had = FOCUS.lock().unwrap().had(Instant::now());
    let focused = windows.values().any(|w| (had || w.is_focused().unwrap_or(false)) && w.is_visible().unwrap_or(false));
    if focused {
        #[cfg(target_os = "macos")]
        {
            let _ = app.hide();
        }
        #[cfg(not(target_os = "macos"))]
        for w in windows.values() {
            let _ = w.hide();
        }
        return;
    }
    #[cfg(target_os = "macos")]
    let _ = app.show();
    let mut shown = false;
    for w in windows.values() {
        let _ = w.unminimize();
        let _ = w.show();
        shown = true;
    }
    match windows.values().next() {
        Some(w) if shown => {
            let _ = w.set_focus();
        }
        _ => crate::focus_or_open(app),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{Focus, Settings};

    #[test]
    fn a_window_has_focus_until_its_event_says_it_lost_it() {
        let t = Instant::now();
        let mut f = Focus::default();
        assert!(!f.had(t));
        f.changed("w0", true, t);
        // The hotkey's grab has taken it, and the event isn't here yet.
        assert!(f.had(t + Duration::from_secs(5)));
        // It's here: it lost focus just now.
        f.changed("w0", false, t + Duration::from_secs(5));
        assert!(f.had(t + Duration::from_millis(5400)));
        assert!(!f.had(t + Duration::from_secs(6)));
        // From one of our windows to another, in either order.
        f.changed("w1", true, t);
        f.changed("w0", false, t);
        assert!(f.had(t + Duration::from_secs(9)));
        f.changed("w0", true, t);
        f.changed("w1", false, t);
        assert!(f.had(t + Duration::from_secs(9)));
        f.windows.remove("w0");
        assert!(!f.had(t + Duration::from_secs(9)));
    }

    #[test]
    fn off_by_default_with_a_default_key() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(!s.hotkey_on);
        assert_eq!(s.keys(), super::DEFAULT_HOTKEY);
        let s: Settings = serde_json::from_str(r#"{"hotkey_on": true, "hotkey": "Super+F12"}"#).unwrap();
        assert!(s.hotkey_on);
        assert_eq!(s.keys(), "Super+F12");
    }

    #[test]
    fn adopts_the_old_apps_settings_once() {
        let base = std::env::temp_dir().join(format!("arugula-adopt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (old, new) = (base.join("wtf.widgets.illogical"), base.join("io.arugula.desktop"));
        assert!(super::adopt(&old, &new).is_empty());
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("desktop.json"), r#"{"hotkey_on":true}"#).unwrap();
        std::fs::write(old.join("cloud.json"), "{}").unwrap();
        assert_eq!(super::adopt(&old, &new), ["desktop.json"]);
        assert_eq!(std::fs::read_to_string(new.join("desktop.json")).unwrap(), r#"{"hotkey_on":true}"#);
        assert!(old.join("desktop.json").is_file() && !new.join("cloud.json").exists());
        // Its own settings win.
        std::fs::write(old.join("daemon.json"), "{}").unwrap();
        std::fs::write(new.join("desktop.json"), "{}").unwrap();
        assert_eq!(super::adopt(&old, &new), ["daemon.json"]);
        assert_eq!(std::fs::read_to_string(new.join("desktop.json")).unwrap(), "{}");
        let _ = std::fs::remove_dir_all(&base);
    }
}
