//! The core as a C library, for the iPhone app's Swift (`arugula_core.h`).
//!
//! Almost everything crosses as a JSON string, so the app's types are its own
//! and a field added here breaks nothing there. A failed call returns NULL and
//! sets `*err` to JSON, `{"status": 401, "error": "...", "reason": "..."}`, or
//! just `{"error": "..."}`. A `reason` of `signed_out` means the session has
//! ended; `older_control`, that control doesn't take the app's sign-in;
//! `update_app`, that control no longer serves this build of the app. Free
//! both kinds of string with `aru_string_free`.
//! Only the terminal's rows cross as C structs, because they're drawn each
//! frame.
#![allow(clippy::missing_safety_doc)]

use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
    sync::Mutex,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use arugula_proto::{BlockType, Delta, PaneInfo, State, TabView};
use libghostty_vt::{
    key::{Key, Mods},
    render::{CellIterator, Dirty, RenderState, RowIterator},
    style::RgbColor,
    terminal::ScrollViewport,
};
use serde_json::{Value, json};

use crate::{
    Pane,
    chat_live::{ChatLive, Event as ChatEvent},
    control::{self, CALLBACK_SCHEME, Control, OlderControl, Refused, Session, SignIn, UpdateApp},
    link::{self, Event, Link},
};

/// A request to a machine waits this long.
const MACHINE_ANSWER: Duration = Duration::from_secs(30);

fn to_c(s: String) -> *mut c_char {
    CString::new(s).map_or(ptr::null_mut(), CString::into_raw)
}

unsafe fn text(p: *const c_char) -> String {
    if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
}

unsafe fn set_err(err: *mut *mut c_char, e: &anyhow::Error) {
    if !err.is_null() {
        unsafe { *err = to_c(error_json(e).to_string()) };
    }
}

/// A failure as the app reads it, `{"status", "error", "reason"}`: the same
/// for Swift's `*err` and Kotlin's `CoreError`.
pub(crate) fn error_json(e: &anyhow::Error) -> Value {
    // Signed out, wherever it came from: the app asks for a sign-in again.
    let refused = e.chain().find_map(|c| c.downcast_ref::<Refused>());
    if let Some(r) = refused {
        let reason = if r.signed_out() { Some("signed_out") } else { r.reason.as_deref() };
        json!({ "status": r.status, "error": r.error, "reason": reason })
    } else if e.downcast_ref::<UpdateApp>().is_some() {
        json!({ "error": format!("{e:#}"), "reason": "update_app" })
    } else if e.downcast_ref::<OlderControl>().is_some() {
        json!({ "error": format!("{e:#}"), "reason": "older_control" })
    } else if let Some(r) = e.downcast_ref::<link::Refused>() {
        json!({ "error": format!("{e:#}"), "reason": r.code })
    } else {
        json!({ "error": format!("{e:#}") })
    }
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

// ---- signing in

/// Whether control at `url` serves this build of the app: 1, or 0 and
/// `*err` (`update_app` when it doesn't, else why it couldn't tell). Blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_check_app(url: *const c_char, err: *mut *mut c_char) -> i32 {
    match control::check_app(&unsafe { text(url) }) {
        Ok(()) => 1,
        Err(e) => {
            unsafe { set_err(err, &e) };
            0
        }
    }
}

pub struct AruSignIn(pub(crate) SignIn);

/// Ask control at `url` for a sign-in. `name` says what this is on the page
/// where someone signed in allows it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_signin_start(
    url: *const c_char,
    name: *const c_char,
    err: *mut *mut c_char,
) -> *mut AruSignIn {
    let (url, name) = unsafe { (text(url), text(name)) };
    match SignIn::start(&url, &name) {
        Ok(s) => Box::into_raw(Box::new(AruSignIn(s))),
        Err(e) => {
            unsafe { set_err(err, &e) };
            ptr::null_mut()
        }
    }
}

/// `{"link": ..., "callback_scheme": "arugula", "expires_in_secs": ...}`:
/// what the web sheet opens, and the scheme it waits for.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_signin_info(s: *const AruSignIn) -> *mut c_char {
    let s = &unsafe { &*s }.0;
    let v = json!({
        "link": s.link,
        "ticket": s.ticket(),
        "callback_scheme": CALLBACK_SCHEME,
        "expires_in_secs": s.expires_in_secs,
    });
    to_c(v.to_string())
}

/// Swap the grant in `callback` (the address the web sheet came back with)
/// for a session: what to put in the keychain. Blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_signin_finish(
    s: *const AruSignIn,
    callback: *const c_char,
    err: *mut *mut c_char,
) -> *mut c_char {
    let s = &unsafe { &*s }.0;
    let r = s.finish(&unsafe { text(callback) }).and_then(|c| Ok(serde_json::to_string(&c.session)?));
    unsafe { give(r, err, to_c) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_signin_free(s: *mut AruSignIn) {
    if !s.is_null() {
        drop(unsafe { Box::from_raw(s) });
    }
}

// ---- control

/// From what the keychain holds (`aru_signin_finish`'s answer).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_control_open(stored: *const c_char, err: *mut *mut c_char) -> *mut Control {
    let open = || -> Result<Control> {
        let s: Session = serde_json::from_str(&unsafe { text(stored) }).context("the saved sign-in")?;
        Control::new(s)
    };
    match open() {
        Ok(c) => Box::into_raw(Box::new(c)),
        Err(e) => {
            unsafe { set_err(err, &e) };
            ptr::null_mut()
        }
    }
}

/// End the session at control. Blocks. The app forgets it either way.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_control_sign_out(c: *const Control) {
    let _ = unsafe { &*c }.sign_out();
}

/// A request as the account: its JSON. `body` is JSON or NULL. Blocks: call it
/// off the main thread. A session that has ended answers `"reason":
/// "signed_out"`.
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
    pub(crate) link: Link,
    pub(crate) events: Mutex<LinkState>,
}

#[derive(Default)]
pub(crate) struct LinkState {
    /// The machine's state as it stands, the deltas applied.
    full: Option<State>,
    state: Option<String>,
    said: Option<String>,
    closed: Option<String>,
}

/// What the app shows of a machine: its terminals (`panes`), its tabs as
/// they're laid out (`tabs`: each one's grid and every pane's rectangle in it,
/// blocks too, so a client can draw the whole tab), and its sessions, which
/// hold the tabs, in order.
///
/// Each pane also says what home answers (Arugula #1256): its reason whole
/// (with what it asks), its own ask card, its block type, the project and
/// card it works on, and whether teammates' agents may mention it; `agents`
/// are the agent blocks, the same way; `roles` is this person's role in each
/// session (none: the owner).
fn summary(s: &State) -> String {
    let pane = |info: &PaneInfo, tab: &TabView| {
        json!({
            "id": info.id,
            "tab": tab.id,
            "tab_name": tab.name,
            "command": info.command,
            "cwd": info.cwd,
            "running": info.running,
            // What tells panes apart: whether it wants you and why (with what
            // it asks), what its program calls it, what it's busy with, where.
            "attention": info.attention,
            "reason": info.reason,
            "ask": info.ask,
            "type": info.kind,
            "title": info.title,
            "work": info.work,
            "resumes": info.resumes,
            "project": info.project.as_ref().map(|p| &p.name),
            "session_project": info.session_project,
            "on_issue": info.on_issue,
            "mentions_off": info.mentions_off,
            "branch": info.branch,
            "last_ms": info.activity.as_ref().map(|a| a.last_ms),
        })
    };
    // Terminals, and agent blocks apart: a browser or a diff block is a web
    // page, for later (web views).
    let of = |kind: BlockType| -> Vec<Value> {
        s.tabs
            .iter()
            .flat_map(|t| {
                t.layout.panes.iter().filter_map(move |(p, _)| {
                    let info = s.panes.iter().find(|i| i.id == *p && i.kind == kind)?;
                    Some(pane(info, t))
                })
            })
            .collect()
    };
    let tabs: Vec<Value> = s
        .tabs
        .iter()
        .map(|t| {
            let cells: Vec<Value> = t
                .layout
                .panes
                .iter()
                .map(|(p, r)| {
                    let kind = s.panes.iter().find(|i| i.id == *p).map(|i| i.kind);
                    json!({ "pane": p, "kind": kind, "x": r.x, "y": r.y, "cols": r.cols, "rows": r.rows })
                })
                .collect();
            json!({ "id": t.id, "name": t.name, "cols": t.cols, "rows": t.rows, "cells": cells })
        })
        .collect();
    let sessions: Vec<Value> = s
        .sessions
        .iter()
        .map(|x| json!({ "id": x.id, "name": x.name, "project": x.project.as_ref().map(|p| &p.name), "tabs": x.tabs }))
        .collect();
    json!({
        "panes": of(BlockType::Terminal),
        "agents": of(BlockType::Agent),
        "tabs": tabs,
        "sessions": sessions,
        "roles": s.roles,
    })
    .to_string()
}

/// Apply a delta to the last state, as the web's client does: each pane's
/// changed fields over what it had (`null`: back to its default), panes
/// gone, and machines, presence and notices whole when sent.
fn apply(s: &mut State, d: Delta) {
    for patch in d.panes {
        let Some(id) = patch.get("id").and_then(Value::as_u64) else { continue };
        let at = s.panes.iter().position(|p| u64::from(p.id) == id);
        let mut v = at.and_then(|i| serde_json::to_value(&s.panes[i]).ok()).unwrap_or_else(|| json!({}));
        for (k, x) in patch {
            if x.is_null() {
                v.as_object_mut().map(|o| o.remove(&k));
            } else {
                v[k] = x;
            }
        }
        let Ok(p) = serde_json::from_value::<PaneInfo>(v) else { continue };
        match at {
            Some(i) => s.panes[i] = p,
            None => s.panes.push(p),
        }
    }
    s.panes.retain(|p| !d.gone.contains(&p.id));
    if let Some(m) = d.machines {
        s.machines = m;
    }
    if let Some(p) = d.presence {
        s.presence = p;
    }
    if let Some(n) = d.notices {
        s.notices = n;
    }
}

/// A state or a delta: the app's summary again. True when it changed.
fn take_state(st: &mut LinkState, e: Event) -> bool {
    match e {
        Event::State(s) => st.full = Some(*s),
        Event::Delta(d) => match st.full.as_mut() {
            Some(s) => apply(s, *d),
            None => return false,
        },
        _ => return false,
    }
    st.state = st.full.as_ref().map(summary);
    true
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
            e @ (Event::State(_) | Event::Delta(_)) => {
                if take_state(&mut st, e) {
                    flags |= 2;
                }
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

/// The newest panes: `{"panes": [{id, tab, tab_name, command, cwd, running,
/// attention, reason, title, work, resumes, project, branch, last_ms, rect}],
/// "tabs": [{id, name, cols, rows, cells: [{pane, kind, x, y, cols, rows}]}],
/// "sessions": [{id, name, project, tabs}]}`,
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

/// As `aru_link_feed`, for several panes at once: `terms[i]` takes `panes[i]`'s
/// bytes (a NULL term takes none). Bit 1 says one of them drew something.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_link_feed_many(
    l: *mut AruLink,
    panes: *const u32,
    terms: *const *mut AruTerm,
    n: usize,
) -> u32 {
    let l = unsafe { &*l };
    let (panes, terms) = if n == 0 {
        (&[][..], &[][..])
    } else {
        unsafe { (std::slice::from_raw_parts(panes, n), std::slice::from_raw_parts(terms, n)) }
    };
    let term_of = |p: u32| panes.iter().position(|&x| x == p).map(|i| terms[i]).filter(|t| !t.is_null());
    let mut st = l.events.lock().unwrap();
    let mut flags = 0;
    while let Ok(e) = l.link.events.lock().unwrap().try_recv() {
        match e {
            e @ (Event::State(_) | Event::Delta(_)) => {
                if take_state(&mut st, e) {
                    flags |= 2;
                }
            }
            Event::Snapshot { pane, data } => {
                if let Some(t) = term_of(pane) {
                    unsafe { &mut *t }.pane.snapshot(&data);
                    flags |= 1;
                }
            }
            Event::Output { pane, data } => {
                if let Some(t) = term_of(pane) {
                    unsafe { &mut *t }.pane.output(&data);
                    flags |= 1;
                }
            }
            Event::Said(s) => {
                st.said = Some(s);
                flags |= 8;
            }
            Event::Closed(why) => {
                st.closed = Some(why);
                flags |= 4;
            }
        }
    }
    flags
}

/// Watch a whole tab without taking its size: `panes`' snapshots and output
/// come, at the size the tab already is (`cols`x`rows`, its own).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_link_watch(
    l: *const AruLink,
    tab: u32,
    panes: *const u32,
    n: usize,
    cols: u16,
    rows: u16,
) {
    let panes = if n == 0 { Vec::new() } else { unsafe { std::slice::from_raw_parts(panes, n) }.to_vec() };
    let _ = unsafe { &*l }.link.watch(tab, panes, cols, rows);
}

/// Stop receiving `panes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_link_detach(l: *const AruLink, panes: *const u32, n: usize) {
    let panes = if n == 0 { Vec::new() } else { unsafe { std::slice::from_raw_parts(panes, n) }.to_vec() };
    let _ = unsafe { &*l }.link.detach(panes);
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

// ---- chat's live stream

pub struct AruChatLive(pub(crate) ChatLive);

/// Chat's live stream. `notify` is called from its thread when an event is
/// queued: `aru_chat_live_next` on the main thread then takes it. NULL, with
/// `err`, when control refuses it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_chat_live_open(
    c: *const Control,
    notify: AruNotify,
    ctx: *mut c_void,
    err: *mut *mut c_char,
) -> *mut AruChatLive {
    let ctx = Ctx(ctx);
    let tell = move || {
        let c = &ctx;
        notify(c.0);
    };
    match ChatLive::open(unsafe { &*c }, tell) {
        Ok(live) => Box::into_raw(Box::new(AruChatLive(live))),
        Err(e) => {
            unsafe { set_err(err, &e) };
            ptr::null_mut()
        }
    }
}

/// The next event: control's JSON (`{"type": "message", ...}`), or
/// `{"type":"closed","why":...}` once the stream has ended. NULL when none is
/// queued.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_chat_live_next(l: *const AruChatLive) -> *mut c_char {
    match unsafe { &*l }.0.next() {
        Some(ChatEvent::Data(s)) => to_c(s),
        Some(ChatEvent::Closed(why)) => to_c(json!({ "type": "closed", "why": why }).to_string()),
        None => ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aru_chat_live_free(l: *mut AruChatLive) {
    if !l.is_null() {
        drop(unsafe { Box::from_raw(l) });
    }
}

// ---- the terminal

pub struct AruTerm {
    pub(crate) pane: Pane,
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

#[cfg(test)]
mod tests {
    use super::{Delta, State, apply};
    use serde_json::json;

    /// A machine with one idle terminal pane.
    fn state() -> State {
        let pane = json!({
            "id": 2, "epoch": 0, "cwd": null, "command": "bash", "running": true, "policy": { "kind": "shell" },
            "current": null, "last": null, "attention": "idle", "integration": false, "type": "terminal", "host": null,
        });
        serde_json::from_value(
            json!({ "rev": 1, "sessions": [], "tabs": [], "panes": [pane], "machines": [], "options": {} }),
        )
        .expect("a state")
    }

    #[test]
    fn a_delta_brings_what_a_pane_asks_and_null_takes_it_away() {
        let mut s = state();
        let asks = json!({
            "id": 2, "attention": "needs_input",
            "reason": { "kind": "ask", "since_ms": 5, "headline": "Bash: echo hi", "actions": ["allow", "deny"],
                        "ask": { "id": "toolu_1", "what": "approve", "agent": "claude" } },
        });
        apply(&mut s, serde_json::from_value::<Delta>(json!({ "panes": [asks] })).unwrap());
        let r = s.panes[0].reason.as_ref().expect("a reason");
        assert_eq!(r.ask.as_ref().map(|a| a.id.as_str()), Some("toolu_1"));
        assert_eq!(s.panes[0].command.as_deref(), Some("bash"), "fields the delta didn't name stay");
        apply(&mut s, serde_json::from_value::<Delta>(json!({ "panes": [{ "id": 2, "reason": null }] })).unwrap());
        assert!(s.panes[0].reason.is_none());
        apply(&mut s, serde_json::from_value::<Delta>(json!({ "gone": [2] })).unwrap());
        assert!(s.panes.is_empty());
    }
}
