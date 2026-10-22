//! A deploy's run on a hosted machine (M219, platform step 7, native
//! Arugula #15; aru-102 "A driver is a run with a credential"). The
//! platform's half is `arugula_platform::deploys::apply`.
//!
//! - **Checked here, against what this machine pinned** ([`check`]): the
//!   run's operation is the apply of one deploy (`kind deploy`); the
//!   approval it carries is a project owner's, signed by one of their
//!   browsers over exactly that deploy's operation, not past its
//!   `expires` by this machine's clock, confirmed with a passkey over its
//!   bytes (`arugula_e2e::deploy::check_apply`, with the team's roster and
//!   members' certificates, or the account's chain, as this machine keeps
//!   them). A run whose approval doesn't verify here fails, and nothing
//!   runs.
//! - **Started only once control takes its start** ([`apply`]): its
//!   `running` report is sent and answered before anything runs, because
//!   that's when the platform starts the deploy (and refuses a stale one,
//!   or one whose approval expired).
//! - **Secrets for that run only, then gone.** Asked of control once
//!   (`/api/hosted/work/secrets`), held in this daemon's memory, and handed
//!   to the driver's process on an anonymous pipe (its standard input),
//!   which its shell reads into its own environment before it runs the
//!   driver. Never in a file (not even tmpfs), never on a command line,
//!   never in the ledger or the log; overwritten in memory when the run
//!   ends.
//! - **Never in a pane** ([`Redactor`]). The driver doesn't run in a pane:
//!   it runs beside one, its output piped through this daemon, which
//!   replaces each secret's exact bytes with `[redacted NAME]` before
//!   anything reaches the pane (its scrollback, its log on disk, and what's
//!   sent of it, M213). The pane shows the redacted output through a FIFO
//!   in this daemon's state directory (a FIFO holds nothing on disk).
//! - **As the run user**, as every run is (`runner`, M212): an empty
//!   environment, a fresh `~runner/work/ID`, and everything it left killed
//!   when it ends.

use std::{path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use arugula_control_wire::work::{DeployWork, Secret, SecretsAsk, WORK_SECRETS, WorkItem};
use arugula_e2e::{
    deploy::{PasskeyParts, check_apply},
    now_ms,
    run::{Op, State},
};
#[cfg(unix)]
use arugula_proto::{Policy, StartedBy, api::RunRequest};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tracing::{info, warn};

use super::{
    DIR, Ledger, RUN_PATH, RUNNING, Refused, RunAs, STOPPING, kill_all, me, printable, quote, read, send, update,
};
#[cfg(unix)]
use super::{SESSION, tab_name};
#[cfg(unix)]
use crate::mux::Api;
use crate::server::App;

/// The fake driver's apply (`ARUGULA_HOSTED_FAKE_DRIVER=1`, tests and
/// testnet only): says what it deploys, prints each secret it was given
/// (which the redaction must catch) and its length, leaves a marker in
/// its own directory with the source's digest (never a secret), and fails
/// part way when the source's reference starts with `fail`.
pub const FAKE: &str = r#"echo "fake driver: deploying $DEPLOY_SOURCE_KIND $DEPLOY_SOURCE_REF from $DEPLOY_SOURCE_WHERE to $DEPLOY_ENVIRONMENT"
for n in $DEPLOY_SECRETS; do eval "v=\${$n}"; echo "fake driver: $n is $v (${#v} bytes)"; done
printf 'applied %s\n' "$DEPLOY_SOURCE_REF" > "$HOME/applied" && echo "fake driver: wrote $HOME/applied"
case "$DEPLOY_SOURCE_REF" in fail*) echo "fake driver: failing part way" >&2; exit 3;; esac
echo "fake driver: done""#;

/// The chant→Fly driver's apply (M220, `work_deploy_chant_fly.mjs`): run
/// with `node`, as the plan the owner approved says
/// (`arugula_e2e::drivers::plan_text`). It says what it did on fd 3
/// ([`RESULT_FD`]).
pub const CHANT_FLY: &str = include_str!("work_deploy_chant_fly.mjs");

/// The fd a driver says what its apply did on (M220): a pipe only it is
/// given (not its standard output, which what it runs shares), whose JSON
/// object is reported with the run's end, and which the platform checks and
/// keeps (chant's ledger entry, Fly's image and health).
#[cfg(unix)]
pub const RESULT_FD: i32 = 3;

/// The most a driver's result holds, in bytes.
const MAX_RESULT: usize = 2048;

/// Whether this machine is a deploy's box (M219): control enrolled it for
/// one deploy's run. Nobody gets in, it takes that run only, and it stops
/// once the run's end is reported.
pub fn deploy_box(app: &App) -> bool {
    app.control.enrolled().is_some_and(|e| e.saved.hosted.as_ref().is_some_and(|h| h.deploy))
}

/// The sudo rule a deploy's box keeps for this daemon's user: to the run
/// user only.
#[cfg(unix)]
const RUNNER_RULE: &str = "/etc/sudoers.d/arugulad-runner";

/// On a deploy's box, before its run: this daemon's user gives up root
/// (`/etc/sudoers.d/user`, the template's blanket rule, goes), keeping only
/// `sudo -u <run user>` (and the template's `arugulad _reseed` rule). Then
/// it checks: root must be refused, the run user allowed. Once a daemon; if
/// it can't, nothing runs.
#[cfg(not(unix))]
pub fn harden_sudo(_: &RunAs) -> Result<(), Refused> {
    // Hosted boxes are Linux; elsewhere this only builds.
    Err(Refused("sudo", "a deploy's box is Linux: nothing runs here".into()))
}

/// [`harden_sudo`]'s Unix half: the only one a box runs.
#[cfg(unix)]
pub fn harden_sudo(as_: &RunAs) -> Result<(), Refused> {
    static DONE: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    let mut done = DONE.lock().unwrap_or_else(|e| e.into_inner());
    if *done {
        return Ok(());
    }
    let refuse = |why: &str| Refused("sudo", format!("this deploy box can't give up root ({why}): nothing runs"));
    let me = nix::unistd::User::from_uid(nix::unistd::getuid())
        .ok()
        .flatten()
        .map(|u| u.name)
        .ok_or_else(|| refuse("no user name"))?;
    // `closefrom_override`: the driver's result pipe (fd 3, M220) reaches
    // it through `sudo -C 4`.
    let rule = format!("{me} ALL=({}) NOPASSWD: ALL\nDefaults:{me} closefrom_override\n", as_.user);
    // The narrow rule in, the blanket one out; the user's password locked
    // and its admin groups gone, so nothing gives it root back.
    let script = format!(
        "set -e; umask 0337; printf '%s' \"$1\" > {RUNNER_RULE}.new; visudo -cqf {RUNNER_RULE}.new; \
         mv -f {RUNNER_RULE}.new {RUNNER_RULE}; rm -f /etc/sudoers.d/user; passwd -l \"$2\" >/dev/null; \
         for g in sudo admin wheel; do gpasswd -d \"$2\" \"$g\" >/dev/null 2>&1 || true; done"
    );
    let ran = std::process::Command::new("sudo")
        .args(["-n", "sh", "-c", &script, "sh", &rule, &me])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !ran.is_ok_and(|s| s.success()) {
        return Err(refuse("the sudo rules didn't change"));
    }
    let sudo = |args: &[&str]| {
        std::process::Command::new("sudo")
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    if sudo(&["-n", "true"]) {
        return Err(refuse("sudo still gives root"));
    }
    if !sudo(&["-n", "-u", &as_.user, "true"]) {
        return Err(refuse("sudo doesn't reach the run user"));
    }
    // What sudo says this user may still run: the run user, and the
    // template's reseed, nothing else.
    let listed = std::process::Command::new("sudo")
        .args(["-ln"])
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    match listed.as_deref().map(|l| only_runner_and_reseed(l, &as_.user)) {
        Some(Ok(())) => {}
        Some(Err(line)) => return Err(refuse(&format!("sudo still allows: {}", printable(&line)))),
        None => return Err(refuse("sudo -ln didn't say")),
    }
    info!(user = me, run_user = as_.user, "hosted: a deploy's box gave up root");
    *done = true;
    Ok(())
}

/// What `sudo -ln` lists, read: each rule this user may run is the run
/// user's (`(runner) NOPASSWD: ALL`) or the template's reseed
/// (`arugulad _reseed`); else the first other rule.
#[cfg_attr(not(unix), allow(dead_code))]
pub fn only_runner_and_reseed(listed: &str, run_user: &str) -> Result<(), String> {
    let Some((_, rules)) = listed.split_once("may run the following commands") else {
        return Err("no rules listed".into());
    };
    let runner = format!("({run_user}) NOPASSWD: ALL");
    for line in rules.lines().skip(1).map(str::trim).filter(|l| !l.is_empty()) {
        if line == runner || (line.starts_with("(root) NOPASSWD:") && line.contains("arugulad _reseed")) {
            continue;
        }
        return Err(line.to_owned());
    }
    Ok(())
}

/// Tests only (`ARUGULA_HOSTED_TESTS_UNSAFE=1`, never on a box): a deploy's
/// box with no run user applies its deploy as this daemon's own user.
pub fn tests_unsafe() -> bool {
    std::env::var("ARUGULA_HOSTED_TESTS_UNSAFE").is_ok_and(|v| v == "1")
}

/// A deploy's box applies only as a run user, after giving up root: one
/// with no run user is refused, but in tests ([`tests_unsafe`]).
pub fn deploy_box_user(as_: Option<&RunAs>, unsafe_for_tests: bool) -> Result<(), Refused> {
    match as_ {
        Some(_) => Ok(()),
        None if unsafe_for_tests => Ok(()),
        None => Err(Refused("run_user", "a deploy's box applies only as its run user, and this one has none".into())),
    }
}

/// Whether this box runs the fake driver.
fn fake_on() -> bool {
    std::env::var("ARUGULA_HOSTED_FAKE_DRIVER").is_ok_and(|v| v == "1")
}

/// What applies `driver` on this box: its command line. Only drivers this
/// daemon knows run; the platform names one, never a command.
pub fn driver_argv(driver: &str) -> Result<Vec<String>, Refused> {
    match driver {
        "fake" if fake_on() => Ok(vec!["/bin/sh".into(), "-c".into(), FAKE.into()]),
        arugula_e2e::drivers::CHANT_FLY => {
            Ok(vec!["node".into(), "--input-type=module".into(), "-e".into(), CHANT_FLY.into()])
        }
        "fake" => Err(Refused("driver", "this box doesn't run the fake driver".into())),
        arugula_e2e::deploy::apps_platform::DRIVER => {
            let exe = std::env::current_exe()
                .map_err(|e| Refused("driver", format!("this box can't find its own daemon: {e}")))?;
            let mut argv = vec![exe.to_string_lossy().into_owned(), "_apps-platform".into()];
            argv.extend(apps_platform_test_flags());
            Ok(argv)
        }
        arugula_e2e::drivers::arugula_release::DRIVER => {
            let exe = std::env::current_exe()
                .map_err(|e| Refused("driver", format!("this box can't find its own daemon: {e}")))?;
            let mut argv = vec![exe.to_string_lossy().into_owned(), "_arugula-release".into()];
            argv.extend(arugula_release_test_flags());
            Ok(argv)
        }
        other => Err(Refused("driver", format!("this box has no {} driver yet", printable(other)))),
    }
}

/// What applies deploy `d` on this box: [`driver_argv`], and for a driver
/// that reads them, the operation this machine checked, on its command
/// line (which the driver's shell builds from this daemon's words, and
/// nothing it reads can change), not only in its environment.
pub fn driver_argv_for(d: &DeployWork) -> Result<Vec<String>, Refused> {
    let mut argv = driver_argv(&d.driver)?;
    let release = d.driver == arugula_e2e::drivers::arugula_release::DRIVER;
    if d.driver == arugula_e2e::deploy::apps_platform::DRIVER || release {
        let o = &d.operation;
        for (flag, v) in [
            ("--deploy", &d.deploy),
            ("--environment", &o.environment_name),
            ("--kind", &o.source_kind),
            ("--ref", &o.source_ref),
            ("--where", &o.source_where),
            ("--plan", &o.plan),
        ] {
            argv.extend([flag.to_owned(), v.clone()]);
        }
        // A roll back (M220's): the plan names the deploy it goes back to.
        // Arugula's release never rolls back, and refuses the flag.
        if let Some(r) = &d.roll_back {
            argv.extend(["--roll-back".to_owned(), r.deploy.clone()]);
        }
    }
    // Its settings (M221), which its plan names: the bump and the lane.
    if release {
        for (k, flag) in [("bump", "--bump"), ("lane", "--lane")] {
            if let Some(v) = d.settings.get(k) {
                argv.extend([flag.to_owned(), v.clone()]);
            }
        }
    }
    Ok(argv)
}

/// Where the arugula-release driver clones, pushes and asks, on a box that
/// runs the fake driver too (tests and testnet only): a directory of bare
/// repositories for GitHub (`ARUGULA_HOSTED_ARUGULA_RELEASE_GIT`) and a
/// fake GitHub API (`ARUGULA_HOSTED_ARUGULA_RELEASE_API`), asked often.
fn arugula_release_test_flags() -> Vec<String> {
    if !fake_on() {
        return vec![];
    }
    let mut flags = vec![];
    for (var, flag) in
        [("ARUGULA_HOSTED_ARUGULA_RELEASE_GIT", "--git-base"), ("ARUGULA_HOSTED_ARUGULA_RELEASE_API", "--api-base")]
    {
        if let Some(v) = std::env::var(var).ok().filter(|v| !v.is_empty()) {
            flags.extend([flag.to_owned(), v]);
        }
    }
    if !flags.is_empty() {
        flags.extend(["--poll-ms".into(), "200".into(), "--follow-ms".into(), "30000".into()]);
    }
    flags
}

/// Where the apps-platform driver pushes and asks, on a box that runs the
/// fake driver too (tests and testnet only): a directory of bare
/// repositories for GitHub (`ARUGULA_HOSTED_APPS_PLATFORM_GIT`), a fake
/// app (`ARUGULA_HOSTED_APPS_PLATFORM_HEALTH`), a fake GitHub API
/// (`ARUGULA_HOSTED_APPS_PLATFORM_API`), and no wait for Flux.
fn apps_platform_test_flags() -> Vec<String> {
    if !fake_on() {
        return vec![];
    }
    let mut flags = vec![];
    for (var, flag) in [
        ("ARUGULA_HOSTED_APPS_PLATFORM_GIT", "--git-base"),
        ("ARUGULA_HOSTED_APPS_PLATFORM_HEALTH", "--health-base"),
        ("ARUGULA_HOSTED_APPS_PLATFORM_API", "--api-base"),
    ] {
        if let Some(v) = std::env::var(var).ok().filter(|v| !v.is_empty()) {
            flags.extend([flag.to_owned(), v]);
        }
    }
    if !flags.is_empty() {
        flags.extend(["--settle-ms".into(), "0".into(), "--poll-ms".into(), "200".into()]);
    }
    flags
}

/// A secret's name: an environment variable's, and not one the driver's
/// shell or its own environment uses (`arugula_e2e::deploy::secret_name_ok`).
fn good_name(n: &str) -> bool {
    arugula_e2e::deploy::secret_name_ok(n)
}

/// One word: printable, no space.
fn word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 200 && s.bytes().all(|b| b.is_ascii_graphic())
}

/// The checks a deploy's run passes before it starts, instead of the
/// standing grant's: what this machine can check of the approval itself.
pub fn check(app: &App, item: &WorkItem, d: &DeployWork) -> Result<(), Refused> {
    // Only a deploy's own box applies one: never a people's workspace.
    if !deploy_box(app) {
        return Err(Refused("not_deploy_box", "only a box made for a deploy applies one".into()));
    }
    if item.state != State::Approved.as_str() {
        return Err(Refused("approval_state", "a deploy's run starts only once it's approved".into()));
    }
    let Some(e) = app.control.enrolled() else {
        return Err(Refused("not_hosted", "this machine isn't in control".into()));
    };
    let Some(pin) = e.saved.hosted.as_ref() else {
        return Err(Refused("not_hosted", "this machine isn't a hosted machine".into()));
    };
    let o = &d.operation;
    if o.project != pin.cert.project || item.project != pin.cert.project {
        return Err(Refused("approval_operation", "the deploy is another project's".into()));
    }
    let fields = [&o.environment, &o.environment_name, &o.plan, &o.source_kind, &o.source_ref, &o.source_where];
    if !fields.iter().all(|f| word(f)) {
        return Err(Refused("op_form", "the deploy's fields aren't ones this machine reads".into()));
    }
    // Only the secrets its driver takes (M220's review): a name it doesn't
    // read could only change how it, or a tool it runs, behaves.
    if let Some(bad) = d.secrets.iter().find(|n| !arugula_e2e::deploy::driver_takes(&d.driver, n)) {
        return Err(Refused(
            "secret_name",
            format!("the {} driver doesn't take a secret called {}", d.driver, printable(bad)),
        ));
    }
    let Some(owners) = crate::hosted::runs::owners(&e.saved) else {
        return Err(Refused("approval_not_an_owner", "this machine pinned no owners for its project".into()));
    };
    let pk = d
        .passkey
        .as_ref()
        .map(|p| PasskeyParts { client_data_json: &p.client_data_json, authenticator_data: &p.authenticator_data });
    check_apply(&d.approval, o, &d.deploy, &owners, pk, now_ms()).map_err(|r| Refused(r.code(), r.says().into()))?;
    // The plan the owner approved names the driver, its settings and the
    // roll back (M220): the platform can't swap in another driver, another
    // Fly app, or another image to go back to.
    let settings_ok = arugula_e2e::drivers::settings(&d.driver, &d.settings).is_ok_and(|s| s == d.settings);
    let roll_back_ok =
        d.roll_back.as_ref().is_none_or(|r| arugula_e2e::drivers::roll_back_ok(&d.driver, &d.settings, r));
    let source_ok = arugula_e2e::drivers::source_ok(&d.driver, &o.source_kind, &o.source_ref);
    if !settings_ok || !roll_back_ok || !source_ok {
        return Err(Refused("op_form", "the deploy's settings aren't ones its driver takes".into()));
    }
    if !arugula_e2e::drivers::planned(&d.driver, o, &d.settings, d.roll_back.as_ref()) {
        return Err(Refused("plan_driver", "the approved plan isn't this driver's plan for this deploy".into()));
    }
    // Each deploy applies on this box once, and a box applies one deploy.
    // A box is new for each deploy, so this alone doesn't stop a replay on
    // another box: the platform consumes the approval when its deploy
    // starts, and the drivers' targets refuse a deploy id they applied.
    if applied(app.control.state_dir(), &d.deploy) {
        return Err(Refused("deploy_applied", "this deploy was applied on this machine already".into()));
    }
    // A deploy's box applies one deploy, ever.
    if deploy_box(app) && any_applied(app.control.state_dir()) {
        return Err(Refused("deploy_box", "this box was made for one deploy, and applied it already".into()));
    }
    driver_argv(&d.driver)?;
    Ok(())
}

/// A deploy's id as a file name: `dep_` and 32 hex characters.
fn good_deploy_id(d: &str) -> bool {
    d.strip_prefix("dep_").is_some_and(|h| h.len() == 32 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// Where the box keeps that it applied `deploy`: `<state>/work/deploys/ID`.
fn applied_path(dir: &std::path::Path, deploy: &str) -> Option<PathBuf> {
    good_deploy_id(deploy).then(|| dir.join(DIR).join("deploys").join(deploy))
}

/// Whether `deploy` was applied here (or its mark can't be read: taken).
pub fn applied(dir: &std::path::Path, deploy: &str) -> bool {
    match applied_path(dir, deploy) {
        Some(p) => !matches!(std::fs::symlink_metadata(&p), Err(e) if e.kind() == std::io::ErrorKind::NotFound),
        None => true,
    }
}

/// Whether any deploy was applied here (or its marks can't be read).
fn any_applied(dir: &std::path::Path) -> bool {
    match std::fs::read_dir(dir.join(DIR).join("deploys")) {
        Ok(mut d) => d.next().is_some(),
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    }
}

/// Mark `deploy` applied here, once (`create_new`, fsynced): `true` only
/// for the call that made the mark, before anything of it runs.
pub fn mark_applied(dir: &std::path::Path, deploy: &str, run: &str) -> bool {
    let Some(p) = applied_path(dir, deploy) else { return false };
    let made = (|| -> std::io::Result<()> {
        crate::store::private_dir(&dir.join(DIR).join("deploys"))?;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&p)?;
        std::io::Write::write_all(&mut f, run.as_bytes())?;
        f.sync_all()?;
        std::fs::File::open(dir.join(DIR).join("deploys"))?.sync_all()
    })();
    made.is_ok()
}

/// Whether the run's operation is the apply of exactly the deploy it
/// carries (or, for a run that carries none, not an apply at all).
pub fn matches(op: &Op, d: Option<&DeployWork>) -> Result<(), Refused> {
    let ok = match (&op.deploy, d) {
        (None, None) => true,
        (Some(a), Some(d)) => a.deploy == d.deploy && a.driver == d.driver && a.op == d.approval.op,
        _ => false,
    };
    if ok { Ok(()) } else { Err(Refused("op_deploy", "the operation isn't the apply of the deploy it carries".into())) }
}

/// Replaces each secret's exact bytes in a stream with `[redacted NAME]`,
/// across chunk boundaries: what could still become a secret is held back
/// until the next chunk says, or the stream ends.
pub struct Redactor {
    /// Each secret's bytes and what replaces them, longest first.
    secrets: Vec<(Vec<u8>, Vec<u8>)>,
    held: Vec<u8>,
}

impl Redactor {
    pub fn new(secrets: &[Secret]) -> Self {
        let mut s: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for x in secrets {
            let with = format!("[redacted {}]", x.name).into_bytes();
            // Its exact bytes, and trimmed of whitespace at either end (a
            // driver that prints it as a line, or a value pasted with a
            // newline).
            for v in [x.value.as_str(), x.value.trim()] {
                if !v.is_empty() && !s.iter().any(|(have, _)| have == v.as_bytes()) {
                    s.push((v.as_bytes().to_vec(), with.clone()));
                }
            }
        }
        s.sort_by_key(|(v, _)| std::cmp::Reverse(v.len()));
        Redactor { secrets: s, held: Vec::new() }
    }

    /// What of `chunk` (after what was held) may be shown now.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.held.extend_from_slice(chunk);
        let buf = std::mem::take(&mut self.held);
        let mut out = Vec::with_capacity(buf.len());
        let mut i = 0;
        'scan: while i < buf.len() {
            let rest = &buf[i..];
            for (v, with) in &self.secrets {
                if rest.starts_with(v) {
                    out.extend_from_slice(with);
                    i += v.len();
                    continue 'scan;
                }
            }
            // The start of a secret, cut off by the chunk: wait for more.
            if self.secrets.iter().any(|(v, _)| rest.len() < v.len() && v.starts_with(rest)) {
                self.held = rest.to_vec();
                break;
            }
            out.push(buf[i]);
            i += 1;
        }
        wipe_bytes(buf);
        out
    }

    /// The stream ended. What was held is the start of a secret, cut off:
    /// redacted too, never shown.
    pub fn finish(&mut self) -> Vec<u8> {
        let held = std::mem::take(&mut self.held);
        let out = self
            .secrets
            .iter()
            .find(|(v, _)| v.starts_with(&held))
            .map(|(_, with)| with.clone())
            .filter(|_| !held.is_empty())
            .unwrap_or_default();
        wipe_bytes(held);
        out
    }
}

impl Drop for Redactor {
    fn drop(&mut self) {
        for (v, _) in self.secrets.drain(..) {
            wipe_bytes(v);
        }
        wipe_bytes(std::mem::take(&mut self.held));
    }
}

/// Overwrite bytes before they're freed: a secret isn't left in freed
/// memory for the next allocation to show.
fn wipe_bytes(mut b: Vec<u8>) {
    b.iter_mut().for_each(|x| *x = 0);
    std::hint::black_box(&b);
}

fn wipe(secrets: Vec<Secret>) {
    for s in secrets {
        wipe_bytes(s.value.into_bytes());
    }
}

/// The driver's shell, as the run user: a fresh directory of its own, its
/// secrets read from standard input into its own environment, then the
/// driver, with nothing more on its standard input. Each secret comes as
/// one line of its name and its bytes escaped, unescaped by `printf -v` in
/// the C locale, so a value keeps every byte, trailing newlines too, with
/// no subshell and nothing on a command line. An empty line ends them.
pub fn driver_script(work: &std::path::Path, argv: &[String]) -> String {
    // Each secret is one line, `NAME \xHH\xHH…`: its bytes escaped, so no
    // byte of a value (a newline, a NUL) can end it early or start another
    // header. `printf -v` unescapes it with no subshell (trailing newlines
    // kept); a NUL, which a shell variable can't hold, comes short of the
    // length and runs nothing. A name the deploy doesn't name
    // ($DEPLOY_SECRETS) runs nothing either.
    format!(
        r#"set -e; w={}; rm -rf -- "$w"; mkdir -p -m 700 -- "$w"; cd -- "$w"; export LC_ALL=C; while IFS=' ' read -r n e && [ -n "$n" ]; do case " $DEPLOY_SECRETS " in *" $n "*) ;; *) echo 'arugula: a secret the deploy does not name' >&2; exit 97;; esac; case "$e" in *[!\\x0-9a-f]*) exit 97;; esac; printf -v v '%b' "$e"; [ "${{#v}}" = "$(( ${{#e}} / 4 ))" ] || {{ echo 'arugula: a secret came short' >&2; exit 97; }}; export "$n=$v"; done; unset n e v LC_ALL; set +e; exec {} < /dev/null"#,
        quote(&work.to_string_lossy()),
        super::command_line(argv)
    )
}

/// What the driver's shell reads on its standard input: each secret as a
/// line `NAME \xHH…` (every byte escaped); then an empty line.
pub fn secrets_input(secrets: &[Secret]) -> Vec<u8> {
    use std::fmt::Write as _;
    let mut out = String::new();
    for s in secrets {
        out.push_str(&s.name);
        out.push(' ');
        for b in s.value.as_bytes() {
            let _ = write!(out, "\\x{b:02x}");
        }
        out.push('\n');
    }
    out.push('\n');
    out.into_bytes()
}

/// Where a deploy's run works, its `HOME`: `~runner/work/ID`, or (tests,
/// with no run user) beside the ledger.
fn work_dir(run: &str, as_: Option<&RunAs>, dir: &std::path::Path) -> PathBuf {
    match as_ {
        Some(a) => a.work(run),
        None => dir.join(DIR).join(format!("{run}.home")),
    }
}

/// Remove a run's directory as the run user (who made it, after
/// `kill_all`), so nothing it wrote outlives the run.
async fn remove_home(as_: Option<&RunAs>, work: &std::path::Path) {
    let gone = match as_ {
        Some(a) => tokio::process::Command::new("sudo")
            .args(["-n", "-u", &a.user, "--", "rm", "-rf", "--"])
            .arg(work)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .is_ok_and(|s| s.success()),
        None => std::fs::remove_dir_all(work).is_ok() || !work.exists(),
    };
    if !gone {
        warn!(dir = %work.display(), "hosted: a deploy run's directory wasn't removed");
    }
}

/// The driver's process: as `as_` (or, in tests, this daemon's user),
/// with an empty environment but its own and the deploy's (never a
/// secret's: those come on its standard input).
fn driver_command(
    run: &str,
    d: &DeployWork,
    argv: &[String],
    as_: Option<&RunAs>,
    dir: &std::path::Path,
    result: &ResultPipe,
) -> tokio::process::Command {
    let work = work_dir(run, as_, dir);
    let user = as_.map_or_else(|| "arugula".to_owned(), |a| a.user.clone());
    let o = &d.operation;
    let names: Vec<&str> = d.secrets.iter().map(String::as_str).collect();
    let env: Vec<(String, String)> = [
        ("HOME", work.to_string_lossy().into_owned()),
        ("USER", user.clone()),
        ("LOGNAME", user),
        ("SHELL", "/bin/bash".into()),
        ("PATH", RUN_PATH.into()),
        ("TERM", "dumb".into()),
        ("LANG", "C.UTF-8".into()),
        ("DEPLOY_ID", d.deploy.clone()),
        ("DEPLOY_ENVIRONMENT", o.environment_name.clone()),
        ("DEPLOY_SOURCE_KIND", o.source_kind.clone()),
        ("DEPLOY_SOURCE_REF", o.source_ref.clone()),
        ("DEPLOY_SOURCE_WHERE", o.source_where.clone()),
        ("DEPLOY_PLAN", o.plan.clone()),
        ("DEPLOY_SECRETS", names.join(" ")),
        // M220: who approved it (chant's approve gate records them), the
        // settings and the roll back the plan names.
        ("DEPLOY_RUN", run.to_owned()),
        ("DEPLOY_APPROVER", d.approval.account.clone()),
        ("DEPLOY_APPROVED_BY", d.approval.by.clone()),
        ("DEPLOY_APPROVAL", d.approval_id.clone().unwrap_or_default()),
        ("DEPLOY_OP_HASH", d.approval.op.clone()),
        ("DEPLOY_SETTINGS", serde_json::to_string(&d.settings).unwrap_or_default()),
        ("DEPLOY_ROLL_BACK", d.roll_back.as_ref().and_then(|r| serde_json::to_string(r).ok()).unwrap_or_default()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    let script = driver_script(&work, argv);
    let mut c = match as_ {
        Some(a) => {
            let mut c = tokio::process::Command::new("sudo");
            // `-C 4`: fd 3, its result pipe, stays open through sudo.
            c.args(["-n", "-C", "4", "-u", &a.user, "--", "env", "-i"]);
            c.args(env.iter().map(|(k, v)| format!("{k}={v}")));
            c.args(["/bin/bash", "--noprofile", "--norc", "-c", &script]);
            c
        }
        None => {
            let mut c = tokio::process::Command::new("/bin/bash");
            c.env_clear().envs(env).args(["--noprofile", "--norc", "-c", &script]);
            c
        }
    };
    c.current_dir("/").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    result.give(&mut c);
    c
}

/// The driver's result pipe (M220): its write end becomes the driver's fd
/// 3, its read end stays here. Unix only: a deploy's box is Linux.
#[cfg(unix)]
pub struct ResultPipe {
    r: std::os::fd::OwnedFd,
    w: std::os::fd::OwnedFd,
}

#[cfg(unix)]
impl ResultPipe {
    pub fn new() -> Result<Self, String> {
        let (r, w) = crate::agent::pipe_cloexec().map_err(|e| format!("its result pipe: {e}"))?;
        Ok(Self { r, w })
    }

    /// Make the write end `c`'s fd 3 (dup2 leaves the copy open across
    /// exec).
    fn give(&self, c: &mut tokio::process::Command) {
        use std::os::fd::AsRawFd as _;
        let w = self.w.as_raw_fd();
        // SAFETY: dup2 is async-signal-safe, and nothing else runs in the
        // child between fork and exec.
        unsafe {
            c.pre_exec(move || {
                if nix::libc::dup2(w, RESULT_FD) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    /// The driver has its own copy: this one goes, so the pipe ends with
    /// the driver.
    fn spawned(self) -> std::os::fd::OwnedFd {
        drop(self.w);
        self.r
    }
}

/// What's in the driver's result pipe once every writer is gone: at most
/// [`MAX_RESULT`] bytes, never waiting.
#[cfg(unix)]
fn read_result(fd: std::os::fd::OwnedFd) -> Vec<u8> {
    use std::io::Read as _;
    let mut f = std::fs::File::from(fd);
    let _ = nix::fcntl::fcntl(&f, nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK));
    let mut out = Vec::new();
    let mut buf = [0u8; 1024];
    while out.len() <= MAX_RESULT {
        match f.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
        }
    }
    out
}

/// Not on Windows: a deploy's box is Linux.
#[cfg(not(unix))]
pub struct ResultPipe;

#[cfg(not(unix))]
impl ResultPipe {
    pub fn new() -> Result<Self, String> {
        Err("a deploy's driver runs on a Linux box only".into())
    }

    fn give(&self, _: &mut tokio::process::Command) {}

    fn spawned(self) -> NoPipe {
        NoPipe
    }
}

#[cfg(not(unix))]
pub struct NoPipe;

#[cfg(not(unix))]
fn read_result(_: NoPipe) -> Vec<u8> {
    Vec::new()
}

/// Send the run's reports until its `running` is answered (at most a
/// minute): `Ok` when control took it, `Err` why not.
async fn started(app: &App, run: &str) -> Result<(), String> {
    let dir = app.control.state_dir().to_owned();
    for _ in 0..60 {
        send(app, run).await;
        if let Ok(Some(l)) = read(&dir, run)
            && let Some(k) = l.events.iter().find(|k| k.report.to == State::Running.as_str())
            && k.answered
        {
            return match &k.refused {
                None => Ok(()),
                Some(why) => Err(format!("control refused its start: {why}")),
            };
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err("control didn't take its start".into())
}

/// A deploy's box, its run ended: once control has its every report (for
/// up to ten minutes), the daemon stops. The platform deletes the box.
async fn stop_when_reported(app: &App, run: &str) {
    let dir = app.control.state_dir().to_owned();
    for _ in 0..120 {
        if read(&dir, run).is_ok_and(|l| l.is_some_and(|l| l.events.iter().all(|k| k.answered))) {
            info!(run, "hosted: a deploy's box did its one run: stopping");
            std::process::exit(0);
        }
        send(app, run).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    warn!(run, "hosted: a deploy's box couldn't report its run's end; staying up for the platform to delete");
}

/// The most of control's answer with a run's secrets that's read.
const MAX_SECRETS_ANSWER: usize = 1024 * 1024;

/// Ask control for the run's secrets, once: only the names the deploy
/// carries, each once, none reserved. Read by hand from the answer's bytes,
/// so no parse error can quote a value, and the bytes are overwritten
/// after.
async fn secrets(app: &App, run: &str, names: &[String]) -> Result<Vec<Secret>, String> {
    let ask = SecretsAsk { run: run.into() };
    let bytes = match app.control.post_signed_bytes(WORK_SECRETS, &ask, MAX_SECRETS_ANSWER).await {
        Ok(b) => b,
        Err(e) => return Err(format!("its secrets: {e:#}")),
    };
    let read = read_given(&bytes, names);
    wipe_bytes(bytes);
    read
}

/// `SecretsGiven` from its bytes, by hand: a refusal's reason, or the
/// secrets, each a name `names` lists, once. What's wrong is said without
/// any value.
pub fn read_given(bytes: &[u8], names: &[String]) -> Result<Vec<Secret>, String> {
    let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return Err("its secrets: control's answer doesn't read".into());
    };
    if v["kind"] == "refused" {
        return Err(format!("its secrets: {}", printable(v["why"].as_str().unwrap_or("refused"))));
    }
    let mut out: Vec<Secret> = Vec::new();
    let mut bad = None;
    if v["kind"] != "given" {
        bad = Some("control's answer doesn't read".to_owned());
    }
    if let Some(list) = v.get_mut("secrets").and_then(|l| l.as_array_mut()) {
        for item in list.iter_mut() {
            let name = item["name"].as_str().unwrap_or_default().to_owned();
            let value = match item.get_mut("value").map(std::mem::take) {
                Some(serde_json::Value::String(s)) => s,
                _ => String::new(),
            };
            let named = names.contains(&name) && good_name(&name);
            if !named || out.iter().any(|s| s.name == name) {
                bad.get_or_insert_with(|| {
                    format!("control gave a secret the deploy doesn't name: {}", printable(&name))
                });
            }
            // A NUL can't reach the driver's environment, and would throw
            // its framing out: refused.
            if value.contains('\0') {
                bad.get_or_insert_with(|| format!("control gave {} with a NUL byte in it", printable(&name)));
            }
            // A short-lived token's revoke URL (https, or a test's loopback).
            let revoke = item
                .get("revoke")
                .and_then(|r| r.as_str())
                .filter(|r| r.starts_with("https://") || r.starts_with("http://127.0.0.1:"))
                .map(str::to_owned);
            out.push(Secret { name, value, revoke });
        }
    } else {
        bad.get_or_insert_with(|| "control's answer doesn't read".to_owned());
    }
    match bad {
        Some(why) => {
            wipe(out);
            Err(format!("its secrets: {why}"))
        }
        None => Ok(out),
    }
}

/// The pane a deploy's run shows in: `cat` of a FIFO in this daemon's
/// state directory, which only this daemon writes, redacted. Its id and
/// epoch, and the FIFO open for writing.
#[cfg(not(unix))]
async fn pane(_: &App, _: &Ledger, _: &DeployWork) -> Result<(u32, Option<u64>, PathBuf, tokio::fs::File), String> {
    // Hosted boxes are Linux; elsewhere this only builds.
    Err("a deploy's box is Linux: no pane here".into())
}

/// [`pane`]'s Unix half: the only one a box runs.
#[cfg(unix)]
async fn pane(app: &App, l: &Ledger, d: &DeployWork) -> Result<(u32, Option<u64>, PathBuf, tokio::fs::File), String> {
    let dir = app.control.state_dir().to_owned();
    let fifo = dir.join(DIR).join(format!("{}.out", l.run));
    let _ = std::fs::remove_file(&fifo);
    nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR)
        .map_err(|e| format!("its pane's pipe: {e}"))?;
    // Read and write: opening doesn't wait for the pane's reader, and the
    // pane sees the end only when this daemon closes it.
    let out =
        std::fs::OpenOptions::new().read(true).write(true).open(&fifo).map_err(|e| format!("its pane's pipe: {e}"))?;
    let req = RunRequest {
        command: Some(format!("exec cat -- {}", quote(&fifo.to_string_lossy()))),
        session: Some(SESSION.into()),
        cwd: Some("/".into()),
        policy: Some(Policy::None),
        ..Default::default()
    };
    let pane = match app.mux.api(|r| Api::Run(req, r)).await {
        Some(Ok(p)) => p,
        Some(Err(e)) => return Err(e),
        None => return Err("the daemon is stopping".into()),
    };
    let shown = format!("deploy {} by {}", d.deploy, d.driver);
    let _ = app.mux.api(|r| Api::OwnTab(pane, Some(tab_name(&l.run, &l.op_hash, &shown)), r)).await;
    app.mux.send(crate::mux::Cmd::Api(Api::StartedBy(
        pane,
        StartedBy { by: format!("run {} (operation sha256:{})", l.run, l.op_hash), block: None },
    )));
    let epoch = app.mux.api(|r| Api::Pane(pane, r)).await.flatten().map(|h| h.epoch);
    Ok((pane, epoch, fifo, tokio::fs::File::from_std(out)))
}

/// Apply the deploy `item` carries (checked, and its ledger entry made):
/// its pane, its start taken by control, its secrets, its driver with its
/// output redacted into the pane, and its end reported.
pub async fn apply(app: Arc<App>, mut l: Ledger, item: WorkItem, as_: Option<RunAs>) {
    let dir = app.control.state_dir().to_owned();
    let Some(d) = item.deploy.clone() else { return };
    let run_id = l.run.clone();
    let finish = |end: Result<i32, String>, result: Option<serde_json::Value>| {
        let (app, run) = (app.clone(), run_id.clone());
        async move {
            RUNNING.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            let mut end = end;
            if let Some(why) = STOPPING.lock().unwrap_or_else(|e| e.into_inner()).remove(&run) {
                end = Err(why);
            }
            let Some((keys, me, ..)) = me(&app) else { return };
            let dir = app.control.state_dir().to_owned();
            // What the driver said it did (M220), with its exit.
            let exited = |code: i32| {
                let mut v = serde_json::json!({ "exit": code });
                if let Some(r) = &result {
                    v["result"] = r.clone();
                }
                v
            };
            let ended = update(&dir, &run, |now| match &end {
                Ok(0) => now.push(&keys, &me, State::Succeeded, exited(0)),
                Ok(code) => now.push(&keys, &me, State::Failed, exited(*code)),
                Err(why) => {
                    // Already failed (refused by control): nothing more.
                    if now.state == State::Running.as_str() {
                        now.push(&keys, &me, State::Failed, serde_json::json!({ "reason": why }));
                    }
                }
            });
            match ended {
                Ok(Some(now)) => info!(run = now.run, state = now.state, "hosted: a deploy's run ended"),
                Ok(None) => warn!(run, "hosted: a deploy run's ledger entry is gone"),
                Err(e) => warn!(run, error = %format!("{e:#}"), "hosted: can't write the ledger"),
            }
            send(&app, &run).await;
            if deploy_box(&app) {
                stop_when_reported(&app, &run).await;
            }
        }
    };
    let Some((keys, me, ..)) = me(&app) else {
        return finish(Err("this machine left control".into()), None).await;
    };
    let (pane, epoch, fifo, mut out) = match pane(&app, &l, &d).await {
        Ok(p) => p,
        Err(why) => return finish(Err(format!("start: {why}")), None).await,
    };
    let o = &d.operation;
    let banner = format!(
        "{}\n{}\n",
        printable(&format!("arugula run {} · operation sha256:{}", l.run, l.op_hash)),
        printable(&format!(
            "deploy {} to {}: {} {} from {}, by the {} driver",
            d.deploy, o.environment_name, o.source_kind, o.source_ref, o.source_where, d.driver
        )),
    );
    let _ = out.write_all(banner.as_bytes()).await;
    // `running`, signed and taken by control before anything runs: that's
    // when the platform starts the deploy, or refuses it.
    l.pane = Some(pane);
    l.epoch = epoch;
    l.push(
        &keys,
        &me,
        State::Running,
        serde_json::json!({ "pane": pane, "epoch": epoch, "op": l.op_hash, "deploy": d.deploy }),
    );
    if let Err(e) = super::write(&dir, &l) {
        warn!(run = l.run, error = %format!("{e:#}"), "hosted: can't write the ledger");
    }
    // Its log goes to the run-log bucket as it runs (M213): the pane's
    // output, which is already redacted. The box is deleted after, so this
    // is the only copy.
    let (done, follow) = tokio::sync::watch::channel(false);
    let follower = tokio::spawn(crate::run_logs::follow(app.clone(), l.run.clone(), pane, epoch, follow));
    let said = |why: &str| format!("arugula: {}\n", printable(why));
    // The short-lived tokens it was given, to revoke when its driver ends.
    let mut revocable: Vec<(String, String)> = Vec::new();
    // What the driver said it did, on its result pipe (M220).
    let mut said_on_fd: Vec<u8> = Vec::new();
    let end = async {
        // Once per deploy on this machine, marked before anything runs.
        if !mark_applied(&dir, &d.deploy, &l.run) {
            return Err(format!("deploy_applied: {} was applied on this machine already", d.deploy));
        }
        started(&app, &l.run).await?;
        let argv = driver_argv_for(&d).map_err(|r| r.1)?;
        let given = secrets(&app, &l.run, &d.secrets).await?;
        revocable = given.iter().filter_map(|s| s.revoke.clone().map(|u| (u, s.value.clone()))).collect();
        let pipe = match ResultPipe::new() {
            Ok(p) => p,
            Err(e) => {
                wipe(given);
                return Err(e);
            }
        };
        let spawned = driver_command(&l.run, &d, &argv, as_.as_ref(), &dir, &pipe).spawn();
        let result_r = pipe.spawned();
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => {
                wipe(given);
                return Err(format!("its driver didn't start: {e}"));
            }
        };
        info!(run = l.run, deploy = d.deploy, driver = d.driver, secrets = given.len(), "hosted: applying a deploy");
        // Its secrets on its standard input, a pipe, then closed.
        if let Some(mut stdin) = child.stdin.take() {
            let mut input = secrets_input(&given);
            let _ = stdin.write_all(&input).await;
            input.iter_mut().for_each(|b| *b = 0);
            drop(stdin);
        }
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
        for stream in [
            child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
            child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let mut r = Redactor::new(&given);
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut stream = stream;
                let mut buf = vec![0u8; 8192];
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let shown = r.feed(&buf[..n]);
                            if !shown.is_empty() && tx.send(shown).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                buf.iter_mut().for_each(|b| *b = 0);
                let rest = r.finish();
                if !rest.is_empty() {
                    let _ = tx.send(rest).await;
                }
            });
        }
        drop(tx);
        wipe(given);
        // Its output to the pane while it runs; once it ends, everything the
        // run user left goes first (M220), so nothing it started writes after
        // the end or holds its output open.
        let ran = async {
            let status = child.wait().await;
            if let Some(a) = &as_ {
                kill_all(a).await;
            }
            status
        };
        let shown = async {
            while let Some(chunk) = rx.recv().await {
                let _ = out.write_all(&chunk).await;
            }
        };
        let (status, ()) = tokio::join!(ran, shown);
        said_on_fd = read_result(result_r);
        let status = status.map_err(|e| format!("its driver: {e}"))?;
        status.code().ok_or_else(|| "stopped".to_owned())
    }
    .await;
    if let Some(a) = &as_ {
        kill_all(a).await;
    }
    // Its tokens are revoked by the box itself, the token as its own
    // bearer (the platform tries too).
    revoke_tokens(std::mem::take(&mut revocable)).await;
    let result = result_of(&said_on_fd);
    let tail_line = match &end {
        Ok(0) => said("the deploy was applied"),
        Ok(code) => said(&format!("the driver exited {code}")),
        Err(why) => said(why),
    };
    let _ = out.write_all(tail_line.as_bytes()).await;
    let _ = out.flush().await;
    drop(out);
    // The pane has its end; the FIFO goes once it's read it.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let _ = std::fs::remove_file(&fifo);
    // The rest of its log and its artifacts, before its end is reported,
    // while the box is still busy with it; then its directory (its HOME)
    // goes as the run user, whatever it left there.
    let _ = done.send(true);
    let _ = follower.await;
    crate::run_logs::finish(&app, &l.run, pane, epoch, as_.as_ref()).await;
    remove_home(as_.as_ref(), &work_dir(&l.run, as_.as_ref(), &dir)).await;
    finish(end, result).await;
}

/// Revoke each short-lived token at its revoke URL, the token as its own
/// bearer (GitHub's `DELETE /installation/token`): a few tries each, then
/// its value is overwritten.
async fn revoke_tokens(tokens: Vec<(String, String)>) {
    if tokens.is_empty() {
        return;
    }
    let http = crate::roots::http().timeout(Duration::from_secs(20)).build().expect("http client");
    for (url, token) in tokens {
        let mut ok = false;
        for attempt in 0..3u64 {
            let r = http
                .delete(&url)
                .bearer_auth(&token)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .header("User-Agent", "arugulad")
                .send()
                .await;
            // Revoked now, or it was already (GitHub says 401 to a dead one).
            if r.is_ok_and(|r| r.status().is_success() || r.status() == reqwest::StatusCode::UNAUTHORIZED) {
                ok = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(500 * (attempt + 1))).await;
        }
        if !ok {
            warn!("hosted: a deploy run's token wasn't revoked; it expires within the hour");
        }
        wipe_bytes(token.into_bytes());
    }
}

/// What a driver said its apply did (M220): the JSON object it wrote on
/// its result pipe, if it's one of at most [`MAX_RESULT`] bytes.
pub fn result_of(said: &[u8]) -> Option<serde_json::Value> {
    if said.len() > MAX_RESULT {
        return None;
    }
    serde_json::from_slice::<serde_json::Value>(said).ok().filter(serde_json::Value::is_object)
}

// A deploy's box is Linux: the driver's result on fd 3, node and bash.
#[cfg(all(test, unix))]
#[path = "work_deploy_chant_fly_tests.rs"]
mod chant_fly_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, value: &str) -> Secret {
        Secret { name: name.into(), value: value.into(), revoke: None }
    }

    /// Each secret's exact bytes (and its trimmed form) are replaced,
    /// however the stream is cut, and nothing else changes; the start of a
    /// secret cut off at the end is redacted, never shown.
    #[test]
    fn a_secret_never_reaches_the_pane_however_its_bytes_arrive() {
        let secrets = [s("TOKEN", "hunter2-xyz"), s("SHORT", "ab"), s("EMPTY", ""), s("LINE", "pasted-key\n")];
        let text = b"before hunter2-xyz middle ab after hunter2 not-whole pasted-key; cut hunter2-x";
        for cut in 1..text.len() {
            let mut r = Redactor::new(&secrets);
            let mut out = Vec::new();
            for chunk in text.chunks(cut) {
                out.extend(r.feed(chunk));
            }
            out.extend(r.finish());
            assert_eq!(
                String::from_utf8(out).unwrap(),
                "before [redacted TOKEN] middle [redacted SHORT] after hunter2 not-whole [redacted LINE]; cut [redacted TOKEN]",
                "cut every {cut}"
            );
        }
    }

    /// The driver's shell reads its secrets from its standard input into
    /// its own environment, every byte (trailing newlines too): not a file,
    /// not its command line, no subshell.
    // A deploy's box is Linux: its shell, bash.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_drivers_secrets_come_on_its_standard_input() {
        let dir = crate::hosted::scratch("deploy-driver");
        let work = dir.join("w");
        let argv =
            vec!["/bin/sh".to_owned(), "-c".to_owned(), "printf '%s|' \"$TOKEN\" \"$OTHER\" \"$(pwd)\"".to_owned()];
        let script = driver_script(&work, &argv);
        let secret = "multi\nline 'quoted' $value \\n é\n\n";
        assert!(!script.contains("multi"), "{script}");
        let out = drive(&script, "TOKEN OTHER", &secrets_input(&[s("TOKEN", secret), s("OTHER", "two")])).await;
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{secret}|two|{}|", work.display()));
        let on_disk: Vec<String> = walk(&dir).iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect();
        assert!(on_disk.iter().all(|f| !f.contains("multi")), "nothing of it on disk");
    }

    /// The driver's shell, given `input` on its standard input, with
    /// `DEPLOY_SECRETS` and a `DEPLOY_SOURCE_REF` of its own.
    #[cfg(unix)]
    async fn drive(script: &str, names: &str, input: &[u8]) -> std::process::Output {
        let mut c = tokio::process::Command::new("/bin/bash");
        c.env_clear()
            .env("PATH", RUN_PATH)
            .env("LANG", "C.UTF-8")
            .env("DEPLOY_SECRETS", names)
            .env("DEPLOY_SOURCE_REF", "the-approved-ref")
            .args(["--noprofile", "--norc", "-c", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input).await.unwrap();
        drop(stdin);
        child.wait_with_output().await.unwrap()
    }

    /// A NUL in a value (which `read -N` drops) can't throw the framing
    /// out and pass the rest of the value off as more headers: the length
    /// doesn't match, and nothing runs. Nor does a name the deploy doesn't
    /// name.
    // A deploy's box is Linux: its shell, bash.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_nul_or_an_unnamed_secret_runs_nothing() {
        let dir = crate::hosted::scratch("deploy-nul");
        let argv = vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "printf 'RAN %s %s' \"$DEPLOY_SOURCE_REF\" \"$PATH\"".to_owned(),
        ];
        let script = driver_script(&dir.join("w"), &argv);
        // A value that, NUL dropped, would read on as `DEPLOY_SOURCE_REF 4`
        // and `PATH 4` headers.
        let evil = "x\0\nDEPLOY_SOURCE_REF 4\nevilPATH 4\n/tmp";
        let out = drive(&script, "APP_TOKEN", &secrets_input(&[s("APP_TOKEN", evil)])).await;
        assert_eq!(out.status.code(), Some(97), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(!String::from_utf8_lossy(&out.stdout).contains("RAN"), "the driver didn't run");
        let out = drive(&script, "APP_TOKEN", &secrets_input(&[s("PATH", "/tmp/evil")])).await;
        assert_eq!(out.status.code(), Some(97), "an unnamed secret");
        let out = drive(&script, "APP_TOKEN", &secrets_input(&[s("APP_TOKEN", "fine")])).await;
        assert!(String::from_utf8_lossy(&out.stdout).starts_with("RAN the-approved-ref "), "{out:?}");
        // And control's answer with a NUL in a value is refused before any of it.
        let names = vec!["APP_TOKEN".to_owned()];
        let given = serde_json::json!({ "kind": "given", "secrets": [{ "name": "APP_TOKEN", "value": evil }] });
        let why = read_given(given.to_string().as_bytes(), &names).unwrap_err();
        assert!(why.contains("NUL") && !why.contains("evil"), "{why}");
    }

    /// A deploy's box gives up root down to the run user and the reseed:
    /// what `sudo -ln` lists is read, and anything else refused. One with no
    /// run user is refused, but in tests.
    #[test]
    fn a_deploy_box_keeps_only_the_run_user_and_the_reseed() {
        let ok = "Matching Defaults entries for user on box:\n    env_reset\n\nUser user may run the following commands on box:\n    (runner) NOPASSWD: ALL\n    (root) NOPASSWD: /usr/local/bin/arugulad _reseed *\n";
        assert_eq!(only_runner_and_reseed(ok, "runner"), Ok(()));
        let blanket = format!("{ok}    (ALL : ALL) NOPASSWD: ALL\n");
        assert_eq!(only_runner_and_reseed(&blanket, "runner"), Err("(ALL : ALL) NOPASSWD: ALL".into()));
        let other = ok.replace("(runner)", "(root)");
        assert!(only_runner_and_reseed(&other, "runner").is_err(), "root, not the run user");
        assert!(only_runner_and_reseed("nothing", "runner").is_err());
        let as_ = RunAs { user: "runner".into(), home: "/home/runner".into() };
        assert!(deploy_box_user(Some(&as_), false).is_ok());
        assert_eq!(deploy_box_user(None, false).unwrap_err().0, "run_user");
        assert!(deploy_box_user(None, true).is_ok(), "tests only");
    }

    /// What control gives is read by hand: only the names the deploy
    /// carries, each once, never reserved, and what's wrong is said
    /// without a value.
    #[test]
    fn only_the_secrets_a_deploy_names_are_taken() {
        let names = vec!["APP_TOKEN".to_owned()];
        let given = |v: serde_json::Value| read_given(v.to_string().as_bytes(), &names);
        let ok = given(serde_json::json!({ "kind": "given", "secrets": [{ "name": "APP_TOKEN", "value": "v1\n" }] }));
        assert_eq!(
            ok.unwrap().iter().map(|s| (s.name.as_str(), s.value.as_str())).collect::<Vec<_>>(),
            [("APP_TOKEN", "v1\n")]
        );
        for bad in [
            serde_json::json!({ "kind": "given", "secrets": [{ "name": "PATH", "value": "s3cr3t" }] }),
            serde_json::json!({ "kind": "given", "secrets": [{ "name": "OTHER", "value": "s3cr3t" }] }),
            serde_json::json!({ "kind": "given", "secrets": [{ "name": "APP_TOKEN", "value": "s3cr3t" }, { "name": "APP_TOKEN", "value": "s3cr3t" }] }),
            serde_json::json!({ "kind": "given", "secrets": "s3cr3t" }),
            serde_json::json!({ "kind": "else", "s3cr3t": 1 }),
        ] {
            let why = given(bad.clone()).unwrap_err();
            assert!(!why.contains("s3cr3t"), "{why}");
        }
        assert!(
            read_given(b"{\"kind\":\"given\",\"secrets\":[{\"name\":\"APP_TOKEN\",\"value\":\"s3cr3t", &names)
                .unwrap_err()
                .contains("doesn't read")
        );
        assert_eq!(given(serde_json::json!({ "kind": "refused", "why": "once" })).unwrap_err(), "its secrets: once");
    }

    /// A deploy applies on a machine once: the mark is made once, and a
    /// mark that's there (or a deploy id that isn't one) counts as applied.
    #[test]
    fn a_deploy_is_applied_here_once() {
        let dir = crate::hosted::scratch("deploy-applied");
        let d = format!("dep_{}", "a".repeat(32));
        assert!(!applied(&dir, &d));
        assert!(mark_applied(&dir, &d, "run_1"));
        assert!(!mark_applied(&dir, &d, "run_2"), "once");
        assert!(applied(&dir, &d));
        assert!(applied(&dir, "dep_../../x"), "not an id: taken");
        assert!(!mark_applied(&dir, "dep_../../x", "run_3"));
    }

    #[cfg(unix)]
    fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
        let mut out = vec![];
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { out.extend(walk(&p)) } else { out.push(p) }
        }
        out
    }

    #[test]
    fn only_drivers_this_box_knows_run() {
        assert_eq!(driver_argv("chant-fly").unwrap()[..3], ["node", "--input-type=module", "-e"]);
        let ap = driver_argv("apps-platform").unwrap();
        assert_eq!(ap[1], "_apps-platform", "the daemon itself, as the apps-platform driver: {ap:?}");
        let ar = driver_argv("arugula-release").unwrap();
        assert_eq!(ar[1], "_arugula-release", "the daemon itself, as Arugula's release: {ar:?}");
        assert!(good_name("FLY_API_TOKEN") && !good_name("1X") && !good_name("a") && !good_name("A B"));
    }

    #[test]
    fn a_drivers_result_is_one_small_object() {
        assert_eq!(result_of(br#"{"a":2}"#).unwrap()["a"], 2);
        assert!(result_of(b"[1]").is_none(), "an object only");
        assert!(result_of(format!("{{\"a\":\"{}\"}}", "x".repeat(3000)).as_bytes()).is_none());
        assert!(result_of(b"").is_none());
    }
}
