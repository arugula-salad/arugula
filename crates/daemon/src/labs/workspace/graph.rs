//! A workspace block's graph (#620): behold's estate graph for its
//! members, drawn in the block.
//!
//! The block runs `behold serve <root> --port 0` on this host the first
//! time someone asks for the graph, reads the port behold says it bound,
//! and frames it through a site of its own (`sites.rs`), as an editor block
//! frames code-server. behold runs in a process group of its own until the
//! block closes or the daemon stops; either stops the whole group (behold,
//! a wrapper such as `npx`, and the chant processes behold started):
//! `SIGTERM`, then `SIGKILL` after [`STOP_GRACE`].
//!
//! **No env on the command line.** behold started with `--env` reads the
//! env live (`chant graph --live`) at startup and on every source change.
//! The block's env reaches behold through the frame instead: `gates=<env>`
//! for the gates behold draws, and `env=<env>` only when the person asks
//! for a live read there, since that needs the host's credentials. Never
//! `--local`, which starts emulators.
//!
//! **Its environment** is the one a member's Shell pane gets on this host:
//! the user's login shell's over the daemon's ([`Runner::user`]), without
//! the service manager's variables or a VS Code terminal's (as code-server
//! starts, `editor/server.rs`). Whatever credentials that shell has, behold
//! has; Arugula adds none.
//!
//! **Who reaches it.** behold listens on loopback. The block's site proxies
//! to it with `Host: localhost:<port>` and, for a request that has one,
//! `Origin: http://localhost:<port>` (`sites.rs` `rewrite_request`), so
//! behold sees its own loopback name and no `--allow-host` is needed.
//! Starting it is the owner's (`authz.rs`).
//!
//! **Which behold:** `$ARUGULA_BEHOLD` in the daemon's environment (a
//! command, split on spaces: `node /src/behold/bin/behold.js`), else
//! `node_modules/.bin/behold` in the root, else `behold` on `PATH`.
//!
//! **Freshness:** behold doesn't poll the repo beside the block. When a
//! read of the block's settles on a new fingerprint, it tells behold with
//! `POST /api/refresh?notify=1` ([`Graph::notify`]).
//!
//! Not on a VM yet: the block says so.

use std::{
    collections::HashSet,
    net::{Ipv4Addr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info, warn};

use crate::{
    block::BlockCtx,
    ports::Target,
    review::Runner,
    sites::{self, Site},
};

/// How long behold may take to answer once started (it reads the
/// workspace through chant first).
const START: Duration = Duration::from_secs(90);
/// How long a stopped behold has after `SIGTERM`, before `SIGKILL`.
const STOP_GRACE: Duration = Duration::from_secs(2);

/// `sh -c SERVE sh ROOT BEHOLD`: behold, as the module says, on a port it
/// picks. `$BEHOLD` is split on spaces on purpose.
const SERVE: &str = r#"cd "$1" || exit 1
if [ -n "$2" ]; then exec $2 serve "$1" --port 0; fi
if [ -x node_modules/.bin/behold ]; then exec node_modules/.bin/behold serve "$1" --port 0; fi
if command -v behold >/dev/null 2>&1; then exec behold serve "$1" --port 0; fi
echo "no behold here: not in node_modules/.bin and not on PATH (npm install -g @intentius/behold, or set ARUGULA_BEHOLD for the daemon)" >&2
exit 127"#;

/// The process groups of every behold this daemon runs, for [`stop_all`].
static GROUPS: Mutex<Option<HashSet<i32>>> = Mutex::new(None);

fn group(pid: i32, on: bool) {
    let mut all = GROUPS.lock().unwrap();
    let all = all.get_or_insert_with(HashSet::new);
    if on {
        all.insert(pid);
    } else {
        all.remove(&pid);
    }
}

/// Send `sig` to process group `pgid`; whether it's still there.
#[cfg(unix)]
fn signal(pgid: i32, sig: nix::sys::signal::Signal) -> bool {
    nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pgid), sig).is_ok()
}

/// Whether process group `pgid` has a process left.
#[cfg(unix)]
fn alive(pgid: i32) -> bool {
    nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pgid), None).is_ok()
}

/// The daemon is stopping: every behold's group gets `SIGTERM`, and
/// `SIGKILL` if anything is left after [`STOP_GRACE`]. Blocks.
pub fn stop_all() {
    let groups: Vec<i32> = GROUPS.lock().unwrap().take().map(|g| g.into_iter().collect()).unwrap_or_default();
    #[cfg(unix)]
    {
        use nix::sys::signal::Signal;
        let groups: Vec<i32> = groups.into_iter().filter(|g| signal(*g, Signal::SIGTERM)).collect();
        let t0 = Instant::now();
        while groups.iter().any(|g| alive(*g)) && t0.elapsed() < STOP_GRACE {
            std::thread::sleep(Duration::from_millis(50));
        }
        for g in groups {
            signal(g, Signal::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = groups;
}

/// What the block's state says of its graph.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "is")]
pub(super) enum Status {
    #[default]
    Off,
    Starting,
    /// `src` is the site's root; the page adds the view to it. `run`
    /// counts starts, so a frame loads again after a restart.
    Running {
        src: String,
        run: u64,
    },
    Failed {
        error: String,
    },
}

struct Running {
    port: u16,
    stop: tokio::sync::oneshot::Sender<()>,
}

#[derive(Default)]
pub(super) struct Graph {
    status: Mutex<Status>,
    running: Mutex<Option<Running>>,
    site: Mutex<Option<Arc<Site>>>,
    key: Mutex<Option<String>>,
    starting: tokio::sync::Mutex<()>,
    runs: Mutex<u64>,
    /// The fingerprint behold last heard about.
    told: Mutex<Option<String>>,
    /// The block closed: a start still under way stops what it started.
    closed: AtomicBool,
}

impl Graph {
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    fn set(&self, ctx: &BlockCtx, s: Status) {
        *self.status.lock().unwrap() = s;
        ctx.changed();
    }

    /// behold on `root`, started if it isn't running: the site's root. A
    /// second caller while it starts waits for that start. `changed` says
    /// the block's state changed (behold stopped by itself).
    pub async fn ensure(
        self: &Arc<Self>,
        ctx: &BlockCtx,
        runner: &Runner,
        root: &str,
        changed: impl Fn() + Send + 'static,
    ) -> Result<String, String> {
        let _one = self.starting.lock().await;
        if self.closed.load(Ordering::SeqCst) {
            return Err("the block closed".into());
        }
        if let Status::Running { src, .. } = self.status()
            && self.running.lock().unwrap().is_some()
        {
            return Ok(src);
        }
        self.set(ctx, Status::Starting);
        match self.start(ctx, runner, root, changed).await {
            Ok(src) => Ok(src),
            Err(e) => {
                warn!(block = ctx.id, error = %e, "behold didn't start");
                self.set(ctx, Status::Failed { error: e.clone() });
                Err(e)
            }
        }
    }

    async fn start(
        self: &Arc<Self>,
        ctx: &BlockCtx,
        runner: &Runner,
        root: &str,
        changed: impl Fn() + Send + 'static,
    ) -> Result<String, String> {
        let Runner::Local { env: vars, home } = runner else {
            return Err("the graph runs on this host only for now: this workspace is on a VM".into());
        };
        let sites = sites::get().ok_or("the graph needs block sites: start arugulad with --block-listen")?;
        let log_path = ctx.dir.join("behold.log");
        let log = std::fs::File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
        let behold = std::env::var("ARUGULA_BEHOLD").unwrap_or_default();
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c")
            .arg(SERVE)
            .args(["sh", root, behold.trim()])
            .envs(vars.iter().map(|(k, v)| (k, v)))
            .current_dir(home)
            .stdin(std::process::Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log)
            .kill_on_drop(true);
        let daemon: Vec<String> = std::env::vars_os().map(|(k, _)| k.to_string_lossy().into_owned()).collect();
        for k in clean_env(vars.iter().map(|(k, _)| k.as_str()).chain(daemon.iter().map(String::as_str))) {
            cmd.env_remove(k);
        }
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd.spawn().map_err(|e| format!("can't run sh: {e}"))?;
        let pid = child.id().unwrap_or(0) as i32;
        group(pid, true);
        info!(block = ctx.id, pid, root, "behold starting");
        let t0 = Instant::now();
        let port = loop {
            if self.closed.load(Ordering::SeqCst) {
                stop_group(pid, &mut child).await;
                return Err("the block closed".into());
            }
            if let Some(port) = bound_port(&std::fs::read_to_string(&log_path).unwrap_or_default())
                && tokio::net::TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await.is_ok()
            {
                break port;
            }
            if let Ok(Some(st)) = child.try_wait() {
                group(pid, false);
                return Err(format!("behold exited ({st}): {}", tail(&log_path)));
            }
            if t0.elapsed() > START {
                stop_group(pid, &mut child).await;
                return Err(format!("behold didn't answer in {}s: {}", START.as_secs(), tail(&log_path)));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        if let Err(e) = sites.allowed(&Target::Local(port)) {
            stop_group(pid, &mut child).await;
            return Err(e);
        }
        info!(block = ctx.id, pid, port, ms = t0.elapsed().as_millis() as u64, "behold is up");
        let site = {
            let mut site = self.site.lock().unwrap();
            let key = self.key.lock().unwrap().get_or_insert_with(crate::browser::new_key).clone();
            site.get_or_insert_with(|| sites.open(ctx.id, &key, |_| {})).clone()
        };
        site.set_target(Target::Local(port));
        let (stop, stopped) = tokio::sync::oneshot::channel();
        *self.running.lock().unwrap() = Some(Running { port, stop });
        *self.told.lock().unwrap() = None;
        // Closed while it started, after the last look: `close` found
        // nothing to stop, and may have closed the site before it opened.
        if self.closed.load(Ordering::SeqCst) {
            self.running.lock().unwrap().take();
            self.close_site(ctx);
            stop_group(pid, &mut child).await;
            return Err("the block closed".into());
        }
        let run = {
            let mut runs = self.runs.lock().unwrap();
            *runs += 1;
            *runs
        };
        let src = site.url("/");
        self.set(ctx, Status::Running { src: src.clone(), run });
        // It goes when told to, or by itself (then the next Graph starts it).
        let me = Arc::downgrade(self);
        ctx.rt.spawn(async move {
            tokio::select! {
                st = child.wait() => {
                    info!(pid, status = ?st, "behold stopped");
                    // What it started may still be there.
                    stop_group(pid, &mut child).await;
                    let why = format!(
                        "behold stopped ({}): {}",
                        st.map(|s| s.to_string()).unwrap_or_else(|e| e.to_string()),
                        tail(&log_path)
                    );
                    if let Some(g) = me.upgrade() && g.exited(run, why) {
                        changed();
                    }
                }
                _ = stopped => {
                    stop_group(pid, &mut child).await;
                    info!(pid, "behold stopped");
                }
            }
        });
        Ok(src)
    }

    /// behold's run `run` stopped by itself: whether that's news (it's the
    /// one running, not one stopped on purpose).
    fn exited(&self, run: u64, why: String) -> bool {
        let mut status = self.status.lock().unwrap();
        if !matches!(&*status, Status::Running { run: r, .. } if *r == run) {
            return false;
        }
        self.running.lock().unwrap().take();
        *status = Status::Failed { error: why };
        true
    }

    /// Stop behold, if it runs.
    pub fn stop(&self, ctx: &BlockCtx) {
        // Not under `running`'s lock: `exited` takes `status`, then it.
        let running = self.running.lock().unwrap().take();
        if let Some(r) = running {
            let _ = r.stop.send(());
            self.set(ctx, Status::Off);
        }
    }

    /// The block's read settled on `print`: tell behold, if it runs and
    /// hasn't heard of this one.
    pub fn notify(&self, ctx: &BlockCtx, print: Option<&str>) {
        let Some(port) = self.running.lock().unwrap().as_ref().map(|r| r.port) else { return };
        let print = print.map(str::to_owned);
        {
            let mut told = self.told.lock().unwrap();
            if !should_tell(told.as_deref(), print.as_deref()) {
                return;
            }
            told.clone_from(&print);
        }
        ctx.rt.spawn(async move {
            if let Err(e) = refresh(port).await {
                warn!(port, error = %e, "telling behold the workspace changed");
            }
        });
    }

    fn close_site(&self, ctx: &BlockCtx) {
        if self.site.lock().unwrap().take().is_some()
            && let Some(sites) = sites::get()
        {
            sites.close(ctx.id);
        }
    }

    /// Close the site and stop behold (the block closed).
    pub fn close(&self, ctx: &BlockCtx) {
        self.closed.store(true, Ordering::SeqCst);
        self.stop(ctx);
        self.close_site(ctx);
    }
}

/// Stop behold's process group: `SIGTERM`, then `SIGKILL` if anything is
/// left after [`STOP_GRACE`]; and reap `child`.
async fn stop_group(pid: i32, child: &mut tokio::process::Child) {
    #[cfg(unix)]
    {
        use nix::sys::signal::Signal;
        if pid > 0 && signal(pid, Signal::SIGTERM) {
            let t0 = Instant::now();
            while alive(pid) && t0.elapsed() < STOP_GRACE {
                let _ = tokio::time::timeout(Duration::from_millis(50), child.wait()).await;
            }
            signal(pid, Signal::SIGKILL);
        }
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
    group(pid, false);
}

/// The variables behold doesn't get, of `names`: the service manager's,
/// and a VS Code terminal's, as code-server (`editor/server.rs`), and a
/// pane's own.
fn clean_env<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = crate::sys::SERVICE_ENV.iter().map(|k| (*k).to_owned()).collect();
    out.extend(["ARUGULA_PANE", "ILLOGICAL_PANE"].map(str::to_owned));
    out.extend(names.filter(|k| k.starts_with("VSCODE_")).map(str::to_owned));
    out.sort();
    out.dedup();
    out
}

/// The port behold says it listens on, from its log: the first
/// `http://<host>:<port>` it prints (`behold → http://localhost:4600`).
fn bound_port(log: &str) -> Option<u16> {
    log.lines().find_map(|l| {
        let at = l.find("http://")?;
        let rest = &l[at + "http://".len()..];
        let authority = rest.split(['/', ' ']).next()?;
        authority.rsplit_once(':')?.1.parse().ok().filter(|p| *p > 0)
    })
}

/// Whether a settled read on `now` is news to behold, which last heard of
/// `told` (nothing yet: the read it started on).
fn should_tell(told: Option<&str>, now: Option<&str>) -> bool {
    match (told, now) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(t), Some(n)) => t != n,
    }
}

/// `POST /api/refresh?notify=1` to behold on `port`: behold drops what it
/// cached and its pages read again.
pub(super) async fn refresh(port: u16) -> Result<(), String> {
    let talk = async {
        let mut s = tokio::net::TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await?;
        let req = format!(
            "POST /api/refresh?notify=1 HTTP/1.1\r\nHost: localhost:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        s.write_all(req.as_bytes()).await?;
        let mut head = [0u8; 16];
        let n = s.read(&mut head).await?;
        Ok::<_, std::io::Error>(String::from_utf8_lossy(&head[..n]).into_owned())
    };
    let head = tokio::time::timeout(Duration::from_secs(10), talk)
        .await
        .map_err(|_| "no answer".to_owned())?
        .map_err(|e| e.to_string())?;
    match head.split_whitespace().nth(1) {
        Some("200") => Ok(()),
        _ => Err(format!("answered {:?}", head.lines().next().unwrap_or_default())),
    }
}

/// The last lines of behold's log, for an error.
fn tail(path: &std::path::Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(3)..].join(" / ")
}

#[cfg(test)]
mod tests {
    use super::{bound_port, clean_env, refresh, should_tell};

    #[test]
    fn the_port_is_the_one_behold_says() {
        let log = "behold: serving chant workspace toy (chant.workspace.json), 1 of 2 members drawn\n\
                   behold \u{2192} http://localhost:53536\n  project: /w/delivery\n";
        assert_eq!(bound_port(log), Some(53536));
        assert_eq!(bound_port("behold \u{2192} http://127.0.0.1:4600/\n"), Some(4600));
        // Nothing yet, or a port of 0.
        assert_eq!(bound_port("behold: serving chant workspace toy\n"), None);
        assert_eq!(bound_port("http://localhost:0\n"), None);
    }

    #[test]
    fn behold_gets_no_service_or_vscode_variables() {
        let gone = clean_env(["PATH", "VSCODE_IPC_HOOK_CLI", "AWS_PROFILE"].into_iter());
        assert!(gone.contains(&"VSCODE_IPC_HOOK_CLI".to_owned()));
        assert!(gone.contains(&"ARUGULA_PANE".to_owned()));
        for k in crate::sys::SERVICE_ENV {
            assert!(gone.contains(&(*k).to_owned()), "{k}");
        }
        // The shell's own, credentials included, stay.
        assert!(!gone.contains(&"AWS_PROFILE".to_owned()) && !gone.contains(&"PATH".to_owned()));
    }

    #[test]
    fn behold_hears_of_each_new_fingerprint_once() {
        assert!(should_tell(None, Some("a 1")));
        assert!(!should_tell(Some("a 1"), Some("a 1")));
        assert!(should_tell(Some("a 1"), Some("a 2")));
        // No fingerprint: nothing to say.
        assert!(!should_tell(Some("a 1"), None));
    }

    #[tokio::test]
    async fn the_notice_is_a_post_to_refresh_with_notify() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let got = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let n = s.read(&mut buf).await.unwrap();
            s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 17\r\n\r\n{\"notified\":true}").await.unwrap();
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        refresh(port).await.unwrap();
        let req = got.await.unwrap();
        assert!(req.starts_with("POST /api/refresh?notify=1 HTTP/1.1\r\n"), "{req}");
    }

    #[tokio::test]
    async fn a_refusal_is_an_error() {
        use tokio::io::AsyncWriteExt;
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            s.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n").await.unwrap();
        });
        assert!(refresh(port).await.is_err());
    }
}
