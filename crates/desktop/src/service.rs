//! macOS: the daemon as the app's own launch agent (M46).
//!
//! The bundle carries a launch agent
//! (`Contents/Library/LaunchAgents/io.arugula.desktop.daemon.plist`) that
//! runs the bundled `arugulad`. On a Mac with no daemon yet, the app
//! registers it through SMAppService, so it shows under Login Items as
//! Arugula's, starts at login, and runs the copy inside the app: an app
//! update brings a new daemon with it. The plist's program is relative to
//! the bundle (`BundleProgram`, #177), so that's wherever the app is:
//! /Applications or ~/Applications, under either name (#505: an app from
//! before the rename updates in place and keeps illogical.app). Not from a
//! disk image or a translocated copy (`usable`), which go away: an app run
//! from there installs the daemon with `arugulad install` instead.
//!
//! The plist is the bundle's, so it carries no daemon flags: `arugulad
//! install -- FLAGS` keeps them in the state dir's `daemon-args.json`,
//! which the daemon reads when this agent starts it with none (#550).
//!
//! A daemon that `arugulad install` (install.sh, Homebrew, an older
//! app) set up keeps its own plist in `~/Library/LaunchAgents` (or
//! `/Library/LaunchDaemons` with `--system`): the app adopts that one and
//! never registers a second.

use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use arugula_proto::service::APP_LABEL;
use objc2_foundation::NSString;
use objc2_service_management::{SMAppService, SMAppServiceStatus};

pub const LABEL: &str = APP_LABEL;

/// This app can be the one the agent runs: a bundle that carries
/// arugulad, and that stays where it is.
pub fn usable() -> bool {
    crate::bundled("arugulad").is_some_and(|p| runs(&p))
}

/// `exe` is a bundle's daemon the agent's plist can run: in an installed
/// app, not on a disk image (`/Volumes/...`) or in a copy Gatekeeper
/// translocated (an app opened where it was downloaded), which are gone
/// once the app quits.
fn runs(exe: &std::path::Path) -> bool {
    let s = exe.to_string_lossy();
    // The default file system ignores case: Arugula.app is arugula.app.
    let in_app = s.to_ascii_lowercase().ends_with(".app/contents/macos/arugulad");
    in_app && !s.starts_with("/Volumes/") && !s.contains("/AppTranslocation/")
}

fn agent() -> objc2::rc::Retained<SMAppService> {
    unsafe { SMAppService::agentServiceWithPlistName(&NSString::from_str(&format!("{APP_LABEL}.plist"))) }
}

/// What SMAppService says of the agent.
pub fn status() -> &'static str {
    let s = unsafe { agent().status() };
    match s {
        SMAppServiceStatus::NotRegistered => "not registered",
        SMAppServiceStatus::Enabled => "enabled",
        SMAppServiceStatus::RequiresApproval => "requires approval",
        SMAppServiceStatus::NotFound => "not found",
        _ => "unknown",
    }
}

/// The agent is registered (and allowed to run).
pub fn registered() -> bool {
    unsafe { agent().status() == SMAppServiceStatus::Enabled }
}

/// Register the agent, which starts it.
pub fn register() -> Result<(), String> {
    let a = agent();
    unsafe { a.registerAndReturnError() }.map_err(|e| e.localizedDescription().to_string())?;
    match unsafe { a.status() } {
        SMAppServiceStatus::Enabled => Ok(()),
        SMAppServiceStatus::RequiresApproval => {
            Err("macOS wants you to allow Arugula in System Settings > General > Login Items before its daemon runs."
                .into())
        }
        _ => Err(format!("SMAppService says the daemon's agent is {}", status())),
    }
}

pub fn unregister() -> Result<(), String> {
    unsafe { agent().unregisterAndReturnError() }.map_err(|e| e.localizedDescription().to_string())
}

/// Restart the running agent on the bundle's (newer) daemon. Panes keep
/// running: the agent sets `ARUGULA_KEEP_PANES`, as `arugulad
/// install`'s plist does.
pub fn restart() -> Result<(), String> {
    let uid = unsafe { libc_getuid() };
    let mut kick = Command::new("launchctl")
        .args(["kickstart", "-k", &format!("gui/{uid}/{LABEL}")])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    // `kickstart` waits for as long as launchd can't start the program,
    // which is for good once the app has moved: stop waiting then.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = kick.try_wait().map_err(|e| e.to_string())? {
            if status.success() {
                return Ok(());
            }
            let mut err = String::new();
            if let Some(mut e) = kick.stderr.take() {
                let _ = e.read_to_string(&mut err);
            }
            return Err(format!("launchctl kickstart: {}", err.trim()));
        }
        if spawn_failed() || Instant::now() > deadline {
            let _ = kick.kill();
            let _ = kick.wait();
            return Err("launchd can't start the daemon's launch agent".into());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// launchd couldn't start the agent's program: the bundle it was
/// registered from isn't there any more (it says `spawn failed`, exit 78).
pub fn spawn_failed() -> bool {
    let uid = unsafe { libc_getuid() };
    Command::new("launchctl")
        .args(["print", &format!("gui/{uid}/{LABEL}")])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("job state = spawn failed"))
}

/// A plist from `arugulad install` is there: that install owns the daemon.
/// Its launch agent (label `arugulad`, what install.sh sets up), or the
/// LaunchDaemon `arugulad install --system` wrote, which a later
/// `arugulad install` (install.sh run again) keeps.
pub fn installed_by_script() -> bool {
    let l = arugula_proto::service::LABEL;
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let user = std::env::var("USER").unwrap_or_default();
    home.is_some_and(|h| h.join(format!("Library/LaunchAgents/{l}.plist")).is_file())
        || std::path::Path::new(&format!("/Library/LaunchDaemons/{l}.{user}.plist")).is_file()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_agent_runs_an_installed_app() {
        let runs = |p: &str| super::runs(p.as_ref());
        assert!(runs("/Applications/Arugula.app/Contents/MacOS/arugulad"));
        assert!(runs("/Applications/arugula.app/Contents/MacOS/arugulad"));
        assert!(runs("/Applications/illogical.app/Contents/MacOS/arugulad"));
        assert!(runs("/Users/me/Applications/Arugula.app/Contents/MacOS/arugulad"));
        assert!(!runs("/Volumes/Arugula/Arugula.app/Contents/MacOS/arugulad"));
        assert!(!runs("/private/var/folders/x/T/AppTranslocation/0A1B/d/Arugula.app/Contents/MacOS/arugulad"));
        assert!(!runs("/Users/me/src/arugula/target/release/arugulad"));
    }
}

unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}
