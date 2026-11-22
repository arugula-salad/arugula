//! The core for Android's Kotlin (`io.arugula.core`, native/android), over
//! JNI. Each call goes through the same `aru_*` function Swift calls
//! ([`crate::ffi`]): this only carries Kotlin's strings, arrays and handles
//! across, so both apps get one behaviour.
//!
//! A handle is a pointer as a `Long`. A failed call throws
//! `io.arugula.core.CoreError`, its message the JSON Swift reads from `*err`
//! (`{"status", "error", "reason"}`). A terminal's frame crosses as one
//! `ByteArray` ([`frame`]), since it's drawn each frame.
#![allow(clippy::missing_safety_doc)]

use std::{
    ffi::{CString, c_char, c_void},
    ptr,
    sync::Mutex,
};

use jni::{
    Env, EnvUnowned, JavaVM,
    errors::ErrorPolicy,
    jni_sig, jni_str,
    objects::{Global, JByteArray, JClass, JIntArray, JLongArray, JObject, JString},
    strings::JNIString,
    sys::{jboolean, jbyteArray, jint, jlong, jstring},
};
use serde_json::json;

use crate::{
    chat_live::ChatLive,
    control::{Control, SignIn},
    ffi::{self, AruChatLive, AruFrame, AruLink, AruRun, AruTerm, error_json},
    link::Link,
};

/// Why a call failed: the core's (thrown as `CoreError`), or JNI's own.
enum Fail {
    Core(anyhow::Error),
    Jni(jni::errors::Error),
}

impl From<jni::errors::Error> for Fail {
    fn from(e: jni::errors::Error) -> Self {
        Self::Jni(e)
    }
}

impl From<anyhow::Error> for Fail {
    fn from(e: anyhow::Error) -> Self {
        Self::Core(e)
    }
}

type Res<T> = Result<T, Fail>;

/// Throws `CoreError` with the core's JSON, or a `RuntimeException` for JNI's
/// failures and panics, and returns null or 0.
struct Throw;

impl<T: Default> ErrorPolicy<T, Fail> for Throw {
    type Captures<'unowned_env_local: 'native_method, 'native_method> = ();

    fn on_error<'unowned_env_local: 'native_method, 'native_method>(
        env: &mut Env<'unowned_env_local>,
        _cap: &mut (),
        err: Fail,
    ) -> jni::errors::Result<T> {
        if env.exception_check() {
            return Ok(T::default());
        }
        let _ = match err {
            Fail::Core(e) => {
                env.throw_new(jni_str!("io/arugula/core/CoreError"), JNIString::from(error_json(&e).to_string()))
            }
            Fail::Jni(e) => env.throw(format!("{e}")),
        };
        Ok(T::default())
    }

    fn on_panic<'unowned_env_local: 'native_method, 'native_method>(
        env: &mut Env<'unowned_env_local>,
        _cap: &mut (),
        payload: Box<dyn std::any::Any + Send + 'static>,
    ) -> jni::errors::Result<T> {
        let why = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        std::mem::forget(payload);
        if !env.exception_check() {
            let _ = env.throw(format!("the core panicked: {why}"));
        }
        Ok(T::default())
    }
}

/// Run `f` as a native method's body, under [`Throw`].
fn run<'local, T: Default + 'local>(mut env: EnvUnowned<'local>, f: impl FnOnce(&mut Env<'local>) -> Res<T>) -> T {
    env.with_env(f).resolve::<Throw>()
}

fn text(env: &Env, s: &JString) -> Res<String> {
    Ok(s.try_to_string(env)?)
}

fn text_or_null(env: &Env, s: &JString) -> Res<Option<String>> {
    if s.is_null() { Ok(None) } else { text(env, s).map(Some) }
}

fn c(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

fn jstr(env: &mut Env, s: &str) -> Res<jstring> {
    Ok(env.new_string(s)?.into_raw())
}

/// A string `ffi` allocated (or NULL) as Kotlin's, freed.
fn taken(env: &mut Env, p: *mut c_char) -> Res<jstring> {
    if p.is_null() {
        return Ok(ptr::null_mut());
    }
    let s = unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { ffi::aru_string_free(p) };
    jstr(env, &s)
}

/// An `ffi` call's answer, or `CoreError` with the JSON it set in `*err`.
fn answer(env: &mut Env, p: *mut c_char, err: *mut c_char) -> Res<jstring> {
    if p.is_null() {
        let why = if err.is_null() {
            json!({ "error": "the core gave no answer" }).to_string()
        } else {
            let s = unsafe { std::ffi::CStr::from_ptr(err) }.to_string_lossy().into_owned();
            unsafe { ffi::aru_string_free(err) };
            s
        };
        env.throw_new(jni_str!("io/arugula/core/CoreError"), JNIString::from(why))?;
        return Ok(ptr::null_mut());
    }
    taken(env, p)
}

fn ints(env: &Env, a: &JIntArray) -> Res<Vec<u32>> {
    let mut v = vec![0; a.len(env)?];
    a.get_region(env, 0, &mut v)?;
    Ok(v.into_iter().map(|i| i as u32).collect())
}

fn bytes(env: &Env, a: &JByteArray) -> Res<Vec<u8>> {
    Ok(env.convert_byte_array(a)?)
}

fn byte_array(env: &mut Env, b: &[u8]) -> Res<jbyteArray> {
    Ok(env.byte_array_from_slice(b)?.into_raw())
}

/// Kotlin's `Notify`, called from the core's threads: `ready()` says there's
/// something to take.
fn notifier(env: &mut Env, notify: &JObject) -> Res<impl Fn() + Send + 'static> {
    let vm: JavaVM = env.get_java_vm()?;
    let obj: Global<JObject<'static>> = env.new_global_ref(notify)?;
    Ok(move || {
        let _ = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
            env.call_method(&obj, jni_str!("ready"), jni_sig!("()V"), &[])?;
            Ok(())
        });
    })
}

// ---- once, before anything else

/// Hands the app's context to the TLS verifier, which asks Android's trust
/// store about control's certificate. Off Android it does nothing.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_init<'l>(env: EnvUnowned<'l>, _: JClass<'l>, context: JObject<'l>) {
    run(env, |env| {
        #[cfg(target_os = "android")]
        rustls_platform_verifier::android::init_with_env(env, context)?;
        #[cfg(not(target_os = "android"))]
        let _ = (env, context);
        Ok(())
    })
}

// ---- signing in

/// As `aru_check_app`: throws `CoreError` (`update_app` when control no
/// longer serves this build).
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_checkApp<'l>(env: EnvUnowned<'l>, _: JClass<'l>, url: JString<'l>) {
    run(env, |env| {
        let url = c(&text(env, &url)?);
        let mut err = ptr::null_mut();
        if unsafe { ffi::aru_check_app(url.as_ptr(), &mut err) } == 0 {
            answer(env, ptr::null_mut(), err)?;
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_signInStart<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    url: JString<'l>,
    name: JString<'l>,
) -> jlong {
    run(env, |env| {
        let s = SignIn::start(&text(env, &url)?, &text(env, &name)?)?;
        Ok(Box::into_raw(Box::new(ffi::AruSignIn(s))) as jlong)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_signInInfo<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
) -> jstring {
    run(env, |env| taken(env, unsafe { ffi::aru_signin_info(h as *const ffi::AruSignIn) }))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_signInFinish<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
    callback: JString<'l>,
) -> jstring {
    run(env, |env| {
        let callback = c(&text(env, &callback)?);
        let mut err = ptr::null_mut();
        let got = unsafe { ffi::aru_signin_finish(h as *const ffi::AruSignIn, callback.as_ptr(), &mut err) };
        answer(env, got, err)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_signInFree<'l>(_: EnvUnowned<'l>, _: JClass<'l>, h: jlong) {
    unsafe { ffi::aru_signin_free(h as *mut ffi::AruSignIn) }
}

// ---- control

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_controlOpen<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    stored: JString<'l>,
) -> jlong {
    run(env, |env| {
        let stored = c(&text(env, &stored)?);
        let mut err = ptr::null_mut();
        let got = unsafe { ffi::aru_control_open(stored.as_ptr(), &mut err) };
        if got.is_null() {
            answer(env, ptr::null_mut(), err)?;
        }
        Ok(got as jlong)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_controlSignOut<'l>(_: EnvUnowned<'l>, _: JClass<'l>, h: jlong) {
    unsafe { ffi::aru_control_sign_out(h as *const Control) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_controlCall<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
    method: JString<'l>,
    path: JString<'l>,
    body: JString<'l>,
) -> jstring {
    run(env, |env| {
        let (method, path) = (c(&text(env, &method)?), c(&text(env, &path)?));
        let body = text_or_null(env, &body)?.map(|b| c(&b));
        let mut err = ptr::null_mut();
        let got = unsafe {
            ffi::aru_control_call(
                h as *const Control,
                method.as_ptr(),
                path.as_ptr(),
                body.as_ref().map_or(ptr::null(), |b| b.as_ptr()),
                &mut err,
            )
        };
        answer(env, got, err)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_controlFree<'l>(_: EnvUnowned<'l>, _: JClass<'l>, h: jlong) {
    unsafe { ffi::aru_control_free(h as *mut Control) }
}

// ---- which sessions show (Arugula #1355)

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_shown<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    at_id: JString<'l>,
    at_personal: jboolean,
    session_project: JString<'l>,
    on_issue_project: JString<'l>,
    yours: jboolean,
) -> jboolean {
    run(env, |env| {
        let at = text_or_null(env, &at_id)?.map(|s| c(&s));
        let session = text_or_null(env, &session_project)?.map(|s| c(&s));
        let issue = text_or_null(env, &on_issue_project)?.map(|s| c(&s));
        let p = |s: &Option<CString>| s.as_ref().map_or(ptr::null(), |s| s.as_ptr());
        let got = unsafe { ffi::aru_shown(p(&at), at_personal as i32, p(&session), p(&issue), yours as i32) };
        Ok((got != 0) as jboolean)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_machines<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    input: JString<'l>,
) -> jstring {
    run(env, |env| {
        let input = c(&text(env, &input)?);
        let mut err = ptr::null_mut();
        let got = unsafe { ffi::aru_machines(input.as_ptr(), &mut err) };
        answer(env, got, err)
    })
}

// ---- a machine

/// As `aru_link_open`, with Kotlin's `Notify` in place of a C callback: the
/// socket's thread owns it, so it lives as long as anything can call it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_linkOpen<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    control: jlong,
    machine: JString<'l>,
    notify: JObject<'l>,
) -> jlong {
    run(env, |env| {
        let machine = text(env, &machine)?;
        let tell = notifier(env, &notify)?;
        let link = Link::open_notifying(unsafe { &*(control as *const Control) }, &machine, tell)?;
        Ok(Box::into_raw(Box::new(AruLink { link, events: Mutex::default() })) as jlong)
    })
}

/// As `aru_link_feed_many`: `terms[i]` (a terminal's handle, or 0) takes
/// `panes[i]`'s bytes. The same bits back.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_linkFeed<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
    panes: JIntArray<'l>,
    terms: JLongArray<'l>,
) -> jint {
    run(env, |env| {
        let panes = ints(env, &panes)?;
        let mut handles = vec![0; terms.len(env)?];
        terms.get_region(env, 0, &mut handles)?;
        let terms: Vec<*mut AruTerm> = handles.iter().map(|&t| t as *mut AruTerm).collect();
        let n = panes.len().min(terms.len());
        Ok(unsafe { ffi::aru_link_feed_many(h as *mut AruLink, panes.as_ptr(), terms.as_ptr(), n) } as jint)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_linkPanes<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
) -> jstring {
    run(env, |env| taken(env, unsafe { ffi::aru_link_panes(h as *const AruLink) }))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_linkClosed<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
) -> jstring {
    run(env, |env| taken(env, unsafe { ffi::aru_link_closed(h as *const AruLink) }))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_linkSaid<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
) -> jstring {
    run(env, |env| taken(env, unsafe { ffi::aru_link_said(h as *const AruLink) }))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_linkWatch<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
    tab: jint,
    panes: JIntArray<'l>,
    cols: jint,
    rows: jint,
) {
    run(env, |env| {
        let panes = ints(env, &panes)?;
        unsafe {
            ffi::aru_link_watch(h as *const AruLink, tab as u32, panes.as_ptr(), panes.len(), cols as u16, rows as u16)
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_linkDetach<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
    panes: JIntArray<'l>,
) {
    run(env, |env| {
        let panes = ints(env, &panes)?;
        unsafe { ffi::aru_link_detach(h as *const AruLink, panes.as_ptr(), panes.len()) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_linkAttach<'l>(
    _: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
    tab: jint,
    pane: jint,
    cols: jint,
    rows: jint,
) {
    unsafe { ffi::aru_link_attach(h as *const AruLink, tab as u32, pane as u32, cols as u16, rows as u16) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_linkView<'l>(
    _: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
    tab: jint,
    pane: jint,
    cols: jint,
    rows: jint,
) {
    unsafe { ffi::aru_link_view(h as *const AruLink, tab as u32, pane as u32, cols as u16, rows as u16) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_linkInput<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
    pane: jint,
    data: JByteArray<'l>,
) {
    run(env, |env| {
        let data = bytes(env, &data)?;
        unsafe { ffi::aru_link_input(h as *const AruLink, pane as u32, data.as_ptr(), data.len()) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_linkRequest<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
    method: JString<'l>,
    path: JString<'l>,
    body: JString<'l>,
) -> jstring {
    run(env, |env| {
        let (method, path) = (c(&text(env, &method)?), c(&text(env, &path)?));
        let body = text_or_null(env, &body)?.map(|b| c(&b));
        let mut err = ptr::null_mut();
        let got = unsafe {
            ffi::aru_link_request(
                h as *const AruLink,
                method.as_ptr(),
                path.as_ptr(),
                body.as_ref().map_or(ptr::null(), |b| b.as_ptr()),
                &mut err,
            )
        };
        answer(env, got, err)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_linkFree<'l>(_: EnvUnowned<'l>, _: JClass<'l>, h: jlong) {
    unsafe { ffi::aru_link_free(h as *mut AruLink) }
}

// ---- chat's live stream

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_chatOpen<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    control: jlong,
    notify: JObject<'l>,
) -> jlong {
    run(env, |env| {
        let tell = notifier(env, &notify)?;
        let live = ChatLive::open(unsafe { &*(control as *const Control) }, tell)?;
        Ok(Box::into_raw(Box::new(AruChatLive(live))) as jlong)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_chatNext<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
) -> jstring {
    run(env, |env| taken(env, unsafe { ffi::aru_chat_live_next(h as *const AruChatLive) }))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_chatFree<'l>(_: EnvUnowned<'l>, _: JClass<'l>, h: jlong) {
    unsafe { ffi::aru_chat_live_free(h as *mut AruChatLive) }
}

// ---- the terminal

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_termNew<'l>(
    _: EnvUnowned<'l>,
    _: JClass<'l>,
    cols: jint,
    rows: jint,
) -> jlong {
    ffi::aru_term_new(cols as u16, rows as u16) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_termFree<'l>(_: EnvUnowned<'l>, _: JClass<'l>, h: jlong) {
    unsafe { ffi::aru_term_free(h as *mut AruTerm) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_termResize<'l>(
    _: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
    cols: jint,
    rows: jint,
    cell_w: jint,
    cell_h: jint,
) {
    unsafe { ffi::aru_term_resize(h as *mut AruTerm, cols as u16, rows as u16, cell_w as u32, cell_h as u32) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_termScroll<'l>(
    _: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
    delta: jint,
) {
    unsafe { ffi::aru_term_scroll(h as *mut AruTerm, delta) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_termScrollBottom<'l>(_: EnvUnowned<'l>, _: JClass<'l>, h: jlong) {
    unsafe { ffi::aru_term_scroll_bottom(h as *mut AruTerm) }
}

/// As `aru_term_key`: the bytes for a key under the terminal's modes.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_termKey<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
    code: jint,
    mods: jint,
    key_text: JString<'l>,
) -> jbyteArray {
    run(env, |env| {
        let key_text = text_or_null(env, &key_text)?.map(|t| c(&t));
        let mut out = [0u8; 64];
        let n = unsafe {
            ffi::aru_term_key(
                h as *mut AruTerm,
                code as u32,
                mods as u32,
                key_text.as_ref().map_or(ptr::null(), |t| t.as_ptr()),
                out.as_mut_ptr(),
                out.len(),
            )
        };
        byte_array(env, &out[..n])
    })
}

/// As `aru_term_render`, as one [`frame`].
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_arugula_core_Native_termRender<'l>(
    env: EnvUnowned<'l>,
    _: JClass<'l>,
    h: jlong,
) -> jbyteArray {
    run(env, |env| {
        let mut rows = Rows::default();
        let mut f = AruFrame::default();
        unsafe { ffi::aru_term_render(h as *mut AruTerm, (&raw mut rows).cast(), row, &mut f) };
        byte_array(env, &frame(&f, &rows))
    })
}

#[derive(Default)]
struct Rows {
    count: u16,
    bytes: Vec<u8>,
}

extern "C" fn row(ctx: *mut c_void, y: u16, runs: *const AruRun, n: usize) {
    let rows = unsafe { &mut *ctx.cast::<Rows>() };
    let runs = if n == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(runs, n) } };
    rows.count += 1;
    let b = &mut rows.bytes;
    b.extend_from_slice(&y.to_le_bytes());
    b.extend_from_slice(&(runs.len() as u16).to_le_bytes());
    for r in runs {
        let text = if r.text_len == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(r.text, r.text_len) } };
        b.extend_from_slice(&r.col.to_le_bytes());
        b.extend_from_slice(&r.cells.to_le_bytes());
        b.extend_from_slice(&r.fg.to_le_bytes());
        b.extend_from_slice(&r.bg.to_le_bytes());
        b.push(r.flags);
        b.extend_from_slice(&(text.len() as u16).to_le_bytes());
        b.extend_from_slice(text);
    }
}

/// A frame for `Frame.decode` (Kotlin), little-endian: `dirty` u8,
/// `cursor_visible` u8, `cols` `rows` `cursor_x` `cursor_y` u16, `fg` `bg`
/// `cursor_color` u32, the number of rows u16; then each changed row, `y` u16
/// and its runs' count u16, each run `col` `cells` u16, `fg` `bg` u32, `flags`
/// u8, its UTF-8 text's length u16 and the text.
fn frame(f: &AruFrame, rows: &Rows) -> Vec<u8> {
    let mut b = Vec::with_capacity(28 + rows.bytes.len());
    b.push(f.dirty as u8);
    b.push(u8::from(f.cursor_visible));
    for v in [f.cols, f.rows, f.cursor_x, f.cursor_y] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for v in [f.fg, f.bg, f.cursor_color] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&rows.count.to_le_bytes());
    b.extend_from_slice(&rows.bytes);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_crosses_as_kotlin_reads_it() {
        let t = ffi::aru_term_new(10, 2);
        unsafe {
            (*t).pane.output(b"\x1b[1mhi\x1b[0m there");
            let mut rows = Rows::default();
            let mut f = AruFrame::default();
            ffi::aru_term_render(t, (&raw mut rows).cast(), row, &mut f);
            let b = frame(&f, &rows);
            ffi::aru_term_free(t);
            assert_eq!(b[0], 2, "the first frame is all dirty");
            assert_eq!(u16::from_le_bytes([b[2], b[3]]), 10, "cols");
            assert_eq!(u16::from_le_bytes([b[22], b[23]]), 2, "both rows");
            // Row 0: "hi" bold, then " there" plain.
            let r = &b[24..];
            assert_eq!(u16::from_le_bytes([r[0], r[1]]), 0);
            assert_eq!(u16::from_le_bytes([r[2], r[3]]), 2, "two runs");
            assert_eq!(r[16], 1, "the first is bold");
            assert_eq!(u16::from_le_bytes([r[17], r[18]]), 2);
            assert_eq!(&r[19..21], b"hi");
        }
    }

    /// What TermTest.kt expects on the device.
    #[test]
    fn the_next_frame_is_clean_and_keys_are_their_bytes() {
        let t = ffi::aru_term_new(80, 24);
        unsafe {
            let mut f = AruFrame::default();
            let mut rows = Rows::default();
            ffi::aru_term_render(t, (&raw mut rows).cast(), row, &mut f);
            assert_eq!((f.dirty, rows.count), (2, 24));
            let mut rows = Rows::default();
            ffi::aru_term_render(t, (&raw mut rows).cast(), row, &mut f);
            assert_eq!((f.dirty, rows.count), (0, 0));
            let mut out = [0u8; 8];
            let c = c"c";
            let n = ffi::aru_term_key(t, u32::from(b'C'), 2, c.as_ptr(), out.as_mut_ptr(), out.len());
            assert_eq!(&out[..n], &[3]);
            ffi::aru_term_free(t);
        }
    }
}
