//! The core as a C library, for the iPhone app's Swift (`arugula_core.h`).
//!
//! Almost everything crosses as a JSON string, so the app's types are its own
//! and a field added here breaks nothing there. A failed call returns NULL and
//! sets `*err` to JSON, `{"status": 401, "error": "...", "reason": "..."}`, or
//! just `{"error": "..."}`. Free both kinds of string with `aru_string_free`.
//! Only the terminal's rows cross as C structs, because they're drawn each
//! frame.
#![allow(clippy::missing_safety_doc)]

use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
    sync::Mutex,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, ensure};
use arugula_e2e::{Cert, Trust};
use arugula_proto::{BlockType, State};
use libghostty_vt::{
    key::{Key, Mods},
    render::{CellIterator, Dirty, RenderState, RowIterator},
    style::RgbColor,
    terminal::ScrollViewport,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    DeviceKeys, Pane,
    control::{Control, Pairing, Refused},
    device,
    link::{Event, Link},
};

/// A request to a machine waits this long.
const MACHINE_ANSWER: Duration = Duration::from_secs(30);

/// What the app keeps in the keychain: the device, and where it's paired.
#[derive(Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    url: String,
    keys: StoredKeys,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cert: Option<Cert>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trust: Option<Trust>,
}

#[derive(Serialize, Deserialize)]
struct StoredKeys {
    noise_private: String,
    noise_public: String,
    sign: String,
}

impl StoredKeys {
    fn of(k: &DeviceKeys) -> Self {
        Self {
            noise_private: hex::encode(k.noise_private),
            noise_public: hex::encode(k.noise_public),
            sign: hex::encode(k.sign.to_bytes()),
        }
    }

    fn keys(&self) -> Result<DeviceKeys> {
        let b = |s: &str| -> Result<[u8; 32]> {
            hex::decode(s)?.try_into().map_err(|_| anyhow!("a device key isn't 32 bytes"))
        };
        Ok(device(b(&self.noise_private)?, b(&self.noise_public)?, b(&self.sign)?))
    }
}

fn to_c(s: String) -> *mut c_char {
    CString::new(s).map_or(ptr::null_mut(), CString::into_raw)
}

unsafe fn text(p: *const c_char) -> String {
    if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
}

unsafe fn set_err(err: *mut *mut c_char, e: &anyhow::Error) {
    if err.is_null() {
        return;
    }
    let v = match e.downcast_ref::<Refused>() {
        Some(r) => json!({ "status": r.status, "error": r.error, "reason": r.reason }),
        None => json!({ "error": format!("{e:#}") }),
    };
    unsafe { *err = to_c(v.to_string()) };
}

/// A result as the C side takes it: NULL and `*err` on failure.
unsafe fn give<T>(r: Result<T>, err: *mut *mut c_char, f: impl FnOnce(T) -> *mut c_char) -> *mut c_char {
    match r {
        Ok(v) => f(v),
        Err(e) => {
            unsafe { set_err(err, &e) };
            ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

// ---- pairing

pub struct AruPairing(Option<Pairing>);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_pairing_start(
    url: *const c_char,
    name: *const c_char,
    err: *mut *mut c_char,
) -> *mut AruPairing {
    let (url, name) = unsafe { (text(url), text(name)) };
    match Pairing::start(&url, DeviceKeys::generate(), &name) {
        Ok(p) => Box::into_raw(Box::new(AruPairing(Some(p)))),
        Err(e) => {
            unsafe { set_err(err, &e) };
            ptr::null_mut()
        }
    }
}

/// `{"link": ..., "code": ..., "expires_in_secs": ...}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_pairing_info(p: *const AruPairing) -> *mut c_char {
    let Some(p) = (unsafe { &*p }).0.as_ref() else { return ptr::null_mut() };
    to_c(json!({ "link": p.link(), "code": p.whole_code(), "expires_in_secs": p.expires_in_secs }).to_string())
}

/// Look once. Still waiting: NULL with `*err` left NULL. Approved: what to put
/// in the keychain. Failed: NULL and `*err`, and it can't be polled again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_pairing_poll(p: *mut AruPairing, err: *mut *mut c_char) -> *mut c_char {
    let slot = &mut unsafe { &mut *p }.0;
    let r = slot.take().context("this pairing has ended").and_then(Pairing::poll);
    match r {
        Ok(Err(still)) => {
            *slot = Some(still);
            ptr::null_mut()
        }
        Ok(Ok(c)) => {
            let saved = Stored {
                url: c.url.clone(),
                keys: StoredKeys::of(&c.keys),
                cert: Some(c.cert.clone()),
                trust: Some(c.trust.clone()),
            };
            unsafe { give(serde_json::to_string(&saved).map_err(Into::into), err, to_c) }
        }
        Err(e) => {
            unsafe { set_err(err, &e) };
            ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_pairing_free(p: *mut AruPairing) {
    if !p.is_null() {
        drop(unsafe { Box::from_raw(p) });
    }
}

// ---- control

/// From what the keychain holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_control_open(stored: *const c_char, err: *mut *mut c_char) -> *mut Control {
    let open = || -> Result<Control> {
        let s: Stored = serde_json::from_str(&unsafe { text(stored) }).context("the saved device")?;
        let cert = s.cert.context("the saved device has no certificate")?;
        let trust = s.trust.context("the saved device has no account root")?;
        Control::new(&s.url, s.keys.keys()?, cert, trust)
    };
    match open() {
        Ok(c) => Box::into_raw(Box::new(c)),
        Err(e) => {
            unsafe { set_err(err, &e) };
            ptr::null_mut()
        }
    }
}

/// A signed request: its JSON. `body` is JSON or NULL. Blocks: call it off the
/// main thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_control_call(
    c: *const Control,
    method: *const c_char,
    path: *const c_char,
    body: *const c_char,
    err: *mut *mut c_char,
) -> *mut c_char {
    let c = unsafe { &*c };
    let (method, path) = unsafe { (text(method), text(path)) };
    let call = || -> Result<String> {
        let body: Option<Value> =
            if body.is_null() { None } else { Some(serde_json::from_str(&unsafe { text(body) })?) };
        Ok(c.call(&method, &path, body.as_ref())?.to_string())
    };
    unsafe { give(call(), err, to_c) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_control_free(c: *mut Control) {
    if !c.is_null() {
        drop(unsafe { Box::from_raw(c) });
    }
}

// ---- a machine

struct Ctx(*mut c_void);
// SAFETY: the app's context is only handed back to its own callback, which
// hops to the main thread before touching anything.
unsafe impl Send for Ctx {}

pub type AruNotify = extern "C" fn(ctx: *mut c_void);

pub struct AruLink {
    link: Link,
    events: Mutex<LinkState>,
}

#[derive(Default)]
struct LinkState {
    state: Option<String>,
    said: Option<String>,
    closed: Option<String>,
}

/// What the app shows of a machine's panes.
fn summary(s: &State) -> String {
    let panes: Vec<Value> = s
        .tabs
        .iter()
        .flat_map(|t| {
            t.layout.panes.iter().filter_map(move |(p, _)| {
                // Terminals only: a browser or a diff block is a web page, for
                // later (web views).
                let info = s.panes.iter().find(|i| i.id == *p && i.kind == BlockType::Terminal)?;
                Some(json!({
                    "id": info.id,
                    "tab": t.id,
                    "tab_name": t.name,
                    "command": info.command,
                    "cwd": info.cwd,
                    "running": info.running,
                }))
            })
        })
        .collect();
    json!({ "panes": panes }).to_string()
}

/// `notify` is called from the socket's thread when there's something to look
/// at: `aru_link_feed` on the main thread then takes it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_link_open(
    c: *const Control,
    machine: *const c_char,
    notify: AruNotify,
    ctx: *mut c_void,
    err: *mut *mut c_char,
) -> *mut AruLink {
    let ctx = Ctx(ctx);
    let tell = move || {
        let c = &ctx;
        notify(c.0);
    };
    match Link::open_notifying(unsafe { &*c }, &unsafe { text(machine) }, tell) {
        Ok(link) => Box::into_raw(Box::new(AruLink { link, events: Mutex::default() })),
        Err(e) => {
            unsafe { set_err(err, &e) };
            ptr::null_mut()
        }
    }
}

/// Take what's queued: `pane`'s bytes go into `term` (which may be NULL).
/// Returns bits: 1 the terminal changed, 2 the panes changed, 4 the channel
/// closed (`aru_link_closed` says why), 8 the machine said something.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_link_feed(l: *mut AruLink, term: *mut AruTerm, pane: u32) -> u32 {
    let l = unsafe { &*l };
    let mut st = l.events.lock().unwrap();
    let mut flags = 0;
    while let Ok(e) = l.link.events.lock().unwrap().try_recv() {
        match e {
            Event::State(s) => {
                st.state = Some(summary(&s));
                flags |= 2;
            }
            Event::Snapshot { pane: p, data } if p == pane && !term.is_null() => {
                unsafe { &mut *term }.pane.snapshot(&data);
                flags |= 1;
            }
            Event::Output { pane: p, data } if p == pane && !term.is_null() => {
                unsafe { &mut *term }.pane.output(&data);
                flags |= 1;
            }
            Event::Said(s) => {
                st.said = Some(s);
                flags |= 8;
            }
            Event::Closed(why) => {
                st.closed = Some(why);
                flags |= 4;
            }
            _ => {}
        }
    }
    flags
}

/// The newest panes: `{"panes": [{id, tab, tab_name, command, cwd, running}]}`,
/// or NULL before the machine's first word.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_link_panes(l: *const AruLink) -> *mut c_char {
    unsafe { &*l }.events.lock().unwrap().state.clone().map_or(ptr::null_mut(), to_c)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_link_closed(l: *const AruLink) -> *mut c_char {
    unsafe { &*l }.events.lock().unwrap().closed.clone().map_or(ptr::null_mut(), to_c)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_link_said(l: *const AruLink) -> *mut c_char {
    unsafe { &*l }.events.lock().unwrap().said.take().map_or(ptr::null_mut(), to_c)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_link_attach(l: *const AruLink, tab: u32, pane: u32, cols: u16, rows: u16) {
    let _ = unsafe { &*l }.link.attach(tab, pane, cols, rows);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_link_view(l: *const AruLink, tab: u32, pane: u32, cols: u16, rows: u16) {
    let _ = unsafe { &*l }.link.view(tab, pane, cols, rows);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_link_input(l: *const AruLink, pane: u32, data: *const u8, len: usize) {
    let _ = unsafe { &*l }.link.input(pane, unsafe { std::slice::from_raw_parts(data, len) });
}

/// One request to the machine's own API: `{"status": N, "body": ...}`. Blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_link_request(
    l: *const AruLink,
    method: *const c_char,
    path: *const c_char,
    body: *const c_char,
    err: *mut *mut c_char,
) -> *mut c_char {
    let l = unsafe { &*l };
    let (method, path) = unsafe { (text(method), text(path)) };
    let call = || -> Result<String> {
        let body: Option<Value> =
            if body.is_null() { None } else { Some(serde_json::from_str(&unsafe { text(body) })?) };
        let (status, v) = l.link.request(&method, &path, body.as_ref(), MACHINE_ANSWER)?;
        ensure!((200..300).contains(&status), "the machine answered {status}: {v}");
        Ok(json!({ "status": status, "body": v }).to_string())
    };
    unsafe { give(call(), err, to_c) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_link_free(l: *mut AruLink) {
    if !l.is_null() {
        drop(unsafe { Box::from_raw(l) });
    }
}

// ---- the terminal

pub struct AruTerm {
    pane: Pane,
    render: RenderState<'static>,
    rows: RowIterator<'static>,
    cells: CellIterator<'static>,
    key_buf: Vec<u8>,
}

/// One run of a row: cells with the same colors and flags. Outside ASCII, one
/// run per cell, so wide characters stay on the grid.
#[repr(C)]
pub struct AruRun {
    pub col: u16,
    pub cells: u16,
    pub fg: u32,
    /// 0xRRGGBB, or u32::MAX for the default background.
    pub bg: u32,
    /// 1 bold, 2 italic.
    pub flags: u8,
    pub text: *const u8,
    pub text_len: usize,
}

#[repr(C)]
#[derive(Default)]
pub struct AruFrame {
    pub dirty: u32,
    pub cols: u16,
    pub rows: u16,
    pub fg: u32,
    pub bg: u32,
    pub cursor_visible: bool,
    pub cursor_x: u16,
    pub cursor_y: u16,
    pub cursor_color: u32,
}

fn rgb(c: RgbColor) -> u32 {
    u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
}

#[unsafe(no_mangle)]
pub extern "C" fn aru_term_new(cols: u16, rows: u16) -> *mut AruTerm {
    let make = || -> Result<AruTerm> {
        Ok(AruTerm {
            pane: Pane::new(cols, rows)?,
            render: RenderState::new()?,
            rows: RowIterator::new()?,
            cells: CellIterator::new()?,
            key_buf: Vec::new(),
        })
    };
    make().map_or(ptr::null_mut(), |t| Box::into_raw(Box::new(t)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_term_free(t: *mut AruTerm) {
    if !t.is_null() {
        drop(unsafe { Box::from_raw(t) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_term_resize(t: *mut AruTerm, cols: u16, rows: u16, cell_w: u32, cell_h: u32) {
    let _ = unsafe { &mut *t }.pane.resize(cols, rows, cell_w, cell_h);
}

/// Scroll the view back (negative) or forward, in rows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_term_scroll(t: *mut AruTerm, delta: i32) {
    unsafe { &mut *t }.pane.term.scroll_viewport(ScrollViewport::Delta(delta as isize));
}

/// Back to the live screen.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_term_scroll_bottom(t: *mut AruTerm) {
    unsafe { &mut *t }.pane.term.scroll_viewport(ScrollViewport::Bottom);
}

fn key_of(code: u32) -> Key {
    use Key::*;
    const LETTERS: [Key; 26] = [A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z];
    match code {
        1 => Escape,
        2 => Tab,
        3 => Enter,
        4 => Backspace,
        5 => ArrowUp,
        6 => ArrowDown,
        7 => ArrowLeft,
        8 => ArrowRight,
        9 => Home,
        10 => End,
        11 => PageUp,
        12 => PageDown,
        13 => Delete,
        65..=90 => LETTERS[(code - 65) as usize],
        _ => Unidentified,
    }
}

/// The bytes for a key under the terminal's modes, written to `out`; returns
/// how many. `code`: 1 Esc, 2 Tab, 3 Enter, 4 Backspace, 5-8 up, down, left,
/// right, 9 Home, 10 End, 11 PageUp, 12 PageDown, 13 Delete, or 'A' to 'Z'.
/// `mods`: 1 shift, 2 ctrl, 4 alt. `text` may be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_term_key(
    t: *mut AruTerm,
    code: u32,
    mods: u32,
    key_text: *const c_char,
    out: *mut u8,
    cap: usize,
) -> usize {
    let t = unsafe { &mut *t };
    let mut m = Mods::empty();
    for (bit, flag) in [(1, Mods::SHIFT), (2, Mods::CTRL), (4, Mods::ALT)] {
        if mods & bit != 0 {
            m |= flag;
        }
    }
    let given = (!key_text.is_null()).then(|| unsafe { text(key_text) });
    let AruTerm { pane, key_buf, .. } = t;
    key_buf.clear();
    if let Ok(bytes) = pane.key(key_of(code), true, m, given.as_deref()) {
        key_buf.extend_from_slice(bytes);
    }
    let n = key_buf.len().min(cap);
    unsafe { ptr::copy_nonoverlapping(key_buf.as_ptr(), out, n) };
    n
}

pub type AruRowFn = extern "C" fn(ctx: *mut c_void, row: u16, runs: *const AruRun, n: usize);

/// Walk the render state: `row_fn` for every row that changed since the last
/// call (all of them when `dirty` comes back 2), then fill `frame`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_term_render(t: *mut AruTerm, ctx: *mut c_void, row_fn: AruRowFn, frame: *mut AruFrame) {
    let AruTerm { pane, render, rows, cells, .. } = unsafe { &mut *t };
    let Ok(snap) = render.update(&pane.term) else { return };
    let Ok(colors) = snap.colors() else { return };
    let dirty = snap.dirty().unwrap_or(Dirty::Full);
    let full = dirty == Dirty::Full;
    let mut runs: Vec<AruRun> = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    let mut cell_text = String::new();
    if let Ok(mut it) = rows.update(&snap) {
        let mut y = 0u16;
        while let Some(row) = it.next() {
            if full || row.dirty().unwrap_or(true) {
                runs.clear();
                texts.clear();
                if let Ok(mut cit) = cells.update(row) {
                    let mut x = 0u16;
                    // (fg, bg, flags) of the open run, and where it began.
                    let mut open: Option<(u32, u32, u8, u16)> = None;
                    let mut buf = String::new();
                    let close = |open: &mut Option<(u32, u32, u8, u16)>,
                                 buf: &mut String,
                                 end: u16,
                                 texts: &mut Vec<String>,
                                 runs: &mut Vec<AruRun>| {
                        if let Some((fg, bg, flags, start)) = open.take() {
                            texts.push(std::mem::take(buf));
                            runs.push(AruRun {
                                col: start,
                                cells: end - start,
                                fg,
                                bg,
                                flags,
                                text: ptr::null(),
                                text_len: 0,
                            });
                        }
                    };
                    while let Some(cell) = cit.next() {
                        let n = cell.graphemes_len().unwrap_or(0);
                        let bg = cell.bg_color().ok().flatten();
                        let mut fg = cell.fg_color().ok().flatten().unwrap_or(colors.foreground);
                        let mut flags = 0u8;
                        let mut inverse = false;
                        if cell.has_styling().unwrap_or(false)
                            && let Ok(st) = cell.style()
                        {
                            flags |= u8::from(st.bold) | u8::from(st.italic) << 1;
                            inverse = st.inverse;
                        }
                        let mut bgc = bg.map_or(u32::MAX, rgb);
                        if inverse || cell.is_selected().unwrap_or(false) {
                            bgc = rgb(fg);
                            fg = bg.unwrap_or(colors.background);
                        }
                        let fgc = rgb(fg);
                        if n == 0 {
                            cell_text.clear();
                        } else {
                            let _ = cell.graphemes_utf8(&mut cell_text);
                        }
                        let ascii = cell_text.is_ascii();
                        let same = matches!(open, Some((f, b, fl, _)) if f == fgc && b == bgc && fl == flags);
                        if !(same && ascii) {
                            close(&mut open, &mut buf, x, &mut texts, &mut runs);
                        }
                        // The tail of a wide character belongs to the run before.
                        if n == 0
                            && !same
                            && runs.last().is_some_and(|r| r.col + r.cells == x)
                            && texts.last().is_some_and(|s| !s.is_ascii())
                        {
                            if let Some(r) = runs.last_mut() {
                                r.cells += 1;
                            }
                            x += 1;
                            continue;
                        }
                        if open.is_none() {
                            open = Some((fgc, bgc, flags, x));
                        }
                        buf.push_str(if n == 0 { " " } else { &cell_text });
                        if !ascii {
                            close(&mut open, &mut buf, x + 1, &mut texts, &mut runs);
                        }
                        x += 1;
                    }
                    close(&mut open, &mut buf, x, &mut texts, &mut runs);
                }
                for (r, s) in runs.iter_mut().zip(&texts) {
                    r.text = s.as_ptr();
                    r.text_len = s.len();
                }
                row_fn(ctx, y, runs.as_ptr(), runs.len());
                let _ = row.set_dirty(false);
            }
            y += 1;
        }
    }
    let f = unsafe { &mut *frame };
    f.dirty = match dirty {
        Dirty::Clean => 0,
        Dirty::Partial => 1,
        Dirty::Full => 2,
    };
    f.cols = snap.cols().unwrap_or(0);
    f.rows = snap.rows().unwrap_or(0);
    f.fg = rgb(colors.foreground);
    f.bg = rgb(colors.background);
    f.cursor_color = rgb(colors.cursor.unwrap_or(colors.foreground));
    f.cursor_visible = false;
    if snap.cursor_visible().unwrap_or(false)
        && let Ok(Some(c)) = snap.cursor_viewport()
    {
        f.cursor_visible = true;
        f.cursor_x = c.x;
        f.cursor_y = c.y;
    }
    let _ = snap.set_dirty(Dirty::Clean);
}
