//! A workspace block's graph (#620): behold's estate graph for its
//! members, drawn in the block.
//!
//! The block runs `behold serve <root> --port <p> --env <env>` on this host
//! the first time someone asks for the graph, and frames it through a site
//! of its own (`sites.rs`), as an editor block frames code-server. behold
//! runs until the block closes, or until the block's env changes (then the
//! next *Graph* starts it again on the new env). It stays in the daemon's
//! process group, so it goes when the daemon does.
//!
//! **Its environment** is the one a member's Shell pane gets on this host:
//! the user's login shell's over the daemon's ([`Runner::user`]). Whatever
//! credentials that shell has, behold has; Arugula adds none. `--env` is
//! the block's env, for the gates behold draws (`status <env>`). The frame
//! opens on the source graph; a live read of an env is the person's choice
//! in the frame, since it needs the host's credentials. Never `--local`,
//! which starts emulators.
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
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
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

/// `sh -c SERVE sh ROOT PORT ENV BEHOLD`: behold, as the module says.
/// `$BEHOLD` is split on spaces on purpose.
const SERVE: &str = r#"cd "$1" || exit 1
if [ -n "$4" ]; then exec $4 serve "$1" --port "$2" --env "$3"; fi
if [ -x node_modules/.bin/behold ]; then exec node_modules/.bin/behold serve "$1" --port "$2" --env "$3"; fi
if command -v behold >/dev/null 2>&1; then exec behold serve "$1" --port "$2" --env "$3"; fi
echo "no behold here: not in node_modules/.bin and not on PATH (npm install -g @intentius/behold, or set ARUGULA_BEHOLD for the daemon)" >&2
exit 127"#;

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
        env: String,
        run: u64,
    },
    Failed {
        error: String,
    },
}

struct Running {
    port: u16,
    env: String,
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
    closed: std::sync::atomic::AtomicBool,
}

impl Graph {
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    fn set(&self, ctx: &BlockCtx, s: Status) {
        *self.status.lock().unwrap() = s;
        ctx.changed();
    }

    /// behold on `root`, started if it isn't running on `env`: the site's
    /// root. `changed` says the block's state changed (behold stopped by itself).
    pub async fn ensure(
        self: &Arc<Self>,
        ctx: &BlockCtx,
        runner: &Runner,
        root: &str,
        env: &str,
        changed: impl Fn() + Send + 'static,
    ) -> Result<String, String> {
        let _one = self.starting.lock().await;
        if let Status::Running { src, env: on, .. } = self.status()
            && on == env
            && self.running.lock().unwrap().is_some()
        {
            return Ok(src);
        }
        self.stop(ctx);
        self.set(ctx, Status::Starting);
        match self.start(ctx, runner, root, env, changed).await {
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
        env: &str,
        changed: impl Fn() + Send + 'static,
    ) -> Result<String, String> {
        let Runner::Local { env: vars, home } = runner else {
            return Err("the graph runs on this host only for now: this workspace is on a VM".into());
        };
        let sites = sites::get().ok_or("the graph needs block sites: start arugulad with --block-listen")?;
        let port = free_port()?;
        sites.allowed(&Target::Local(port))?;
        let log_path = ctx.dir.join("behold.log");
        let log = std::fs::File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
        let behold = std::env::var("ARUGULA_BEHOLD").unwrap_or_default();
        let mut child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(SERVE)
            .args(["sh", root, &port.to_string(), env, behold.trim()])
            .envs(vars.iter().map(|(k, v)| (k, v)))
            // Its own pages, not a pane's.
            .env_remove("ARUGULA_PANE")
            .env_remove("ILLOGICAL_PANE")
            .current_dir(home)
            .stdin(std::process::Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("can't run sh: {e}"))?;
        let pid = child.id().unwrap_or(0);
        info!(block = ctx.id, pid, port, root, env, "behold starting");
        let t0 = Instant::now();
        loop {
            if tokio::net::TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await.is_ok() {
                break;
            }
            if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            if let Ok(Some(st)) = child.try_wait() {
                return Err(format!("behold exited ({st}): {}", tail(&log_path)));
            }
            if t0.elapsed() > START {
                let _ = child.start_kill();
                return Err(format!("behold didn't answer in {}s: {}", START.as_secs(), tail(&log_path)));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        info!(block = ctx.id, pid, ms = t0.elapsed().as_millis() as u64, "behold is up");
        let site = {
            let mut site = self.site.lock().unwrap();
            let key = self.key.lock().unwrap().get_or_insert_with(crate::browser::new_key).clone();
            site.get_or_insert_with(|| sites.open(ctx.id, &key, |_| {})).clone()
        };
        site.set_target(Target::Local(port));
        let (stop, stopped) = tokio::sync::oneshot::channel();
        *self.running.lock().unwrap() = Some(Running { port, env: env.to_owned(), stop });
        *self.told.lock().unwrap() = None;
        // Closed while it started: `close` found nothing to stop.
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            self.running.lock().unwrap().take();
            let _ = child.start_kill();
            return Err("the block closed".into());
        }
        let run = {
            let mut runs = self.runs.lock().unwrap();
            *runs += 1;
            *runs
        };
        let src = site.url("/");
        self.set(ctx, Status::Running { src: src.clone(), env: env.to_owned(), run });
        // It goes when told to, or by itself (then the next Graph starts it).
        let me = Arc::downgrade(self);
        ctx.rt.spawn(async move {
            tokio::select! {
                st = child.wait() => {
                    info!(pid, status = ?st, "behold stopped");
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
                    let _ = child.start_kill();
                    let _ = child.wait().await;
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

    /// The env behold was started on, while it runs.
    pub fn env(&self) -> Option<String> {
        self.running.lock().unwrap().as_ref().map(|r| r.env.clone())
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

    /// Close the site and stop behold (the block closed).
    pub fn close(&self, ctx: &BlockCtx) {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        self.stop(ctx);
        if self.site.lock().unwrap().take().is_some()
            && let Some(sites) = sites::get()
        {
            sites.close(ctx.id);
        }
    }
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

/// A port nothing listens on now, on loopback.
fn free_port() -> Result<u16, String> {
    let l = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| format!("no free port: {e}"))?;
    l.local_addr().map(|a| a.port()).map_err(|e| e.to_string())
}

/// The last lines of behold's log, for an error.
fn tail(path: &std::path::Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(3)..].join(" / ")
}

#[cfg(test)]
mod tests {
    use super::{refresh, should_tell};

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
