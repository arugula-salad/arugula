//! A hosted box follows control's release (Arugula #1284).
//!
//! A box runs the daemon its template was built with, and a workspace's
//! snapshot (M263) keeps that one through every rebuild, so without this a
//! box made in October offered a session October's tools long after
//! control had newer ones. Control says which daemon release it was built
//! against, the one at its `open/` pin (`/control.json`'s `build.daemon`,
//! Arugula #806), which also names the template new boxes are made from.
//!
//! So before the relay dials, at the daemon's start and after each resume,
//! the box holds the dial ([`Control::hold_dial`]) and reads that. When
//! control's release is newer than its own, it downloads it from the
//! releases (`arugulad update`'s download, checked against the release's
//! signed `SHA256SUMS`), puts `arugulad` and `arugula` where the running
//! ones are (with the template's sudo rule where it can't write there), and
//! starts the new daemon in place (`exec`), whose own look finds nothing to
//! do and dials. The hub sees only the new one, so a session the wake was
//! for starts on its tools. Its panes are kept, as on any restart of a
//! hosted daemon. It never goes back to an older release, and the disk,
//! with the work on it, stays as it is.
//!
//! When it can't (the releases out of reach, the download too slow for the
//! hub's wait, no sudo), it dials on the daemon it has, and [`note`] says
//! so, and how to try again, in what `capabilities` tells a session.
//!
//! [`Control::hold_dial`]: crate::control::Control::hold_dial

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, bail, ensure};
use tracing::{info, warn};

use crate::control::{Control, DialHold};

/// How long control has to answer `/control.json`.
const LOOK: Duration = Duration::from_secs(5);
/// How long the whole look, download and install may take before the box
/// dials on the daemon it has: inside the relay's own [`HOLD_MAX`].
///
/// [`HOLD_MAX`]: crate::control::HOLD_MAX
const BUDGET: Duration = Duration::from_secs(40);

/// What a session is told while this box's daemon is behind control's.
static NOTE: Mutex<Option<String>> = Mutex::new(None);
/// One look at a time (a resume can signal twice).
static LOOKING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Why this box's daemon is older than control's, when the last look found
/// it so and couldn't update it.
pub fn note() -> Option<String> {
    NOTE.lock().unwrap().clone()
}

/// The daemon release in a `/control.json`: `build.daemon`, `v0.70.3` →
/// `0.70.3`. None from a control that doesn't say.
pub fn release_in(control_json: &serde_json::Value) -> Option<String> {
    let v = control_json.get("build")?.get("daemon")?.as_str()?;
    let v = v.trim().trim_start_matches('v');
    crate::update::newer(v, "0.0.0").then(|| v.to_owned())
}

/// The release this box should move to: control's, when it's newer than
/// `ours`. Never an older one.
pub fn to_move_to(ours: &str, controls: Option<&str>) -> Option<String> {
    controls.filter(|c| crate::update::newer(c, ours)).map(str::to_owned)
}

/// The control this box is in, else the one it enrolls with.
fn control_url(control: &Control, enrolling: &str) -> String {
    let url = control.enrolled().map_or_else(|| enrolling.to_owned(), |e| e.saved.url.clone());
    url.trim_end_matches('/').to_owned()
}

async fn controls_release(url: &str) -> anyhow::Result<Option<String>> {
    let client = crate::roots::http().timeout(LOOK).user_agent("arugula").build()?;
    let res = client.get(format!("{url}/control.json")).send().await.context("asking control its release")?;
    ensure!(res.status().is_success(), "control's /control.json: {}", res.status());
    let json: serde_json::Value = res.json().await.context("reading control's /control.json")?;
    Ok(release_in(&json))
}

/// Look, and when control's release is newer, update to it and start it in
/// place. Returns (dropping `hold`, so the relay dials) when this daemon
/// stays. `enrolling` is control's URL for a box not yet enrolled.
pub async fn follow(control: Arc<Control>, enrolling: String, hold: DialHold) {
    let Ok(_one) = LOOKING.try_lock() else { return };
    let url = control_url(&control, &enrolling);
    let ours = arugula_core::version::get();
    let state_dir = control.state_dir().to_path_buf();
    let went = tokio::time::timeout(BUDGET, update(&url, ours, &state_dir)).await;
    let (to, why) = match went {
        Ok(Ok(None)) => {
            *NOTE.lock().unwrap() = None;
            return drop(hold);
        }
        Ok(Ok(Some(exe))) => {
            let Err(e) = start_again(&exe);
            (version_of(&exe), format!("{e:#}"))
        }
        Ok(Err(Unknown(e))) => {
            // Control out of reach, or one that doesn't say: nothing to
            // tell a session.
            info!(error = %format!("{e:#}"), "hosted: can't tell control's daemon release; keeping this one");
            return drop(hold);
        }
        Ok(Err(Behind(to, e))) => (Some(to), format!("{e:#}")),
        Err(_) => (None, format!("updating took longer than {} s", BUDGET.as_secs())),
    };
    warn!(error = %why, "hosted: this box's daemon is older than control's and couldn't update itself");
    *NOTE.lock().unwrap() = Some(behind(ours, to.as_deref(), &why));
    drop(hold);
}

/// What a session is told: which tools it may lack, why, and what to do.
pub fn behind(ours: &str, controls: Option<&str>, why: &str) -> String {
    let controls = controls.map_or_else(|| "a newer release".to_owned(), |c| format!("Arugula {c}"));
    format!(
        "this box's daemon is Arugula {ours} and control runs {controls}, so tools added since aren't offered here; \
         it couldn't update itself ({why}). It tries again each time the box wakes; to try now, run \
         `sudo systemctl restart arugulad` in a terminal on the box, then start the session again"
    )
}

/// Why [`update`] didn't.
enum Failed {
    /// Before it knew control's release.
    Unknown(anyhow::Error),
    /// Behind control's release (the one named) and couldn't move to it.
    Behind(String, anyhow::Error),
}
use Failed::{Behind, Unknown};

/// The new daemon, in place of this one's, when control's release is
/// newer; None when this one is current.
async fn update(url: &str, ours: &str, state_dir: &Path) -> Result<Option<PathBuf>, Failed> {
    let controls = controls_release(url).await.map_err(Unknown)?;
    let Some(to) = to_move_to(ours, controls.as_deref()) else { return Ok(None) };
    info!(from = ours, to = %to, "hosted: control runs a newer daemon; updating this box to it");
    install(&to, state_dir).await.map(Some).map_err(|e| Behind(to, e))
}

async fn install(to: &str, state_dir: &Path) -> anyhow::Result<PathBuf> {
    let here = std::env::current_exe().context("finding this daemon")?;
    let dir = state_dir.join("update");
    let _ = std::fs::remove_dir_all(&dir);
    let new = crate::selfupdate::fetch(&crate::selfupdate::releases(&crate::update::url()), to, &dir).await?;
    let got = version_of(&new);
    ensure!(got.as_deref() == Some(to), "the release's arugulad says it's {got:?}, not {to}");
    // The CLI beside it, where there's one beside this daemon.
    let cli = new.with_file_name("arugula");
    if let Some(bin) = here.parent()
        && cli.is_file()
        && bin.join("arugula").is_file()
    {
        put(&cli, &bin.join("arugula"))?;
    }
    put(&new, &here)?;
    let _ = std::fs::remove_dir_all(&dir);
    info!(to, at = %here.display(), "hosted: installed the newer daemon");
    Ok(here)
}

fn version_of(exe: &Path) -> Option<String> {
    crate::selfupdate::version_of(exe)
}

/// Put `from` at `to`, which may be running: beside it, then renamed over
/// it. Where this user can't write (the template's /usr/local/bin is
/// root's), through the template's sudo rule for `user`.
pub fn put(from: &Path, to: &Path) -> anyhow::Result<()> {
    let name = to.file_name().context("no file name")?.to_string_lossy().into_owned();
    let beside = to.with_file_name(format!(".{name}.new"));
    let direct = (|| {
        std::fs::copy(from, &beside)?;
        set_executable(&beside)?;
        std::fs::rename(&beside, to)
    })();
    match direct {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            let _ = std::fs::remove_file(&beside);
        }
        Err(e) => {
            let _ = std::fs::remove_file(&beside);
            return Err(e).with_context(|| format!("putting {}", to.display()));
        }
    }
    let sudo = |args: &[&std::ffi::OsStr]| -> anyhow::Result<()> {
        let out = Command::new("sudo").arg("-n").args(args).output().context("running sudo")?;
        if !out.status.success() {
            bail!("sudo can't put {} ({})", to.display(), String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(())
    };
    sudo(&["install".as_ref(), "-m".as_ref(), "0755".as_ref(), from.as_os_str(), beside.as_os_str()])?;
    sudo(&["mv".as_ref(), "-f".as_ref(), beside.as_os_str(), to.as_os_str()])
}

#[cfg(unix)]
fn set_executable(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn set_executable(_: &Path) -> std::io::Result<()> {
    Ok(())
}

/// The new daemon, in this one's place: same arguments, same process (its
/// supervisor sees no exit), panes kept by their shims.
#[cfg(unix)]
fn start_again(exe: &Path) -> anyhow::Result<std::convert::Infallible> {
    use std::os::unix::process::CommandExt;
    info!(exe = %exe.display(), "hosted: starting the new daemon in this one's place");
    let err = Command::new(exe).args(std::env::args_os().skip(1)).exec();
    Err(anyhow::anyhow!("installed, but can't start it: {err}"))
}

#[cfg(not(unix))]
fn start_again(_: &Path) -> anyhow::Result<std::convert::Infallible> {
    bail!("--hosted runs on Linux")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_controls_daemon_release() {
        let j = serde_json::json!({ "control": true, "build": { "release": "server-412", "daemon": "v0.70.3" } });
        assert_eq!(release_in(&j).as_deref(), Some("0.70.3"));
        // A control from before #806, or a dev build's nonsense.
        assert_eq!(release_in(&serde_json::json!({ "control": true })), None);
        assert_eq!(release_in(&serde_json::json!({ "build": { "daemon": "dev" } })), None);
    }

    #[test]
    fn moves_only_forward() {
        assert_eq!(to_move_to("0.66.1", Some("0.70.3")).as_deref(), Some("0.70.3"));
        assert_eq!(to_move_to("0.70.3", Some("0.70.3")), None);
        // A box newer than control's pin (a release ahead of the hub's
        // deploy) keeps its own.
        assert_eq!(to_move_to("0.70.4", Some("0.70.3")), None);
        assert_eq!(to_move_to("0.70.3", None), None);
    }

    #[test]
    fn the_note_says_what_and_how() {
        let n = behind("0.66.1", Some("0.70.3"), "downloading …: 404");
        assert!(n.contains("Arugula 0.66.1") && n.contains("Arugula 0.70.3") && n.contains("404"), "{n}");
        assert!(n.contains("sudo systemctl restart arugulad"), "{n}");
    }

    #[test]
    fn puts_over_a_file_in_place() {
        let d = super::super::scratch("follow-put");
        let (from, to) = (d.join("new"), d.join("arugulad"));
        std::fs::write(&from, b"new").unwrap();
        std::fs::write(&to, b"old").unwrap();
        put(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"new");
        assert!(!d.join(".arugulad.new").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&to).unwrap().permissions().mode() & 0o777, 0o755);
        }
    }
}
