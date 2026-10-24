//! The app as a client of Arugula control (M48, #159): once this machine
//! is joined, the window is control's own client, with every machine in
//! the account and the team, each reached directly or through the relay,
//! end to end. Before that it's the local page (whose Getting started has
//! the button that joins).
//!
//! Signing in happens in the person's browser, where passkeys work (an
//! app's webview has none, except a macOS build associated with control's
//! site, which may also use them in the window, D2, `passkeys_on`): the
//! app makes a verifier it keeps,
//! listens on a loopback port, asks control for a ticket, and opens
//! control's page on it. Once the person allows it there, their browser
//! hands a one-time grant to that port (so only the app on the computer
//! the browser runs on hears it), and the window opens control's page with
//! the grant and the verifier, which signs the window in (control's
//! `app_login.rs`). Nothing to compare: the loopback is the check (#758).
//!
//! The app's sign-in page also makes the window's device keys and sends
//! their public half with the ask. A browser that's one of the account's
//! devices signs the app's certificate when it allows the sign-in, and hands
//! it over with the grant; the window then takes the keys and the
//! certificate, and is a device at once. Without a certificate it's a new
//! device, which a trusted device approves on control's page as any browser
//! is. All of that is control's page's own code; this module only routes
//! the window and runs the ticket.
//!
//! #326: the machine's join comes first, and is its own approval (a code,
//! approved on a device the person uses, where the team is picked). A
//! machine control dropped (#325) counts as not joined here: the window
//! goes to the daemon's page, whose Getting started joins it again, not to
//! a sign-in that would only approve the app. Once it's joined, signing
//! the app in is an optional second step, and its page says so and why.

use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

/// The app's own sign-in page.
pub const SIGNIN: &str = "signin.html";

fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).build().into())
}

/// What the local daemon says about itself, read when a window opens.
#[derive(Clone, Default)]
pub struct Local {
    /// This machine's name, for the device ("Arugula app on jake-air").
    pub name: String,
    /// The control it's joined to (`ARUGULA_CONTROL` overrides, for tests).
    /// None while control has dropped it (#325, #326): it isn't in.
    pub control: Option<String>,
    /// Where it's joined, as the daemon's `control_state` says: "the team
    /// arugula", "lex00's account".
    pub place: Option<String>,
}

/// Whether `/api/host`'s `control_state` says control dropped this
/// machine (#325): what it saved doesn't count.
fn dropped(host: &Value) -> bool {
    host["control_state"]["state"].as_str() == Some("dropped")
}

/// Where `/api/host`'s `control_state` says this machine is joined.
fn place(host: &Value) -> Option<String> {
    let s = &host["control_state"];
    if s["state"].as_str() != Some("joined") {
        return None;
    }
    let name = s["name"].as_str().filter(|n| !n.is_empty());
    Some(match (s["kind"].as_str(), name) {
        (Some("team"), Some(n)) => format!("the team {n}"),
        (Some("team"), None) => "a team".into(),
        (_, Some(n)) => format!("{n}'s account"),
        (_, None) => "your account".into(),
    })
}

static LOCAL: Mutex<Option<Local>> = Mutex::new(None);
/// "Just this machine" for the rest of this run.
static LOCAL_ONLY: Mutex<bool> = Mutex::new(false);

pub fn local() -> Local {
    let read = || -> Option<Local> {
        let mut req = agent().get(&format!("{}/api/host", crate::page()));
        if let Some(b) = crate::bearer() {
            req = req.header("Authorization", &b);
        }
        let v: Value = req.call().ok()?.body_mut().read_json().ok()?;
        let control = std::env::var("ARUGULA_CONTROL")
            .ok()
            .or_else(|| v["control"].as_str().map(str::to_owned))
            .map(|c| c.trim_end_matches('/').to_owned())
            .filter(|_| !dropped(&v));
        Some(Local { name: v["name"].as_str().unwrap_or("this machine").to_owned(), control, place: place(&v) })
    };
    match read() {
        Some(l) => {
            *LOCAL.lock().unwrap() = Some(l.clone());
            l
        }
        None => LOCAL.lock().unwrap().clone().unwrap_or_default(),
    }
}

/// The control the window may show, from what was last read.
pub fn control() -> Option<String> {
    LOCAL.lock().unwrap().as_ref().and_then(|l| l.control.clone())
}

/// What control's `/control.json` said, for the control it was read from:
/// its block domain (M50), whose block origins stay in the window
/// (`frames.rs`), and its platform page's origin (M157), which does too
/// (`platform.rs`).
struct Said {
    control: String,
    domain: Option<String>,
    platform: Option<tauri::Url>,
    read: bool,
    at: Instant,
}

static SAID: Mutex<Option<Said>> = Mutex::new(None);

/// How often control's `/control.json` is read again once read, and tried
/// again when it couldn't be.
const SAID_REREAD: Duration = Duration::from_secs(10 * 60);
const SAID_RETRY: Duration = Duration::from_secs(30);

/// The block domain of the control the window may show, if it said one.
pub fn block_domain() -> Option<String> {
    let c = control()?;
    SAID.lock().unwrap().as_ref().filter(|b| b.control == c).and_then(|b| b.domain.clone())
}

/// The platform page's origin of the control the window may show, if it
/// said one.
pub fn platform() -> Option<tauri::Url> {
    platform_of(&control()?)
}

/// The platform page's origin `control` said, if it's the control last
/// read: for the one a machine just left, too, until another is read.
pub fn platform_of(control: &str) -> Option<tauri::Url> {
    SAID.lock().unwrap().as_ref().filter(|b| b.control == control).and_then(|b| b.platform.clone())
}

/// Read control's `/control.json` when it isn't known for this control
/// yet, or it's time to again. Blocks on the network: for a thread of its
/// own (`follow_join`'s), never a navigation's.
pub fn learn_control_json() {
    let Some(c) = control() else { return };
    let due = match &*SAID.lock().unwrap() {
        Some(b) if b.control == c => b.at.elapsed() >= if b.read { SAID_REREAD } else { SAID_RETRY },
        _ => true,
    };
    if !due {
        return;
    }
    let said =
        agent().get(&format!("{c}/control.json")).call().ok().and_then(|mut r| r.body_mut().read_json::<Value>().ok());
    let mut known = SAID.lock().unwrap();
    let before = known.as_ref().filter(|b| b.control == c);
    let (was_domain, was_platform) = (before.and_then(|b| b.domain.clone()), before.and_then(|b| b.platform.clone()));
    let (domain, platform) = match &said {
        Some(v) => (
            v["block_domain"].as_str().and_then(crate::frames::block_domain),
            v["platform"].as_str().and_then(crate::platform::origin),
        ),
        // Not reached this time: what it said before stands.
        None => (was_domain.clone(), was_platform.clone()),
    };
    if said.is_some() && domain != was_domain {
        eprintln!("arugula: {c}'s block domain is {}", domain.as_deref().unwrap_or("none"));
    }
    if said.is_some() && platform != was_platform {
        eprintln!("arugula: {c}'s platform page is {}", platform.as_ref().map_or("none", |p| p.as_str()));
    }
    *known = Some(Said { control: c, domain, platform, read: said.is_some(), at: Instant::now() });
}

/// The app's name as a device: "Arugula app on geek". An AppImage whose
/// WebKitGTK is older than the profile's keeps a profile of its own
/// (`profile.rs`), so it's a second device beside the package on the same
/// machine: "Arugula app (AppImage) on geek" (#817).
pub fn device_name() -> String {
    let name = LOCAL.lock().unwrap().as_ref().map(|l| l.name.clone()).unwrap_or_default();
    let own_profile = std::env::var_os("APPIMAGE").is_some() && crate::profile::dir().is_some();
    name_for(&name, own_profile)
}

fn name_for(machine: &str, appimage: bool) -> String {
    let app = if appimage { "Arugula app (AppImage)" } else { "Arugula app" };
    if machine.is_empty() { app.into() } else { format!("{app} on {machine}") }
}

pub fn set_local_only(on: bool) {
    *LOCAL_ONLY.lock().unwrap() = on;
}

pub fn local_only() -> bool {
    *LOCAL_ONLY.lock().unwrap()
}

/// Beside the window's profile when it has one of its own (`profile.rs`):
/// its sessions are its own.
fn state_file(app: &AppHandle) -> Option<PathBuf> {
    crate::profile::dir().or_else(|| app.path().app_config_dir().ok()).map(|d| d.join("cloud.json"))
}

/// Whether the window holds a session on `control` (as far as the app
/// knows: control's page says so if it lapsed, and the window comes back
/// here).
pub fn signed_in(app: &AppHandle, control: &str) -> bool {
    state_file(app)
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .is_some_and(|v| v["signed_in"][control].as_bool() == Some(true))
}

pub fn set_signed_in(app: &AppHandle, control: &str, on: bool) {
    let Some(f) = state_file(app) else { return };
    let mut v: Value =
        std::fs::read(&f).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| json!({}));
    v["signed_in"][control] = json!(on);
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, serde_json::to_vec_pretty(&v).unwrap_or_default());
}

/// The app's own page, as a URL to navigate a window to.
pub fn app_url(page: &str) -> tauri::Url {
    let base = if cfg!(windows) { "http://tauri.localhost/" } else { "tauri://localhost/" };
    format!("{base}{page}").parse().unwrap()
}

/// Whether a window showing `url` moves to the new home when the daemon's
/// control goes from `was` to `now` (#204). Joined: the daemon's own page
/// (`daemon_page`) gives way to the app's sign-in, or control's page once
/// signed in. Left, or joined elsewhere: the old control's pages, its
/// platform page (`was_platform`, M157) and the sign-in give way. Anything
/// else (the setup page, another window's choice) stays.
pub fn moves(
    was: Option<&str>,
    was_platform: Option<&tauri::Url>,
    now: Option<&str>,
    url: &tauri::Url,
    daemon_page: bool,
) -> bool {
    if was == now {
        return false;
    }
    match was {
        None => daemon_page,
        Some(old) => {
            let on_old = old.parse::<tauri::Url>().is_ok_and(|c| c.origin() == url.origin())
                || crate::platform::is_platform(url, was_platform);
            let signin = matches!(url.host_str(), Some("localhost" | "tauri.localhost"))
                && matches!(url.scheme(), "tauri" | "http")
                && url.path() == format!("/{SIGNIN}");
            on_old || signin
        }
    }
}

/// Whether `url` is control's own sign-in, which can't finish in the
/// window (GitHub's pages, passkeys): the app's sign-in takes over.
pub fn is_control_signin(url: &tauri::Url) -> bool {
    let Some(c) = control() else { return false };
    url.as_str().starts_with(&format!("{c}/auth/github"))
}

/// The app's own window dragging (#316), on every page in its windows.
///
/// The window's titlebar is the page's (an overlay titlebar on macOS, none
/// on Linux), so a page that doesn't mark its bar as a drag region (an
/// older daemon's, an error page, the app's own pages) left the window
/// stuck. A primary-button press in the top strip (the bar's height, or
/// on macOS the titlebar and native tab bar's, `--native-tabs`, #323) on
/// nothing interactive moves the window (on macOS at once; elsewhere once
/// the mouse moves with the button held); a double-click there zooms it
/// (on mouseup on macOS, unless the mouse moved, as AppKit does). Where
/// the page marks its own regions (`data-tauri-drag-region`, Tauri's drag
/// script, which runs first) this stays out, so nothing is handled twice.
const DRAG_SCRIPT: &str = r#"(() => {
  const STRIP = 36;
  const macos = __OS__ === 'macos';
  const CLICKABLE = new Set(['A', 'BUTTON', 'INPUT', 'SELECT', 'TEXTAREA', 'LABEL', 'SUMMARY', 'OPTION', 'VIDEO', 'AUDIO', 'IFRAME']);
  const ROLES = new Set(['button', 'link', 'menuitem', 'tab', 'checkbox', 'radio', 'switch', 'option', 'slider', 'textbox']);
  // With AppKit's tab bar showing, the app sets --native-tabs (#323).
  const strip = () => {
    const tabs = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--native-tabs')) || 0;
    return Math.max(STRIP, tabs);
  };
  // Whether a press at this path moves the window: not on anything
  // interactive, nor where the page handles dragging itself.
  const ours = (path) => {
    for (const el of path) {
      if (!(el instanceof HTMLElement)) continue;
      if (el === document.body || el === document.documentElement) return true;
      if (el.hasAttribute('data-tauri-drag-region')) return false;
      if (CLICKABLE.has(el.tagName) || ROLES.has(el.getAttribute('role') || '') || el.isContentEditable) return false;
      if (el.hasAttribute('tabindex') && el.getAttribute('tabindex') !== '-1') return false;
      if (el.classList.contains('tab') || el.classList.contains('xterm')) return false;
      const s = getComputedStyle(el);
      if ((s.getPropertyValue('app-region') || s.getPropertyValue('-webkit-app-region')) === 'no-drag') return false;
      if (s.cursor === 'pointer' || s.cursor === 'text') return false;
    }
    return true;
  };
  const invoke = (cmd) => window.__TAURI_INTERNALS__?.invoke('plugin:window|' + cmd).catch(() => {});
  let down = null;
  let press = null;
  addEventListener('mousedown', (e) => {
    down = null;
    press = null;
    if (e.defaultPrevented || e.button !== 0 || e.clientY >= strip()) return;
    if (!(e.detail === 1 || e.detail === 2) || !ours(e.composedPath())) return;
    if (macos && e.detail === 2) {
      down = [e.clientX, e.clientY];
      return;
    }
    e.preventDefault();
    if (e.detail === 2) invoke('internal_toggle_maximize');
    else if (macos) invoke('start_dragging');
    // Elsewhere the move starts once the mouse moves with the button
    // held (#316): on mousedown, the window manager's move often begins
    // after the button is up (the invoke is async), and then it takes
    // the next click, so a double-click never reached the page.
    else press = [e.clientX, e.clientY];
  });
  if (!macos) {
    addEventListener('mousemove', (e) => {
      if (!press) return;
      if (!(e.buttons & 1)) {
        press = null;
      } else if (e.clientX !== press[0] || e.clientY !== press[1]) {
        press = null;
        invoke('start_dragging');
      }
    });
    addEventListener('mouseup', () => (press = null));
  }
  if (macos) {
    addEventListener('mouseup', (e) => {
      const at = down;
      down = null;
      if (at && e.button === 0 && e.detail === 2 && e.clientX === at[0] && e.clientY === at[1]) {
        invoke('internal_toggle_maximize');
      }
    });
  }
})();"#;

/// Whether the window can use passkeys on `control`'s page (D2). A
/// WKWebView in an app that isn't a browser gets WebAuthn only for a
/// relying party the app is associated with: so on macOS, only a build
/// signed with `webcredentials:<control's host>` in its associated-domains
/// entitlement (a Developer ID build with its provisioning profile,
/// scripts/macos-sign), whose control names the app back (control's
/// `apple_site.rs`). Linux's and Windows' windows offer none.
fn passkeys_on(control: &str) -> bool {
    associated_with(control, associated_domains())
}

/// Whether `domains` (an associated-domains entitlement's) associate the
/// app with `control`'s host for its passkeys.
fn associated_with(control: &str, domains: &[String]) -> bool {
    let Some(host) = control.parse::<tauri::Url>().ok().and_then(|u| u.host_str().map(str::to_owned)) else {
        return false;
    };
    domains.iter().any(|d| *d == format!("webcredentials:{host}"))
}

/// This app's associated domains, as its signature claims them (macOS).
#[cfg(target_os = "macos")]
fn associated_domains() -> &'static [String] {
    use objc2_core_foundation::{CFArray, CFRetained, CFString, CFType};
    use objc2_security::SecTask;
    static D: OnceLock<Vec<String>> = OnceLock::new();
    D.get_or_init(|| {
        // SAFETY: a task for this process, asked for one entitlement, with
        // no error out; an array's elements are CF objects.
        unsafe {
            let Some(task) = SecTask::from_self(None) else { return Vec::new() };
            let key = CFString::from_static_str("com.apple.developer.associated-domains");
            let Some(v) = task.value_for_entitlement(&key, std::ptr::null_mut()) else { return Vec::new() };
            let Ok(a) = v.downcast::<CFArray>() else { return Vec::new() };
            let a: CFRetained<CFArray<CFType>> = CFRetained::cast_unchecked(a);
            a.iter().filter_map(|d| d.downcast_ref::<CFString>().map(|s| s.to_string())).collect()
        }
    })
}

#[cfg(not(target_os = "macos"))]
fn associated_domains() -> &'static [String] {
    &[]
}

/// Script for every page in the app's windows: the name control's page
/// gives this device, no passkey button where passkeys can't work
/// (`passkeys_on`), and window dragging (`DRAG_SCRIPT`).
pub fn init_script() -> String {
    let control = control().unwrap_or_default();
    let hide = !passkeys_on(&control);
    // Debug builds only: a test's script for the page, to drive the window
    // where nothing else can.
    #[cfg(debug_assertions)]
    let test =
        std::env::var_os("ARUGULA_TEST_SCRIPT").and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    #[cfg(not(debug_assertions))]
    let test = String::new();
    let s = format!(
        "window.__arugulaApp = {{ name: {}, platform: {:?} }};\n\
         if ({hide} && {control:?} && location.origin === new URL({control:?}).origin) {{\n\
           addEventListener('DOMContentLoaded', () => {{\n\
             const s = document.createElement('style');\n\
             s.textContent = '[data-signin=passkey] {{ display: none !important; }}';\n\
             document.head.appendChild(s);\n\
           }});\n\
         }}",
        serde_json::to_string(&device_name()).unwrap(),
        if cfg!(target_os = "macos") { "macos" } else { "linux" },
    );
    // Windows keeps its system titlebar.
    let drag = if cfg!(any(target_os = "macos", target_os = "linux")) {
        DRAG_SCRIPT.replace("__OS__", if cfg!(target_os = "macos") { "'macos'" } else { "'linux'" })
    } else {
        String::new()
    };
    #[cfg(debug_assertions)]
    if !test.is_empty() {
        eprintln!("arugula: a test script for the page ({} bytes)", test.len());
    }
    s + "\n" + &drag + "\n" + &test
}

#[derive(serde::Serialize)]
pub struct Status {
    control: Option<String>,
    /// The app's name as a device ("Arugula app on jake-air").
    name: String,
    /// This machine's name.
    machine: String,
    /// Where it's joined ("the team arugula"), when the daemon says.
    place: Option<String>,
    /// `ARUGULA_SIGNIN_AUTO=1` (for tests): start signing in on load.
    auto: bool,
}

#[tauri::command]
pub fn cloud_status() -> Status {
    let local = LOCAL.lock().unwrap().clone().unwrap_or_default();
    Status {
        control: control(),
        name: device_name(),
        machine: local.name,
        place: local.place,
        auto: std::env::var_os("ARUGULA_SIGNIN_AUTO").is_some(),
    }
}

#[derive(serde::Serialize)]
pub struct Started {
    /// A control from before #758 still has the person compare a code.
    code: Option<String>,
    url: String,
}

/// The window's device keys, made by the sign-in page (WebCrypto): the
/// public halves (raw) and the private ones (PKCS#8), hex. The private
/// halves only go to control's page in this window, in the redeem link's
/// fragment, and only with a certificate for them.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Keys {
    noise: String,
    sign: String,
    secret_noise: String,
    secret_sign: String,
}

impl Keys {
    fn ok(&self) -> bool {
        let hex = |s: &str, n: std::ops::RangeInclusive<usize>| {
            n.contains(&s.len())
                && s.len().is_multiple_of(2)
                && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
        };
        // PKCS#8 is 48 bytes, or more where an engine adds the public key.
        hex(&self.noise, 64..=64)
            && hex(&self.sign, 64..=64)
            && hex(&self.secret_noise, 96..=512)
            && hex(&self.secret_sign, 96..=512)
    }
}

/// Start signing in: a verifier and a loopback port, a ticket from
/// control, its page in the browser, and a listener that sends `window` to
/// redeem it once the person's browser hands over the grant.
#[tauri::command]
pub async fn cloud_signin(app: AppHandle, window: tauri::WebviewWindow, keys: Option<Keys>) -> Result<Started, String> {
    use sha2::{Digest, Sha256};
    let control = control().ok_or("this machine isn't joined to Arugula cloud")?;
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| e.to_string())?;
    let verifier = hex(&secret);
    let challenge = hex(&Sha256::digest(verifier.as_bytes()));
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| format!("can't listen for the sign-in: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let keys = keys.filter(Keys::ok);
    let mut ask = json!({ "name": device_name(), "challenge": challenge, "port": port, "features": ["no-code"] });
    if let Some(k) = &keys {
        ask["device"] = json!({ "noise": k.noise, "sign": k.sign });
    }
    let c = control.clone();
    let t: Value = tauri::async_runtime::spawn_blocking(move || {
        let mut r = agent()
            .post(&format!("{c}/auth/app"))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(ask)
            .map_err(|e| format!("can't reach {c}: {e}"))?;
        let ok = r.status().is_success();
        let v = r.body_mut().read_json::<Value>().map_err(|e| e.to_string())?;
        if !ok {
            return Err(v["error"].as_str().unwrap_or("control refused the sign-in").to_owned());
        }
        Ok(v)
    })
    .await
    .map_err(|e| e.to_string())??;
    let get = |k: &str| t[k].as_str().map(str::to_owned).ok_or(format!("control's answer has no {k}"));
    let (id, url) = (get("ticket")?, get("url")?);
    let code = t["code"].as_str().map(str::to_owned);
    if let Ok(u) = url.parse::<tauri::Url>() {
        crate::open_outside(&u);
    }
    let label = window.label().to_owned();
    std::thread::spawn(move || {
        let Some((grant, cert)) = await_grant(&listener, &id, &control) else { return };
        let redeem = match (cert, &keys) {
            // Signed by the browser that allowed it: the window takes its
            // keys, and is a device at once.
            (Some(cert), Some(k)) => {
                format!("{control}/#app-redeem={id}.{grant}.{verifier}.{}.{}.{cert}", k.secret_noise, k.secret_sign)
            }
            _ => format!("{control}/#app-redeem={id}.{grant}.{verifier}"),
        };
        let redeem: tauri::Url = redeem.parse().unwrap();
        set_signed_in(&app, &control, true);
        let a = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(w) = a.get_webview_window(&label) {
                let _ = w.navigate(redeem);
                let _ = w.set_focus();
            }
        });
    });
    Ok(Started { code, url })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The grant for ticket `id`, when the person's browser brings it to the
/// loopback port (`GET /arugula-signin?ticket=…&grant=…[&cert=…]`), and
/// the certificate it signed for the window's keys, if it did (base64url,
/// passed on as it is: control's page in the window checks it); none if
/// ten minutes pass. The browser goes back to control's page.
fn await_grant(listener: &std::net::TcpListener, id: &str, control: &str) -> Option<(String, Option<String>)> {
    use std::io::{BufRead, BufReader, Write};
    let deadline = std::time::Instant::now() + Duration::from_secs(600);
    listener.set_nonblocking(true).ok()?;
    while std::time::Instant::now() < deadline {
        let conn = match listener.accept() {
            Ok((c, _)) => c,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(_) => return None,
        };
        let _ = conn.set_nonblocking(false);
        let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
        let mut line = String::new();
        let mut r = BufReader::new(&conn);
        if r.read_line(&mut line).is_err() {
            continue;
        }
        let target = line.split_whitespace().nth(1).unwrap_or_default();
        let query = target.strip_prefix("/arugula-signin?").unwrap_or_default();
        let param = |k: &str| {
            query.split('&').find_map(|kv| kv.strip_prefix(k).and_then(|v| v.strip_prefix('='))).map(str::to_owned)
        };
        let ok = param("ticket").as_deref() == Some(id);
        let grant = param("grant").filter(|g| ok && !g.is_empty() && g.bytes().all(|c| c.is_ascii_hexdigit()));
        let reply = match &grant {
            Some(_) => format!(
                "HTTP/1.1 303 See Other\r\nLocation: {control}/#app-done\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ),
            None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
        };
        let _ = (&conn).write_all(reply.as_bytes());
        if let Some(grant) = grant {
            let cert = param("cert").filter(|c| {
                !c.is_empty()
                    && c.len() <= 8192
                    && c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            });
            return Some((grant, cert));
        }
    }
    None
}

/// "Just this machine": the local page, for the rest of this run.
#[tauri::command]
pub fn cloud_local(window: tauri::WebviewWindow) -> Result<(), String> {
    set_local_only(true);
    window.navigate(crate::page_at("/")).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> tauri::Url {
        s.parse().unwrap()
    }

    /// Passkeys only on the control the app is associated with, for
    /// webcredentials (D2); none here, where the app claims no domains.
    #[test]
    fn passkeys_only_where_the_app_is_associated() {
        let d = ["applinks:control.arugula.io".to_owned(), "webcredentials:control.arugula.io".to_owned()];
        assert!(associated_with("https://control.arugula.io", &d));
        assert!(!associated_with("https://control.example.com", &d));
        assert!(!associated_with("https://control.arugula.io", &d[..1]));
        assert!(!associated_with("", &d));
        let script = init_script();
        assert!(script.contains("if (true && "), "the button stays hidden: {script}");
    }

    #[test]
    fn an_appimage_with_its_own_profile_says_so() {
        assert_eq!(name_for("geek", false), "Arugula app on geek");
        assert_eq!(name_for("geek", true), "Arugula app (AppImage) on geek");
        assert_eq!(name_for("", false), "Arugula app");
        assert_eq!(name_for("", true), "Arugula app (AppImage)");
    }

    #[test]
    fn a_join_moves_the_daemons_page_and_a_leave_moves_controls() {
        let c = Some("https://control.test");
        let daemon = url("http://127.0.0.1:7681/");
        // Joined while the window showed the daemon's page: it moves.
        assert!(moves(None, None, c, &daemon, true));
        // Nothing changed: nothing moves.
        assert!(!moves(c, None, c, &url("https://control.test/"), false));
        assert!(!moves(None, None, None, &daemon, true));
        // The setup page and other sites stay put.
        assert!(!moves(None, None, c, &url("tauri://localhost/index.html"), false));
        assert!(!moves(None, None, c, &url("https://example.com/"), false));
        // Left: control's page and the app's sign-in go back home.
        assert!(moves(c, None, None, &url("https://control.test/#x"), false));
        assert!(moves(c, None, None, &app_url(SIGNIN), false));
        assert!(moves(c, None, None, &url("http://tauri.localhost/signin.html"), false));
        assert!(!moves(c, None, None, &daemon, true), "already home");
        // Joined elsewhere: off the old control.
        assert!(moves(c, None, Some("https://other.test"), &url("https://control.test/"), false));
        // Its platform page (M157) too, when it said one; other sites stay.
        let p = crate::platform::origin("https://app.test");
        assert!(moves(c, p.as_ref(), None, &url("https://app.test/p/x"), false));
        assert!(moves(c, p.as_ref(), Some("https://other.test"), &url("https://app.test/"), false));
        assert!(!moves(c, None, None, &url("https://app.test/"), false), "a platform it never said");
        assert!(!moves(c, p.as_ref(), None, &url("https://x.app.test/"), false));
        assert!(!moves(c, p.as_ref(), c, &url("https://app.test/"), false), "nothing changed");
    }

    /// #326: a machine control dropped isn't joined, for where the window
    /// goes: the daemon's page joins it again, not a sign-in that would
    /// only approve the app. Joined, the sign-in page says where.
    #[test]
    fn a_dropped_machine_is_not_joined_and_a_joined_one_says_where() {
        let dropped_host = json!({
            "control": "https://control.test",
            "control_state": { "state": "dropped", "url": "https://control.test", "kind": "team", "name": "arugula" },
        });
        assert!(dropped(&dropped_host));
        assert_eq!(place(&dropped_host), None);
        // An older daemon says nothing about its standing: as before.
        assert!(!dropped(&json!({ "control": "https://control.test" })));

        let team =
            json!({ "control_state": { "state": "joined", "kind": "team", "name": "arugula", "connected": true } });
        assert!(!dropped(&team));
        assert_eq!(place(&team).as_deref(), Some("the team arugula"));
        let mine = json!({ "control_state": { "state": "joined", "kind": "account", "name": "lex00" } });
        assert_eq!(place(&mine).as_deref(), Some("lex00's account"));
        let unnamed = json!({ "control_state": { "state": "joined", "kind": "account", "name": "" } });
        assert_eq!(place(&unnamed).as_deref(), Some("your account"));
        assert_eq!(place(&json!({ "control_state": { "state": "not_joined" } })), None);

        // Dropped then joined again: the window moves from the daemon's
        // page to the sign-in (or control's page), as for a first join.
        let daemon = url("http://127.0.0.1:7681/");
        assert!(moves(None, None, Some("https://control.test"), &daemon, true));
        // And on the drop, off control's page to the daemon's.
        assert!(moves(Some("https://control.test"), None, None, &url("https://control.test/"), false));
    }

    #[test]
    fn the_window_keys_are_taken_only_when_they_are_hex_of_the_right_size() {
        let k = |noise: &str, secret: &str| Keys {
            noise: noise.into(),
            sign: "b".repeat(64),
            secret_noise: secret.into(),
            secret_sign: "c".repeat(96),
        };
        assert!(k(&"a".repeat(64), &"d".repeat(96)).ok());
        assert!(!k(&"a".repeat(63), &"d".repeat(96)).ok(), "a public key is 32 bytes");
        assert!(!k(&"A".repeat(64), &"d".repeat(96)).ok(), "lowercase hex only");
        assert!(!k(&"a".repeat(64), &"d".repeat(40)).ok(), "too short for PKCS#8");
        assert!(!k(&"a".repeat(64), "d.e").ok(), "nothing that would break the redeem link");
    }

    #[test]
    fn the_sign_in_challenge_is_the_lowercase_hex_of_the_verifiers_sha256() {
        use sha2::{Digest, Sha256};
        assert_eq!(hex(&[0x00, 0x0f, 0xab, 0xff]), "000fabff");
        assert_eq!(hex(&Sha256::digest(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
