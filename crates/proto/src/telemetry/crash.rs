//! Crash reports (M102, #984): a panic in `arugulad`, `arugula` or the
//! desktop app, written down for the daemon to send with its next report.
//!
//! - **[`install_hook`]** wraps the panic hook there is, and runs it after,
//!   so a panic prints as it always did.
//! - **Only while telemetry is on** ([`enabled`](super::enabled), one file
//!   read at the panic): off, nothing is written.
//! - **Where, never what:** the panic's file and line in our own source
//!   (`crates/…`), and our own functions on the stack
//!   (`arugula_mux::pane::Pane::resize`). Never the panic's message or
//!   payload, which can hold a path or someone's text.
//! - **The location always; frames when they can be had.** The location is
//!   the panic's own (`file!()`/`line!()`, compiled in). Frames come from a
//!   backtrace, which names functions only in a build that keeps its
//!   symbols: a stripped one (the desktop app's release) has none.
//! - **One file a crash,** `crashes/<ms>-<process>.json` in the state dir,
//!   renamed into place whole, as an `arugula_control_wire::telemetry::Crash`
//!   (this crate doesn't depend on that one, so [`Crash`] here is its twin;
//!   a daemon test holds them together). At most [`KEEP`] files: the oldest
//!   go.
//! - The daemon sends them and deletes each once a report holding it was
//!   taken (`crates/daemon/src/telemetry.rs`). A no empties `crashes/`.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

/// The directory crashes wait in, in the state dir.
pub const DIR: &str = "crashes";
/// The most crash files kept.
pub const KEEP: usize = 50;
/// The most frames a crash keeps (`arugula_control_wire::telemetry::MAX_FRAMES`).
pub const MAX_FRAMES: usize = 32;
/// Where a crash is when neither the panic nor any of our frames says.
pub const UNKNOWN: &str = "crates/unknown";
/// Set, a debug `arugulad` panics as it starts (M106's live check).
pub const TEST_PANIC_ENV: &str = "ARUGULA_TEST_PANIC";

/// Which of our programs crashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Process {
    Daemon,
    Cli,
    Desktop,
}

impl Process {
    fn name(self) -> &'static str {
        match self {
            Process::Daemon => "daemon",
            Process::Cli => "cli",
            Process::Desktop => "desktop",
        }
    }
}

/// A crash file: `arugula_control_wire::telemetry::Crash`, field for field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crash {
    pub process: Process,
    pub version: String,
    pub location: String,
    pub frames: Vec<String>,
    pub at: u64,
}

/// Write each panic in this process down as a crash, `version` being the
/// program's, in `state_dir`'s `crashes/` while telemetry is on there; then
/// the hook there was runs.
pub fn install_hook(process: Process, version: &'static str, state_dir: &Path) {
    let dir = state_dir.to_owned();
    install_with(process, version, state_dir, Box::new(move || super::enabled(&dir).on));
}

/// [`install_hook`], with `on` saying whether telemetry is on (a test's).
pub fn install_with(
    process: Process,
    version: &'static str,
    state_dir: &Path,
    on: Box<dyn Fn() -> bool + Send + Sync>,
) {
    let dir = state_dir.to_owned();
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if on() {
            let panic = info.location().map(|l| (l.file().to_owned(), l.line()));
            let trace = std::backtrace::Backtrace::force_capture().to_string();
            let crash = crash(process, version, panic, &trace, now_ms());
            // A panic hook that fails has nowhere left to say so.
            let _ = write(&dir, &crash);
        }
        before(info);
    }));
}

/// The crash a panic at `panic` (file, line) makes, from the backtrace
/// `trace` (as `Backtrace` prints it).
pub fn crash(process: Process, version: &str, panic: Option<(String, u32)>, trace: &str, at: u64) -> Crash {
    let frames = frames(process, trace);
    let location = panic
        .and_then(|(file, line)| ours(process, &file).map(|f| format!("{f}:{line}")))
        .or_else(|| frames.iter().find_map(|(_, at)| at.clone()))
        .unwrap_or_else(|| UNKNOWN.to_owned());
    let mut names: Vec<String> = Vec::new();
    for (name, _) in frames {
        if names.last() != Some(&name) {
            names.push(name);
        }
    }
    names.truncate(MAX_FRAMES);
    Crash { process, version: version.to_owned(), location, frames: names, at }
}

/// Our frames in `trace`, innermost first, below the panic machinery: each
/// one's name, and its `crates/…:line` when the trace has it.
fn frames(process: Process, trace: &str) -> Vec<(String, Option<String>)> {
    // (symbol, `at` of the line after it). A frame is `  N: name`; a
    // function inlined into it is another name under it, unnumbered; the
    // full format puts `0x… - ` before the name.
    let mut all: Vec<(String, Option<String>)> = Vec::new();
    for line in trace.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(at) = t.strip_prefix("at ") {
            if let Some(last) = all.last_mut().filter(|l| l.1.is_none()) {
                last.1 = Some(at.trim().to_owned());
            }
            continue;
        }
        let sym = match t.split_once(": ") {
            Some((n, sym)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => sym.trim_start(),
            _ => t,
        };
        let sym = match sym.split_once(" - ") {
            Some((ip, name)) if ip.starts_with("0x") => name,
            _ => sym,
        };
        all.push((sym.trim().to_owned(), None));
    }
    // The hook and the panic machinery are above the panic: what's ours
    // starts after the first of std's panicking frames.
    let start =
        all.iter().position(|(s, _)| s.contains("panicking::") || s.contains("rust_begin_unwind")).map_or(0, |i| i + 1);
    all.drain(..start);
    all.into_iter()
        .filter_map(|(sym, at)| {
            let name = name(&sym)?;
            let at = at.and_then(|a| {
                let (file, line) = file_line(&a)?;
                Some(format!("{}:{line}", ours(process, file)?))
            });
            Some((name, at))
        })
        .collect()
}

/// `path:line:column` (Unix) or `path:line` (Windows, whose paths can hold
/// a `:` of their own): the path and the line.
fn file_line(at: &str) -> Option<(&str, u32)> {
    let num = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let (rest, last) = at.rsplit_once(':').filter(|(_, n)| num(n))?;
    match rest.rsplit_once(':').filter(|(_, n)| num(n)) {
        Some((file, line)) => Some((file, line.parse().ok()?)),
        None => Some((rest, last.parse().ok()?)),
    }
}

/// Our crates' directory, where this one was built from.
fn crates_dir() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent().unwrap_or(here).to_owned()
}

/// `file` as `crates/…`, if it's in our source.
fn ours(process: Process, file: &str) -> Option<String> {
    let f = file.replace('\\', "/");
    let f = f.strip_prefix("./").unwrap_or(&f);
    let rel = if f.starts_with("crates/") {
        f.to_owned()
    } else if process == Process::Desktop && f.starts_with("src/") {
        // The desktop app is its own workspace: its paths start there.
        format!("crates/desktop/{f}")
    } else {
        let root = crates_dir().to_string_lossy().replace('\\', "/");
        let rest = f.strip_prefix(root.trim_end_matches('/'))?.strip_prefix('/')?;
        format!("crates/{rest}")
    };
    (!rel.contains("..") && plain(&rel)).then_some(rel)
}

/// The characters control takes in a location or a frame.
fn plain(s: &str) -> bool {
    s.len() <= 200 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_:/.-<>".contains(&b))
}

/// A symbol as a crash names it, if it's ours: no hash, no generic
/// arguments, no closures (`{{closure}}`, MSVC's `closure$0`, and its
/// `impl$0`), `<T as Trait>::f` as `T::f`.
fn name(sym: &str) -> Option<String> {
    let mut s = sym.trim();
    // `…::h0123456789abcdef`
    if let Some((head, hash)) = s.rsplit_once("::h")
        && hash.len() == 16
        && hash.bytes().all(|b| b.is_ascii_hexdigit())
    {
        s = head;
    }
    let owned;
    if let Some(inner) = s.strip_prefix('<') {
        // `<A as B>::rest` or `<A>::rest`: A, then rest.
        let mut depth = 0usize;
        let close = inner.char_indices().find(|&(_, c)| {
            match c {
                '<' => depth += 1,
                '>' if depth == 0 => return true,
                '>' => depth -= 1,
                _ => {}
            }
            false
        })?;
        let (ty, rest) = (&inner[..close.0], &inner[close.0 + 1..]);
        let ty = top_level_as(ty).unwrap_or(ty);
        owned = format!("{ty}{rest}");
        s = &owned;
    }
    let mut out = String::new();
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let name: Vec<&str> = out.split("::").filter(|seg| !seg.is_empty() && !seg.contains(['{', '$'])).collect();
    let krate = *name.first()?;
    let ours = krate == "arugula" || krate == "arugulad" || krate.starts_with("arugula_");
    let name = name.join("::");
    (ours && plain(&name)).then_some(name)
}

/// The type in `A as B`, where ` as ` isn't inside A's generics.
fn top_level_as(s: &str) -> Option<&str> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ' ' if depth == 0 && s[i..].starts_with(" as ") => return Some(&s[..i]),
            _ => {}
        }
    }
    None
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// The crash files in `state_dir`, oldest first, with when each was.
pub fn list(state_dir: &Path) -> Vec<(u64, PathBuf)> {
    let Ok(entries) = fs::read_dir(state_dir.join(DIR)) else { return Vec::new() };
    let mut files: Vec<(u64, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| {
            let ms = p.file_stem()?.to_str()?.split('-').next()?.parse().ok()?;
            Some((ms, p))
        })
        .collect();
    files.sort();
    files
}

/// Writes `crash` to its own file, and drops the oldest past [`KEEP`].
pub fn write(state_dir: &Path, crash: &Crash) -> io::Result<PathBuf> {
    let dir = state_dir.join(DIR);
    let mut at = crash.at;
    let mut path = dir.join(format!("{at}-{}.json", crash.process.name()));
    while path.exists() {
        at += 1;
        path = dir.join(format!("{at}-{}.json", crash.process.name()));
    }
    let text = serde_json::to_vec_pretty(crash).map_err(io::Error::other)?;
    crate::flags::write_atomic(&path, &text)?;
    let files = list(state_dir);
    for (_, old) in files.iter().take(files.len().saturating_sub(KEEP)) {
        let _ = fs::remove_file(old);
    }
    Ok(path)
}

/// Deletes every crash file (a no).
pub fn clear(state_dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(state_dir.join(DIR)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arugula-crash-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// The message the child panics with: it must never be written down.
    const SECRET: &str = "the-secret-in-the-message-/home/someone/private";
    const CHILD: &str = "ARUGULA_TEST_CRASH_DIR";
    const TRACE: &str = "--- the child's backtrace ---";

    /// The child of the hook tests: with the hook on the state dir it
    /// names, and telemetry on as that dir's file says, a thread panics.
    /// Run alone, it does nothing.
    #[test]
    fn panic_in_child() {
        let Some(d) = std::env::var_os(CHILD) else { return };
        let d = PathBuf::from(d);
        let at = d.clone();
        // The hook before ours: the backtrace as this process resolves it,
        // for the parent to tell whether frames could be named at all.
        let print = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            eprintln!("{TRACE}\n{}\n{TRACE}", std::backtrace::Backtrace::force_capture());
            print(info);
        }));
        // Not `enabled()`: a test build is always off, and so is nextest.
        install_with(Process::Cli, "0.42.0", &d, Box::new(move || super::super::enabled_in(&at, &|_| None).on));
        let _ = std::thread::spawn(|| panic!("{SECRET}")).join();
    }

    /// The hook is the process's, so it's tried in a child: this test
    /// binary again, running only `panic_in_child`. What it wrote.
    fn crash_in_child(d: &Path) -> (String, Vec<(u64, PathBuf)>) {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "telemetry::crash::tests::panic_in_child", "--nocapture", "--test-threads=1"])
            .env(CHILD, d)
            .env_remove("RUST_BACKTRACE")
            .output()
            .unwrap();
        (String::from_utf8_lossy(&out.stderr).into_owned(), list(d))
    }

    #[test]
    fn a_panic_with_telemetry_on_is_written_down_without_its_message() {
        let d = dir("on");
        super::super::set(&d, true, "cli", 1).unwrap();
        let (stderr, files) = crash_in_child(&d);
        assert!(stderr.contains(SECRET), "the hook before still prints it: {stderr}");
        assert_eq!(files.len(), 1, "{stderr}");
        let text = fs::read_to_string(&files[0].1).unwrap();
        assert!(!text.contains(SECRET) && !text.contains("secret") && !text.contains("/home/"), "{text}");
        let c: Crash = serde_json::from_str(&text).unwrap();
        assert_eq!((c.process, c.version.as_str()), (Process::Cli, "0.42.0"));
        assert!(c.location.starts_with("crates/proto/src/telemetry/crash.rs:"), "{c:?}");
        // Frames are best-effort: only a build whose backtraces name our
        // functions has them. The location always is.
        let trace = stderr.split(TRACE).nth(1).unwrap_or_default();
        assert!(c.frames.iter().all(|f| f.starts_with("arugula_proto::")), "{c:?}");
        if trace.contains("arugula_proto::") {
            let want = "arugula_proto::telemetry::crash::tests::panic_in_child";
            assert!(c.frames.iter().any(|f| f == want), "{c:?}\nfrom:\n{trace}");
        } else {
            eprintln!("this build's backtraces name none of our functions, so no frames: {trace}");
        }
        let name = files[0].1.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(name, format!("{}-cli.json", c.at));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn with_telemetry_off_no_crash_is_written() {
        let d = dir("off");
        let (stderr, files) = crash_in_child(&d);
        assert!(stderr.contains(SECRET), "{stderr}");
        assert!(files.is_empty(), "not asked");
        super::super::set(&d, false, "cli", 1).unwrap();
        assert!(crash_in_child(&d).1.is_empty(), "said no");
        assert!(!d.join(DIR).exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn past_fifty_the_oldest_go_and_a_no_empties_them() {
        let d = dir("keep");
        let c = |at| Crash {
            process: Process::Daemon,
            version: "0.42.0".into(),
            location: UNKNOWN.into(),
            frames: vec![],
            at,
        };
        for at in 1000..1060 {
            write(&d, &c(at)).unwrap();
        }
        let files = list(&d);
        assert_eq!(files.len(), KEEP);
        assert_eq!(files[0].0, 1010, "the oldest went");
        assert_eq!(files[KEEP - 1].0, 1059);
        write(&d, &c(1059)).unwrap();
        assert_eq!(list(&d).last().unwrap().0, 1060, "two in one ms are two files");
        super::super::set(&d, true, "cli", 1).unwrap();
        super::super::set(&d, false, "cli", 2).unwrap();
        assert!(list(&d).is_empty(), "a no deletes them");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn names_and_locations_are_ours_alone() {
        assert_eq!(
            name("arugula_mux::pane::Pane::resize::h0123456789abcdef").as_deref(),
            Some("arugula_mux::pane::Pane::resize")
        );
        assert_eq!(
            name("arugula_mux::pane::Pane::resize::{{closure}}").as_deref(),
            Some("arugula_mux::pane::Pane::resize")
        );
        assert_eq!(
            name("arugula_mux::mux::Mux::open::<alloc::string::String>").as_deref(),
            Some("arugula_mux::mux::Mux::open")
        );
        assert_eq!(
            name("<arugula_mux::pane::Pane<T> as core::ops::drop::Drop>::drop").as_deref(),
            Some("arugula_mux::pane::Pane::drop")
        );
        assert_eq!(
            name("arugula_proto::x::tests::panic_in_child::closure$1").as_deref(),
            Some("arugula_proto::x::tests::panic_in_child"),
            "MSVC's"
        );
        assert_eq!(name("arugula_mux::pane::impl$3::drop").as_deref(), Some("arugula_mux::pane::drop"));
        assert_eq!(name("arugulad::main").as_deref(), Some("arugulad::main"));
        assert_eq!(name("arugula::cmd::run").as_deref(), Some("arugula::cmd::run"));
        assert_eq!(name("<serde_json::de::Deserializer<R> as serde::Deserializer>::deserialize_any"), None);
        assert_eq!(name("arugulafake::x"), None);
        assert_eq!(name("std::rt::lang_start"), None);

        assert_eq!(ours(Process::Daemon, "crates/mux/src/pane.rs").as_deref(), Some("crates/mux/src/pane.rs"));
        assert_eq!(ours(Process::Daemon, "./crates/mux/src/pane.rs").as_deref(), Some("crates/mux/src/pane.rs"));
        let abs = crates_dir().join("mux/src/pane.rs");
        assert_eq!(ours(Process::Daemon, &abs.to_string_lossy()).as_deref(), Some("crates/mux/src/pane.rs"));
        assert_eq!(ours(Process::Desktop, "src/main.rs").as_deref(), Some("crates/desktop/src/main.rs"));
        assert_eq!(ours(Process::Daemon, "src/main.rs"), None);
        assert_eq!(ours(Process::Daemon, "/home/me/.cargo/registry/src/index.crates.io-1/serde-1.0/src/de.rs"), None);
        assert_eq!(ours(Process::Daemon, "/rustc/abc/library/core/src/option.rs"), None);
    }

    /// Windows prints `path:line`, with backslashes and a drive's colon.
    #[test]
    fn windows_frames_are_read_too() {
        assert_eq!(file_line("./crates/a.rs:12:9"), Some(("./crates/a.rs", 12)));
        assert_eq!(file_line(r".\crates\a.rs:12"), Some((r".\crates\a.rs", 12)));
        assert_eq!(file_line(r"C:\s\crates\a.rs:12"), Some((r"C:\s\crates\a.rs", 12)));
        assert_eq!(file_line("nowhere"), None);
        // As std prints it there (MSVC, a dev build).
        let trace = r"   0: std::panicking::panic_handler
             at /rustc/abc/library\std\src\panicking.rs:679
   1: core::panicking::panic_fmt
             at /rustc/abc/library\core\src\panicking.rs:80
   2: arugula_proto::telemetry::crash::tests::panic_in_child::closure$1
             at .\crates\proto\src\telemetry\crash.rs:312
   3: core::hint::black_box
             at /rustc/abc/library\core\src\hint.rs:491
";
        let c = crash(Process::Cli, "0.42.0", None, trace, 1);
        assert_eq!(c.frames, ["arugula_proto::telemetry::crash::tests::panic_in_child"]);
        assert_eq!(c.location, "crates/proto/src/telemetry/crash.rs:312");
    }

    /// A function inlined into a frame is a name under it with no number;
    /// the full format has the address first.
    #[test]
    fn inlined_and_full_frames_are_read_too() {
        let trace = "   0: core::panicking::panic_fmt
   1: std::thread::lifecycle::spawn_unchecked::{{closure}}
      arugula_proto::telemetry::crash::tests::panic_in_child::{{closure}}
             at ./crates/proto/src/telemetry/crash.rs:312:39
   2:     0x5555deadbeef - arugula_mux::pane::Pane::resize::h0123456789abcdef
";
        let c = crash(Process::Cli, "0.42.0", None, trace, 1);
        assert_eq!(
            c.frames,
            ["arugula_proto::telemetry::crash::tests::panic_in_child", "arugula_mux::pane::Pane::resize"]
        );
        assert_eq!(c.location, "crates/proto/src/telemetry/crash.rs:312");
    }

    /// A panic in a dependency is where the first of our frames called it.
    #[test]
    fn a_panic_outside_our_crates_is_our_first_frame() {
        let trace = "   0: std::backtrace::Backtrace::force_capture
             at /rustc/abc/library/std/src/backtrace.rs:312:9
   1: arugula_proto::telemetry::crash::install_with::{{closure}}
             at ./crates/proto/src/telemetry/crash.rs:80:25
   2: std::panicking::rust_panic_with_hook
   3: core::panicking::panic_fmt
   4: serde_json::de::from_str
             at /home/me/.cargo/registry/src/index.crates.io-1/serde_json-1.0/src/de.rs:10:5
   5: arugula_mux::pane::Pane::resize::{{closure}}
             at ./crates/mux/src/pane.rs:812:9
   6: arugula_mux::pane::Pane::resize
             at ./crates/mux/src/pane.rs:800:5
   7: std::rt::lang_start
";
        let dep = Some(("/home/me/.cargo/registry/src/index.crates.io-1/serde_json-1.0/src/de.rs".into(), 10));
        let c = crash(Process::Daemon, "0.42.0", dep, trace, 7);
        assert_eq!(c.location, "crates/mux/src/pane.rs:812");
        assert_eq!(c.frames, ["arugula_mux::pane::Pane::resize"], "not the hook's, and each once");
        let c = crash(Process::Daemon, "0.42.0", None, "", 7);
        assert_eq!((c.location.as_str(), c.frames.len()), (UNKNOWN, 0));
    }
}
