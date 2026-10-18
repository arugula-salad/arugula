//! Whether this machine sends telemetry (M101, #983): its owner's answer,
//! and the rules that keep it off whatever they said. The daemon sends the
//! reports (`crates/daemon/src/telemetry.rs`); what's in one is
//! `arugula_control_wire::telemetry`. `docs/telemetry.md` says it all for
//! people.
//!
//! The answer lives in one file in the state dir, `telemetry.json`:
//!
//! ```json
//! { "telemetry": true, "asked": 1760000000000, "via": "web", "install": "0123456789abcdef" }
//! ```
//!
//! - **No file is not asked, and not asked is off.** So is a file that
//!   doesn't parse: it is logged once and read as no file.
//! - **Yes makes the install id; no deletes it.** No also deletes what was
//!   counted and not yet sent ([`COUNTS_FILE`]) and the crashes waiting
//!   ([`crash::DIR`]). A yes after a no is a new install id, so the two
//!   can't be joined up.
//! - **Some things keep it off, whatever the file says** ([`enabled`]):
//!   `DO_NOT_TRACK`, `ARUGULA_TELEMETRY=off`, `CI`, and tests (nextest, and
//!   the testkit's daemons, which set `ARUGULA_TELEMETRY=off`).
//! - **Reads are never cached and writes rename a new file over the old**,
//!   as `flags.json`'s are, so `arugulad telemetry` works with the daemon
//!   stopped and a running one follows. Keys it doesn't know are kept.

use std::{
    fs, io,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub mod crash;

/// The owner's answer, in the state dir.
pub const FILE: &str = "telemetry.json";
/// What the daemon counted and hasn't sent yet, in the state dir: today's
/// counts and the reports waiting to go. Deleted by a no.
pub const COUNTS_FILE: &str = "telemetry.counts.json";
/// `0`, `off` or `false` keeps telemetry off.
pub const ENV: &str = "ARUGULA_TELEMETRY";
/// Where reports go instead of [`HOSTED`].
pub const URL_ENV: &str = "ARUGULA_TELEMETRY_URL";
/// The hosted control, which takes reports and is where they go.
pub const HOSTED: &str = "https://control.arugula.io";

/// `telemetry.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Consent {
    pub telemetry: bool,
    /// When they answered, in ms.
    #[serde(default)]
    pub asked: u64,
    /// Where they answered: `web`, `desktop`, `cli`, `tui` (an older
    /// CLI's TUI), `phone`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// 16 hex digits, while the answer is yes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install: Option<String>,
    /// What an older or newer daemon wrote that this one doesn't know.
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

/// Why telemetry is on or off here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "TelemetryWhy"))]
pub enum Why {
    /// The owner said yes.
    Yes,
    /// Nobody has answered yet.
    NotAsked,
    /// The owner said no.
    No,
    /// `DO_NOT_TRACK` is set.
    DoNotTrack,
    /// `ARUGULA_TELEMETRY` says off.
    Env,
    /// `CI` is set.
    Ci,
    /// A test run.
    Test,
}

impl Why {
    /// A sentence for `arugulad telemetry status`.
    pub fn line(self) -> &'static str {
        match self {
            Why::Yes => "on: the owner said yes",
            Why::NotAsked => "off: not asked yet",
            Why::No => "off: the owner said no",
            Why::DoNotTrack => "off: DO_NOT_TRACK is set",
            Why::Env => "off: ARUGULA_TELEMETRY says off",
            Why::Ci => "off: CI is set",
            Why::Test => "off: a test run",
        }
    }
}

/// Whether telemetry is on, why, and the answer it comes from.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub on: bool,
    pub why: Why,
    /// The file, if there's one that parses.
    pub consent: Option<Consent>,
}

impl State {
    /// The install id, when it's on.
    pub fn install(&self) -> Option<&str> {
        self.consent.as_ref().filter(|_| self.on)?.install.as_deref()
    }
}

/// Whether this machine sends telemetry now, from the environment and the
/// file. Read on every call. A test build of this crate is always off.
pub fn enabled(state_dir: &Path) -> State {
    let s = enabled_in(state_dir, &|k| std::env::var(k).ok());
    if cfg!(test) { State { on: false, why: Why::Test, ..s } } else { s }
}

/// [`enabled`], with the environment `env` looks up (a test's own).
pub fn enabled_in(state_dir: &Path, env: &dyn Fn(&str) -> Option<String>) -> State {
    let consent = read(state_dir);
    let set = |k: &str| env(k).is_some_and(|v| !v.trim().is_empty());
    let no = |k: &str| env(k).is_some_and(|v| ["0", "off", "false", "no"].contains(&v.trim().to_lowercase().as_str()));
    let off = if set("NEXTEST_RUN_ID") {
        Some(Why::Test)
    } else if set("DO_NOT_TRACK") && !no("DO_NOT_TRACK") {
        Some(Why::DoNotTrack)
    } else if no(ENV) {
        Some(Why::Env)
    } else if set("CI") {
        Some(Why::Ci)
    } else {
        None
    };
    let why = off.unwrap_or(match &consent {
        None => Why::NotAsked,
        Some(c) if c.telemetry && c.install.as_deref().is_some_and(is_install) => Why::Yes,
        Some(_) => Why::No,
    });
    State { on: why == Why::Yes, why, consent }
}

/// Where reports go: `ARUGULA_TELEMETRY_URL`, or [`HOSTED`].
pub fn url_in(env: &dyn Fn(&str) -> Option<String>) -> String {
    env(URL_ENV).filter(|u| !u.trim().is_empty()).unwrap_or_else(|| HOSTED.into()).trim_end_matches('/').to_owned()
}

/// 16 lowercase hex digits.
pub fn is_install(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// The file, if there is one and it parses.
pub fn read(state_dir: &Path) -> Option<Consent> {
    let path = state_dir.join(FILE);
    let text = fs::read_to_string(&path).ok().filter(|t| !t.trim().is_empty())?;
    match serde_json::from_str(&text) {
        Ok(c) => Some(c),
        Err(e) => {
            static SAID: AtomicBool = AtomicBool::new(false);
            if !SAID.swap(true, Ordering::Relaxed) {
                tracing::warn!(file = %path.display(), error = %e, "telemetry.json doesn't parse: telemetry is off");
            }
            None
        }
    }
}

/// The owner's answer, `via` where they gave it, at `now` (ms). Yes keeps
/// the install id it had, or makes one; no deletes it, what was counted and
/// the crashes waiting.
pub fn set(state_dir: &Path, on: bool, via: &str, now: u64) -> io::Result<Consent> {
    let mut c = read(state_dir).unwrap_or_default();
    let kept = c.install.take().filter(|i| c.telemetry && is_install(i));
    c.telemetry = on;
    c.asked = now;
    c.via = Some(via.to_owned());
    c.install = if on { Some(kept.map_or_else(new_install, Ok)?) } else { None };
    let mut text = serde_json::to_string_pretty(&c).expect("the consent serializes");
    text.push('\n');
    crate::flags::write_atomic(&state_dir.join(FILE), text.as_bytes())?;
    if !on {
        match fs::remove_file(state_dir.join(COUNTS_FILE)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        crash::clear(state_dir)?;
    }
    Ok(c)
}

fn new_install() -> io::Result<String> {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).map_err(|e| io::Error::other(format!("no random source: {e}")))?;
    Ok(b.iter().map(|b| format!("{b:02x}")).collect())
}

/// `GET /api/telemetry` (the `telemetry.get` operation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TelemetryInfo {
    pub on: bool,
    pub why: Why,
    /// When the owner answered (ms), if they have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub asked: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub via: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub install: Option<String>,
    /// Where reports go.
    pub url: String,
    /// Reports are signed as this machine: it's joined to the control they
    /// go to.
    pub signed: bool,
    /// The next report, exactly as it would be sent now (an
    /// `arugula_control_wire::telemetry::Report`). While it's off, what a
    /// yes would send first: no counts, a placeholder for the install id,
    /// and `"preview_only": true`. A daemon from M101 always gives one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "unknown"))]
    pub preview: Option<Value>,
}

/// `PUT /api/telemetry` (the `telemetry.set` operation): the owner's answer,
/// and where they gave it (`web` when not said).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TelemetrySetRequest {
    pub on: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub via: Option<String>,
}

/// `POST /api/telemetry/note` (the `telemetry.note` operation, M103): what
/// the web client counted since its last note, for the day's counts. Views
/// by `arugula_control_wire::telemetry::View`'s names (one it doesn't know
/// counts as `other`), and how many errors our code hit. Counts only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TelemetryNoteRequest {
    #[serde(default)]
    pub views: std::collections::BTreeMap<String, u32>,
    #[serde(default)]
    pub web_errors: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("arugula-telemetry-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// What a daemon with only `env` decides.
    fn decide(d: &Path, env: &[(&str, &str)]) -> Why {
        let env = |k: &str| env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        let s = enabled_in(d, &env);
        assert_eq!(s.on, s.why == Why::Yes);
        s.why
    }

    #[test]
    fn no_file_is_off_and_the_environment_keeps_it_off() {
        let d = dir("none");
        assert_eq!(decide(&d, &[]), Why::NotAsked);
        set(&d, true, "cli", 1).unwrap();
        assert_eq!(decide(&d, &[]), Why::Yes);
        assert_eq!(decide(&d, &[("DO_NOT_TRACK", "1")]), Why::DoNotTrack);
        assert_eq!(decide(&d, &[("DO_NOT_TRACK", "0")]), Why::Yes);
        assert_eq!(decide(&d, &[(ENV, "off")]), Why::Env);
        assert_eq!(decide(&d, &[(ENV, "on")]), Why::Yes, "the variable only turns it off");
        assert_eq!(decide(&d, &[("CI", "true")]), Why::Ci);
        assert_eq!(decide(&d, &[("NEXTEST_RUN_ID", "x")]), Why::Test);
        assert_eq!(enabled(&d).why, Why::Test, "a test build is always off");
        fs::write(d.join(FILE), r#"{"telemetry": true, "asked": 1}"#).unwrap();
        assert_eq!(decide(&d, &[]), Why::No, "yes with no install id isn't a yes");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn yes_makes_an_install_id_and_no_deletes_it_and_the_counts() {
        let d = dir("set");
        let a = set(&d, true, "web", 5).unwrap();
        let id = a.install.clone().unwrap();
        assert!(is_install(&id));
        assert_eq!((a.asked, a.via.as_deref()), (5, Some("web")));
        assert_eq!(set(&d, true, "cli", 6).unwrap().install.as_deref(), Some(id.as_str()), "a second yes keeps it");
        fs::write(d.join(COUNTS_FILE), "{}").unwrap();
        let no = set(&d, false, "cli", 7).unwrap();
        assert_eq!(no.install, None);
        assert!(!d.join(COUNTS_FILE).exists(), "no drops what was counted");
        let again = set(&d, true, "cli", 8).unwrap().install.unwrap();
        assert_ne!(again, id, "a yes after a no is a new install");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_that_doesnt_parse_is_not_asked_and_unknown_keys_stay() {
        let d = dir("bad");
        fs::write(d.join(FILE), "not json").unwrap();
        assert_eq!(read(&d), None);
        fs::write(d.join(FILE), r#"{"telemetry": false, "asked": 1, "newer": [1]}"#).unwrap();
        set(&d, true, "web", 2).unwrap();
        assert_eq!(read(&d).unwrap().rest["newer"], serde_json::json!([1]));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_url_is_the_hosted_controls_unless_told() {
        assert_eq!(url_in(&|_| None), HOSTED);
        assert_eq!(url_in(&|_| Some("http://127.0.0.1:9/".into())), "http://127.0.0.1:9");
    }
}
