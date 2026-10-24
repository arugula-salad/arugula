//! What control would push this device, as native notifications while the
//! app runs. A webview has no `PushManager` (S25), so control's page in the
//! window can't take Web Push; *Notify this device* there turns this on
//! instead.
//!
//! The page subscribes the way a browser does (`push.ts`, control's
//! `push::subscribe`), signed by the window's device key, with keys this
//! module makes and keeps; only the endpoint is control's own live one
//! (control's `push_live.rs`). So everything that would reach a phone
//! reaches the app, under the same Push choices: chat, the machines' panes
//! (encrypted by each daemon for this subscription alone) and projects, and
//! the account's own notices. Control answers the subscription with a
//! listen token, which the page hands here.
//!
//! The app holds the stream, not the page: it outlives the window (on
//! macOS, closing the last one keeps the app running), a move to the
//! platform page, and a second window, which would each have notified
//! again. Nothing arrives while the app isn't running: on Linux and
//! Windows, closing its last window quits it (unless the global hotkey
//! keeps it in the tray).
//!
//! A click opens what the service worker's would (`sw.ts`, `tap.ts`): a
//! pane on its machine, a room, an issue, a run, a deploy, a workspace, or
//! control's page for what waits there. A pane that needs you on this
//! machine's own daemon is the daemon's notification already (`watch` in
//! main.rs): its relayed copy is dropped here.

use std::{
    io::BufRead,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

/// Where control's live endpoints are.
const LIVE: &str = "/api/push/live/";

/// What's kept between runs (`notices.json` in the app's config
/// directory): the subscription and its private half.
#[derive(Clone, Serialize, Deserialize)]
struct Kept {
    /// Control's origin.
    control: String,
    endpoint: String,
    listen: String,
    /// The P-256 key the subscription's `p256dh` is the public half of, in
    /// hex, and its `auth` secret (base64url).
    key: String,
    auth: String,
}

/// Keys made for a subscription the page hasn't finished yet.
struct Pending {
    id: String,
    key: String,
    auth: String,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
/// Bumped to stop the stream that's running.
static RUN: AtomicU64 = AtomicU64::new(0);

fn file(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_config_dir().ok()?.join("notices.json"))
}

fn load(app: &AppHandle) -> Option<Kept> {
    serde_json::from_str(&std::fs::read_to_string(file(app)?).ok()?).ok()
}

fn save(app: &AppHandle, k: &Kept) -> Result<(), String> {
    write_private(&file(app).ok_or("no config directory")?, serde_json::to_string_pretty(k).unwrap().as_bytes())
}

/// `bytes` at `f`, readable by this user alone from the start (it holds a
/// private key): a new file made 0600, then renamed over the old.
fn write_private(f: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let dir = f.parent().ok_or("no directory")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = f.with_extension(format!("tmp-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let written = o.open(&tmp).and_then(|mut w| {
        // Not 0600 after all (a filesystem that ignores it): refused.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if w.metadata()?.permissions().mode() & 0o077 != 0 {
                return Err(std::io::Error::other("can't make it private"));
            }
        }
        w.write_all(bytes)?;
        w.sync_all()
    });
    match written.and_then(|()| std::fs::rename(&tmp, f)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(format!("saving {}: {e}", f.display()))
        }
    }
}

fn forget(app: &AppHandle) {
    RUN.fetch_add(1, Ordering::SeqCst);
    if let Some(f) = file(app) {
        let _ = std::fs::remove_file(f);
    }
}

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// The origin of the page that asks: control's, or nothing here is its.
fn asker(window: &tauri::WebviewWindow) -> Result<String, String> {
    let page = window.url().map_err(|e| e.to_string())?.origin().ascii_serialization();
    let control =
        crate::cloud::control().and_then(|c| c.parse::<tauri::Url>().ok()).map(|c| c.origin().ascii_serialization());
    if control.as_deref() != Some(page.as_str()) {
        return Err("only control's page turns the app's notifications on".into());
    }
    Ok(page)
}

/// For the page's *Notify this device*: a new key and endpoint id for its
/// subscription, `{ id, p256dh, auth }`, the private half kept here.
#[tauri::command]
pub fn notices_keys(window: tauri::WebviewWindow) -> Result<Value, String> {
    asker(&window)?;
    let key = loop {
        if let Ok(k) = p256::SecretKey::from_slice(&random::<32>()?) {
            break k;
        }
    };
    let auth = B64.encode(random::<16>()?);
    let id = hex(&random::<16>()?);
    let p256dh = B64.encode(key.public_key().to_encoded_point(false).as_bytes());
    *PENDING.lock().unwrap() = Some(Pending { id: id.clone(), key: hex(&key.to_bytes()), auth: auth.clone() });
    Ok(json!({ "id": id, "p256dh": p256dh, "auth": auth }))
}

/// Control took the subscription at `endpoint` and answered `listen`: keep
/// it, and listen from now on, this run and the next.
#[tauri::command]
pub fn notices_start(
    app: AppHandle,
    window: tauri::WebviewWindow,
    endpoint: String,
    listen: String,
) -> Result<(), String> {
    let control = asker(&window)?;
    let p = PENDING.lock().unwrap().take().ok_or("no keys were made for it")?;
    if endpoint != format!("{control}{LIVE}{}", p.id) || listen.is_empty() {
        return Err("not the endpoint these keys were made for".into());
    }
    let kept = Kept { control, endpoint, listen, key: p.key, auth: p.auth };
    save(&app, &kept)?;
    start(&app, kept);
    Ok(())
}

/// Off: the endpoint it listened at, for the page to unsubscribe.
#[tauri::command]
pub fn notices_stop(app: AppHandle, window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    asker(&window)?;
    let was = load(&app).map(|k| k.endpoint);
    forget(&app);
    Ok(was)
}

/// `on` when the app listens for this control's notices, else `off`.
#[tauri::command]
pub fn notices_state(app: AppHandle, window: tauri::WebviewWindow) -> Result<&'static str, String> {
    let control = asker(&window)?;
    Ok(if load(&app).is_some_and(|k| k.control == control) { "on" } else { "off" })
}

/// At launch: listen again, if it was on.
pub fn resume(app: &AppHandle) {
    if let Some(k) = load(app) {
        start(app, k);
    }
}

fn start(app: &AppHandle, k: Kept) {
    let run = RUN.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    let _ = std::thread::Builder::new().name("notices".into()).spawn(move || follow(app, run, k));
}

/// Why a stream stopped.
enum Stop {
    /// Control has no such subscription now (unsubscribed elsewhere, the
    /// device revoked): off, until it's turned on again.
    Gone,
    /// Turned off, or on again with new keys.
    Replaced,
    Failed(String),
}

/// Keep the stream up, reconnecting (with what it missed) as needed.
fn follow(app: AppHandle, run: u64, k: Kept) {
    let mut last: Option<String> = None;
    let mut wait = Duration::from_secs(1);
    loop {
        match listen(&app, run, &k, &mut last) {
            Ok(()) => wait = Duration::from_secs(1),
            Err(Stop::Replaced) => return,
            Err(Stop::Gone) => {
                eprintln!("arugula: control has no notifications for this app now; turned off");
                if RUN.load(Ordering::SeqCst) == run {
                    forget(&app);
                }
                return;
            }
            Err(Stop::Failed(e)) => {
                eprintln!("arugula: control's notifications: {e}");
                wait = (wait * 2).min(Duration::from_secs(60));
            }
        }
        std::thread::sleep(wait);
        if RUN.load(Ordering::SeqCst) != run {
            return;
        }
    }
}

/// One stream, until it ends. Control sends a keep-alive every 25 s; a
/// stream is given 10 minutes, then made again, so one that went dead
/// without a word doesn't stay.
fn listen(app: &AppHandle, run: u64, k: &Kept, last: &mut Option<String>) -> Result<(), Stop> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(600)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut req = agent
        .get(&k.endpoint)
        .header("Authorization", &format!("Bearer {}", k.listen))
        .header("Accept", "text/event-stream");
    if let Some(id) = last.as_deref() {
        req = req.header("Last-Event-ID", id);
    }
    let res = req.call().map_err(|e| Stop::Failed(e.to_string()))?;
    match res.status().as_u16() {
        200 => {}
        401 | 403 | 404 => return Err(Stop::Gone),
        s => return Err(Stop::Failed(format!("HTTP {s}"))),
    }
    let mut events = Events::default();
    for line in std::io::BufReader::new(res.into_body().into_reader()).lines() {
        if RUN.load(Ordering::SeqCst) != run {
            return Err(Stop::Replaced);
        }
        let Ok(line) = line else { return Ok(()) };
        let Some((id, data)) = events.line(&line) else { continue };
        if id.is_some() {
            *last = id;
        }
        match open(k, &data) {
            Some(msg) => shown(app, k, &msg),
            None => eprintln!("arugula: a notification from control that doesn't open with this app's key"),
        }
    }
    Ok(())
}

/// Server-Sent Events, line by line: an event's id and data at its blank
/// line.
#[derive(Default)]
struct Events {
    id: Option<String>,
    data: Vec<String>,
}

impl Events {
    fn line(&mut self, line: &str) -> Option<(Option<String>, String)> {
        if line.is_empty() {
            let data = std::mem::take(&mut self.data);
            let id = self.id.take();
            return (!data.is_empty()).then(|| (id, data.join("\n")));
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "data" => self.data.push(value.to_owned()),
            "id" => self.id = Some(value.to_owned()),
            _ => {}
        }
        None
    }
}

/// A notice as its push service would have been handed it (aes128gcm,
/// RFC 8291, base64url), opened with the subscription's key.
fn open(k: &Kept, data: &str) -> Option<Value> {
    use aes_gcm::{Aes128Gcm, KeyInit, aead::Aead};
    use hkdf::Hkdf;
    use sha2::Sha256;

    let body = B64.decode(data.trim()).ok()?;
    let ua = p256::SecretKey::from_slice(&unhex(&k.key)?).ok()?;
    let auth = B64.decode(&k.auth).ok()?;
    if body.len() < 21 {
        return None;
    }
    let (salt, rest) = body.split_at(16);
    let id_len = rest[4] as usize;
    let (as_public, sealed) = rest.get(5..)?.split_at_checked(id_len)?;
    let ua_public = ua.public_key().to_encoded_point(false);
    let shared = p256::ecdh::diffie_hellman(
        ua.to_nonzero_scalar(),
        p256::PublicKey::from_sec1_bytes(as_public).ok()?.as_affine(),
    );
    let mut info = b"WebPush: info\0".to_vec();
    info.extend_from_slice(ua_public.as_bytes());
    info.extend_from_slice(as_public);
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&auth), shared.raw_secret_bytes().as_ref()).expand(&info, &mut ikm).ok()?;
    let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).ok()?;
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce).ok()?;
    let mut plain = Aes128Gcm::new_from_slice(&cek).ok()?.decrypt(&nonce.into(), sealed).ok()?;
    // The last record's delimiter, after any padding.
    while plain.last() == Some(&0) {
        plain.pop();
    }
    if plain.pop() != Some(2) {
        return None;
    }
    serde_json::from_slice(&plain).ok()
}

/// This machine's own daemon's device on control, from its `control.json`
/// (only its public certificate: the key is elsewhere).
fn local_daemon() -> Option<String> {
    let f = crate::state_file("control.json")?;
    let v: Value = serde_json::from_str(&std::fs::read_to_string(f).ok()?).ok()?;
    v["cert"]["device"].as_str().map(str::to_owned)
}

fn shown(app: &AppHandle, k: &Kept, msg: &Value) {
    let pane = msg["pane"].as_u64().and_then(|p| u32::try_from(p).ok());
    // This machine's pane that needs you: the daemon's own notification.
    if let Some(p) = pane
        && msg["daemon"].as_str().is_some_and(|d| Some(d) == local_daemon().as_deref())
        && crate::needs_you(p)
    {
        return;
    }
    // Its platform page, once control's `/control.json` has said (until
    // then, control's page).
    let platform = crate::cloud::platform_of(&k.control).map(|p| p.origin().ascii_serialization());
    let tap = Tap::of(msg, &k.control, platform.as_deref());
    let title = msg["title"].as_str().filter(|t| !t.is_empty()).unwrap_or("Arugula").to_owned();
    let body = msg["body"].as_str().unwrap_or_default().to_owned();
    crate::notify(app, crate::Click::Notice(tap), title, body);
}

/// Where a click goes: `url`, or for a window already on control's page,
/// `event` (`arugula:notice`'s detail, for main.tsx) without a reload.
#[derive(Clone, Debug, PartialEq)]
pub struct Tap {
    pub url: String,
    pub event: Option<Value>,
}

/// `prefix` and then hex digits, `min` to `max` of them.
fn id(s: Option<&str>, prefix: &str, min: usize, max: usize) -> Option<String> {
    let s = s?;
    let rest = s.strip_prefix(prefix)?;
    ((min..=max).contains(&rest.len()) && rest.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .then(|| s.to_owned())
}

/// A room's id: two letters, `_`, 8 to 32 hex digits.
fn room(v: &Value) -> Option<String> {
    let s = v.as_str()?;
    let (kind, _) = s.split_once('_')?;
    (kind.len() == 2 && kind.bytes().all(|b| b.is_ascii_lowercase())).then_some(())?;
    id(Some(s), &format!("{kind}_"), 8, 32)
}

impl Tap {
    /// As `tap.ts` says for the service worker: the platform page's
    /// (`platformTap`), else a room on control's page (`chatTap`), else
    /// control's page for its own notices, else the pane (`tapUrl`).
    pub fn of(msg: &Value, control: &str, platform: Option<&str>) -> Tap {
        let control = control.trim_end_matches('/');
        let seq = |v: &Value| v.as_i64().map(|n| n.to_string());
        if msg["control"] == true {
            if let (Some(p), Some(path)) = (platform, platform_path(msg)) {
                return Tap { url: format!("{p}{path}"), event: None };
            }
            if let Some(r) = room(&msg["chat"]["room"]) {
                let at = seq(&msg["chat"]["seq"]).map(|n| format!("&msg={n}")).unwrap_or_default();
                let hash = format!("chat={r}{at}");
                return Tap { url: format!("{control}/#{hash}"), event: Some(json!({ "hash": hash })) };
            }
            return Tap { url: format!("{control}/"), event: Some(json!({ "refresh": true })) };
        }
        let Some(pane) = msg["pane"].as_u64().filter(|p| *p > 0) else {
            return Tap { url: format!("{control}/"), event: None };
        };
        let card = !msg["reason"].is_null();
        let daemon = msg["daemon"].as_str().filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_hexdigit()));
        let at = daemon.map(|d| format!("{d}.{pane}")).unwrap_or_else(|| pane.to_string());
        Tap {
            url: format!("{control}/#{}={at}", if card { "swarm" } else { "pane" }),
            event: Some(json!({ "pane": pane, "daemon": daemon, "card": card })),
        }
    }

    /// Open it: in a window already on control's page, told (not on Linux,
    /// where the page loaded at the address shows it, as in `open_pane`);
    /// else in a window on its page's origin, or any, or a new one.
    pub fn open(&self, app: &AppHandle) {
        #[cfg(target_os = "macos")]
        let _ = app.show();
        let Ok(url) = self.url.parse::<tauri::Url>() else { return };
        let on = |u: &tauri::Url| u.origin() == url.origin();
        if let Some(ev) = &self.event
            && !cfg!(target_os = "linux")
            && let Some(w) = crate::showing(app, on)
        {
            let _ = w.eval(format!("dispatchEvent(new CustomEvent('arugula:notice', {{ detail: {ev} }}))"));
            return crate::bring_forward(&w);
        }
        match crate::showing(app, on).or_else(|| app.webview_windows().into_values().next()) {
            Some(w) => {
                let _ = w.navigate(url);
                crate::bring_forward(&w);
            }
            None => {
                let _ = crate::open_window(app, tauri::WebviewUrl::External(url));
            }
        }
    }

    /// Windows: the link a toast's click opens, which comes back to this
    /// app (`links.rs`): `arugula://notice?n=ID`, the tap kept here by an id
    /// of its own. Never a URL: a link any site can open mustn't say what
    /// the window shows.
    #[cfg(any(windows, test))]
    pub fn link(&self) -> String {
        format!("arugula://notice?n={}", remember(self.clone()))
    }
}

/// The taps of the notifications shown lately, by id (Windows: a toast's
/// link names one).
#[cfg(any(windows, test))]
static RECENT: Mutex<std::collections::VecDeque<(u64, Tap)>> = Mutex::new(std::collections::VecDeque::new());
#[cfg(any(windows, test))]
const RECENT_MAX: usize = 64;

#[cfg(any(windows, test))]
fn remember(tap: Tap) -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    let mut r = RECENT.lock().unwrap();
    r.push_back((id, tap));
    while r.len() > RECENT_MAX {
        r.pop_front();
    }
    id
}

#[cfg(any(windows, test))]
fn recall(id: u64) -> Option<Tap> {
    RECENT.lock().unwrap().iter().find(|(n, _)| *n == id).map(|(_, t)| t.clone())
}

/// `platformTap`: where a push opens on the platform page.
fn platform_path(msg: &Value) -> Option<String> {
    let seq = |v: &Value, sep: &str| v.as_i64().map(|n| format!("{sep}msg={n}")).unwrap_or_default();
    if let Some(d) = id(msg["deploy"].as_str(), "dep_", 32, 32) {
        return Some(format!("/deploys/{d}"));
    }
    let h = &msg["hosted"];
    if let (Some(p), Some(w)) = (id(h["project"].as_str(), "prj_", 32, 32), id(h["workspace"].as_str(), "hw_", 32, 32))
    {
        return Some(format!("/p/{p}/workspaces/{w}"));
    }
    if let Some(r) = room(&msg["chat"]["room"]) {
        return Some(format!("/chat/{r}{}", seq(&msg["chat"]["seq"], "?")));
    }
    let i = &msg["inbox"];
    if i.is_object() {
        let at = seq(&i["seq"], "?");
        if let Some(item) = id(i["item"].as_str(), "iss_", 8, 64) {
            return Some(format!("/issues/{item}{at}"));
        }
        if let Some(run) = id(i["item"].as_str(), "run_", 32, 32) {
            return Some(format!("/runs/{run}"));
        }
        if let Some(r) = room(&i["room"]) {
            return Some(format!("/chat/{r}{at}"));
        }
        return Some("/inbox".into());
    }
    None
}

/// A toast's `arugula://notice?n=ID` (Windows): the tap it was shown with,
/// if it's one of the recent ones; nothing else.
#[cfg(windows)]
pub fn open_recent(app: &AppHandle, id: u64) {
    let Some(tap) = recall(id) else {
        eprintln!("arugula: no notification {id} here");
        return;
    };
    let a = app.clone();
    let _ = app.run_on_main_thread(move || tap.open(&a));
}

/// The commands, for control's page alone (`allow_control` in main.rs).
pub const PERMISSIONS: [&str; 4] =
    ["allow-notices-keys", "allow-notices-start", "allow-notices-stop", "allow-notices-state"];

#[cfg(test)]
mod tests {
    use super::*;

    const C: &str = "https://control.test";
    const P: Option<&str> = Some("https://platform.test");

    #[test]
    fn a_click_opens_what_the_service_workers_would() {
        let tap = |m: Value, p| Tap::of(&m, C, p).url;
        let d = "ab".repeat(16);
        assert_eq!(tap(json!({ "pane": 3, "daemon": "d1" }), P), "https://control.test/#pane=d1.3");
        assert_eq!(tap(json!({ "pane": 3, "daemon": "d1", "reason": {} }), P), "https://control.test/#swarm=d1.3");
        assert_eq!(
            tap(json!({ "control": true, "deploy": format!("dep_{d}") }), P),
            format!("https://platform.test/deploys/dep_{d}")
        );
        let chat = json!({ "control": true, "chat": { "room": "ch_0123456789abcdef", "seq": 4 } });
        assert_eq!(tap(chat.clone(), P), "https://platform.test/chat/ch_0123456789abcdef?msg=4");
        assert_eq!(tap(chat, None), "https://control.test/#chat=ch_0123456789abcdef&msg=4");
        let issue = json!({ "control": true, "inbox": { "item": "iss_0123abcd", "seq": 2 } });
        assert_eq!(tap(issue, P), "https://platform.test/issues/iss_0123abcd?msg=2");
        assert_eq!(
            tap(json!({ "control": true, "inbox": { "item": format!("run_{d}") } }), P),
            format!("https://platform.test/runs/run_{d}")
        );
        assert_eq!(tap(json!({ "control": true, "inbox": {} }), P), "https://platform.test/inbox");
        let hosted =
            json!({ "control": true, "hosted": { "project": format!("prj_{d}"), "workspace": format!("hw_{d}") } });
        assert_eq!(tap(hosted, P), format!("https://platform.test/p/prj_{d}/workspaces/hw_{d}"));
        let waits = Tap::of(&json!({ "control": true, "title": "A new browser wants in" }), C, P);
        assert_eq!((waits.url.as_str(), waits.event), ("https://control.test/", Some(json!({ "refresh": true }))));
        // Nothing that isn't an id gets into the address.
        assert_eq!(tap(json!({ "control": true, "chat": { "room": "../x" } }), P), "https://control.test/");
        assert_eq!(tap(json!({ "pane": 3, "daemon": "d1/../x" }), P), "https://control.test/#pane=3");
    }

    /// A toast's link names a tap kept here, never a URL; an id that isn't
    /// one of them opens nothing.
    #[test]
    fn a_toasts_link_names_a_tap_kept_here() {
        let tap = Tap { url: "https://control.test/#pane=d1.3".into(), event: None };
        let link = tap.link();
        let id: u64 = link.strip_prefix("arugula://notice?n=").unwrap().parse().unwrap();
        assert!(!link.contains("control.test"));
        assert_eq!(recall(id), Some(tap));
        assert_eq!(recall(id + 1_000_000), None);
        for _ in 0..RECENT_MAX {
            Tap { url: "https://control.test/".into(), event: None }.link();
        }
        assert_eq!(recall(id), None, "only the recent ones");
    }

    /// The file with the key is this user's alone from the moment it's
    /// there, and is replaced whole.
    #[cfg(unix)]
    #[test]
    fn the_key_file_is_private_from_the_start() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("arugula-notices-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let f = dir.join("notices.json");
        write_private(&f, b"one").unwrap();
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        // An old file anyone could read is replaced by a private one.
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&f, b"two").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"two");
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temporary file left");
        // Somewhere it can't be written: an error, not a quiet failure.
        assert!(write_private(&f.join("under-a-file.json"), b"x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn events_come_at_their_blank_line() {
        let mut e = Events::default();
        assert_eq!(e.line(": keep-alive"), None);
        assert_eq!(e.line(""), None);
        assert_eq!(e.line("id: 7"), None);
        assert_eq!(e.line("data: abc"), None);
        assert_eq!(e.line(""), Some((Some("7".into()), "abc".into())));
    }

    /// RFC 8291 the way a daemon and control encrypt (`arugula_e2e::push`).
    fn encrypt(plain: &[u8], ua_public: &[u8], auth: &[u8]) -> Vec<u8> {
        use aes_gcm::{Aes128Gcm, KeyInit, aead::Aead};
        use hkdf::Hkdf;
        use sha2::Sha256;
        let local = p256::SecretKey::from_slice(&[9; 32]).unwrap();
        let salt = [5u8; 16];
        let ua = p256::PublicKey::from_sec1_bytes(ua_public).unwrap();
        let as_public = local.public_key().to_encoded_point(false).as_bytes().to_vec();
        let shared = p256::ecdh::diffie_hellman(local.to_nonzero_scalar(), ua.as_affine());
        let mut info = b"WebPush: info\0".to_vec();
        info.extend_from_slice(ua_public);
        info.extend_from_slice(&as_public);
        let mut ikm = [0u8; 32];
        Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes().as_ref()).expand(&info, &mut ikm).unwrap();
        let prk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
        let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
        prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).unwrap();
        prk.expand(b"Content-Encoding: nonce\0", &mut nonce).unwrap();
        let mut padded = plain.to_vec();
        padded.push(2);
        let sealed = Aes128Gcm::new_from_slice(&cek).unwrap().encrypt(&nonce.into(), padded.as_slice()).unwrap();
        let mut out = salt.to_vec();
        out.extend_from_slice(&4096u32.to_be_bytes());
        out.push(as_public.len() as u8);
        out.extend_from_slice(&as_public);
        out.extend_from_slice(&sealed);
        out
    }

    #[test]
    fn a_notice_opens_with_the_subscriptions_key_alone() {
        let key = p256::SecretKey::from_slice(&[3; 32]).unwrap();
        let auth = [4u8; 16];
        let k = Kept {
            control: C.into(),
            endpoint: format!("{C}{LIVE}{}", "0".repeat(32)),
            listen: "t".into(),
            key: hex(&key.to_bytes()),
            auth: B64.encode(auth),
        };
        let msg = json!({ "title": "%3 needs you", "pane": 3 });
        let body = encrypt(msg.to_string().as_bytes(), key.public_key().to_encoded_point(false).as_bytes(), &auth);
        assert_eq!(open(&k, &B64.encode(&body)), Some(msg));
        let other = Kept { key: hex(&[8; 32]), ..k.clone() };
        assert_eq!(open(&other, &B64.encode(&body)), None);
        assert_eq!(open(&k, "not base64!"), None);
    }
}
